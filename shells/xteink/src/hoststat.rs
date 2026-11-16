//! Measurement build only (`test-image`): the host slots of a key press and of a
//! frame by name (`host_slot::name_of`): how many, the time inside them with
//! and without the slots nested in them, the longest, and what the counting
//! itself costs per slot. Printed after the key's handling and after the frame,
//! then cleared (#267; the earlier by-site lines could not tell the kinds apart).

use core::cell::RefCell;

use critical_section::Mutex;
use paperpaw_core::native_host::host_slot;

const N: usize = host_slot::SLOT_NAMES;
const DEPTH: usize = 12;

struct Stat {
    count: [u32; N],
    /// Microseconds from begin to end, nested slots included.
    incl: [u64; N],
    /// As `incl`, without the slots nested in it.
    excl: [u64; N],
    max: [u32; N],
    /// What is open: the slot, when it began, what its nested slots took.
    stack: [(u8, u64, u64); DEPTH],
    depth: usize,
    outer_micros: u64,
    outer_start: u64,
    calls: u32,
}

impl Stat {
    const NEW: Stat = Stat {
        count: [0; N],
        incl: [0; N],
        excl: [0; N],
        max: [0; N],
        stack: [(0, 0, 0); DEPTH],
        depth: 0,
        outer_micros: 0,
        outer_start: 0,
        calls: 0,
    };

    fn begin(&mut self, id: u8, t: u64) {
        self.calls += 1;
        if self.depth == 0 {
            self.outer_start = t;
        }
        if let Some(open) = self.stack.get_mut(self.depth) {
            *open = (id, t, 0);
        }
        self.depth += 1;
    }

    fn end(&mut self, t: u64) {
        if self.depth == 0 {
            return;
        }
        self.depth -= 1;
        if self.depth == 0 {
            self.outer_micros += t.saturating_sub(self.outer_start);
        }
        let Some(&(id, t0, nested)) = self.stack.get(self.depth) else {
            return;
        };
        let us = t.saturating_sub(t0);
        let at = usize::from(id).min(N - 1);
        self.count[at] += 1;
        self.incl[at] += us;
        self.excl[at] += us.saturating_sub(nested);
        self.max[at] = self.max[at].max(u32::try_from(us).unwrap_or(u32::MAX));
        if let Some(parent) = self
            .depth
            .checked_sub(1)
            .and_then(|d| self.stack.get_mut(d))
        {
            parent.2 += us;
        }
    }

    fn clear(&mut self) {
        let depth = self.depth;
        let stack = self.stack;
        *self = Stat::NEW;
        // Slots open now end after the clear: keep them.
        self.depth = depth;
        self.stack = stack;
    }
}

static STAT: Mutex<RefCell<Stat>> = Mutex::new(RefCell::new(Stat::NEW));
static CALIBRATION: Mutex<RefCell<Stat>> = Mutex::new(RefCell::new(Stat::NEW));
static OVERHEAD_NS: Mutex<RefCell<Option<u64>>> = Mutex::new(RefCell::new(None));

/// A host slot begins (the hook of `host_slot::set`).
pub fn begin() {
    let id = host_slot::last_id();
    let t = crate::micros();
    critical_section::with(|cs| STAT.borrow_ref_mut(cs).begin(id, t));
}

/// The host slot returned.
pub fn end() {
    let t = crate::micros();
    critical_section::with(|cs| STAT.borrow_ref_mut(cs).end(t));
}

/// What one begin and end pair costs, in nanoseconds: the counting is part of
/// every number below, and is taken from none of them.
fn overhead_ns() -> u64 {
    if let Some(known) = critical_section::with(|cs| *OVERHEAD_NS.borrow_ref(cs)) {
        return known;
    }
    const PAIRS: u64 = 500;
    let start = crate::micros();
    for _ in 0..PAIRS {
        let id = host_slot::last_id();
        let t = crate::micros();
        critical_section::with(|cs| CALIBRATION.borrow_ref_mut(cs).begin(id, t));
        let t = crate::micros();
        critical_section::with(|cs| CALIBRATION.borrow_ref_mut(cs).end(t));
    }
    let ns = crate::micros().saturating_sub(start) * 1000 / PAIRS;
    critical_section::with(|cs| {
        CALIBRATION.borrow_ref_mut(cs).clear();
        *OVERHEAD_NS.borrow_ref_mut(cs) = Some(ns);
    });
    ns
}

/// Prints what was counted since the last report and clears it.
pub fn report(what: &str) {
    let overhead = overhead_ns();
    let (calls, outer, rows) = critical_section::with(|cs| {
        let mut s = STAT.borrow_ref_mut(cs);
        let mut rows = [(0_u32, 0_u64, 0_u64, 0_u32); N];
        for (i, row) in rows.iter_mut().enumerate() {
            *row = (s.count[i], s.incl[i], s.excl[i], s.max[i]);
        }
        let out = (s.calls, s.outer_micros, rows);
        s.clear();
        out
    });
    if calls == 0 {
        return;
    }
    log::info!(
        "hoststat {what}: {calls} slots, {} ms inside them (outermost); counting costs {} ns per slot, {} ms of it",
        outer / 1000,
        overhead,
        u64::from(calls) * overhead / 1_000_000
    );
    // The biggest by time without the nested slots.
    let mut used = [false; N];
    for _ in 0..16 {
        let mut best: Option<usize> = None;
        for (i, row) in rows.iter().enumerate() {
            if !used[i] && row.0 > 0 && best.is_none_or(|b| row.2 > rows[b].2) {
                best = Some(i);
            }
        }
        let Some(b) = best else { break };
        used[b] = true;
        let (n, incl, excl, max) = rows[b];
        log::info!(
            "hoststat {what}: {} x{n}: {} ms alone ({} us each), {} ms with nested, longest {} ms",
            host_slot::name_of(u8::try_from(b).unwrap_or(u8::MAX)),
            excl / 1000,
            excl / u64::from(n),
            incl / 1000,
            max / 1000
        );
    }
}
