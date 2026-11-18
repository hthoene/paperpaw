//! The real plugin driven by the scripted host: the action that asks for a
//! fetch at a good moment declares work (a network work that may wait an hour,
//! under one key, replacing the one that waits), and the work that is due is
//! taken as the action to fetch (the scripted host has no settings, so the fetch
//! says it needs its set-up, it does not fail the callback).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{EventActionRun, EventWorkDue, Record};
use paperpaw_plugin_wallabag::{
    NATIVE_ENTRY,
    declarations::{FETCH_TOKEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

/// The action `wallabag.fetch`, run by a rule (`background`) or by the reader.
fn fetch(p: &Plug<'_>, background: bool) -> Callback {
    event(
        p,
        FETCH_TOKEN,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "wallabag.fetch",
            flags: u32::from(background),
        }),
    )
}

/// A scripted `core.work` that keeps what is declared (op 1: the record's bytes).
fn work(host: &ScriptedHost) -> Rc<RefCell<Vec<Vec<u8>>>> {
    let declared = Rc::new(RefCell::new(Vec::new()));
    let seen = declared.clone();
    host.service("core.work", 1, 0, move |op, input| {
        if op == 1 {
            seen.borrow_mut().push(input.to_vec());
            return Ok(Vec::new());
        }
        Err(-1)
    });
    host.reply_bound("core.work", 1, 64);
    declared
}

fn has(bytes: &[u8], needle: &str) -> bool {
    bytes
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[test]
fn a_fetch_a_rule_runs_declares_one_work_under_a_key_each_time() {
    let host = ScriptedHost::new();
    let declared = work(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let asked = fetch(&p, true);
    assert_eq!(asked.status, 0);
    assert_eq!(declared.borrow().len(), 1);
    assert!(has(&declared.borrow()[0], "wallabag.fetch-due"));
    // Asked again, it is declared again (the host replaces the one that waits).
    assert_eq!(fetch(&p, true).status, 0);
    assert_eq!(declared.borrow().len(), 2);
}

#[test]
fn the_readers_own_run_fetches_at_once_and_declares_no_work() {
    let host = ScriptedHost::new();
    let declared = work(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // No account and no user interface in the scripted host: the fetch ends at
    // once (its callback may report that, as for the work that is due), nothing
    // is deferred.
    let _ = fetch(&p, false).status;
    assert!(declared.borrow().is_empty());
}

#[test]
fn a_work_the_host_refuses_costs_that_fetch_and_does_not_fail_the_callback() {
    let host = ScriptedHost::new();
    host.service("core.work", 1, 0, |_, _| Err(-1));
    host.reply_bound("core.work", 1, 64);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(fetch(&p, true).status, 0);
}

#[test]
fn without_a_work_service_a_rule_fetch_is_quiet() {
    let host = ScriptedHost::new();
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(fetch(&p, true).status, 0);
}

#[test]
fn the_work_that_is_due_runs_the_fetch_and_declares_nothing_more() {
    let host = ScriptedHost::new();
    let declared = work(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let due = event(
        &p,
        WORK_TOKEN,
        Record::EventWorkDue(EventWorkDue {
            now_ms: 0,
            token: WORK_TOKEN,
            why: 3,
            key: "wallabag.fetch-due",
        }),
    );
    // As the action does it now: no user interface to say the set-up is needed
    // in the scripted host, so the callback ends (not a panic, no new work).
    let _ = due.status;
    assert!(declared.borrow().is_empty());
}
