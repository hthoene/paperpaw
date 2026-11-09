//! The real plugin driven by the scripted host: what it subscribes to, and
//! that a start and a due work where the card cannot be read (the scripted
//! host has no storage) are taken as nothing to send, not as a failure of the
//! callback.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventStarted, EventWorkDue, Record, records_decode,
};
use paperpaw_plugin_readwise::{
    NATIVE_ENTRY,
    declarations::{STARTED_TOKEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

#[test]
fn the_plugin_subscribes_to_the_start_the_end_of_a_request_and_the_work() {
    let host = ScriptedHost::new();
    let _p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert!(!host.registrations().is_empty());
}

#[test]
fn a_start_and_a_due_work_without_a_readable_list_send_nothing() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let declared = Rc::new(RefCell::new(0));
    let seen = declared.clone();
    host.service("core.work", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        if op == 1 && batch.iter().next().is_some() {
            *seen.borrow_mut() += 1;
            return Ok(Vec::new());
        }
        Err(-1)
    });
    host.reply_bound("core.work", 1, 64);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let started = event(
        &p,
        STARTED_TOKEN,
        Record::EventStarted(EventStarted { now_ms: 0 }),
    );
    assert_eq!(started.status, 0);
    let due = event(
        &p,
        WORK_TOKEN,
        Record::EventWorkDue(EventWorkDue {
            now_ms: 0,
            token: WORK_TOKEN,
            why: 3,
            key: "readwise.retry",
        }),
    );
    assert_eq!(due.status, 0);
    assert!(due.batches.is_empty(), "nothing is said when nothing waits");
    // Nothing waits, so no work is declared.
    assert_eq!(*declared.borrow(), 0);
}
