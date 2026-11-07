//! The input-filter chain: verdicts, capture of a taken press or contact,
//! release on a stopped or failed owner, the guard and X4 rules, and the
//! broker as the runner.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native_host::{
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations::{InputAction, InputGesture, InputKey, InputPointer},
};
use alloc::{rc::Rc, vec, vec::Vec};
use core::cell::RefCell;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_STATE, ERR_UNSUPPORTED};
use paperpaw_sdk::Id;

const A: u64 = 1;
const B: u64 = 2;
const C: u64 = 3;

fn row(plugin: u64) -> Subscription<'static> {
    Subscription {
        stage: stage::KEY,
        plugin,
        token: u32::try_from(plugin).unwrap(),
        order: 0,
        index: 0,
        signature: 0,
        enabled: true,
        style_granted: false,
    }
}

fn key(name: &'static str, state: u32) -> Record<'static> {
    Record::InputKey(InputKey {
        now_ms: 5,
        key: name,
        state,
    })
}

fn pointer(contact: u32, phase: u32) -> Record<'static> {
    Record::InputPointer(InputPointer {
        now_ms: 5,
        x: 10,
        y: 20,
        phase,
        contact: Some(contact),
    })
}

/// What a scripted filter answers.
#[derive(Clone)]
enum Answer {
    Pass,
    Take,
    Cancel,
    Run(&'static str),
    Raise(&'static str),
    Replace(Vec<u8>),
    Kind(u32),
    /// A failed callback.
    Fail,
    /// Bytes that are no `filter.verdict` batch.
    Garbage,
}

fn verdict(answer: &Answer, out: &mut [u8]) -> Result<usize, i32> {
    let record = match answer {
        Answer::Pass => stage::FilterVerdict::new(0, "", &[], ""),
        Answer::Take => stage::FilterVerdict::new(1, "", &[], ""),
        Answer::Run(action) => stage::FilterVerdict::new(2, action, &[], ""),
        Answer::Cancel => stage::FilterVerdict::new(3, "", &[], ""),
        Answer::Replace(input) => stage::FilterVerdict::new(4, "", input, ""),
        Answer::Raise(name) => stage::FilterVerdict::new(5, "", &[], name),
        Answer::Kind(kind) => stage::FilterVerdict::new(*kind, "", &[], ""),
        Answer::Fail => return Err(ERR_STATE),
        Answer::Garbage => {
            out[..3].copy_from_slice(&[7, 7, 7]);
            return Ok(3);
        }
    };
    record.encode(out)
}

/// The inputs a filter was called with, and its scripted answers.
#[derive(Default)]
struct Script {
    answers: Vec<(u64, Answer)>,
    calls: Vec<(u64, Vec<u8>)>,
    flags: Vec<u32>,
}

impl Script {
    fn set(&mut self, plugin: u64, answer: Answer) {
        self.answers.retain(|(p, _)| *p != plugin);
        self.answers.push((plugin, answer));
    }

    fn called(&self) -> Vec<u64> {
        self.calls.iter().map(|(p, _)| *p).collect()
    }

    fn take_calls(&mut self) -> Vec<u64> {
        let calls = self.called();
        self.calls.clear();
        calls
    }
}

impl FilterRunner for Script {
    fn call(
        &mut self,
        filter: &Subscription<'_>,
        payload: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, i32> {
        // The payload is one `filter.input` holding one input record.
        let input = stage::FilterInput::decode(payload).unwrap();
        wire::records_decode_single(input.input).unwrap();
        self.calls.push((filter.plugin, input.input.to_vec()));
        self.flags.push(input.flags);
        let answer = self
            .answers
            .iter()
            .find(|(p, _)| *p == filter.plugin)
            .map_or(Answer::Pass, |(_, a)| a.clone());
        verdict(&answer, reply)
    }
}

fn all(_: u64) -> bool {
    true
}

fn declared(owner: u64, name: &str) -> bool {
    owner == A && name == "input.a.mine"
}

fn policy(touch: bool) -> Policy<'static> {
    Policy {
        touch,
        secret: false,
        runs: &all,
        declared: &declared,
    }
}

fn one(record: &Record<'_>) -> Vec<u8> {
    Encoded::from_record(record).unwrap().batch().to_vec()
}

fn dispatch(
    chain: &mut InputFilters,
    script: &mut Script,
    rows: &[Subscription<'static>],
    record: &Record<'_>,
) -> Dispatched {
    chain.dispatch(script, rows, &policy(true), record).unwrap()
}

fn is_taken(d: &Dispatched) -> bool {
    matches!(d.routed, Routed::Taken)
}

fn is_pass(d: &Dispatched) -> bool {
    matches!(d.routed, Routed::Pass)
}

#[test]
fn filters_are_called_in_order_with_the_input_and_pass_leaves_it_alone() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(B), row(A), row(C)];
    let press = key("button.menu", 0);
    let out = dispatch(&mut chain, &mut script, &rows, &press);
    assert!(is_pass(&out));
    assert_eq!(out.refused, 0);
    assert_eq!(script.called(), [B, A, C]);
    assert!(script.calls.iter().all(|(_, input)| *input == one(&press)));
    assert_eq!(chain.held(), 0);
}

#[test]
fn only_four_filters_are_called_and_disabled_ones_do_not_count() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let mut off = row(9);
    off.enabled = false;
    let rows = [off, row(1), row(2), row(3), row(4), row(5)];
    dispatch(&mut chain, &mut script, &rows, &key("a", 0));
    assert_eq!(script.called(), [1, 2, 3, 4]);
}

#[test]
fn a_taken_press_is_held_whole_and_its_release_goes_nowhere_else() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    script.set(A, Answer::Take);
    let rows = [row(A), row(B)];
    let down = dispatch(&mut chain, &mut script, &rows, &key("button.up", 0));
    assert!(is_taken(&down));
    // The later filter never heard the press.
    assert_eq!(script.take_calls(), [A]);
    assert_eq!(chain.held(), 1);
    // The release is swallowed; only the capturer hears it.
    script.set(A, Answer::Pass);
    let up = dispatch(&mut chain, &mut script, &rows, &key("button.up", 1));
    assert!(is_taken(&up));
    assert_eq!(script.take_calls(), [A]);
    assert_eq!(chain.held(), 0);
    // The next press starts fresh with all filters.
    let again = dispatch(&mut chain, &mut script, &rows, &key("button.up", 0));
    assert!(is_pass(&again));
    assert_eq!(script.take_calls(), [A, B]);
}

#[test]
fn only_the_start_is_taken_anything_else_is_a_pass() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Take);
    // A release, repeat, move or up whose start nobody captured goes on:
    // the later filter hears it and nothing is held.
    for input in [
        key("k", 1),
        key("k", 2),
        pointer(1, 1),
        pointer(1, 2),
        pointer(1, 3),
    ] {
        let out = dispatch(&mut chain, &mut script, &rows, &input);
        assert!(is_pass(&out), "{input:?}");
        assert_eq!(script.take_calls(), [A, B]);
        assert_eq!(chain.held(), 0);
    }
    script.set(A, Answer::Cancel);
    assert!(is_pass(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 2)
    )));
}

#[test]
fn a_replace_may_not_make_the_power_key() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Replace(one(&key("power", 0))));
    let out = dispatch(&mut chain, &mut script, &rows, &key("button.menu", 0));
    assert!(is_pass(&out));
    assert_eq!(out.refused, 1);
    // B heard the original.
    assert_eq!(script.calls[1], (B, one(&key("button.menu", 0))));
    // Any other name, device-specific ones included, passes through.
    script.set(A, Answer::Replace(one(&key("scroll-wheel", 0))));
    assert!(matches!(
        dispatch(&mut chain, &mut script, &rows, &key("menu", 0)).routed,
        Routed::Replaced(_)
    ));
}

#[test]
fn a_taken_or_cancelled_pointer_down_is_held_to_its_up_per_contact() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Take);
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 0)
    )));
    // Contact 2 is its own: B cancels it.
    script.set(A, Answer::Pass);
    script.set(B, Answer::Cancel);
    let second = dispatch(&mut chain, &mut script, &rows, &pointer(2, 0));
    assert!(matches!(second.routed, Routed::Cancelled));
    assert_eq!(chain.held(), 2);
    script.take_calls();
    // Interleaved moves and ups: each goes to its own capturer alone.
    for (contact, heard) in [(2, vec![A, B]), (1, vec![A]), (2, vec![A, B])] {
        let moved = dispatch(&mut chain, &mut script, &rows, &pointer(contact, 1));
        assert!(is_taken(&moved));
        assert_eq!(script.take_calls(), heard);
    }
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 2)
    )));
    script.take_calls();
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(2, 3)
    )));
    assert_eq!(chain.held(), 0);
    // Contact 1 is free again: a new down meets every filter.
    script.set(B, Answer::Pass);
    script.take_calls();
    assert!(is_pass(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 0)
    )));
    assert_eq!(script.take_calls(), [A, B]);
}

#[test]
fn a_new_down_while_one_is_held_starts_fresh() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    // The up was lost: the same contact goes down again and meets all filters.
    script.set(A, Answer::Pass);
    script.take_calls();
    let again = dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    assert!(is_pass(&again));
    assert_eq!(script.take_calls(), [A, B]);
    assert_eq!(chain.held(), 0);
}

#[test]
fn run_ends_the_chain_without_a_capture_and_needs_an_action() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Run("core.next"));
    let out = dispatch(&mut chain, &mut script, &rows, &key("k", 0));
    match out.routed {
        Routed::Run(action) => assert_eq!(action.as_str(), "core.next"),
        other => panic!("{other:?}"),
    }
    assert_eq!(script.take_calls(), [A]);
    assert_eq!(chain.held(), 0);
    // An empty action is not a verdict: pass on, B hears it.
    script.set(A, Answer::Run(""));
    let out = dispatch(&mut chain, &mut script, &rows, &key("k", 0));
    assert!(is_pass(&out));
    assert_eq!(out.refused, 1);
    assert_eq!(script.take_calls(), [A, B]);
}

#[test]
fn replace_passes_the_new_input_to_the_filters_after_it() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    let other = key("button.down", 0);
    script.set(A, Answer::Replace(one(&other)));
    let out = dispatch(&mut chain, &mut script, &rows, &key("button.up", 0));
    let Routed::Replaced(replaced) = out.routed else {
        panic!("{:?}", out.routed);
    };
    assert_eq!(replaced.record().unwrap(), other);
    // B saw the replacement, not the original.
    assert_eq!(script.calls[1], (B, one(&other)));
    // A taker after a replace holds the physical input, not the replaced one:
    // its release is `button.up`'s, and the replacer before it replaces that too.
    script.set(B, Answer::Take);
    script.take_calls();
    dispatch(&mut chain, &mut script, &rows, &key("button.up", 0));
    assert_eq!(chain.held(), 1);
    script.take_calls();
    let release = dispatch(&mut chain, &mut script, &rows, &key("button.up", 1));
    assert!(is_taken(&release));
    let heard = script.calls.clone();
    assert_eq!(script.take_calls(), [A, B]);
    assert_eq!(heard[1].1, one(&other));
    assert_eq!(chain.held(), 0);
}

#[test]
fn a_replace_that_is_not_one_input_record_is_a_pass() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    let mut bytes = [0; 128];
    let action = Record::InputAction(InputAction {
        now_ms: 1,
        action: "core.x",
    });
    let len = wire::records_encode(&[action], &mut bytes).unwrap();
    for bad in [bytes[..len].to_vec(), vec![9, 9], Vec::new()] {
        script.set(A, Answer::Replace(bad));
        let out = dispatch(&mut chain, &mut script, &rows, &key("k", 0));
        assert!(is_pass(&out));
        assert_eq!(out.refused, 1);
    }
}

#[test]
fn raise_must_name_an_input_the_plugin_declared() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Raise("input.a.mine"));
    let out = dispatch(&mut chain, &mut script, &rows, &key("k", 0));
    match out.routed {
        Routed::Raise { owner, input } => {
            assert_eq!((owner, input.as_str()), (A, "input.a.mine"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(script.take_calls(), [A]);
    for name in ["input.b.theirs", "button.menu", ""] {
        script.set(A, Answer::Raise(name));
        let out = dispatch(&mut chain, &mut script, &rows, &key("k", 0));
        assert!(is_pass(&out), "{name}");
        assert_eq!(out.refused, 1);
    }
}

#[test]
fn failures_and_unknown_verdicts_pass_and_are_counted() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B), row(C)];
    script.set(A, Answer::Fail);
    script.set(B, Answer::Garbage);
    script.set(C, Answer::Kind(6));
    let out = dispatch(&mut chain, &mut script, &rows, &key("k", 0));
    assert!(is_pass(&out));
    assert_eq!(out.refused, 3);
    assert_eq!(script.called(), [A, B, C]);
    // A later filter still takes it.
    script.set(C, Answer::Take);
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &key("k", 0)
    )));
}

#[test]
fn a_capturer_that_fails_releases_the_capture_and_the_rest_still_goes_nowhere() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    script.set(A, Answer::Fail);
    script.take_calls();
    let moved = dispatch(&mut chain, &mut script, &rows, &pointer(1, 1));
    assert!(is_taken(&moved));
    assert_eq!(moved.refused, 1);
    assert_eq!(script.take_calls(), [A]);
    // Released: no more calls, swallowed to the up.
    let moved = dispatch(&mut chain, &mut script, &rows, &pointer(1, 1));
    assert!(is_taken(&moved));
    assert!(script.calls.is_empty());
    assert_eq!(chain.held(), 1);
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 2)
    )));
    assert!(script.calls.is_empty());
    assert_eq!(chain.held(), 0);
    // The next input starts fresh with all filters.
    script.set(A, Answer::Pass);
    assert!(is_pass(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 0)
    )));
    assert_eq!(script.take_calls(), [A, B]);
}

#[test]
fn a_stopped_or_vanished_owner_releases_without_a_call() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &key("k", 0));
    dispatch(&mut chain, &mut script, &rows, &pointer(4, 0));
    script.take_calls();
    chain.owner_stopped(A);
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &key("k", 1)
    )));
    assert!(script.calls.is_empty());
    // A filter gone from the list (unregistered) behaves the same.
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &[row(B)],
        &pointer(4, 2)
    )));
    assert!(script.calls.is_empty());
    assert_eq!(chain.held(), 0);
    // Another plugin's stop leaves a capture alone.
    script.set(A, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &key("k", 0));
    chain.owner_stopped(B);
    script.take_calls();
    dispatch(&mut chain, &mut script, &rows, &key("k", 1));
    assert_eq!(script.take_calls(), [A]);
}

#[test]
fn the_capture_table_is_bounded_and_a_full_table_passes() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    script.set(A, Answer::Take);
    let rows = [row(A)];
    for contact in 0..MAX_CAPTURES as u32 {
        assert!(is_taken(&dispatch(
            &mut chain,
            &mut script,
            &rows,
            &pointer(contact, 0)
        )));
    }
    assert_eq!(chain.held(), MAX_CAPTURES);
    let over = dispatch(&mut chain, &mut script, &rows, &pointer(99, 0));
    assert!(is_pass(&over));
    assert_eq!(over.refused, 1);
    assert_eq!(chain.held(), MAX_CAPTURES);
    chain.clear();
    assert_eq!(chain.held(), 0);
}

#[test]
fn gestures_are_filtered_and_never_held() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    script.set(A, Answer::Take);
    let gesture = Record::InputGesture(InputGesture {
        now_ms: 1,
        kind: 0,
        x: 1,
        y: 2,
    });
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &[row(A)],
        &gesture
    )));
    assert_eq!(chain.held(), 0);
}

#[test]
fn the_guard_rule_skips_a_filter_without_calling_it() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    script.set(A, Answer::Take);
    let rows = [row(A), row(B)];
    let only_b = |plugin: u64| plugin == B;
    let policy = Policy {
        touch: true,
        secret: false,
        runs: &only_b,
        declared: &declared,
    };
    let out = chain
        .dispatch(&mut script, &rows, &policy, &key("k", 0))
        .unwrap();
    assert!(is_pass(&out));
    assert_eq!(script.called(), [B]);
    // A capture of a filter the guard now skips still swallows, uncalled.
    let mut chain = InputFilters::new();
    script.set(B, Answer::Pass);
    dispatch(&mut chain, &mut script, &rows, &key("k", 0));
    script.calls.clear();
    let out = chain
        .dispatch(&mut script, &rows, &policy, &key("k", 1))
        .unwrap();
    assert!(is_taken(&out));
    assert!(script.calls.is_empty());
    assert_eq!(chain.held(), 0);
}

#[test]
fn the_x4_filters_keys_only() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    script.set(A, Answer::Take);
    let rows = [row(A)];
    let x4 = policy(false);
    let touch = chain
        .dispatch(&mut script, &rows, &x4, &pointer(1, 0))
        .unwrap();
    assert!(is_pass(&touch));
    let gesture = Record::InputGesture(InputGesture {
        now_ms: 1,
        kind: 0,
        x: 0,
        y: 0,
    });
    assert!(is_pass(
        &chain.dispatch(&mut script, &rows, &x4, &gesture).unwrap()
    ));
    assert!(script.calls.is_empty());
    let press = chain
        .dispatch(&mut script, &rows, &x4, &key("k", 0))
        .unwrap();
    assert!(is_taken(&press));
}

#[test]
fn what_is_no_key_pointer_or_gesture_is_refused() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let action = Record::InputAction(InputAction {
        now_ms: 1,
        action: "core.x",
    });
    for bad in [action, key("k", 7), pointer(1, 9)] {
        assert_eq!(
            chain
                .dispatch(&mut script, &[row(A)], &policy(true), &bad)
                .err(),
            Some(ERR_INVALID)
        );
    }
    assert!(script.calls.is_empty());
}

// The broker as the runner: a real retained-registry call, one subscription
// per run, at depth 0.

struct Fake {
    answer: Rc<RefCell<Answer>>,
    seen: Rc<RefCell<Vec<Vec<u8>>>>,
    live: core::cell::Cell<bool>,
}
impl NativeInstance for Fake {
    fn running(&self) -> bool {
        self.live.get()
    }
    fn stop(&self) {
        self.live.set(false);
    }
}
impl Dispatch for Fake {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn dropped(&self, _: u32, _: i32) {}
    fn stage(&self, _: u32, bytes: &[u8], out: &mut [u8], _: &Env<'_>) -> Result<i32, i32> {
        let Record::StageCall(call) = wire::records_decode_single(bytes).unwrap() else {
            panic!("stage.call");
        };
        assert_eq!((call.stage, call.schema), ("core.input-filter", 1));
        let input = stage::FilterInput::decode(call.payload).unwrap();
        self.seen.borrow_mut().push(input.input.to_vec());
        let answer = self.answer.borrow().clone();
        verdict(&answer, out).map(|len| i32::try_from(len).unwrap())
    }
}

#[test]
fn the_broker_runs_each_filter_alone_and_a_refused_callback_is_a_pass() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let answer = Rc::new(RefCell::new(Answer::Pass));
    let mut registry = NativeInstances::default();
    let mut owners = Vec::new();
    for name in ["alpha", "beta", "gamma"] {
        let lease = NativeLease::try_new(Fake {
            answer: answer.clone(),
            seen: seen.clone(),
            live: core::cell::Cell::new(true),
        })
        .unwrap();
        owners.push(
            registry
                .test_insert(Id::new_static(name), Some(lease), true)
                .unwrap(),
        );
    }
    let rows: Vec<Subscription<'static>> = owners.iter().map(|o| row(*o)).collect();
    let depth = DispatchDepth::new();
    let schema = schema();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let mut runner = BrokerRunner {
        schema: &schema,
        depth: &depth,
        registry: &registry,
        env: &env,
    };
    let mut chain = InputFilters::new();
    // Passing filters: all three called through the broker.
    let press = key("button.menu", 0);
    let out = chain
        .dispatch(&mut runner, &rows, &policy(true), &press)
        .unwrap();
    assert!(is_pass(&out));
    assert_eq!(seen.borrow().len(), 3);
    assert!(seen.borrow().iter().all(|i| *i == one(&press)));
    // An unknown plugin row is a refused call, not a panic: the chain goes on.
    let mut with_gone = rows.clone();
    with_gone.insert(0, row(0xdead_beef));
    let out = chain
        .dispatch(&mut runner, &with_gone, &policy(true), &press)
        .unwrap();
    assert!(is_pass(&out));
    assert_eq!(out.refused, 1);
    // The first filter takes: the press is held and the others never hear it.
    seen.borrow_mut().clear();
    *answer.borrow_mut() = Answer::Take;
    let out = chain
        .dispatch(&mut runner, &rows, &policy(true), &press)
        .unwrap();
    assert!(is_taken(&out));
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(chain.held(), 1);
    // The release reaches the capturer alone, through the broker, and ends it.
    seen.borrow_mut().clear();
    let release = key("button.menu", 1);
    let out = chain
        .dispatch(&mut runner, &rows, &policy(true), &release)
        .unwrap();
    assert!(is_taken(&out));
    assert_eq!(*seen.borrow(), [one(&release)]);
    assert_eq!(chain.held(), 0);
    // A failing callback is a refused pass for every filter.
    *answer.borrow_mut() = Answer::Fail;
    let out = chain
        .dispatch(&mut runner, &rows, &policy(true), &press)
        .unwrap();
    assert!(is_pass(&out));
    assert_eq!(out.refused, 3);
    assert_eq!(registry.depth(), 0);
}

#[test]
fn a_key_repeat_is_more_of_the_held_key_and_a_replaced_key_name_is_checked() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    // Not held: a repeat is filtered like any key input.
    assert!(is_pass(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &key("button.up", 2)
    )));
    assert_eq!(script.take_calls(), [A, B]);
    // Held: repeats go to the capturer alone and the release ends it.
    script.set(A, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &key("button.up", 0));
    script.set(A, Answer::Pass);
    script.take_calls();
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &key("button.up", 2)
    )));
    assert_eq!(script.take_calls(), [A]);
    dispatch(&mut chain, &mut script, &rows, &key("button.up", 1));
    assert_eq!(chain.held(), 0);
    // A replacement key: 1 to 32 bytes, no control characters.
    for bad in ["", "bad\nkey"] {
        let mut bytes = [0; 128];
        let record = Record::InputKey(InputKey {
            now_ms: 1,
            key: bad,
            state: 0,
        });
        let len = wire::records_encode(&[record], &mut bytes).unwrap();
        script.set(A, Answer::Replace(bytes[..len].to_vec()));
        let out = dispatch(&mut chain, &mut script, &rows, &key("k", 0));
        assert!(is_pass(&out), "{bad:?}");
        assert_eq!(out.refused, 1);
    }
}

#[test]
fn the_rest_of_a_capture_goes_through_earlier_filters_then_the_capturer_with_the_flag() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B), row(C)];
    script.set(B, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    assert_eq!(chain.held(), 1);
    script.calls.clear();
    script.flags.clear();
    // A (before B) hears the move first, without the flag; C (after) never.
    // Its take is ignored for the rest.
    script.set(A, Answer::Take);
    script.set(B, Answer::Pass);
    let moved = dispatch(&mut chain, &mut script, &rows, &pointer(1, 1));
    assert!(is_taken(&moved));
    assert_eq!(script.called(), [A, B]);
    assert_eq!(script.flags, [0, FLAG_CAPTURED]);
    // The secret flag rides along.
    script.calls.clear();
    script.flags.clear();
    let secret = Policy {
        secret: true,
        ..policy(true)
    };
    chain
        .dispatch(&mut script, &rows, &secret, &pointer(1, 2))
        .unwrap();
    assert_eq!(script.flags, [FLAG_SECRET, FLAG_SECRET | FLAG_CAPTURED]);
    assert_eq!(chain.held(), 0);
}

#[test]
fn an_earlier_replacer_replaces_the_rest_for_the_capturer() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    let other = pointer(7, 1);
    script.set(B, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    script.calls.clear();
    script.set(A, Answer::Replace(one(&other)));
    script.set(B, Answer::Pass);
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 1)
    )));
    assert_eq!(script.calls[1], (B, one(&other)));
}

#[test]
fn the_capturers_run_and_raise_act_on_the_rest_and_replace_is_ignored() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(A, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    script.set(A, Answer::Raise("input.a.mine"));
    let moved = dispatch(&mut chain, &mut script, &rows, &pointer(1, 1));
    assert!(matches!(moved.routed, Routed::Raise { owner: A, .. }));
    assert_eq!(chain.held(), 1);
    script.set(A, Answer::Replace(one(&pointer(3, 1))));
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 1)
    )));
    // A cancel or take on the rest only ends it; run acts at the up.
    script.set(A, Answer::Cancel);
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 1)
    )));
    script.set(A, Answer::Run("core.next"));
    let up = dispatch(&mut chain, &mut script, &rows, &pointer(1, 2));
    assert!(matches!(up.routed, Routed::Run(a) if a.as_str() == "core.next"));
    assert_eq!(chain.held(), 0);
    // A raise of a name not declared ends the rest, counted.
    script.set(A, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    script.set(A, Answer::Raise("input.b.theirs"));
    let bad = dispatch(&mut chain, &mut script, &rows, &pointer(1, 1));
    assert!(is_taken(&bad));
    assert_eq!(bad.refused, 1);
}

#[test]
fn a_capture_belongs_to_one_subscription_and_one_token() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let mut second = row(A);
    second.token = 99;
    let rows = [row(A), second];
    // Both subscriptions are A's; the second takes.
    struct ByToken(Script);
    impl FilterRunner for ByToken {
        fn call(
            &mut self,
            filter: &Subscription<'_>,
            payload: &[u8],
            reply: &mut [u8],
        ) -> Result<usize, i32> {
            self.0.set(
                filter.plugin,
                if filter.token == 99 {
                    Answer::Take
                } else {
                    Answer::Pass
                },
            );
            self.0.call(filter, payload, reply)
        }
    }
    let mut runner = ByToken(core::mem::take(&mut script));
    chain
        .dispatch(&mut runner, &rows, &policy(true), &pointer(1, 0))
        .unwrap();
    runner.0.calls.clear();
    chain
        .dispatch(&mut runner, &rows, &policy(true), &pointer(1, 1))
        .unwrap();
    // Token 1 (before) and token 99 (the holder) both heard it.
    assert_eq!(runner.0.called(), [A, A]);
    assert_eq!(runner.0.flags.last(), Some(&FLAG_CAPTURED));
    assert_eq!(runner.0.flags[runner.0.flags.len() - 2], 0);
}

#[test]
fn a_gesture_is_raised_at_the_up_by_its_recognizer() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A)];
    script.set(A, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    script.set(A, Answer::Raise("input.a.mine"));
    // Nothing on the moves (a raise there acts too, but a recognizer waits).
    script.set(A, Answer::Pass);
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &rows,
        &pointer(1, 1)
    )));
    script.set(A, Answer::Raise("input.a.mine"));
    let up = dispatch(&mut chain, &mut script, &rows, &pointer(1, 2));
    assert!(matches!(up.routed, Routed::Raise { owner: A, .. }));
    assert_eq!(chain.held(), 0);
}

#[test]
fn the_capturing_token_leaving_releases_the_capture_even_if_another_token_stays() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let mut second = row(A);
    second.token = 99;
    script.set(A, Answer::Take);
    // Token 1 takes (the first of A's two subscriptions).
    dispatch(&mut chain, &mut script, &[row(A), second], &pointer(1, 0));
    script.calls.clear();
    // Token 1 is removed; token 99 of the same plugin must not stand in.
    let moved = dispatch(&mut chain, &mut script, &[second], &pointer(1, 1));
    assert!(is_taken(&moved));
    assert!(script.calls.is_empty());
    assert!(is_taken(&dispatch(
        &mut chain,
        &mut script,
        &[second],
        &pointer(1, 2)
    )));
    assert_eq!(chain.held(), 0);
}

#[test]
fn a_released_capture_is_evicted_first_when_the_table_is_full() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    script.set(A, Answer::Take);
    let rows = [row(A)];
    for contact in 0..MAX_CAPTURES as u32 {
        dispatch(&mut chain, &mut script, &rows, &pointer(contact, 0));
    }
    // One plugin stops: its captures wait for ups that may never come. A new
    // down takes the room of a released one instead of failing.
    chain.owner_stopped(A);
    script.set(B, Answer::Take);
    let rows_b = [row(B)];
    let fresh = dispatch(&mut chain, &mut script, &rows_b, &pointer(50, 0));
    assert!(is_taken(&fresh));
    assert_eq!(fresh.refused, 0);
    assert_eq!(chain.held(), MAX_CAPTURES);
    // With every capture live, the next is refused as before.
    chain.clear();
    for contact in 0..MAX_CAPTURES as u32 {
        dispatch(&mut chain, &mut script, &rows, &pointer(contact, 0));
    }
    assert!(is_pass(&dispatch(
        &mut chain,
        &mut script,
        &rows_b,
        &pointer(60, 0)
    )));
}

#[test]
fn a_malformed_verdict_on_the_rest_of_a_capture_is_counted() {
    let mut chain = InputFilters::new();
    let mut script = Script::default();
    let rows = [row(A), row(B)];
    script.set(B, Answer::Take);
    dispatch(&mut chain, &mut script, &rows, &pointer(1, 0));
    // An earlier filter and the capturer both answer an unknown verdict kind:
    // each counts once; the rest is taken all the same.
    script.set(A, Answer::Kind(9));
    script.set(B, Answer::Kind(9));
    let moved = dispatch(&mut chain, &mut script, &rows, &pointer(1, 1));
    assert!(is_taken(&moved));
    assert_eq!(moved.refused, 2);
    // A replace that is no input record, from an earlier filter, counts too.
    script.set(A, Answer::Replace(vec![9, 9]));
    script.set(B, Answer::Pass);
    let moved = dispatch(&mut chain, &mut script, &rows, &pointer(1, 1));
    assert!(is_taken(&moved));
    assert_eq!(moved.refused, 1);
    assert_eq!(chain.held(), 1);
}
