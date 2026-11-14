//! Measurement build only (`test-image`): how many host slots a key press and a
//! frame use, and the time inside them, by where in the core the slot is
//! (`host_slot::last_site`: file number * 10000 + line). Printed after the
//! key's handling and after the frame, then cleared (#2936).

use core::cell::RefCell;

use critical_section::Mutex;

const SITES: usize = 40;
const DEPTH: usize = 12;

struct Stat {
    key: [u32; SITES],
    count: [u32; SITES],
    micros: [u64; SITES],
    stack: [(u32, u64); DEPTH],
    depth: usize,
    outer_micros: u64,
    outer_start: u64,
    calls: u32,
    dropped: u32,
}

static STAT: Mutex<RefCell<Stat>> = Mutex::new(RefCell::new(Stat {
    key: [0; SITES],
    count: [0; SITES],
    micros: [0; SITES],
    stack: [(0, 0); DEPTH],
    depth: 0,
    outer_micros: 0,
    outer_start: 0,
    calls: 0,
    dropped: 0,
}));

/// A host slot begins (the hook of `host_slot::set`).
pub fn begin() {
    let site = paperpaw_core::native_host::host_slot::last_site();
    let t = crate::micros();
    critical_section::with(|cs| {
        let mut s = STAT.borrow_ref_mut(cs);
        s.calls += 1;
        if s.depth == 0 {
            s.outer_start = t;
        }
        let d = s.depth;
        if d < DEPTH {
            s.stack[d] = (site, t);
        }
        s.depth = d + 1;
    });
}

/// The host slot returned.
pub fn end() {
    let t = crate::micros();
    critical_section::with(|cs| {
        let mut s = STAT.borrow_ref_mut(cs);
        if s.depth == 0 {
            return;
        }
        s.depth -= 1;
        let d = s.depth;
        if d == 0 {
            s.outer_micros += t.saturating_sub(s.outer_start);
        }
        if d < DEPTH {
            let (site, t0) = s.stack[d];
            let us = t.saturating_sub(t0);
            let mut at = None;
            for i in 0..SITES {
                if s.key[i] == site || s.key[i] == 0 {
                    at = Some(i);
                    break;
                }
            }
            match at {
                Some(i) => {
                    s.key[i] = site;
                    s.count[i] += 1;
                    s.micros[i] += us;
                }
                None => s.dropped += 1,
            }
        }
    });
}

/// Prints what was counted since the last report and clears it.
pub fn report(what: &str) {
    let (calls, outer, dropped, rows) = critical_section::with(|cs| {
        let mut s = STAT.borrow_ref_mut(cs);
        let mut rows = [(0_u32, 0_u32, 0_u64); SITES];
        for i in 0..SITES {
            rows[i] = (s.key[i], s.count[i], s.micros[i]);
        }
        let out = (s.calls, s.outer_micros, s.dropped, rows);
        s.key = [0; SITES];
        s.count = [0; SITES];
        s.micros = [0; SITES];
        s.calls = 0;
        s.outer_micros = 0;
        s.dropped = 0;
        out
    });
    if calls == 0 {
        return;
    }
    log::info!(
        "hoststat {what}: {calls} host slots, {} ms inside them (outermost), {dropped} sites over",
        outer / 1000
    );
    // The biggest by inclusive time, three passes of a small table.
    let mut used = [false; SITES];
    for _ in 0..8 {
        let mut best = None;
        for i in 0..SITES {
            if !used[i] && rows[i].0 != 0 && best.is_none_or(|b: usize| rows[i].2 > rows[b].2) {
                best = Some(i);
            }
        }
        let Some(b) = best else { break };
        used[b] = true;
        log::info!(
            "hoststat {what}: site {} x{} {} ms",
            rows[b].0,
            rows[b].1,
            rows[b].2 / 1000
        );
    }
}
