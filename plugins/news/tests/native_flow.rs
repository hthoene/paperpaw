//! The real plugin driven by the scripted host: what it registers, the work it
//! declares for the background fetch, and what a due work does where the card
//! cannot be read (the scripted host has no storage, which the plugin must
//! take as an unreadable list, not as a failure of the callback).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventSettingChanged, EventWorkDue, Record, records_decode,
};
use paperpaw_plugin_news::{
    NATIVE_ENTRY,
    declarations::{FETCH_TOKEN, SETTING_TOKEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

/// `core.work`: every declared work is written down.
fn works(host: &ScriptedHost) -> Rc<RefCell<Vec<String>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    host.service("core.work", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        if let (1, Some(Record::ServiceWorkSpec(spec))) = (op, batch.iter().next()) {
            seen.borrow_mut().push(format!(
                "{} token {} needs {} flags {} moments {} after {}",
                spec.key, spec.token, spec.needs, spec.flags, spec.moments, spec.after_ms
            ));
            return Ok(Vec::new());
        }
        Err(-1)
    });
    host.reply_bound("core.work", 1, 64);
    log
}

#[test]
fn the_plugin_registers_its_setting_and_the_events_it_reads() {
    let host = ScriptedHost::new();
    let _p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // The setting for the articles a feed keeps, and the two subscriptions.
    let all: Vec<u8> = host.registrations().concat();
    assert!(host.registrations().len() >= 2);
    assert!(all.windows(21).any(|w| w == b"news.setting.articles"));
}

#[test]
fn the_fetch_action_declares_a_work_for_when_it_is_cheap() {
    let host = ScriptedHost::new();
    let log = works(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let run = event(
        &p,
        FETCH_TOKEN,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "news.fetch",
            flags: 0,
        }),
    );
    assert_eq!(run.status, 0);
    // Network (1), optional (1), at the wake and when online (1 | 2), an hour on.
    assert_eq!(
        *log.borrow(),
        [format!(
            "news.fetch token {WORK_TOKEN} needs 1 flags 1 moments 3 after 3600000"
        )]
    );
}

#[test]
fn a_work_that_cannot_be_declared_costs_that_fetch_only() {
    let host = ScriptedHost::new();
    host.service("core.work", 1, 0, |_, _| Err(-5));
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let run = event(
        &p,
        FETCH_TOKEN,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "news.fetch",
            flags: 0,
        }),
    );
    assert_eq!(run.status, 0);
}

#[test]
fn a_due_work_without_a_readable_list_says_so() {
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
            key: "news.fetch",
        }),
    );
    assert_eq!(due.status, 0);
    assert_eq!(due.batches.len(), 1, "the notice");
}

#[test]
fn a_setting_change_before_any_screen_is_ignored_and_the_plugin_still_works() {
    let host = ScriptedHost::new();
    let log = works(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // No state exists yet: the change is dropped and creates none.
    let changed = event(
        &p,
        SETTING_TOKEN,
        Record::EventSettingChanged(EventSettingChanged {
            now_ms: 0,
            key: "news.widget.count",
            old: &[],
        }),
    );
    assert_eq!(changed.status, 0);
    assert!(changed.commands.is_empty());
    let run = event(
        &p,
        FETCH_TOKEN,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "news.fetch",
            flags: 0,
        }),
    );
    assert_eq!(run.status, 0);
    assert_eq!(log.borrow().len(), 1);
}
