//! The picker of the setting `home.status.line` over a scripted `core.registry`
//! and `core.ui`: home's screen `status-items` in its lines mode, as interface
//! 6 had the core screen `core.status-lines`. It lists the status lines the
//! registry offers (not the items), a choice sets the setting at once, a line
//! named by an id it had before is that line, *Back* is left to the host (which
//! closes the screen), and a closed screen keeps nothing.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::SettingValueRef;
use paperpaw_core::native_host::registrations::{
    EventScreenClosed, EventScreenOpened, EventScreenShown, InputAction, RawRecord, Record,
    ServiceEnd, ServiceRegQuery, ServiceRegStatus, ServiceUiParam, record_encode_one,
};
use paperpaw_plugin_home::NATIVE_ENTRY;
use paperpaw_scripted_host::{Plugin, ScriptedHost, event_batch, row};
use std::cell::RefCell;
use std::rc::Rc;

/// The token of the screen `status-items` (`declarations::STATUS_ITEMS`).
const ITEMS_TOKEN: u32 = 13;
const SCREEN: &str = "home.status-items";
const LINE: &str = "home.status.line";
/// `core.registry` op `status` (REG op 21).
const OP_STATUS: u32 = 21;
/// `core.ui` op `params`.
const OP_PARAMS: u32 = 17;
const HANDOVER: u32 = 7;

/// The lines the registry offers: id, title.
const LINES: [(&str, &str); 3] = [
    ("core.status-line.items", "Text and symbols"),
    ("reader.status-line.progress", "Progress"),
    ("reader.status-line.empty", "Nothing"),
];
/// The items it offers.
const ITEMS: [(&str, &str); 2] = [("core.status.clock", "Time"), ("wifi.status", "Wi-Fi")];

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// One page: the rows of `scope` (`line` or `item`), then the end.
fn status_page(scope: &str) -> Vec<u8> {
    let (kind, flags, rows): (u32, u32, &[(&str, &str)]) = if scope == "line" {
        (1, 1, &LINES)
    } else {
        (0, 0, &ITEMS)
    };
    let mut out = Vec::new();
    for (id, title) in rows {
        out.extend(
            row(&Record::ServiceRegStatus(ServiceRegStatus {
                id,
                plugin: id.split('.').next().unwrap(),
                kind,
                flags,
                priority: 0,
                size: 1000,
                tap: "",
                title,
            }))
            .unwrap(),
        );
    }
    let total = u32::try_from(rows.len()).unwrap();
    out.extend(
        row(&Record::ServiceEnd(ServiceEnd {
            flags: 1,
            total,
            generation: 1,
            next: total,
            cursor: None,
        }))
        .unwrap(),
    );
    out
}

/// The host with the setting `setting` holding `value`, opened on `setting`;
/// the scopes the registry was asked for are logged.
fn setup(setting: &'static str, value: &str) -> (ScriptedHost, Rc<RefCell<Vec<String>>>) {
    let host = ScriptedHost::new();
    host.service("core.ui", 1, 0, move |op, _| match op {
        OP_PARAMS => Ok(stream(&[
            Record::ServiceUiParam(ServiceUiParam {
                key: "id",
                value: setting,
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
    });
    let scopes = Rc::new(RefCell::new(Vec::new()));
    let log = scopes.clone();
    host.registry().on(OP_STATUS, move |input| {
        let query = RawRecord::single(input)?.decode::<ServiceRegQuery<'_>>()?;
        log.borrow_mut().push(query.scope.to_string());
        Ok(status_page(query.scope))
    });
    for folder in ["/../../crates/paperpaw-core/i18n", "/i18n"] {
        host.add_catalogs_in(format!("{}{folder}", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    }
    host.set_setting(setting, SettingValueRef::Text(value));
    (host, scopes)
}

fn open(host: &ScriptedHost) -> (Plugin<'_>, i32) {
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: SCREEN,
        instance: 1,
        slot: "",
        flags: 0,
        handover: HANDOVER,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    let status = plugin.event(ITEMS_TOKEN, &opened).unwrap().status;
    let shown = event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen: SCREEN,
        width: 480,
        height: 800,
        instance: Some(1),
        last_input: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(ITEMS_TOKEN, &shown).unwrap().status, 0);
    (plugin, status)
}

fn press(plugin: &Plugin<'_>, action: &str) -> paperpaw_scripted_host::Callback {
    let input = event_batch(&Record::InputAction(InputAction { now_ms: 0, action })).unwrap();
    plugin.input(ITEMS_TOKEN, &input).unwrap()
}

/// What the plugin set `setting` to in `done`, if it set it.
fn set_to(done: &paperpaw_scripted_host::Callback, setting: &str, value: &str) -> bool {
    done.batches_contain(setting.as_bytes()) && done.batches_contain(value.as_bytes())
}

#[test]
fn the_picker_lists_the_status_lines_the_registry_offers() {
    let (host, scopes) = setup(LINE, "");
    let (_plugin, status) = open(&host);
    assert_eq!(status, 0);
    // The lines, not the items: every page asked for is of the scope `line`.
    let scopes = scopes.borrow();
    assert!(!scopes.is_empty());
    assert!(scopes.iter().all(|scope| scope == "line"), "{scopes:?}");
}

#[test]
fn choosing_a_line_sets_the_setting_at_once_and_back_leaves() {
    let (host, _) = setup(LINE, "");
    let (plugin, _) = open(&host);
    // The second line: down, then confirm.
    press(&plugin, "core.down");
    let done = press(&plugin, "core.confirm");
    assert!(
        set_to(&done, LINE, "reader.status-line.progress"),
        "{done:?}"
    );
    assert!(!done.batches_contain(b"core.status-line.items"));
    // *Back* is not the picker's: the host goes back (the screen closes) and
    // nothing is set.
    let back = press(&plugin, "core.back");
    assert_eq!(back.status, 0);
    assert!(!back.batches_contain(LINE.as_bytes()));
}

#[test]
fn a_line_named_by_an_id_it_had_before_is_that_line() {
    // The kernel's line of items was the reader's `reader.status-line.items`:
    // it is shown first (as chosen), and choosing another one writes its id
    // now, not the old one beside it.
    let (host, _) = setup(LINE, "reader.status-line.items");
    let (plugin, _) = open(&host);
    press(&plugin, "core.down");
    let done = press(&plugin, "core.confirm");
    assert!(
        set_to(
            &done,
            LINE,
            "core.status-line.items, reader.status-line.progress"
        ),
        "{done:?}"
    );
    assert!(!done.batches_contain(b"reader.status-line.items"));
}

#[test]
fn a_closed_picker_keeps_nothing() {
    let (host, _) = setup(LINE, "");
    let (plugin, _) = open(&host);
    let closed = event_batch(&Record::EventScreenClosed(EventScreenClosed {
        now_ms: 0,
        screen: SCREEN,
        instance: 1,
    }))
    .unwrap();
    assert_eq!(plugin.event(ITEMS_TOKEN, &closed).unwrap().status, 0);
    // No editor is left to take the press.
    let done = press(&plugin, "core.confirm");
    assert_eq!(done.status, 0);
    assert!(!done.batches_contain(LINE.as_bytes()));
}

#[test]
fn the_status_lines_parts_list_items() {
    let (host, scopes) = setup("home.status.left", "core.status.clock");
    let (plugin, status) = open(&host);
    assert_eq!(status, 0);
    assert!(scopes.borrow().iter().all(|scope| scope == "item"));
    // Wi-Fi goes after the time.
    press(&plugin, "core.down");
    let done = press(&plugin, "core.confirm");
    assert!(
        set_to(&done, "home.status.left", "core.status.clock, wifi.status"),
        "{done:?}"
    );
}
