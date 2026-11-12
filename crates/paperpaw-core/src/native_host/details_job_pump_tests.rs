//! The host's details job through the kernel's pump (`details_job_pump`): a
//! kernel with a native format plugin in its instance registry and in its
//! declarations, the production document and library hosts wired on one card.
//! The format answers each op as its own call (the pump's route); the details
//! it names are kept, the catalog is wanted again once, and the rules of the
//! one background slot (start, reading, sleep, failed books) hold.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::native_host::{
    declaration_store::{Allocations, ImageOwner, PendingDeclarations},
    doc_host::DocState,
    doc_service::{DocHost, FAILED, FormatRef, Limits, PREPARING, READY},
    lib_host::LibraryHost,
    lib_reads::CatalogEnv,
    lib_service::LibQuery,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations::{self as wire, REGISTRATION_MAX_BYTES, Record, records_encode},
    shared::Shared,
    stages_wire::core_format_v1 as local,
};
use alloc::{rc::Rc, string::ToString, vec, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_platform::{DirEntry, Instant};
use paperpaw_sdk::{Command, Id, Manifest, MemoryStorage, Params, PluginState, Version};

/// The card, shared by the kernel and the hosts.
struct Card(Rc<MemoryStorage>);
impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.0.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.0.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.0.append(p, d)
    }
    fn write_at(&self, p: &str, o: u64, d: &[u8]) -> Result<(), StorageError> {
        self.0.write_at(p, o, d)
    }
    fn truncate(&self, p: &str, len: u64) -> Result<(), StorageError> {
        self.0.truncate(p, len)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.0.remove(p)
    }
    fn exists(&self, p: &str) -> bool {
        self.0.exists(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.0.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.0.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(p)
    }
    fn entry_identity(&self, p: &str) -> Result<Option<u64>, StorageError> {
        self.0.entry_identity(p)
    }
}

/// The format plugin's declaration, under the key the registry gave it.
struct Declarer(u64);
impl ImageOwner for Declarer {
    fn plugin_id(&self) -> &str {
        "format-books"
    }
    fn generation(&self) -> u32 {
        1
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(self.0)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(Some(&[]));
    }
    fn invalid_declaration(&self, _: i32) {}
}

/// What the format answers, and the ops it was called with.
#[derive(Default)]
struct Format {
    ops: RefCell<Vec<u32>>,
    /// The state its open answers (and its status).
    open: Cell<u32>,
    status: Cell<i32>,
    /// The title its `meta` answers.
    title: RefCell<std::string::String>,
}

impl Format {
    fn opens(&self) -> usize {
        self.ops.borrow().iter().filter(|op| **op == 2).count()
    }
}

struct Probe(Rc<Format>);

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
}

fn encoded(record: local::Record<'_>, out: &mut [u8]) -> usize {
    local::reply_encode(&[record], out).unwrap()
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
    fn stage(
        &self,
        token: u32,
        bytes: &[u8],
        out: &mut [u8],
        _: &paperpaw_sdk::Env<'_>,
    ) -> Result<i32, i32> {
        assert_eq!(token, 5, "the token of the format's declaration");
        let Record::StageCall(call) = wire::records_decode_single(bytes).unwrap() else {
            panic!("not a stage.call")
        };
        let Some(Ok(local::Record::FmtCall(call))) =
            local::input_decode(call.payload).unwrap().next()
        else {
            panic!("not a fmt.call")
        };
        self.0.ops.borrow_mut().push(call.op);
        let state = |state, status| {
            local::Record::FmtState(local::FmtState::new(
                call.document,
                state,
                0,
                status,
                0,
                call.conversion,
                "format-books.books",
            ))
        };
        let title = self.0.title.borrow();
        // A format that names no title names nothing else either.
        let authors = u32::from(!title.is_empty());
        let n = match call.op {
            2 => encoded(state(self.0.open.get(), self.0.status.get()), out),
            3 => encoded(state(READY, 0), out),
            5 => encoded(
                local::Record::FmtMeta(local::FmtMeta::new(
                    &title, "en", "", "", 0, 0, 3, authors, 0, 0, 0,
                )),
                out,
            ),
            6 => encoded(
                local::Record::FmtText(local::FmtText::new(1, 0, 6, "Author")),
                out,
            ),
            _ => local::reply_encode(&[], out).unwrap(),
        };
        Ok(i32::try_from(n).unwrap())
    }
}

/// A plugin with home's screen (offered for slot `core.home`) and a page of
/// its own (offered for none), as the store's.
struct Shelf;
const HOME_SCREEN: Id = Id::new_static("shelf.home");
const PAGE: Id = Id::new_static("shelf.page");

struct Blank;
impl paperpaw_sdk::Screen for Blank {
    fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

impl paperpaw_sdk::Plugin for Shelf {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("shelf"), "Shelf", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        for id in [HOME_SCREEN, PAGE] {
            r.add_screen(paperpaw_sdk::ScreenSpec::new(
                id,
                "shelf.screen",
                alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Blank)),
            ))?;
        }
        r.bind_slot(slots::HOME, HOME_SCREEN, paperpaw_sdk::Priority::DEFAULT)
    }
}

/// No walk runs, no clock.
struct Calm;
impl CatalogEnv for Calm {
    fn walking(&self) -> Option<u32> {
        None
    }
    fn today(&self) -> Option<u32> {
        None
    }
}

struct World {
    kernel: crate::Kernel,
    card: Rc<MemoryStorage>,
    format: Rc<Format>,
    docs: Rc<DocState>,
    library: Rc<LibraryHost>,
}

/// A kernel with the native format `format-books.books` (EPUB, DOCX) on a card
/// holding `books`, home's screen on top; not started.
fn world(books: &[&str]) -> World {
    let mut card = MemoryStorage::new();
    for book in books {
        card = card.with(book, b"a book".to_vec());
    }
    let card = Rc::new(card);
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(Card(card.clone()))
        .plugin(Shelf)
        .build()
        .unwrap();
    kernel.open(HOME_SCREEN, Some(slots::HOME), &Params::default());
    let format = Rc::new(Format::default());
    *format.title.borrow_mut() = "Dune".to_string();
    let key = kernel
        .shared
        .native_instances
        .test_insert(
            Id::new_static("format-books"),
            Some(NativeLease::try_new(Probe(format.clone())).unwrap()),
            true,
        )
        .unwrap();
    let hosts = kernel.shared.native_instances.hosts().unwrap();
    let docs = Rc::new(
        DocState::try_new(Limits::X4, 800, hosts.in_use().clone())
            .unwrap()
            .with_storage(Some(Rc::new(Card(card.clone())))),
    );
    hosts.wire_documents(docs.clone()).unwrap();
    let library = Rc::new(LibraryHost::new(
        Rc::new(Card(card.clone())),
        Rc::new(Calm),
        None,
    ));
    hosts.wire_library(library.clone()).unwrap();
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    declare(&mut directory.store.borrow_mut(), key);
    kernel.shared.registry.native_declarations = Some(directory);
    kernel.add_plugin_info(
        Manifest::new(
            Id::new_static("format-books"),
            "Books",
            Version::new(1, 0, 0),
        ),
        PluginState::Running,
        true,
    );
    World {
        kernel,
        card,
        format,
        docs,
        library,
    }
}

fn declare(store: &mut crate::native_host::declaration_store::DeclarationStore, key: u64) {
    let record = Record::FormatSpec(wire::FormatSpec {
        name: "books",
        title: "format.title",
        extensions: "epub\ndocx",
        flags: 0,
        priority: 0,
        token: 5,
    });
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&[record], &mut bytes).unwrap();
    let mut allocations = Allocations::default();
    let source: Rc<dyn ImageOwner> = Rc::new(Declarer(key));
    let mut pending = PendingDeclarations::new(Id::new_static("format-books"), source);
    pending
        .stage_batch(&bytes[..len], &mut allocations)
        .unwrap();
    store.prepare(pending, &mut allocations).unwrap().commit();
}

/// The catalog as a walk leaves it: each book with its size where its details
/// were read (`true`), else none.
fn catalog(w: &World, books: &[(&str, bool)]) {
    let records: Vec<catalog::Record> = books
        .iter()
        .map(|(path, read)| {
            let mut book = paperpaw_sdk::library::Book::default();
            book.size = if *read { 6 } else { 0 };
            catalog::Record::of(path, &book)
        })
        .collect();
    catalog::write(&*w.card, &records).unwrap();
}

/// The kernel's start has returned (a marker left from before is read).
fn started(w: &mut World) {
    let _ = w.kernel.finish_started();
    w.kernel.output.commands.clear();
}

fn pump(w: &mut World, now: u64) {
    w.kernel.now = Instant(now);
    w.kernel.native_service_pump();
}

/// How many `core.catalog-wanted` (made anew) the kernel has to run.
fn wanted(w: &World) -> usize {
    w.kernel
        .output
        .commands
        .iter()
        .filter(|c| {
            matches!(c, Command::Emit(e, p) if *e == events::CATALOG_WANTED
                && p.get("anew") == Some("true"))
        })
        .count()
}

fn kept_title(w: &World, path: &str) -> Option<std::string::String> {
    library::try_book(&*w.card, path)
        .ok()
        .flatten()
        .and_then(|book| book.title)
}

#[test]
fn the_quiet_second_counts_from_the_start() {
    let mut w = world(&[]);
    w.kernel.last_activity = Instant(5_000);
    started(&mut w);
    pump(&mut w, 5_500);
    assert!(
        !DocHost::cover_quiet(&*w.docs),
        "no cover opens at the start"
    );
    assert!(DocHost::cover_must_wait(&*w.docs));
    pump(&mut w, 6_100);
    assert!(DocHost::cover_quiet(&*w.docs), "quiet a second after it");
}

#[test]
fn native_books_are_read_one_at_a_time_and_the_catalog_is_wanted_once_at_its_end() {
    let mut w = world(&["books/a.epub", "books/b.docx", "books/c.epub"]);
    catalog(
        &w,
        &[
            ("books/a.epub", false),
            ("books/b.docx", false),
            ("books/c.epub", true),
        ],
    );
    started(&mut w);
    w.kernel.native_details_arm();
    pump(&mut w, 1_000);
    // One book a pass, read at once (its format is ready at the open).
    assert_eq!(*w.format.ops.borrow(), [2, 5, 6, 4]);
    assert_eq!(kept_title(&w, "books/a.epub").as_deref(), Some("Dune"));
    assert!(!w.docs.cover_job_active(), "closed again");
    assert_eq!(wanted(&w), 0, "never once per book");
    pump(&mut w, 2_000);
    assert_eq!(w.format.opens(), 2);
    assert_eq!(kept_title(&w, "books/b.docx").as_deref(), Some("Dune"));
    // The third book's details were read: the catalog has no book left.
    pump(&mut w, 3_000);
    assert_eq!(w.format.opens(), 2);
    assert_eq!(wanted(&w), 1);
    pump(&mut w, 4_000);
    assert_eq!(wanted(&w), 1, "disarmed");
    assert!(!w.docs.details_job().unwrap().run().armed());
}

#[test]
fn while_it_runs_the_catalog_is_wanted_at_most_once_in_two_minutes_and_none_when_nothing_was_kept()
{
    let books = [
        "books/a.epub",
        "books/b.epub",
        "books/c.epub",
        "books/d.epub",
    ];
    let mut w = world(&books);
    catalog(&w, &books.map(|book| (book, false)));
    started(&mut w);
    w.kernel.now = Instant(0);
    w.kernel.native_details_arm();
    pump(&mut w, 10_000);
    assert_eq!(wanted(&w), 0);
    pump(&mut w, 125_000);
    assert_eq!(wanted(&w), 1, "two minutes since the arm, a book kept");
    pump(&mut w, 130_000);
    assert_eq!(wanted(&w), 1, "not again within two minutes");
    pump(&mut w, 131_000);
    pump(&mut w, 132_000);
    assert_eq!(wanted(&w), 2, "the end, books kept since the last");
    // The walk it wants arms the job again; a pass that keeps nothing wants
    // nothing (the lines here still have no size: the format names nothing).
    w.format.title.borrow_mut().clear();
    w.kernel.output.commands.clear();
    w.kernel.native_details_arm();
    for at in 0..6 {
        pump(&mut w, 140_000 + at * 1_000);
    }
    assert_eq!(wanted(&w), 0);
    assert!(!w.docs.details_job().unwrap().run().armed());
}

#[test]
fn no_job_starts_before_the_start_returned_nor_unarmed_nor_while_a_book_is_open_for_reading() {
    let mut w = world(&["books/a.epub"]);
    catalog(&w, &[("books/a.epub", false)]);
    w.kernel.native_details_arm();
    pump(&mut w, 1_000);
    assert_eq!(w.format.opens(), 0, "the start has not returned");
    started(&mut w);
    w.docs.details_job().unwrap().run().disarm();
    pump(&mut w, 2_000);
    assert_eq!(w.format.opens(), 0, "not armed");
    // A book open for reading (a document of a plugin that is not background).
    let (reading, _) = w
        .docs
        .table()
        .open(7, true, FormatRef(0), READY, 0, &|_| false)
        .unwrap();
    w.kernel.native_details_arm();
    pump(&mut w, 3_000);
    assert_eq!(w.format.opens(), 0, "a book is open for reading");
    w.docs.table().close(7, reading).unwrap();
    pump(&mut w, 4_000);
    assert_eq!(w.format.opens(), 1);
    assert_eq!(kept_title(&w, "books/a.epub").as_deref(), Some("Dune"));
}

#[test]
fn a_book_whose_job_an_open_took_is_looked_at_again_and_never_holds_the_device_awake() {
    let mut w = world(&["books/a.epub", "books/b.epub"]);
    catalog(&w, &[("books/a.epub", false), ("books/b.epub", false)]);
    started(&mut w);
    w.format.open.set(PREPARING);
    w.kernel.native_details_arm();
    pump(&mut w, 1_000);
    assert_eq!(w.format.opens(), 1);
    assert!(w.docs.cover_job_active());
    // The job prepares: nothing asks for another pass, a timed wake for its
    // first step.
    assert!(!w.kernel.shared.native_instances.service_more());
    assert_eq!(w.kernel.service_due(), Some(Instant(1_100)));
    // A reader's open took its place (`doc_service::open`).
    assert!(w.docs.table().take_cover_job().is_some());
    w.format.open.set(READY);
    pump(&mut w, 2_000);
    pump(&mut w, 3_000);
    // The same book again first, then the next.
    assert_eq!(w.format.opens(), 3);
    assert_eq!(kept_title(&w, "books/a.epub").as_deref(), Some("Dune"));
    assert_eq!(kept_title(&w, "books/b.epub").as_deref(), Some("Dune"));
}

#[test]
fn sleep_ends_the_job_and_none_starts_after_the_wake_until_armed_again() {
    let mut w = world(&["books/a.epub"]);
    catalog(&w, &[("books/a.epub", false)]);
    started(&mut w);
    w.format.open.set(PREPARING);
    w.kernel.native_details_arm();
    pump(&mut w, 1_000);
    assert!(w.docs.cover_job_active());
    w.kernel.native_cover_job_suspend();
    assert!(!w.docs.cover_job_active(), "closed at the sleep");
    assert_eq!(w.format.ops.borrow().last(), Some(&4), "the format's close");
    w.format.open.set(READY);
    pump(&mut w, 2_000);
    pump(&mut w, 3_000);
    assert_eq!(w.format.opens(), 1, "not restarted after the wake");
    w.kernel.native_details_arm();
    pump(&mut w, 4_000);
    assert_eq!(w.format.opens(), 2);
    assert_eq!(kept_title(&w, "books/a.epub").as_deref(), Some("Dune"));
}

#[test]
fn a_book_the_format_failed_is_skipped_and_not_tried_again_in_this_run() {
    let mut w = world(&["books/a.epub", "books/b.epub"]);
    catalog(&w, &[("books/a.epub", false), ("books/b.epub", false)]);
    started(&mut w);
    w.format.open.set(FAILED);
    w.format.status.set(ERR_INVALID);
    w.kernel.native_details_arm();
    pump(&mut w, 1_000);
    assert!(w.library.book_failed(path_key("books/a.epub")));
    assert_eq!(kept_title(&w, "books/a.epub"), None, "nothing kept");
    w.format.open.set(READY);
    w.format.status.set(0);
    // Armed again (a walk): the failed book is passed by.
    w.kernel.native_details_arm();
    pump(&mut w, 2_000);
    pump(&mut w, 3_000);
    assert_eq!(w.format.opens(), 2);
    assert_eq!(kept_title(&w, "books/a.epub"), None);
    assert_eq!(kept_title(&w, "books/b.epub").as_deref(), Some("Dune"));
}

#[test]
fn a_book_whose_job_a_reset_ended_is_skipped_from_the_start() {
    let mut w = world(&["books/a.epub", "books/b.epub"]);
    catalog(&w, &[("books/a.epub", false), ("books/b.epub", false)]);
    // The format crashed the device in the job of `a`: its marker is left.
    let marker = std::format!("{:016x}", path_key("books/a.epub"));
    w.card
        .write(".paperpaw/cover-job.txt", marker.as_bytes())
        .unwrap();
    started(&mut w);
    assert!(w.library.book_failed(path_key("books/a.epub")));
    w.kernel.native_details_arm();
    pump(&mut w, 1_000);
    assert_eq!(w.format.opens(), 1);
    assert_eq!(kept_title(&w, "books/a.epub"), None);
    assert_eq!(kept_title(&w, "books/b.epub").as_deref(), Some("Dune"));
}

/// Runs what is due within a minute and more, as time goes on.
fn run(kernel: &mut crate::Kernel, mut now: u64, mut effects: crate::Effects) -> u64 {
    for _ in 0..2000 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 61_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    now
}

#[test]
fn the_walk_after_the_job_makes_the_books_line_from_what_the_format_named() {
    use paperpaw_sdk::event::EventData;
    let mut w = world(&["books/a.epub"]);
    let effects = w.kernel.start(Instant(0));
    let mut now = run(&mut w.kernel, 0, effects);
    // A view asks for the catalog: the walk lists the book by its file.
    let ask = events::CatalogWanted::new(false).to_params();
    let effects = w
        .kernel
        .send_event(Instant(now), events::CATALOG_WANTED.id().clone(), ask);
    now = run(&mut w.kernel, now, effects);
    let line = || {
        let mut found = None;
        catalog::each(&*w.card, &mut |record| found = Some(record));
        found.unwrap()
    };
    // The passes that come anyway: the job reads the book, wants the catalog.
    for _ in 0..4 {
        now += 1_000;
        let effects = w.kernel.tick(Instant(now));
        now = run(&mut w.kernel, now, effects);
    }
    assert_eq!(w.format.opens(), 1, "read once");
    let record = line();
    assert_eq!(record.path, "books/a.epub");
    assert_eq!(record.title, "Dune");
    assert_eq!(record.author, "Author");
    assert_eq!(record.size, Some(6), "its details are read");
    assert!(!w.docs.details_job().unwrap().run().armed());
}

#[test]
fn a_preparing_book_is_converted_by_the_pumps_preparing_loop_then_read() {
    let mut w = world(&["books/a.docx"]);
    catalog(&w, &[("books/a.docx", false)]);
    started(&mut w);
    w.format.open.set(PREPARING);
    w.kernel.native_details_arm();
    pump(&mut w, 1_000);
    assert_eq!(*w.format.ops.borrow(), [2], "opened, preparing");
    assert_eq!(kept_title(&w, "books/a.docx"), None);
    // The next pass: a preparing step (the format is done), then the read.
    pump(&mut w, 2_000);
    assert_eq!(*w.format.ops.borrow(), [2, 3, 5, 6, 4]);
    assert_eq!(kept_title(&w, "books/a.docx").as_deref(), Some("Dune"));
}

#[test]
fn the_job_goes_on_by_timed_wakes_holds_no_timer_off_and_is_slower_when_saving() {
    let mut w = world(&["books/a.docx", "books/b.epub", "books/c.epub"]);
    catalog(
        &w,
        &[
            ("books/a.docx", false),
            ("books/b.epub", false),
            ("books/c.epub", false),
        ],
    );
    started(&mut w);
    w.format.open.set(PREPARING);
    w.kernel.native_details_arm();
    let asleep_at = w.kernel.sleep_due();
    pump(&mut w, 1_000);
    assert_eq!(*w.format.ops.borrow(), [2], "opened, preparing");
    // No "more": a timed wake for its first step.
    assert!(!w.kernel.shared.native_instances.service_more());
    assert_eq!(w.kernel.service_due(), Some(Instant(1_100)));
    // A pass for another reason before then: the step and the read; the
    // next book waits for the wake.
    w.format.open.set(READY);
    pump(&mut w, 1_050);
    assert_eq!(*w.format.ops.borrow(), [2, 3, 5, 6, 4]);
    pump(&mut w, 1_060);
    assert_eq!(w.format.opens(), 1);
    assert_eq!(w.kernel.service_due(), Some(Instant(1_100)));
    // At the wake: the next book, read at once (ready).
    pump(&mut w, 1_100);
    assert_eq!(*w.format.ops.borrow(), [2, 3, 5, 6, 4, 2, 5, 6, 4]);
    assert_eq!(
        w.kernel.service_due(),
        Some(Instant(1_200)),
        "the next book"
    );
    // Saving: the gap is a second.
    w.kernel.shared.power.level = paperpaw_sdk::power::PowerLevel::Saving;
    pump(&mut w, 1_200);
    assert_eq!(w.format.opens(), 3);
    assert_eq!(w.kernel.service_due(), Some(Instant(2_200)));
    // The catalog's end: wanted once, and no wake is left.
    pump(&mut w, 2_200);
    assert_eq!(wanted(&w), 1);
    assert_eq!(w.kernel.service_due(), None);
    // The job is no activity: the sleep is due when it was.
    assert_eq!(w.kernel.sleep_due(), asleep_at);
    assert!(!w.kernel.busy_at_sleep(Instant(2_200)));
}

#[test]
fn a_displaced_job_waits_the_quiet_moment_by_a_timed_wake() {
    use crate::native_host::doc_service::JobKind;
    let mut w = world(&["books/a.epub"]);
    catalog(&w, &[("books/a.epub", false)]);
    started(&mut w);
    w.format.open.set(PREPARING);
    w.kernel.native_details_arm();
    pump(&mut w, 1_000);
    assert!(w.docs.cover_job_active());
    // A reader's open took its place (`doc_service::taken`).
    w.docs.set_now(1_000);
    assert!(w.docs.table().take_cover_job().is_some());
    w.docs.cover_job_displaced(JobKind::Details);
    w.format.open.set(READY);
    pump(&mut w, 1_500);
    assert_eq!(w.format.opens(), 1, "the quiet moment");
    assert_eq!(w.kernel.service_due(), Some(Instant(3_000)));
    assert_eq!(w.docs.cover_job_given_up(), 0, "not given up");
    pump(&mut w, 3_000);
    assert_eq!(w.format.opens(), 2);
    assert_eq!(kept_title(&w, "books/a.epub").as_deref(), Some("Dune"));
}

#[test]
fn no_book_begins_within_the_quiet_time_after_the_start_or_an_input() {
    let mut w = world(&["books/a.epub", "books/b.epub"]);
    catalog(&w, &[("books/a.epub", false), ("books/b.epub", false)]);
    // The start at 0 (`Kernel::start` sets the last activity).
    started(&mut w);
    w.kernel.native_details_arm();
    pump(&mut w, QUIET_MS / 2);
    assert_eq!(w.format.opens(), 0, "beside the start's peak");
    assert_eq!(w.kernel.service_due(), Some(Instant(QUIET_MS)));
    pump(&mut w, QUIET_MS);
    assert_eq!(w.format.opens(), 1);
    // An input (`Kernel::handle`): the next book waits again.
    w.kernel.last_activity = Instant(QUIET_MS + 50);
    pump(&mut w, QUIET_MS + 200);
    assert_eq!(w.format.opens(), 1, "beside the user");
    assert_eq!(w.kernel.service_due(), Some(Instant(2 * QUIET_MS + 50)));
    pump(&mut w, 2 * QUIET_MS + 50);
    assert_eq!(w.format.opens(), 2);
    assert_eq!(kept_title(&w, "books/b.epub").as_deref(), Some("Dune"));
}

#[test]
fn no_book_begins_while_a_first_walk_is_under_way() {
    let mut w = world(&["books/a.epub"]);
    catalog(&w, &[("books/a.epub", false)]);
    // A first walk published the books it found so far and goes on
    // (`catalog::is_growing`: its new catalog and its first-walk mark).
    let building = alloc::format!("{}/catalog.tmp", library::DIR);
    let first = alloc::format!("{}/walk.first", library::DIR);
    w.card.write(&building, b"").unwrap();
    w.card.write(&first, b"").unwrap();
    assert!(catalog::is_growing(&*w.card));
    started(&mut w);
    w.kernel.native_details_arm();
    pump(&mut w, 2_000);
    assert_eq!(w.format.opens(), 0, "beside the walk");
    assert_eq!(w.kernel.service_due(), Some(Instant(2_000 + QUIET_MS)));
    // The walk ended.
    w.card.remove(&building).unwrap();
    w.card.remove(&first).unwrap();
    // A later kernel call: what the last one read is read again (`recall.rs`).
    w.kernel.shared.recall.clear();
    pump(&mut w, 2_000 + QUIET_MS);
    assert_eq!(w.format.opens(), 1);
    assert_eq!(kept_title(&w, "books/a.epub").as_deref(), Some("Dune"));
}

#[test]
fn marks_of_a_walk_that_does_not_go_on_are_looked_at_ever_less_often() {
    let mut w = world(&["books/a.epub"]);
    catalog(&w, &[("books/a.epub", false)]);
    // A first walk cut short (the power gone): its marks stay until a view
    // of the library wants the catalog.
    let building = alloc::format!("{}/catalog.tmp", library::DIR);
    let first = alloc::format!("{}/walk.first", library::DIR);
    w.card.write(&building, b"").unwrap();
    w.card.write(&first, b"").unwrap();
    started(&mut w);
    w.kernel.native_details_arm();
    let mut now = 2_000;
    for wait in [1_000, 2_000, 4_000, 8_000, 16_000, 32_000, 32_000] {
        pump(&mut w, now);
        assert_eq!(w.kernel.service_due(), Some(Instant(now + wait)));
        now += wait;
    }
    assert_eq!(w.format.opens(), 0);
    // The walk went on and ended: the book at the next look, and a later
    // walk waits from a second again.
    w.card.remove(&building).unwrap();
    w.card.remove(&first).unwrap();
    w.kernel.shared.recall.clear();
    pump(&mut w, now);
    assert_eq!(w.format.opens(), 1);
    let run = w.docs.details_job().unwrap().run();
    assert_eq!(run.hold_waits(QUIET_MS, HOLD_WAIT_MAX_MS), QUIET_MS);
}

#[test]
fn a_screen_other_than_homes_or_the_librarys_holds_the_job_until_it_goes() {
    let mut w = world(&["books/a.epub"]);
    catalog(&w, &[("books/a.epub", false)]);
    started(&mut w);
    w.kernel.native_details_arm();
    // A page of a plugin's own on top (the store's), opened by its id.
    w.kernel.open(PAGE, None, &Params::default());
    pump(&mut w, 2_000);
    assert_eq!(w.format.opens(), 0, "a plugin's page");
    assert_eq!(
        w.kernel.native_details_due(),
        None,
        "no timed wake: input takes it away"
    );
    // Home's screen opened for another slot (an overlay's, say) holds too.
    w.kernel
        .open(HOME_SCREEN, Some(slots::MENU), &Params::default());
    pump(&mut w, 3_000);
    assert_eq!(w.format.opens(), 0, "opened for another slot");
    // Home's screen opened by its id: it is offered for `core.home`.
    w.kernel.open(HOME_SCREEN, None, &Params::default());
    pump(&mut w, 4_000);
    assert_eq!(w.format.opens(), 1);
    assert_eq!(kept_title(&w, "books/a.epub").as_deref(), Some("Dune"));
}

#[test]
fn a_screen_opened_for_the_library_lets_it_run_and_no_screen_holds_it() {
    let mut w = world(&["books/a.epub", "books/b.epub"]);
    catalog(&w, &[("books/a.epub", false), ("books/b.epub", false)]);
    started(&mut w);
    w.kernel.native_details_arm();
    // Nothing on the stack (before the first screen, say): held.
    w.kernel.stack.clear();
    pump(&mut w, 2_000);
    assert_eq!(w.format.opens(), 0, "no screen");
    // A screen opened for slot `core.library` (it need not be offered for it).
    w.kernel
        .open(PAGE, Some(slots::LIBRARY), &Params::default());
    pump(&mut w, 3_000);
    assert_eq!(w.format.opens(), 1, "the library's screen");
}

#[test]
fn network_work_holds_the_job_until_it_ends() {
    let mut w = world(&["books/a.epub"]);
    catalog(&w, &[("books/a.epub", false)]);
    started(&mut w);
    w.kernel.native_details_arm();
    let reply = Id::new_static("shelf.fetched");
    let mut out = Vec::new();
    let request = paperpaw_sdk::HttpRequest::get("https://example.org/", reply.clone());
    let storage = w.kernel.shared.storage.clone();
    w.kernel
        .io
        .http
        .request(request, true, false, &*storage, &mut out);
    assert!(w.kernel.details_busy());
    let mut now = 2_000;
    for wait in [1_000, 2_000, 4_000] {
        pump(&mut w, now);
        assert_eq!(w.format.opens(), 0, "a request waits");
        assert_eq!(w.kernel.native_details_due(), Some(now + wait));
        now += wait;
    }
    w.kernel.io.http.cancel(&reply, &mut out);
    assert!(!w.kernel.details_busy());
    pump(&mut w, now);
    assert_eq!(w.format.opens(), 1, "at the next timed look");
    // The next hold waits from a second again.
    let run = w.docs.details_job().unwrap().run();
    assert_eq!(run.hold_waits(QUIET_MS, HOLD_WAIT_MAX_MS), QUIET_MS);
}

#[test]
fn a_job_of_the_install_queue_holds_the_job_until_it_ends() {
    use crate::native_host::firmware_driver::Driver;
    use paperpaw_platform::firmware::WRITE_BLOCK;
    use paperpaw_platform::firmware_fake::{FakeFirmware, paperpaw_facts};
    let mut w = world(&["books/a.epub"]);
    catalog(&w, &[("books/a.epub", false)]);
    started(&mut w);
    w.kernel.native_details_arm();
    let fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    let driver = Rc::new(RefCell::new(Driver::new(
        alloc::boxed::Box::new(fake),
        Rc::new(RefCell::new(
            crate::native_host::firmware_install::FirmwareInstall::new(),
        )),
    )));
    driver
        .borrow_mut()
        .submit_check(9, 1, "update.bin")
        .unwrap();
    w.kernel.firmware = Some(driver.clone());
    // The details job's own pump alone: the queue's pump would run the check.
    w.kernel.now = Instant(2_000);
    w.kernel.native_details_job_pump();
    assert_eq!(w.format.opens(), 0, "a check waits in the queue");
    assert_eq!(w.kernel.native_details_due(), Some(3_000));
    // A queue its own pump holds borrowed counts as busy too.
    {
        let _held = driver.borrow_mut();
        w.kernel.now = Instant(3_000);
        w.kernel.native_details_job_pump();
        assert_eq!(w.format.opens(), 0, "the queue is borrowed");
        assert_eq!(w.kernel.native_details_due(), Some(5_000));
    }
    driver.borrow_mut().cancel(9, 1);
    w.kernel.shared.recall.clear();
    w.kernel.now = Instant(5_000);
    w.kernel.native_details_job_pump();
    assert_eq!(w.format.opens(), 1, "the queue is empty");
}

#[test]
fn a_device_left_on_home_reads_every_book_through_holds() {
    // The starvation rule: holds only defer; left alone on home, the job
    // reaches the catalog's end.
    let books = ["books/a.epub", "books/b.epub"];
    let mut w = world(&books);
    catalog(&w, &books.map(|book| (book, false)));
    started(&mut w);
    w.kernel.native_details_arm();
    w.kernel.open(PAGE, None, &Params::default());
    pump(&mut w, 2_000);
    assert_eq!(w.format.opens(), 0);
    // Back home (an input), then left alone.
    w.kernel.close_top(paperpaw_sdk::CloseCause::Back);
    w.kernel.last_activity = Instant(2_500);
    let mut now = 3_000;
    while now < 20_000 && w.docs.details_job().unwrap().run().armed() {
        pump(&mut w, now);
        now += 1_000;
    }
    assert_eq!(w.format.opens(), 2);
    assert!(
        books
            .iter()
            .all(|book| kept_title(&w, book).as_deref() == Some("Dune"))
    );
}
