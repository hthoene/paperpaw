//! The scripted host of the provider's tests: a `core.net` that answers
//! every request with one status and body, a `core.ui` that hands over a
//! word and a text, and the calls the host makes to the provider.
#![allow(dead_code, reason = "each test binary uses a part")]
use paperpaw_core::native_host::registrations::{
    EventHttpDone, EventScreenOpened, EventSuspend, Record, ServiceEnd, ServiceNetAnswer,
    ServiceNetBytes, ServiceNetStarted, ServiceUiParam, ServiceUiText, StageCall,
    record_encode_one, records_decode,
};
use paperpaw_plugin_wikipedia::declarations::{
    FETCHED_TOKEN, PROVIDER_TOKEN, SCREEN_TOKEN, SUSPEND_TOKEN,
};
use paperpaw_scripted_host::{Plugin, ScriptedHost, event_batch};
use paperpaw_sdk::native::stage_schemas::core_lookup_v1 as lookup;
use std::cell::RefCell;
use std::rc::Rc;

pub const SUMMARY: &str =
    r#"{"title":"Katze","extract":"Die Hauskatze ist eine Zuchtform der Falbkatze."}"#;
pub const EXTRACT: &str = "Die Hauskatze ist eine Zuchtform der Falbkatze.";
/// The host's reply buffer of a stage call (the stage's `max_reply`).
pub const REPLY: usize = lookup::MAX_REPLY;
pub const PENDING: u32 = 2;
pub const UNREACHABLE: &str =
    "Wikipedia could not be reached. Check the Wi-Fi connection and try again.";
pub const NOT_FOUND: &str = "Wikipedia has no article on this.";
/// The hand-over of the opened screen.
pub const HANDOVER: u32 = 9;

/// The requests asked for (`url token`).
pub type Log = Rc<RefCell<Vec<String>>>;

pub fn stream(records: &[Record<'_>]) -> Vec<u8> {
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
pub fn host_with_net(status: u32) -> (ScriptedHost, Log) {
    host_with(status, SUMMARY)
}

/// A host whose `core.net` answers every request with `status` and `body`.
pub fn host_with(status: u32, body: &'static str) -> (ScriptedHost, Log) {
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
                body_len: u32::try_from(body.len()).unwrap(),
                etag: "",
                content_type: "",
                final_url: "",
            })])),
            (4, Some(Record::ServiceNetRead(read))) => {
                let from = (read.offset as usize).min(body.len());
                let to = (from + 3840).min(body.len());
                Ok(stream(&[Record::ServiceNetBytes(ServiceNetBytes {
                    data: &body.as_bytes()[from..to],
                })]))
            }
            _ => Ok(Vec::new()),
        }
    });
    (host, log)
}

/// A `core.ui` whose hand-over [`HANDOVER`] carries the params `word` and
/// `language` and the text `text` (op 10).
pub fn ui_handing_over(host: &ScriptedHost, word: &'static str, text: String) {
    host.service("core.ui", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (10, Some(Record::ServiceUiTextAt(at))) if at.text == HANDOVER => {
                let from = (at.offset as usize).min(text.len());
                let to = (from + 3072).min(text.len());
                Ok(stream(&[Record::ServiceUiText(ServiceUiText {
                    text: at.text,
                    offset: at.offset,
                    total: u32::try_from(text.len()).unwrap(),
                    flags: 0,
                    data: &text[from..to],
                })]))
            }
            (17, _) => Ok(stream(&[
                Record::ServiceUiParam(ServiceUiParam {
                    key: "word",
                    value: word,
                }),
                Record::ServiceUiParam(ServiceUiParam {
                    key: "language",
                    value: "de",
                }),
                Record::ServiceEnd(ServiceEnd {
                    flags: 0,
                    total: 2,
                    generation: 0,
                    next: 0,
                    cursor: None,
                }),
            ])),
            _ => Err(-1),
        }
    });
}

/// The summary screen opened with the hand-over [`HANDOVER`]; its status.
pub fn open_screen(p: &Plugin<'_>) -> i32 {
    let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "wikipedia.lookup",
        instance: 1,
        slot: "",
        flags: 0,
        handover: HANDOVER,
        params: 2,
        text_bytes: 0,
    }))
    .unwrap();
    p.event(SCREEN_TOKEN, &opened).unwrap().status
}

/// The provider's answer to an ask of `text` in `language`: title, text,
/// flags and the screen it opens.
pub fn ask(p: &Plugin<'_>, text: &str, language: &str) -> (String, String, u32, String) {
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

/// The end of request `id` with `status`, `reason` and a body of
/// `body_len` bytes; whether `command.lookup_ready` was staged for the
/// ask of *Katze* in *de*.
pub fn done_with(p: &Plugin<'_>, id: u32, status: u32, reason: u32, body_len: usize) -> bool {
    let record = Record::EventHttpDone(EventHttpDone {
        now_ms: 0,
        id,
        token: FETCHED_TOKEN,
        status,
        reason,
        flags: 0,
        body_len: u32::try_from(body_len).unwrap(),
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

/// The end of request `id` answered with [`SUMMARY`] (reason 0) or none.
pub fn done(p: &Plugin<'_>, id: u32, status: u32, reason: u32) -> bool {
    let body_len = if reason == 0 { SUMMARY.len() } else { 0 };
    done_with(p, id, status, reason, body_len)
}

/// PaperPaw suspends.
pub fn suspend(p: &Plugin<'_>) {
    let record = Record::EventSuspend(EventSuspend { now_ms: 0 });
    let callback = p
        .event(SUSPEND_TOKEN, &event_batch(&record).unwrap())
        .unwrap();
    assert_eq!(callback.status, 0, "the suspend is taken");
}

/// A `core.ui` whose hand-over text says 5 bytes in its first part and 20
/// in its second (a text changed under the read).
pub fn ui_handing_over_a_growing_text(host: &ScriptedHost) {
    host.service("core.ui", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (10, Some(Record::ServiceUiTextAt(at))) => {
                let (total, data) = if at.offset == 0 {
                    (5, "abc")
                } else {
                    (20, "defghijklmnopqrst")
                };
                Ok(stream(&[Record::ServiceUiText(ServiceUiText {
                    text: at.text,
                    offset: at.offset,
                    total,
                    flags: 0,
                    data,
                })]))
            }
            (17, _) => Ok(stream(&[
                Record::ServiceUiParam(ServiceUiParam {
                    key: "word",
                    value: "Katze",
                }),
                Record::ServiceEnd(ServiceEnd {
                    flags: 0,
                    total: 1,
                    generation: 0,
                    next: 0,
                    cursor: None,
                }),
            ])),
            _ => Err(-1),
        }
    });
}
