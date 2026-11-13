//! The kernel follows a native reader's session as it followed the
//! interface-6 reader's events (#2449): the books opened last, how far a book
//! is read and finished, with the writes Beta 3 made and no more; each event
//! told to a native plugin once.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    commands::CommandStage,
    doc_reading::{Edge, Facts, Reading},
    doc_service::{
        DocHost, Document, DocumentTable, FormatCall, FormatRef, Limits, Options, tests::Fake,
    },
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::{Id, MemoryStorage, library};

const ALICE: &str = "books/alice.epub";
const BOB: &str = "books/bob.epub";
const OPENED: u32 = wire::EVENT_BOOK_OPENED_TAG;
const TURNED: u32 = wire::EVENT_PAGE_TURNED_TAG;
const FINISHED: u32 = wire::EVENT_BOOK_FINISHED_TAG;
const CLOSED: u32 = wire::EVENT_BOOK_CLOSED_TAG;

/// A card that notes each path written.
#[derive(Clone)]
struct Card {
    storage: Rc<MemoryStorage>,
    written: Rc<RefCell<Vec<String>>>,
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.storage.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.storage.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.written.borrow_mut().push(path.into());
        self.storage.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.written.borrow_mut().push(path.into());
        self.storage.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.storage.list(dir, visit)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.storage.list_from(dir, from, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.storage.remove(path)
    }
}

/// A native plugin that notes the tags of the reading events it is told.
struct Probe(Rc<RefCell<Vec<u32>>>);

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        [OPENED, TURNED, FINISHED, CLOSED].contains(&tag)
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let tag = wire::RawRecord::single(bytes)
            .map_err(|_| ERR_INVALID)?
            .tag();
        self.0.borrow_mut().push(tag);
        Ok(OK)
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        _: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.service_event(bytes, env)
    }
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
}

/// The document service as the kernel's pump sees it: a real reading session
/// (`doc_reading::Reading`) and, for everything else, the service's fake.
struct Session {
    fake: Rc<Fake>,
    reading: Reading,
}

impl DocHost for Session {
    fn as_dyn(&self) -> &dyn DocHost {
        self
    }
    fn table(&self) -> &DocumentTable {
        self.fake.table()
    }
    fn screen_long_side(&self) -> u32 {
        self.fake.screen_long_side()
    }
    fn root_access(&self, root: u32) -> Result<(), i32> {
        self.fake.root_access(root)
    }
    fn select_format(&self, root: u32, path: &str, requested: &str) -> Result<FormatRef, i32> {
        self.fake.select_format(root, path, requested)
    }
    fn format_id(&self, format: FormatRef, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        self.fake.format_id(format, visit)
    }
    fn conversion_key(
        &self,
        open: &wire::ServiceDocOpen<'_>,
        format: FormatRef,
        options: Options<'_>,
    ) -> Result<u64, i32> {
        self.fake.conversion_key(open, format, options)
    }
    fn call_format(&self, call: &FormatCall<'_>, out: &mut [u8]) -> Result<usize, i32> {
        self.fake.call_format(call, out)
    }
    fn close_format(&self, document: &Document) {
        self.fake.close_format(document);
    }
    fn end_images(&self, document: &Document) {
        self.fake.end_images(document);
    }
    fn document_closed(&self, document: &Document, reason: u32) {
        self.fake.document_closed(document, reason);
    }
    fn map_image(
        &self,
        document: &Document,
        image: &wire::ServiceDocImage,
        in_block: bool,
    ) -> Result<Option<u32>, i32> {
        self.fake.map_image(document, image, in_block)
    }
    fn formats_of_owner(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
        self.fake.formats_of_owner(owner, visit);
    }
    fn formats_owned(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
        self.fake.formats_owned(owner, visit);
    }
    fn formats(
        &self,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(wire::Record<'_>) -> ControlFlow<()>,
    ) -> Result<crate::abi7::context::RowsEnd, i32> {
        self.fake.formats(cursor, generation, visit)
    }
    fn readers(
        &self,
        scope: &str,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(wire::Record<'_>) -> ControlFlow<()>,
    ) -> Result<crate::abi7::context::RowsEnd, i32> {
        self.fake.readers(scope, cursor, generation, visit)
    }
    fn format_for(
        &self,
        root: u32,
        path: &str,
        sniff: bool,
        visit: &mut dyn FnMut(wire::Record<'_>),
    ) -> Result<(), i32> {
        self.fake.format_for(root, path, sniff, visit)
    }
    fn take_reading_edge(&self) -> Option<Edge> {
        self.reading.take_edge()
    }
    fn reading_sleep(&self, now_ms: u64) {
        self.reading.sleep(now_ms);
    }
}

struct World {
    kernel: crate::Kernel,
    card: Card,
    told: Rc<RefCell<Vec<u32>>>,
    session: Rc<Session>,
}

fn world() -> World {
    let card = Card {
        storage: Rc::new(
            MemoryStorage::new()
                .with(ALICE, alloc::vec![1_u8; 100])
                .with(BOB, alloc::vec![2_u8; 100]),
        ),
        written: Rc::default(),
    };
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(card.clone())
        .build()
        .unwrap();
    let told = Rc::new(RefCell::new(Vec::new()));
    kernel
        .shared
        .native_instances
        .test_insert(
            Id::new_static("probe"),
            Some(NativeLease::try_new(Probe(Rc::clone(&told))).unwrap()),
            true,
        )
        .unwrap();
    let session = Rc::new(Session {
        fake: Fake::new(Limits::LARGE),
        reading: Reading::new(),
    });
    let host: Rc<dyn DocHost> = session.clone();
    kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_documents(host)
        .unwrap();
    World {
        kernel,
        card,
        told,
        session,
    }
}

impl World {
    /// The reader on screen reports the book `path` (document `document`) at
    /// `offset` and `ppm`, read to the end where `finished`; then the pump.
    fn report(&mut self, document: u32, path: &str, offset: u32, ppm: u32, finished: bool) {
        self.reported(document, path, offset, ppm, finished);
        self.pump();
    }

    /// The same, the events left waiting for the pump.
    fn reported(&self, document: u32, path: &str, offset: u32, ppm: u32, finished: bool) {
        let report = wire::ServiceDocReport {
            document,
            section: 0,
            offset,
            insert: 0,
            progress_ppm: ppm,
            flags: u32::from(finished),
            title: "Title",
            chapter: "One",
            page: None,
            pages: None,
            chapter_left: None,
        };
        let facts = Facts {
            document,
            counts: true,
            path,
        };
        self.session.reading.report(1_000, &facts, &report).unwrap();
    }

    /// The kernel's pump takes the session's events and tells them.
    fn pump(&mut self) {
        while self.kernel.native_reading_pump() {}
        while self.kernel.native_setting_pump() {}
        self.kernel.run_commands();
    }

    fn writes(&self) -> usize {
        self.card.written.borrow().len()
    }

    fn progress(&self, path: &str) -> Option<u8> {
        library::book(&*self.card.storage, path).and_then(|book| book.progress)
    }
}

#[test]
fn a_native_reading_session_keeps_the_books_opened_the_progress_and_the_end() {
    let mut w = world();
    w.report(1, ALICE, 0, 0, false);
    assert_eq!(library::opened_books(&*w.card.storage), [ALICE]);
    assert_eq!(w.progress(ALICE), Some(0));
    // 12.5 % as the interface-6 reader gave it (one decimal), shown as 13.
    w.report(1, ALICE, 10, 125_000, false);
    assert_eq!(w.progress(ALICE), Some(13));
    // The last page: read to the end.
    w.report(1, ALICE, 90, 1_000_000, true);
    assert_eq!(w.progress(ALICE), Some(100));
    w.session.reading.document_ended(2_000, 1);
    w.pump();
    assert_eq!(w.progress(ALICE), Some(100));
    assert_eq!(library::opened_books(&*w.card.storage), [ALICE]);
    // The native plugin heard each event once, in order.
    assert_eq!(*w.told.borrow(), [OPENED, TURNED, TURNED, FINISHED, CLOSED]);
}

#[test]
fn a_book_opened_again_is_listed_once_and_the_last_opened_first() {
    let mut w = world();
    w.report(1, ALICE, 0, 0, false);
    w.session.reading.document_ended(2_000, 1);
    w.report(2, BOB, 0, 0, false);
    w.session.reading.document_ended(3_000, 2);
    w.report(3, ALICE, 0, 0, false);
    assert_eq!(library::opened_books(&*w.card.storage), [ALICE, BOB]);
    w.report(4, BOB, 0, 0, false);
    assert_eq!(library::opened_books(&*w.card.storage), [BOB, ALICE]);
}

#[test]
fn pages_turned_within_one_percent_write_nothing() {
    let mut w = world();
    w.report(1, ALICE, 0, 0, false);
    w.report(1, ALICE, 1, 20_000, false);
    let writes = w.writes();
    // Five more pages, all at 2 %: the library is not written.
    for page in 2..7 {
        w.report(1, ALICE, page, 20_000 + page * 100, false);
    }
    assert_eq!(w.writes(), writes);
    assert_eq!(w.told.borrow().len(), 7);
    // The next percent is written once.
    w.report(1, ALICE, 7, 30_000, false);
    assert_eq!(w.writes(), writes + 1);
    assert_eq!(w.progress(ALICE), Some(3));
}

#[test]
fn a_book_closed_for_sleep_and_opened_after_it_writes_nothing() {
    let mut w = world();
    w.report(1, ALICE, 0, 0, false);
    w.report(1, ALICE, 1, 50_000, false);
    let writes = w.writes();
    // Sleep closes the book (reason 1); the reader's next report opens it
    // again (reason 1): the native plugin hears both, the library nothing.
    w.session.reading.sleep(2_000);
    w.pump();
    w.report(1, ALICE, 1, 50_000, false);
    assert_eq!(w.writes(), writes);
    assert_eq!(*w.told.borrow(), [OPENED, TURNED, CLOSED, OPENED]);
    assert_eq!(library::opened_books(&*w.card.storage), [ALICE]);
}

#[test]
fn a_page_turned_still_waiting_when_the_device_sleeps_is_kept_once() {
    let mut w = world();
    w.report(1, ALICE, 0, 0, false);
    // The page turn waits for the pump when the sleep comes: the sleep's own
    // `emit_from` raises it one call deeper, then closes the book (reason 1).
    w.reported(1, ALICE, 4, 70_000, false);
    w.kernel.emit(
        &paperpaw_sdk::builtin::events::SUSPEND.id().clone(),
        &paperpaw_sdk::Params::new(),
    );
    while w.kernel.native_setting_pump() {}
    w.kernel.run_commands();
    assert_eq!(w.progress(ALICE), Some(7));
    assert_eq!(*w.told.borrow(), [OPENED, TURNED, CLOSED]);
    assert!(w.session.reading.take_edge().is_none());
}

#[test]
fn the_percentage_has_one_decimal_as_beta_3_gave_it() {
    use super::percent;
    // The same value, bit for bit: the one decimal is exact as Beta 3 made it.
    let is = |ppm: u32, want: f32| assert_eq!(percent(ppm).to_bits(), want.to_bits(), "{ppm}");
    is(0, 0.0);
    is(124_999, 12.5);
    is(999_499, 99.9);
    // 99.95 % and over is 100, as Beta 3's one decimal made it.
    is(999_500, 100.0);
    is(1_000_000, 100.0);
    is(u32::MAX, 100.0);
}
