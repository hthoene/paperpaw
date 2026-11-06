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
//! within its `STT_FUNC` symbol's `st_size` (never on into a neighbour; a
//! range that ends inside an instruction, or starts off a halfword, is
//! refused), as a graph: a worklist visits every instruction along every
//! path, with what the registers hold, `sp` and the frame pointer's value
//! relative to the function's start, joined where paths meet (a register that
//! differs is unknown; `sp` is the lowest of the paths'). The frame is the
//! most negative `sp` any path reaches. Bytes of the `.text` that no function
//! symbol covers are refused unless they are padding (zeros, `nop` and
//! `c.nop`): code scanned into a neighbour or not scanned at all is how a
//! frame hides.
//!
//! A jump through a register that is not a return:
//! - with the frame given back (`sp` at the function's start): a tail call
//!   (`jr a5` after the epilogue), or a jump anywhere in the function; the
//!   state at the jump is joined into every instruction of the function, so
//!   a jump back to the start finds nothing it can grow;
//! - inside a frame: refused. That is a jump table (a `match`), and where it
//!   goes is not known; build the plugin with `-C jump-tables=no`. (The
//!   listing of the firmware's own frames, which is no gate, reads such a
//!   jump's cases from the state at the jump.)
//!
//! A call to a function that never returns ends its path. Which functions
//! those are is proved from the code, not from names: one whose paths from
//! its start do not return, jump out of it (except to such a function) or run
//! off its end, found as the largest set with that property (a trap,
//! `ebreak` and `unimp`, ends a path: the firmware's handler never resumes
//! it).
//!
//! What this is for, and not for: the goal is that no single function lowers
//! `sp` by [`LIMIT`] or more in total without a store in between, which the
//! band at the stack's end would see. The depth of a whole call path and
//! loops through calls are not decided here (not statically decidable); the
//! band catches those at run time, and the device's high-water test measures
//! them. The scan is conservative where it must be: a function's frame is the
//! lowest `sp` along its paths, so two frames that exclude each other are not
//! added, but a frame taken in steps is, and a plugin that needs a path it
//! cannot prove (a computed `sp`, a jump table) is refused.
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
    /// A jump through a register at `at`, inside a frame (a jump table).
    Table { at: usize },
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
    /// A call that does not return (a panic's): nothing follows it.
    NoReturn,
    /// A jump through a register that is not a return.
    Indirect,
    /// `ebreak`, `unimp`: the CPU takes an exception and the firmware's
    /// handler never resumes the code (`shells/xteink/src/crash.rs`).
    Trap,
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
fn wide(w: u32, state: &mut State, pc: u32, env: &Env<'_>) -> Effect {
    let opcode = w & 0x7f;
    let rd = (w >> 7 & 31) as usize;
    let rs1 = (w >> 15 & 31) as usize;
    let rs2 = (w >> 20 & 31) as usize;
    let funct3 = w >> 12 & 7;
    let imm_i = (w as i32) >> 20;
    let mut effect = fall();
    // Whether `sp` was written by one of the forms that are let through.
    let mut handled = false;
    if w == 0x0010_0073 || w == 0xc000_1073 {
        // ebreak, unimp (`csrrw x0, cycle, x0`)
        effect.ctl = Ctl::Trap;
        return effect;
    }
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
                if env.is_noreturn(pc.wrapping_add(offset as u32)) {
                    effect.ctl = Ctl::NoReturn;
                }
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
                // A call through a register, an `auipc` pair's: where to?
                let target = state.regs[rs1].map(|v| (v as u32).wrapping_add(imm_i as u32));
                if target.is_some_and(|t| env.is_noreturn(t)) {
                    effect.ctl = Ctl::NoReturn;
                }
                state.called();
                state.set(rd, None);
            }
        }
        // auipc: the pc plus the immediate, a known address
        0x17 => state.set(rd, Some(pc.wrapping_add(w & 0xffff_f000) as i32)),
        // loads, atomics, system, floating-point to a register
        0x03 | 0x2f | 0x73 | 0x53 | 0x07 => state.set(rd, None),
        _ => {}
    }
    // Any other write to sp is unbounded: `mv sp, a0` after `sub a0, sp, a0`
    // is a dynamic alloca.
    if !handled && rd == 2 && writes_rd(opcode) {
        effect.unbounded = true;
    }
    effect
}

/// The offset of `c.j` and `c.jal`: imm[11|4|9:8|10|6|7|3:1|5].
fn cj_offset(w: u32) -> i32 {
    (((w >> 12 & 1) << 11
        | (w >> 11 & 1) << 4
        | (w >> 9 & 3) << 8
        | (w >> 8 & 1) << 10
        | (w >> 7 & 1) << 6
        | (w >> 6 & 1) << 7
        | (w >> 3 & 7) << 1
        | (w >> 2 & 1) << 5) as i32)
        << 20
        >> 20
}

/// A 16-bit (compressed) instruction.
fn narrow(w: u16, state: &mut State, pc: u32, env: &Env<'_>) -> Effect {
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
        // c.unimp: the all-zero halfword is an illegal instruction
        (0, 0) if w == 0 => effect.ctl = Ctl::Trap,
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
        (1, 1) => {
            if env.is_noreturn(pc.wrapping_add(cj_offset(w) as u32)) {
                effect.ctl = Ctl::NoReturn;
            }
            state.called();
        }
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
        // c.j
        (1, 5) => effect.ctl = Ctl::Jump(i64::from(cj_offset(w))),
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
                (_, 0) if rd == 0 => effect.ctl = Ctl::Trap,
                // c.jalr: a call through a register
                (_, 0) => {
                    if state.regs[rd].is_some_and(|t| env.is_noreturn(t as u32)) {
                        effect.ctl = Ctl::NoReturn;
                    }
                    state.called();
                }
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

/// What the walk needs to know of the code's place: where it lies, and the
/// functions that never return (the panics: code after a call to one is not
/// reached from it).
#[derive(Clone, Copy)]
pub struct Env<'a> {
    /// The address of the first byte of the code.
    pub base: u32,
    /// The start addresses of the functions that do not return, sorted.
    pub noreturn: &'a [u32],
    /// Reads the cases of a jump table from the state at the jump (the
    /// listing of the firmware's own frames, `paperpaw-sign frames`). A
    /// plugin's pack is strict: a jump through a register inside a frame is
    /// refused, because where it goes is not known.
    pub lenient: bool,
}

impl Env<'_> {
    /// No place known, no function that does not return.
    #[cfg(test)]
    pub fn none() -> Env<'static> {
        Env {
            base: 0x4200_0000,
            noreturn: &[],
            lenient: false,
        }
    }

    fn is_noreturn(&self, target: u32) -> bool {
        self.noreturn.binary_search(&target).is_ok()
    }
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
    /// The first jump through a register inside a frame (a jump table), if
    /// any, in a strict walk.
    pub table: Option<usize>,
    /// Whether a path from the start can leave the function by returning,
    /// jumping out or running off its end (not through a call to a function
    /// that never returns, or a trap).
    pub returns: bool,
}

/// How often one instruction may be visited before the walk gives up on it
/// (a loop that moves `sp` never settles).
const VISITS: u8 = 64;

/// The walk of one function.
struct Walk<'c> {
    code: &'c [u8],
    env: Env<'c>,
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
    /// is inside the function. A jump out of it leaves it (a tail call): the
    /// function returns by it unless it goes to a function that never does.
    fn target(&mut self, at: usize, offset: i64) -> Option<usize> {
        let t = (at as i64)
            .checked_add(offset)
            .and_then(|t| usize::try_from(t).ok())
            .filter(|&t| t >= self.start && t < self.end);
        let Some(t) = t else {
            let to = self
                .env
                .base
                .wrapping_add(at as u32)
                .wrapping_add(offset as u32);
            if !self.env.is_noreturn(to) {
                self.out.returns = true;
            }
            return None;
        };
        match self.insns.binary_search_by_key(&t, |&(a, _)| a) {
            Ok(j) => Some(j),
            // Into the middle of an instruction: not followed, and not bounded.
            Err(_) => {
                self.out.unbounded.get_or_insert(at);
                self.out.returns = true;
                None
            }
        }
    }

    /// Runs instruction `i` on its state and queues where it goes.
    fn step(&mut self, i: usize) {
        let (at, len) = self.insns[i];
        if self.visits[i] >= VISITS {
            self.out.unbounded.get_or_insert(at);
            self.out.returns = true;
            return;
        }
        self.visits[i] += 1;
        let Some(mut state) = self.states[i].clone() else {
            return;
        };
        let code = self.code;
        let pc = self.env.base.wrapping_add(at as u32);
        let effect = if len == 4 {
            wide(
                u32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]),
                &mut state,
                pc,
                &self.env,
            )
        } else {
            narrow(
                u16::from_le_bytes([code[at], code[at + 1]]),
                &mut state,
                pc,
                &self.env,
            )
        };
        if effect.unbounded {
            self.out.unbounded.get_or_insert(at);
        }
        if state.sp < -i64::from(self.out.frame) {
            self.out.frame = u32::try_from(-state.sp).unwrap_or(u32::MAX);
            self.out.frame_at = at;
        }
        let next = (i + 1 < self.insns.len()).then_some(i + 1);
        if next.is_none() && matches!(effect.ctl, Ctl::Fall | Ctl::Branch(_)) {
            // Runs on past the function's end.
            self.out.returns = true;
        }
        let goes = match effect.ctl {
            Ctl::Fall => [next, None],
            Ctl::Branch(offset) => [next, self.target(at, offset)],
            Ctl::Jump(offset) => [self.target(at, offset), None],
            Ctl::Ret => {
                self.out.returns = true;
                [None, None]
            }
            Ctl::NoReturn | Ctl::Trap => [None, None],
            Ctl::Indirect => {
                self.out.returns = true;
                if self.env.lenient {
                    // The cases are entered from the jump, with the frame
                    // pointer it had, and no register known.
                    let mut s = state.clone();
                    s.regs = [None; 32];
                    s.regs[0] = Some(0);
                    self.indirect = Some(match self.indirect.take() {
                        Some(old) => old.join(&s),
                        None => s,
                    });
                } else if state.sp != 0 {
                    // Inside a frame: where it goes is not known (a jump
                    // table's case, or the start, to grow the frame again).
                    self.out.table.get_or_insert(at);
                } else {
                    // With no frame: a tail call out of the function, or a
                    // jump anywhere in it. `sp` there is at most this (the
                    // walk keeps the lowest); the rest is not known.
                    let mut s = state.clone();
                    s.regs = [None; 32];
                    s.regs[0] = Some(0);
                    s.fp = None;
                    for j in 0..self.insns.len() {
                        self.join_into(j, &s);
                    }
                }
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
pub fn walk(code: &[u8], start: usize, end: usize, env: Env<'_>) -> Walked {
    let end = end.min(code.len());
    let mut out = Walked {
        frame: 0,
        frame_at: start,
        unbounded: None,
        table: None,
        returns: false,
    };
    let mut insns: Vec<(usize, usize)> = Vec::new();
    let mut at = start;
    if !start.is_multiple_of(2) {
        out.unbounded = Some(start);
        out.returns = true;
        return out;
    }
    while at < end {
        let len = match code.get(at..at + 2) {
            Some(h) if h[0] & 3 == 3 => 4,
            Some(_) => 2,
            None => 0,
        };
        // A range that ends inside an instruction does not hold it whole:
        // the rest of it is read by no range, which a plugin could use to hide
        // a frame.
        if len == 0 || at + len > end {
            out.unbounded = Some(at);
            out.returns = true;
            break;
        }
        insns.push((at, len));
        at += len;
    }
    let count = insns.len();
    let mut w = Walk {
        code,
        env,
        start,
        end,
        insns,
        states: vec![None; count],
        visits: vec![0; count],
        work: Vec::new(),
        indirect: None,
        out,
    };
    if count > 0 {
        w.states[0] = Some(State::entry());
        w.work.push(0);
    }
    let mut returns = None;
    loop {
        while let Some(i) = w.work.pop() {
            w.step(i);
        }
        // What the start reaches decides whether the function returns; the
        // code that no path reaches is read for its frames only.
        let returns = *returns.get_or_insert(w.out.returns);
        // Code no path reaches (dead code after a call that does not return,
        // or a switch's cases in a lenient walk): read from the state at the
        // indirect jump (or the start's), no register known.
        let Some(j) = w.states.iter().position(Option::is_none) else {
            w.out.returns = returns;
            break;
        };
        let mut seed = w.indirect.clone().unwrap_or_else(State::entry);
        seed.regs = [None; 32];
        seed.regs[0] = Some(0);
        w.states[j] = Some(seed);
        w.work.push(j);
        w.out.returns = returns;
    }
    w.out
}

/// The start addresses of the functions in `ranges` that never return,
/// proved from the code: no path from the start returns, jumps out of the
/// function (but to such a function) or runs off its end, and every call that
/// goes to such a function is the end of its path. A function is taken to
/// not return until a path says otherwise, and the rest are found by
/// repeating that until nothing changes, so that functions that only call
/// each other never to return are found, and a function that merely has a
/// name like a panic's is not trusted. Sorted.
pub fn noreturn_functions(
    code: &[u8],
    ranges: &[(usize, usize)],
    base: u32,
    lenient: bool,
) -> Vec<u32> {
    let mut set: Vec<u32> = ranges
        .iter()
        .map(|&(s, _)| base.wrapping_add(s as u32))
        .collect();
    set.sort_unstable();
    set.dedup();
    loop {
        let before = set.len();
        let kept: Vec<u32> = ranges
            .iter()
            .filter(|&&(s, _)| set.binary_search(&base.wrapping_add(s as u32)).is_ok())
            .filter(|&&(s, e)| {
                let env = Env {
                    base,
                    noreturn: &set,
                    lenient,
                };
                let w = walk(code, s, e, env);
                !w.returns
            })
            .map(|&(s, _)| base.wrapping_add(s as u32))
            .collect();
        set = kept;
        set.sort_unstable();
        set.dedup();
        if set.len() == before {
            return set;
        }
    }
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
pub fn scan_ranges(code: &[u8], ranges: &[(usize, usize)], env: Env<'_>) -> Option<Finding> {
    for &(start, end) in ranges {
        let w = walk(code, start, end, env);
        if let Some(at) = w.unbounded {
            return Some(Finding::Unknown { at });
        }
        if let Some(at) = w.table {
            return Some(Finding::Table { at });
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
    scan_ranges(code, &ranges(code, starts), Env::none())
}

/// The frame of each function in `ranges` (`None`: `sp` not bounded).
#[allow(
    dead_code,
    reason = "the tests and the firmware listing (`paperpaw-sign frames`)"
)]
pub fn frame_sizes_in(code: &[u8], ranges: &[(usize, usize)], env: Env<'_>) -> Vec<Option<u32>> {
    ranges
        .iter()
        .map(|&(s, e)| {
            let w = walk(code, s, e, env);
            (w.unbounded.is_none() && w.table.is_none()).then_some(w.frame)
        })
        .collect()
}

/// [`frame_sizes_in`] for functions that start at `starts` (sorted) and run
/// to the next.
#[cfg(test)]
pub fn frame_sizes(code: &[u8], starts: &[usize]) -> Vec<Option<u32>> {
    frame_sizes_in(code, &ranges(code, starts), Env::none())
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

/// The names in a mangled symbol (legacy `_ZN…E` or v0 `_R…`) joined with
/// `::`: the length-prefixed identifiers, without the crate's disambiguator
/// and the hash. Not a demangler: enough to match `crash::panic`.
#[allow(
    dead_code,
    reason = "the budget of the firmware's frames (`paperpaw-sign frames`)"
)]
pub fn path(mangled: &str) -> String {
    let b = mangled.as_bytes();
    let v0 = mangled.starts_with("_R");
    let mut i = if v0 {
        2
    } else if mangled.starts_with("_ZN") {
        3
    } else {
        return mangled.to_string();
    };
    let mut parts: Vec<&str> = Vec::new();
    while i < b.len() {
        // A crate's disambiguator, `s<base62>_`, after its `C`.
        if b[i] == b'C' && b.get(i + 1) == Some(&b's') {
            i += 2;
            while i < b.len() && b[i] != b'_' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let n: usize = mangled[start..i].parse().unwrap_or(0);
            // In v0 a name that starts with an underscore or a digit follows
            // the number after an underscore of its own.
            if v0 && n > 0 && b.get(i) == Some(&b'_') {
                i += 1;
            }
            let ident = i
                .checked_add(n)
                .and_then(|end| mangled.get(i..end))
                .filter(|id| n > 0 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'));
            if let Some(ident) = ident {
                // The legacy hash, `h` and sixteen hex digits.
                let hash = ident.len() == 17
                    && ident.starts_with('h')
                    && ident[1..].bytes().all(|c| c.is_ascii_hexdigit());
                if !hash {
                    parts.push(ident);
                }
                i += n;
            }
            continue;
        }
        i += 1;
    }
    parts.join("::")
}

/// Why a plugin with `finding` is refused, with the function it lies in.
fn refusal(finding: &Finding, text: &Section, symbols: &[Symbol], text_index: usize) -> String {
    let at = match finding {
        Finding::Large { at, .. }
        | Finding::Unknown { at }
        | Finding::Uncovered { at }
        | Finding::Table { at } => *at,
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
        Finding::Table { .. } => format!(
            "{place} jumps through a register inside its stack frame (a jump table, compiled \
             from a `match`): the packer cannot tell where it goes. Build the plugin with \
             `-C jump-tables=no` (the rustflags of `samples/abi7-x4-min/.cargo/config.toml` \
             set it)"
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
    let noreturn = noreturn_functions(code, &merged, text.addr, false);
    let env = Env {
        base: text.addr,
        noreturn: &noreturn,
        lenient: false,
    };
    let found = uncovered(code, &merged)
        .map(|at| Finding::Uncovered { at })
        .or_else(|| scan_ranges(code, &merged, env));
    match found {
        None => Ok(()),
        Some(found) => Err(refusal(&found, text, symbols, text_index)),
    }
}

#[cfg(test)]
mod tests;
