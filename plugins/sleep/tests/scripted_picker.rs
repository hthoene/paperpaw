//! The picker of the picture and the widget over the page over a scripted
//! `core.files` and `core.registry`: the focus starts on the row that is chosen
//! now, a choice stages the settings of the row (and the preset), and a refused
//! call is an error or the rows without it, never an abort or a leak. Drawing
//! and the screen command that closes the screen are not staged by the scripted
//! host (BLOCKED H1); what the plugin submitted is what is looked at.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::{SettingValueRef, settings_encode};
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, EventScreenShown, InputAction, Record, ServiceEnd, ServiceFsEntry,
    ServiceRegItem, ServiceRegValue, ServiceRegWidget, record_encode_one,
};
use paperpaw_native::host_glue::ERR_IO;
use paperpaw_plugin_sleep::NATIVE_ENTRY;
use paperpaw_scripted_host::{Plugin, ScriptedHost, Slot, event_batch, row, sweep};
use paperpaw_sdk::{SettingSpec, builtin::groups};
use std::mem::MaybeUninit;

#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the tests read the token only")]
mod declared;

const OP_PRESETS: u32 = 16;
const OP_VALUES: u32 = 18;
const OP_WIDGETS: u32 = 19;
const PICTURE: &str = "sleep.overlay-picture";
const WIDGET: &str = "sleep.overlay-widget";

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// The pictures folder `sleep` of the card: files in no order, and what is no
/// picture. Two entries a page.
const ENTRIES: [(&str, bool); 7] = [
    ("z.GIF", false),
    ("b.png", false),
    ("notes.txt", false),
    ("sub", true),
    ("a.jpg", false),
    (".hidden.png", false),
    ("C.JPG", false),
];

fn script_files(host: &ScriptedHost) {
    host.service("core.files", 1, 0, |op, input| {
        let batch =
            paperpaw_core::native_host::registrations::records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        let (1, Some(Record::ServiceFsList(list))) = (op, rows.first()) else {
            return Err(-1);
        };
        if list.path != "sleep" {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        let from = usize::try_from(list.cursor).unwrap();
        let mut out: Vec<Record<'_>> = ENTRIES
            .iter()
            .skip(from)
            .take(2)
            .map(|(name, folder)| {
                Record::ServiceFsEntry(ServiceFsEntry {
                    name,
                    flags: u32::from(*folder),
                    size: 0,
                    modified: None,
                })
            })
            .collect();
        let more = from + 2 < ENTRIES.len();
        out.push(Record::ServiceEnd(ServiceEnd {
            flags: u32::from(!more),
            total: 0,
            generation: 0,
            next: 0,
            cursor: more.then(|| (from + 2) as u64),
        }));
        Ok(stream(&out))
    });
}

fn text_value(text: &str) -> Vec<u8> {
    let mut bytes = [MaybeUninit::<u8>::uninit(); 256];
    let used = settings_encode(SettingValueRef::Text(text), &mut bytes).unwrap();
    // SAFETY: `settings_encode` initialized the first `used` bytes.
    bytes[..used]
        .iter()
        .map(|b| unsafe { b.assume_init() })
        .collect()
}

/// The registry: two widgets and a preset that picks `owls/owl.png`.
fn script_registry(host: &ScriptedHost) {
    let registry = host.registry();
    let widget = |id, title| {
        row(&Record::ServiceRegWidget(ServiceRegWidget {
            id,
            plugin: "home",
            kind: "time",
            icon: "",
            flags: 0,
            title,
            cells: 0,
            previews: "",
        }))
        .unwrap()
    };
    registry.set_rows(
        OP_WIDGETS,
        vec![widget("home.clock", "Clock"), widget("home.date", "Date")],
    );
    registry.set_rows(
        OP_PRESETS,
        vec![
            row(&Record::ServiceRegItem(ServiceRegItem {
                id: "owls.night",
                plugin: "owls",
                flags: 0,
                title: "Night owl",
            }))
            .unwrap(),
        ],
    );
    registry.set_rows(
        OP_VALUES,
        vec![
            row(&Record::ServiceRegValue(ServiceRegValue {
                key: PICTURE,
                value: &text_value("owls/owl.png"),
                flags: 0,
            }))
            .unwrap(),
        ],
    );
}

fn setup(widget: &str, picture: &str) -> ScriptedHost {
    let mut host = ScriptedHost::for_plugin("sleep");
    configure(&mut host, widget, picture);
    host
}

fn configure(host: &mut ScriptedHost, widget: &str, picture: &str) {
    // The settings the picker sets, so that the host admits the commands.
    for key in [WIDGET, PICTURE] {
        host.settings_mut()
            .define(SettingSpec::text(key, key, "", groups::OTHER))
            .unwrap();
    }
    script_files(host);
    script_registry(host);
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    host.set_setting("sleep.folder", SettingValueRef::Text("/sleep/"));
    host.set_setting(WIDGET, SettingValueRef::Text(widget));
    host.set_setting(PICTURE, SettingValueRef::Text(picture));
}

fn open(plugin: &Plugin<'_>) -> i32 {
    let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "sleep.pick-overlay",
        instance: 1,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    let status = plugin.event(declared::PICK, &opened).unwrap().status;
    let shown = event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen: "sleep.pick-overlay",
        width: 480,
        height: 800,
        instance: Some(1),
        last_input: 0,
    }))
    .unwrap();
    // The redraw a show asks for is not asked: the picker draws only when asked.
    plugin.event(declared::PICK, &shown).unwrap();
    status
}

fn action(name: &str) -> Vec<u8> {
    event_batch(&Record::InputAction(InputAction {
        now_ms: 0,
        action: name,
    }))
    .unwrap()
}

/// The row the focus starts on is chosen (this host refuses the redraw a move
/// of the focus asks for, so the focus cannot be moved): what it submitted.
fn choose(host: &ScriptedHost) -> paperpaw_scripted_host::Callback {
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(open(&plugin), 0);
    // Choosing sets the settings and then closes the screen, which the scripted
    // host does not stage (the callback's status says so, and drops what it
    // staged): the batches it submitted are what shows.
    plugin
        .input(declared::PICK, &action("core.confirm"))
        .unwrap()
}

#[test]
fn the_folder_row_clears_the_picture_and_the_widget() {
    let host = setup("", "");
    // The focus starts on the folder row: nothing is set, the folder's turn.
    let done = choose(&host);
    assert!(done.batches_contain(PICTURE.as_bytes()), "{done:?}");
    assert!(done.batches_contain(WIDGET.as_bytes()), "{done:?}");
    assert!(!done.batches_contain(b"sleep/"), "{done:?}");
}

#[test]
fn the_pictures_of_the_folder_are_rows_and_a_choice_clears_the_widget() {
    // A picture of the folder, whatever its case, is a row, and the focus
    // starts on it; the widget is set to nothing with it.
    for name in ["C.JPG", "a.jpg", "b.png", "z.GIF"] {
        let host = setup("", &format!("sleep/{name}"));
        let done = choose(&host);
        assert!(
            done.batches_contain(format!("sleep/{name}").as_bytes()),
            "{name}: {done:?}"
        );
        assert!(done.batches_contain(WIDGET.as_bytes()), "{name}: {done:?}");
    }
    // What is no picture (a text file, a folder, a hidden file, a file that is
    // not there) is no row: the focus stays on the folder row, which sets
    // nothing named for a picture.
    for name in ["notes.txt", "sub", ".hidden.png", "gone.png"] {
        let host = setup("", &format!("sleep/{name}"));
        let done = choose(&host);
        assert!(!done.batches_contain(name.as_bytes()), "{name}: {done:?}");
    }
}

#[test]
fn the_focus_starts_on_the_picture_that_is_set() {
    // Written with slashes round it or not: the same row.
    for picture in ["sleep/b.png", "/sleep/b.png"] {
        let host = setup("", picture);
        let done = choose(&host);
        assert!(done.batches_contain(b"sleep/b.png"), "{done:?}");
    }
}

#[test]
fn a_widget_is_chosen_and_leaves_the_picture_as_it_is() {
    // The widget set is where the focus starts, and wins over the picture.
    let host = setup("home.date", "sleep/a.jpg");
    let done = choose(&host);
    assert!(done.batches_contain(b"home.date"), "{done:?}");
    assert!(!done.batches_contain(PICTURE.as_bytes()), "{done:?}");
    // A widget no plugin has any more marks no row: the folder's.
    let host = setup("gone.widget", "");
    let done = choose(&host);
    assert!(!done.batches_contain(b"gone.widget"), "{done:?}");
}

#[test]
fn a_preset_is_applied_where_every_value_of_it_is_what_the_setting_holds() {
    let host = setup("", "owls/owl.png");
    // The focus starts on the preset: its values are what the settings hold.
    let done = choose(&host);
    assert!(done.batches_contain(b"owls.night"), "{done:?}");
    assert!(done.batches_contain(WIDGET.as_bytes()), "{done:?}");
}

#[test]
fn no_refused_call_makes_the_picker_abort() {
    for slot in [Slot::ServiceCall, Slot::ServiceOpen] {
        let results = sweep(slot, -ERR_IO.abs(), |host, _| {
            configure(host, "", "");
            let Ok(plugin) = host.load(NATIVE_ENTRY.entry()) else {
                return 0;
            };
            let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
                now_ms: 0,
                screen: "sleep.pick-overlay",
                instance: 1,
                slot: "",
                flags: 0,
                handover: 0,
                params: 0,
                text_bytes: 0,
            }))
            .unwrap();
            plugin
                .event(declared::PICK, &opened)
                .map_or_else(|status| status, |done| done.status)
        });
        assert_eq!(results[0].1, 0);
        assert!(
            results.iter().all(|(_, status)| *status <= 0),
            "{results:?}"
        );
    }
}

/// The picker gives the card back (`Files::release`) whatever it read: what the
/// plugin holds after it opened is the registry it keeps for its lifetime.
#[test]
fn the_card_is_given_back_after_the_folder_is_read() {
    let host = setup("", "");
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(open(&plugin), 0);
    assert_eq!(host.open_handles(), 1);
}
