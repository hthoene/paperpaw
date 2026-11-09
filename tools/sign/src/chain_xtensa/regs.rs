//! What the S3 firmware scan needs to follow a call through a register: which
//! `a` register an instruction writes, where a branch goes and which
//! instructions end a run of straight-line code.
//!
//! The S3's code reaches every function through `l32r aN, <literal>` and
//! `callx8 aN` (the long call LLVM makes for a target out of the range of
//! `call8`): 71,308 `callx8` against 7,298 `call8` in an X4 Pro image. A scan
//! that cannot read the literal sees nothing of the call graph. The reading is
//! kept narrow and default-deny, as the classifier of `frames_xtensa` is: an
//! instruction not named here makes the call unresolved (it stays counted as
//! an indirect call, which the budget must list), never guessed.

/// What an instruction does to the `a` registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Writes {
    /// No `a` register.
    Nothing,
    /// This `a` register.
    Reg(u32),
    /// A call, `entry` or a return: the registers of the window are not the
    /// ones before it.
    Window,
    /// A form this reader does not name.
    Unknown,
}

/// One decoded instruction.
#[derive(Debug, Clone, Copy)]
pub(super) struct Insn {
    pub pc: u32,
    pub word: u32,
    pub wide: bool,
}

fn sext(value: u32, bits: u32) -> i64 {
    let v = i64::from(value & ((1 << bits) - 1));
    if v >> (bits - 1) != 0 {
        v - (1 << bits)
    } else {
        v
    }
}

impl Insn {
    fn len(self) -> u32 {
        if self.wide { 3 } else { 2 }
    }

    /// The `a` register of the `l32r` this is, and the address of its literal
    /// (`((pc + 3) & !3) + (0xFFFF_0000 | imm16) * 4`, as the ISA gives it).
    pub fn l32r(self) -> Option<(u32, u32)> {
        if !self.wide || self.word & 0xF != 1 {
            return None;
        }
        let imm = 0xFFFF_0000 | (self.word >> 8) & 0xFFFF;
        let base = self.pc.wrapping_add(3) & !3;
        Some(((self.word >> 4) & 0xF, base.wrapping_add(imm << 2)))
    }

    /// The register of a `callx8` or `callx12`, if this is one.
    pub fn callx(self) -> Option<u32> {
        let w = self.word;
        let t = (w >> 4) & 0xF;
        let snm0 = self.wide && w & 0xF == 0 && (w >> 12) & 0xF == 0 && (w >> 16) & 0xFF == 0;
        (snm0 && t >= 0xE).then_some((w >> 8) & 0xF)
    }

    /// The `a` registers this instruction writes.
    pub fn writes(self) -> Writes {
        if self.wide {
            wide(self.word)
        } else {
            narrow(self.word)
        }
    }

    /// Whether the instruction after this one cannot be reached by falling
    /// through (`j`, `jx`, `ret`, `retw`, `ill`): it starts a run of its own.
    pub fn ends_flow(self) -> bool {
        let w = self.word;
        if self.wide {
            let st0 = w & 0xF == 0 && (w >> 12) & 0xFFF == 0;
            let t = (w >> 4) & 0xF;
            (st0 && matches!(t, 0 | 8..=10)) || w & 0x3F == 0x06
        } else {
            // `ret.n`, `retw.n` (t = 0, 1) and `ill.n` (t = 6) of ST3.
            w & 0xF == 0xD && (w >> 12) & 0xF == 0xF && matches!((w >> 4) & 0xF, 0 | 1 | 6)
        }
    }

    /// The places a branch of this instruction goes to (up to two: a `loop`
    /// has its body and its end); `None` for a branch form this reader does
    /// not model (the caller fails closed).
    pub fn targets(self) -> Option<[Option<u32>; 2]> {
        let (pc, w) = (self.pc, self.word);
        let at = |offset: i64| Some((i64::from(pc) + offset) as u32);
        if !self.wide {
            // `beqz.n`, `bnez.n`: ST2 with bit 7 of `t` set.
            if w & 0xF == 0xC && (w >> 4) & 8 != 0 {
                let imm = (((w >> 4) & 3) << 4) | ((w >> 12) & 0xF);
                return Some([at(4 + i64::from(imm)), None]);
            }
            return Some([None, None]);
        }
        Some(match w & 0xF {
            // B: `beq`, `bltu`, `bbci`, ... : an 8-bit offset.
            7 => [at(4 + sext(w >> 16, 8)), None],
            6 => match (w >> 4) & 3 {
                // J: 18 bits (the caller reads these as the existing edge).
                0 => [at(4 + sext(w >> 6, 18)), None],
                // `beqz`, `bnez`, `bltz`, `bgez`: 12 bits.
                1 => [at(4 + sext(w >> 12, 12)), None],
                // `beqi`, `bnei`, `blti`, `bgei`: 8 bits.
                2 => [at(4 + sext(w >> 16, 8)), None],
                _ => match (w >> 6) & 3 {
                    // B1 (`r` picks the form): `bf` (0) and `bt` (1) branch by
                    // a signed 8-bit offset; `loop`, `loopnez`, `loopgtz`
                    // (8 to 10) have their body and their end (an unsigned
                    // offset); any other `r` is not modelled.
                    1 => match (w >> 12) & 0xF {
                        0 | 1 => [at(4 + sext(w >> 16, 8)), None],
                        8..=10 => [at(3), at(4 + i64::from((w >> 16) & 0xFF))],
                        _ => return None,
                    },
                    // `bltui`, `bgeui`.
                    2 | 3 => [at(4 + sext(w >> 16, 8)), None],
                    // `entry`.
                    _ => [None, None],
                },
            },
            _ => [None, None],
        })
    }

    pub fn next(self) -> u32 {
        self.pc.wrapping_add(self.len())
    }
}

/// Whether the special register `sr` (`r << 4 | s` of a `wsr` or `xsr`) names
/// the register window: `WINDOWBASE` (72), `WINDOWSTART` (73) and `PS` (230,
/// which holds the window's own increment and offset). A write to one moves
/// what the `a` registers are.
fn moves_window(sr: u32) -> bool {
    matches!(sr, 72 | 73 | 230)
}

/// The 24-bit forms, as `frames_xtensa::wide_class` names them.
fn wide(w: u32) -> Writes {
    use Writes::{Nothing, Reg, Unknown, Window};
    let op0 = w & 0xF;
    let t = (w >> 4) & 0xF;
    let s = (w >> 8) & 0xF;
    let r = (w >> 12) & 0xF;
    let op1 = (w >> 16) & 0xF;
    let op2 = (w >> 20) & 0xF;
    match op0 {
        0 => match (op1, op2) {
            // `snm0`: `ill`, `ret`, `retw`, `jx` write nothing; `callx` moves the
            // window. `movsp` (1), `sync` (2), `break` (4), `rsil` (6), `waiti` (7).
            (0, 0) => match r {
                0 => match t {
                    0 | 8..=10 => Nothing,
                    12..=15 => Window,
                    _ => Unknown,
                },
                1 | 6 => Reg(t),
                2 | 4 | 7 => Nothing,
                _ => Unknown,
            },
            // `and`, `or`, `xor`: `ar`.
            (0, 1..=3) => Reg(r),
            // `st1`: the shift-amount forms write no `a`; `rer`, `nsa`, `nsau` write `at`.
            (0, 4) => match r {
                0..=4 | 7 => Nothing,
                6 | 14 | 15 => Reg(t),
                _ => Unknown,
            },
            (0, 6) if s <= 1 => Reg(r),
            (0, 8..=15) => Reg(r),
            // RST1: shifts, `mul16`: `ar`; `xsr` swaps `at` (or moves the window).
            (1, 0..=4 | 8..=13) => Reg(r),
            (1, 6) if moves_window(r << 4 | s) => Window,
            (1, 6) => Reg(t),
            // RST2: the boolean forms write a boolean register; the rest `ar`.
            (2, 0..=4) => Nothing,
            (2, 8 | 10..=15) => Reg(r),
            // RST3: `rsr` writes `at`; `wsr` a special register; the rest `ar`;
            // `wur` a user register.
            (3, 0) => Reg(t),
            (3, 1) if moves_window(r << 4 | s) => Window,
            (3, 1) => Nothing,
            (3, 2..=14) => Reg(r),
            (3, 15) => Nothing,
            (4 | 5, _) => Reg(r),
            (8, 0 | 4) => Nothing,
            (8, 1 | 5) => Reg(s),
            (9, 0) => Reg(t),
            (9, 4) => Nothing,
            (10, 0..=2 | 4..=7 | 12 | 13) => Nothing,
            (10, 8..=11 | 14) => Reg(r),
            (10, 15) => match t {
                4 => Reg(r),
                2 => Unknown,
                _ => Nothing,
            },
            (11, 1..=13) => Nothing,
            _ => Unknown,
        },
        // `l32r`.
        1 => Reg(t),
        2 => match r {
            0 | 1 | 2 | 9..=14 => Reg(t),
            4..=7 | 15 => Nothing,
            _ => Unknown,
        },
        3 => match r {
            0 | 4 => Nothing,
            8 | 12 => Reg(s),
            _ => Unknown,
        },
        // `call0`, `call4`, `call8`, `call12`.
        5 => Window,
        6 => match ((w >> 4) & 3, (w >> 6) & 3) {
            // `entry` moves the window.
            (3, 0) => Window,
            _ => Nothing,
        },
        // `beq`, `bbci`, ...
        7 => Nothing,
        _ => Unknown,
    }
}

/// The 16-bit forms.
fn narrow(w: u32) -> Writes {
    use Writes::{Nothing, Reg, Unknown};
    let t = (w >> 4) & 0xF;
    let s = (w >> 8) & 0xF;
    let r = (w >> 12) & 0xF;
    match w & 0xF {
        8 => Reg(t),
        9 => Nothing,
        0xA | 0xB => Reg(r),
        0xC => {
            if t & 8 == 0 {
                Reg(s)
            } else {
                Nothing
            }
        }
        0xD => match (r, t) {
            (0, _) => Reg(t),
            (15, 0..=3 | 6) => Nothing,
            _ => Unknown,
        },
        _ => Unknown,
    }
}
