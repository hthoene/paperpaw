//! Input in any order and at any time: random sequences of button
//! presses and releases, long presses, taps, swipes and pen strokes, with
//! ticks, frames shown and clock jumps between them, never panic, never
//! leave the screen stack empty or a refresh outside the screen, and
//! never leave a button stuck down; and the cases they are made of, one
//! by one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use common::{BUTTONS, Harness, SCREEN, button, pointer, press, render, within};
use paperpaw_core::Kernel;
use paperpaw_platform::{ButtonState, Instant, PointerPhase, PointerTool};
use proptest::prelude::*;

/// One step of a sequence.
#[derive(Debug, Clone)]
enum Step {
    Press(usize),
    Release(usize),
    Pointer {
        id: u8,
        phase: PointerPhase,
        x: i32,
        y: i32,
        pen: bool,
    },
    Tick,
    /// The frame rendered is on the panel now.
    Shown,
    /// Time goes on by this many milliseconds, or back where negative.
    Wait(i64),
}

/// A step, with pointers of `fingers` ids.
fn step(fingers: u8) -> impl Strategy<Value = Step> {
    let phase = prop_oneof![
        Just(PointerPhase::Down),
        Just(PointerPhase::Move),
        Just(PointerPhase::Up),
        Just(PointerPhase::Cancel),
    ];
    prop_oneof![
        3 => (0..BUTTONS.len()).prop_map(Step::Press),
        3 => (0..BUTTONS.len()).prop_map(Step::Release),
        4 => (0..fingers, phase, -30i32..150, -30i32..190, any::<bool>())
            .prop_map(|(id, phase, x, y, pen)| Step::Pointer { id, phase, x, y, pen }),
        2 => Just(Step::Tick),
        1 => Just(Step::Shown),
        3 => prop_oneof![0i64..100, 100i64..3000, -500i64..0].prop_map(Step::Wait),
    ]
}

/// Checks what every call must leave: a screen shown, a refresh on the
/// screen, rows passed on the screen.
fn check(kernel: &mut Kernel, effects: paperpaw_core::Effects) {
    assert!(kernel.active_screen().is_some(), "no screen left");
    if let Some((region, _)) = effects.refresh() {
        assert!(within(region, SCREEN), "refresh {region:?} off the screen");
    }
    render(kernel);
}

/// Plays `steps` from time 1000 on; returns the time after them. With
/// `well_formed`, buttons go down only when up and up only when down, as
/// a shell without key repeat sends them; else in any order.
fn play(kernel: &mut Kernel, steps: &[Step], well_formed: bool) -> u64 {
    let mut now: u64 = 1000;
    let mut held = [false; BUTTONS.len()];
    for step in steps {
        let at = Instant(now);
        let effects = match *step {
            Step::Press(i) if well_formed && held[i] => continue,
            Step::Release(i) if well_formed && !held[i] => continue,
            Step::Press(i) => {
                held[i] = true;
                kernel.handle(at, button(BUTTONS[i], ButtonState::Pressed))
            }
            Step::Release(i) => {
                held[i] = false;
                kernel.handle(at, button(BUTTONS[i], ButtonState::Released))
            }
            Step::Pointer {
                id,
                phase,
                x,
                y,
                pen,
            } => {
                let tool = if pen {
                    PointerTool::Pen
                } else {
                    PointerTool::Finger
                };
                kernel.handle(at, pointer(id, phase, x, y, tool))
            }
            Step::Tick => kernel.tick(at),
            Step::Shown => {
                kernel.shown(at);
                paperpaw_core::Effects::default()
            }
            Step::Wait(ms) => {
                now = now.saturating_add_signed(ms).max(1);
                continue;
            }
        };
        check(kernel, effects);
    }
    now
}

/// Lets go of everything, lets time pass and shows the frame: the
/// device as it is when nobody touches it.
fn let_go(kernel: &mut Kernel, mut now: u64) -> u64 {
    for name in BUTTONS {
        now += 10;
        let effects = kernel.handle(Instant(now), button(name, ButtonState::Released));
        check(kernel, effects);
    }
    for id in 0..3 {
        now += 10;
        let effects = kernel.handle(
            Instant(now),
            pointer(id, PointerPhase::Cancel, 60, 80, PointerTool::Finger),
        );
        check(kernel, effects);
    }
    now += 10_000;
    let effects = kernel.tick(Instant(now));
    check(kernel, effects);
    kernel.shown(Instant(now));
    kernel.shown(Instant(now));
    now
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(192)) })]

    /// Any sequence leaves a screen, refreshes on the screen, and a
    /// working page-forward button once everything is let go.
    #[test]
    fn any_input_sequence_leaves_a_working_kernel(steps in prop::collection::vec(step(3), 0..60)) {
        works_after(&steps, true)?;
    }

    /// Nothing waits for a time that has come, with one finger.
    #[test]
    fn wake_ups_asked_for_are_never_in_the_past(steps in prop::collection::vec(step(1), 0..40)) {
        wakes_ahead_after(&steps)?;
    }
}

/// After `steps`, a tick at each time asked for asks for a later one (or
/// none): a shell is never asked to wake at once, again and again.
fn wakes_ahead_after(steps: &[Step]) -> Result<(), TestCaseError> {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let mut now = play(&mut kernel, steps, false);
    let mut effects = kernel.tick(Instant(now));
    for _ in 0..8 {
        match effects.wake_at() {
            Some(at) if at.millis() <= now => {
                prop_assert!(
                    false,
                    "asked to wake at {at:?}, already {now}, after {steps:?}"
                );
            }
            Some(at) => {
                now = at.millis();
                effects = kernel.tick(at);
            }
            None => break,
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(192)) })]

    /// As [`wake_ups_asked_for_are_never_in_the_past`], with several
    /// fingers.
    #[test]
    fn wake_ups_with_several_fingers_are_never_in_the_past(steps in prop::collection::vec(step(3), 0..40)) {
        wakes_ahead_after(&steps)?;
    }
}

/// Found by `wake_ups_with_several_fingers_are_never_in_the_past`: two
/// fingers down make no long press, but the time of one stays asked for.
#[test]
fn two_fingers_resting_on_the_screen_ask_for_no_wake_up_in_the_past() {
    let down = |id| Step::Pointer {
        id,
        phase: PointerPhase::Down,
        x: 20,
        y: 39,
        pen: false,
    };
    wakes_ahead_after(&[down(1), down(0), Step::Wait(500)]).unwrap();
}

/// Whether page-forward turns the page once everything is let go after
/// `steps`.
fn works_after(steps: &[Step], well_formed: bool) -> Result<(), TestCaseError> {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let now = play(&mut kernel, steps, well_formed);
    let now = let_go(&mut kernel, now);
    harness.take();
    press(&mut kernel, now + 1000, "page-forward");
    let turned = harness
        .lines("")
        .iter()
        .any(|l| l.ends_with("action core.next-page"));
    prop_assert!(
        turned,
        "page-forward did nothing after {steps:?}: {:?}",
        harness.log.borrow()
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(192)) })]

    /// As [`any_input_sequence_leaves_a_working_kernel`], with presses
    /// repeated by the system and releases lost or doubled.
    #[test]
    fn any_button_order_leaves_a_working_kernel(steps in prop::collection::vec(step(3), 0..60)) {
        works_after(&steps, false)?;
    }
}

/// Found by `any_button_order_leaves_a_working_kernel`: the clock went
/// back between a frame shown and a repeated press.
#[test]
fn a_repeated_press_dropped_after_the_clock_went_back_does_not_stick() {
    let steps = [Step::Press(0), Step::Shown, Step::Wait(-2), Step::Press(0)];
    works_after(&steps, false).unwrap();
}

#[test]
fn a_release_without_a_press_does_nothing() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let effects = kernel.handle(Instant(100), button("page-forward", ButtonState::Released));
    assert!(effects.refresh().is_none());
    assert!(harness.lines("base:action").is_empty());
}

#[test]
fn an_unknown_button_does_nothing() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    press(&mut kernel, 100, "no-such-button");
    press(&mut kernel, 200, "");
    assert!(harness.lines("base:action").is_empty());
    assert_eq!(kernel.active_screen(), Some(&common::BASE));
}

#[test]
fn a_button_turns_the_page_as_it_goes_down() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    kernel.handle(Instant(100), button("page-forward", ButtonState::Pressed));
    assert_eq!(harness.lines("base:action"), ["base:action core.next-page"]);
    kernel.handle(Instant(150), button("page-forward", ButtonState::Released));
    assert_eq!(harness.lines("base:action").len(), 1, "once, not again");
}

#[test]
fn a_held_button_with_a_long_press_fires_it_once_and_not_its_short_press() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    // `back` has a long press (home): nothing on the press itself.
    let effects = kernel.handle(Instant(100), button("back", ButtonState::Pressed));
    assert!(harness.lines("base:action").is_empty());
    assert_eq!(effects.wake_at(), Some(Instant(600)));
    kernel.tick(Instant(599));
    assert!(harness.lines("base:action").is_empty());
    kernel.tick(Instant(600));
    kernel.tick(Instant(900));
    kernel.handle(Instant(1000), button("back", ButtonState::Released));
    assert_eq!(harness.lines("base:action"), ["base:action core.home"]);
}

#[test]
fn key_repeat_keeps_the_time_of_the_first_press() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    kernel.handle(Instant(100), button("back", ButtonState::Pressed));
    // The system repeats the key while it is held.
    for at in [300, 350, 400, 450] {
        let effects = kernel.handle(Instant(at), button("back", ButtonState::Pressed));
        assert_eq!(effects.wake_at(), Some(Instant(600)));
    }
    kernel.tick(Instant(600));
    kernel.handle(Instant(700), button("back", ButtonState::Released));
    assert_eq!(harness.lines("base:action"), ["base:action core.home"]);
}

#[test]
fn a_clock_going_back_while_a_button_is_held_fires_no_long_press_early() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    kernel.handle(Instant(10_000), button("back", ButtonState::Pressed));
    // The clock was set back: no time has passed as far as the press goes.
    kernel.tick(Instant(5_000));
    kernel.tick(Instant(10_499));
    assert!(harness.lines("base:action").is_empty());
    kernel.tick(Instant(10_500));
    assert_eq!(harness.lines("base:action"), ["base:action core.home"]);
}

#[test]
fn a_press_released_before_the_long_press_is_a_short_press() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    kernel.handle(Instant(100), button("back", ButtonState::Pressed));
    kernel.tick(Instant(300));
    kernel.handle(Instant(400), button("back", ButtonState::Released));
    // Its long press never comes.
    kernel.tick(Instant(5000));
    assert_eq!(harness.lines("base:action"), ["base:action core.back"]);
}

/// A press dropped while a screen came up (see `Kernel::shown`), then a
/// repeat of it from the system's key repeat after the screen showed, is
/// let go once: the button must not stay down.
#[test]
fn a_dropped_press_repeated_after_the_frame_does_not_stick() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    // A screen opens; it shows at 2000.
    harness.run(
        &mut kernel,
        1000,
        [paperpaw_core::Ask::OpenScreen(
            common::FULL,
            paperpaw_core::sdk::Params::new(),
        )],
    );
    render(&mut kernel);
    kernel.shown(Instant(2000));
    harness.take();
    // Pressed before it showed: dropped. Repeated after: taken. Let go.
    kernel.handle(Instant(1500), button("back", ButtonState::Pressed));
    kernel.handle(Instant(2100), button("back", ButtonState::Pressed));
    kernel.handle(Instant(2200), button("back", ButtonState::Released));
    // Nobody touches the device: no long press may come.
    kernel.tick(Instant(9000));
    assert!(
        harness.lines("full:action core.home").is_empty(),
        "a long press fired for a button let go: {:?}",
        harness.log.borrow()
    );
}

/// Two presses dropped while a screen came up (a key repeat) are let go
/// once: the next real press must not lose its release.
#[test]
fn a_dropped_press_repeated_before_the_frame_does_not_eat_the_next_release() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    harness.run(
        &mut kernel,
        1000,
        [paperpaw_core::Ask::OpenScreen(
            common::FULL,
            paperpaw_core::sdk::Params::new(),
        )],
    );
    render(&mut kernel);
    kernel.shown(Instant(2000));
    harness.take();
    kernel.handle(Instant(1500), button("back", ButtonState::Pressed));
    kernel.handle(Instant(1600), button("back", ButtonState::Pressed));
    kernel.handle(Instant(1700), button("back", ButtonState::Released));
    kernel.shown(Instant(2100));
    // A real short press of back: it goes back.
    kernel.handle(Instant(3000), button("back", ButtonState::Pressed));
    kernel.handle(Instant(3100), button("back", ButtonState::Released));
    assert_eq!(
        kernel.active_screen(),
        Some(&common::BASE),
        "{:?}",
        harness.log.borrow()
    );
}

#[test]
fn pointer_moves_and_lifts_without_a_touch_do_nothing() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    for phase in [PointerPhase::Move, PointerPhase::Up, PointerPhase::Cancel] {
        let effects = kernel.handle(Instant(100), pointer(7, phase, 60, 80, PointerTool::Finger));
        assert!(effects.refresh().is_none());
    }
    let log = harness.take();
    assert!(log.is_empty(), "{log:?}");
}

#[test]
fn a_cancelled_touch_makes_no_gesture() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    kernel.handle(
        Instant(100),
        pointer(0, PointerPhase::Down, 60, 80, PointerTool::Finger),
    );
    kernel.handle(
        Instant(150),
        pointer(0, PointerPhase::Cancel, 60, 80, PointerTool::Finger),
    );
    kernel.tick(Instant(2000));
    assert!(harness.lines("base:gesture").is_empty());
}

#[test]
fn a_touch_down_twice_counts_from_the_second() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    // A lost lift: the same finger comes down again elsewhere.
    kernel.handle(
        Instant(100),
        pointer(0, PointerPhase::Down, 40, 80, PointerTool::Finger),
    );
    kernel.handle(
        Instant(200),
        pointer(0, PointerPhase::Down, 60, 80, PointerTool::Finger),
    );
    kernel.handle(
        Instant(260),
        pointer(0, PointerPhase::Up, 60, 80, PointerTool::Finger),
    );
    assert_eq!(harness.lines("base:gesture"), ["base:gesture Tap"]);
    // The centre: the menu.
    assert_eq!(kernel.active_screen(), Some(&common::OVERLAY));
}

#[test]
fn touches_far_off_the_screen_count_at_its_edges() {
    for (x, y) in [
        (-10_000, 80),
        (10_000, 80),
        (60, i32::MIN / 2),
        (60, i32::MAX / 2),
        (i32::MIN, i32::MIN),
        (i32::MAX, i32::MAX),
    ] {
        let harness = Harness::new();
        let mut kernel = harness.kernel();
        kernel.handle(
            Instant(100),
            pointer(1, PointerPhase::Down, x, y, PointerTool::Finger),
        );
        let effects = kernel.handle(
            Instant(150),
            pointer(1, PointerPhase::Up, x, y, PointerTool::Finger),
        );
        check(&mut kernel, effects);
        assert_eq!(
            harness.lines("base:gesture"),
            ["base:gesture Tap"],
            "at {x},{y}"
        );
    }
}

#[test]
fn a_long_touch_fires_once() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let effects = kernel.handle(
        Instant(100),
        pointer(0, PointerPhase::Down, 60, 80, PointerTool::Finger),
    );
    assert_eq!(effects.wake_at(), Some(Instant(600)));
    kernel.tick(Instant(600));
    kernel.tick(Instant(700));
    kernel.handle(
        Instant(800),
        pointer(0, PointerPhase::Up, 60, 80, PointerTool::Finger),
    );
    assert_eq!(harness.lines("base:gesture"), ["base:gesture LongPress"]);
}
