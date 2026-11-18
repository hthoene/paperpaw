//! The touch lock's input filter, as the host's `core.input-filter` stage
//! calls it: every finger is taken while locked, a key or a gesture and every
//! finger while unlocked pass, and the way out runs the toggle.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::registrations::{InputKey, InputPointer, Record, records_encode};
use paperpaw_plugin_lock::native_touch_lock::{RUN, TAKE, TOGGLE_ACTION, TouchLock};
use paperpaw_sdk::native::stage_schemas::core_input_filter_v1 as filter;

const DOWN: u32 = 0;
const MOVE: u32 = 1;
const UP: u32 = 2;

fn payload(record: &Record<'_>) -> Vec<u8> {
    let mut input = [0; 128];
    let length = records_encode(std::slice::from_ref(record), &mut input).unwrap();
    let row = [filter::Record::FilterInput(filter::FilterInput::new(
        &input[..length],
        0,
    ))];
    let mut out = [0; 512];
    let length = filter::input_encode(&row, &mut out).unwrap();
    out[..length].to_vec()
}

fn finger(now_ms: u64, phase: u32) -> Vec<u8> {
    payload(&Record::InputPointer(InputPointer {
        now_ms,
        x: 200,
        y: 300,
        phase,
        contact: Some(1),
    }))
}

fn ask(lock: &mut TouchLock, locked: bool, bytes: &[u8]) -> Option<(u32, &'static str)> {
    lock.answer(|| locked, bytes).unwrap()
}

#[test]
fn a_locked_touch_is_taken_and_an_unlocked_one_passes() {
    let mut lock = TouchLock::default();
    assert_eq!(ask(&mut lock, true, &finger(0, DOWN)), Some((TAKE, "")));
    assert_eq!(ask(&mut lock, true, &finger(50, UP)), Some((TAKE, "")));
    assert_eq!(ask(&mut lock, false, &finger(5_000, DOWN)), None);
}

#[test]
fn the_buttons_pass_while_touch_is_locked() {
    let mut lock = TouchLock::default();
    let key = payload(&Record::InputKey(InputKey {
        now_ms: 0,
        key: "page-forward",
        state: 0,
    }));
    // The switch is not even read for a button.
    assert_eq!(
        lock.answer(|| panic!("a key does not read the switch"), &key)
            .unwrap(),
        None
    );
}

#[test]
fn tapping_twice_and_holding_the_second_tap_runs_the_toggle() {
    let mut lock = TouchLock::default();
    for (at, phase) in [(0, DOWN), (100, UP), (400, DOWN), (500, UP), (800, DOWN)] {
        assert_eq!(ask(&mut lock, true, &finger(at, phase)), Some((TAKE, "")));
    }
    assert_eq!(
        ask(&mut lock, true, &finger(800 + 1_500, UP)),
        Some((RUN, TOGGLE_ACTION))
    );
    // Held long enough and still down: a little movement unlocks as well.
    let mut lock = TouchLock::default();
    for (at, phase) in [(0, DOWN), (100, UP), (400, DOWN)] {
        ask(&mut lock, true, &finger(at, phase));
    }
    assert_eq!(
        ask(&mut lock, true, &finger(400 + 1_510, MOVE)),
        Some((RUN, TOGGLE_ACTION))
    );
}
