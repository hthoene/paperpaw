//! The clock and the day's new cards.
//! Part of [`cards`](super); moved out of it to keep files short.

use super::*;

/// What `state.txt` says: the time last known, and the new cards shown
/// on a day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Kept {
    pub(super) clock: i64,
    pub(super) day: i64,
    pub(super) new: u32,
}

pub(super) fn kept(storage: &dyn Storage) -> Kept {
    let mut kept = Kept::default();
    let _ = each_line(storage, STATE, |_, line| {
        let mut words = line.split(' ');
        match (words.next(), words.next(), words.next()) {
            (Some("clock"), Some(t), _) => kept.clock = t.parse().unwrap_or(0),
            (Some("new"), Some(day), Some(n)) => {
                kept.day = day.parse().unwrap_or(0);
                kept.new = n.parse().unwrap_or(0);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
    kept
}

pub(super) fn write_kept(storage: &dyn Storage, kept: Kept) {
    let mut text = String::new();
    let made = push(&mut text, "clock ")
        .and_then(|()| push_signed(&mut text, kept.clock))
        .and_then(|()| push(&mut text, "\nnew "))
        .and_then(|()| push_signed(&mut text, kept.day))
        .and_then(|()| push(&mut text, " "))
        .and_then(|()| push_number(&mut text, kept.new))
        .and_then(|()| push(&mut text, "\n"));
    if made.is_ok() {
        let _ = write_whole(storage, STATE, text.as_bytes());
    }
}

pub(super) fn remember_clock(storage: &dyn Storage, now: i64) {
    let mut k = kept(storage);
    if now > k.clock {
        k.clock = now;
        write_kept(storage, k);
    }
}

/// The time now when the device tells it (`unix`), else the last one known.
pub(crate) fn now_of(storage: &dyn Storage, unix: Option<i64>) -> i64 {
    unix.unwrap_or_else(|| kept(storage).clock)
}

/// The local day a time is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Day {
    /// Days since 1970, by the local time.
    pub number: i64,
    /// When it ends, in seconds since 1970: what is due by then is due
    /// today.
    pub end: i64,
}

impl Day {
    /// The day `now` is in, where the local time is `local`.
    pub(crate) fn of(now: i64, local: i64) -> Day {
        Day {
            number: local.div_euclid(DAY),
            end: now + (DAY - local.rem_euclid(DAY)),
        }
    }
}

/// How many new cards may still be shown on `day`, `per_day` a day.
pub(crate) fn new_left(storage: &dyn Storage, day: Day, per_day: u32) -> usize {
    let k = kept(storage);
    let shown = if k.day == day.number { k.new } else { 0 };
    per_day.saturating_sub(shown) as usize
}

/// A new card was shown (and answered) on `day`, at `now`.
pub(crate) fn new_shown(storage: &dyn Storage, day: Day, now: i64) {
    let mut k = kept(storage);
    if k.day != day.number {
        k.day = day.number;
        k.new = 0;
    }
    k.new += 1;
    k.clock = k.clock.max(now);
    write_kept(storage, k);
}
