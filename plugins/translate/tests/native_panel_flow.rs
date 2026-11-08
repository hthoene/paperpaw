//! The real plugin driven by the scripted host through the panel's life: the
//! selected text read from the hand-over, the request sent when the panel is
//! first shown, an end that shows the translation (a redraw), no key (nothing is
//! sent, the key is asked for on the keyboard, kept and used), the offer to go
//! online (once) and the closing key. What the panel draws needs the font and
//! is checked by its state tests (`src/native_panel/tests.rs`).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventAnswered, EventHttpDone, EventScreenClosed, EventScreenOpened, EventScreenShown,
    InputAction, Record, ServiceNetAnswer, ServiceNetBytes, ServiceNetStarted, ServiceSecValue,
    ServiceUiRequest, ServiceUiText, record_encode_one, records_decode,
};
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_plugin_translate::{
    NATIVE_ENTRY,
    declarations::{HTTP_TOKEN, PANEL_TOKEN},
};
use paperpaw_scripted_host::{Callback, ScriptedHost, Slot, event_batch, sweep};
use std::cell::RefCell;
use std::rc::Rc;

const INSTANCE: u32 = 4;
const HANDOVER: u32 = 7;
const KEY_REQUEST: u32 = 9;
const ANSWER: &str = r#"{"translations":[{"detected_source_language":"EN","text":"Hallo"}]}"#;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// What the services were asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

struct World {
    log: Log,
    /// The key kept, if any.
    key: Rc<RefCell<Option<String>>>,
    /// What the text of the request for the key says.
    typed: Rc<RefCell<String>>,
}

/// Scripts the three services: `core.ui` (the hand-over's text, the question for
/// the key and the text typed), `core.net` (every request ends with `status`),
/// `core.secrets` (one key; `unsupported` refuses keeping it).
fn world(
    host: &ScriptedHost,
    selected: &'static str,
    key: Option<&str>,
    unsupported: bool,
) -> World {
    let log = Log::default();
    let kept = Rc::new(RefCell::new(key.map(str::to_string)));
    let typed = Rc::new(RefCell::new(String::new()));
    ui_service(host, selected, &log, &typed);
    secrets_service(host, unsupported, &log, &kept);
    net_service(host, &log);
    World {
        log,
        key: kept,
        typed,
    }
}

/// The `core.ui` service: the hand-over's text, the question for the key and
/// the text typed.
fn ui_service(host: &ScriptedHost, selected: &'static str, log: &Log, typed: &Rc<RefCell<String>>) {
    let (l, t) = (log.clone(), typed.clone());
    host.service("core.ui", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (10, Some(Record::ServiceUiTextAt(at))) => {
                let text = if at.text == HANDOVER {
                    selected.to_string()
                } else {
                    t.borrow().clone()
                };
                l.borrow_mut().push(format!("text {}", at.text));
                let from = (at.offset as usize).min(text.len());
                Ok(stream(&[Record::ServiceUiText(ServiceUiText {
                    text: at.text,
                    offset: at.offset,
                    total: u32::try_from(text.len()).unwrap(),
                    flags: 0,
                    data: &text[from..],
                })]))
            }
            (8, Some(Record::ServiceUiAsk(ask))) => {
                l.borrow_mut()
                    .push(format!("ask kind {} flags {}", ask.kind, ask.flags));
                Ok(stream(&[Record::ServiceUiRequest(ServiceUiRequest {
                    request: KEY_REQUEST,
                })]))
            }
            _ => Err(-1),
        }
    });
}

/// The `core.secrets` service: one key; `unsupported` refuses keeping it.
fn secrets_service(
    host: &ScriptedHost,
    unsupported: bool,
    log: &Log,
    kept: &Rc<RefCell<Option<String>>>,
) {
    let (l, k) = (log.clone(), kept.clone());
    host.service("core.secrets", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, Some(Record::ServiceSecName(_))) => match &*k.borrow() {
                Some(value) => Ok(stream(&[Record::ServiceSecValue(ServiceSecValue {
                    flags: 1,
                    label: "",
                    value,
                })])),
                None => Err(ERR_NOT_FOUND),
            },
            (3, Some(Record::ServiceSecKeep(keep))) => {
                l.borrow_mut().push("keep".to_string());
                if unsupported {
                    return Err(ERR_UNSUPPORTED);
                }
                *k.borrow_mut() = Some(keep.value.to_string());
                Ok(Vec::new())
            }
            _ => Err(-1),
        }
    });
}

/// The `core.net` service: every request ends with `status` 200.
fn net_service(host: &ScriptedHost, log: &Log) {
    let (l, next) = (log.clone(), RefCell::new(0_u32));
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                let body = rows.iter().find_map(|r| match r {
                    Record::ServiceNetBody(b) => Some(String::from_utf8_lossy(b.data).to_string()),
                    _ => None,
                });
                let auth = rows.iter().find_map(|r| match r {
                    Record::ServiceNetHeader(h) if h.name == "Authorization" => {
                        Some(h.value.to_string())
                    }
                    _ => None,
                });
                l.borrow_mut().push(format!(
                    "request {} {} {} {}",
                    request.method,
                    request.url,
                    auth.unwrap_or_default(),
                    body.unwrap_or_default()
                ));
                *next.borrow_mut() += 1;
                Ok(stream(&[Record::ServiceNetStarted(ServiceNetStarted {
                    id: *next.borrow(),
                })]))
            }
            (2, _) => Ok(Vec::new()),
            (3, _) => Ok(stream(&[Record::ServiceNetAnswer(ServiceNetAnswer {
                status: 200,
                flags: 0,
                body_len: u32::try_from(ANSWER.len()).unwrap(),
                etag: "",
                content_type: "",
                final_url: "",
            })])),
            (4, Some(Record::ServiceNetRead(read))) => {
                let from = (read.offset as usize).min(ANSWER.len());
                Ok(stream(&[Record::ServiceNetBytes(ServiceNetBytes {
                    data: &ANSWER.as_bytes()[from..],
                })]))
            }
            _ => Err(-1),
        }
    });
}

fn event(host: &paperpaw_scripted_host::Plugin<'_>, token: u32, record: Record<'_>) -> Callback {
    host.event(token, &event_batch(&record).unwrap()).unwrap()
}

fn opened(p: &paperpaw_scripted_host::Plugin<'_>) -> Callback {
    event(
        p,
        PANEL_TOKEN,
        Record::EventScreenOpened(EventScreenOpened {
            now_ms: 0,
            screen: "translate.panel",
            instance: INSTANCE,
            slot: "",
            flags: 0,
            handover: HANDOVER,
            params: 0,
            text_bytes: 5,
        }),
    )
}

fn shown(p: &paperpaw_scripted_host::Plugin<'_>) -> Callback {
    event(
        p,
        PANEL_TOKEN,
        Record::EventScreenShown(EventScreenShown {
            now_ms: 0,
            screen: "translate.panel",
            width: 480,
            height: 800,
            instance: Some(INSTANCE),
        }),
    )
}

fn done(p: &paperpaw_scripted_host::Plugin<'_>, id: u32, status: u32, reason: u32) -> Callback {
    event(
        p,
        HTTP_TOKEN,
        Record::EventHttpDone(EventHttpDone {
            now_ms: 0,
            id,
            token: 1,
            status,
            reason,
            flags: 0,
            body_len: if status == 200 {
                u32::try_from(ANSWER.len()).unwrap()
            } else {
                0
            },
        }),
    )
}

/// The input status: 1 taken, 0 not taken. (The scripted host stages no
/// screen commands, so what a key asks of the host — a redraw, closing, going
/// online — is not seen here; the panel's state tests cover what each key means.)
fn action(p: &paperpaw_scripted_host::Plugin<'_>, name: &str) -> i32 {
    p.input(
        PANEL_TOKEN,
        &event_batch(&Record::InputAction(InputAction {
            now_ms: 0,
            action: name,
        }))
        .unwrap(),
    )
    .unwrap()
    .status
}

fn requests(world: &World) -> Vec<String> {
    world
        .log
        .borrow()
        .iter()
        .filter(|l| l.starts_with("request"))
        .cloned()
        .collect()
}

#[test]
fn the_selected_text_is_sent_when_the_panel_is_first_shown_and_the_translation_redraws() {
    let host = ScriptedHost::new();
    let world = world(&host, "Hello world", Some("abc:fx"), false);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(opened(&p).status, 0);
    assert!(
        requests(&world).is_empty(),
        "nothing is sent before it is shown"
    );
    let first = shown(&p);
    assert_eq!(first.status, 0);
    // The language of the reader is not there (no catalog): PaperPaw's own,
    // English, is DeepL's EN-GB.
    assert_eq!(
        requests(&world),
        [
            "request POST https://api-free.deepl.com/v2/translate DeepL-Auth-Key abc:fx \
          text=Hello+world&target_lang=EN-GB"
        ]
    );
    // Shown again (after a menu closed): not sent again.
    shown(&p);
    assert_eq!(requests(&world).len(), 1);
    let end = done(&p, 1, 200, 0);
    assert_eq!(end.status, 0);
    // Select closes it.
    assert_eq!(action(&p, "core.confirm"), 1);
    // Closing it cancels what is in flight and frees the panel.
    event(
        &p,
        PANEL_TOKEN,
        Record::EventScreenClosed(EventScreenClosed {
            now_ms: 0,
            screen: "translate.panel",
            instance: INSTANCE,
        }),
    );
    assert_eq!(action(&p, "core.confirm"), 0);
}

#[test]
fn without_a_key_nothing_is_sent_and_select_asks_for_it_masked() {
    let host = ScriptedHost::new();
    let world = world(&host, "Hello", None, false);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    opened(&p);
    let first = shown(&p);
    assert!(requests(&world).is_empty());
    assert_eq!(first.status, 0);
    assert_eq!(action(&p, "core.confirm"), 1);
    // A text question (kind 0) with the secret flag (bit 2).
    assert!(
        world
            .log
            .borrow()
            .contains(&"ask kind 0 flags 4".to_string()),
        "{:?}",
        world.log.borrow()
    );
    // It is typed, with spaces round it: kept, and the text goes off with it.
    *world.typed.borrow_mut() = "  new:fx \n".to_string();
    let answered = event(
        &p,
        PANEL_TOKEN,
        Record::EventAnswered(EventAnswered {
            now_ms: 0,
            request: KEY_REQUEST,
            kind: 0,
            outcome: 0,
            choice: 0,
            length: 10,
            flags: 0,
        }),
    );
    assert_eq!(answered.status, 0);
    assert_eq!(world.key.borrow().as_deref(), Some("new:fx"));
    assert_eq!(
        requests(&world),
        [
            "request POST https://api-free.deepl.com/v2/translate DeepL-Auth-Key new:fx \
          text=Hello&target_lang=EN-GB"
        ]
    );
    // An answer to another question, or a cancelled one, changes nothing.
    for (request, outcome) in [(KEY_REQUEST + 1, 0), (KEY_REQUEST, 1)] {
        let before = world.log.borrow().len();
        event(
            &p,
            PANEL_TOKEN,
            Record::EventAnswered(EventAnswered {
                now_ms: 0,
                request,
                kind: 0,
                outcome,
                choice: 0,
                length: 0,
                flags: 0,
            }),
        );
        assert_eq!(world.log.borrow().len(), before);
    }
}

#[test]
fn a_host_that_cannot_keep_the_key_still_uses_it_this_time() {
    let host = ScriptedHost::new();
    let world = world(&host, "Hello", None, true);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    opened(&p);
    shown(&p);
    action(&p, "core.confirm");
    *world.typed.borrow_mut() = "abc".to_string();
    let answered = event(
        &p,
        PANEL_TOKEN,
        Record::EventAnswered(EventAnswered {
            now_ms: 0,
            request: KEY_REQUEST,
            kind: 0,
            outcome: 0,
            choice: 0,
            length: 3,
            flags: 0,
        }),
    );
    assert_eq!(answered.status, 0);
    assert!(world.log.borrow().contains(&"keep".to_string()));
    assert!(world.key.borrow().is_none(), "nothing was kept");
    assert_eq!(requests(&world).len(), 1, "used all the same");
}

#[test]
fn offline_mode_offers_to_go_online_once_and_go_online_sends_again() {
    let host = ScriptedHost::new();
    let world = world(&host, "Hello", Some("abc"), false);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    opened(&p);
    shown(&p);
    // Held by the offline power mode (reason 11): the offer, not a message.
    assert_eq!(done(&p, 1, 0, 11).status, 0);
    // Go online is the focused button: the text is sent again.
    assert_eq!(action(&p, "core.confirm"), 1);
    assert_eq!(requests(&world).len(), 2);
    // Held again: not offered again, it says it needs a network; Select closes.
    assert_eq!(done(&p, 2, 0, 11).status, 0);
    assert_eq!(action(&p, "core.confirm"), 1);
    assert_eq!(requests(&world).len(), 2);
}

#[test]
fn not_now_says_it_needs_a_network_and_sends_nothing() {
    let host = ScriptedHost::new();
    let world = world(&host, "Hello", Some("abc"), false);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    opened(&p);
    shown(&p);
    done(&p, 1, 0, 11);
    assert_eq!(action(&p, "core.down"), 1, "the focus moves to Not now");
    assert_eq!(action(&p, "core.confirm"), 1);
    assert_eq!(requests(&world).len(), 1, "nothing is sent again");
}

#[test]
fn a_panel_without_a_network_service_says_so_and_never_aborts() {
    let host = ScriptedHost::new();
    let world = world(&host, "Hello", Some("abc"), false);
    // The device has no network at all: the service does not open (the third
    // one the panel opens: the screens', the secrets', then the network's).
    host.refuse_nth(
        Slot::ServiceOpen,
        2,
        paperpaw_native::host_glue::ERR_UNSUPPORTED,
    );
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    opened(&p);
    let first = shown(&p);
    assert_eq!(first.status, 0);
    assert!(requests(&world).is_empty());
}

#[test]
fn no_refusal_makes_the_panel_abort() {
    for slot in [Slot::ServiceCall, Slot::ServiceOpen] {
        let results = sweep(slot, -5, |host, _| {
            let _world = world(host, "Hello", Some("abc"), false);
            let Ok(p) = host.load(NATIVE_ENTRY.entry()) else {
                return -5;
            };
            let mut worst = 0;
            for status in [
                opened(&p).status,
                shown(&p).status,
                done(&p, 1, 200, 0).status,
                action(&p, "core.confirm"),
            ] {
                worst = worst.min(status);
            }
            worst
        });
        assert_eq!(results[0].1, 0);
        assert!(results.len() > 3, "{}", results.len());
        assert!(results.iter().skip(1).all(|(_, status)| *status <= 0));
    }
}
