//! A shell that refuses a call (`Hooks::call_begin` answers `false`): plugin
//! code is not run, no `call_end` follows, and nothing panics.
use super::instance_tests::{KEPT_OWNER, load};
use super::*;
use crate::native_host::flat::FlatInstance;

#[test]
fn a_refused_make_runs_no_plugin_code_and_the_load_says_so() {
    reset(NORMAL);
    REFUSE.with(|r| r.set(true));
    assert_eq!(load().err(), Some(Error::Refused));
    assert_eq!(MAKES.with(Cell::get), 0, "make was not run");
    assert_eq!(CALLS.with(Cell::get), 0);
    assert_eq!(OPEN.with(Cell::get), 0, "no call_end without a call");
    // Once the shell accepts calls again the same plugin loads.
    REFUSE.with(|r| r.set(false));
    assert!(load().is_ok());
}

#[test]
fn a_refused_callback_is_not_run_and_changes_nothing() {
    reset(NORMAL);
    let instance = load().unwrap();
    let depth = DispatchDepth::new();
    let (events, calls) = (EVENTS.with(Cell::get), CALLS.with(Cell::get));
    REFUSE.with(|r| r.set(true));
    assert_eq!(
        instance.dispatch_event(0, &[1, 2, 3], &mut [], &depth),
        Err(super::super::super::Error::Busy),
        "a refusal is a busy answer: the caller tries again"
    );
    assert_eq!(EVENTS.with(Cell::get), events, "the callback did not run");
    assert_eq!(CALLS.with(Cell::get), calls);
    assert_eq!(OPEN.with(Cell::get), 0);
    // Accepted again, the callback runs.
    REFUSE.with(|r| r.set(false));
    assert_eq!(
        instance.dispatch_event(0, &[1, 2, 3], &mut [], &depth),
        Ok(OK)
    );
    assert_eq!(EVENTS.with(Cell::get), events + 1);
}

#[test]
fn a_refused_destructor_keeps_the_owner_reports_leaked_and_does_not_panic() {
    reset(NORMAL);
    let instance = load().unwrap();
    // The kept owner is never freed, by design: a static that points at
    // it keeps it reachable for Miri's leak check.
    KEPT_OWNER.store(
        instance.hosted_address(),
        core::sync::atomic::Ordering::Relaxed,
    );
    REFUSE.with(|r| r.set(true));
    drop(instance);
    REFUSE.with(|r| r.set(false));
    assert_eq!(logged(), ["leaked"], "told, and nothing else ran");
    assert!(
        MODULE_ALIVE.with(Cell::get),
        "the module outlives the kept owner"
    );
    assert_eq!(OPEN.with(Cell::get), 0);
}
