//! A plugin's hot code (the SDK's `hot!`): where it lies, and the patches
//! that let it run elsewhere than the rest of the image (format 3, see
//! `paperpaw_native::PatchKind`).
//!
//! `paperpaw-plugin.x` puts the hot code at the end of `.text`, between
//! `PAPERPAW_HOT` and `PAPERPAW_HOT_END`. A device may run it from
//! faster memory, so every reference across its edge by a distance
//! changes. On RISC-V those are `auipc` with its partner (a call, or a
//! load or store of an address), which the loader patches; any other
//! reach across the edge (a jump of short reach, a difference of two
//! places) fails the pack. Plugins link with `--no-relax`, so calls stay
//! `auipc` and `jalr`.

use std::collections::{BTreeMap, BTreeSet};

use paperpaw_native::{CODE_OFFSET, Chip, LINK_BASE, PatchKind};

use crate::elf::{Elf, SHT_RELA, STT_SECTION, Symbol, le32};
use crate::pack::{Image, Place};
use crate::relocation::{self, Kind};

/// Where the linker script begins and ends the hot code.
const HOT_SYMBOL: &str = "PAPERPAW_HOT";
const HOT_END_SYMBOL: &str = "PAPERPAW_HOT_END";

/// The most problems told at once.
const MAX_TOLD: usize = 20;

// RISC-V's relocation kinds the patches come from (binutils' numbers).
const CALL: u32 = 18;
const CALL_PLT: u32 = 19;
const PCREL_HI20: u32 = 23;
const PCREL_LO12_I: u32 = 24;
const PCREL_LO12_S: u32 = 25;

/// The hot code of a plugin and its patches.
#[derive(Debug, Default)]
pub struct Hot {
    /// Where it begins in the image (0 if there is none).
    pub at: u32,
    pub len: u32,
    /// The patches, in order: the first word of each, then its reference.
    pub patches: Vec<[u32; 2]>,
}

/// A relocation of the image that reaches into it.
struct Reach<'a> {
    /// Its kind, as the ELF file numbers it.
    kind: u32,
    /// Where it applies: an image offset.
    site: u32,
    /// What it reaches: an image offset.
    target: u32,
    symbol: Option<&'a Symbol>,
}

impl Hot {
    /// The hot code of the linked plugin `elf` and its patches; `words`
    /// the words to relocate. Xtensa's hot code stays in place: the X4 Pro
    /// runs all code from PSRAM, and `hot!` marks nothing there.
    pub fn of(
        elf: &Elf<'_>,
        image: &Image<'_>,
        chip: Chip,
        symbols: &[Symbol],
        words: &BTreeSet<u32>,
    ) -> Result<Hot, String> {
        if chip != Chip::Esp32C3 {
            return Ok(Hot::default());
        }
        let start = code_symbol(image, symbols, HOT_SYMBOL)?;
        let end = code_symbol(image, symbols, HOT_END_SYMBOL)?;
        if end < start || !start.is_multiple_of(4) || !end.is_multiple_of(4) {
            return Err(format!(
                "`{HOT_SYMBOL}` and `{HOT_END_SYMBOL}` do not hold the hot code in whole words: link the plugin with paperpaw-plugin.x as it is now"
            ));
        }
        if start == end {
            return Ok(Hot::default());
        }
        let mut hot = Hot {
            at: start,
            len: end - start,
            patches: Vec::new(),
        };
        let mut problems = Vec::new();
        if let Some(word) = words.iter().find(|&&w| hot.contains(w)) {
            problems.push(format!(
                "a word to relocate lies in hot code, at {}: hot code holds no addresses",
                function_at(image, symbols, *word)
            ));
        }
        let reaches = reaches(elf, image, chip, symbols)?;
        hot.patch(image, symbols, &reaches, &mut problems);
        if problems.is_empty() {
            hot.patches.sort_unstable();
            return Ok(hot);
        }
        let mut text = String::from("the plugin cannot be packed:");
        for problem in problems.iter().take(MAX_TOLD) {
            text.push_str("\n    ");
            text.push_str(problem);
        }
        if problems.len() > MAX_TOLD {
            text.push_str(&format!("\n    and {} more", problems.len() - MAX_TOLD));
        }
        Err(text)
    }

    fn contains(&self, at: u32) -> bool {
        at.wrapping_sub(self.at) < self.len
    }

    /// Whether a reach from `site` to `target` crosses the hot code's edge.
    fn crosses(&self, site: u32, target: u32) -> bool {
        self.contains(site) != self.contains(target)
    }

    /// Records the patches for `reaches`, or says why one cannot be.
    fn patch(
        &mut self,
        image: &Image<'_>,
        symbols: &[Symbol],
        reaches: &[Reach<'_>],
        problems: &mut Vec<String>,
    ) {
        // The upper parts by their `auipc`, for the lower parts that
        // name them.
        let upper: BTreeMap<u32, u32> = reaches
            .iter()
            .filter(|r| r.kind == PCREL_HI20)
            .map(|r| (r.site, r.target))
            .collect();
        for reach in reaches {
            let (site, target) = (reach.site, reach.target);
            match reach.kind {
                CALL | CALL_PLT if self.crosses(site, target) => {
                    self.upper(image, site, target, problems);
                    self.lower(image, PatchKind::Lo12I, site + 4, site, target, problems);
                }
                PCREL_HI20 if self.crosses(site, target) => {
                    self.upper(image, site, target, problems);
                }
                PCREL_LO12_I | PCREL_LO12_S => {
                    // Its symbol is the label of its `auipc`.
                    let Some(&reached) = upper.get(&target) else {
                        problems.push(format!(
                            "{} at {} names no `auipc` with a {}",
                            relocation::name(Chip::Esp32C3, reach.kind),
                            function_at(image, symbols, site),
                            relocation::name(Chip::Esp32C3, PCREL_HI20)
                        ));
                        continue;
                    };
                    if self.crosses(target, reached) {
                        let kind = match reach.kind {
                            PCREL_LO12_I => PatchKind::Lo12I,
                            _ => PatchKind::Lo12S,
                        };
                        self.lower(image, kind, site, target, reached, problems);
                    }
                }
                CALL | CALL_PLT | PCREL_HI20 => {}
                kind if self.crosses(site, target) => problems.push(format!(
                    "`{}` reaches `{}` across the edge of hot code ({} at {}), which cannot be patched: mark both functions hot or neither",
                    function_name(image, symbols, site),
                    target_name(image, symbols, reach),
                    relocation::name(Chip::Esp32C3, kind),
                    image.at_section(site)
                )),
                _ => {}
            }
        }
    }

    /// Records the upper part of a distance at the `auipc` at `site`,
    /// checked to be one that reaches `target` as linked.
    fn upper(&mut self, image: &Image<'_>, site: u32, target: u32, problems: &mut Vec<String>) {
        let insn = image.word_at(site);
        let distance = linked(image, target).wrapping_sub(linked(image, site));
        if insn & 0x7F != 0x17 || insn & 0xFFFF_F000 != hi20(distance) {
            problems.push(format!(
                "no `auipc` at {} reaches as its relocation says: link the plugin with `--no-relax`",
                image.at_section(site)
            ));
            return;
        }
        self.record(PatchKind::Hi20, site, target, problems);
    }

    /// Records the lower part of a distance at `site`, whose `auipc` at
    /// `auipc` reaches `target`, checked as linked.
    fn lower(
        &mut self,
        image: &Image<'_>,
        kind: PatchKind,
        site: u32,
        auipc: u32,
        target: u32,
        problems: &mut Vec<String>,
    ) {
        let insn = image.word_at(site);
        let distance = linked(image, target).wrapping_sub(linked(image, auipc));
        let lo = distance.wrapping_sub(hi20(distance));
        let imm = match kind {
            PatchKind::Lo12S => (((insn as i32) >> 25) << 5) as u32 | ((insn >> 7) & 0x1F),
            _ => ((insn as i32) >> 20) as u32,
        };
        if imm != lo {
            problems.push(format!(
                "the instruction at {} does not reach as its relocation says: link the plugin with `--no-relax`",
                image.at_section(site)
            ));
            return;
        }
        self.record(kind, site, auipc, problems);
    }

    fn record(&mut self, kind: PatchKind, site: u32, reference: u32, problems: &mut Vec<String>) {
        if site >= PatchKind::SITES {
            problems.push(format!(
                "hot code reached from beyond {} bytes into the image",
                PatchKind::SITES
            ));
            return;
        }
        self.patches.push([kind.record(site), reference]);
    }
}

/// The upper part of `distance` as `auipc` holds it, rounded so that the
/// lower part is signed.
fn hi20(distance: u32) -> u32 {
    distance.wrapping_add(0x800) & 0xFFFF_F000
}

/// The address image offset `at` is linked at: its data address in the
/// read-only data, its instruction address in the code.
fn linked(image: &Image<'_>, at: u32) -> u32 {
    if at < image.code_at {
        LINK_BASE + at
    } else {
        LINK_BASE + CODE_OFFSET + at
    }
}

/// The image offset of the symbol `name` in the code.
fn code_symbol(image: &Image<'_>, symbols: &[Symbol], name: &str) -> Result<u32, String> {
    let symbol = symbols
        .iter()
        .find(|s| s.name == name && s.shndx != crate::elf::SHN_UNDEF)
        .ok_or_else(|| {
            format!("no `{name}`: link the plugin with paperpaw-plugin.x as it is now")
        })?;
    symbol
        .value
        .checked_sub(LINK_BASE + CODE_OFFSET)
        .filter(|at| (image.code_at..=image.len).contains(at))
        .ok_or_else(|| format!("`{name}` is at {:#010x}, not in .text", symbol.value))
}

/// The relocations of the image that reach into it, as image offsets.
fn reaches<'s>(
    elf: &Elf<'_>,
    image: &Image<'_>,
    chip: Chip,
    symbols: &'s [Symbol],
) -> Result<Vec<Reach<'s>>, String> {
    let mut reaches = Vec::new();
    for section in elf.sections.iter().filter(|s| s.kind == SHT_RELA) {
        let target = section.info as usize;
        let (start, applies_to) = if target == image.rodata_index {
            (0, image.rodata)
        } else if target == image.text_index {
            (image.code_at, image.text)
        } else {
            continue;
        };
        for rela in elf.relas(section)? {
            let symbol = match rela.symbol {
                0 => None,
                i => symbols.get(i as usize),
            };
            let kind = relocation::kind(chip, rela.kind);
            if matches!(kind, Kind::Marker | Kind::Unknown) || image.place(symbol) != Place::Image {
                continue;
            }
            let value = symbol
                .map_or(0, |s| s.value)
                .wrapping_add(rela.addend as u32);
            let Some(target) = image.offset_of(value) else {
                continue;
            };
            reaches.push(Reach {
                kind: rela.kind,
                site: start + rela.offset.wrapping_sub(applies_to.addr),
                target,
                symbol,
            });
        }
    }
    Ok(reaches)
}

/// The function whose code holds image offset `at` and the offset into
/// it, as a problem names a place.
fn function_at(image: &Image<'_>, symbols: &[Symbol], at: u32) -> String {
    match function(image, symbols, at) {
        Some(s) => format!("`{}`+{:#x}", s.name, linked(image, at) - s.value),
        None => image.at_section(at),
    }
}

/// The name of the function whose code holds image offset `at`.
fn function_name(image: &Image<'_>, symbols: &[Symbol], at: u32) -> String {
    function(image, symbols, at).map_or_else(|| image.at_section(at), |s| s.name.clone())
}

/// What `reach` reaches, by name: the function there, or its symbol.
fn target_name(image: &Image<'_>, symbols: &[Symbol], reach: &Reach<'_>) -> String {
    if let Some(s) = function(image, symbols, reach.target) {
        return s.name.clone();
    }
    match reach.symbol {
        Some(s) if s.kind() != STT_SECTION => s.name.clone(),
        _ => image.at_section(reach.target),
    }
}

/// The function symbol whose code holds image offset `at`.
fn function<'s>(image: &Image<'_>, symbols: &'s [Symbol], at: u32) -> Option<&'s Symbol> {
    if at < image.code_at {
        return None;
    }
    let address = linked(image, at);
    symbols.iter().find(|s| {
        usize::from(s.shndx) == image.text_index
            && s.kind() != STT_SECTION
            && s.size > 0
            && address.wrapping_sub(s.value) < s.size
    })
}

impl Image<'_> {
    /// The 32-bit word at image offset `at` (any alignment), 0 past the end.
    fn word_at(&self, at: u32) -> u32 {
        let at = at as usize;
        if at + 4 <= self.bytes.len() {
            le32(&self.bytes, at)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_native::{API_SYMBOL, ENTRY_SYMBOL, HEADER_LEN, Relocator};

    use super::*;
    use crate::elf::build::Builder;
    use crate::elf::{EM_RISCV, EM_XTENSA, ET_EXEC, SHF_ALLOC, SHN_ABS};
    use crate::pack::pack;

    const TEXT: u32 = LINK_BASE + CODE_OFFSET + 96;
    const CONSTANT: u32 = LINK_BASE + 80;
    /// Cold code: `cold` calls `hot` at 0, `helper` at 8. Hot code from
    /// 16: `hot` loads and stores `CONSTANT` and calls `helper` at 28,
    /// then a word holds the address of `cold`.
    const HOT: u32 = TEXT + 16;
    const HELPER: u32 = TEXT + 8;
    const END: u32 = TEXT + 40;

    fn auipc(rd: u32, distance: u32) -> u32 {
        hi20(distance) | (rd << 7) | 0x17
    }

    fn lo12(distance: u32) -> u32 {
        distance.wrapping_sub(hi20(distance)) & 0xFFF
    }

    fn i_type(opcode: u32, funct3: u32, rd: u32, rs1: u32, distance: u32) -> u32 {
        (lo12(distance) << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
    }

    fn sw(distance: u32) -> u32 {
        let lo = lo12(distance);
        ((lo & 0xFE0) << 20) | (11 << 20) | (10 << 15) | (2 << 12) | ((lo & 0x1F) << 7) | 0x23
    }

    /// The plugin, its hot code from `hot_from` on, as a builder to add
    /// to.
    struct Plugin {
        b: Builder,
        text: usize,
        cold: u32,
        hot: u32,
    }

    fn plugin(machine: u16, hot_from: u32) -> Plugin {
        let mut rodata = [0u8; 96];
        rodata[..8].copy_from_slice(&1u64.to_le_bytes());
        rodata[12..16].copy_from_slice(&(64u32 | (5 << 16)).to_le_bytes());
        let to_constant = CONSTANT.wrapping_sub(HOT);
        let to_helper = HELPER.wrapping_sub(HOT + 12);
        let mut text = Vec::new();
        for insn in [
            auipc(1, HOT - TEXT),
            i_type(0x67, 0, 1, 1, HOT - TEXT),
            0x0000_8067,
            0x0000_0013,
            auipc(10, to_constant),
            i_type(0x03, 2, 11, 10, to_constant),
            sw(to_constant),
            auipc(1, to_helper),
            i_type(0x67, 0, 1, 1, to_helper),
            TEXT,
        ] {
            text.extend_from_slice(&insn.to_le_bytes());
        }
        let mut b = Builder::new(machine, ET_EXEC);
        let ro = b.section(".rodata", SHF_ALLOC, LINK_BASE, &rodata, 16) as u16;
        let tx = b.section(".text", SHF_ALLOC | 4, TEXT, &text, 4);
        b.section(
            ".comment",
            0x30,
            0,
            b"rustc version 1.99.0 (b940084d7 2026-09-28)\0",
            1,
        );
        b.symbol(ENTRY_SYMBOL, LINK_BASE, 32, ro, false);
        b.symbol(API_SYMBOL, LINK_BASE + 32, 0, SHN_ABS, false);
        for (n, name) in paperpaw_native::TABLE_SYMBOLS.iter().enumerate() {
            b.symbol(name, LINK_BASE + 36 + 4 * n as u32, 0, SHN_ABS, false);
        }
        let constant = b.symbol("CONSTANT", CONSTANT, 4, ro, true);
        let label = b.symbol(".Lpcrel_hi0", HOT, 0, tx as u16, true);
        let cold = b.symbol("cold", TEXT, 8, tx as u16, false);
        let helper = b.symbol("helper", HELPER, 4, tx as u16, false);
        let hot = b.symbol("hot", HOT, 20, tx as u16, false);
        b.symbol(HOT_SYMBOL, hot_from, 0, tx as u16, false);
        b.symbol(HOT_END_SYMBOL, END, 0, tx as u16, false);
        b.rela(tx, TEXT, hot, CALL_PLT);
        b.rela(tx, HOT, constant, PCREL_HI20);
        b.rela(tx, HOT + 4, label, PCREL_LO12_I);
        b.rela(tx, HOT + 8, label, PCREL_LO12_S);
        b.rela(tx, HOT + 12, helper, CALL);
        Plugin {
            b,
            text: tx,
            cold,
            hot,
        }
    }

    #[test]
    fn reaches_across_the_edge_become_patches() {
        let packed = pack(&plugin(EM_RISCV, HOT).b.build()).unwrap();
        let header = packed.header;
        assert_eq!((header.hot_at, header.hot_len), (112, 24));
        let words: Vec<u32> = packed.file[HEADER_LEN + header.image_len as usize..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_le_bytes(*w))
            .collect();
        assert_eq!(
            words.as_chunks::<2>().0,
            [
                [PatchKind::Hi20.record(96), 112],
                [PatchKind::Hi20.record(112), 80],
                [PatchKind::Hi20.record(124), 104],
                [PatchKind::Lo12I.record(100), 96],
                [PatchKind::Lo12I.record(116), 112],
                [PatchKind::Lo12I.record(128), 124],
                [PatchKind::Lo12S.record(120), 112],
            ]
        );
        // The loader takes them: the call of `hot` reaches it where it
        // runs.
        let len = header.image_len as usize;
        let mut image = packed.file[HEADER_LEN..HEADER_LEN + len].to_vec();
        let relocator = Relocator::new(&header, &words, LINK_BASE, 0)
            .and_then(|r| r.hot_at(0x4039_0000))
            .unwrap();
        relocator.apply(0, &mut image).unwrap();
        let hi = le32(&image, 96) & 0xFFFF_F000;
        let lo = ((le32(&image, 100) as i32) >> 20) as u32;
        assert_eq!(TEXT.wrapping_add(hi).wrapping_add(lo), 0x4039_0000);
    }

    #[test]
    fn without_hot_code_and_on_xtensa_there_is_none() {
        for (machine, hot_from) in [(EM_RISCV, END), (EM_XTENSA, HOT)] {
            let header = pack(&plugin(machine, hot_from).b.build()).unwrap().header;
            assert_eq!((header.hot_at, header.hot_len, header.patches), (0, 0, 0));
        }
    }

    #[test]
    fn a_reach_that_cannot_be_patched_is_refused() {
        // A jump of short reach from cold code into hot code.
        let mut p = plugin(EM_RISCV, HOT);
        p.b.rela(p.text, TEXT + 4, p.hot, 17);
        let error = pack(&p.b.build()).unwrap_err();
        assert!(
            error.contains(
                "`cold` reaches `hot` across the edge of hot code (R_RISCV_JAL at .text+0x4), \
                 which cannot be patched: mark both functions hot or neither"
            ),
            "{error}"
        );
        // Within hot code it is fine.
        let mut p = plugin(EM_RISCV, HOT);
        p.b.rela(p.text, HOT + 16, p.hot, 17);
        assert!(pack(&p.b.build()).is_ok());
        // A word to relocate in hot code.
        let mut p = plugin(EM_RISCV, HOT);
        p.b.rela(p.text, HOT + 20, p.cold, 1);
        let error = pack(&p.b.build()).unwrap_err();
        assert!(
            error.contains("a word to relocate lies in hot code, at .text+0x24"),
            "{error}"
        );
    }

    #[test]
    fn code_that_does_not_reach_as_its_relocation_says_is_refused() {
        // A call relaxed or linked otherwise: no `auipc` where it says.
        let mut p = plugin(EM_RISCV, HOT);
        p.b.rela(p.text, TEXT + 8, p.hot, CALL);
        let error = pack(&p.b.build()).unwrap_err();
        assert!(
            error.contains("no `auipc` at .text+0x8 reaches as its relocation says"),
            "{error}"
        );
        assert!(error.contains("--no-relax"), "{error}");
    }
}
