//! What the walk knows before an instruction: the registers, and `sp`.

/// What a register holds, as far as the scan knows.
pub(super) type Regs = [Option<i32>; 32];

/// What a register holds of a jump table, as far as the scan follows it: the
/// code that jumps by a table is `base + index` (an add of a constant and an
/// index), a load of the word at it, a jump to that word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    None,
    /// A constant (the table's address, whole or its upper part) plus an index.
    Indexed(u32),
    /// The word loaded at that address of `.rodata`.
    Entry(u32),
}

/// The registers an instruction sequence wrote, as a mask (bit `r` for
/// `x{r}`). Not part of what two states are equal by: it only reports what a
/// sequence did, it is not something the walk follows.
#[derive(Clone, Copy, Default)]
pub(super) struct Written(pub(super) u32);

impl PartialEq for Written {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Written {}

/// What the walk knows before an instruction.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct State {
    pub(super) regs: Regs,
    /// What the registers hold of a jump table.
    pub(super) tab: [Tab; 32],
    /// `sp` relative to the function's start (negative: the frame).
    pub(super) sp: i64,
    /// `s0` as `sp` was when it was set from it, relative to the start;
    /// `None` when it is not known to be a frame pointer.
    pub(super) fp: Option<i64>,
    /// Every register written since the start, by its destination register
    /// (what `set` is told), not by comparing values.
    pub(super) written: Written,
}

impl State {
    /// The state at a function's start.
    pub(super) fn entry() -> Self {
        let mut regs: Regs = [None; 32];
        regs[0] = Some(0);
        State {
            regs,
            tab: [Tab::None; 32],
            sp: 0,
            fp: None,
            written: Written(0),
        }
    }

    /// No register known (but zero), as at a jump's targets.
    pub(super) fn forget_regs(&mut self) {
        self.regs = [None; 32];
        self.regs[0] = Some(0);
        self.tab = [Tab::None; 32];
    }

    /// Where paths meet: a register that differs is unknown, the frame
    /// pointer is kept only if it is the same, `sp` is the lowest.
    pub(super) fn join(&self, other: &State) -> State {
        let mut regs = self.regs;
        for (r, o) in regs.iter_mut().zip(other.regs) {
            if *r != o {
                *r = None;
            }
        }
        let mut tab = self.tab;
        for (t, o) in tab.iter_mut().zip(other.tab) {
            if *t != o {
                *t = Tab::None;
            }
        }
        State {
            regs,
            tab,
            sp: self.sp.min(other.sp),
            fp: if self.fp == other.fp { self.fp } else { None },
            written: Written(self.written.0 | other.written.0),
        }
    }

    /// After a call that links in `link`: what a callee may change is unknown;
    /// the callee-saved registers (`s0` to `s11`) and `sp` are kept, as the
    /// calling convention keeps them (a table's base in `s8`, set before a
    /// call, still holds after it). A call that links in `t0` is an outlined
    /// sequence of the compiler's (a prologue's or epilogue's saves and
    /// restores), which sets callee-saved registers: all are forgotten then,
    /// but `s0` (as before).
    pub(super) fn called(&mut self, link: usize) {
        let kept = self.regs;
        let kept_tab = self.tab;
        self.regs = [None; 32];
        self.tab = [Tab::None; 32];
        self.regs[0] = kept[0];
        self.regs[8] = kept[8];
        if link != 5 {
            for r in [9, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27] {
                self.regs[r] = kept[r];
                self.tab[r] = kept_tab[r];
            }
            self.tab[8] = kept_tab[8];
        }
    }

    pub(super) fn set(&mut self, rd: usize, value: Option<i32>) {
        if rd == 8 {
            // Written otherwise than as the frame pointer's set: not one.
            self.fp = None;
        }
        if rd != 0 {
            self.regs[rd] = value;
            self.tab[rd] = Tab::None;
            self.written.0 |= 1 << rd;
        }
    }

    /// `rd = rs1 + rs2`: a constant if both are; a table's address plus an
    /// index if one is a constant and the other is not known at all.
    pub(super) fn add(&mut self, rd: usize, rs1: usize, rs2: usize) {
        let (a, b) = (self.regs[rs1], self.regs[rs2]);
        let value = a.zip(b).map(|(a, b)| a.wrapping_add(b));
        let tab = match (a, b, self.tab[rs1], self.tab[rs2]) {
            (Some(c), None, _, Tab::None) | (None, Some(c), Tab::None, _) => Tab::Indexed(c as u32),
            _ => Tab::None,
        };
        self.set(rd, value);
        if rd != 0 {
            self.tab[rd] = tab;
        }
    }

    /// `rd = rs1 + imm`.
    pub(super) fn add_imm(&mut self, rd: usize, rs1: usize, imm: i32) {
        let value = self.regs[rs1].map(|v| v.wrapping_add(imm));
        let tab = match self.tab[rs1] {
            Tab::Indexed(b) => Tab::Indexed(b.wrapping_add(imm as u32)),
            t if imm == 0 => t,
            _ => Tab::None,
        };
        self.set(rd, value);
        if rd != 0 {
            self.tab[rd] = tab;
        }
    }

    /// `rd = word at rs1 + imm`: the entry of a table if `rs1` is an index
    /// into one.
    pub(super) fn load(&mut self, rd: usize, rs1: usize, imm: i32) {
        let tab = match self.tab[rs1] {
            Tab::Indexed(b) => Tab::Entry(b.wrapping_add(imm as u32)),
            _ => Tab::None,
        };
        self.set(rd, None);
        if rd != 0 {
            self.tab[rd] = tab;
        }
    }
}

