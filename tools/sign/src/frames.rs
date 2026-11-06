//! The pack-time scan for large stack frames in a plugin's code.
//!
//! A plugin runs on the firmware's one stack, and the X4 watches the stack's
//! end with a band the CPU's data trigger fires in (`shells/xteink/src/
//! stack.rs`, `paperpaw_x4::stack_band`). A frame bigger than the band that
//! never stores into it steps over the band unseen. So the packer reads the
//! code for what makes a frame, the changes of `sp`, and refuses a plugin
//! with a frame of [`LIMIT`] bytes or more, or with a change it cannot bound.
//!
//! The code is RISC-V (RV32IMC): 32-bit and 16-bit (compressed)
//! instructions, told apart by their two lowest bits. Each function is read
//! within its `STT_FUNC` symbol's `st_size` (never on into a neighbour), as a
//! graph: a worklist visits every instruction along every path, with what the
//! registers hold, `sp` and the frame pointer's value relative to the
//! function's start, joined where paths meet (a register that differs is
//! unknown; `sp` is the lowest of the paths'). The frame is the most negative
//! `sp` any path reaches. Code in the function that no path reaches (a
//! switch's cases, entered through a jump table) is read from the state at
//! the indirect jump with no register known. Bytes of the `.text` that no
//! function symbol covers are refused unless they are padding (zeros, `nop`
//! and `c.nop`): code scanned into a neighbour or not scanned at all is how a
//! frame hides.
//!
//! What changes `sp` and is let through:
//! - `addi sp, sp, imm`, `c.addi`, `c.addi16sp`: an immediate;
//! - `add`/`sub sp, sp, rX` where `rX` is a constant on every path to it,
//!   built from immediates (`lui`, `li`, `addi`, `c.li`, `c.lui`, `c.addi`,
//!   `mv`, `add`, `sub` of constants). That is what a compiler emits for a
//!   frame beyond `addi`'s 2 KiB (LLVM: `lui t0, hi; addi t0, t0, lo; add sp,
//!   sp, t0`), and such a frame is 4 KiB or more, so it is a refusal by the
//!   limit all the same; the exact size is kept for the listing of the
//!   firmware's own frames (`paperpaw-sign frames`);
//! - `addi sp, s0, imm` and `mv sp, s0`, only where `s0` was set from `sp`
//!   in the same function (`addi s0, sp, imm`, `mv s0, sp`) and not written
//!   since: the frame pointer's restore, which puts `sp` where the prologue
//!   left it, relative to the function's start.
//!
//! Every other write to `sp` (a dynamic `alloca` is `sub a0, sp, a0; mv sp,
//! a0`; `lw sp`, `lui sp`, a register that is not a constant on every path)
//! is refused as unbounded.

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
    /// A change of `sp` that is not bounded, at `at`.
    Unknown { at: usize },
    /// Code at `at` that no function symbol covers and that is no padding.
    Uncovered { at: usize },
}

/// What a register holds, as far as the scan knows.
type Regs = [Option<i32>; 32];

/// What the walk knows before an instruction.
#[derive(Clone, PartialEq, Eq)]
struct State {
    regs: Regs,
    /// `sp` relative to the function's start (negative: the frame).
    sp: i64,
    /// `s0` as `sp` was when it was set from it, relative to the start;
    /// `None` when it is not known to be a frame pointer.
    fp: Option<i64>,
}

impl State {
    /// The state at a function's start.
    fn entry() -> Self {
        let mut regs: Regs = [None; 32];
        regs[0] = Some(0);
        State {
            regs,
            sp: 0,
            fp: None,
        }
    }

    /// Where paths meet: a register that differs is unknown, the frame
    /// pointer is kept only if it is the same, `sp` is the lowest.
    fn join(&self, other: &State) -> State {
        let mut regs = self.regs;
        for (r, o) in regs.iter_mut().zip(other.regs) {
            if *r != o {
                *r = None;
            }
        }
        State {
            regs,
            sp: self.sp.min(other.sp),
            fp: if self.fp == other.fp { self.fp } else { None },
        }
    }

    /// After a call: what a callee may change is unknown (`s0` and `sp` are
    /// kept: the calling convention does).
    fn called(&mut self) {
        let s0 = self.regs[8];
        let zero = self.regs[0];
        self.regs = [None; 32];
        self.regs[0] = zero;
        self.regs[8] = s0;
    }

    fn set(&mut self, rd: usize, value: Option<i32>) {
        if rd == 8 {
            // Written otherwise than as the frame pointer's set: not one.
            self.fp = None;
        }
        if rd != 0 {
            self.regs[rd] = value;
        }
    }
}

/// How an instruction goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctl {
    Fall,
    /// Falls through or goes to `offset` from the instruction.
    Branch(i64),
    /// Goes to `offset`, does not fall through.
    Jump(i64),
    Ret,
    /// A jump through a register that is not a return.
    Indirect,
}

/// What one instruction does: its way on, and whether it wrote `sp` in a
/// way that is not bounded.
struct Effect {
    ctl: Ctl,
    unbounded: bool,
}

fn fall() -> Effect {
    Effect {
        ctl: Ctl::Fall,
        unbounded: false,
    }
}

/// `sp = sp ± reg`: the register's constant, if it has one.
fn by_register(state: &mut State, rs2: usize, negate: bool) -> bool {
    match state.regs[rs2] {
        Some(v) => {
            state.sp += if negate { -i64::from(v) } else { i64::from(v) };
            false
        }
        None => true,
    }
}

/// `sp` set from the frame pointer, plus `imm`: where the prologue left it.
fn from_frame_pointer(state: &mut State, imm: i64) -> bool {
    match state.fp {
        Some(fp) => {
            state.sp = fp + imm;
            false
        }
        None => true,
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
fn wide(w: u32, state: &mut State) -> Effect {
    let opcode = w & 0x7f;
    let rd = (w >> 7 & 31) as usize;
    let rs1 = (w >> 15 & 31) as usize;
    let rs2 = (w >> 20 & 31) as usize;
    let funct3 = w >> 12 & 7;
    let imm_i = (w as i32) >> 20;
    let mut effect = fall();
    // Whether `sp` was written by one of the forms that are let through.
    let mut handled = false;
    match opcode {
        // lui
        0x37 => state.set(rd, Some((w & 0xffff_f000) as i32)),
        // addi and the other immediate operations
        0x13 => {
            if funct3 == 0 {
                if rd == 2 && rs1 == 2 {
                    state.sp += i64::from(imm_i);
                    handled = true;
                } else if rd == 2 && rs1 == 8 {
                    effect.unbounded = from_frame_pointer(state, i64::from(imm_i));
                    handled = true;
                } else if rd == 8 && rs1 == 2 {
                    // The frame pointer is set from sp.
                    state.set(8, None);
                    state.fp = Some(state.sp + i64::from(imm_i));
                    return effect;
                }
                let value = state.regs[rs1].map(|v| v.wrapping_add(imm_i));
                state.set(rd, value);
            } else {
                state.set(rd, None);
            }
        }
        // add, sub and the other register operations
        0x33 => {
            let add = funct3 == 0 && w >> 25 == 0;
            let sub = funct3 == 0 && w >> 25 == 0x20;
            if rd == 2 && rs1 == 2 && (add || sub) {
                effect.unbounded = by_register(state, rs2, sub);
                handled = true;
            }
            let value = match (add, sub, state.regs[rs1], state.regs[rs2]) {
                (true, _, Some(a), Some(b)) => Some(a.wrapping_add(b)),
                (_, true, Some(a), Some(b)) => Some(a.wrapping_sub(b)),
                _ => None,
            };
            state.set(rd, value);
        }
        // The branches: imm[12|10:5] and imm[4:1|11].
        0x63 => {
            let offset = ((((w >> 31) << 12
                | (w >> 25 & 0x3f) << 5
                | (w >> 8 & 0xf) << 1
                | (w >> 7 & 1) << 11) as i32)
                << 19
                >> 19) as i64;
            effect.ctl = Ctl::Branch(offset);
        }
        // jal: imm[20|10:1|11|19:12]. A call falls through, a jump does not.
        0x6f => {
            let offset = ((((w >> 31) << 20
                | (w >> 21 & 0x3ff) << 1
                | (w >> 20 & 1) << 11
                | (w >> 12 & 0xff) << 12) as i32)
                << 11
                >> 11) as i64;
            if rd == 0 {
                effect.ctl = Ctl::Jump(offset);
            } else {
                state.called();
                state.set(rd, None);
            }
        }
        // jalr: a return, a call (falls through), or a jump through a register.
        0x67 => {
            if rd == 0 {
                effect.ctl = if rs1 == 1 && imm_i == 0 {
                    Ctl::Ret
                } else {
                    Ctl::Indirect
                };
            } else {
                state.called();
                state.set(rd, None);
            }
        }
        // loads, auipc, atomics, system, floating-point to a register
        0x03 | 0x17 | 0x2f | 0x73 | 0x53 | 0x07 => state.set(rd, None),
        _ => {}
    }
    // Any other write to sp is unbounded: `mv sp, a0` after `sub a0, sp, a0`
    // is a dynamic alloca.
    if !handled && rd == 2 && writes_rd(opcode) {
        effect.unbounded = true;
    }
    effect
}

/// A 16-bit (compressed) instruction.
fn narrow(w: u16, state: &mut State) -> Effect {
    let w = u32::from(w);
    let funct3 = w >> 13;
    let rd = (w >> 7 & 31) as usize;
    let rs2 = (w >> 2 & 31) as usize;
    // The 6-bit immediate of c.addi, c.li, c.lui and c.andi.
    let imm6 = (((w >> 12 & 1) << 5 | (w >> 2 & 31)) as i32) << 26 >> 26;
    // The three-bit register fields: x8 to x15.
    let prime = (w >> 7 & 7) as usize + 8;
    let mut effect = fall();
    match (w & 3, funct3) {
        // c.addi4spn: rd' = sp + imm, a frame pointer if it is x8
        (0, 0) => {
            let r = (w >> 2 & 7) as usize + 8;
            let imm =
                (w >> 6 & 1) << 2 | (w >> 5 & 1) << 3 | (w >> 11 & 3) << 4 | (w >> 7 & 15) << 6;
            if r == 8 {
                state.set(8, None);
                state.fp = Some(state.sp + i64::from(imm));
            } else {
                state.set(r, None);
            }
        }
        // c.lw
        (0, 2) => state.set((w >> 2 & 7) as usize + 8, None),
        // c.addi, and c.nop (rd 0)
        (1, 0) if rd == 2 => state.sp += i64::from(imm6),
        (1, 0) => {
            let value = state.regs[rd].map(|v| v.wrapping_add(imm6));
            state.set(rd, value);
        }
        // c.jal: a call
        (1, 1) => state.called(),
        // c.li
        (1, 2) if rd == 2 => effect.unbounded = true,
        (1, 2) => state.set(rd, Some(imm6)),
        // c.addi16sp (rd 2), c.lui
        (1, 3) if rd == 2 => {
            let imm = (w >> 12 & 1) << 9
                | (w >> 3 & 3) << 7
                | (w >> 5 & 1) << 6
                | (w >> 2 & 1) << 5
                | (w >> 6 & 1) << 4;
            state.sp += i64::from((imm as i32) << 22 >> 22);
        }
        (1, 3) => state.set(rd, Some(imm6 << 12)),
        // c.srli, c.srai, c.andi, c.sub, c.xor, c.or, c.and
        (1, 4) => state.set(prime, None),
        // c.j: imm[11|4|9:8|10|6|7|3:1|5]
        (1, 5) => {
            let offset = (((w >> 12 & 1) << 11
                | (w >> 11 & 1) << 4
                | (w >> 9 & 3) << 8
                | (w >> 8 & 1) << 10
                | (w >> 7 & 1) << 6
                | (w >> 6 & 1) << 7
                | (w >> 3 & 7) << 1
                | (w >> 2 & 1) << 5) as i32)
                << 20
                >> 20;
            effect.ctl = Ctl::Jump(i64::from(offset));
        }
        // c.beqz, c.bnez: imm[8|4:3|7:6|2:1|5]
        (1, 6 | 7) => {
            let offset = (((w >> 12 & 1) << 8
                | (w >> 10 & 3) << 3
                | (w >> 5 & 3) << 6
                | (w >> 3 & 3) << 1
                | (w >> 2 & 1) << 5) as i32)
                << 23
                >> 23;
            effect.ctl = Ctl::Branch(i64::from(offset));
        }
        // c.slli, c.lwsp
        (2, 0 | 2) if rd == 2 => effect.unbounded = true,
        (2, 0 | 2) => state.set(rd, None),
        (2, 4) => {
            let bit12 = w >> 12 & 1;
            match (bit12, rs2) {
                // c.jr: a return through `ra`, or a jump through a register
                (0, 0) => effect.ctl = if rd == 1 { Ctl::Ret } else { Ctl::Indirect },
                // c.mv: `mv sp, s0` is the frame pointer's restore, `mv s0,
                // sp` sets it; any other value for `sp` is not followed
                (0, _) => {
                    if rd == 2 {
                        if rs2 == 8 {
                            effect.unbounded = from_frame_pointer(state, 0);
                        } else {
                            effect.unbounded = true;
                        }
                    } else if rd == 8 && rs2 == 2 {
                        state.set(8, None);
                        state.fp = Some(state.sp);
                    } else {
                        let value = state.regs[rs2];
                        state.set(rd, value);
                    }
                }
                // c.ebreak
                (_, 0) if rd == 0 => {}
                // c.jalr: a call
                (_, 0) => state.called(),
                // c.add
                _ => {
                    if rd == 2 {
                        effect.unbounded = by_register(state, rs2, false);
                    } else {
                        let value = match (state.regs[rd], state.regs[rs2]) {
                            (Some(a), Some(b)) => Some(a.wrapping_add(b)),
                            _ => None,
                        };
                        state.set(rd, value);
                    }
                }
            }
        }
        _ => {}
    }
    effect
}

/// What the graph walk of one function found.
#[derive(Debug, PartialEq, Eq)]
pub struct Walked {
    /// The most negative `sp` along any path, as a frame in bytes.
    pub frame: u32,
    /// Where it was first reached.
    pub frame_at: usize,
    /// The first change of `sp` that is not bounded, if any.
    pub unbounded: Option<usize>,
}

/// How often one instruction may be visited before the walk gives up on it
/// (a loop that moves `sp` never settles).
const VISITS: u8 = 64;

/// The walk of one function.
struct Walk<'c> {
    code: &'c [u8],
    start: usize,
    end: usize,
    /// The function's instructions: offset and length.
    insns: Vec<(usize, usize)>,
    states: Vec<Option<State>>,
    visits: Vec<u8>,
    work: Vec<usize>,
    /// What the state was at a jump through a register, joined.
    indirect: Option<State>,
    out: Walked,
}

impl Walk<'_> {
    /// Joins `state` into instruction `j`'s, queueing it if that changed.
    fn join_into(&mut self, j: usize, state: &State) {
        let joined = match &self.states[j] {
            None => state.clone(),
            Some(old) => old.join(state),
        };
        if self.states[j].as_ref() != Some(&joined) {
            self.states[j] = Some(joined);
            self.work.push(j);
        }
    }

    /// The instruction a jump of `offset` from instruction `at` goes to, if it
    /// is inside the function (a tail jump out of it is not followed).
    fn target(&mut self, at: usize, offset: i64) -> Option<usize> {
        let t = usize::try_from((at as i64).checked_add(offset)?).ok()?;
        if t < self.start || t >= self.end {
            return None;
        }
        match self.insns.binary_search_by_key(&t, |&(a, _)| a) {
            Ok(j) => Some(j),
            // Into the middle of an instruction: not followed, and not bounded.
            Err(_) => {
                self.out.unbounded.get_or_insert(at);
                None
            }
        }
    }

    /// Runs instruction `i` on its state and queues where it goes.
    fn step(&mut self, i: usize) {
        let (at, len) = self.insns[i];
        self.visits[i] += 1;
        if self.visits[i] > VISITS {
            self.out.unbounded.get_or_insert(at);
            return;
        }
        let Some(mut state) = self.states[i].clone() else {
            return;
        };
        let code = self.code;
        let effect = if len == 4 {
            wide(
                u32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]),
                &mut state,
            )
        } else {
            narrow(u16::from_le_bytes([code[at], code[at + 1]]), &mut state)
        };
        if effect.unbounded {
            self.out.unbounded.get_or_insert(at);
        }
        if state.sp < -i64::from(self.out.frame) {
            self.out.frame = u32::try_from(-state.sp).unwrap_or(u32::MAX);
            self.out.frame_at = at;
        }
        let next = (i + 1 < self.insns.len()).then_some(i + 1);
        let goes = match effect.ctl {
            Ctl::Fall => [next, None],
            Ctl::Branch(offset) => [next, self.target(at, offset)],
            Ctl::Jump(offset) => [self.target(at, offset), None],
            Ctl::Ret => [None, None],
            Ctl::Indirect => {
                // The cases are entered from the jump, with the frame pointer
                // it had, and no register known.
                let mut s = state.clone();
                s.regs = [None; 32];
                s.regs[0] = Some(0);
                self.indirect = Some(match self.indirect.take() {
                    Some(old) => old.join(&s),
                    None => s,
                });
                [None, None]
            }
        };
        for j in goes.into_iter().flatten() {
            self.join_into(j, &state);
        }
    }
}

/// Walks the function that is `code[start..end]`: every instruction along
/// every path, as described in the module documentation.
pub fn walk(code: &[u8], start: usize, end: usize) -> Walked {
    let end = end.min(code.len());
    let mut insns: Vec<(usize, usize)> = Vec::new();
    let mut at = start;
    while at + 2 <= end {
        let low = u16::from_le_bytes([code[at], code[at + 1]]);
        let len = if low & 3 == 3 { 4 } else { 2 };
        if at + len > end {
            break;
        }
        insns.push((at, len));
        at += len;
    }
    let count = insns.len();
    let mut w = Walk {
        code,
        start,
        end,
        insns,
        states: vec![None; count],
        visits: vec![0; count],
        work: Vec::new(),
        indirect: None,
        out: Walked {
            frame: 0,
            frame_at: start,
            unbounded: None,
        },
    };
    if count > 0 {
        w.states[0] = Some(State::entry());
        w.work.push(0);
    }
    loop {
        while let Some(i) = w.work.pop() {
            w.step(i);
        }
        // Code no path reaches: a switch's cases, or dead code. Read from the
        // state at the indirect jump (or the start's), no register known.
        let Some(j) = w.states.iter().position(Option::is_none) else {
            break;
        };
        let mut seed = w.indirect.clone().unwrap_or_else(State::entry);
        seed.regs = [None; 32];
        seed.regs[0] = Some(0);
        w.states[j] = Some(seed);
        w.work.push(j);
    }
    w.out
}

/// The ranges of the functions in `symbols` (their start and `st_size`), each
/// from its start to the end its size gives, but no further than the next
/// function's start: a symbol whose size runs into its neighbour is read
/// only as far as the neighbour begins, which has its own range. Aliases
/// (the same start) are one, with the larger size.
pub fn function_ranges(symbols: &[(usize, usize)], code_len: usize) -> Vec<(usize, usize)> {
    let mut sorted: Vec<(usize, usize)> = symbols
        .iter()
        .copied()
        .filter(|&(start, size)| size > 0 && start < code_len)
        .collect();
    sorted.sort_unstable();
    let mut unique: Vec<(usize, usize)> = Vec::new();
    for (start, size) in sorted {
        match unique.last_mut() {
            Some(last) if last.0 == start => last.1 = last.1.max(size),
            _ => unique.push((start, size)),
        }
    }
    let starts: Vec<usize> = unique.iter().map(|&(s, _)| s).collect();
    unique
        .iter()
        .enumerate()
        .map(|(n, &(start, size))| {
            let end = start.saturating_add(size).min(code_len);
            (start, starts.get(n + 1).map_or(end, |&next| end.min(next)))
        })
        .collect()
}

/// The function ranges for `starts` (sorted): each runs to the next start,
/// the last to the end of `code`.
#[cfg(test)]
fn ranges(code: &[u8], starts: &[usize]) -> Vec<(usize, usize)> {
    starts
        .iter()
        .enumerate()
        .map(|(n, &s)| (s, starts.get(n + 1).copied().unwrap_or(code.len())))
        .collect()
}

/// The first frame in the functions `ranges` of `code` that is [`LIMIT`]
/// bytes or larger, or whose `sp` is not bounded.
pub fn scan_ranges(code: &[u8], ranges: &[(usize, usize)]) -> Option<Finding> {
    for &(start, end) in ranges {
        let w = walk(code, start, end);
        if let Some(at) = w.unbounded {
            return Some(Finding::Unknown { at });
        }
        if w.frame >= LIMIT {
            return Some(Finding::Large {
                at: w.frame_at,
                bytes: w.frame,
            });
        }
    }
    None
}

/// [`scan_ranges`] for functions that start at `starts` (sorted) and run to
/// the next.
#[cfg(test)]
pub fn scan(code: &[u8], starts: &[usize]) -> Option<Finding> {
    scan_ranges(code, &ranges(code, starts))
}

/// The frame of each function in `ranges` (`None`: `sp` not bounded).
#[allow(
    dead_code,
    reason = "the tests and the firmware listing (`paperpaw-sign frames`)"
)]
pub fn frame_sizes_in(code: &[u8], ranges: &[(usize, usize)]) -> Vec<Option<u32>> {
    ranges
        .iter()
        .map(|&(s, e)| {
            let w = walk(code, s, e);
            w.unbounded.is_none().then_some(w.frame)
        })
        .collect()
}

/// [`frame_sizes_in`] for functions that start at `starts` (sorted) and run
/// to the next.
#[cfg(test)]
pub fn frame_sizes(code: &[u8], starts: &[usize]) -> Vec<Option<u32>> {
    frame_sizes_in(code, &ranges(code, starts))
}

/// The first byte of `code` that no range in `ranges` (sorted by start)
/// covers and that is not padding: zeros, `nop` (`addi x0, x0, 0`), `c.nop`.
pub fn uncovered(code: &[u8], ranges: &[(usize, usize)]) -> Option<usize> {
    let mut at = 0usize;
    let mut next = 0usize;
    while at < code.len() {
        while next < ranges.len() && ranges[next].1 <= at {
            next += 1;
        }
        match ranges.get(next) {
            Some(&(s, e)) if s <= at => {
                at = e;
                continue;
            }
            _ => {}
        }
        // A gap up to the next range's start (or the end).
        let gap_end = ranges
            .get(next)
            .map_or(code.len(), |&(s, _)| s)
            .min(code.len());
        let mut i = at;
        while i < gap_end {
            let rest = &code[i..gap_end];
            if rest[0] == 0 {
                i += 1;
            } else if rest.len() >= 2 && rest[0] == 0x01 && rest[1] == 0x00 {
                i += 2;
            } else if rest.len() >= 4 && rest[..4] == [0x13, 0x00, 0x00, 0x00] {
                i += 4;
            } else {
                return Some(i);
            }
        }
        at = gap_end;
    }
    None
}

/// Why a plugin with `finding` is refused, with the function it lies in.
fn refusal(finding: &Finding, text: &Section, symbols: &[Symbol], text_index: usize) -> String {
    let at = match finding {
        Finding::Large { at, .. } | Finding::Unknown { at } | Finding::Uncovered { at } => *at,
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
            "{place} changes the stack pointer in a way the packer cannot bound (a dynamically \
             sized stack allocation, or a register that is not a constant): use a bounded \
             buffer or ask the host for the memory"
        ),
        Finding::Uncovered { .. } => format!(
            ".text+{at:#x} is code that no function symbol covers: the packer reads each \
             function within its symbol's size and does not scan the rest (do not strip the \
             plugin's symbols, and do not link code without them)"
        ),
    }
}

/// Refuses a plugin whose `.text` has a frame of [`LIMIT`] bytes or more,
/// a change of `sp` the scan cannot bound, or code outside every function's
/// symbol.
pub fn check(
    elf: &Elf<'_>,
    text: &Section,
    text_index: usize,
    symbols: &[Symbol],
) -> Result<(), String> {
    let code = elf.data(text)?;
    let sized: Vec<(usize, usize)> = symbols
        .iter()
        .filter(|s| s.kind() == STT_FUNC && s.shndx as usize == text_index)
        .map(|s| (s.value.wrapping_sub(text.addr) as usize, s.size as usize))
        .collect();
    let merged = function_ranges(&sized, code.len());
    let found = uncovered(code, &merged)
        .map(|at| Finding::Uncovered { at })
        .or_else(|| scan_ranges(code, &merged));
    match found {
        None => Ok(()),
        Some(found) => Err(refusal(&found, text, symbols, text_index)),
    }
}

#[cfg(test)]
mod tests;
