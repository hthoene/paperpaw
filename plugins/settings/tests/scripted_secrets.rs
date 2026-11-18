//! Passwords and keys over a scripted `core.secrets`, with the plugin made and
//! driven by the scripted host: what the list asks the service (all plugins'
//! secrets, page after page), that the plugin is not harmed by a service it
//! may not use, and that no refusal of the host aborts it.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventAnswered, EventScreenOpened, EventScreenShown, InputAction, Record, ServiceEnd,
    ServiceSecEntry, ServiceUiParam, ServiceUiRequest, ServiceUiText, record_encode_one,
};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_UNSUPPORTED};
use paperpaw_plugin_settings::NATIVE_ENTRY;
use paperpaw_scripted_host::{Callback, ScriptedHost, Slot, event_batch, sweep};
use std::cell::RefCell;
use std::rc::Rc;

const SECRETS_LIST: u32 = 2;
const SECRETS_KEEP: u32 = 3;
const SECRETS_REMOVE: u32 = 4;
const SECRETS_TOKEN: u32 = 15;
const PAGE: usize = 2;
const SECRET_TOKEN: u32 = 16;
const HANDOVER: u32 = 7;
const ASKED: u32 = 3;
const INSTANCE: u32 = 5;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// (owner, name, label, changeable)
fn kept() -> Vec<(&'static str, &'static str, &'static str, bool)> {
    vec![
        ("mail", "password", "mail.secret.password", true),
        ("kosync", "key", "Sync key", false),
        ("wifi", "Home", "Wi-Fi Home", true),
        ("lock", "code", "", false),
        ("wifi", "Cafe", "Wi-Fi Cafe", true),
    ]
}

/// What the service was asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

fn script(host: &ScriptedHost, log: &Log, answer: Option<i32>) {
    // Opened with the registry reader for every screen; this one reads none of it.
    let _ = host.registry();
    let seen = log.clone();
    host.service("core.secrets", 1, 0, move |op, input| {
        let batch = paperpaw_core::native_host::registrations::records_decode(input).unwrap();
        let rows: Vec<Record<'_>> = batch.iter().collect();
        if let Some(status) = answer {
            return Err(status);
        }
        match (op, rows.first()) {
            (SECRETS_LIST, Some(Record::ServiceSecList(query))) => {
                seen.borrow_mut()
                    .push(format!("list {} {}", query.flags, query.cursor));
                let all = kept();
                let from = query.cursor as usize;
                let to = (from + PAGE).min(all.len());
                let mut out: Vec<Record<'_>> = all[from..to]
                    .iter()
                    .map(|(plugin, name, label, changeable)| {
                        Record::ServiceSecEntry(ServiceSecEntry {
                            flags: u32::from(*changeable),
                            plugin,
                            name,
                            label,
                        })
                    })
                    .collect();
                let last = to == all.len();
                out.push(Record::ServiceEnd(ServiceEnd {
                    flags: u32::from(last),
                    total: u32::try_from(all.len()).unwrap(),
                    generation: 0,
                    next: u32::try_from(to).unwrap(),
                    cursor: None,
                }));
                Ok(stream(&out))
            }
            (SECRETS_KEEP, Some(Record::ServiceSecKeep(keep))) => {
                seen.borrow_mut()
                    .push(format!("keep {} {} {}", keep.plugin, keep.name, keep.value));
                Ok(Vec::new())
            }
            (SECRETS_REMOVE, Some(Record::ServiceSecName(name))) => {
                seen.borrow_mut()
                    .push(format!("remove {} {}", name.plugin, name.name));
                Ok(Vec::new())
            }
            _ => Err(-1),
        }
    });
}

fn opened() -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "settings.secrets",
        instance: 1,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

/// The scripted host does not answer `i18n.copy` and `i18n.format`, nor the
/// registry the list of the settings (`ERR_UNSUPPORTED`): the rows end there,
/// after the secrets have been read.
fn finished(status: i32) -> bool {
    status == 0 || status == ERR_UNSUPPORTED
}

fn run(host: &ScriptedHost) -> i32 {
    host.service("core.ui", 1, 0, |_, _| Ok(Vec::new()));
    // A refusal while the plugin is made is the factory's status.
    let plugin = match host.load(NATIVE_ENTRY.entry()) {
        Ok(plugin) => plugin,
        Err(status) => return status,
    };
    plugin
        .event(SECRETS_TOKEN, &opened())
        .map_or_else(|status| status, |done| done.status)
}

#[test]
fn the_list_asks_for_every_plugins_secrets_a_page_at_a_time() {
    let host = ScriptedHost::new();
    let log: Log = Log::default();
    script(&host, &log, None);
    let status = run(&host);
    assert!(finished(status), "{status} {:?}", log.borrow());
    // Flag 1 is every plugin's; the cursor goes on from the rows read.
    assert_eq!(*log.borrow(), ["list 1 0", "list 1 2", "list 1 4"]);
}

#[test]
fn a_service_that_will_not_list_leaves_the_list_empty_not_failed() {
    for status in [ERR_DENIED, ERR_UNSUPPORTED] {
        let host = ScriptedHost::new();
        script(&host, &Log::default(), Some(status));
        let result = run(&host);
        assert!(finished(result), "{status}: {result}");
    }
}

#[test]
fn no_refusal_of_the_host_makes_it_abort() {
    for slot in [Slot::ServiceCall, Slot::ServiceOpen] {
        let results = sweep(slot, -5, |host, _| {
            script(host, &Log::default(), None);
            run(host)
        });
        assert!(finished(results[0].1));
        assert!(results.len() >= 2, "{}", results.len());
        // A refused call is an error status (nothing staged), never a hang.
        assert!(results.iter().skip(1).all(|(_, status)| *status <= 0));
    }
}

/// What was typed (the text of the asked request).
type Typed = Rc<RefCell<String>>;

/// `core.ui` for the one-secret screen: the hand-over's `id`, the questions
/// asked (as `ask <kind> <title> | <choices>`) and the text typed.
fn ui(host: &ScriptedHost, log: &Log, typed: &Typed, which: &'static str) {
    let (seen, typed) = (log.clone(), typed.clone());
    host.service("core.ui", 1, 0, move |op, input| {
        let batch = paperpaw_core::native_host::registrations::records_decode(input).unwrap();
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (17, _) => Ok(stream(&[
                Record::ServiceUiParam(ServiceUiParam {
                    key: "id",
                    value: which,
                }),
                // A page that is not paged: its rows counted, nothing more.
                Record::ServiceEnd(ServiceEnd {
                    flags: 0,
                    total: 1,
                    generation: 0,
                    next: 0,
                    cursor: None,
                }),
            ])),
            (8, Some(Record::ServiceUiAsk(ask))) => {
                seen.borrow_mut()
                    .push(format!("ask {} {} | {}", ask.kind, ask.title, ask.choices));
                seen.borrow_mut().push(format!("flags {}", ask.flags));
                Ok(stream(&[Record::ServiceUiRequest(ServiceUiRequest {
                    request: ASKED,
                })]))
            }
            // The text a question starts from: a secret is asked for anew,
            // never from the value it has.
            (9, _) => {
                seen.borrow_mut().push("write".to_owned());
                Ok(Vec::new())
            }
            (10, Some(Record::ServiceUiTextAt(at))) => {
                let text = typed.borrow().clone();
                Ok(stream(&[Record::ServiceUiText(ServiceUiText {
                    text: at.text,
                    offset: at.offset,
                    total: u32::try_from(text.len()).unwrap(),
                    flags: 0,
                    data: &text,
                })]))
            }
            _ => Err(-1),
        }
    });
}

type Screen<'h> = paperpaw_scripted_host::Plugin<'h>;

fn secret_screen(host: &ScriptedHost) -> Screen<'_> {
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "settings.secret",
        instance: INSTANCE,
        slot: "",
        flags: 0,
        handover: HANDOVER,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(SECRET_TOKEN, &opened).unwrap().status, 0);
    let shown = event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen: "settings.secret",
        width: 480,
        height: 800,
        instance: Some(INSTANCE),
        last_input: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(SECRET_TOKEN, &shown).unwrap().status, 0);
    plugin
}

/// The first row is chosen (moving the focus asks the host to redraw, which
/// the scripted host does not stage: BLOCKED H1). Returns the input's status.
fn choose_first(plugin: &Screen<'_>) -> i32 {
    let batch = event_batch(&Record::InputAction(InputAction {
        now_ms: 0,
        action: "core.confirm",
    }))
    .unwrap();
    plugin.input(SECRET_TOKEN, &batch).unwrap().status
}

/// The dialog's answer. Done, the screen goes back (a screen command the
/// scripted host does not stage: BLOCKED H1), so the status is that refusal
/// or 0; what was asked of `core.secrets` is the test's own subject.
fn answered(plugin: &Screen<'_>, kind: u32, choice: u32, length: u32) -> Callback {
    let batch = event_batch(&Record::EventAnswered(EventAnswered {
        now_ms: 0,
        request: ASKED,
        kind,
        outcome: 0,
        choice,
        length,
        flags: 0,
    }))
    .unwrap();
    let done = plugin.event(SECRET_TOKEN, &batch).unwrap();
    assert!(finished(done.status), "{}", done.status);
    done
}

fn world(typed: &str, which: &'static str) -> (ScriptedHost, Log) {
    let host = ScriptedHost::new();
    let log = Log::default();
    let typed = Typed::new(RefCell::new(typed.to_string()));
    script(&host, &log, None);
    ui(&host, &log, &typed, which);
    (host, log)
}

fn asked(log: &Log) -> Vec<String> {
    log.borrow()
        .iter()
        .filter(|l| l.starts_with("ask"))
        .cloned()
        .collect()
}

fn calls(log: &Log, verb: &str) -> Vec<String> {
    log.borrow()
        .iter()
        .filter(|l| l.starts_with(verb))
        .cloned()
        .collect()
}

#[test]
fn removing_a_secret_asks_first_and_removes_only_on_yes() {
    // Not changeable: remove is its only row.
    for (choice, removed) in [(0, false), (1, true)] {
        let (host, log) = world("", "kosync:key");
        let plugin = secret_screen(&host);
        assert_eq!(choose_first(&plugin), 1);
        assert_eq!(asked(&log), ["ask 2 Remove “Sync key”? | Remove\nKeep"]);
        assert!(calls(&log, "remove").is_empty());
        answered(&plugin, 2, choice, 0);
        let expected: &[&str] = if removed { &["remove kosync key"] } else { &[] };
        assert_eq!(calls(&log, "remove"), expected);
    }
}

#[test]
fn a_secret_that_is_not_kept_any_more_is_removed_without_asking() {
    let (host, log) = world("", "mail:gone");
    let plugin = secret_screen(&host);
    // The screen then goes back: a screen command the scripted host refuses
    // (BLOCKED H1), so the status is that refusal or 1.
    let status = choose_first(&plugin);
    assert!(status == 1 || status == ERR_UNSUPPORTED, "{status}");
    assert!(asked(&log).is_empty());
    assert_eq!(calls(&log, "remove"), ["remove mail gone"]);
}

#[test]
fn typing_a_secret_anew_keeps_what_was_typed_and_nothing_typed_leaves_it() {
    for (typed, kept) in [("  hunter2 ", Some("keep wifi Home hunter2")), ("  ", None)] {
        let (host, log) = world(typed, "wifi:Home");
        let plugin = secret_screen(&host);
        assert_eq!(choose_first(&plugin), 1);
        // A text question (kind 0) titled with the secret's label.
        let questions = asked(&log);
        assert_eq!(questions.len(), 1, "{questions:?}");
        assert!(
            questions[0].starts_with("ask 0 Wi-Fi Home"),
            "{questions:?}"
        );
        assert!(calls(&log, "keep").is_empty());
        // Masked (flag 4), and started from nothing: the old value is never
        // written to the dialog.
        assert_eq!(calls(&log, "flags"), ["flags 4"]);
        assert!(calls(&log, "write").is_empty());
        let done = answered(&plugin, 0, 0, u32::try_from(typed.len()).unwrap());
        // The secret goes to `core.secrets` alone, never into a command.
        assert!(!done.batches_contain(b"hunter2"));
        let did = calls(&log, "keep");
        assert_eq!(did.first().map(String::as_str), kept, "{did:?}");
        assert_eq!(did.len(), usize::from(kept.is_some()));
    }
}
