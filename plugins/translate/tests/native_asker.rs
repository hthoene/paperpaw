//! The calls that ask DeepL and keep the key, run against scripted
//! `core.net` and `core.secrets` services by a probe plugin that drives
//! `Asker` from events (the real plugin starts them from its panel, slice 3):
//! the request as the host is given it (the method, the address, the three
//! headers with the key in `Authorization`, the form inline), the answer and
//! the body read in pieces while `http_done` is delivered, what each status
//! says, a request cancelled by a newer one, the key kept as the plugin's own
//! secret (and `ERR_UNSUPPORTED` as [`Kept::Unsupported`]), and that no
//! refusal makes anything abort.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventHttpDone, Record, ServiceNetAnswer, ServiceNetBytes, ServiceNetStarted, ServiceSecValue,
    record_encode_one, records_decode,
};
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_plugin_translate::native_ask::{Asker, KEY_LABEL, KEY_NAME, Kept, REQUEST_TOKEN};
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    declare::{Declarations, EventKind},
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
};
use std::cell::RefCell;
use std::rc::Rc;

const ASK: u32 = 1;
const KEEP: u32 = 2;
const KEY: u32 = 3;
const DONE: u32 = 4;
const CANCEL: u32 = 5;

thread_local! {
    /// What the probe is told to ask, and what it saw.
    static ARGS: RefCell<[String; 3]> = const { RefCell::new([String::new(), String::new(), String::new()]) };
    static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn say(line: String) {
    SEEN.with(|seen| seen.borrow_mut().push(line));
}

fn seen() -> Vec<String> {
    SEEN.with(|seen| seen.take())
}

struct Probe(Asker);

impl Plugin for Probe {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        // The host delivers an event to a token that a subscription gave it.
        let mut batch = Vec::new();
        let mut d = Declarations::new(|bytes: &[u8]| -> Result<(), i32> {
            batch.extend_from_slice(bytes);
            Ok(())
        });
        for (kind, token) in [
            (EventKind::Started, ASK),
            (EventKind::Resume, KEEP),
            (EventKind::Suspend, KEY),
            (EventKind::Stopping, DONE),
            (EventKind::PageTurned, CANCEL),
        ] {
            d.subscribe_event(kind, token).map_err(|_| Error::Invalid)?;
        }
        d.submit().map_err(|_| Error::Invalid)?;
        drop(d);
        registrations::submit(host, &batch)?;
        Ok(Self(Asker::new()))
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let [a, b, c] = ARGS.with(|args| args.borrow().clone());
        match token {
            ASK => {
                let result = self.0.ask(host, &a, &b, &c);
                say(format!("ask {result:?} waiting {}", self.0.waiting()));
                result
            }
            KEEP => {
                let result = self.0.keep_key(host, &a);
                say(format!("keep {result:?}"));
                result.map(|_| ())
            }
            KEY => {
                let key = self.0.key(host)?;
                say(format!("key {:?}", key.as_ref().map(|k| k.as_str())));
                Ok(())
            }
            CANCEL => {
                self.0.cancel(host);
                say(format!("cancelled waiting {}", self.0.waiting()));
                Ok(())
            }
            DONE => {
                if let Some(Event::HttpDone(done)) = incoming.decode_as::<kinds::HttpDone>()? {
                    let outcome = self.0.done(host, &done)?;
                    say(format!("done {outcome:?}"));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = PROBE_ENTRY
);

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// What the network was asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

/// A scripted `core.net`: `status` and `body` answer every request, which gets
/// the ids 1, 2, ...
fn net(host: &ScriptedHost, log: Log, status: u32, body: &'static str) {
    let next = RefCell::new(0_u32);
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                let mut line = format!("request {} {}", request.method, request.url);
                for row in &rows[1..] {
                    match row {
                        Record::ServiceNetHeader(h) => {
                            line += &format!(" [{}: {}]", h.name, h.value);
                        }
                        Record::ServiceNetBody(b) => {
                            line += &format!(" body {}", String::from_utf8_lossy(b.data));
                        }
                        Record::ServiceNetFile(f) => line += &format!(" file {}", f.path),
                        _ => return Err(-1),
                    }
                }
                line += &format!(" max_body {}", request.max_body);
                log.borrow_mut().push(line);
                *next.borrow_mut() += 1;
                Ok(stream(&[Record::ServiceNetStarted(ServiceNetStarted {
                    id: *next.borrow(),
                })]))
            }
            (2, Some(Record::ServiceNetToken(token))) => {
                log.borrow_mut().push(format!("cancel {}", token.token));
                Ok(Vec::new())
            }
            (3, Some(Record::ServiceNetId(id))) => {
                log.borrow_mut().push(format!("answer {}", id.id));
                Ok(stream(&[Record::ServiceNetAnswer(ServiceNetAnswer {
                    status,
                    flags: 0,
                    body_len: u32::try_from(body.len()).unwrap(),
                    etag: "",
                    content_type: "",
                    final_url: "",
                })]))
            }
            (4, Some(Record::ServiceNetRead(read))) => {
                log.borrow_mut().push(format!("body {}", read.offset));
                let from = (read.offset as usize).min(body.len());
                // Pieces of 5 bytes: the body is read in several calls.
                let to = (from + 5).min(body.len());
                Ok(stream(&[Record::ServiceNetBytes(ServiceNetBytes {
                    data: &body.as_bytes()[from..to],
                })]))
            }
            _ => Err(-1),
        }
    });
}

/// A scripted `core.secrets` with one secret slot.
fn secrets(host: &ScriptedHost, kept: Rc<RefCell<Option<(String, String)>>>, unsupported: bool) {
    host.service("core.secrets", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, Some(Record::ServiceSecName(name))) => {
                assert_eq!((name.plugin, name.name), ("", KEY_NAME));
                match &*kept.borrow() {
                    Some((label, value)) => {
                        Ok(stream(&[Record::ServiceSecValue(ServiceSecValue {
                            flags: 1,
                            label,
                            value,
                        })]))
                    }
                    None => Err(ERR_NOT_FOUND),
                }
            }
            (3, Some(Record::ServiceSecKeep(keep))) => {
                assert_eq!((keep.plugin, keep.name), ("", KEY_NAME));
                assert_eq!(keep.flags, 1, "changeable in the list of keys");
                if unsupported {
                    return Err(ERR_UNSUPPORTED);
                }
                *kept.borrow_mut() = Some((keep.label.to_string(), keep.value.to_string()));
                Ok(Vec::new())
            }
            _ => Err(-1),
        }
    });
}

fn tell(args: [&str; 3]) {
    ARGS.with(|a| *a.borrow_mut() = args.map(String::from));
}

fn done(id: u32, status: u32, reason: u32, body_len: usize) -> Vec<u8> {
    event_batch(&Record::EventHttpDone(EventHttpDone {
        now_ms: 0,
        id,
        token: REQUEST_TOKEN,
        status,
        reason,
        flags: 0,
        body_len: u32::try_from(body_len).unwrap(),
    }))
    .unwrap()
}

/// A batch for the events the probe does not read.
fn ping() -> Vec<u8> {
    done(1, 0, 0, 0)
}

const ANSWER: &str = r#"{"translations":[{"detected_source_language":"EN","text":"Hallo"}]}"#;

#[test]
fn a_request_goes_out_as_interface_6_sent_it_and_the_translation_comes_back() {
    let host = ScriptedHost::new();
    let log = Log::default();
    net(&host, log.clone(), 200, ANSWER);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    tell(["abc:fx", "Hello world", "DE"]);
    assert_eq!(plugin.event(ASK, &ping()).unwrap().status, 0);
    assert_eq!(
        *log.borrow(),
        ["request POST https://api-free.deepl.com/v2/translate \
          [Authorization: DeepL-Auth-Key abc:fx] \
          [Content-Type: application/x-www-form-urlencoded] \
          [User-Agent: PaperPaw/0.1 (e-reader)] \
          body text=Hello+world&target_lang=DE max_body 24576"]
    );
    assert_eq!(seen(), ["ask Ok(()) waiting true"]);
    log.borrow_mut().clear();
    let batch = done(1, 200, 0, ANSWER.len());
    assert_eq!(plugin.event(DONE, &batch).unwrap().status, 0);
    assert_eq!(seen(), ["done Some(Translated(\"Hallo\"))"]);
    // The head, then the body in pieces of five bytes (the scripted ones).
    assert_eq!(log.borrow()[0], "body 0");
    assert!(log.borrow().len() > 10);
    // Nothing waits any more: the same end again is not it.
    log.borrow_mut().clear();
    assert_eq!(plugin.event(DONE, &batch).unwrap().status, 0);
    assert_eq!(seen(), ["done None"]);
    assert!(log.borrow().is_empty());
    assert_eq!(
        host.open_handles(),
        1,
        "the net handle, released with the plugin"
    );
}

#[test]
fn the_paid_service_is_asked_with_a_key_that_does_not_end_in_fx() {
    let host = ScriptedHost::new();
    let log = Log::default();
    net(&host, log.clone(), 200, ANSWER);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    tell(["secret-key", "Hi", "EN-GB"]);
    assert_eq!(plugin.event(ASK, &ping()).unwrap().status, 0);
    assert!(log.borrow()[0].starts_with("request POST https://api.deepl.com/v2/translate "));
    seen();
}

#[test]
fn each_status_says_what_interface_6_said() {
    for (status, reason, body, expected) in [
        (401, 0, "", "BadKey"),
        (403, 0, "", "BadKey"),
        (456, 0, "", "Quota"),
        (500, 0, "", "Failed"),
        (200, 0, "{}", "Failed"),
        (0, 1, "", "Unreachable"),
        (0, 2, "", "Unreachable"),
        (0, 4, "", "Unreachable"),
        (0, 11, "", "Unreachable"),
    ] {
        let host = ScriptedHost::new();
        net(&host, Log::default(), status, body);
        let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
        tell(["k", "Hi", "DE"]);
        plugin.event(ASK, &ping()).unwrap();
        seen();
        let batch = done(1, status, reason, body.len());
        assert_eq!(plugin.event(DONE, &batch).unwrap().status, 0);
        assert_eq!(
            seen(),
            [format!("done Some({expected})")],
            "{status}/{reason}"
        );
    }
}

#[test]
fn a_request_cancelled_or_replaced_says_nothing_and_a_newer_one_cancels_the_older() {
    let host = ScriptedHost::new();
    let log = Log::default();
    net(&host, log.clone(), 200, ANSWER);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    tell(["k", "One", "DE"]);
    plugin.event(ASK, &ping()).unwrap();
    plugin.event(ASK, &ping()).unwrap();
    assert_eq!(
        log.borrow()
            .iter()
            .map(|l| l.split(' ').next().unwrap())
            .collect::<Vec<_>>(),
        ["request", "cancel", "request"]
    );
    seen();
    // The first one ends, cancelled: it is not the one that waits (id 2).
    plugin.event(DONE, &done(1, 0, 8, 0)).unwrap();
    assert_eq!(seen(), ["done None"]);
    plugin.event(CANCEL, &ping()).unwrap();
    assert_eq!(seen(), ["cancelled waiting false"]);
    // The cancelled second ends: nobody waits for it.
    plugin.event(DONE, &done(2, 0, 8, 0)).unwrap();
    assert_eq!(seen(), ["done None"]);
}

#[test]
fn a_long_text_that_does_not_fit_inline_goes_through_a_file_or_is_refused_not_cut() {
    let host = ScriptedHost::new();
    let log = Log::default();
    net(&host, log.clone(), 200, ANSWER);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    // 3,000 Cyrillic letters: far over 2,048 bytes once encoded. The scripted
    // host has no storage, so the form cannot be written and the request is
    // refused whole: it never goes out cut.
    let long = "ж".repeat(3000);
    tell(["k", &long, "DE"]);
    plugin.event(ASK, &ping()).unwrap();
    assert!(seen()[0].starts_with("ask Err("));
    assert!(log.borrow().is_empty());
}

#[test]
fn the_key_is_kept_trimmed_as_the_plugins_own_secret_and_read_back() {
    let host = ScriptedHost::new();
    let kept = Rc::default();
    secrets(&host, Rc::clone(&kept), false);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    plugin.event(KEY, &ping()).unwrap();
    assert_eq!(seen(), ["key None"]);
    tell(["  abc:fx \n", "", ""]);
    plugin.event(KEEP, &ping()).unwrap();
    assert_eq!(seen(), ["keep Ok(Saved)"]);
    assert_eq!(*kept.borrow(), Some((KEY_LABEL.into(), "abc:fx".into())));
    plugin.event(KEY, &ping()).unwrap();
    assert_eq!(seen(), ["key Some(\"abc:fx\")"]);
    // Nothing typed changes nothing.
    tell(["   ", "", ""]);
    plugin.event(KEEP, &ping()).unwrap();
    assert_eq!(seen(), ["keep Ok(Empty)"]);
    assert_eq!(kept.borrow().as_ref().unwrap().1, "abc:fx");
}

#[test]
fn a_host_that_cannot_keep_passwords_is_told_apart() {
    let host = ScriptedHost::new();
    secrets(&host, Rc::default(), true);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    tell(["abc:fx", "", ""]);
    plugin.event(KEEP, &ping()).unwrap();
    assert_eq!(seen(), ["keep Ok(Unsupported)"]);
    let _ = Kept::Saved;
}

#[test]
fn an_empty_kept_key_is_no_key() {
    let host = ScriptedHost::new();
    secrets(
        &host,
        Rc::new(RefCell::new(Some((String::new(), String::new())))),
        false,
    );
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    plugin.event(KEY, &ping()).unwrap();
    assert_eq!(seen(), ["key None"]);
}

#[test]
fn no_refusal_makes_asking_or_reading_abort() {
    let results = sweep(Slot::ServiceCall, -5, |host, _| {
        net(host, Log::default(), 200, ANSWER);
        secrets(host, Rc::default(), false);
        let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
        tell(["abc", "Hello", "DE"]);
        let mut worst = 0;
        for (token, batch) in [
            (KEEP, ping()),
            (KEY, ping()),
            (ASK, ping()),
            (DONE, done(1, 200, 0, ANSWER.len())),
        ] {
            let status = plugin.event(token, &batch).map_or_else(|s| s, |c| c.status);
            worst = worst.min(status);
        }
        seen();
        worst
    });
    assert_eq!(results[0].1, 0);
    assert!(results.len() > 8, "{}", results.len());
    assert!(results.iter().skip(1).all(|(_, status)| *status <= 0));
    let results = sweep(Slot::ServiceOpen, -5, |host, _| {
        net(host, Log::default(), 200, ANSWER);
        secrets(host, Rc::default(), false);
        let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
        tell(["abc", "Hello", "DE"]);
        let status = plugin.event(ASK, &ping()).map_or_else(|s| s, |c| c.status);
        seen();
        status
    });
    assert!(results.iter().skip(1).all(|(_, status)| *status <= 0));
}
