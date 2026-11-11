//! The real plugin driven by the scripted host: what it subscribes to, that
//! the action declares the fetch as work, that a fetch without a network says
//! so, and that a page turn and the plugin's stopping, where there is nothing
//! to note or to cancel, are taken as that and not as a failure of the
//! callback. (The fetch itself is tested over stand-ins in
//! `native_fetch_tests.rs`.)
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventPageTurned, EventStopping, EventWorkDue, Record, records_decode,
};
use paperpaw_plugin_read_later_cloud::{
    NATIVE_ENTRY,
    declarations::{FETCH_TOKEN, PAGE_TOKEN, STOP_TOKEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

#[test]
fn the_plugin_subscribes_to_the_end_of_a_request_page_turns_work_and_stopping() {
    let host = ScriptedHost::new();
    let _p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert!(!host.registrations().is_empty());
}

#[test]
fn the_fetch_action_declares_the_fetch_as_work() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let declared = Rc::new(RefCell::new(Vec::new()));
    let seen = declared.clone();
    host.service("core.work", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        if op == 1 && batch.iter().next().is_some() {
            seen.borrow_mut().push(input.len());
            return Ok(Vec::new());
        }
        Err(-1)
    });
    host.reply_bound("core.work", 1, 64);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let run = event(
        &p,
        FETCH_TOKEN,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "fetch",
            flags: 0,
        }),
    );
    assert_eq!(run.status, 0);
    assert_eq!(declared.borrow().len(), 1);
}

#[test]
fn a_fetch_without_a_network_says_so() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let due = event(
        &p,
        WORK_TOKEN,
        Record::EventWorkDue(EventWorkDue {
            now_ms: 0,
            token: WORK_TOKEN,
            why: 3,
            key: "read-later-cloud.fetch",
        }),
    );
    assert_eq!(due.status, 0);
    assert!(
        due.batches_contain(b"Fetching articles needs a network"),
        "{:?}",
        due.batches
    );
}

#[test]
fn a_page_turn_with_nothing_to_note_and_a_stop_with_nothing_to_cancel_change_nothing() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let turned = event(
        &p,
        PAGE_TOKEN,
        Record::EventPageTurned(EventPageTurned {
            now_ms: 0,
            section: 0,
            offset: 0,
            progress_ppm: 1_000_000,
            path: "Books/Reader/Some article.html",
            title: "Some article",
            chapter: "",
            page: Some(5),
            pages: Some(5),
            insert: None,
            chapter_left: None,
        }),
    );
    assert_eq!(turned.status, 0);
    assert!(turned.batches.is_empty());
    let stopped = event(
        &p,
        STOP_TOKEN,
        Record::EventStopping(EventStopping { now_ms: 0 }),
    );
    assert_eq!(stopped.status, 0);
    assert!(stopped.batches.is_empty());
}
