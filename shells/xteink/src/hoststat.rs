//! Measurement build only (`test-image`): how many host slots a key press and a
//! frame use, and the time inside them, by where in the core the slot is
//! (`host_slot::last_site`: file number * 10000 + line). Printed after the
//! key's handling and after the frame, then cleared (#2936).

use core::cell::RefCell;

use critical_section::Mutex;

const SITES: usize = 40;
const DEPTH: usize = 12;

/// What the card did inside one host slot site: commands, blocks, microseconds
/// of reads and of writes (the innermost open slot at the time).
#[derive(Clone, Copy)]
struct CardRow {
    site: u32,
    read: [u64; 3],
    write: [u64; 3],
}

const CARD_SITES: usize = 6;
const NO_CARD: CardRow = CardRow {
    site: 0,
    read: [0; 3],
    write: [0; 3],
};

struct Stat {
    card: [CardRow; CARD_SITES],
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
    card: [NO_CARD; CARD_SITES],
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
    let site = paperpaw_core::native_host::host_slot::last_site()
        | (u32::from(paperpaw_core::native_host::host_slot::last_id()) << 24);
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

/// The card did a read or a write of `blocks` blocks in `micros` (counted for
/// the innermost open host slot; site 1 where none is open).
pub fn card(write: bool, blocks: u32, micros: u64) {
    critical_section::with(|cs| {
        let mut s = STAT.borrow_ref_mut(cs);
        let site = if s.depth > 0 && s.depth <= DEPTH {
            s.stack[s.depth - 1].0 & 0xFF_FFFF
        } else {
            1
        };
        for i in 0..CARD_SITES {
            if s.card[i].site == site || s.card[i].site == 0 {
                let row = &mut s.card[i];
                row.site = site;
                let c = if write { &mut row.write } else { &mut row.read };
                c[0] += 1;
                c[1] += u64::from(blocks);
                c[2] += micros;
                return;
            }
        }
    });
}

/// Forgets what was counted (before a key's handling: the pump's work between
/// two presses is not the key's).
pub fn clear() {
    critical_section::with(|cs| {
        let mut s = STAT.borrow_ref_mut(cs);
        s.key = [0; SITES];
        s.count = [0; SITES];
        s.micros = [0; SITES];
        s.card = [NO_CARD; CARD_SITES];
        s.calls = 0;
        s.outer_micros = 0;
        s.dropped = 0;
    });
}

/// Prints what was counted since the last report and clears it.
pub fn report(what: &str) {
    let (calls, outer, dropped, rows, card) = critical_section::with(|cs| {
        let mut s = STAT.borrow_ref_mut(cs);
        let card = s.card;
        s.card = [NO_CARD; CARD_SITES];
        let mut rows = [(0_u32, 0_u32, 0_u64); SITES];
        for i in 0..SITES {
            rows[i] = (s.key[i], s.count[i], s.micros[i]);
        }
        let out = (s.calls, s.outer_micros, s.dropped, rows, card);
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
    for row in card.iter().filter(|row| row.site != 0) {
        log::info!(
            "hoststat {what}: card in site {}: reads {} cmds {} blocks {} ms, writes {} cmds {} blocks {} ms",
            row.site,
            row.read[0],
            row.read[1],
            row.read[2] / 1000,
            row.write[0],
            row.write[1],
            row.write[2] / 1000
        );
    }
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
            "hoststat {what}: site {} {} x{} {} ms",
            rows[b].0 & 0xFF_FFFF,
            paperpaw_core::native_host::host_slot::name_of((rows[b].0 >> 24) as u8),
            rows[b].1,
            rows[b].2 / 1000
        );
    }
}
