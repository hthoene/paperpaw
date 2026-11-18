//! The library's own list over a scripted `core.library`, `core.ui` and
//! `core.files`: what it asks the catalog (the archive left out, the state),
//! that a card that is gone or a folder that cannot be read still opens a
//! list, that a change of the library reads it again, and what holding a book
//! does: the sheet's question, the archive's move to a free name, the
//! deleting asked about first, finished, information and opening.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventAnswered, EventLibraryChanged, EventScreenOpened, EventScreenShown, InputAction, Record,
    ServiceEnd, ServiceFsEntry, ServiceLibBook, ServiceLibEntry, ServiceLibResult, ServiceLibState,
    ServiceUiHandle, ServiceUiParam, ServiceUiRequest, record_encode_one, records_decode,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_IO, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED};
use paperpaw_plugin_files::NATIVE_ENTRY;
use paperpaw_scripted_host::{Plugin, ScriptedHost, Slot, event_batch, sweep};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const BROWSER_TOKEN: u32 = 10;
const SHELF_TOKEN: u32 = 11;
const EVENTS_TOKEN: u32 = 3;
const GENERATION: u32 = 4;

/// The scripted host has no screens: a command that needs one (a redraw, a
/// notice, an open) is refused with this status, after the plugin asked the
/// services what the test looks at. A callback is fine when it ends so.
const NO_SCREENS: i32 = ERR_UNSUPPORTED;

/// A hand-over the host did not give out: the scripted host has no stage that knows it.
const NO_HANDOVER: i32 = paperpaw_native::host_glue::ERR_INVALID;

fn fine(status: i32) -> bool {
    status == 0 || status == NO_SCREENS || status == NO_HANDOVER
}

/// What a test changes in the scripted services.
#[derive(Default)]
struct Knobs {
    /// The catalog's phase: 0 empty, 1 walking, 2 ready.
    phase: Cell<u32>,
    /// A status `state` answers instead of a state.
    state_error: Cell<i32>,
    /// A status `query` answers instead of books.
    query_error: Cell<i32>,
    /// The books are all in the archive: a query that leaves it out finds none.
    archived: Cell<bool>,
    /// The book is marked finished.
    finished: Cell<bool>,
    /// What `delete` says of the book.
    delete_status: Cell<i32>,
    /// How many `move`s answer "taken" before one succeeds.
    taken: Cell<u32>,
    /// The card answers `stat`.
    card_gone: Cell<bool>,
    /// What the services were asked, in order.
    log: RefCell<Vec<String>>,
}

type World = Rc<Knobs>;

fn said(world: &World, line: String) {
    world.log.borrow_mut().push(line);
}

fn log(world: &World) -> Vec<String> {
    world.log.borrow().clone()
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

fn end(total: u32) -> Record<'static> {
    Record::ServiceEnd(ServiceEnd {
        flags: 1,
        total,
        generation: GENERATION,
        next: total,
        cursor: None,
    })
}

fn entry(key: u32, title: &'static str, progress: u32) -> Record<'static> {
    Record::ServiceLibEntry(ServiceLibEntry {
        key,
        title,
        author: "Ann",
        series: "",
        number: "",
        progress,
        flags: 2,
        added: 0,
        badges: "",
    })
}

fn script_library(host: &ScriptedHost, world: &World) {
    let world = world.clone();
    host.service("core.library", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, _) => {
                said(&world, "state".into());
                if world.state_error.get() != 0 {
                    return Err(world.state_error.get());
                }
                Ok(stream(&[Record::ServiceLibState(ServiceLibState {
                    state: world.phase.get(),
                    books: 3,
                    generation: GENERATION,
                    walk_ppm: 500_000,
                })]))
            }
            (2, Some(Record::ServiceLibQuery(query))) => {
                said(
                    &world,
                    format!("query show {} flags {}", query.show, query.flags),
                );
                if world.query_error.get() != 0 {
                    return Err(world.query_error.get());
                }
                if world.archived.get() && query.flags & 2 != 0 {
                    return Ok(stream(&[end(0)]));
                }
                Ok(stream(&[
                    entry(1, "One", 40),
                    entry(2, "Two", 0),
                    entry(3, "Three", 100),
                    end(3),
                ]))
            }
            (3, Some(Record::ServiceLibRef(book))) => {
                let key = book.key.unwrap_or(1);
                said(&world, format!("book {key}"));
                Ok(stream(&[Record::ServiceLibBook(ServiceLibBook {
                    key,
                    generation: GENERATION,
                    path: "books/Crime/one.epub",
                    title: "One",
                    author: "Ann",
                    series: "",
                    number: "",
                    progress: 40,
                    flags: if world.finished.get() { 4 } else { 2 },
                    added: 0,
                    size: 1000,
                    format: "epub",
                    tags: 0,
                    fields: 0,
                    title_bytes: 3,
                    author_bytes: 3,
                    series_bytes: 0,
                })]))
            }
            (8, Some(Record::ServiceLibRef(book))) => {
                let Some(Record::ServiceLibStatus(status)) = rows.get(1) else {
                    return Err(-1);
                };
                said(
                    &world,
                    format!(
                        "status {} set {} clear {}",
                        book.path, status.set, status.clear
                    ),
                );
                Ok(Vec::new())
            }
            (9 | 10, Some(Record::ServiceLibPaths(paths))) => {
                let name = if op == 9 { "can_delete" } else { "delete" };
                said(
                    &world,
                    format!("{name} {} flags {}", paths.paths, paths.flags),
                );
                let status = if op == 10 {
                    world.delete_status.get()
                } else {
                    0
                };
                Ok(stream(&[
                    Record::ServiceLibResult(ServiceLibResult {
                        path: paths.paths,
                        status,
                        flags: 0,
                    }),
                    Record::ServiceEnd(ServiceEnd {
                        flags: 0,
                        total: 1,
                        generation: 0,
                        next: 0,
                        cursor: None,
                    }),
                ]))
            }
            _ => Err(-1),
        }
    });
}

fn script_ui(host: &ScriptedHost, world: &World) {
    let world = world.clone();
    let asked = Rc::new(Cell::new(0u32));
    host.service("core.ui", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (8, Some(Record::ServiceUiAsk(ask))) => {
                asked.set(asked.get() + 1);
                said(
                    &world,
                    format!(
                        "ask kind {} title {} text {} choices {}",
                        ask.kind,
                        ask.title,
                        ask.text,
                        ask.choices.replace('\n', "|")
                    ),
                );
                Ok(stream(&[Record::ServiceUiRequest(ServiceUiRequest {
                    request: asked.get(),
                })]))
            }
            (17, _) => {
                let params = [("kind", "favourites"), ("value", "")];
                let mut out: Vec<Record<'_>> = params
                    .iter()
                    .map(|(key, value)| Record::ServiceUiParam(ServiceUiParam { key, value }))
                    .collect();
                out.push(Record::ServiceEnd(ServiceEnd {
                    flags: 0,
                    total: 2,
                    generation: 0,
                    next: 0,
                    cursor: None,
                }));
                Ok(stream(&out))
            }
            (16, _) => {
                let mut params = Vec::new();
                for row in &rows {
                    if let Record::ServiceUiParam(param) = row {
                        params.push(format!("{}={}", param.key, param.value));
                    }
                }
                said(&world, format!("handover {}", params.join(",")));
                Ok(stream(&[Record::ServiceUiHandle(ServiceUiHandle {
                    handle: 7,
                })]))
            }
            _ => Err(-1),
        }
    });
}

fn script_files(host: &ScriptedHost, world: &World) {
    let world = world.clone();
    let moves = Rc::new(Cell::new(0u32));
    host.service("core.files", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (2, Some(Record::ServiceFsPath(path))) => {
                said(&world, format!("stat {:?}", path.path));
                if world.card_gone.get() {
                    return Err(ERR_IO);
                }
                Ok(stream(&[Record::ServiceFsEntry(ServiceFsEntry {
                    name: "",
                    flags: 1,
                    size: 0,
                    modified: None,
                })]))
            }
            (4, Some(Record::ServiceFsPath(path))) => {
                said(&world, format!("make_dir {}", path.path));
                Ok(Vec::new())
            }
            (6, Some(Record::ServiceFsMove(go))) => {
                said(
                    &world,
                    format!("move {} -> {} replace {}", go.from, go.to, go.flags),
                );
                if moves.get() < world.taken.get() {
                    moves.set(moves.get() + 1);
                    return Err(ERR_STATE);
                }
                Ok(Vec::new())
            }
            _ => Err(-1),
        }
    });
}

fn opened(screen: &str, handover: u32) -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen,
        instance: 1,
        slot: "",
        flags: 0,
        handover,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

fn shown(screen: &str) -> Vec<u8> {
    event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen,
        width: 480,
        height: 800,
        instance: Some(1),
        last_input: 0,
    }))
    .unwrap()
}

fn action(name: &str) -> Vec<u8> {
    event_batch(&Record::InputAction(InputAction {
        now_ms: 0,
        action: name,
    }))
    .unwrap()
}

fn answered(request: u32, kind: u32, choice: u32) -> Vec<u8> {
    event_batch(&Record::EventAnswered(EventAnswered {
        now_ms: 0,
        request,
        kind,
        outcome: 0,
        choice,
        length: 0,
        flags: 0,
    }))
    .unwrap()
}

fn changed(what: u32) -> Vec<u8> {
    event_batch(&Record::EventLibraryChanged(EventLibraryChanged {
        now_ms: 0,
        what,
        generation: GENERATION,
        path: "",
    }))
    .unwrap()
}

/// A host with the services scripted and the library opened on its list.
fn setup(world: &World) -> ScriptedHost {
    let host = ScriptedHost::new();
    script_library(&host, world);
    script_ui(&host, world);
    script_files(&host, world);
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    // The archive folder as the plugin's setting ships it.
    host.set_setting(
        "files.archive",
        paperpaw_core::native_host::environment::SettingValueRef::Text("books/Archive"),
    );
    host
}

fn open_screen<'h>(host: &'h ScriptedHost, token: u32, screen: &str, handover: u32) -> Plugin<'h> {
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let done = plugin.event(token, &opened(screen, handover)).unwrap();
    assert_eq!(done.status, 0);
    // Shown: the list is read again, and the redraw it asks for is refused.
    let done = plugin.event(token, &shown(screen)).unwrap();
    assert!(fine(done.status), "{}", done.status);
    plugin
}

fn open_browser(host: &ScriptedHost) -> Plugin<'_> {
    open_screen(host, BROWSER_TOKEN, "files.browser", 0)
}

/// The favourites: a list without a filter row, the focus on its first book.
fn open_favourites(host: &ScriptedHost) -> Plugin<'_> {
    open_screen(host, SHELF_TOKEN, "files.shelf", 7)
}

/// Holds the first book with the menu button.
fn hold_first_book(plugin: &Plugin<'_>) {
    let menu = plugin.input(SHELF_TOKEN, &action("core.menu")).unwrap();
    assert_eq!(menu.status, 1);
}

fn queries(world: &World) -> usize {
    log(world).iter().filter(|l| l.starts_with("query")).count()
}

fn log_has(world: &World, line: &str) -> bool {
    log(world).iter().any(|l| l == line)
}

#[test]
fn the_library_asks_for_the_books_with_the_archive_left_out() {
    let world = World::default();
    world.phase.set(2);
    let host = setup(&world);
    let _plugin = open_browser(&host);
    let log = log(&world);
    // The list is of the order of reading, the archive left out (flag 2).
    assert!(log_has(&world, "query show 0 flags 2"), "{log:?}");
    assert!(log_has(&world, "state"));
    // The card is asked about only when the catalog cannot be read.
    assert!(
        !log.iter().any(|line| line.starts_with("stat \"")),
        "{log:?}"
    );
}

#[test]
fn every_state_of_the_library_still_opens_its_list() {
    for (phase, state_error, query_error, card_gone) in [
        // Preparing, scanning, ready.
        (0, 0, 0, false),
        (1, 0, 0, false),
        (2, 0, 0, false),
        // The catalog cannot be asked: a folder that cannot be read.
        (2, ERR_IO, 0, false),
        (2, 0, ERR_IO, false),
        // The card is gone.
        (2, ERR_IO, ERR_IO, true),
        // The host is busy making the catalog.
        (2, ERR_BUSY, 0, false),
    ] {
        let world = World::default();
        world.phase.set(phase);
        world.state_error.set(state_error);
        world.query_error.set(query_error);
        world.card_gone.set(card_gone);
        let host = setup(&world);
        let _plugin = open_browser(&host);
        let asked_card = log(&world).iter().any(|line| line.starts_with("stat \""));
        assert_eq!(
            asked_card,
            state_error == ERR_IO || query_error == ERR_IO,
            "{phase} {state_error} {query_error}"
        );
    }
}

#[test]
fn a_library_whose_books_are_all_in_the_archive_asks_whether_it_has_any() {
    let world = World::default();
    world.phase.set(2);
    world.archived.set(true);
    let host = setup(&world);
    let _plugin = open_browser(&host);
    // The list leaves the archive out and finds none; the question is asked
    // again without that, and a library with books says they are all there.
    assert!(log_has(&world, "query show 0 flags 0"), "{:?}", log(&world));
    assert!(queries(&world) >= 2);
}

#[test]
fn a_change_of_the_books_reads_the_list_again_and_a_ready_cover_does_not() {
    let world = World::default();
    world.phase.set(2);
    let host = setup(&world);
    let plugin = open_browser(&host);
    let first = queries(&world);
    // Bit 6: a cover is ready; no book changed.
    let done = plugin.event(EVENTS_TOKEN, &changed(1 << 6)).unwrap();
    assert!(fine(done.status), "{}", done.status);
    assert_eq!(queries(&world), first);
    // Bit 0: books were added; bit 5: the walk progressed; bit 1: a status.
    for what in [1, 1 << 5, 1 << 1] {
        let before = queries(&world);
        let done = plugin.event(EVENTS_TOKEN, &changed(what)).unwrap();
        assert!(fine(done.status), "{what}");
        assert_eq!(queries(&world), before + 1, "{what}");
    }
    // A catalog that cannot be read now: the list stays as it is, the
    // callback ends cleanly.
    world.query_error.set(ERR_NOT_FOUND);
    let done = plugin.event(EVENTS_TOKEN, &changed(1)).unwrap();
    assert!(fine(done.status), "{}", done.status);
}

#[test]
fn holding_a_book_asks_what_to_do_with_it() {
    let world = World::default();
    world.phase.set(2);
    let host = setup(&world);
    let plugin = open_favourites(&host);
    hold_first_book(&plugin);
    let log = log(&world);
    assert!(log_has(&world, "book 1"), "{log:?}");
    assert!(
        log_has(&world, "can_delete books/Crime/one.epub flags 0"),
        "{log:?}"
    );
    assert!(
        log_has(
            &world,
            "ask kind 1 title One text one.epub choices \
             Open|Mark as finished|Book information|Move to the archive|Delete …"
        ),
        "{log:?}"
    );
}

#[test]
fn a_finished_book_offers_to_mark_it_unread() {
    let world = World::default();
    world.phase.set(2);
    world.finished.set(true);
    let host = setup(&world);
    let plugin = open_favourites(&host);
    hold_first_book(&plugin);
    assert!(
        log(&world).iter().any(|l| l.ends_with(
            "choices Open|Mark as unread|Book information|Move to the archive|Delete …"
        )),
        "{:?}",
        log(&world)
    );
}

#[test]
fn the_sheets_choices_do_what_they_say() {
    let world = World::default();
    world.phase.set(2);
    let host = setup(&world);
    let plugin = open_favourites(&host);
    // Finished: the book is marked through its path, then the list is read again.
    hold_first_book(&plugin);
    let before = log(&world).len();
    let done = plugin.event(0, &answered(1, 1, 1)).unwrap();
    assert!(fine(done.status), "{}", done.status);
    assert!(
        log(&world)[before..].contains(&"status books/Crime/one.epub set 1 clear 0".to_string()),
        "{:?}",
        log(&world)
    );
    // Information opens its screen with the path (the open itself is refused
    // by this host: it has no screens).
    hold_first_book(&plugin);
    let done = plugin.event(0, &answered(2, 1, 2)).unwrap();
    assert!(fine(done.status), "{}", done.status);
    assert!(log_has(&world, "handover path=books/Crime/one.epub"));
    // Open goes to the reader with the path too.
    let before = log(&world).len();
    hold_first_book(&plugin);
    let done = plugin.event(0, &answered(3, 1, 0)).unwrap();
    assert!(fine(done.status), "{}", done.status);
    assert!(
        log(&world)[before..].contains(&"handover path=books/Crime/one.epub".to_string()),
        "{:?}",
        log(&world)
    );
    assert!(!done.batches.is_empty());
}

#[test]
fn the_archive_takes_the_book_under_a_free_name() {
    let world = World::default();
    world.phase.set(2);
    world.taken.set(1);
    let host = setup(&world);
    let plugin = open_favourites(&host);
    hold_first_book(&plugin);
    let done = plugin.event(0, &answered(1, 1, 3)).unwrap();
    assert!(fine(done.status), "{}", done.status);
    let log = log(&world);
    assert!(
        log.contains(&"make_dir books/Archive".to_string()),
        "{log:?}"
    );
    // The first name is taken; the book goes in as `one (2).epub`.
    for to in ["books/Archive/one.epub", "books/Archive/one (2).epub"] {
        assert!(
            log.contains(&format!("move books/Crime/one.epub -> {to} replace 0")),
            "{log:?}"
        );
    }
    // A move never replaces, and it says how it went (a notice, which this
    // host refuses for want of screens, after the move).
    assert!(!log.iter().any(|l| l.ends_with("replace 1")));
    assert!(!done.batches.is_empty());
}

#[test]
fn a_book_is_deleted_only_after_the_question_is_answered_yes() {
    let world = World::default();
    world.phase.set(2);
    let host = setup(&world);
    let plugin = open_favourites(&host);
    hold_first_book(&plugin);
    // Delete is the sheet's last choice: it asks first, deleting nothing.
    let done = plugin.event(0, &answered(1, 1, 4)).unwrap();
    assert_eq!(done.status, 0);
    assert!(!log(&world).iter().any(|l| l.starts_with("delete")));
    assert!(
        log_has(
            &world,
            "ask kind 2 title Delete this book, with its place, bookmarks and notes? \
             This cannot be undone. text  choices Delete|Keep it"
        ),
        "{:?}",
        log(&world)
    );
    // No: nothing is deleted.
    let done = plugin.event(0, &answered(2, 2, 0)).unwrap();
    assert_eq!(done.status, 0);
    assert!(!log(&world).iter().any(|l| l.starts_with("delete ")));
    // Yes: the book goes with what is kept for it (flag 1: with notes).
    hold_first_book(&plugin);
    plugin.event(0, &answered(3, 1, 4)).unwrap();
    let done = plugin.event(0, &answered(4, 2, 1)).unwrap();
    assert!(fine(done.status), "{}", done.status);
    assert!(
        log_has(&world, "delete books/Crime/one.epub flags 1"),
        "{:?}",
        log(&world)
    );
}

#[test]
fn a_refused_delete_is_told_and_never_aborts() {
    for status in [0, ERR_NOT_FOUND, ERR_IO, -22] {
        let world = World::default();
        world.phase.set(2);
        world.delete_status.set(status);
        let host = setup(&world);
        let plugin = open_favourites(&host);
        hold_first_book(&plugin);
        plugin.event(0, &answered(1, 1, 4)).unwrap();
        let done = plugin.event(0, &answered(2, 2, 1)).unwrap();
        assert!(fine(done.status), "{status}: {}", done.status);
        // A notice said how it went: submitted, though this host has no screen for it.
        assert!(!done.batches.is_empty(), "{status}");
    }
}

#[test]
fn the_menu_button_on_the_filter_row_opens_the_librarys_menu() {
    let world = World::default();
    world.phase.set(2);
    let host = setup(&world);
    let plugin = open_browser(&host);
    let done = plugin.input(BROWSER_TOKEN, &action("core.menu")).unwrap();
    assert!(fine(done.status), "{}", done.status);
    assert!(
        log_has(&world, "handover menu=files.menu"),
        "{:?}",
        log(&world)
    );
}

#[test]
fn no_refusal_of_a_service_makes_the_list_or_the_sheet_abort() {
    for slot in [Slot::ServiceCall, Slot::ServiceOpen] {
        let results = sweep(slot, -5, |host, _| {
            let world = World::default();
            world.phase.set(2);
            script_library(host, &world);
            script_ui(host, &world);
            script_files(host, &world);
            host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
                .unwrap();
            let Ok(plugin) = host.load(NATIVE_ENTRY.entry()) else {
                return 0;
            };
            let open = plugin
                .event(SHELF_TOKEN, &opened("files.shelf", 7))
                .map_or_else(|status| status, |done| done.status);
            if open != 0 {
                return open;
            }
            let _ = plugin.event(SHELF_TOKEN, &shown("files.shelf"));
            let _ = plugin.input(SHELF_TOKEN, &action("core.menu"));
            let _ = plugin.event(0, &answered(1, 1, 3));
            0
        });
        // A refused call is an error status of the callback, never a hang or a panic.
        assert_eq!(results[0].1, 0);
        assert!(
            results.iter().all(|(_, status)| *status <= 0),
            "{results:?}"
        );
    }
}
