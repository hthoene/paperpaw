//! The real plugin as a lookup provider, driven by the scripted host: an ask
//! is answered *pending* and fetched once, the end of the request keeps the
//! answer and tells the askers (`lookup_ready`), and the next ask is answered
//! from what was kept, with the summary screen to open (Beta 3's dictionary
//! panel: *Looking it up …*, then the summary with *Open Wikipedia ›*). Where
//! the network is not reached the answer says so, as in Beta 3.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventHttpDone, Record, ServiceNetAnswer, ServiceNetBytes, ServiceNetStarted, StageCall,
    record_encode_one, records_decode,
};
use paperpaw_plugin_wikipedia::{
    NATIVE_ENTRY,
    declarations::{FETCHED_TOKEN, PROVIDER_TOKEN},
};
use paperpaw_scripted_host::{ScriptedHost, event_batch};
use paperpaw_sdk::native::stage_schemas::core_lookup_v1 as lookup;
use std::cell::RefCell;
use std::rc::Rc;

const SUMMARY: &str =
    r#"{"title":"Katze","extract":"Die Hauskatze ist eine Zuchtform der Falbkatze."}"#;
/// The host's reply buffer of a stage call (the stage's `max_reply`).
const REPLY: usize = lookup::MAX_REPLY;
const PENDING: u32 = 2;
const UNREACHABLE: &str =
    "Wikipedia could not be reached. Check the Wi-Fi connection and try again.";

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// A host whose `core.net` answers every request with `status` and
/// [`SUMMARY`]; the requests are logged.
fn host_with_net(status: u32) -> (ScriptedHost, Rc<RefCell<Vec<String>>>) {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                seen.borrow_mut()
                    .push(format!("{} {}", request.url, request.token));
                let id = u32::try_from(seen.borrow().len()).unwrap();
                Ok(stream(&[Record::ServiceNetStarted(ServiceNetStarted {
                    id,
                })]))
            }
            (3, _) => Ok(stream(&[Record::ServiceNetAnswer(ServiceNetAnswer {
                status,
                flags: 0,
                body_len: u32::try_from(SUMMARY.len()).unwrap(),
                etag: "",
                content_type: "",
                final_url: "",
            })])),
            (4, Some(Record::ServiceNetRead(read))) => {
                let from = (read.offset as usize).min(SUMMARY.len());
                Ok(stream(&[Record::ServiceNetBytes(ServiceNetBytes {
                    data: &SUMMARY.as_bytes()[from..],
                })]))
            }
            _ => Ok(Vec::new()),
        }
    });
    (host, log)
}

/// The provider's answer to an ask of `text` in `language`: title, text,
/// flags and the screen it opens.
fn ask(
    p: &paperpaw_scripted_host::Plugin<'_>,
    text: &str,
    language: &str,
) -> (String, String, u32, String) {
    let mut payload = vec![0; lookup::MAX_INPUT];
    let used = lookup::input_encode(
        &[lookup::Record::LookupAsk(lookup::LookupAsk::new(
            text, language, "", 0,
        ))],
        &mut payload,
    )
    .unwrap();
    let call = Record::StageCall(StageCall {
        stage: lookup::KEY,
        schema: 1,
        payload: &payload[..used],
    });
    let (callback, reply) = p
        .stage_call(PROVIDER_TOKEN, &event_batch(&call).unwrap(), REPLY)
        .unwrap();
    assert_eq!(callback.status, 0, "the ask is answered");
    let records = lookup::reply_decode(&reply).unwrap();
    let mut entries = records.filter_map(|record| match record.unwrap() {
        lookup::Record::LookupEntry(entry) => Some((
            entry.title.to_string(),
            entry.text.to_string(),
            entry.flags,
            entry.opens.to_string(),
        )),
        _ => None,
    });
    let entry = entries.next().expect("one entry");
    assert!(entries.next().is_none(), "exactly one entry");
    entry
}

fn done(p: &paperpaw_scripted_host::Plugin<'_>, id: u32, status: u32, reason: u32) -> bool {
    let record = Record::EventHttpDone(EventHttpDone {
        now_ms: 0,
        id,
        token: FETCHED_TOKEN,
        status,
        reason,
        flags: 0,
        body_len: if reason == 0 {
            u32::try_from(SUMMARY.len()).unwrap()
        } else {
            0
        },
    });
    let callback = p
        .event(FETCHED_TOKEN, &event_batch(&record).unwrap())
        .unwrap();
    assert_eq!(callback.status, 0, "the end of the request is taken");
    // `command.lookup_ready` carries the ask's text and language.
    callback.batches.iter().any(|batch| {
        batch.windows(5).any(|w| w == b"Katze") && batch.windows(2).any(|w| w == b"de")
    })
}

#[test]
fn an_ask_is_pending_then_ready_and_answered_from_what_was_kept() {
    let (host, log) = host_with_net(200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let (title, text, flags, _) = ask(&p, "Katze", "de");
    assert_eq!((title.as_str(), text.as_str()), ("Encyclopedia", ""));
    assert_eq!(flags & PENDING, PENDING, "answered later");
    assert_eq!(
        log.borrow().as_slice(),
        [format!(
            "https://de.wikipedia.org/api/rest_v1/page/summary/Katze {FETCHED_TOKEN}"
        )]
    );
    // Asked again while it is fetched: still pending, not fetched twice.
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    assert_eq!(log.borrow().len(), 1);
    done(&p, 1, 200, 0);
    let (title, text, flags, opens) = ask(&p, "Katze", "de");
    assert_eq!(title, "Encyclopedia");
    assert_eq!(text, "Die Hauskatze ist eine Zuchtform der Falbkatze.");
    assert_eq!(flags & PENDING, 0);
    assert_eq!(opens, "wikipedia.lookup", "Open Wikipedia ›");
    assert_eq!(log.borrow().len(), 1, "answered from what was kept");
}

#[test]
fn without_a_network_service_the_answer_says_the_network_was_not_reached() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let (title, text, flags, opens) = ask(&p, "Katze", "de");
    assert_eq!(title, "Encyclopedia");
    assert_eq!(text, UNREACHABLE);
    assert_eq!((flags & PENDING, opens.as_str()), (0, ""));
}

#[test]
fn a_request_that_does_not_get_through_is_kept_as_not_reached() {
    let (host, _log) = host_with_net(0);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    // Reason 1: no network came.
    done(&p, 1, 0, 1);
    let (_, text, flags, opens) = ask(&p, "Katze", "de");
    assert_eq!(text, UNREACHABLE);
    assert_eq!((flags & PENDING, opens.as_str()), (0, ""));
}

#[test]
fn another_request_ending_is_not_the_providers() {
    let (host, _log) = host_with_net(200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    assert!(
        !done(&p, 7, 200, 0),
        "no ready for a request it did not make"
    );
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
}

/// The end of the request tells the askers: `command.lookup_ready` with the
/// ask's text and language. The host does not stage the command yet (it is
/// refused, and the provider keeps its answer all the same).
#[test]
#[ignore = "needs the host to stage command.lookup_ready (separate step after #1387)"]
fn the_end_of_the_request_tells_the_askers() {
    let (host, _log) = host_with_net(200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    assert!(done(&p, 1, 200, 0), "lookup_ready staged");
}
