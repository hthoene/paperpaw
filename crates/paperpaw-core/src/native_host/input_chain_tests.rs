//! The `core.input-filter` chain on the kernel's input path: an event as the
//! stage's record and back, and what a swallowing, replacing, failing filter
//! (and Power) makes of an input.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native_host::codec;
use alloc::vec;
use paperpaw_native::host_glue::ERR_STATE;

fn all(_: u64) -> bool {
    true
}
fn none(_: u64, _: &str) -> bool {
    false
}

fn filter(plugin: u64) -> Subscription<'static> {
    Subscription {
        stage: stage::KEY,
        plugin,
        token: 1,
        order: 0,
        index: 0,
        signature: 0,
        enabled: true,
        style_granted: false,
    }
}

/// A filter that answers what it was told to, one answer per call.
struct Scripted {
    answers: Vec<Result<stage::FilterVerdict<'static>, i32>>,
    called: usize,
}

impl FilterRunner for Scripted {
    fn call(&mut self, _: &Subscription<'_>, _: &[u8], reply: &mut [u8]) -> Result<usize, i32> {
        self.called += 1;
        match self.answers.remove(0) {
            Ok(verdict) => verdict.encode(reply),
            Err(status) => Err(status),
        }
    }
}

fn pass() -> Result<stage::FilterVerdict<'static>, i32> {
    Ok(stage::FilterVerdict::new(0, "", &[], ""))
}

/// The bytes of a `replace` verdict's input: a one-record batch.
fn batch(record: &Record<'_>) -> &'static [u8] {
    let mut bytes = [0; 128];
    let length = codec::encode_batch(core::slice::from_ref(record), &mut bytes).unwrap();
    alloc::boxed::Box::leak(bytes[..length].to_vec().into_boxed_slice())
}

const DISPLAY: (u16, u16) = (480, 800);
const KEYS: [&str; 1] = ["extra"];

fn run(
    event: InputEvent,
    answers: Vec<Result<stage::FilterVerdict<'static>, i32>>,
) -> (Native, usize, usize) {
    let mut chain = InputFilters::new();
    let mut runner = Scripted { answers, called: 0 };
    let policy = Policy {
        touch: true,
        secret: false,
        runs: &all,
        declared: &none,
    };
    let (native, refused) = run_chain(
        &mut chain,
        &mut runner,
        &[filter(1)],
        &policy,
        &event,
        (5, DISPLAY, &KEYS),
    );
    (native, refused, runner.called)
}

fn press(name: &'static str) -> InputEvent {
    InputEvent::Button {
        name,
        state: ButtonState::Pressed,
    }
}

fn finger(phase: PointerPhase, x: i32, y: i32) -> InputEvent {
    InputEvent::Pointer(PointerEvent::new(
        3,
        phase,
        Point::new(x, y),
        PointerTool::Finger,
        1000,
    ))
}

#[test]
fn an_event_is_the_stages_record_and_power_and_hovering_are_not() {
    assert_eq!(
        record_of(&press("up"), 9),
        Some(Record::InputKey(wire::InputKey {
            now_ms: 9,
            key: "up",
            state: 0
        }))
    );
    let released = InputEvent::Button {
        name: "up",
        state: ButtonState::Released,
    };
    assert!(matches!(
        record_of(&released, 9),
        Some(Record::InputKey(wire::InputKey { state: 1, .. }))
    ));
    assert_eq!(record_of(&press(buttons::POWER), 9), None);
    for (phase, number) in [
        (PointerPhase::Down, 0),
        (PointerPhase::Move, 1),
        (PointerPhase::Up, 2),
        (PointerPhase::Cancel, 3),
    ] {
        assert_eq!(
            record_of(&finger(phase, 10, 20), 9),
            Some(Record::InputPointer(wire::InputPointer {
                now_ms: 9,
                x: 10,
                y: 20,
                phase: number,
                contact: Some(3)
            }))
        );
    }
    assert_eq!(record_of(&finger(PointerPhase::Hover, 1, 1), 9), None);
}

#[test]
fn a_replaced_event_is_known_or_it_is_not() {
    let key = |name, state| {
        Record::InputKey(wire::InputKey {
            now_ms: 1,
            key: name,
            state,
        })
    };
    let was = press("up");
    let make = |record: &Record<'_>| event_of(record, &was, DISPLAY, &KEYS);
    // A platform button, and one of the device's.
    assert_eq!(
        make(&key("down", 1)),
        Some(InputEvent::Button {
            name: "down",
            state: ButtonState::Released
        })
    );
    assert_eq!(make(&key("extra", 0)), Some(press("extra")));
    // Not a name anyone knows, Power, a repeat: no event.
    assert_eq!(make(&key("nonsense", 0)), None);
    assert_eq!(make(&key(buttons::POWER, 0)), None);
    assert_eq!(make(&key("up", 2)), None);
    // A pointer is clamped to the display and keeps the original's tool.
    let pen = InputEvent::Pointer(PointerEvent::new(
        1,
        PointerPhase::Move,
        Point::new(0, 0),
        PointerTool::Pen,
        1000,
    ));
    let far = Record::InputPointer(wire::InputPointer {
        now_ms: 1,
        x: 9000,
        y: -5,
        phase: 0,
        contact: Some(2),
    });
    let Some(InputEvent::Pointer(made)) = event_of(&far, &pen, DISPLAY, &KEYS) else {
        panic!("a pointer");
    };
    assert_eq!(
        (made.point.x, made.point.y, made.id, made.tool),
        (479, 0, 2, PointerTool::Pen)
    );
    assert_eq!(made.phase, PointerPhase::Down);
    // A gesture is raised, never routed as an event.
    let gesture = Record::InputGesture(wire::InputGesture {
        now_ms: 1,
        kind: 0,
        x: 0,
        y: 0,
    });
    assert_eq!(make(&gesture), None);
}

#[test]
fn a_filter_swallows_a_key() {
    let take = Ok(stage::FilterVerdict::new(1, "", &[], ""));
    assert_eq!(run(press("up"), vec![take]), (Native::Taken, 0, 1));
}

#[test]
fn a_filter_replaces_a_key_with_a_known_one_and_an_unknown_one_passes() {
    let replace = |name| {
        let record = Record::InputKey(wire::InputKey {
            now_ms: 5,
            key: name,
            state: 0,
        });
        Ok(stage::FilterVerdict::new(4, "", batch(&record), ""))
    };
    assert_eq!(
        run(press("up"), vec![replace("down")]),
        (Native::Replaced(press("down")), 0, 1)
    );
    // The key path takes known names only: the original goes on, counted.
    assert_eq!(
        run(press("up"), vec![replace("nonsense")]),
        (Native::Pass, 1, 1)
    );
    // A filter cannot make Power.
    assert_eq!(
        run(press("up"), vec![replace(buttons::POWER)]),
        (Native::Pass, 1, 1)
    );
}

#[test]
fn a_filter_replaces_a_pointer_and_it_is_clamped() {
    let record = Record::InputPointer(wire::InputPointer {
        now_ms: 5,
        x: 5000,
        y: 20,
        phase: 0,
        contact: Some(3),
    });
    let replace = Ok(stage::FilterVerdict::new(4, "", batch(&record), ""));
    let (native, refused, _) = run(finger(PointerPhase::Down, 10, 20), vec![replace]);
    assert_eq!(refused, 0);
    assert_eq!(
        native,
        Native::Replaced(finger(PointerPhase::Down, 479, 20))
    );
}

#[test]
fn power_is_never_filtered() {
    // The runner has no answer for it: it is never called.
    assert_eq!(run(press(buttons::POWER), vec![]), (Native::Pass, 0, 0));
}

#[test]
fn a_filter_that_fails_or_answers_badly_passes_the_input() {
    assert_eq!(run(press("up"), vec![Err(ERR_STATE)]), (Native::Pass, 1, 1));
    // A verdict the host does not know.
    let strange = Ok(stage::FilterVerdict::new(99, "", &[], ""));
    assert_eq!(run(press("up"), vec![strange]), (Native::Pass, 1, 1));
    assert_eq!(run(press("up"), vec![pass()]), (Native::Pass, 0, 1));
}

#[test]
fn cancel_and_run_are_told_as_they_are() {
    let cancel = Ok(stage::FilterVerdict::new(3, "", &[], ""));
    assert_eq!(run(press("up"), vec![cancel]).0, Native::Cancelled);
    let action = Ok(stage::FilterVerdict::new(2, "lock.open", &[], ""));
    assert_eq!(
        run(press("up"), vec![action]).0,
        Native::Run("lock.open".into())
    );
}
