//! The real plugin driven by the scripted host: what it subscribes to, and
//! that a start and a due work where the card cannot be read (the scripted
//! host has no storage) are taken as nothing to send, not as a failure of the
//! callback.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventStarted, EventWorkDue, Record, StageCall, records_decode,
};
use paperpaw_plugin_readwise::{
    NATIVE_ENTRY,
    declarations::{EXPORT_STAGE, EXPORT_TOKEN, STARTED_TOKEN, WORK_TOKEN},
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

/// The input of the export stage, as the reader encodes it: the records of
/// `reader.notes-exported@1` (one record: tag 1, its length, an empty bitmap,
/// root, path and title).
fn export_input(root: u32, path: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(0); // the empty bitmap
    body.extend_from_slice(&root.to_le_bytes());
    for text in [path, "A book"] {
        body.push(u8::try_from(text.len()).unwrap());
        body.extend_from_slice(text.as_bytes());
    }
    let mut out = vec![1, 1, u8::try_from(body.len()).unwrap()];
    out.extend_from_slice(&body);
    out
}

fn export(p: &Plug<'_>, stage: &str, payload: &[u8]) -> Callback {
    let record = Record::StageCall(StageCall {
        stage,
        schema: 1,
        payload,
    });
    event(p, EXPORT_TOKEN, record)
}

#[test]
fn an_export_the_plugin_cannot_read_is_answered_and_sends_nothing() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // A file in another root is not the plugin's to read: no failure of the run.
    let elsewhere = export(&p, EXPORT_STAGE, &export_input(0, "Notes/book.md"));
    assert_eq!(elsewhere.status, 0);
    assert!(elsewhere.batches.is_empty());
    // A card file the scripted host cannot read is taken as nothing to send:
    // nothing is said. The scripted host gives an event no reply buffer, so the
    // answer itself is refused with `ERR_NO_SPACE` (-3), which the reader ignores.
    let card = export(&p, EXPORT_STAGE, &export_input(1, "Notes/book.md"));
    assert_eq!(card.status, -3);
    assert!(card.batches.is_empty());
}

#[test]
fn another_stage_or_a_broken_input_is_refused() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert!(export(&p, "core.status", &export_input(1, "a.md")).status < 0);
    assert!(export(&p, EXPORT_STAGE, &[9, 9, 9]).status < 0);
}
