#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]
use super::*;
use alloc::vec::Vec;

fn finger(id: u32, phase: Phase, (x, y): (i32, i32)) -> Touch {
    Touch {
        id,
        phase,
        point: Point::new(x, y),
    }
}

type Step = (u64, u32, Phase, (i32, i32));

/// Runs `steps` (when, which finger, what, where): the verdicts, the hints
/// left out (as taken: they change nothing else).
fn run(steps: &[Step]) -> Vec<Verdict> {
    run_with_hints(steps)
        .into_iter()
        .map(|v| match v {
            Verdict::Hint => Verdict::Take,
            v => v,
        })
        .collect()
}

fn run_with_hints(steps: &[Step]) -> Vec<Verdict> {
    let mut escape = Escape::default();
    let mut hinted = None;
    steps
        .iter()
        .map(|&(at, id, phase, point)| escape.locked(at, &finger(id, phase, point), &mut hinted))
        .collect()
}

use Phase::{Cancel, Down, Move, Up};
const AT: (i32, i32) = (200, 300);

#[test]
fn a_tap_then_a_held_tap_unlock() {
    let verdicts = run(&[
        (0, 0, Down, AT),
        (100, 0, Up, AT),
        (400, 0, Down, (205, 302)),
        (400 + HOLD_MS, 0, Up, (205, 302)),
    ]);
    assert_eq!(verdicts.last(), Some(&Verdict::Unlock));
    assert!(verdicts[..3].iter().all(|v| *v == Verdict::Take));
    // Still down, once held long enough: a little movement unlocks.
    let held = run(&[
        (0, 0, Down, AT),
        (100, 0, Up, AT),
        (400, 0, Down, AT),
        (400 + HOLD_MS + 10, 0, Move, (201, 301)),
    ]);
    assert_eq!(held.last(), Some(&Verdict::Unlock));
}

#[test]
fn a_resting_hand_does_not_unlock() {
    // A palm still for a long while, then lifted.
    assert!(!run(&[(0, 0, Down, AT), (10_000, 0, Up, AT)]).contains(&Verdict::Unlock));
    // A palm and a thumb: one lifts after the other's taps and hold.
    let verdicts = run(&[
        (0, 0, Down, AT),
        (100, 1, Down, (300, 600)),
        (150, 1, Up, (300, 600)),
        (400, 1, Down, (300, 600)),
        (450, 1, Up, (300, 600)),
        (700, 1, Down, (300, 600)),
        (700 + HOLD_MS, 1, Up, (300, 600)),
        (5_000, 0, Up, AT),
    ]);
    assert!(!verdicts.contains(&Verdict::Unlock), "{verdicts:?}");
}

#[test]
fn taps_too_slow_or_too_far_apart_do_not_unlock() {
    // The held tap coming too late after the tap: a hand resting.
    let slow = run(&[
        (0, 0, Down, AT),
        (100, 0, Up, AT),
        (100 + GAP_MS + 1, 0, Down, AT),
        (100 + GAP_MS + 1 + HOLD_MS, 0, Up, AT),
    ]);
    assert!(!slow.contains(&Verdict::Unlock));
    // The held tap far from the tap.
    let far = run(&[
        (0, 0, Down, AT),
        (100, 0, Up, AT),
        (400, 0, Down, (400, 700)),
        (400 + HOLD_MS, 0, Up, (400, 700)),
    ]);
    assert!(!far.contains(&Verdict::Unlock));
    let moved = run(&[
        (0, 0, Down, AT),
        (100, 0, Up, AT),
        (400, 0, Down, AT),
        (500, 0, Up, AT),
        (800, 0, Down, AT),
        (900, 0, Move, (350, 300)),
        (800 + HOLD_MS, 0, Up, (350, 300)),
    ]);
    assert!(!moved.contains(&Verdict::Unlock));
}

#[test]
fn a_cancel_for_one_finger_ends_them_all() {
    // Two fingers down, the gesture cancelled for one of them alone:
    // the way out works afterwards all the same.
    let verdicts = run(&[
        (0, 0, Down, AT),
        (10, 1, Down, (300, 600)),
        (20, 0, Cancel, AT),
        (1_000, 2, Down, AT),
        (1_100, 2, Up, AT),
        (1_400, 2, Down, AT),
        (1_500, 2, Up, AT),
        (1_800, 2, Down, AT),
        (1_800 + HOLD_MS, 2, Up, AT),
    ]);
    assert_eq!(verdicts.last(), Some(&Verdict::Unlock));
}

fn hints(verdicts: &[Verdict]) -> usize {
    verdicts.iter().filter(|v| **v == Verdict::Hint).count()
}

/// Touches that are no step of the way out say how to unlock, at most
/// every HINT_MS; the way out itself is never interrupted by it.
#[test]
fn touches_say_how_to_unlock_now_and_then() {
    // Single taps two seconds apart, for forty seconds: said every
    // ten seconds, the first time at the second tap.
    let taps: Vec<Step> = (0..20u64)
        .flat_map(|n| [(n * 2_000, 0, Down, AT), (n * 2_000 + 50, 0, Up, AT)])
        .collect();
    let verdicts = run_with_hints(&taps);
    assert_eq!(verdicts[2], Verdict::Hint);
    assert_eq!(hints(&verdicts), 4);
    // A hand resting, then lifted: said once, when it is lifted.
    let rest = run_with_hints(&[(0, 0, Down, AT), (10_000, 0, Up, AT)]);
    assert_eq!(rest, [Verdict::Take, Verdict::Hint]);
    // A swipe.
    let swipe = run_with_hints(&[
        (0, 0, Down, AT),
        (50, 0, Move, (300, 300)),
        (80, 0, Up, (400, 300)),
    ]);
    assert_eq!(hints(&swipe), 1);
    // The way out: nothing said between its steps.
    let verdicts = run_with_hints(&[
        (0, 0, Down, AT),
        (100, 0, Up, AT),
        (400, 0, Down, AT),
        (500, 0, Up, AT),
        (800, 0, Down, AT),
        (800 + HOLD_MS, 0, Up, AT),
    ]);
    assert_eq!(hints(&verdicts), 0);
    assert_eq!(verdicts.last(), Some(&Verdict::Unlock));
    // Held too short after the taps: then it is said.
    let short = run_with_hints(&[
        (0, 0, Down, AT),
        (100, 0, Up, AT),
        (400, 0, Down, AT),
        (500, 0, Up, AT),
        (800, 0, Down, AT),
        (1_200, 0, Up, AT),
    ]);
    assert_eq!(short.last(), Some(&Verdict::Hint));
}

/// A slow screen holds touches back: taps up to TAP_MS long, GAP_MS
/// apart, still count; and once held long enough, a little movement
/// of the finger unlocks without lifting it.
#[test]
fn the_way_out_allows_for_a_slow_screen_and_needs_no_lift() {
    let slow = run(&[
        (0, 0, Down, AT),
        (TAP_MS - 50, 0, Up, AT),
        (TAP_MS + GAP_MS - 100, 0, Down, AT),
        (2 * TAP_MS + GAP_MS - 150, 0, Up, AT),
        (2 * TAP_MS + 2 * GAP_MS - 200, 0, Down, AT),
        (2 * TAP_MS + 2 * GAP_MS - 200 + HOLD_MS, 0, Up, AT),
    ]);
    assert_eq!(slow.last(), Some(&Verdict::Unlock));
    let held = run(&[
        (0, 0, Down, AT),
        (100, 0, Up, AT),
        (400, 0, Down, AT),
        (500, 0, Up, AT),
        (800, 0, Down, AT),
        (1_000, 0, Move, (201, 301)),
        (800 + HOLD_MS + 10, 0, Move, (202, 300)),
    ]);
    assert_eq!(held.last(), Some(&Verdict::Unlock));
    assert!(held[..6].iter().all(|v| *v == Verdict::Take));
}
