//! What the walk knows before an instruction: the registers, and `sp`.

/// What a register holds, as far as the scan knows.
pub(super) type Regs = [Option<i32>; 32];

/// What a register holds of a jump table, as far as the scan follows it: the
/// code that jumps by a table is `base + index` (an add of a constant and an
/// index), a load of the word at it, a jump to that word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    None,
    /// A constant (the table's address, whole or its upper part) plus an
    /// index, which is at most the second number (in bytes) where a compare
    /// on every path to here has bounded it.
    Indexed(u32, Option<u32>),
    /// The word loaded at that address of `.rodata`, from a table whose index
    /// was bounded the same way.
    Entry(u32, Option<u32>),
}

/// The larger of two bounds: none if either path has none.
fn wider(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    a.zip(b).map(|(a, b)| a.max(b))
}

impl Tab {
    /// Where two paths meet: the same table with the wider of the two bounds,
    /// or nothing.
    fn join(self, other: Tab) -> Tab {
        match (self, other) {
            (Tab::Indexed(a, x), Tab::Indexed(b, y)) if a == b => Tab::Indexed(a, wider(x, y)),
            (Tab::Entry(a, x), Tab::Entry(b, y)) if a == b => Tab::Entry(a, wider(x, y)),
            (a, b) if a == b => a,
            _ => Tab::None,
        }
    }
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
    /// What a register is at most (unsigned), where it is not a known
    /// constant and a compare on every path to here has said so.
    pub(super) lim: [Option<u32>; 32],
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
            lim: [None; 32],
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
        self.lim = [None; 32];
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
            *t = t.join(o);
        }
        let mut lim = self.lim;
        for (l, o) in lim.iter_mut().zip(other.lim) {
            *l = wider(*l, o);
        }
        State {
            regs,
            tab,
            lim,
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
        self.lim = [None; 32];
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
            self.lim[rd] = None;
            self.written.0 |= 1 << rd;
        }
    }

    /// `rd = rs1 + rs2`: a constant if both are; a table's address plus an
    /// index if one is a constant and the other is not known at all.
    pub(super) fn add(&mut self, rd: usize, rs1: usize, rs2: usize) {
        let (a, b) = (self.regs[rs1], self.regs[rs2]);
        let value = a.zip(b).map(|(a, b)| a.wrapping_add(b));
        let tab = match (a, b, self.tab[rs1], self.tab[rs2]) {
            (Some(c), None, _, Tab::None) => Tab::Indexed(c as u32, self.lim[rs2]),
            (None, Some(c), Tab::None, _) => Tab::Indexed(c as u32, self.lim[rs1]),
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
            Tab::Indexed(b, l) => Tab::Indexed(b.wrapping_add(imm as u32), l),
            t if imm == 0 => t,
            _ => Tab::None,
        };
        let lim = if imm == 0 { self.lim[rs1] } else { None };
        self.set(rd, value);
        if rd != 0 {
            self.tab[rd] = tab;
            self.lim[rd] = lim;
        }
    }

    /// `rd = word at rs1 + imm`: the entry of a table if `rs1` is an index
    /// into one.
    pub(super) fn load(&mut self, rd: usize, rs1: usize, imm: i32) {
        let tab = match self.tab[rs1] {
            Tab::Indexed(b, l) => Tab::Entry(b.wrapping_add(imm as u32), l),
            _ => Tab::None,
        };
        self.set(rd, None);
        if rd != 0 {
            self.tab[rd] = tab;
        }
    }

    /// What `r` is at most, as an unsigned number: its constant, or the bound
    /// a compare gave it.
    fn bound(&self, r: usize) -> Option<u32> {
        self.regs[r].map(|v| v as u32).or(self.lim[r])
    }

    /// `rd = rs1 << shamt`: the bound moves with it where it does not
    /// overflow.
    pub(super) fn shift_left(&mut self, rd: usize, rs1: usize, shamt: u32) {
        let lim = self
            .bound(rs1)
            .and_then(|b| b.checked_mul(1_u32.checked_shl(shamt)?));
        self.set(rd, None);
        if rd != 0 {
            self.lim[rd] = lim;
        }
    }

    /// What a branch on an unsigned compare of `a` and `b` (`bgeu` if `ge`,
    /// else `bltu`) says on the edge that is taken or not: a register that is
    /// compared with a constant is below it, or at most it. A bound that is
    /// there already stays if it is lower.
    pub(super) fn refine(&mut self, taken: bool, ge: bool, a: usize, b: usize) {
        // The edge where `a < b`, or where `a >= b`.
        let less = taken != ge;
        let (ca, cb) = (
            self.regs[a].map(|v| v as u32),
            self.regs[b].map(|v| v as u32),
        );
        let (reg, at_most) = match (less, ca, cb) {
            (true, None, Some(cb)) if cb > 0 => (a, cb - 1),
            (false, Some(ca), None) => (b, ca),
            _ => return,
        };
        self.lim[reg] = Some(self.lim[reg].map_or(at_most, |l| l.min(at_most)));
    }
}
