//! The touch lock without the host: while it is on, taps, swipes and the pen
//! do nothing, so that a hand resting on the screen turns no page; the buttons
//! go on working. The way out needs no menu and is not made by a resting
//! hand: one finger taps, then comes down again at the same spot and stays
//! there for `HOLD_MS` (tap twice and hold the second tap); after two taps
//! the third touch held counts too. A touch that is no step of the way out
//! says how to unlock now and then.
use alloc::vec::Vec;
use paperpaw_sdk::Point;

/// A tap is shorter than this, and the next touch comes within [`GAP_MS`]:
/// both allow for a slow screen.
pub const TAP_MS: u64 = 500;
pub const GAP_MS: u64 = 1_000;
pub const HOLD_MS: u64 = 1_500;
/// A touch that is no step says how to unlock at most this often.
pub const HINT_MS: u64 = 10_000;
/// How far the finger may stray from the first tap, in pixels.
const SLOP: i32 = 40;
/// Fingers followed at once; more are not counted (a hand resting).
const FINGERS: usize = 16;

/// What a pointer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}

/// One finger or the pen at a time `now_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Touch {
    pub id: u32,
    pub phase: Phase,
    pub point: Point,
}

/// What the filter answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Not a touch: it goes on.
    Pass,
    /// The touch is taken.
    Take,
    /// Taken, and the way out is done: the lock switches.
    Unlock,
    /// Taken, and how to unlock is said.
    Hint,
}

/// How far the way out has come.
#[derive(Debug, Default)]
pub struct Escape {
    /// The fingers down now, by id.
    down: Vec<u32>,
    /// The finger that may go on with it: its id, when and where it came down.
    finger: Option<(u32, u64)>,
    /// Taps so far (two at most), where the first was, when the last ended.
    taps: u8,
    first: Option<Point>,
    last_up: Option<u64>,
}

impl Escape {
    fn over(&mut self) {
        self.finger = None;
        self.taps = 0;
        self.first = None;
        self.last_up = None;
    }

    fn near(&self, point: Point) -> bool {
        self.first.is_none_or(|at| {
            (point.x.saturating_sub(at.x)).abs() <= SLOP
                && (point.y.saturating_sub(at.y)).abs() <= SLOP
        })
    }

    /// Follows `touch` at `now`: whether it completed the way out.
    pub fn follow(&mut self, now: u64, touch: &Touch) -> bool {
        let id = touch.id;
        match touch.phase {
            Phase::Down => {
                if !self.down.contains(&id) && self.down.len() < FINGERS {
                    // Out of room: the tracking resumes at the next free slot.
                    if self.down.try_reserve(1).is_ok() {
                        self.down.push(id);
                    }
                }
                let late = self
                    .last_up
                    .is_some_and(|up| now.saturating_sub(up) > GAP_MS);
                if self.down.len() > 1 || late || !self.near(touch.point) {
                    self.over();
                }
                if self.down.len() == 1 {
                    self.finger = Some((id, now));
                    self.first.get_or_insert(touch.point);
                }
            }
            Phase::Move => {
                let Some((f, at)) = self.finger else {
                    return false;
                };
                if f != id {
                    return false;
                }
                if !self.near(touch.point) {
                    self.over();
                } else if self.taps > 0 && now.saturating_sub(at) >= HOLD_MS {
                    // Held long enough, still down: no need to lift.
                    self.down.clear();
                    self.over();
                    return true;
                }
            }
            Phase::Up => {
                self.down.retain(|d| *d != id);
                let Some((f, at)) = self.finger.take() else {
                    return false;
                };
                if f != id {
                    self.over();
                    return false;
                }
                let held = now.saturating_sub(at);
                if held < TAP_MS && self.taps < 2 {
                    self.taps += 1;
                    self.last_up = Some(now);
                } else if held >= HOLD_MS && self.taps > 0 {
                    self.down.clear();
                    self.over();
                    return true;
                } else {
                    self.over();
                }
            }
            // A cancel ends every touch: a shell may send it for one pointer
            // alone, and the others left would count as a second finger from
            // then on.
            Phase::Cancel => {
                self.down.clear();
                self.over();
            }
        }
        false
    }

    /// What the filter does with `touch` while touch is locked: taken, the
    /// unlock once the way out is done, or how to unlock said where it was not
    /// said for [`HINT_MS`] and the touch is no step of the way out: one that
    /// ends without being one of its taps (a swipe, a hand resting, a hold too
    /// short), or one that comes long after a tap (the way out given up).
    /// Never between its steps: the refresh would hold the next back.
    pub fn locked(&mut self, now: u64, touch: &Touch, hinted: &mut Option<u64>) -> Verdict {
        let taps = self.taps;
        let given_up = self
            .last_up
            .is_some_and(|up| now.saturating_sub(up) > GAP_MS);
        if self.follow(now, touch) {
            return Verdict::Unlock;
        }
        let no_step = match touch.phase {
            Phase::Up => self.taps <= taps && self.finger.is_none(),
            Phase::Down => taps > 0 && given_up,
            _ => false,
        };
        if no_step && hinted.is_none_or(|at| now.saturating_sub(at) >= HINT_MS) {
            *hinted = Some(now);
            return Verdict::Hint;
        }
        Verdict::Take
    }
}

#[cfg(test)]
mod tests;
