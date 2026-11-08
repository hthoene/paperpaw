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
//!   windowed ABI), and an `.xtensa.info` that says `ABI=1`;
//! - a function that does not decode to its symbol's end, and an instruction
//!   of an `op0` this scan does not know (14, 15).
//!
//! The bytes no function covers are the literal pool the linker script puts
//! first in `.text` (`l32r` reads it); they are data, so they are not decoded.
//! What the scan cannot see is how deep a *path* of calls goes (not
//! statically decidable; the stack gate's band and the device's high-water
//! test measure that, `abi/heap-word.md` section 6) and window-overflow
//! spills, which the exception handler writes below `sp`.
//!
//! This is a scan of code LLVM wrote, not a disassembler: it classifies the
//! forms that can name `a1` as a destination and lets every other 24-bit
//! instruction through. A form it mislabels is caught by the corpus test
//! (`tools/sign/src/frames_xtensa/tests.rs`).

use crate::elf::{Elf, Section, Symbol};

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
    /// The function ends inside an instruction.
    Cut { at: usize },
}

/// The length in bytes of the instruction whose first byte is `b0`, or
/// `None` for an `op0` that is reserved (14, 15).
fn length(b0: u8) -> Option<usize> {
    match b0 & 0xF {
        0..=7 => Some(3),
        8..=13 => Some(2),
        _ => None,
    }
}

/// Whether the 24-bit instruction `w` (little endian) is one of the forms
/// that write `a1`. The fields: `op0` bits 3:0, `t` 7:4, `s` 11:8, `r` 15:12,
/// `op1` 19:16, `op2` 23:20. The classes were checked against the encodings
/// LLVM's assembler makes (`tests.rs`, `forms_that_write_a1`).
fn wide_writes_a1(w: u32) -> bool {
    let op0 = w & 0xF;
    let t = (w >> 4) & 0xF;
    let r = (w >> 12) & 0xF;
    let op1 = (w >> 16) & 0xF;
    let op2 = (w >> 20) & 0xF;
    match op0 {
        // L32R: the literal into `at`.
        1 => t == 1,
        // LSAI: the loads (`l8ui` r=0, `l16ui` 1, `l32i` 2, `l16si` 9,
        // `l32ai` 0xB) and `movi` (0xA), `addi` (0xC), `addmi` (0xD) write
        // `at`; the stores and the cache forms do not.
        2 => matches!(r, 0 | 1 | 2 | 9 | 0xA | 0xB | 0xC | 0xD) && t == 1,
        0 => match (op1, op2) {
            // RST0 `st0`: `movsp at, as` (r = 1; any of its operands may be
            // `a1`, which is refused too) and `rsil at`.
            (0, 0) => r == 1 || (r == 6 && t == 1),
            // `and`, `or`, `xor`, `neg`/`abs` (6), `add`, `addx*`, `sub`,
            // `subx*`: `ar`.
            (0, 1..=3 | 6 | 8..=15) => r == 1,
            // `st1`: `rer`, `nsa`, `nsau` write `at`; `ssr`, `ssl`, `ssa8l`,
            // `ssa8b`, `ssai`, `wer`, `rotw` do not.
            (0, 4) => t == 1 && matches!(r, 6 | 0xE | 0xF),
            // RST1: the shifts, `src`, `mul16*`: `ar`; `xsr` (6): `at`.
            (1, 6) => t == 1,
            (1, 15) => false,
            (1, _) => r == 1,
            // RST2: `mull`, `mulsh`, `muluh`, `quou`, `quos`, `remu`, `rems`:
            // `ar`.
            (2, _) => r == 1,
            // RST3: `rsr` (0) writes `at`; `wsr` (1) and `wur` (15) none;
            // `sext`, `clamps`, `min*`/`max*`, `mov*z`/`mov*`, `rur`: `ar`.
            (3, 0) => t == 1,
            (3, 1 | 15) => false,
            (3, _) => r == 1,
            // `extui`.
            (4 | 5, _) => r == 1,
            _ => false,
        },
        _ => false,
    }
}

/// Whether the 16-bit instruction `w` (little endian) writes `a1`.
fn narrow_writes_a1(w: u32) -> bool {
    let op0 = w & 0xF;
    let t = (w >> 4) & 0xF;
    let s = (w >> 8) & 0xF;
    let r = (w >> 12) & 0xF;
    match op0 {
        // L32I.N writes `at`.
        8 => t == 1,
        // ADD.N and ADDI.N write `ar`.
        0xA | 0xB => r == 1,
        // ST2: MOVI.N (bit 7 clear) writes `as`; BEQZ.N and BNEZ.N do not.
        0xC => (t & 8) == 0 && s == 1,
        // ST3: MOV.N (r = 0) writes `at`; the rest (`ret.n`, `retw.n`, `break.n`,
        // `nop.n`, `ill.n`) write no register.
        0xD => r == 0 && t == 1,
        _ => false,
    }
}

/// The frame `entry` takes, if the 24-bit instruction `w` is an `entry`
/// (`36 s1 ii`: op0 = 6, n = 3, m = 0; `imm12` is bits 23:12).
fn entry_frame(w: u32) -> Option<u32> {
    (w & 0xFF == 0x36).then(|| ((w >> 12) << 3) + BASE_SAVE)
}

/// Whether `w` is `call0` (op0 = 5, n = 0) or `callx0` (RST0, `st0`, `snm0`
/// with `m = 3`, `n = 0`: `t = 0xC`).
fn is_call0(w: u32) -> bool {
    let (op0, t) = (w & 0xF, (w >> 4) & 0xF);
    let (r, op1, op2) = ((w >> 12) & 0xF, (w >> 16) & 0xF, (w >> 20) & 0xF);
    (op0 == 5 && t & 3 == 0) || (op0 == 0 && op1 == 0 && op2 == 0 && r == 0 && t == 0xC)
}

/// Scans the function at `code[start..end]`.
fn function(code: &[u8], start: usize, end: usize) -> Option<Finding> {
    let mut at = start;
    while at < end {
        let Some(len) = length(code[at]) else {
            return Some(Finding::Unknown { at });
        };
        if at + len > end {
            return Some(Finding::Cut { at });
        }
        if len == 3 {
            let w =
                u32::from(code[at]) | u32::from(code[at + 1]) << 8 | u32::from(code[at + 2]) << 16;
            if let Some(size) = entry_frame(w) {
                if size >= LIMIT {
                    return Some(Finding::Frame { at, size });
                }
            } else if is_call0(w) {
                return Some(Finding::Call0 { at });
            } else if wide_writes_a1(w) {
                return Some(Finding::WritesSp { at });
            }
        } else {
            let w = u32::from(code[at]) | u32::from(code[at + 1]) << 8;
            if narrow_writes_a1(w) {
                return Some(Finding::WritesSp { at });
            }
        }
        at += len;
    }
    None
}

/// The first finding of the functions in `ranges` (each `(start, end)` in
/// `code`, sorted, not overlapping).
pub fn scan_ranges(code: &[u8], ranges: &[(usize, usize)]) -> Option<Finding> {
    ranges
        .iter()
        .find_map(|&(start, end)| function(code, start, end.min(code.len())))
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
    match scan_ranges(code, &ranges) {
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
    }
}

#[cfg(test)]
mod tests;
