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
//! function's start, at every jump, branch and call, and at every place a
//! branch or jump goes to (found in a first pass), so a size set on one path
//! is not trusted on another: it is refused as unknown, the safe way. Nor is
//! it trusted in a function that jumps indirectly (a switch's table).
//!
//! Every other write to `sp` is refused as unknown, whatever its form: a
//! dynamic `alloca` is `sub a0, sp, a0; mv sp, a0`. Only `sp` set from the
//! frame pointer (`addi sp, s0, -N`, `mv sp, s0`: the epilogue of a function
//! that keeps one) is taken as the frame given back. A function's frame is
//! how deep its prologue and later steps take `sp` below its start, summed
//! (`-1024` twice is 2048), and a function start inside what the walk took
//! for one instruction (data before it) is where the walk goes on from.
//!
//! Matched: `addi sp, sp, -N` and `c.addi16sp` (which reaches 512 bytes only
//! but is parsed all the same), `add sp, sp, rX` with a negative value and
//! `sub sp, sp, rX` with a positive one (known), and the two with a value not
//! known (refused).

use crate::elf::{Elf, Section, Symbol};

/// A frame this large (in bytes) or larger is refused: 1.5 KiB, under the
/// band's 2 KiB with room for the rest of the frames of the call.
pub const LIMIT: u32 = 1536;

/// `STT_FUNC`: a symbol that names a function.
const STT_FUNC: u8 = 2;

/// What the scan found.
#[derive(Debug, PartialEq, Eq)]
pub enum Finding {
    /// A frame of `bytes`, reached at `at` in the code.
    Large { at: usize, bytes: u32 },
    /// A change of `sp` whose size is not known, at `at`.
    Unknown { at: usize },
}

/// A change of `sp` the walk found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adj {
    /// By this many bytes (negative: the frame grows).
    By(i64),
    /// To the frame pointer's value: the frame given back.
    Restore,
    /// By a size not known, or to a value the walk cannot follow.
    Unknown,
}

/// What a register holds, as far as the scan knows.
type Regs = [Option<i32>; 32];

/// What the walk knows at one place.
struct State {
    regs: Regs,
    /// The function jumped indirectly (a switch's table): a place in it may
    /// be entered from anywhere, so no register's value is trusted.
    loose: bool,
}

impl State {
    fn new() -> Self {
        let mut regs: Regs = [None; 32];
        regs[0] = Some(0);
        State { regs, loose: false }
    }

    /// Forgets the registers, as at a place entered from elsewhere.
    fn forget(&mut self) {
        let loose = self.loose;
        *self = Self::new();
        self.loose = loose;
    }
}

/// The instructions of `code` as `(offset, length)`: 32-bit and 16-bit
/// ones, told apart by the two lowest bits. A function's start that falls
/// inside what would be one instruction (data before it, an odd count of
/// bytes) is where the next one begins, so a function is never hidden in
/// its predecessor's tail.
fn layout(code: &[u8], starts: &[usize]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(code.len() / 3);
    let mut at = 0usize;
    while at + 2 <= code.len() {
        let low = u16::from_le_bytes([code[at], code[at + 1]]);
        let len = if low & 3 == 3 { 4 } else { 2 };
        let next = starts.partition_point(|&s| s <= at);
        if let Some(&start) = starts.get(next) {
            if start < at + len {
                at = start;
                continue;
            }
        }
        if at + len > code.len() {
            break;
        }
        out.push((at, len));
        at += len;
    }
    out
}

/// The offsets that a branch or a jump of `code` goes to, sorted.
fn targets(code: &[u8], layout: &[(usize, usize)]) -> Vec<usize> {
    let mut out = Vec::new();
    for &(at, len) in layout {
        let offset = if len == 4 {
            let w = u32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]);
            match w & 0x7f {
                // The branches: imm[12|10:5] and imm[4:1|11].
                0x63 => Some(
                    (((w >> 31) << 12
                        | (w >> 25 & 0x3f) << 5
                        | (w >> 8 & 0xf) << 1
                        | (w >> 7 & 1) << 11) as i32)
                        << 19
                        >> 19,
                ),
                // jal: imm[20|10:1|11|19:12].
                0x6f => Some(
                    (((w >> 31) << 20
                        | (w >> 21 & 0x3ff) << 1
                        | (w >> 20 & 1) << 11
                        | (w >> 12 & 0xff) << 12) as i32)
                        << 11
                        >> 11,
                ),
                _ => None,
            }
        } else {
            let w = u32::from(u16::from_le_bytes([code[at], code[at + 1]]));
            match (w & 3, w >> 13) {
                // c.j, c.jal.
                (1, 1 | 5) => Some(
                    (((w >> 12 & 1) << 11
                        | (w >> 11 & 1) << 4
                        | (w >> 9 & 3) << 8
                        | (w >> 8 & 1) << 10
                        | (w >> 7 & 1) << 6
                        | (w >> 6 & 1) << 7
                        | (w >> 3 & 7) << 1
                        | (w >> 2 & 1) << 5) as i32)
                        << 20
                        >> 20,
                ),
                // c.beqz, c.bnez.
                (1, 6 | 7) => Some(
                    (((w >> 12 & 1) << 8
                        | (w >> 10 & 3) << 3
                        | (w >> 5 & 3) << 6
                        | (w >> 3 & 3) << 1
                        | (w >> 2 & 1) << 5) as i32)
                        << 23
                        >> 23,
                ),
                _ => None,
            }
        };
        if let Some(offset) = offset {
            if let Some(to) = at.checked_add_signed(offset as isize) {
                if to < code.len() {
                    out.push(to);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Walks `code` (a function's start is in `starts`, sorted, offsets into the
/// code) and calls `visit` with the offset and size of every change of `sp`
/// found, until it returns `false`.
pub fn walk(code: &[u8], starts: &[usize], visit: &mut dyn FnMut(usize, Adj) -> bool) {
    let layout = layout(code, starts);
    let targets = targets(code, &layout);
    let mut state = State::new();
    for &(at, len) in &layout {
        if starts.binary_search(&at).is_ok() {
            state = State::new();
        } else if targets.binary_search(&at).is_ok() {
            state.forget();
        }
        let adj = if len == 4 {
            let w = u32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]);
            wide(w, &mut state)
        } else {
            narrow(u16::from_le_bytes([code[at], code[at + 1]]), &mut state)
        };
        if let Some(adj) = adj {
            if !visit(at, adj) {
                return;
            }
        }
    }
}

/// The first frame in `code` that is [`LIMIT`] bytes or larger (summed over
/// a function's steps), or whose size cannot be told.
pub fn scan(code: &[u8], starts: &[usize]) -> Option<Finding> {
    let mut found = None;
    let mut depth = vec![0i64; starts.len()];
    walk(code, starts, &mut |at, adj| {
        let n = starts.partition_point(|&s| s <= at).checked_sub(1);
        found = match (adj, n) {
            (Adj::Unknown, _) => Some(Finding::Unknown { at }),
            (Adj::Restore, _) => None,
            (Adj::By(delta), Some(n)) => {
                depth[n] -= delta;
                (depth[n] >= i64::from(LIMIT)).then_some(Finding::Large {
                    at,
                    bytes: u32::try_from(depth[n]).unwrap_or(u32::MAX),
                })
            }
            // A step before any function's start: on its own.
            (Adj::By(delta), None) => (-delta >= i64::from(LIMIT)).then_some(Finding::Large {
                at,
                bytes: u32::try_from(-delta).unwrap_or(u32::MAX),
            }),
        };
        found.is_none()
    });
    found
}

/// The deepest `sp` goes below its value at each function's start, for
/// every function in `starts` (`None`: some change was not readable): the
/// function's own frame, as the prologue makes it and the epilogues give
/// it back.
#[allow(dead_code)] // the firmware listing (`paperpaw-sign frames`) reads them
pub fn frame_sizes(code: &[u8], starts: &[usize]) -> Vec<Option<u32>> {
    let mut sizes: Vec<Option<u32>> = vec![Some(0); starts.len()];
    let mut depth = vec![0i64; starts.len()];
    walk(code, starts, &mut |at, adj| {
        // The function this lies in: the last start at or before it.
        let Some(n) = starts.partition_point(|&s| s <= at).checked_sub(1) else {
            return true;
        };
        match adj {
            Adj::Unknown => sizes[n] = None,
            Adj::Restore => {}
            Adj::By(delta) => {
                depth[n] -= delta;
                if let Some(size) = sizes[n].as_mut() {
                    let now = u32::try_from(depth[n].max(0)).unwrap_or(u32::MAX);
                    *size = (*size).max(now);
                }
            }
        }
        true
    });
    sizes
}

fn set(state: &mut State, rd: usize, value: Option<i32>) {
    if rd != 0 {
        state.regs[rd] = value;
    }
}

/// `sp = sp ± reg`, the register's value as the state has it.
fn by_register(state: &State, rs2: usize, negate: bool) -> Adj {
    match (state.loose, state.regs[rs2]) {
        (false, Some(v)) => Adj::By(if negate { -i64::from(v) } else { i64::from(v) }),
        _ => Adj::Unknown,
    }
}

/// The opcodes whose bits 11 to 7 are a register they write: `lui`,
/// `auipc`, `jal`, `jalr`, the loads, the immediate and register operations,
/// the system and atomic instructions.
fn writes_rd(opcode: u32) -> bool {
    matches!(
        opcode,
        0x37 | 0x17 | 0x6f | 0x67 | 0x03 | 0x13 | 0x33 | 0x73 | 0x2f
    )
}

/// A 32-bit instruction.
fn wide(w: u32, state: &mut State) -> Option<Adj> {
    let opcode = w & 0x7f;
    let rd = (w >> 7 & 31) as usize;
    let rs1 = (w >> 15 & 31) as usize;
    let rs2 = (w >> 20 & 31) as usize;
    let funct3 = w >> 12 & 7;
    let imm_i = (w as i32) >> 20;
    let mut adj = None;
    match opcode {
        // lui
        0x37 => set(state, rd, Some((w & 0xffff_f000) as i32)),
        // addi and the other immediate operations
        0x13 => {
            if funct3 == 0 {
                if rd == 2 && rs1 == 2 {
                    adj = Some(Adj::By(i64::from(imm_i)));
                } else if rd == 2 && rs1 == 8 {
                    adj = Some(Adj::Restore);
                }
                let value = state.regs[rs1].map(|v| v.wrapping_add(imm_i));
                set(state, rd, value);
            } else {
                set(state, rd, None);
            }
        }
        // add, sub and the other register operations
        0x33 => {
            let add = funct3 == 0 && w >> 25 == 0;
            let sub = funct3 == 0 && w >> 25 == 0x20;
            if rd == 2 && rs1 == 2 && (add || sub) {
                adj = Some(by_register(state, rs2, sub));
            }
            let value = match (add, sub, state.regs[rs1], state.regs[rs2]) {
                (true, _, Some(a), Some(b)) => Some(a.wrapping_add(b)),
                (_, true, Some(a), Some(b)) => Some(a.wrapping_sub(b)),
                _ => None,
            };
            set(state, rd, value);
        }
        // jal, the branches: what is known is not carried over
        0x6f | 0x63 => state.forget(),
        // jalr: a call or a return (what is known is not carried over); a
        // jump that is neither (a switch's table, a tail call) loosens the
        // function.
        0x67 => {
            let is_return = rd == 0 && rs1 == 1 && imm_i == 0;
            if rd == 0 && !is_return {
                state.loose = true;
            }
            state.forget();
        }
        // loads, auipc, atomics, system, floating-point to a register
        0x03 | 0x17 | 0x2f | 0x73 | 0x53 | 0x07 => set(state, rd, None),
        _ => {}
    }
    // Any other write to sp is not followed: `mv sp, a0` after `sub a0, sp,
    // a0` is a dynamic alloca.
    if adj.is_none() && rd == 2 && writes_rd(opcode) {
        adj = Some(Adj::Unknown);
    }
    adj
}

/// A 16-bit (compressed) instruction.
fn narrow(w: u16, state: &mut State) -> Option<Adj> {
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
        (0, 0 | 2) => set(state, (w >> 2 & 7) as usize + 8, None),
        // c.addi, and c.nop (rd 0)
        (1, 0) if rd == 2 => return Some(Adj::By(i64::from(imm6))),
        (1, 0) => {
            let value = state.regs[rd].map(|v| v.wrapping_add(imm6));
            set(state, rd, value);
        }
        // c.jal
        (1, 1) => state.forget(),
        // c.li
        (1, 2) if rd == 2 => return Some(Adj::Unknown),
        (1, 2) => set(state, rd, Some(imm6)),
        // c.addi16sp (rd 2), c.lui
        (1, 3) if rd == 2 => {
            let imm = (w >> 12 & 1) << 9
                | (w >> 3 & 3) << 7
                | (w >> 5 & 1) << 6
                | (w >> 2 & 1) << 5
                | (w >> 6 & 1) << 4;
            return Some(Adj::By(i64::from((imm as i32) << 22 >> 22)));
        }
        (1, 3) => set(state, rd, Some(imm6 << 12)),
        // c.srli, c.srai, c.andi, c.sub, c.xor, c.or, c.and
        (1, 4) => set(state, prime, None),
        // c.j, c.beqz, c.bnez
        (1, 5..=7) => state.forget(),
        // c.slli, c.lwsp
        (2, 0 | 2) if rd == 2 => return Some(Adj::Unknown),
        (2, 0 | 2) => set(state, rd, None),
        (2, 4) => {
            let bit12 = w >> 12 & 1;
            match (bit12, rs2) {
                // c.jr: a return through `ra`, or a jump of another kind
                (0, 0) => {
                    if rd != 1 {
                        state.loose = true;
                    }
                    state.forget();
                }
                // c.mv: `mv sp, s0` is the frame given back; any other value
                // for `sp` is not followed
                (0, _) => {
                    if rd == 2 {
                        return Some(if rs2 == 8 { Adj::Restore } else { Adj::Unknown });
                    }
                    let value = state.regs[rs2];
                    set(state, rd, value);
                }
                // c.ebreak
                (_, 0) if rd == 0 => {}
                // c.jalr
                (_, 0) => state.forget(),
                // c.add
                _ => {
                    if rd == 2 {
                        return Some(by_register(state, rs2, false));
                    }
                    let value = match (state.regs[rd], state.regs[rs2]) {
                        (Some(a), Some(b)) => Some(a.wrapping_add(b)),
                        _ => None,
                    };
                    set(state, rd, value);
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
