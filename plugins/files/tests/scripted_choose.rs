//! The folder chooser of a setting over a scripted `core.files` and `core.ui`:
//! it starts on the folder the setting names (the top where that is gone),
//! reads folders a page at a time, uses the shown folder for the setting, and
//! back goes up a folder before it leaves.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::SettingValueRef;
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, EventScreenShown, InputAction, Record, ServiceEnd, ServiceFsEntry,
    ServiceUiHandle, ServiceUiParam, record_encode_one, records_decode,
};
use paperpaw_native::host_glue::{ERR_IO, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_plugin_files::NATIVE_ENTRY;
use paperpaw_scripted_host::{Plugin, ScriptedHost, Slot, event_batch, sweep};
use std::cell::RefCell;
use std::rc::Rc;

const CHOOSE_TOKEN: u32 = 15;

/// The card's folders: path, then its entries (name, is a folder); what was asked.
struct Card {
    tree: Vec<(&'static str, Vec<(&'static str, bool)>)>,
    log: RefCell<Vec<String>>,
}

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

fn card() -> Rc<Card> {
    Rc::new(Card {
        tree: vec![
            (
                "",
                vec![
                    ("books", true),
                    ("Notes", true),
                    ("a.txt", false),
                    (".hidden", true),
                ],
            ),
            (
                "books",
                vec![
                    ("Crime", true),
                    ("Archive", true),
                    ("one.epub", false),
                    ("Zoo", true),
                ],
            ),
            ("books/Archive", vec![("Old", true)]),
            ("books/Crime", vec![]),
        ],
        log: RefCell::new(Vec::new()),
    })
}

fn script_files(host: &ScriptedHost, card: &Rc<Card>) {
    let card = card.clone();
    host.service("core.files", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, Some(Record::ServiceFsList(list))) => {
                card.log
                    .borrow_mut()
                    .push(format!("list {:?} from {}", list.path, list.cursor));
                let Some((_, entries)) = card.tree.iter().find(|(path, _)| *path == list.path)
                else {
                    return Err(ERR_NOT_FOUND);
                };
                // Two entries a page.
                let from = usize::try_from(list.cursor).unwrap();
                let page = entries.iter().skip(from).take(2);
                let mut out: Vec<Record<'_>> = page
                    .map(|(name, folder)| {
                        Record::ServiceFsEntry(ServiceFsEntry {
                            name,
                            flags: u32::from(*folder),
                            size: 0,
                            modified: None,
                        })
                    })
                    .collect();
                let more = from + 2 < entries.len();
                out.push(Record::ServiceEnd(ServiceEnd {
                    flags: u32::from(!more),
                    total: 0,
                    generation: 0,
                    next: 0,
                    cursor: more.then(|| (from + 2) as u64),
                }));
                Ok(stream(&out))
            }
            _ => Err(-1),
        }
    });
}

/// The library is opened with the screens; the chooser asks it nothing.
fn script_library(host: &ScriptedHost) {
    host.service("core.library", 1, 0, |_, _| Err(-1));
}

fn script_ui(host: &ScriptedHost) {
    host.service("core.ui", 1, 0, move |op, _| match op {
        16 => Ok(stream(&[Record::ServiceUiHandle(ServiceUiHandle {
            handle: 7,
        })])),
        17 => {
            let mut out = vec![Record::ServiceUiParam(ServiceUiParam {
                key: "id",
                value: "files.archive",
            })];
            out.push(Record::ServiceEnd(ServiceEnd {
                flags: 0,
                total: 1,
                generation: 0,
                next: 0,
                cursor: None,
            }));
            Ok(stream(&out))
        }
        _ => Err(-1),
    });
}

fn setup(card: &Rc<Card>, archive: &str) -> ScriptedHost {
    let host = ScriptedHost::new();
    script_files(&host, card);
    script_library(&host);
    script_ui(&host);
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    host.set_setting("files.archive", SettingValueRef::Text(archive));
    host
}

fn open(host: &ScriptedHost) -> (Plugin<'_>, i32) {
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "files.choose-folder",
        instance: 1,
        slot: "core.choose-folder",
        flags: 0,
        handover: 7,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    let status = plugin.event(CHOOSE_TOKEN, &opened).unwrap().status;
    let shown = event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen: "files.choose-folder",
        width: 480,
        height: 800,
        instance: Some(1),
        last_input: 0,
    }))
    .unwrap();
    // The redraw a show asks for is refused: this host has no screens.
    let shown = plugin.event(CHOOSE_TOKEN, &shown).unwrap().status;
    assert!(shown == 0 || shown == ERR_UNSUPPORTED, "{shown}");
    (plugin, status)
}

fn action(name: &str) -> Vec<u8> {
    event_batch(&Record::InputAction(InputAction {
        now_ms: 0,
        action: name,
    }))
    .unwrap()
}

#[test]
fn it_starts_on_the_folder_the_setting_names_and_reads_it_page_by_page() {
    let card = card();
    let host = setup(&card, "books");
    let (_plugin, status) = open(&host);
    assert_eq!(status, 0);
    let log = card.log.borrow();
    // Four entries in two pages: the second goes on from where the first ended.
    assert_eq!(log[..2], ["list \"books\" from 0", "list \"books\" from 2"]);
}

#[test]
fn a_folder_that_is_gone_starts_the_chooser_at_the_top() {
    let card = card();
    let host = setup(&card, "books/Gone");
    let (_plugin, status) = open(&host);
    assert_eq!(status, 0);
    let log = card.log.borrow();
    assert_eq!(log[0], "list \"books/Gone\" from 0");
    assert_eq!(log[1], "list \"\" from 0");
}

#[test]
fn using_the_shown_folder_writes_it_to_the_setting() {
    let card = card();
    let host = setup(&card, "books/Archive");
    let (plugin, _) = open(&host);
    // The focus is on the first row: use this folder.
    // The setting is not registered with this host, which refuses the change
    // (and the back that follows needs a screen): what the plugin submitted is
    // what is looked at.
    let done = plugin.input(CHOOSE_TOKEN, &action("core.confirm")).unwrap();
    assert!(done.batches_contain(b"files.archive"));
    assert!(done.batches_contain(b"books/Archive"));
}

#[test]
fn back_goes_up_a_folder_and_at_the_top_leaves() {
    let card = card();
    let host = setup(&card, "books/Archive");
    let (plugin, _) = open(&host);
    card.log.borrow_mut().clear();
    let up = plugin.input(CHOOSE_TOKEN, &action("core.back")).unwrap();
    // Taken: the parent is read (the redraw after it is refused by this host).
    assert!(
        up.status == 1 || up.status == ERR_UNSUPPORTED,
        "{}",
        up.status
    );
    assert_eq!(card.log.borrow()[0], "list \"books\" from 0");
}

#[test]
fn at_the_top_back_is_left_to_the_host() {
    let card = card();
    let host = setup(&card, "");
    let (plugin, _) = open(&host);
    let done = plugin.input(CHOOSE_TOKEN, &action("core.back")).unwrap();
    assert_eq!(done.status, 0);
}

#[test]
fn no_refusal_of_the_card_makes_the_chooser_abort() {
    for slot in [Slot::ServiceCall, Slot::ServiceOpen] {
        let results = sweep(slot, -ERR_IO.abs(), |host, _| {
            let card = card();
            script_files(host, &card);
            script_library(host);
            script_ui(host);
            host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
                .unwrap();
            host.set_setting("files.archive", SettingValueRef::Text("books"));
            let Ok(plugin) = host.load(NATIVE_ENTRY.entry()) else {
                return 0;
            };
            let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
                now_ms: 0,
                screen: "files.choose-folder",
                instance: 1,
                slot: "core.choose-folder",
                flags: 0,
                handover: 7,
                params: 0,
                text_bytes: 0,
            }))
            .unwrap();
            plugin
                .event(CHOOSE_TOKEN, &opened)
                .map_or_else(|status| status, |done| done.status)
        });
        assert_eq!(results[0].1, 0);
        assert!(
            results.iter().all(|(_, status)| *status <= 0),
            "{results:?}"
        );
    }
}
