//! A plugin file (`.ppn`, see `paperpaw-native`) from a native plugin
//! linked with `paperpaw-plugin.x` and `--emit-relocs`.
//!
//! The image is the linked `.rodata` (at [`LINK_BASE`]) followed by
//! `.text` (at `LINK_BASE + CODE_OFFSET` and the size of `.rodata`), so
//! that the code reaches its data, and itself, by distances that stay
//! right wherever the image is placed. The relocations the linker kept
//! say which words hold an address in the plugin (to relocate), and show
//! what could not move with it, which is refused with the reason:
//!
//! - writable data (a plugin keeps its state on the kernel's heap), and
//!   any other section that would take memory;
//! - symbols nobody defines;
//! - addresses of the plugin built into instructions (on RISC-V, code
//!   not built with `-C code-model=medium`);
//! - relative reaches and differences out of the image, which would
//!   change when it moves;
//! - a plugin that does not say which compiler built it, or says more
//!   than one (its `.comment`): it runs only on firmware built by the
//!   same, so its file names it.
//!
//! The hot code and the patches that move it come from [`crate::hot`].

use std::collections::{BTreeMap, BTreeSet};

use paperpaw_native::{
    API_SYMBOL, API_WORDS, CODE_OFFSET, Chip, ENTRY_SYMBOL, HEADER_LEN, Header, LINK_BASE,
    MAX_IMAGE_LEN, TABLE_SYMBOLS,
};

use crate::elf::{
    EM_RISCV, EM_XTENSA, ET_EXEC, ET_REL, Elf, SHN_ABS, SHN_UNDEF, SHT_REL, SHT_RELA, STT_SECTION,
    Section, Symbol, le32,
};
use crate::hot::Hot;
use crate::relocation::{self, Kind};

/// The bytes an [`Entry`](paperpaw_native::Entry) takes.
const ENTRY_LEN: u32 = 32;
/// The most problems with relocations told at once.
const MAX_TOLD: usize = 20;

/// A plugin file made.
#[derive(Debug)]
pub struct Packed {
    pub header: Header,
    /// The compiler it was built with: its release, as
    /// `paperpaw_native::compiler::release` gives it.
    pub compiler: String,
    /// The whole file, checked with [`Header::check_file`].
    pub file: Vec<u8>,
}

/// The plugin file of the linked plugin `elf`.
pub fn pack(elf: &[u8]) -> Result<Packed, String> {
    let elf = Elf::parse(elf)?;
    match elf.kind {
        ET_EXEC => {}
        ET_REL => return Err("an object, not a linked plugin: pack the plugin as linked".into()),
        _ => return Err("not a linked program".into()),
    }
    let chip = match elf.machine {
        EM_RISCV => Chip::Esp32C3,
        EM_XTENSA => Chip::Esp32S3,
        other => {
            return Err(format!(
                "made for machine {other}, neither RISC-V nor Xtensa"
            ));
        }
    };
    let (symtab, symbols) = elf
        .symbols()?
        .ok_or("no symbol table: do not strip the plugin")?;
    let image = Image::of(&elf, &symbols)?;
    let entry = image.entry(&symbols)?;
    let api = image.api(&symbols)?;
    let words = image.words(&elf, chip, symtab, &symbols)?;
    if let Some(at) = words.iter().find(|&&w| w >= api && w < api + 4 * API_WORDS) {
        return Err(format!(
            "the word at {at:#x} after `{API_SYMBOL}` has a relocation: nothing may be linked \
             into the firmware's words"
        ));
    }
    let hot = Hot::of(&elf, &image, chip, &symbols, &words)?;

    let compiler = compiler(&elf)?;
    let fingerprint = u64::from(image.word(entry)) | (u64::from(image.word(entry + 4)) << 32);
    // The entry after `make` (a word on the chips): what the plugin needs.
    let needs = image.word(entry + 12);
    let (needs_api, interface) = (needs & 0xFFFF, (needs >> 16) as u16);
    let mut families = [0u16; 8];
    for (i, family) in families.iter_mut().enumerate() {
        let word = image.word(entry + 16 + 4 * (i as u32 / 2));
        *family = (word >> (16 * (i % 2))) as u16;
    }
    if interface == 0 {
        return Err(
            "its entry names no plugin interface: built with an SDK before interface 5".into(),
        );
    }
    let align = [image.rodata.addralign, image.text.addralign, 4]
        .into_iter()
        .max()
        .unwrap_or(4)
        .min(4096);
    let mut rest = image.bytes;
    for word in words.iter().chain(hot.patches.iter().flatten()) {
        rest.extend_from_slice(&word.to_le_bytes());
    }
    let mut header = Header {
        chip,
        fingerprint,
        base: LINK_BASE,
        image_len: image.len,
        code_at: image.code_at,
        entry,
        api,
        relocs: words.len() as u32,
        align,
        compiler: paperpaw_native::compiler::id(&compiler),
        hot_at: hot.at,
        hot_len: hot.len,
        patches: hot.patches.len() as u32,
        needs_api,
        // The SDK's revision of every family; the families the code does
        // not use are to be 0 here (the words it reads: firmware step of
        // interface 5).
        families,
        interface,
        sha256: [0; 32],
    };
    let mut sha = header.digest();
    sha.update(&rest);
    header.sha256 = sha.finish();
    let mut file = Vec::with_capacity(HEADER_LEN + rest.len());
    file.extend_from_slice(&header.to_bytes());
    file.extend_from_slice(&rest);
    Header::check_file(&file).map_err(|e| format!("the file made does not hold: {e}"))?;
    Ok(Packed {
        header,
        compiler,
        file,
    })
}

/// The release of the one compiler that built `elf`, as its `.comment`
/// names it: Rust writes its own into every object, the core libraries'
/// included.
fn compiler(elf: &Elf<'_>) -> Result<String, String> {
    let named = match elf.section(".comment") {
        Some((_, section)) => paperpaw_native::compiler::named_in(elf.data(section)?)
            .into_iter()
            .map(String::from)
            .collect(),
        None => Vec::new(),
    };
    match named.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(
            "no .comment names the compiler: link the plugin with paperpaw-plugin.x as it is now,              which keeps it"
                .into(),
        ),
        _ => Err(format!(
            "built by more than one compiler ({}): build the plugin and the core libraries with              the one of PaperPaw's release",
            named.join(", ")
        )),
    }
}

/// The image: `.rodata`, then `.text`.
pub(crate) struct Image<'e> {
    pub(crate) rodata: &'e Section,
    pub(crate) rodata_index: usize,
    pub(crate) text: &'e Section,
    pub(crate) text_index: usize,
    pub(crate) bytes: Vec<u8>,
    pub(crate) code_at: u32,
    pub(crate) len: u32,
}

/// Where what a relocation refers to lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// In the image: it moves with it.
    Image,
    /// Somewhere that stays (the ROM, for instance).
    Outside,
    /// Nowhere: nobody defines it.
    Undefined,
}

impl<'e> Image<'e> {
    /// The image of `elf`, all its sections checked.
    fn of(elf: &'e Elf<'_>, symbols: &[Symbol]) -> Result<Self, String> {
        let (rodata_index, rodata) = elf
            .section(".rodata")
            .ok_or("no .rodata: link the plugin with paperpaw-plugin.x")?;
        let (text_index, text) = elf
            .section(".text")
            .ok_or("no .text: the plugin has no code")?;
        if rodata.addr != LINK_BASE {
            return Err(format!(
                ".rodata is linked at {:#010x}, not at {LINK_BASE:#010x}: link the plugin with paperpaw-plugin.x",
                rodata.addr
            ));
        }
        let code = LINK_BASE + CODE_OFFSET + rodata.size;
        if text.addr != code {
            return Err(format!(
                ".text is linked at {:#010x}, not right after .rodata at {code:#010x}: link the plugin with paperpaw-plugin.x",
                text.addr
            ));
        }
        if text.size == 0 {
            return Err(".text is empty: the plugin has no code".into());
        }
        writable_data(elf, symbols)?;
        if let Some(other) = elf
            .sections
            .iter()
            .find(|s| s.is_alloc() && s.size > 0 && s.name != ".rodata" && s.name != ".text")
        {
            return Err(format!(
                "an unexpected section {} ({} bytes): a plugin is only its .rodata and .text",
                other.name, other.size
            ));
        }
        let len = u64::from(rodata.size) + u64::from(text.size);
        if len > u64::from(MAX_IMAGE_LEN) {
            return Err(format!(
                "the image is {len} bytes, more than the {MAX_IMAGE_LEN} a plugin may have"
            ));
        }
        let mut bytes = elf.data(rodata)?.to_vec();
        bytes.extend_from_slice(elf.data(text)?);
        Ok(Self {
            rodata,
            rodata_index,
            text,
            text_index,
            bytes,
            code_at: rodata.size,
            len: len as u32,
        })
    }

    /// The offset of the symbol called `name` in `.rodata`, for something
    /// of `len` bytes.
    fn in_rodata(&self, symbols: &[Symbol], name: &str, len: u32) -> Result<u32, String> {
        let symbol = symbols
            .iter()
            .find(|s| s.name == name && s.shndx != SHN_UNDEF)
            .ok_or_else(|| format!("no `{name}`"))?;
        symbol
            .value
            .checked_sub(LINK_BASE)
            .filter(|at| at.checked_add(len).is_some_and(|end| end <= self.code_at))
            .ok_or_else(|| format!("`{name}` is at {:#010x}, not in .rodata", symbol.value))
    }

    /// The plugin's entry: its offset.
    fn entry(&self, symbols: &[Symbol]) -> Result<u32, String> {
        let at = self
            .in_rodata(symbols, ENTRY_SYMBOL, ENTRY_LEN)
            .map_err(|e| format!("{e}: a plugin's entry is made by the SDK's `plugin!`"))?;
        if !at.is_multiple_of(8) {
            return Err(format!("`{ENTRY_SYMBOL}` is not 8-aligned"));
        }
        Ok(at)
    }

    /// The word for the address of the firmware's table: its offset.
    fn api(&self, symbols: &[Symbol]) -> Result<u32, String> {
        let at = self
            .in_rodata(symbols, API_SYMBOL, 4)
            .map_err(|e| format!("{e}: link the plugin with paperpaw-plugin.x"))?;
        if !at.is_multiple_of(4) {
            return Err(format!("`{API_SYMBOL}` is not 4-aligned"));
        }
        if self.word(at) != 0 {
            return Err(format!("`{API_SYMBOL}` is not linked as 0"));
        }
        // The families' words follow it, as paperpaw-plugin.x makes them.
        for (n, name) in TABLE_SYMBOLS.iter().enumerate() {
            let word = self.in_rodata(symbols, name, 4).map_err(|e| {
                format!("{e}: link the plugin with this PaperPaw's paperpaw-plugin.x")
            })?;
            if word != at + 4 * (n as u32 + 1) || self.word(word) != 0 {
                return Err(format!(
                    "`{name}` is not linked as 0 right after the words before it"
                ));
            }
        }
        Ok(at)
    }

    fn word(&self, at: u32) -> u32 {
        le32(&self.bytes, at as usize)
    }

    /// The image offset of `value`, an address in the image as linked:
    /// by data in the read-only data, by code in the code (its end
    /// included).
    pub(crate) fn offset_of(&self, value: u32) -> Option<u32> {
        let data = value.wrapping_sub(LINK_BASE);
        let code = value.wrapping_sub(LINK_BASE + CODE_OFFSET);
        if data < self.code_at {
            Some(data)
        } else {
            (self.code_at..=self.len).contains(&code).then_some(code)
        }
    }

    /// Image offset `at` as a section and the offset into it.
    pub(crate) fn at_section(&self, at: u32) -> String {
        match at.checked_sub(self.code_at) {
            Some(offset) => format!("{}+{offset:#x}", self.text.name),
            None => format!("{}+{at:#x}", self.rodata.name),
        }
    }

    /// Whether `value` is an address in the image as linked, by data or by
    /// code (its end included), as the loader takes it.
    fn points_inside(&self, value: u32) -> bool {
        value.wrapping_sub(LINK_BASE) <= self.len
            || value.wrapping_sub(LINK_BASE + CODE_OFFSET) <= self.len
    }

    /// Where `symbol` lies (`None`: a relocation without a symbol, its
    /// addend an absolute value).
    pub(crate) fn place(&self, symbol: Option<&Symbol>) -> Place {
        let Some(symbol) = symbol else {
            return Place::Outside;
        };
        match symbol.shndx {
            SHN_UNDEF => Place::Undefined,
            i if usize::from(i) == self.rodata_index || usize::from(i) == self.text_index => {
                Place::Image
            }
            // A linker script's symbol may be absolute though it names a
            // place in the image.
            SHN_ABS if self.points_inside(symbol.value) => Place::Image,
            _ => Place::Outside,
        }
    }

    /// The words to relocate, in order, every relocation of the image
    /// checked.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn words(
        &self,
        elf: &Elf<'_>,
        chip: Chip,
        symtab: usize,
        symbols: &[Symbol],
    ) -> Result<BTreeSet<u32>, String> {
        let mut words = BTreeSet::new();
        let mut problems = Vec::new();
        let mut undefined = BTreeMap::new();
        let mut any = false;
        for section in &elf.sections {
            if section.kind != SHT_RELA && section.kind != SHT_REL {
                continue;
            }
            let target = section.info as usize;
            let (start, applies_to) = if target == self.rodata_index {
                (0, self.rodata)
            } else if target == self.text_index {
                (self.code_at, self.text)
            } else {
                continue;
            };
            if section.kind == SHT_REL {
                return Err(format!(
                    "{}: relocations without addends are not read",
                    section.name
                ));
            }
            if section.link as usize != symtab {
                return Err(format!("{}: refers to another symbol table", section.name));
            }
            any = true;
            for rela in elf.relas(section)? {
                let offset = rela.offset.wrapping_sub(applies_to.addr);
                if offset >= applies_to.size {
                    return Err(format!(
                        "{}: a relocation at {:#010x} lies outside {}",
                        section.name, rela.offset, applies_to.name
                    ));
                }
                let at = start + offset;
                let symbol = match rela.symbol {
                    0 => None,
                    i => Some(symbols.get(i as usize).ok_or_else(|| {
                        format!(
                            "{}: a relocation names symbol {i}, which is not there",
                            section.name
                        )
                    })?),
                };
                let place = self.place(symbol);
                let kind = relocation::kind(chip, rela.kind);
                let told = || Told {
                    elf,
                    chip,
                    kind: rela.kind,
                    section: applies_to,
                    offset,
                    symbol,
                };
                match (kind, place) {
                    (Kind::Marker, _) => {}
                    (Kind::Unknown, _) => {
                        problems.push(format!("{}: not known to this tool", told()));
                    }
                    (_, Place::Undefined) => {
                        undefined
                            .entry(told().symbol_name())
                            .or_insert_with(|| told().place());
                    }
                    (Kind::Word, Place::Image) => {
                        if !at.is_multiple_of(4) {
                            problems.push(format!(
                                "{}: a word to relocate, but not 4-aligned in the image",
                                told()
                            ));
                        } else if at + 4 > self.len {
                            problems.push(format!("{}: runs past the image", told()));
                        } else if !self.points_inside(self.word(at)) {
                            problems.push(format!(
                                "{}: the word holds {:#010x}, which points outside the image",
                                told(),
                                self.word(at)
                            ));
                        } else {
                            words.insert(at);
                        }
                    }
                    (Kind::Absolute, Place::Image) => {
                        let hint = match chip {
                            Chip::Esp32C3 => "; build it with `-C code-model=medium`",
                            Chip::Esp32S3 => "",
                        };
                        problems.push(format!(
                            "{}: an address of the plugin in an instruction cannot move{hint}",
                            told()
                        ));
                    }
                    (Kind::Relative, Place::Outside) => {
                        problems.push(format!(
                            "{}: a relative reach out of the plugin breaks when it moves (reach it through a pointer)",
                            told()
                        ));
                    }
                    (Kind::Difference, Place::Outside) => {
                        problems.push(format!(
                            "{}: a difference with a place outside the plugin changes when it moves",
                            told()
                        ));
                    }
                    (Kind::Word | Kind::Absolute, Place::Outside)
                    | (Kind::Relative | Kind::Difference, Place::Image) => {}
                }
            }
        }
        if !any {
            return Err("no relocations kept: link the plugin with `--emit-relocs`".into());
        }
        for (name, place) in undefined {
            problems.push(format!("undefined symbol `{name}` (used at {place})"));
        }
        if problems.is_empty() {
            return Ok(words);
        }
        let count = problems.len();
        let mut text = match count {
            1 => "the plugin cannot be packed:".to_string(),
            _ => format!("the plugin cannot be packed ({count} problems):"),
        };
        for problem in problems.iter().take(MAX_TOLD) {
            text.push_str("\n    ");
            text.push_str(problem);
        }
        if count > MAX_TOLD {
            text.push_str(&format!("\n    and {} more", count - MAX_TOLD));
        }
        Err(text)
    }
}

/// A relocation, as a problem with it is told.
struct Told<'a> {
    elf: &'a Elf<'a>,
    chip: Chip,
    kind: u32,
    section: &'a Section,
    offset: u32,
    symbol: Option<&'a Symbol>,
}

impl Told<'_> {
    fn place(&self) -> String {
        format!("{}+{:#x}", self.section.name, self.offset)
    }

    fn symbol_name(&self) -> String {
        match self.symbol {
            None => "no symbol".into(),
            Some(s) if s.kind() == STT_SECTION => self
                .elf
                .sections
                .get(usize::from(s.shndx))
                .map_or_else(|| "a section".into(), |section| section.name.clone()),
            Some(s) => s.name.clone(),
        }
    }
}

impl std::fmt::Display for Told<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {} against `{}`",
            relocation::name(self.chip, self.kind),
            self.place(),
            self.symbol_name()
        )
    }
}

/// Refuses writable data, naming its largest symbols.
fn writable_data(elf: &Elf<'_>, symbols: &[Symbol]) -> Result<(), String> {
    let mut text = String::new();
    for (index, section) in elf.sections.iter().enumerate() {
        if !section.is_writable() || section.size == 0 {
            continue;
        }
        let mut statics: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| usize::from(s.shndx) == index && s.size > 0 && s.kind() != STT_SECTION)
            .collect();
        statics.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
        text.push_str(&format!("\n    {}: {} bytes", section.name, section.size));
        for (i, s) in statics.iter().take(5).enumerate() {
            let lead = if i == 0 { ", of them" } else { "," };
            text.push_str(&format!("{lead} `{}` ({} bytes)", s.name, s.size));
        }
        if statics.len() > 5 {
            text.push_str(&format!(" and {} more", statics.len() - 5));
        }
    }
    if text.is_empty() {
        return Ok(());
    }
    Err(format!(
        "a plugin has no writable data (its state lives on the kernel's heap), but this one has:{text}"
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::elf::build::Builder;
    use crate::elf::{SHF_ALLOC, SHF_WRITE};

    const TEXT: u32 = LINK_BASE + CODE_OFFSET + 96;
    const DATA: u32 = LINK_BASE + 80;
    const ROM: u32 = 0x4000_1234;
    const FINGERPRINT: u64 = 0x0123_4567_89ab_cdef;
    /// What the linker keeps of the compilers.
    const COMMENT: &[u8] = b"Linker: LLD 21.1.0\0rustc version 1.99.0 (b940084d7 2026-09-28)\0";

    /// The kinds a test uses, by what they are on each chip.
    struct Kinds {
        word: u32,
        call: u32,
        absolute: u32,
        difference: u32,
        marker: u32,
    }

    /// A small plugin that packs: 96 bytes of read-only data (the entry
    /// at 0 with the address of `make` at 8, the firmware's words at 32,
    /// the address of `DATA` at 68, a difference at 72, `DATA` at 80), then 32
    /// bytes of code (a call of `make`, a relative reach of `DATA`, a
    /// literal of `DATA`'s address at 24 and of a ROM function's at 28).
    struct Plugin {
        b: Builder,
        rodata: usize,
        text: usize,
        data: u32,
        rom: u32,
        k: Kinds,
    }

    fn plugin(machine: u16) -> Plugin {
        plugin_with(machine, COMMENT)
    }

    /// The small plugin, with `comment` as its `.comment` (none if empty).
    fn plugin_with(machine: u16, comment: &[u8]) -> Plugin {
        let k = match machine {
            EM_RISCV => Kinds {
                word: 1,
                call: 18,
                absolute: 26,
                difference: 35,
                marker: 51,
            },
            _ => Kinds {
                word: 1,
                call: 20,
                absolute: 35,
                difference: 19,
                marker: 11,
            },
        };
        let mut rodata = [0u8; 96];
        rodata[..8].copy_from_slice(&FINGERPRINT.to_le_bytes());
        rodata[8..12].copy_from_slice(&TEXT.to_le_bytes());
        // Needs 64 bytes of the firmware's table, plugin interface 5.
        rodata[12..16].copy_from_slice(&(64u32 | (5 << 16)).to_le_bytes());
        rodata[68..72].copy_from_slice(&DATA.to_le_bytes());
        rodata[80..88].copy_from_slice(b"PaperPaw");
        let mut text = [0x13u8; 32];
        text[24..28].copy_from_slice(&DATA.to_le_bytes());
        text[28..32].copy_from_slice(&ROM.to_le_bytes());

        let mut b = Builder::new(machine, ET_EXEC);
        let ro = b.section(".rodata", SHF_ALLOC, LINK_BASE, &rodata, 16);
        let tx = b.section(".text", SHF_ALLOC | 4, TEXT, &text, 4);
        if !comment.is_empty() {
            b.section(".comment", 0x30, 0, comment, 1);
        }
        let data = b.symbol("DATA", DATA, 8, ro as u16, true);
        let label = b.symbol(".Lpcrel", TEXT + 4, 0, tx as u16, true);
        b.symbol(ENTRY_SYMBOL, LINK_BASE, 32, ro as u16, false);
        b.symbol(API_SYMBOL, LINK_BASE + 32, 0, SHN_ABS, false);
        for (n, name) in TABLE_SYMBOLS.iter().enumerate() {
            b.symbol(name, LINK_BASE + 36 + 4 * n as u32, 0, SHN_ABS, false);
        }
        let make = b.symbol("make", TEXT, 8, tx as u16, false);
        // No hot code.
        b.symbol("PAPERPAW_HOT", TEXT + 32, 0, tx as u16, false);
        b.symbol("PAPERPAW_HOT_END", TEXT + 32, 0, tx as u16, false);
        let rom = b.symbol("rom_fn", ROM, 0, SHN_ABS, false);
        b.rela(ro, LINK_BASE + 8, make, k.word);
        b.rela(ro, LINK_BASE + 68, data, k.word);
        b.rela(ro, LINK_BASE + 72, label, k.difference);
        b.rela(tx, TEXT, make, k.marker);
        b.rela(tx, TEXT, make, k.call);
        b.rela(tx, TEXT + 4, data, k.call);
        b.rela(tx, TEXT + 24, data, k.word);
        b.rela(tx, TEXT + 28, rom, k.word);
        Plugin {
            b,
            rodata: ro,
            text: tx,
            data,
            rom,
            k,
        }
    }

    /// The file names the one compiler that built the plugin, and a
    /// plugin of none or of two is refused.
    #[test]
    fn the_compiler_is_named_and_must_be_one() {
        let packed = pack(&plugin(EM_RISCV).b.build()).unwrap();
        assert_eq!(packed.compiler, "1.99.0 (b940084d7 2026-09-28)");
        assert_eq!(
            packed.header.compiler,
            paperpaw_native::compiler::id("1.99.0 (b940084d7 2026-09-28)")
        );
        let none = pack(&plugin_with(EM_RISCV, b"Linker: LLD 21.1.0\0").b.build()).unwrap_err();
        assert!(none.contains("no .comment names the compiler"), "{none}");
        let two = pack(
            &plugin_with(
                EM_XTENSA,
                b"rustc version 1.99.0 (b940084d7 2026-09-28)\0rustc version 1.97.0-nightly (8ea53bcd7 2026-07-08) (1.97.0.0)\0",
            )
            .b
            .build(),
        )
        .unwrap_err();
        assert!(two.contains("more than one compiler"), "{two}");
    }

    /// The packed file of the small plugin for `machine`.
    pub(crate) fn packed(machine: u16) -> Vec<u8> {
        pack(&plugin(machine).b.build()).unwrap().file
    }

    fn refused(p: &Plugin) -> String {
        pack(&p.b.build()).unwrap_err()
    }

    #[test]
    fn a_plugin_packs_for_both_chips() {
        for (machine, chip) in [(EM_RISCV, Chip::Esp32C3), (EM_XTENSA, Chip::Esp32S3)] {
            let elf = plugin(machine).b.build();
            let packed = pack(&elf).unwrap();
            let (header, _) = Header::check_file(&packed.file).unwrap();
            assert_eq!(header, packed.header);
            assert_eq!(header.chip, chip);
            assert_eq!(header.fingerprint, FINGERPRINT);
            assert_eq!((header.base, header.image_len), (LINK_BASE, 128));
            assert_eq!((header.code_at, header.entry, header.api), (96, 0, 32));
            assert_eq!((header.needs_api, header.interface), (64, 5));
            assert_eq!((header.relocs, header.align), (3, 16));
            let image = &packed.file[HEADER_LEN..HEADER_LEN + 128];
            assert_eq!(image[..8], FINGERPRINT.to_le_bytes());
            assert_eq!(image[96..100], [0x13; 4]);
            let words: Vec<u32> = packed.file[HEADER_LEN + 128..]
                .chunks(4)
                .map(|w| le32(w, 0))
                .collect();
            // The ROM's address stays as it is.
            assert_eq!(words, [8, 68, 120]);
        }
    }

    #[test]
    fn writable_data_is_refused_naming_its_statics() {
        let mut p = plugin(EM_RISCV);
        let bss = p.b.nobits(".bss", SHF_ALLOC | SHF_WRITE, 0x3FC8_0000, 24);
        p.b.symbol("STATE", 0x3FC8_0010, 8, bss as u16, true);
        p.b.symbol("COUNTER", 0x3FC8_0000, 16, bss as u16, false);
        let error = refused(&p);
        assert!(error.contains("no writable data"), "{error}");
        assert!(
            error.contains(".bss: 24 bytes, of them `COUNTER` (16 bytes), `STATE` (8 bytes)"),
            "{error}"
        );
    }

    #[test]
    fn an_undefined_symbol_is_refused() {
        for machine in [EM_RISCV, EM_XTENSA] {
            let mut p = plugin(machine);
            let missing = p.b.symbol("missing", 0, 0, SHN_UNDEF, false);
            p.b.rela(p.text, TEXT + 16, missing, p.k.call);
            p.b.rela(p.text, TEXT + 20, missing, p.k.call);
            let error = refused(&p);
            assert_eq!(
                error,
                "the plugin cannot be packed:\n    undefined symbol `missing` (used at .text+0x10)"
            );
        }
    }

    #[test]
    fn an_address_in_an_instruction_is_refused() {
        for (machine, name) in [
            (EM_RISCV, "R_RISCV_HI20 at .text+0x10 against `DATA`"),
            (EM_XTENSA, "R_XTENSA_SLOT0_ALT at .text+0x10 against `DATA`"),
        ] {
            let mut p = plugin(machine);
            p.b.rela(p.text, TEXT + 16, p.data, p.k.absolute);
            let error = refused(&p);
            assert!(error.contains(name), "{error}");
            assert!(error.contains("cannot move"), "{error}");
        }
        let mut p = plugin(EM_RISCV);
        p.b.rela(p.text, TEXT + 16, p.data, p.k.absolute);
        assert!(refused(&p).contains("-C code-model=medium"));
        // Against the ROM it stays right.
        let mut p = plugin(EM_RISCV);
        p.b.rela(p.text, TEXT + 16, p.rom, p.k.absolute);
        assert!(pack(&p.b.build()).is_ok());
    }

    #[test]
    fn a_relative_reach_out_of_the_image_is_refused() {
        for (machine, name) in [
            (EM_RISCV, "R_RISCV_CALL at .text+0x10 against `rom_fn`"),
            (
                EM_XTENSA,
                "R_XTENSA_SLOT0_OP at .text+0x10 against `rom_fn`",
            ),
        ] {
            let mut p = plugin(machine);
            p.b.rela(p.text, TEXT + 16, p.rom, p.k.call);
            let error = refused(&p);
            assert!(error.contains(name), "{error}");
            assert!(
                error.contains("relative reach out of the plugin"),
                "{error}"
            );
        }
        let mut p = plugin(EM_XTENSA);
        p.b.rela(p.rodata, LINK_BASE + 28, p.rom, p.k.difference);
        let error = refused(&p);
        assert!(
            error.contains("R_XTENSA_DIFF32 at .rodata+0x1c against `rom_fn`: a difference"),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_kind_is_refused_with_its_number() {
        let mut p = plugin(EM_XTENSA);
        p.b.rela(p.text, TEXT + 16, p.data, 99);
        let error = refused(&p);
        assert!(
            error.contains("relocation kind 99 at .text+0x10 against `DATA`: not known"),
            "{error}"
        );
    }

    #[test]
    fn a_word_must_be_aligned_and_point_into_the_image() {
        let mut p = plugin(EM_RISCV);
        p.b.rela(p.rodata, LINK_BASE + 50, p.data, p.k.word);
        let error = refused(&p);
        assert!(error.contains("R_RISCV_32 at .rodata+0x32"), "{error}");
        assert!(error.contains("not 4-aligned"), "{error}");

        // The word at 32 holds 0, an address nowhere in the image.
        let mut p = plugin(EM_XTENSA);
        p.b.rela(p.rodata, LINK_BASE + 32, p.data, p.k.word);
        let error = refused(&p);
        assert!(
            error.contains("the word holds 0x00000000, which points outside the image"),
            "{error}"
        );
    }

    #[test]
    fn a_file_not_linked_as_a_plugin_is_refused() {
        // No relocations kept.
        let mut b = Builder::new(EM_RISCV, ET_EXEC);
        let mut rodata = [0u8; 96];
        rodata[..8].copy_from_slice(&FINGERPRINT.to_le_bytes());
        let ro = b.section(".rodata", SHF_ALLOC, LINK_BASE, &rodata, 16);
        b.section(
            ".text",
            SHF_ALLOC | 4,
            LINK_BASE + CODE_OFFSET + 96,
            &[0; 16],
            4,
        );
        b.symbol(ENTRY_SYMBOL, LINK_BASE, 32, ro as u16, false);
        b.symbol(API_SYMBOL, LINK_BASE + 32, 0, ro as u16, false);
        for (n, name) in TABLE_SYMBOLS.iter().enumerate() {
            b.symbol(name, LINK_BASE + 36 + 4 * n as u32, 0, ro as u16, false);
        }
        let error = pack(&b.build()).unwrap_err();
        assert!(error.contains("--emit-relocs"), "{error}");

        // Code not right after the data.
        let mut b = Builder::new(EM_RISCV, ET_EXEC);
        b.section(".rodata", SHF_ALLOC, LINK_BASE, &rodata, 16);
        b.section(".text", SHF_ALLOC | 4, 0x4200_0000, &[0; 16], 4);
        let error = pack(&b.build()).unwrap_err();
        assert!(error.contains("not right after .rodata"), "{error}");

        // Another section that takes memory.
        let mut p = plugin(EM_RISCV);
        p.b.section(".got", SHF_ALLOC, 0x3C10_0000, &[0; 8], 4);
        let error = refused(&p);
        assert!(
            error.contains("unexpected section .got (8 bytes)"),
            "{error}"
        );

        // An object, not linked.
        let error = pack(&Builder::new(EM_RISCV, ET_REL).build()).unwrap_err();
        assert!(error.contains("an object, not a linked plugin"), "{error}");
    }
}
