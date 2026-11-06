//! The pack-time scan for large stack frames in a plugin's code.
//!
//! A plugin runs on the firmware's one stack, and the X4 watches the stack's
//! end with a band the CPU's data trigger fires in (`shells/xteink/src/
//! stack.rs`, `paperpaw_x4::stack_band`). A frame bigger than the band that
//! never stores into it steps over the band unseen. So the packer reads the
//! code for what makes a frame, a change of `sp` in a function's prologue,
//! and refuses a plugin with one of [`LIMIT`] bytes or more, or one it cannot
//! size (a dynamic `alloca`).
//!
//! The code is RISC-V (RV32IMC): 32-bit and 16-bit (compressed)
//! instructions, told apart by their two lowest bits. The scan walks the
//! `.text` once, forwards, and knows what a register holds only where the
//! value is built next to the use (`lui`/`li`/`addi`, the compressed forms,
//! `mv`): a frame beyond `addi`'s 2 KiB is `lui t0, hi; addi t0, t0, lo;
//! add sp, sp, t0` (or `sub`). What a register holds is forgotten at every
//! function's start and at every jump, branch and call, so a size set before
//! one is not trusted after it: it is refused as unknown, the safe way.
//!
//! Matched: `addi sp, sp, -N` and `c.addi16sp` (which reaches 512 bytes only
//! but is parsed all the same), `add sp, sp, rX` with a negative value and
//! `sub sp, sp, rX` with a positive one (known), and the two with a value not
//! known (refused). `addi sp, s0, -N` and the like restore `sp` and are not
//! frames.

use crate::elf::{Elf, Section, Symbol};

/// A frame this large (in bytes) or larger is refused: 1.5 KiB, under the
/// band's 2 KiB with room for the rest of the frames of the call.
pub const LIMIT: u32 = 1536;

/// `STT_FUNC`: a symbol that names a function.
const STT_FUNC: u8 = 2;

/// What the scan found.
#[derive(Debug, PartialEq, Eq)]
pub enum Finding {
    /// A frame of `bytes`, at `at` in the code.
    Large { at: usize, bytes: u32 },
    /// A change of `sp` by a register whose value is not known at `at`.
    Unknown { at: usize },
}

/// What a register holds, as far as the scan knows.
type Regs = [Option<i32>; 32];

fn forgotten() -> Regs {
    let mut regs: Regs = [None; 32];
    regs[0] = Some(0);
    regs
}

/// The first frame in `code` (a function's start is in `starts`, offsets into
/// the code) that is [`LIMIT`] bytes or larger, or whose size cannot be told.
pub fn scan(code: &[u8], starts: &[usize]) -> Option<Finding> {
    let mut regs = forgotten();
    let mut at = 0usize;
    while at + 2 <= code.len() {
        if starts.binary_search(&at).is_ok() {
            regs = forgotten();
        }
        let low = u16::from_le_bytes([code[at], code[at + 1]]);
        if low & 3 == 3 {
            if at + 4 > code.len() {
                break;
            }
            let w = u32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]);
            if let Some(found) = wide(w, at, &mut regs) {
                return Some(found);
            }
            at += 4;
        } else {
            if let Some(found) = narrow(low, at, &mut regs) {
                return Some(found);
            }
            at += 2;
        }
    }
    None
}

/// `sp`'s change by `delta` (bytes, negative: it grows the frame).
fn adjust(delta: i64, at: usize) -> Option<Finding> {
    let bytes = delta.checked_neg()?;
    (bytes >= i64::from(LIMIT)).then_some(Finding::Large {
        at,
        bytes: u32::try_from(bytes).unwrap_or(u32::MAX),
    })
}

fn set(regs: &mut Regs, rd: usize, value: Option<i32>) {
    if rd != 0 {
        regs[rd] = value;
    }
}

/// `sp = sp ± reg`, the register's value as `regs` has it.
fn by_register(regs: &Regs, rs2: usize, negate: bool, at: usize) -> Option<Finding> {
    match regs[rs2] {
        None => Some(Finding::Unknown { at }),
        Some(v) => {
            let delta = if negate { -i64::from(v) } else { i64::from(v) };
            adjust(delta, at)
        }
    }
}

/// A 32-bit instruction.
fn wide(w: u32, at: usize, regs: &mut Regs) -> Option<Finding> {
    let rd = (w >> 7 & 31) as usize;
    let rs1 = (w >> 15 & 31) as usize;
    let rs2 = (w >> 20 & 31) as usize;
    let funct3 = w >> 12 & 7;
    let imm_i = (w as i32) >> 20;
    match w & 0x7f {
        // lui
        0x37 => set(regs, rd, Some((w & 0xffff_f000) as i32)),
        // addi and the other immediate operations
        0x13 => {
            if funct3 == 0 {
                if rd == 2 && rs1 == 2 {
                    set(regs, 2, None);
                    return adjust(i64::from(imm_i), at);
                }
                let value = regs[rs1].map(|v| v.wrapping_add(imm_i));
                set(regs, rd, value);
            } else {
                set(regs, rd, None);
            }
        }
        // add, sub and the other register operations
        0x33 => {
            let add = funct3 == 0 && w >> 25 == 0;
            let sub = funct3 == 0 && w >> 25 == 0x20;
            if rd == 2 && rs1 == 2 && (add || sub) {
                let found = by_register(regs, rs2, sub, at);
                set(regs, 2, None);
                return found;
            }
            let value = match (add, sub, regs[rs1], regs[rs2]) {
                (true, _, Some(a), Some(b)) => Some(a.wrapping_add(b)),
                (_, true, Some(a), Some(b)) => Some(a.wrapping_sub(b)),
                _ => None,
            };
            set(regs, rd, value);
        }
        // jal, jalr, the branches: what is known is not carried over
        0x6f | 0x67 | 0x63 => *regs = forgotten(),
        // loads, auipc, atomics, system, floating-point to a register
        0x03 | 0x17 | 0x2f | 0x73 | 0x53 | 0x07 => set(regs, rd, None),
        _ => {}
    }
    None
}

/// A 16-bit (compressed) instruction.
fn narrow(w: u16, at: usize, regs: &mut Regs) -> Option<Finding> {
    let w = u32::from(w);
    let funct3 = w >> 13;
    let rd = (w >> 7 & 31) as usize;
    let rs2 = (w >> 2 & 31) as usize;
    // The 6-bit immediate of c.addi, c.li, c.lui and c.andi.
    let imm6 = (((w >> 12 & 1) << 5 | (w >> 2 & 31)) as i32) << 26 >> 26;
    // The three-bit register fields: x8 to x15.
    let prime = (w >> 7 & 7) as usize + 8;
    match (w & 3, funct3) {
        // c.addi4spn, c.lw: a register of the three-bit field
        (0, 0 | 2) => set(regs, (w >> 2 & 7) as usize + 8, None),
        // c.addi, and c.nop (rd 0)
        (1, 0) if rd == 2 => {
            set(regs, 2, None);
            return adjust(i64::from(imm6), at);
        }
        (1, 0) => {
            let value = regs[rd].map(|v| v.wrapping_add(imm6));
            set(regs, rd, value);
        }
        // c.jal
        (1, 1) => *regs = forgotten(),
        // c.li
        (1, 2) => set(regs, rd, Some(imm6)),
        // c.addi16sp (rd 2), c.lui
        (1, 3) if rd == 2 => {
            let imm = (w >> 12 & 1) << 9
                | (w >> 3 & 3) << 7
                | (w >> 5 & 1) << 6
                | (w >> 2 & 1) << 5
                | (w >> 6 & 1) << 4;
            let imm = (imm as i32) << 22 >> 22;
            set(regs, 2, None);
            return adjust(i64::from(imm), at);
        }
        (1, 3) => set(regs, rd, Some(imm6 << 12)),
        // c.srli, c.srai, c.andi, c.sub, c.xor, c.or, c.and
        (1, 4) => set(regs, prime, None),
        // c.j, c.beqz, c.bnez
        (1, 5 | 6 | 7) => *regs = forgotten(),
        // c.slli, c.lwsp
        (2, 0 | 2) => set(regs, rd, None),
        (2, 4) => {
            let bit12 = w >> 12 & 1;
            match (bit12, rs2) {
                // c.jr
                (0, 0) => *regs = forgotten(),
                // c.mv
                (0, _) => {
                    let value = regs[rs2];
                    set(regs, rd, value);
                }
                // c.ebreak
                (_, 0) if rd == 0 => {}
                // c.jalr
                (_, 0) => *regs = forgotten(),
                // c.add
                _ => {
                    if rd == 2 {
                        let found = by_register(regs, rs2, false, at);
                        set(regs, 2, None);
                        return found;
                    }
                    let value = match (regs[rd], regs[rs2]) {
                        (Some(a), Some(b)) => Some(a.wrapping_add(b)),
                        _ => None,
                    };
                    set(regs, rd, value);
                }
            }
        }
        _ => {}
    }
    None
}

/// Why a plugin with `finding` is refused, with the function it lies in.
fn refusal(finding: &Finding, text: &Section, symbols: &[Symbol], text_index: usize) -> String {
    let at = match finding {
        Finding::Large { at, .. } | Finding::Unknown { at } => *at,
    };
    let place = symbols
        .iter()
        .filter(|s| s.kind() == STT_FUNC && s.shndx as usize == text_index)
        .filter(|s| (s.value.wrapping_sub(text.addr) as usize) <= at)
        .max_by_key(|s| s.value)
        .map(|s| {
            format!(
                "{}+{:#x}",
                s.name,
                at - s.value.wrapping_sub(text.addr) as usize
            )
        })
        .unwrap_or_else(|| format!(".text+{at:#x}"));
    match finding {
        Finding::Large { bytes, .. } => format!(
            "{place} has a stack frame of {bytes} bytes, over the {LIMIT} the X4 watches for \
             (the stack's end band is 2 KiB): keep large buffers in memory the plugin asks the \
             host for, not on the stack"
        ),
        Finding::Unknown { .. } => format!(
            "{place} changes the stack pointer by an amount the packer cannot read (a \
             dynamically sized stack allocation): use a bounded buffer or ask the host for the \
             memory"
        ),
    }
}

/// Refuses a plugin whose `.text` has a frame of [`LIMIT`] bytes or more
/// or one of a size the scan cannot tell.
pub fn check(
    elf: &Elf<'_>,
    text: &Section,
    text_index: usize,
    symbols: &[Symbol],
) -> Result<(), String> {
    let code = elf.data(text)?;
    let mut starts: Vec<usize> = symbols
        .iter()
        .filter(|s| s.kind() == STT_FUNC && s.shndx as usize == text_index)
        .map(|s| s.value.wrapping_sub(text.addr) as usize)
        .filter(|&at| at < code.len())
        .collect();
    starts.sort_unstable();
    starts.dedup();
    match scan(code, &starts) {
        None => Ok(()),
        Some(found) => Err(refusal(&found, text, symbols, text_index)),
    }
}

#[cfg(test)]
mod tests;
