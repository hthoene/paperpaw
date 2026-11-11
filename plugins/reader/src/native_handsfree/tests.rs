use super::*;
use crate::declarations;
use paperpaw_sdk::native::callbacks::{InputGestureKind, KeyState};

fn action(name: &str) -> Input<'_> {
    Input::Action {
        now_ms: 1,
        action: name,
    }
}

#[test]
fn the_interval_is_beta_3s_bounds_in_whole_seconds() {
    assert_eq!(clamp_seconds(i64::MIN), 5);
    assert_eq!(clamp_seconds(0), 5);
    assert_eq!(clamp_seconds(5), 5);
    assert_eq!(clamp_seconds(30), 30);
    assert_eq!(clamp_seconds(600), 600);
    assert_eq!(clamp_seconds(i64::MAX), 600);
    assert_eq!(interval_ms(30), 30_000);
    assert_eq!(interval_ms(1), 5_000);
    assert_eq!(interval_ms(100_000), 600_000);
}

#[test]
fn the_action_token_is_the_place_of_auto_turn_in_the_list() {
    let token = action_token();
    let at = (token - declarations::FIRST_ACTION) as usize;
    assert_eq!(declarations::ACTIONS[at].0, "auto-turn");
    assert!(is_reader_action(token));
    assert!(is_reader_action(declarations::FIRST_ACTION));
    assert!(!is_reader_action(declarations::FIRST_ACTION - 1));
    assert!(!is_reader_action(
        declarations::FIRST_ACTION + declarations::ACTIONS.len() as u32
    ));
    assert!(!is_reader_action(u32::MAX));
    assert_eq!(ACTION, "reader.auto-turn");
    assert_eq!(SETTING, "reader.auto-turn-seconds");
}

#[test]
fn inputs_are_told_apart_as_beta_3_did() {
    for turn in [
        "core.next-page",
        "core.prev-page",
        "core.right",
        "core.left",
        "core.down",
        "core.up",
    ] {
        assert_eq!(classify(&action(turn)), Kind::Turn, "{turn}");
    }
    assert_eq!(classify(&action("reader.auto-turn")), Kind::Toggle);
    assert_eq!(classify(&action("core.menu")), Kind::Stop);
    assert_eq!(classify(&action("core.confirm")), Kind::Stop);
    assert_eq!(classify(&action("reader.search")), Kind::Other);
    assert_eq!(classify(&action("")), Kind::Other);
    let tap = Input::Gesture {
        now_ms: 1,
        position: paperpaw_sdk::Point::new(3, 4),
        kind: InputGestureKind::Tap,
    };
    assert_eq!(classify(&tap), Kind::Other);
    let key = Input::Key {
        now_ms: 1,
        key: "menu",
        state: KeyState::Press,
    };
    assert_eq!(classify(&key), Kind::Ignored);
}

#[test]
fn a_toggle_starts_and_stops_for_the_screen() {
    let mut timer = AutoTurn::<u32>::default();
    assert_eq!(timer.running(), None);
    assert_eq!(timer.toggle(7), Change::Started);
    assert_eq!(timer.running(), Some(7));
    assert_eq!(timer.toggle(7), Change::Stopped);
    assert_eq!(timer.running(), None);
    // Stopped from another screen than it started on: also stopped.
    timer.toggle(7);
    assert_eq!(timer.toggle(9), Change::Stopped);
    assert!(!timer.stop());
}

#[test]
fn a_page_turned_by_hand_keeps_it_running_and_sets_the_time_again() {
    let mut timer = AutoTurn::<u32>::default();
    timer.toggle(1);
    assert_eq!(timer.react(1, Kind::Turn), Reaction::Rearm);
    assert_eq!(timer.running(), Some(1));
}

#[test]
fn the_menu_and_the_middle_stop_it_and_are_taken() {
    let mut timer = AutoTurn::<u32>::default();
    timer.toggle(1);
    assert_eq!(
        timer.react(1, Kind::Stop),
        Reaction::Stopped {
            said: true,
            taken: true
        }
    );
    assert_eq!(timer.running(), None);
}

#[test]
fn any_other_input_stops_it_quietly_and_goes_on() {
    let mut timer = AutoTurn::<u32>::default();
    timer.toggle(1);
    assert_eq!(timer.react(1, Kind::Ignored), Reaction::Pass);
    assert_eq!(timer.running(), Some(1));
    assert_eq!(
        timer.react(1, Kind::Other),
        Reaction::Stopped {
            said: false,
            taken: false
        }
    );
    assert_eq!(timer.running(), None);
}

#[test]
fn an_input_of_another_screen_stops_it_even_a_turn() {
    let mut timer = AutoTurn::<u32>::default();
    timer.toggle(1);
    assert_eq!(
        timer.react(2, Kind::Turn),
        Reaction::Stopped {
            said: false,
            taken: false
        }
    );
    assert_eq!(timer.running(), None);
}

#[test]
fn a_stopped_timer_leaves_every_input_to_the_screen() {
    let mut timer = AutoTurn::<u32>::default();
    for kind in [Kind::Turn, Kind::Stop, Kind::Other, Kind::Ignored] {
        assert_eq!(timer.react(1, kind), Reaction::Pass, "{kind:?}");
    }
    // A toggle is the timer's whether it runs or not.
    assert_eq!(timer.react(1, Kind::Toggle), Reaction::Toggle);
    timer.toggle(1);
    assert_eq!(timer.react(1, Kind::Toggle), Reaction::Toggle);
}

#[test]
fn closing_a_screen_stops_the_timer_of_that_screen_only() {
    let mut timer = AutoTurn::<u32>::default();
    timer.toggle(1);
    assert!(!timer.left(2));
    assert_eq!(timer.running(), Some(1));
    assert!(timer.left(1));
    assert_eq!(timer.running(), None);
    assert!(!timer.left(1));
}

#[test]
fn the_timers_work_is_one_valid_spec_for_every_interval() {
    for seconds in [i64::MIN, 0, 5, 30, 600, i64::MAX] {
        let spec = WorkSpec::new(KEY, WORK_DUE)
            .unwrap()
            .after_ms(interval_ms(seconds));
        assert!(spec.check().is_ok(), "{seconds}");
    }
    assert_eq!(WorkSpec::new(KEY, WORK_DUE).unwrap().key(), KEY);
    assert_ne!(KEY, "place");
}
