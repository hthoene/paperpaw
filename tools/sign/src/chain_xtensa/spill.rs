//! A function address LLVM keeps in the function's own stack frame (#445).
//!
//! Across calls LLVM often spills the address of a direct callee that an
//! `l32r` loaded to a word of the frame (`s32i aN, a1, off`) and reloads it
//! before the `callx8` (`l32i aM, a1, off`). [`super::resolve`] follows such a
//! word as it follows a register: a frame word holds a literal where every path
//! stored that literal's value there and nothing could have changed it since.
//!
//! Only this function writes its frame through `a1` (the window spills of the
//! hardware use the words the compiler leaves to them). Anything else reaches
//! the frame only through an address formed from `a1`, the function's own
//! `addi aN, a1, k` or `addmi`, which a callee or another store may then use:
//! such a pointer points at a stack object from offset `k` up, and a word of the
//! frame is followed only below the lowest such `k` ([`floor`]). Any other use of
//! `a1` (a copy, an add of a register, a store of it, a write of it, a form this
//! reader does not name) means no word of the frame is followed: the reading
//! fails closed and the call stays indirect. The words a callee takes its
//! stack arguments from are written right before a call and never read back as
//! a spill after it.

use std::collections::BTreeMap;

use super::regs::{Insn, Writes};
use super::{Held, Regs};

/// The words of the frame known to hold a literal's value: offset from `a1`
/// to the literal's address.
pub(super) type Slots = BTreeMap<u32, u32>;

/// What an instruction does with the frame through `a1` as its base.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Frame {
    /// `l32i`/`l32i.n` of this register from this offset.
    Load(u32, u32),
    /// `s32i`/`s32i.n` of this register to this offset.
    Store(u32, u32),
    /// `s8i`/`s16i`/`ssi` of this width to this offset.
    Clobber(u32, u32),
    /// `addi`/`addmi`/`addi.n`: an address of the frame at this offset.
    Address(i64),
    /// `add`/`add.n` of `a1` and this register: an address of the frame at
    /// the offset the register holds.
    Add(u32),
    /// `entry`, the frame's start.
    Entry,
    /// No use of `a1`.
    None,
    /// A use of `a1` this reader does not model.
    Other,
}

/// Which fields of an RRR form (`op0` 0) are `a` registers: `R`, `S`, `T`.
/// Where a field is an immediate or another register file, leaving it out
/// only keeps a form from being taken as a use of `a1`; a form `writes` does
/// not name never gets here (it gives the frame up first).
fn rrr(op1: u32, op2: u32, r: u32, t: u32) -> u8 {
    const R: u8 = 1;
    const S: u8 = 2;
    const T: u8 = 4;
    match (op1, op2) {
        (0, 0) => match r {
            0 => S,
            1 | 6 => S | T,
            _ => 0,
        },
        (0, 4) | (8 | 9, _) => S | T,
        (0, 6) | (1, 2..=4 | 9 | 11) | (4 | 5, _) => R | T,
        (1, 0 | 1 | 10) | (3, 2 | 3 | 12 | 13) => R | S,
        (1, 6) | (3, 0 | 1 | 15) | (11, 8..=11) => T,
        (3, 14) | (10, 8..=11 | 14) => R,
        (10, 12 | 13) => S,
        (10, 15) => match t {
            4 => R,
            5 => S,
            _ => 0,
        },
        (2, 0..=4) | (10 | 11, _) => 0,
        _ => R | S | T,
    }
}

fn frame(i: Insn) -> Frame {
    let w = i.word;
    let (t, s, r) = ((w >> 4) & 0xF, (w >> 8) & 0xF, (w >> 12) & 0xF);
    let imm8 = (w >> 16) & 0xFF;
    let sext8 = |v: u32| i64::from(v as u8 as i8);
    let uses = |regs: &[u32]| {
        if regs.contains(&1) {
            Frame::Other
        } else {
            Frame::None
        }
    };
    let add = |other: u32| {
        if r != 1 && (s == 1) != (t == 1) {
            Frame::Add(other)
        } else {
            uses(&[r, s, t])
        }
    };
    let other = if s == 1 { t } else { s };
    if !i.wide {
        return match w & 0xF {
            8 if s == 1 && t != 1 => Frame::Load(t, r * 4),
            9 if s == 1 && t != 1 => Frame::Store(t, r * 4),
            // `l32i.n`/`s32i.n` (their offset in `r`).
            8 | 9 => uses(&[s, t]),
            0xA => add(other),
            0xB if s == 1 && r != 1 => Frame::Address(if t == 0 { -1 } else { i64::from(t) }),
            0xB => uses(&[r, s]),
            // `movi.n` (its register in `s`), `beqz.n`/`bnez.n`.
            0xC => uses(&[s]),
            0xD if r == 0 => uses(&[s, t]),
            0xD if r == 15 => Frame::None,
            _ => Frame::Other,
        };
    }
    let (op1, op2) = ((w >> 16) & 0xF, (w >> 20) & 0xF);
    match w & 0xF {
        0 if (op1, op2) == (0, 8) => add(other),
        0 => {
            let fields = rrr(op1, op2, r, t);
            let named = [(1, r), (2, s), (4, t)];
            uses(&named.map(|(bit, f)| if fields & bit != 0 { f } else { 0 }))
        }
        // `l32r`: only its register.
        1 => uses(&[t]),
        2 => match r {
            _ if s != 1 || t == 1 => match r {
                10 => uses(&[t]),
                _ => uses(&[s, t]),
            },
            2 => Frame::Load(t, imm8 * 4),
            0 | 1 | 9 | 10 => Frame::None,
            6 => Frame::Store(t, imm8 * 4),
            4 => Frame::Clobber(1, imm8),
            5 => Frame::Clobber(2, imm8 * 2),
            12 => Frame::Address(sext8(imm8)),
            13 => Frame::Address(sext8(imm8) << 8),
            _ => Frame::Other,
        },
        // Float loads and stores (`t` a float register): `lsi`, `ssi`.
        3 if s != 1 => Frame::None,
        3 => match r {
            0 => Frame::None,
            4 => Frame::Clobber(4, imm8 * 4),
            _ => Frame::Other,
        },
        // CALLn.
        5 => Frame::None,
        6 => match ((w >> 4) & 3, (w >> 6) & 3) {
            // `j`.
            (0, _) => Frame::None,
            (3, 0) if s == 1 => Frame::Entry,
            (3, 1) if r <= 1 => Frame::None,
            _ => uses(&[s]),
        },
        // Branches: `r` is the condition.
        7 => uses(&[s, t]),
        _ => Frame::Other,
    }
}

/// The value a `movi`/`movi.n` gives.
fn movi(i: Insn) -> Option<i64> {
    let w = i.word;
    let (t, s, r) = ((w >> 4) & 0xF, (w >> 8) & 0xF, (w >> 12) & 0xF);
    if i.wide && w & 0xF == 2 && r == 10 {
        let imm = (s << 8 | (w >> 16) & 0xFF) as i64;
        return Some(if imm >= 0x800 { imm - 0x1000 } else { imm });
    }
    if !i.wide && w & 0xF == 0xC && t & 8 == 0 {
        let imm = i64::from((t & 7) << 4 | r);
        return Some(if imm >= 96 { imm - 128 } else { imm });
    }
    None
}

/// The constant `reg` holds at instruction `n`: a `movi` of it before `n` on
/// the one path there (no instruction from after it to `n` is reached other
/// than from the instruction before it).
fn constant(insns: &[Insn], joined: &[bool], n: usize, reg: u32) -> Option<i64> {
    let mut m = n;
    while !joined[m] {
        m -= 1;
        match insns[m].writes() {
            Writes::Reg(r) if r == reg => return movi(insns[m]),
            Writes::Nothing | Writes::Reg(_) => {}
            Writes::Call(rotation) if reg < rotation => {}
            _ => return None,
        }
    }
    None
}

/// The lowest offset of the frame an address formed from `a1` may point at:
/// the frame's words below it are this function's own. 0 where `a1` has a use
/// this reader does not model, or is written, or the window moves other than by
/// the function's `entry` and its return. `next` are the flow's edges, `entries`
/// the places a jump table goes to.
pub(super) fn floor(insns: &[Insn], next: &[Vec<usize>], entries: &[usize]) -> u32 {
    let mut joined = vec![false; insns.len()];
    joined[0] = true;
    for &e in entries {
        joined[e] = true;
    }
    for (k, tos) in next.iter().enumerate() {
        for &to in tos.iter().filter(|&&to| to != k + 1) {
            joined[to] = true;
        }
    }
    let mut floor = u32::MAX;
    for (n, &i) in insns.iter().enumerate() {
        let writes = i.writes();
        if writes == Writes::Reg(1)
            || writes == Writes::Unknown
            || (writes == Writes::Window && n > 0)
        {
            return 0;
        }
        let at = match frame(i) {
            Frame::Address(k) => k,
            Frame::Add(reg) => match constant(insns, &joined, n, reg) {
                Some(k) => k,
                None => return 0,
            },
            Frame::Entry if n == 0 => continue,
            Frame::Entry | Frame::Other => return 0,
            _ => continue,
        };
        floor = floor.min(u32::try_from(at).unwrap_or(0));
    }
    floor
}

/// What `i` leaves in the registers and the frame's words. A write of a
/// register makes it unknown, or the literal of an `l32r`; a call makes the
/// registers from its rotation up unknown, a move of the window or a form not
/// named all of them. Then the frame: a store of a register that holds a
/// literal makes its word hold it (below `floor` only), any other store of a
/// word forgets it, and a load of a word that holds a literal makes the
/// register hold it.
pub(super) fn step(i: Insn, floor: u32, held: &mut Regs, slots: &mut Slots) {
    match i.writes() {
        Writes::Nothing => {}
        Writes::Reg(r) => {
            held[r as usize] = match i.l32r() {
                Some((_, literal)) => Held::Literal(literal),
                None => Held::Unknown,
            };
        }
        Writes::Call(rotation) => {
            for h in &mut held[rotation as usize..] {
                *h = Held::Unknown;
            }
        }
        Writes::Window | Writes::Unknown => *held = [Held::Unknown; 16],
    }
    let mut forget = |at: u32, len: u32| slots.retain(|&k, _| k + 4 <= at || at + len <= k);
    match frame(i) {
        Frame::Store(reg, at) => {
            forget(at, 4);
            if let Held::Literal(literal) = held[reg as usize]
                && at + 4 <= floor
            {
                slots.insert(at, literal);
            }
        }
        Frame::Clobber(len, at) => forget(at, len),
        Frame::Load(reg, at) => {
            if let Some(&literal) = slots.get(&at) {
                held[reg as usize] = Held::Literal(literal);
            }
        }
        _ => {}
    }
}

/// The words two paths agree on.
pub(super) fn join(old: &Slots, new: &Slots) -> Slots {
    old.iter()
        .filter(|(k, v)| new.get(k) == Some(v))
        .map(|(&k, &v)| (k, v))
        .collect()
}

#[cfg(test)]
#[path = "tests_spill.rs"]
mod tests;
