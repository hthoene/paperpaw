//! The action `opds.sync` by who runs it: the reader's own run syncs at once
//! and declares no work; a background run (a rule, another plugin) declares
//! the sync as work for a good moment (the scripted host has no catalogs, so
//! the sync that starts at once ends with its notice and does not fail).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{EventActionRun, Record};
use paperpaw_plugin_opds::{NATIVE_ENTRY, declarations::SYNC_TOKEN};
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn sync(p: &Plug<'_>, background: bool) -> Callback {
    let record = Record::EventActionRun(EventActionRun {
        now_ms: 0,
        action: "opds.sync",
        flags: u32::from(background),
    });
    p.event(SYNC_TOKEN, &event_batch(&record).unwrap()).unwrap()
}

/// A scripted `core.ui` that takes every request (the plugin opens it first).
fn ui(host: &ScriptedHost) {
    host.service("core.ui", 1, 0, |_, _| Ok(Vec::new()));
    host.reply_bound("core.ui", 1, 64);
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
fn a_background_run_declares_the_sync_as_work() {
    let host = ScriptedHost::new();
    ui(&host);
    let declared = work(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(sync(&p, true).status, 0);
    assert_eq!(declared.borrow().len(), 1);
    assert!(has(&declared.borrow()[0], "opds.sync"));
    // Asked again, it is declared again (the host replaces the one that waits).
    assert_eq!(sync(&p, true).status, 0);
    assert_eq!(declared.borrow().len(), 2);
}

#[test]
fn the_readers_own_run_syncs_at_once_and_declares_no_work() {
    let host = ScriptedHost::new();
    ui(&host);
    let declared = work(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // The scripted host has no catalogs or user interface: the callback may
    // report that; nothing is deferred.
    let _ = sync(&p, false).status;
    assert!(declared.borrow().is_empty());
}

#[test]
fn a_background_run_without_a_work_service_does_not_fail() {
    let host = ScriptedHost::new();
    ui(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // It syncs at once instead (no catalogs here: the callback may report it).
    let _ = sync(&p, true).status;
}
