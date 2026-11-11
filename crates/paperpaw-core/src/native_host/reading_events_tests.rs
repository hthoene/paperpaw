//! The kernel follows a native reader's session as it followed the
//! interface-6 reader's events (#2449): the books opened last, how far a book
//! is read and finished, with the writes Beta 3 made and no more; each event
//! told to a native plugin once.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    commands::CommandStage,
    doc_reading::{Facts, Reading},
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
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

struct World {
    kernel: crate::Kernel,
    card: Card,
    told: Rc<RefCell<Vec<u32>>>,
    reading: Reading,
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
    World {
        kernel,
        card,
        told,
        reading: Reading::new(),
    }
}

impl World {
    /// The reader on screen reports the book `path` (document `document`) at
    /// `offset` and `ppm`, read to the end where `finished`; then the pump.
    fn report(&mut self, document: u32, path: &str, offset: u32, ppm: u32, finished: bool) {
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
        self.reading.report(1_000, &facts, &report).unwrap();
        self.pump();
    }

    /// The session's events, told as the kernel's pump tells them.
    fn pump(&mut self) {
        while let Some(edge) = self.reading.take_edge() {
            self.kernel.native_reading_tell(&edge);
        }
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
    w.reading.document_ended(2_000, 1);
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
    w.reading.document_ended(2_000, 1);
    w.report(2, BOB, 0, 0, false);
    w.reading.document_ended(3_000, 2);
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
    w.reading.sleep(2_000);
    w.pump();
    w.report(1, ALICE, 1, 50_000, false);
    assert_eq!(w.writes(), writes);
    assert_eq!(*w.told.borrow(), [OPENED, TURNED, CLOSED, OPENED]);
    assert_eq!(library::opened_books(&*w.card.storage), [ALICE]);
}
