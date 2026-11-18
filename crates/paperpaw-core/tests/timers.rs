//! Time: events scheduled by plugins, screens' timers, notices and long
//! presses come once, at their time, whatever the clock does between
//! (set back, jumping far ahead, at the end of its range), and what the
//! kernel asks to be woken for is the earliest of them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use common::{FULL, Harness, LATER, LATER2, REPEAT, WAKE};
use paperpaw_core::Ask;
use paperpaw_core::sdk::Params;
use paperpaw_core::sdk::work::Work;
use paperpaw_platform::Instant;
use proptest::prelude::*;
use proptest::test_runner::Config;

fn schedule(event: &paperpaw_core::sdk::Id, at: u64) -> Ask {
    Ask::work(Work::at(event.clone(), Instant(at)))
}

#[test]
fn a_scheduled_event_comes_once_at_its_time() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let effects = harness.run(&mut kernel, 1000, [schedule(&LATER, 5000)]);
    assert_eq!(effects.wake_at(), Some(Instant(5000)));
    kernel.tick(Instant(4999));
    assert!(harness.lines("event").is_empty());
    let effects = kernel.tick(Instant(5000));
    assert_eq!(effects.wake_at(), None);
    for at in [5000, 6000, 1_000_000] {
        kernel.tick(Instant(at));
    }
    assert_eq!(harness.lines("event"), ["event drv.later at 5000 "]);
}

#[test]
fn an_event_scheduled_in_the_past_comes_at_the_next_tick() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let effects = harness.run(&mut kernel, 10_000, [schedule(&LATER, 100)]);
    assert_eq!(effects.wake_at(), Some(Instant(100)));
    kernel.tick(Instant(10_001));
    kernel.tick(Instant(10_002));
    assert_eq!(harness.lines("event"), ["event drv.later at 10001 "]);
}

#[test]
fn a_clock_set_back_does_not_bring_events_early() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    harness.run(&mut kernel, 10_000, [schedule(&LATER, 15_000)]);
    // The clock goes back an hour: the event is still to come.
    let effects = kernel.tick(Instant(1_000));
    assert!(harness.lines("event").is_empty());
    assert_eq!(effects.wake_at(), Some(Instant(15_000)));
    kernel.tick(Instant(15_000));
    assert_eq!(harness.lines("event"), ["event drv.later at 15000 "]);
}

#[test]
fn a_clock_jumping_far_ahead_brings_each_event_once() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    harness.run(
        &mut kernel,
        1000,
        [schedule(&LATER2, 3000), schedule(&LATER, 2000)],
    );
    let effects = kernel.tick(Instant(1_000_000_000));
    assert_eq!(effects.wake_at(), None);
    kernel.tick(Instant(1_000_000_001));
    let mut events = harness.lines("event");
    events.sort();
    assert_eq!(
        events,
        [
            "event drv.later at 1000000000 ",
            "event drv.later2 at 1000000000 "
        ]
    );
}

#[test]
fn a_repeating_event_comes_once_per_period_even_after_a_jump() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let params = Params::new().with(REPEAT, "1000");
    harness.run(
        &mut kernel,
        0,
        [Ask::work(Work::at(LATER, Instant(1000)).with(params))],
    );
    for at in [1000, 1500, 2000, 2999, 3000] {
        kernel.tick(Instant(at));
    }
    assert_eq!(harness.lines("event").len(), 3);
    // Asleep for a day: once, not for every period missed.
    let effects = kernel.tick(Instant(86_400_000));
    assert_eq!(harness.lines("event").len(), 4);
    assert_eq!(effects.wake_at(), Some(Instant(86_401_000)));
}

#[test]
fn times_at_the_end_of_the_clock_do_not_overflow() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let end = u64::MAX;
    let params = Params::new().with(REPEAT, "1000");
    let effects = harness.run(
        &mut kernel,
        end - 10,
        [Ask::work(Work::at(LATER, Instant(end)).with(params))],
    );
    assert_eq!(effects.wake_at(), Some(Instant(end)));
    kernel.tick(Instant(end));
    // Again at the end of the clock: no further.
    kernel.tick(Instant(end));
    let events = harness.lines("event");
    assert!(!events.is_empty() && events.len() <= 2, "{events:?}");
    // Input at the end of the clock too.
    common::press(&mut kernel, end - 100, "back");
    kernel.tick(Instant(end));
}

#[test]
fn a_screen_timer_comes_once_and_not_after_its_screen_closed() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let wake = |at: &str| Ask::OpenScreen(FULL, Params::new().with(WAKE, String::from(at)));
    let effects = harness.run(&mut kernel, 1000, [wake("5000")]);
    assert_eq!(effects.wake_at(), Some(Instant(5000)));
    kernel.tick(Instant(4999));
    kernel.tick(Instant(5000));
    kernel.tick(Instant(5001));
    kernel.tick(Instant(9000));
    assert_eq!(harness.lines("full:timer"), ["full:timer at 5000"]);
    // Closed before its time: no timer, for it or the screen below.
    harness.take();
    harness.run(&mut kernel, 10_000, [Ask::Back, wake("15000")]);
    harness.run(&mut kernel, 11_000, [Ask::Back]);
    let effects = kernel.tick(Instant(15_000));
    assert_eq!(effects.wake_at(), None);
    assert!(
        harness.lines("").iter().all(|l| !l.contains("timer")),
        "{:?}",
        harness.log.borrow()
    );
}

#[test]
fn a_notice_goes_after_its_time_also_when_the_clock_goes_back() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let effects = harness.run(&mut kernel, 10_000, [Ask::Notify(String::from("Saved"))]);
    assert!(effects.refresh().is_some());
    assert_eq!(effects.wake_at(), Some(Instant(14_000)));
    common::render(&mut kernel);
    // Set back: it stays, and still goes at its time.
    assert!(kernel.tick(Instant(2_000)).refresh().is_none());
    let effects = kernel.tick(Instant(14_000));
    assert!(effects.refresh().is_some(), "the notice is taken away");
    assert_eq!(effects.wake_at(), None);
    // Notices one over the other: the last one shows its full time.
    harness.run(&mut kernel, 20_000, [Ask::Notify(String::from("a"))]);
    let effects = harness.run(&mut kernel, 21_000, [Ask::Notify(String::from("b"))]);
    assert_eq!(effects.wake_at(), Some(Instant(25_000)));
}

#[test]
fn the_kernel_asks_to_be_woken_for_the_earliest_thing_due() {
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let effects = harness.run(
        &mut kernel,
        1000,
        [
            schedule(&LATER, 9000),
            schedule(&LATER2, 3000),
            Ask::Notify(String::from("x")),
        ],
    );
    assert_eq!(effects.wake_at(), Some(Instant(3000)));
    kernel.handle(
        Instant(1500),
        common::button("back", paperpaw_platform::ButtonState::Pressed),
    );
    let effects = kernel.tick(Instant(1600));
    // The long press of back, at 2000.
    assert_eq!(effects.wake_at(), Some(Instant(2000)));
}

#[derive(Debug, Clone)]
enum Step {
    Schedule(bool, u64),
    Tick(u64),
}

proptest! {
    #![proptest_config(Config { failure_persistence: None, ..Config::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(192)) })]

    /// Each scheduled event comes once at most per time it was scheduled,
    /// never before its time, and is due no more once its time passed.
    #[test]
    fn scheduled_events_come_once_and_never_early(
        steps in prop::collection::vec(prop_oneof![
            (any::<bool>(), 0u64..20_000).prop_map(|(second, at)| Step::Schedule(second, at)),
            prop_oneof![0u64..20_000, Just(u64::MAX)].prop_map(Step::Tick),
        ], 0..40)
    ) {
        let harness = Harness::new();
        let mut kernel = harness.kernel();
        // When each event is due, as the model has it.
        let mut due: [Option<u64>; 2] = [None, None];
        for step in &steps {
            match *step {
                Step::Schedule(second, at) => {
                    let event = if second { &LATER2 } else { &LATER };
                    harness.run(&mut kernel, 0, [schedule(event, at)]);
                    due[usize::from(second)] = Some(at);
                }
                Step::Tick(now) => {
                    kernel.tick(Instant(now));
                    for (i, name) in ["drv.later ", "drv.later2 "].iter().enumerate() {
                        let came = harness
                            .take_matching(&format!("event {name}"));
                        match due[i] {
                            Some(at) if at <= now => {
                                prop_assert_eq!(came, 1, "{} due at {} at {}", name, at, now);
                                due[i] = None;
                            }
                            _ => prop_assert_eq!(came, 0, "{} came early at {}", name, now),
                        }
                    }
                }
            }
        }
    }
}
