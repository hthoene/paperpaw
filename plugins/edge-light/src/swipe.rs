//! Recognises a swipe along the screen's edge from the samples of one finger.
//!
//! No host, no allocation: the plugin feeds it what the input filter sees and acts
//! on what it answers. The rules:
//!
//! - A finger that goes down inside the strip along the left or right edge is
//!   watched; every other finger is none of its business.
//! - It becomes a swipe once it has moved up or down by more than the slop, and
//!   at least twice as far as it moved sideways. One that moves sideways first
//!   (a page swipe, a swipe in from the edge) is let go for good.
//! - From then on every sample says how far the finger is above the point where
//!   it became a swipe (`rise`, negative below it); the light is set from that.
//! - Lifting ends it. The kernel then recognises a vertical swipe from the same
//!   finger; [`Tracker::gesture`] says that this one is ours, so no page turns.
//!
//! Nothing here keeps a finger the filter did not see go down: the filter cannot
//! take a finger that is already moving, so a swipe that began elsewhere is never
//! entered half way.

/// The edge a swipe runs along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// What a finger does (the pointer phases the filter is told).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}

/// The screen as the strips see it, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zone {
    /// The screen's width.
    pub width: u16,
    /// The width of the strip along the left and the right edge (0: no strip).
    pub left: u16,
    pub right: u16,
    /// How far a finger may move before it is a swipe or not one.
    pub slop: u16,
}

impl Zone {
    /// The edge a finger at `x` is on, if any.
    #[must_use]
    pub fn side_at(&self, x: i32) -> Option<Side> {
        let (left, right) = (i32::from(self.left), i32::from(self.right));
        if x < left {
            Some(Side::Left)
        } else if right > 0 && x >= i32::from(self.width) - right {
            Some(Side::Right)
        } else {
            None
        }
    }
}

/// What the filter should do about one sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    /// Nothing: the finger goes on to the screens as it is.
    Pass,
    /// A swipe along `side`, `rise` pixels above where it began (`first`: the
    /// sample that made it one, `rise` 0).
    Swipe { side: Side, rise: i32, first: bool },
    /// The swipe is over (the finger lifted, was cancelled or joined by another).
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    /// Down in a strip, not yet a swipe.
    Edge {
        contact: u32,
        side: Side,
        x: i32,
        y: i32,
    },
    /// A swipe, from `y`.
    Swiping {
        contact: u32,
        side: Side,
        y: i32,
    },
}

/// The recogniser: one finger at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tracker {
    state: State,
    /// A swipe of ours just ended: the vertical swipe the kernel makes of it is ours too.
    ended: bool,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Idle,
            ended: false,
        }
    }

    /// Whether a swipe is going on.
    #[must_use]
    pub const fn swiping(&self) -> bool {
        matches!(self.state, State::Swiping { .. })
    }

    /// One sample of a finger.
    pub fn pointer(&mut self, zone: &Zone, phase: Phase, contact: u32, x: i32, y: i32) -> Out {
        match (phase, self.state) {
            (Phase::Down, state) => {
                self.ended = false;
                let was = matches!(state, State::Swiping { .. });
                self.state = match zone.side_at(x) {
                    // A second finger while one is down: not a swipe of ours.
                    Some(side) if matches!(state, State::Idle) => State::Edge {
                        contact,
                        side,
                        x,
                        y,
                    },
                    _ => State::Idle,
                };
                if was { Out::End } else { Out::Pass }
            }
            (
                Phase::Move,
                State::Edge {
                    contact: own,
                    side,
                    x: x0,
                    y: y0,
                },
            ) if own == contact => {
                let (dx, dy) = ((x - x0).abs(), (y - y0).abs());
                let slop = i32::from(zone.slop);
                if dy > slop && dy >= dx.saturating_mul(2) {
                    self.state = State::Swiping { contact, side, y };
                    Out::Swipe {
                        side,
                        rise: 0,
                        first: true,
                    }
                } else {
                    if dx > slop {
                        self.state = State::Idle;
                    }
                    Out::Pass
                }
            }
            (
                Phase::Move,
                State::Swiping {
                    contact: own,
                    side,
                    y: y0,
                },
            ) if own == contact => Out::Swipe {
                side,
                rise: y0 - y,
                first: false,
            },
            (Phase::Up | Phase::Cancel, State::Edge { contact: own, .. }) if own == contact => {
                self.state = State::Idle;
                Out::Pass
            }
            (Phase::Up | Phase::Cancel, State::Swiping { contact: own, .. }) if own == contact => {
                self.state = State::Idle;
                self.ended = phase == Phase::Up;
                Out::End
            }
            _ => Out::Pass,
        }
    }

    /// A gesture the kernel recognised: whether it is the swipe of ours that just
    /// ended (the filter takes it). Any gesture ends the claim.
    pub fn gesture(&mut self, vertical_swipe: bool) -> bool {
        let ours = self.ended && vertical_swipe;
        self.ended = false;
        ours
    }
}

/// The step the light moves in, in percent.
pub const STEP: i64 = 5;

/// The level after the finger rose `rise` pixels, from `start` (percent), where
/// `travel` pixels are the whole range: the finger's way rounded to whole steps,
/// so a finger that has not moved a step leaves the level as it was.
#[must_use]
pub fn level_at(start: i64, rise: i32, travel: u16) -> i64 {
    let moved = i64::from(rise) * 100 / i64::from(travel.max(1));
    let steps = (moved + moved.signum() * STEP / 2) / STEP;
    (start + steps * STEP).clamp(0, 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    const ZONE: Zone = Zone {
        width: 480,
        left: 40,
        right: 40,
        slop: 20,
    };

    /// Feeds a finger's samples (phase, x, y) and gives what each answered.
    fn run(tracker: &mut Tracker, samples: &[(Phase, i32, i32)]) -> Vec<Out> {
        samples
            .iter()
            .map(|&(phase, x, y)| tracker.pointer(&ZONE, phase, 0, x, y))
            .collect()
    }

    fn run_in(tracker: &mut Tracker, zone: &Zone, samples: &[(Phase, i32, i32)]) -> Vec<Out> {
        samples
            .iter()
            .map(|&(phase, x, y)| tracker.pointer(zone, phase, 0, x, y))
            .collect()
    }

    fn swipe(side: Side, rise: i32, first: bool) -> Out {
        Out::Swipe { side, rise, first }
    }

    #[test]
    fn a_vertical_finger_in_the_left_strip_is_a_swipe_that_follows_it() {
        let mut t = Tracker::new();
        let out = run(
            &mut t,
            &[
                (Phase::Down, 10, 600),
                (Phase::Move, 11, 590),
                (Phase::Move, 12, 560),
                (Phase::Move, 12, 500),
                (Phase::Move, 13, 560),
                (Phase::Up, 13, 560),
            ],
        );
        assert_eq!(
            out,
            [
                Out::Pass,
                Out::Pass,
                swipe(Side::Left, 0, true),
                swipe(Side::Left, 60, false),
                swipe(Side::Left, 0, false),
                Out::End,
            ]
        );
        // The swipe the kernel makes of it is ours.
        assert!(t.gesture(true));
        assert!(!t.gesture(true));
    }

    #[test]
    fn a_finger_in_the_right_strip_is_the_right_side() {
        let mut t = Tracker::new();
        let out = run(
            &mut t,
            &[
                (Phase::Down, 470, 100),
                (Phase::Move, 470, 150),
                (Phase::Move, 468, 200),
            ],
        );
        assert_eq!(out[1], swipe(Side::Right, 0, true));
        assert_eq!(out[2], swipe(Side::Right, -50, false));
    }

    #[test]
    fn vertical_page_swipes_away_from_the_edge_are_not_taken() {
        let mut t = Tracker::new();
        let out = run(
            &mut t,
            &[
                (Phase::Down, 240, 700),
                (Phase::Move, 240, 500),
                (Phase::Move, 240, 100),
                (Phase::Up, 240, 100),
            ],
        );
        assert!(out.iter().all(|o| *o == Out::Pass));
        assert!(!t.gesture(true));
    }

    #[test]
    fn a_finger_that_moves_sideways_first_is_let_go_for_good() {
        let mut t = Tracker::new();
        let out = run(
            &mut t,
            &[
                (Phase::Down, 10, 400),
                (Phase::Move, 60, 405),
                // Down now, but it began as a page swipe.
                (Phase::Move, 70, 700),
                (Phase::Up, 70, 700),
            ],
        );
        assert!(out.iter().all(|o| *o == Out::Pass));
        assert!(!t.gesture(true));
    }

    #[test]
    fn a_tap_and_a_short_move_in_the_strip_stay_taps() {
        let mut t = Tracker::new();
        let out = run(
            &mut t,
            &[
                (Phase::Down, 10, 400),
                (Phase::Move, 12, 410),
                (Phase::Up, 12, 410),
            ],
        );
        assert!(out.iter().all(|o| *o == Out::Pass));
        assert!(!t.swiping());
        assert!(!t.gesture(true));
    }

    #[test]
    fn a_diagonal_finger_is_no_swipe() {
        let mut t = Tracker::new();
        // As far across as up: not along the edge.
        let out = run(&mut t, &[(Phase::Down, 30, 400), (Phase::Move, 55, 440)]);
        assert_eq!(out[1], Out::Pass);
        assert!(!t.swiping());
    }

    #[test]
    fn a_second_finger_ends_the_swipe_and_a_cancel_leaves_no_claim() {
        let mut t = Tracker::new();
        run(&mut t, &[(Phase::Down, 10, 600), (Phase::Move, 10, 500)]);
        assert!(t.swiping());
        assert_eq!(t.pointer(&ZONE, Phase::Down, 1, 300, 300), Out::End);
        assert!(!t.swiping());
        assert!(!t.gesture(true));

        let mut t = Tracker::new();
        run(&mut t, &[(Phase::Down, 10, 600), (Phase::Move, 10, 500)]);
        assert_eq!(t.pointer(&ZONE, Phase::Cancel, 0, 10, 500), Out::End);
        assert!(!t.gesture(true));
    }

    #[test]
    fn only_a_vertical_swipe_straight_after_is_taken() {
        let mut t = Tracker::new();
        run(
            &mut t,
            &[
                (Phase::Down, 10, 600),
                (Phase::Move, 10, 500),
                (Phase::Up, 10, 500),
            ],
        );
        // A tap, say: not the swipe; and the claim is gone after it.
        assert!(!t.gesture(false));
        assert!(!t.gesture(true));
    }

    #[test]
    fn a_new_finger_forgets_the_claim() {
        let mut t = Tracker::new();
        run(
            &mut t,
            &[
                (Phase::Down, 10, 600),
                (Phase::Move, 10, 500),
                (Phase::Up, 10, 500),
                (Phase::Down, 240, 400),
            ],
        );
        assert!(!t.gesture(true));
    }

    #[test]
    fn a_side_that_sets_nothing_is_no_strip() {
        let mut t = Tracker::new();
        let zone = Zone { right: 0, ..ZONE };
        run_in(
            &mut t,
            &zone,
            &[
                (Phase::Down, 470, 100),
                (Phase::Move, 470, 200),
                (Phase::Up, 470, 200),
            ],
        );
        assert!(!t.gesture(true));
        let out = run_in(
            &mut t,
            &zone,
            &[(Phase::Down, 5, 100), (Phase::Move, 5, 200)],
        );
        assert_eq!(out[1], swipe(Side::Left, 0, true));
    }

    #[test]
    fn no_strip_no_swipe() {
        let mut t = Tracker::new();
        let zone = Zone {
            left: 0,
            right: 0,
            ..ZONE
        };
        assert_eq!(t.pointer(&zone, Phase::Down, 0, 0, 100), Out::Pass);
        assert_eq!(t.pointer(&zone, Phase::Move, 0, 0, 300), Out::Pass);
    }

    #[test]
    fn the_level_follows_the_finger_in_whole_steps() {
        // 480 pixels are the whole range.
        assert_eq!(level_at(40, 0, 480), 40);
        assert_eq!(level_at(40, 10, 480), 40);
        assert_eq!(level_at(40, 24, 480), 45);
        assert_eq!(level_at(40, 240, 480), 90);
        assert_eq!(level_at(40, 480, 480), 100);
        assert_eq!(level_at(40, -48, 480), 30);
        assert_eq!(level_at(40, -480, 480), 0);
        // A level that is not a step stays while the finger stays.
        assert_eq!(level_at(47, 3, 480), 47);
        assert_eq!(level_at(0, 0, 0), 0);
    }
}
