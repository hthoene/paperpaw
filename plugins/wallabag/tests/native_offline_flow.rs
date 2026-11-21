//! The real plugin driven by the scripted host where Offline mode holds the
//! fetch back (#459, as Beta 3): the login request ends with `Reason::Offline`
//! (11) and the reader's fetch asks to go online (the host's confirm); *Go
//! online* sends `command.go_online` and fetches again, held back once more it
//! only says it needs the network; *Not now* says so and fetches nothing; a
//! fetch no reader started (a due work) stays quiet.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::commands::{WireCommand, commands_decode};
use paperpaw_core::native_host::environment::SettingValueRef;
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventHttpDone, EventWorkDue, Record, ServiceNetStarted, ServiceSecValue,
    record_encode_one, records_decode,
};
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_plugin_wallabag::{
    NATIVE_ENTRY,
    declarations::{FETCH_TOKEN, HTTP_TOKEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{Answer, Callback, Dialogs, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;
/// The requests sent, as `METHOD url`.
type Sent = Rc<RefCell<Vec<String>>>;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// A host with the texts (the plugin's and the core's), the account set up
/// (settings and both secrets), questions (`core.ui`) and a `core.net` that
/// starts every request (ids 1, 2, ...).
fn world(host: &ScriptedHost) -> (Dialogs, Sent) {
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    host.add_catalog(include_str!("../../../crates/paperpaw-core/i18n/en.ini"))
        .unwrap();
    for (key, value) in [
        ("wallabag.server", "w.example"),
        ("wallabag.user", "me"),
        ("wallabag.client-id", "cid"),
    ] {
        host.set_setting(key, SettingValueRef::Text(value));
    }
    host.service("core.secrets", 1, 0, |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceSecName(_))) => {
                Ok(stream(&[Record::ServiceSecValue(ServiceSecValue {
                    flags: 1,
                    label: "",
                    value: "s3cret",
                })]))
            }
            (1, _) => Err(ERR_NOT_FOUND),
            _ => Err(-1),
        }
    });
    let sent = Sent::default();
    let log = sent.clone();
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                let mut log = log.borrow_mut();
                log.push(format!("{} {}", request.method, request.url));
                Ok(stream(&[Record::ServiceNetStarted(ServiceNetStarted {
                    id: u32::try_from(log.len()).unwrap(),
                })]))
            }
            (2, _) => Ok(Vec::new()),
            _ => Err(-1),
        }
    });
    (host.dialogs(), sent)
}

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

/// The reader's own run of `wallabag.fetch`.
fn fetch(p: &Plug<'_>) -> Callback {
    event(
        p,
        FETCH_TOKEN,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "wallabag.fetch",
            flags: 0,
        }),
    )
}

/// The end of request `id`, held back by Offline mode (reason 11).
fn held_back(p: &Plug<'_>, id: u32) -> Callback {
    event(
        p,
        HTTP_TOKEN,
        Record::EventHttpDone(EventHttpDone {
            now_ms: 0,
            id,
            token: 1,
            status: 0,
            reason: 11,
            flags: 0,
            body_len: 0,
        }),
    )
}

/// Whether the callback sent `command.go_online`.
fn went_online(callback: &Callback) -> bool {
    callback.batches.iter().any(|batch| {
        commands_decode(batch)
            .unwrap()
            .iter()
            .any(|command| matches!(command, WireCommand::GoOnline { for_ms: 300_000 }))
    })
}

/// What `wallabag.no-network` says in English.
const NO_NETWORK: &str = "Wallabag needs a network.";
const LOGIN: &str = "POST https://w.example/oauth/v2/token";

#[test]
fn offline_mode_offers_to_go_online_once_and_go_online_fetches_again() {
    let host = ScriptedHost::new();
    let (dialogs, sent) = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(fetch(&p).status, 0);
    assert_eq!(*sent.borrow(), [LOGIN]);
    // Held back: the host's question, *Go online* focused.
    assert_eq!(held_back(&p, 1).status, 0);
    let asked = dialogs.last().expect("the offer is asked");
    assert_eq!(asked.kind, 2, "a confirm");
    assert_eq!(asked.title, "Offline mode");
    assert_eq!(asked.choices, "Go online\nNot now");
    // *Go online*: the network is asked for and the fetch starts again.
    let online = p
        .event(
            0,
            &dialogs
                .answer(asked.request, &Answer::Confirm(true))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(online.status, 0);
    assert!(went_online(&online), "command.go_online is sent");
    assert_eq!(*sent.borrow(), [LOGIN, LOGIN]);
    // Held back again (the host did not go online): offered once, it says it
    // needs the network.
    let questions = dialogs.questions().len();
    let again = held_back(&p, 2);
    assert_eq!(again.status, 0);
    assert_eq!(dialogs.questions().len(), questions, "not offered again");
    assert!(again.batches_contain(NO_NETWORK.as_bytes()), "said");
    assert_eq!(*sent.borrow(), [LOGIN, LOGIN]);
    // The reader fetches anew: it may be offered again.
    assert_eq!(fetch(&p).status, 0);
    assert_eq!(held_back(&p, 3).status, 0);
    assert_eq!(dialogs.questions().len(), questions + 1);
}

#[test]
fn not_now_says_it_needs_the_network_and_fetches_nothing() {
    let host = ScriptedHost::new();
    let (dialogs, sent) = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    fetch(&p);
    held_back(&p, 1);
    let asked = dialogs.last().unwrap();
    let not_now = p
        .event(
            0,
            &dialogs
                .answer(asked.request, &Answer::Confirm(false))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(not_now.status, 0);
    assert!(!went_online(&not_now));
    assert!(not_now.batches_contain(NO_NETWORK.as_bytes()), "said");
    assert_eq!(*sent.borrow(), [LOGIN], "nothing is fetched again");
}

#[test]
fn a_fetch_no_reader_started_never_asks() {
    let host = ScriptedHost::new();
    let (dialogs, sent) = world(&host);
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
    assert_eq!(due.status, 0);
    assert_eq!(*sent.borrow(), [LOGIN]);
    let quiet = held_back(&p, 1);
    assert_eq!(quiet.status, 0);
    assert!(dialogs.questions().is_empty(), "quiet: no offer");
    assert!(quiet.batches_contain(NO_NETWORK.as_bytes()), "said");
}
