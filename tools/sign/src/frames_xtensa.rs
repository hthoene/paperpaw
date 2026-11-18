//! The pack-time scan of a target-9 plugin's code (Xtensa, windowed ABI,
//! `esp32s3-xtensa-lx7-ilp32-windowed-le`): the S3 twin of [`crate::frames`],
//! for the stack-frame rule of `abi/ppn5.md` ("Target 9").
//!
//! In the windowed ABI a function takes its frame in **one** instruction,
//! `entry aN, imm12`, whose operand is the frame in 8-byte steps (`imm12 << 3`,
//! 8 to 32,760 bytes). The scan therefore reads each function within its
//! `STT_FUNC` symbol's size, instruction by instruction (the length of an
//! Xtensa instruction is in the low nibble of its first byte: the density
//! option's 16-bit forms are `op0` 8 to 13, every other `op0` below 14 is 24
//! bits), and refuses:
//!
//! - an `entry` whose frame, with the 16 bytes of the windowed ABI's base save
//!   area ([`BASE_SAVE`]), is [`LIMIT`] bytes or more (the same 2,048 as the
//!   X4's, `FRAME_LIMIT`);
//! - anything that writes `a1` (the stack pointer) other than `entry`: a
//!   `movsp`, `addi`/`addmi`/`add`/`sub`/... with `a1` as the destination,
//!   a load or move into it. LLVM makes no other change to `sp` in a plugin,
//!   and a computed one cannot be bounded here (as `frames.rs` refuses a
//!   change it cannot bound);
//! - a `call0` or `callx0` (a `call0` ABI object: the S3 profile is the
//!   windowed ABI), a `call4` or `callx4` (only `call8` and `call12` are
//!   allowed, Kernel ruling), and an `.xtensa.info` that says `ABI=1`;
//! - a function that does not decode to its symbol's end, and an instruction
//!   of an `op0` this scan does not know (14, 15);
//! - **default deny**: every instruction family is named, as leaving `a1`
//!   alone, writing it, or unknown; an unknown form (a coprocessor or vector
//!   `ee.*` form, `rotw`, `wsr`, `xsr`, `rsil`, `waiti`, `l32e`, `s32e`, the
//!   `rf*` returns, the privileged ones, ...) is
//!   refused;
//! - a function symbol of size 0, and code bytes that no sized function
//!   symbol covers, unless they are padding (zeros, `nop`) or a word an
//!   `l32r` in a scanned function reads (the `.literal` sections the linker
//!   script places among the functions').
//!
//! The bytes between functions are the literal pools (`l32r` reads them);
//! they are data, so they are not decoded, but each word must be one an
//! `l32r` reads: code hidden there would not be scanned, and the scan is a
//! guard against what a compiler makes, not a sandbox. The one exception is
//! a dead word (the toolchain's leftover constant) that nothing in the image
//! can reach, in a literal region, up to [`dead::MAX_BYTES`] bytes: the packer
//! warns of each ([`dead`]).
//! What the scan cannot see is how deep a *path* of calls goes (not
//! statically decidable; the stack gate's band and the device's high-water
//! test measure that, `abi/heap-word.md` section 6) and window-overflow
//! spills, which the exception handler writes below `sp`.
//!
//! This is a scan of code LLVM wrote, not a disassembler: it classifies the
//! forms that can name `a1` as a destination and lets every other 24-bit
//! instruction through. A form it mislabels is caught by the corpus test (run by hand)
//! (`tools/sign/src/frames_xtensa/tests.rs`).

mod dead;

use crate::elf::{Elf, SHT_NOBITS, SHT_RELA, Section, Symbol};
pub use dead::Reach;

/// `STT_FUNC`: a symbol that names a function.
const STT_FUNC: u8 = 2;

/// The frame size from which a plugin is refused, as on the X4.
pub const LIMIT: u32 = 2048;
/// The base save area the windowed ABI keeps below the frame's caller (the
/// spilled `a0` to `a3` of the caller's window, 16 bytes): counted in the
/// frame (`abi/entry.md`, "The Xtensa profile").
pub const BASE_SAVE: u32 = 16;

/// What the scan found wrong.
#[derive(Debug, PartialEq, Eq)]
pub enum Finding {
    /// `entry` with a frame of `size` bytes in the function at `at`.
    Frame { at: usize, size: u32 },
    /// An instruction that writes the stack pointer, other than `entry`.
    WritesSp { at: usize },
    /// `call0` or `callx0`: not the windowed ABI.
    Call0 { at: usize },
    /// An instruction the scan does not know how long it is.
    Unknown { at: usize },
    /// An instruction form the scan does not know to leave the stack pointer
    /// alone (default deny).
    Form { at: usize },
    /// Code bytes that no sized function symbol covers and no `l32r` reads
    /// as a literal.
    Uncovered { at: usize },
    /// The function ends inside an instruction.
    Cut { at: usize },
}

/// The length in bytes of the instruction whose first byte is `b0`, or
/// `None` for an `op0` that is reserved (14, 15).
pub(crate) fn length(b0: u8) -> Option<usize> {
    match b0 & 0xF {
        0..=7 => Some(3),
        8..=13 => Some(2),
        _ => None,
    }
}

/// What an instruction does to the stack pointer, `a1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    /// Known to leave `a1` alone (it may read it).
    Safe,
    /// Writes `a1` (or changes what `a1` names).
    Writes,
    /// A form this scan does not know to be safe: refused (default deny).
    Unknown,
}

/// `Writes` if `reg` is `a1`, else `Safe`: the form's destination is `reg`.
fn dest(reg: u32) -> Class {
    if reg == 1 { Class::Writes } else { Class::Safe }
}

/// The class of the 24-bit instruction `w` (little endian). The fields:
/// `op0` bits 3:0, `t` 7:4, `s` 11:8, `r` 15:12, `op1` 19:16, `op2` 23:20.
/// Every family is named; a form that is not is `Unknown`. The encodings
/// are the ones LLVM's assembler makes (`tests.rs`, `forms.rs`) and the
/// Xtensa ISA's opcode maps.
pub(crate) fn wide_class(w: u32) -> Class {
    let op0 = w & 0xF;
    let t = (w >> 4) & 0xF;
    let s = (w >> 8) & 0xF;
    let r = (w >> 12) & 0xF;
    let op1 = (w >> 16) & 0xF;
    let op2 = (w >> 20) & 0xF;
    match op0 {
        0 => qrst(op1, op2, r, s, t),
        // L32R: the literal into `at`.
        1 => dest(t),
        // LSAI: the loads (`l8ui` 0, `l16ui` 1, `l32i` 2, `l16si` 9,
        // `l32ai` 11), `movi` 10, `addi` 12, `addmi` 13 and `s32c1i` 14
        // write `at`; the stores (4, 5, 6), the cache forms (7) and `s32ri`
        // (15) write no `a` register. 3 and 8 are reserved.
        2 => match r {
            0 | 1 | 2 | 9..=14 => dest(t),
            4..=7 | 15 => Class::Safe,
            _ => Class::Unknown,
        },
        // LSCI: `lsi` 0, `ssi` 4; `lsip` 8 and `ssip` 12 update the base `as`.
        3 => match r {
            0 | 4 => Class::Safe,
            8 | 12 => dest(s),
            _ => Class::Unknown,
        },
        // MAC16 (`mula.*` and its loads update an address register): not
        // used by LLVM for a plugin.
        4 => Class::Unknown,
        // CALLN: `call8`, `call12` (`call0` is refused before this; `call4`
        // (n = 1) is refused too).
        5 if t & 3 == 1 => Class::Unknown,
        // CALLN, SI (`j`, `beqz`..., `loop`; `entry` is read before this)
        // and B (`beq`, `bbci`...): they write no `a` register.
        5..=7 => Class::Safe,
        _ => Class::Unknown,
    }
}

/// QRST, `op0` 0, by `op1` (`RST0`, `RST1`, `RST2`, `RST3`, `EXTUI`, ...).
fn qrst(op1: u32, op2: u32, r: u32, s: u32, t: u32) -> Class {
    match (op1, op2) {
        // RST0 `st0`: `snm0` (r = 0: `ill`, `ret`, `retw`, `jx`, `callx*`
        // read `as` only), `movsp` (r = 1: refused whatever its operands),
        // `sync` (r = 2), `break` (r = 4) and `rsil at` (r = 6, which also
        // sets `ps`).
        (0, 0) => match r {
            // `callx4` (m = 3, n = 1) is refused with `call4`: only call8 and
            // call12 keep the caller's frame clear of the band's accounting.
            0 => match (t >> 2, t & 3) {
                (0, 0) | (2, _) | (3, 2 | 3) => Class::Safe,
                _ => Class::Unknown,
            },
            1 => Class::Writes,
            2 | 4 => Class::Safe,
            6 if t == 1 => Class::Writes,
            _ => Class::Unknown,
        },
        // `and`, `or`, `xor`: `ar`.
        (0, 1..=3) => dest(r),
        // `st1`: `ssr`, `ssl`, `ssa8l`, `ssa8b`, `ssai` (0..=4) and `wer`
        // (7) write no `a` register; `rer` (6), `nsa` (14), `nsau` (15)
        // write `at`. `rotw` (8) changes what `a1` names.
        (0, 4) => match r {
            0..=4 | 7 => Class::Safe,
            6 | 14 | 15 => dest(t),
            _ => Class::Unknown,
        },
        // `neg` (s = 0), `abs` (s = 1): `ar`.
        (0, 6) if s <= 1 => dest(r),
        // `add`, `addx*`, `sub`, `subx*`: `ar`.
        (0, 8..=15) => dest(r),
        // RST1: `slli` (0, 1), `srai` (2, 3), `srli` (4), `src` (8), `srl`
        // (9), `sll` (10), `sra` (11), `mul16u` (12), `mul16s` (13): `ar`.
        (1, 0..=4 | 8..=13) => dest(r),
        // `xsr` (6) writes a special register: refused (Kernel ruling,
        // decision 142: `rsr`, `rur`, `wur` are allowed, `wsr` and `xsr` not).
        // RST2: the boolean operations (0..=4) write a boolean register;
        // `mull`, `muluh`, `mulsh`, `quou`, `quos`, `remu`, `rems`: `ar`.
        (2, 0..=4) => Class::Safe,
        (2, 8 | 10..=15) => dest(r),
        // RST3: `rsr` reads any special register into `at`; `wsr` writes
        // one; `sext`, `clamps`, `min*`, `max*`, `mov*z`, `mov*` (2..=13)
        // and `rur` (14): `ar`; `wur` (15) writes a user register.
        (3, 0) => dest(t),
        // `wsr` (1) is refused.
        (3, 2..=14) => dest(r),
        (3, 15) => Class::Safe,
        // `extui`.
        (4 | 5, _) => dest(r),
        // LSCX: `lsx` (0), `ssx` (4); `lsxp` (1), `ssxp` (5) update `as`.
        (8, 0 | 4) => Class::Safe,
        (8, 1 | 5) => dest(s),
        // FP0: `add.s`, `sub.s`, `mul.s`, `madd.s`, `msub.s`, `maddn.s`,
        // `divn.s` (0, 1, 2, 4..=7) and `float.s`, `ufloat.s` (12, 13) write
        // a floating-point register; `round.s` (8), `trunc.s` (9),
        // `floor.s` (10), `ceil.s` (11), `utrunc.s` (14) write `ar`; FP1OP
        // (15): `rfr` (t = 4) writes `ar`, every other (`mov.s`, `abs.s`,
        // `const.s`, `wfr`, `neg.s`, `div0.s`, `recip0.s`, `sqrt0.s`, ...)
        // a floating-point register; t = 2 is reserved.
        (10, 0..=2 | 4..=7 | 12 | 13) => Class::Safe,
        (10, 8..=11 | 14) => dest(r),
        (10, 15) => match t {
            4 => dest(r),
            2 => Class::Unknown,
            _ => Class::Safe,
        },
        // FP1: the compares (1..=7) write a boolean register; the
        // conditional moves (8..=13) a floating-point register.
        (11, 1..=13) => Class::Safe,
        // CUST0, CUST1 (the S3's vector `ee.*` forms), LSC4 (`l32e`,
        // `s32e`), the reserved and the privileged: not in a plugin.
        _ => Class::Unknown,
    }
}

/// The class of the 16-bit instruction `w` (little endian).
pub(crate) fn narrow_class(w: u32) -> Class {
    let op0 = w & 0xF;
    let t = (w >> 4) & 0xF;
    let s = (w >> 8) & 0xF;
    let r = (w >> 12) & 0xF;
    match op0 {
        // L32I.N writes `at`; S32I.N writes no `a` register.
        8 => dest(t),
        9 => Class::Safe,
        // ADD.N and ADDI.N write `ar`.
        0xA | 0xB => dest(r),
        // ST2: MOVI.N (bit 7 of `t` clear) writes `as`; BEQZ.N and BNEZ.N
        // do not.
        0xC => {
            if t & 8 == 0 {
                dest(s)
            } else {
                Class::Safe
            }
        }
        // ST3: MOV.N (r = 0) writes `at`; r = 15: `ret.n`, `retw.n`,
        // `break.n`, `nop.n`, `ill.n`.
        0xD => match (r, t) {
            (0, _) => dest(t),
            (15, 0..=3 | 6) => Class::Safe,
            _ => Class::Unknown,
        },
        _ => Class::Unknown,
    }
}

/// The frame `entry` takes, if the 24-bit instruction `w` is an `entry`
/// (`36 s1 ii`: op0 = 6, n = 3, m = 0; `imm12` is bits 23:12). Whether its
/// register is `a1` is checked by the caller.
pub(crate) fn entry_frame(w: u32) -> Option<u32> {
    (w & 0xFF == 0x36).then(|| ((w >> 12) << 3) + BASE_SAVE)
}

/// Whether `w` is `call0` (op0 = 5, n = 0) or `callx0` (RST0, `st0`, `snm0`
/// with `m = 3`, `n = 0`: `t = 0xC`).
pub(crate) fn is_call0(w: u32) -> bool {
    let (op0, t) = (w & 0xF, (w >> 4) & 0xF);
    let (r, op1, op2) = ((w >> 12) & 0xF, (w >> 16) & 0xF, (w >> 20) & 0xF);
    (op0 == 5 && t & 3 == 0) || (op0 == 0 && op1 == 0 && op2 == 0 && r == 0 && t == 0xC)
}

/// The offset of the literal an `l32r` at `at` reads (`imm16` is a word
/// offset below the instruction, with the high bits set; the base is the
/// next word boundary after the instruction).
fn literal_of(at: usize, w: u32) -> Option<usize> {
    let back = i64::from(0x1_0000 - ((w >> 8) & 0xFFFF)) * 4;
    usize::try_from((((at + 3) & !3) as i64) - back).ok()
}

/// Scans the function at `code[start..end]`, noting the literals its `l32r`
/// read in `literals`.
fn function(
    code: &[u8],
    start: usize,
    end: usize,
    literals: &mut Vec<usize>,
    targets: &mut Vec<usize>,
) -> Option<Finding> {
    let mut at = start;
    while at < end {
        let Some(len) = length(code[at]) else {
            return Some(Finding::Unknown { at });
        };
        if at + len > end {
            return Some(Finding::Cut { at });
        }
        let class = if len == 3 {
            let w =
                u32::from(code[at]) | u32::from(code[at + 1]) << 8 | u32::from(code[at + 2]) << 16;
            if let Some(size) = entry_frame(w) {
                // `entry a1, ...` only: the frame is taken from the stack pointer.
                if (w >> 8) & 0xF != 1 {
                    return Some(Finding::Form { at });
                }
                if size >= LIMIT {
                    return Some(Finding::Frame { at, size });
                }
                Class::Safe
            } else if is_call0(w) {
                return Some(Finding::Call0 { at });
            } else {
                if w & 0xF == 1 {
                    literals.extend(literal_of(at, w));
                }
                targets.extend(dead::wide_target(at, w).and_then(|t| usize::try_from(t).ok()));
                wide_class(w)
            }
        } else {
            let w = u32::from(code[at]) | u32::from(code[at + 1]) << 8;
            targets.extend(dead::narrow_target(at, w).and_then(|t| usize::try_from(t).ok()));
            narrow_class(w)
        };
        match class {
            Class::Safe => {}
            Class::Writes => return Some(Finding::WritesSp { at }),
            Class::Unknown => return Some(Finding::Form { at }),
        }
        at += len;
    }
    None
}

/// The first byte of `code` outside every range that is not padding (zero
/// bytes, `nop`, `nop.n`), not a word an `l32r` reads (`literals`: their
/// offsets, sorted) (a literal: the linker script puts the `.literal`
/// sections among the functions') and not a dead word ([`dead`]: only with
/// `reach`; `targets`, sorted, are the places the scanned code goes to; the
/// dead words are noted in `dead`).
fn uncovered(
    code: &[u8],
    ranges: &[(usize, usize)],
    literals: &[usize],
    targets: &[usize],
    reach: Option<&Reach>,
    dead: &mut Vec<usize>,
) -> Option<usize> {
    let mut at = 0usize;
    for (n, &(start, end)) in ranges.iter().chain([&(code.len(), code.len())]).enumerate() {
        // A gap before a function can hold literals; the last one, after
        // them all, cannot.
        let before_function = n < ranges.len();
        let lower = at;
        while at < start.min(code.len()) {
            let rest = &code[at..start.min(code.len())];
            // `literals` is sorted: the last literal at or before `at`.
            let nearest = literals
                .partition_point(|&l| l <= at)
                .checked_sub(1)
                .map(|n| literals[n]);
            if let Some(word) = nearest.filter(|&l| at < l + 4) {
                at = word + 4;
            } else if rest[0] == 0 {
                at += 1;
            } else if rest.starts_with(&[0xf0, 0x20, 0x00]) {
                at += 3;
            } else if rest.starts_with(&[0x3d, 0xf0]) {
                at += 2;
            } else {
                // A whole aligned word in this gap that nothing reaches.
                let word = at & !3;
                let fits = before_function
                    && word >= lower
                    && (word > lower || n == 0 || dead::ends_control(code, lower))
                    && word + 4 <= start.min(code.len())
                    && (dead.len() + 1) * 4 <= dead::MAX_BYTES;
                let unreached = reach.is_some_and(|r| {
                    !dead::hits(&r.pointed, word)
                        && !dead::hits(targets, word)
                        && !dead::looks_like_control([
                            code[word],
                            code[word + 1],
                            code[word + 2],
                            code[word + 3],
                        ])
                });
                if !(fits && unreached) {
                    return Some(at);
                }
                dead.push(word);
                at = word + 4;
            }
        }
        at = at.max(end);
    }
    None
}

/// The first finding of the functions in `ranges` (each `(start, end)` in
/// `code`, sorted, not overlapping), and then of the bytes between them.
/// A word nothing reads is a finding; see [`scan_ranges_in`] for the one
/// that may be let through.
#[cfg(test)]
pub fn scan_ranges(code: &[u8], ranges: &[(usize, usize)]) -> Option<Finding> {
    scan_ranges_in(code, ranges, None).0
}

/// `scan_ranges` (tests only), and with `reach` (what the rest of the image points at)
/// a dead word ([`dead`]) is let through; the second value is the `.text`
/// offsets of those words.
pub fn scan_ranges_in(
    code: &[u8],
    ranges: &[(usize, usize)],
    reach: Option<&Reach>,
) -> (Option<Finding>, Vec<usize>) {
    let mut literals = Vec::new();
    let mut targets = Vec::new();
    let found = ranges.iter().find_map(|&(start, end)| {
        function(
            code,
            start,
            end.min(code.len()),
            &mut literals,
            &mut targets,
        )
    });
    literals.sort_unstable();
    literals.dedup();
    targets.sort_unstable();
    let mut words = Vec::new();
    let found = found.or_else(|| {
        uncovered(code, ranges, &literals, &targets, reach, &mut words)
            .map(|at| Finding::Uncovered { at })
    });
    (found, words)
}

/// What the image points at inside `.text` (offsets, sorted): the places
/// relocations resolve to (`S + A`) and the words (aligned, in every
/// section that is loaded and has bytes, `.text` apart from its functions)
/// that hold an address in `.text`.
fn reach(
    elf: &Elf<'_>,
    text: &Section,
    symbols: &[Symbol],
    ranges: &[(usize, usize)],
) -> Result<Reach, String> {
    let len = text.size as usize;
    let mut pointed = Vec::new();
    let mut note = |address: u32| {
        if let Some(at) = address
            .checked_sub(text.addr)
            .map(|a| a as usize)
            .filter(|&a| a < len)
        {
            pointed.push(at);
        }
    };
    for section in &elf.sections {
        if section.kind == SHT_RELA {
            // Only the relocations of loaded sections: the debug sections'
            // addresses (`.text + n`: a line or a range) are not references.
            // One that names no section counts (fails closed).
            if elf
                .sections
                .get(section.info as usize)
                .is_some_and(|s| !s.is_alloc())
            {
                continue;
            }
            for rela in elf.relas(section)? {
                if let Some(symbol) = symbols.get(rela.symbol as usize) {
                    note(symbol.value.wrapping_add(rela.addend as u32));
                }
            }
        } else if section.is_alloc() && section.kind != SHT_NOBITS && section.size >= 4 {
            let data = elf.data(section)?;
            let first = (section.addr.wrapping_neg() & 3) as usize;
            let in_text = section.addr == text.addr;
            for at in (first..data.len().saturating_sub(3)).step_by(4) {
                if in_text && ranges.iter().any(|&(s, e)| s < at + 4 && at < e) {
                    continue;
                }
                note(crate::elf::le32(data, at));
            }
        }
    }
    pointed.sort_unstable();
    Ok(Reach { pointed })
}

/// The start of the first function symbol of size 0 in `sized`
/// (`(start, size)` pairs): its code cannot be bounded.
fn unsized_function(sized: &[(usize, usize)]) -> Option<usize> {
    sized
        .iter()
        .find(|&&(_, size)| size == 0)
        .map(|&(at, _)| at)
}

/// Refuses a plugin whose `.text` has a function with a frame of [`LIMIT`]
/// bytes or more, a change of `sp` other than `entry`, a `call0`, or whose
/// `.xtensa.info` says it was built for the call0 ABI.
pub fn check(
    elf: &Elf<'_>,
    text: &Section,
    text_index: usize,
    symbols: &[Symbol],
) -> Result<(), String> {
    if let Some((_, info)) = elf.section(".xtensa.info") {
        let data = elf.data(info)?;
        if data.windows(5).any(|w| w == b"ABI=1") {
            return Err(
                "its .xtensa.info says ABI=1: a call0 object; the S3 profile is the windowed ABI \
                 (build with the `esp` toolchain's xtensa-esp32s3-none-elf target)"
                    .into(),
            );
        }
    }
    let code = elf.data(text)?;
    let sized: Vec<(usize, usize)> = symbols
        .iter()
        .filter(|s| s.kind() == STT_FUNC && s.shndx as usize == text_index)
        .map(|s| (s.value.wrapping_sub(text.addr) as usize, s.size as usize))
        .collect();
    if !text.addr.is_multiple_of(4) {
        return Err("its .text does not start on a word boundary".into());
    }
    if let Some(at) = unsized_function(&sized) {
        let name = symbols
            .iter()
            .find(|s| {
                s.kind() == STT_FUNC
                    && s.shndx as usize == text_index
                    && s.size == 0
                    && s.value.wrapping_sub(text.addr) as usize == at
            })
            .map_or_else(|| "?".into(), |s| crate::frames::path(&s.name));
        return Err(format!(
            "the function symbol `{name}` has size 0 (.text+{at:#x}): its code cannot be \
             bounded; give it a size (`.size`)"
        ));
    }
    let ranges = crate::frames::function_ranges(&sized, code.len());
    let name_at = |at: usize| {
        symbols
            .iter()
            .filter(|s| s.kind() == STT_FUNC && s.shndx as usize == text_index)
            .filter(|s| {
                let from = s.value.wrapping_sub(text.addr) as usize;
                from <= at && at < from + s.size as usize
            })
            .map(|s| crate::frames::path(&s.name))
            .next()
            .unwrap_or_else(|| "?".into())
    };
    let reach = reach(elf, text, symbols, &ranges)?;
    let (found, dead) = scan_ranges_in(code, &ranges, Some(&reach));
    for &at in &dead {
        eprintln!(
            "warning: .text+{at:#x} ({:#x}) is a word nothing reads, points to or jumps to, in a \
             literal pool: accepted as data (the toolchain's dead literal, value {:#x})",
            text.addr as usize + at,
            crate::elf::le32(code, at)
        );
    }
    match found {
        None => Ok(()),
        Some(Finding::Frame { at, size }) => Err(format!(
            "`{}` takes a frame of {size} bytes (`entry` at .text+{at:#x}); a plugin's frames stay \
             under {LIMIT}: keep large buffers on the heap or in the arguments",
            name_at(at)
        )),
        Some(Finding::WritesSp { at }) => Err(format!(
            "`{}` changes the stack pointer other than by `entry` (.text+{at:#x}): a computed frame \
             cannot be bounded",
            name_at(at)
        )),
        Some(Finding::Call0 { at }) => Err(format!(
            "`{}` calls with call0 or callx0 (.text+{at:#x}): the S3 profile is the windowed ABI \
             (call4, call8, call12)",
            name_at(at)
        )),
        Some(Finding::Unknown { at }) => Err(format!(
            "`{}` has an instruction the scan cannot read (.text+{at:#x})",
            name_at(at)
        )),
        Some(Finding::Cut { at }) => Err(format!(
            "`{}` ends inside an instruction (.text+{at:#x})",
            name_at(at)
        )),
        Some(Finding::Form { at }) => Err(format!(
            "`{}` has an instruction (.text+{at:#x}) that this scan does not know to leave the \
             stack pointer alone: a plugin may use only the forms LLVM makes for ordinary code",
            name_at(at)
        )),
        Some(Finding::Uncovered { at }) => Err(format!(
            ".text+{at:#x} is code that no sized function symbol covers and no `l32r` reads as a \
             literal: do not strip the symbols' sizes or hide code between functions"
        )),
    }
}

#[cfg(test)]
mod tests;
