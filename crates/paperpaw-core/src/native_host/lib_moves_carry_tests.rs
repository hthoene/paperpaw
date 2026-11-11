#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! The catalog side of a library move: books carried a step at a time, old
//! keys gone, replays idempotent, events, folder settings, discard.
extern crate std;
use super::*;
use alloc::{borrow::Cow, vec::Vec};
use core::cell::Cell;
use paperpaw_platform::{DirEntry, StorageError};
use paperpaw_sdk::{
    FormatSpec, Id, MemoryStorage, SettingExtra, SettingSpec,
    builtin::groups,
    document::DocumentError,
    library::{BookDataKind, key},
};
use std::string::ToString;

/// A library with `.epub` as a format, one folder of plugin book data
/// (`pos`), and the settings the test wants.
struct Fixture {
    storage: MemoryStorage,
    formats: Formats,
    data: Vec<BookData>,
    settings: Settings,
    resume: Resume,
}

fn fixture() -> Fixture {
    let mut formats = Formats::new();
    formats
        .add(FormatSpec::new(
            Id::new_static("test.epub"),
            "EPUB",
            alloc::vec![Cow::Borrowed("epub")],
            alloc::boxed::Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
        .unwrap();
    Fixture {
        storage: MemoryStorage::new(),
        formats,
        data: alloc::vec![BookData::new(
            ".paperpaw/plugins/x/pos",
            "",
            BookDataKind::UserData
        )],
        settings: Settings::new(),
        resume: Resume::default(),
    }
}

impl Fixture {
    /// A book at `path` with a place kept for it by plugin x.
    fn book(&self, path: &str) {
        self.storage.write(path, b"book").unwrap();
    }

    fn place(&self, path: &str) {
        self.storage
            .write(
                &std::format!(".paperpaw/plugins/x/pos/{}.dat", key(path)),
                b"42",
            )
            .unwrap();
    }

    fn has_place(&self, path: &str) -> bool {
        self.storage
            .exists(&std::format!(".paperpaw/plugins/x/pos/{}.dat", key(path)))
    }

    /// Runs the job's catalog side to its end; the commands sent and the
    /// number of steps.
    fn run(&self, from: &str, to: &str) -> (Vec<Command>, usize) {
        self.run_from(from, to, Phase::Books(0))
    }

    fn run_from(&self, from: &str, to: &str, start: Phase) -> (Vec<Command>, usize) {
        let mut sent = Vec::new();
        let mut steps = 0;
        {
            let mut out = |command| sent.push(command);
            let carry = EnvCarry::from_parts(
                &self.storage,
                &self.formats,
                &self.data,
                &self.settings,
                &self.resume,
                &mut out,
            );
            let mut phase = start;
            while phase != Phase::Done {
                phase = carry.carry(from, to, phase).unwrap();
                steps += 1;
                assert!(steps < 1000, "the job ends");
            }
        }
        (sent, steps)
    }
}

fn moved_events(sent: &[Command]) -> Vec<std::string::String> {
    sent.iter()
        .map(|c| std::format!("{c:?}"))
        .filter(|c| c.contains("book-moved"))
        .collect()
}

#[test]
fn every_book_below_goes_to_its_new_key_with_its_data_and_the_old_key_is_gone() {
    let f = fixture();
    f.book("Books/new/a.epub");
    f.book("Books/new/sub/b.epub");
    f.book("Books/new/notes.txt");
    f.place("Books/old/a.epub");
    f.place("Books/old/sub/b.epub");
    f.place("Books/old/notes.txt");
    library::set_favourite(&f.storage, "Books/old/a.epub", true);
    let (sent, _) = f.run("Books/old", "Books/new");
    assert!(f.has_place("Books/new/a.epub") && !f.has_place("Books/old/a.epub"));
    assert!(f.has_place("Books/new/sub/b.epub") && !f.has_place("Books/old/sub/b.epub"));
    // Not a book of any format: not carried.
    assert!(f.has_place("Books/old/notes.txt") && !f.has_place("Books/new/notes.txt"));
    assert_eq!(library::favourites(&f.storage), ["Books/new/a.epub"]);
    // One event per book, then the library changed, once.
    let events = moved_events(&sent);
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(
        events
            .iter()
            .any(|e| e.contains("Books/old/a.epub") && e.contains("Books/new/a.epub"))
    );
    assert!(events.iter().any(|e| e.contains("Books/old/sub/b.epub")));
    assert_eq!(
        sent.iter()
            .filter(|c| std::format!("{c:?}").contains("library-changed"))
            .count(),
        1
    );
}

#[test]
fn a_big_folder_takes_several_bounded_steps_and_a_replay_changes_nothing() {
    let f = fixture();
    let books = STEP_BOOKS * 2 + 3;
    for n in 0..books {
        f.book(&std::format!("Books/new/b{n:03}.epub"));
        f.place(&std::format!("Books/old/b{n:03}.epub"));
    }
    let (sent, steps) = f.run("Books/old", "Books/new");
    // 3 book steps, assignments, settings, 3 event steps (events: 2 full + the rest).
    assert!(steps >= 3 + 2, "{steps}");
    assert_eq!(moved_events(&sent).len(), books);
    for n in 0..books {
        assert!(f.has_place(&std::format!("Books/new/b{n:03}.epub")));
    }
    // Carried a second time: nothing under the old key, nothing replaced.
    f.storage
        .write(
            &std::format!(".paperpaw/plugins/x/pos/{}.dat", key("Books/new/b000.epub")),
            b"read since",
        )
        .unwrap();
    f.run_from("Books/old", "Books/new", Phase::Books(0));
    let mut kept = std::vec![0u8; 10];
    let n = f
        .storage
        .read_at(
            &std::format!(".paperpaw/plugins/x/pos/{}.dat", key("Books/new/b000.epub")),
            0,
            &mut kept,
        )
        .unwrap();
    assert_eq!(&kept[..n], b"read since");
}

#[test]
fn one_book_carried_for_a_reader_and_the_walk_after_it_agree() {
    let f = fixture();
    f.book("Books/new/a.epub");
    f.place("Books/old/a.epub");
    let mut sent = Vec::new();
    let mut out = |c| sent.push(c);
    let carry = EnvCarry::from_parts(
        &f.storage,
        &f.formats,
        &f.data,
        &f.settings,
        &f.resume,
        &mut out,
    );
    carry
        .carry_book("Books/old", "Books/new", "Books/new/a.epub")
        .unwrap();
    assert!(f.has_place("Books/new/a.epub") && !f.has_place("Books/old/a.epub"));
    // The walk meets it again: no-op.
    carry
        .carry("Books/old", "Books/new", Phase::Books(0))
        .unwrap();
    assert!(f.has_place("Books/new/a.epub"));
}

#[test]
fn a_file_moved_is_one_book_whatever_its_name_now_and_its_bak_goes_with_it() {
    let f = fixture();
    // `x.epub` renamed `x.epub.bak`: not a format any more, still the book.
    f.book("Books/x.epub.bak");
    f.place("Books/x.epub");
    let (sent, _) = f.run("Books/x.epub", "Books/x.epub.bak");
    assert!(f.has_place("Books/x.epub.bak") && !f.has_place("Books/x.epub"));
    assert_eq!(moved_events(&sent).len(), 1);
    // A book with a `.bak` sibling: both go.
    let f = fixture();
    f.book("Books/new/y.epub");
    f.book("Books/new/y.epub.bak");
    f.place("Books/old/y.epub");
    f.place("Books/old/y.epub.bak");
    f.run("Books/old", "Books/new");
    assert!(f.has_place("Books/new/y.epub") && f.has_place("Books/new/y.epub.bak"));
    assert!(!f.has_place("Books/old/y.epub") && !f.has_place("Books/old/y.epub.bak"));
}

#[test]
fn the_replaced_files_data_is_discarded() {
    let f = fixture();
    f.book("Books/b.epub");
    f.place("Books/b.epub");
    library::set_favourite(&f.storage, "Books/b.epub", true);
    let mut out = |_| ();
    let carry = EnvCarry::from_parts(
        &f.storage,
        &f.formats,
        &f.data,
        &f.settings,
        &f.resume,
        &mut out,
    );
    assert!(carry.discard("Books/b.epub").unwrap());
    assert!(!f.has_place("Books/b.epub"));
    assert!(library::favourites(&f.storage).is_empty());
}

#[test]
fn the_discard_of_a_book_with_much_data_is_progress_until_all_is_gone() {
    let f = fixture();
    f.book("Books/b.epub");
    let key = library::key("Books/b.epub");
    for n in 0..(STEP_PIECES as usize * 3) {
        f.storage
            .write(&format!(".paperpaw/plugins/x/pos/{key}.p{n}"), b"x")
            .unwrap();
    }
    let mut out = |_| ();
    let carry = EnvCarry::from_parts(
        &f.storage,
        &f.formats,
        &f.data,
        &f.settings,
        &f.resume,
        &mut out,
    );
    // Not a refusal: `false` is "run the step again", and it ends.
    let mut calls = 0;
    while !carry.discard("Books/b.epub").unwrap() {
        calls += 1;
        assert!(calls < 10, "never ends");
    }
    assert_eq!(calls, 2);
    let mut left = 0;
    let _ = f
        .storage
        .list(".paperpaw/plugins/x/pos", &mut |_| left += 1);
    assert_eq!(left, 0);
}

/// A card that reads but refuses every change (`Io`): the power is flaky.
struct Refusing<'a>(&'a MemoryStorage);

impl Storage for Refusing<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.0.size(p)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(d, v)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn rename(&self, _: &str, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn create_dir(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
    fn remove_dir_all(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
}

/// A refused write while a book is carried is the step's status, not "done":
/// the phase does not move on, and the repeat on a card that works finishes
/// (the favourites and the data are at the new key).
#[test]
fn a_card_that_refuses_a_write_fails_the_step_and_the_repeat_finishes_it() {
    let f = fixture();
    f.book("Books/new/a.epub");
    f.place("Books/old/a.epub");
    library::set_favourite(&f.storage, "Books/old/a.epub", true);
    let refusing = Refusing(&f.storage);
    let mut out = |_| ();
    let carry = EnvCarry::from_parts(
        &refusing,
        &f.formats,
        &f.data,
        &f.settings,
        &f.resume,
        &mut out,
    );
    assert_eq!(
        carry.carry("Books/old", "Books/new", Phase::Books(0)),
        Err(super::super::storage::status(StorageError::Io))
    );
    assert!(f.has_place("Books/old/a.epub") && !f.has_place("Books/new/a.epub"));
    assert_eq!(library::favourites(&f.storage), ["Books/old/a.epub"]);
    f.run("Books/old", "Books/new");
    assert!(f.has_place("Books/new/a.epub") && !f.has_place("Books/old/a.epub"));
    assert_eq!(library::favourites(&f.storage), ["Books/new/a.epub"]);
}

/// Counts the entries a step removes.
struct Removals<'a> {
    card: &'a MemoryStorage,
    removed: Cell<usize>,
}

impl Storage for Removals<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.removed.set(self.removed.get() + 1);
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.card.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.card.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.removed.set(self.removed.get() + 1);
        self.card.remove_dir_all(p)
    }
}

/// The host's document cache of a moved book goes with its old key, a bounded
/// piece at a time, one budget for the whole step whatever the number of books:
/// a folder of books with big caches takes several steps, none of which removes
/// more than the budget, and nothing of another book's cache is touched.
#[test]
fn the_document_caches_of_a_moved_folder_are_removed_over_steps_within_one_budget() {
    let f = fixture();
    let books = 20;
    for n in 0..books {
        f.book(&std::format!("Books/new/b{n:02}.epub"));
        f.place(&std::format!("Books/old/b{n:02}.epub"));
        let old = key(&std::format!("Books/old/b{n:02}.epub"));
        for page in 0..300 {
            f.storage
                .write(
                    &std::format!(".paperpaw/cache/{old}/epub/c1/pages/p{page}.bin"),
                    b"x",
                )
                .unwrap();
        }
        f.storage
            .write(
                &std::format!(".paperpaw/cache/{old}/epub/c1/.host/.complete"),
                b"k",
            )
            .unwrap();
    }
    let other = key("Books/other.epub");
    f.storage
        .write(&std::format!(".paperpaw/cache/{other}/.complete"), b"o")
        .unwrap();
    let counted = Removals {
        card: &f.storage,
        removed: Cell::new(0),
    };
    let mut out = |_| ();
    let carry = EnvCarry::from_parts(
        &counted,
        &f.formats,
        &f.data,
        &f.settings,
        &f.resume,
        &mut out,
    );
    let (mut phase, mut steps, mut worst) = (Phase::Books(0), 0, 0);
    while !matches!(phase, Phase::Assignments) {
        let before = counted.removed.get();
        phase = carry.carry("Books/old", "Books/new", phase).unwrap();
        worst = worst.max(counted.removed.get() - before);
        steps += 1;
        assert!(steps < 2000, "the job ends");
    }
    // 8 pieces of at most 64 removals, and a stamp or so for each book.
    assert!(worst <= 8 * 65 + 2 * books, "a step removed {worst}");
    assert!(steps > 10, "the caches took many steps: {steps}");
    assert!(
        f.storage
            .exists(&std::format!(".paperpaw/cache/{other}/.complete"))
    );
    for n in 0..books {
        let old = key(&std::format!("Books/old/b{n:02}.epub"));
        assert!(
            !f.storage
                .exists(&std::format!(".paperpaw/cache/{old}/epub/c1/pages/p0.bin"))
        );
        assert!(f.has_place(&std::format!("Books/new/b{n:02}.epub")));
    }
}

/// A card status the card gives for good is left (as for the data), another
/// one repeats the step.
#[test]
fn a_status_for_good_is_left_and_a_transient_one_repeats() {
    assert_eq!(moved_status(Ok(())), Ok(()));
    assert_eq!(moved_status(Err(StorageError::InvalidPath)), Ok(()));
    assert_eq!(
        moved_status(Err(StorageError::Io)),
        Err(super::super::storage::status(StorageError::Io))
    );
}

#[test]
fn folder_settings_that_name_the_moved_folder_follow_it() {
    static FOLDER: SettingExtra =
        SettingExtra::new().editor_slot(paperpaw_sdk::builtin::slots::CHOOSE_FOLDER);
    let mut f = fixture();
    f.settings
        .define(SettingSpec::text("t.dir", "", "", groups::OTHER).with_extra(&FOLDER))
        .unwrap();
    f.settings
        .define(SettingSpec::text("t.plain", "", "", groups::OTHER))
        .unwrap();
    f.settings
        .set(
            "t.dir",
            SettingValue::Text("Books/old/inner".to_string().into()),
        )
        .unwrap();
    f.settings
        .set(
            "t.plain",
            SettingValue::Text("Books/old".to_string().into()),
        )
        .unwrap();
    f.storage.write("Books/new/inner/a.epub", b"x").unwrap();
    f.storage.write("Books/old/.keep", b"x").unwrap();
    let (sent, _) = f.run("Books/old", "Books/new");
    let changes: Vec<_> = sent
        .iter()
        .map(|c| std::format!("{c:?}"))
        .filter(|c| c.contains("SetSetting"))
        .collect();
    // Only the folder-chosen setting follows; the plain text is not a folder.
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert!(changes[0].contains("t.dir") && changes[0].contains("Books/new/inner"));
}

#[test]
fn a_cut_events_step_replays_from_its_cursor_or_from_the_start() {
    let f = fixture();
    for n in 0..(STEP_BOOKS + 4) {
        f.book(&std::format!("Books/new/b{n:03}.epub"));
    }
    // The events phase replayed from 0 sends every book again (at least
    // once) and ends the same way.
    let (first, _) = f.run_from("Books/old", "Books/new", Phase::Events(0));
    let (again, _) = f.run_from("Books/old", "Books/new", Phase::Events(0));
    assert_eq!(moved_events(&first), moved_events(&again));
    assert_eq!(moved_events(&first).len(), STEP_BOOKS + 4);
    // From a cursor: the rest only.
    let (rest, _) = f.run_from("Books/old", "Books/new", Phase::Events(STEP_BOOKS as u64));
    assert_eq!(moved_events(&rest).len(), 4);
}

/// A card that counts the directory entries the walk is shown.
struct Counting {
    inner: MemoryStorage,
    shown: Cell<usize>,
}

impl Storage for Counting {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.inner.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.inner.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.inner.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(d, &mut |e| {
            self.shown.set(self.shown.get() + 1);
            v(e);
        })
    }
    fn list_from(
        &self,
        d: &str,
        from: u64,
        v: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.inner.list_from(d, from, &mut |e| {
            self.shown.set(self.shown.get() + 1);
            v(e)
        })
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.inner.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.inner.rename(f, t)
    }
}

#[test]
fn a_step_costs_its_books_not_the_folder_on_a_big_library() {
    // 1,000 books in 10 folders of 100, one flat folder of 1,000 beside.
    for layout in [10, 1] {
        let f = fixture();
        let counting = Counting {
            inner: MemoryStorage::new(),
            shown: Cell::new(0),
        };
        let per = 1000 / layout;
        for n in 0..1000 {
            counting
                .inner
                .write(&std::format!("Books/new/f{}/b{n:04}.epub", n / per), b"b")
                .unwrap();
        }
        let mut sent = Vec::new();
        let mut out = |c| sent.push(c);
        let carry = EnvCarry::from_parts(
            &counting,
            &f.formats,
            &f.data,
            &f.settings,
            &f.resume,
            &mut out,
        );
        let (mut phase, mut steps, mut worst) = (Phase::Books(0), 0, 0);
        while phase != Phase::Done {
            let before = counting.shown.get();
            phase = carry.carry("Books/old", "Books/new", phase).unwrap();
            worst = worst.max(counting.shown.get() - before);
            steps += 1;
            assert!(steps < 500);
        }
        // Each step is shown what it takes plus a folder's entries at most
        // (the list is counted from a place here; a FAT card seeks to it).
        assert!(worst <= 16 + per + layout + 2, "{layout}: worst {worst}");
        // Two passes over the books (carry, events) and the folders once.
        let total = counting.shown.get();
        assert!(
            total <= 2 * (1000 + layout) + steps * (per + 2),
            "{layout}: {total}"
        );
        assert_eq!(moved_events(&sent).len(), 1000);
    }
}

#[test]
fn a_book_the_first_pass_missed_is_carried_by_the_events_pass() {
    let f = fixture();
    f.book("Books/new/a.epub");
    f.place("Books/old/a.epub");
    // The books pass did not reach it; the events pass carries it too.
    let (sent, _) = f.run_from("Books/old", "Books/new", Phase::Events(0));
    assert!(f.has_place("Books/new/a.epub") && !f.has_place("Books/old/a.epub"));
    assert_eq!(moved_events(&sent).len(), 1);
}

#[test]
fn a_saved_walk_that_is_not_at_the_cursor_begins_again_and_gives_the_same_books() {
    let f = fixture();
    for n in 0..(STEP_BOOKS * 3) {
        f.book(&std::format!("Books/new/b{n:03}.epub"));
    }
    let take = |cursor: u64| {
        f.resume
            .books(
                &f.storage,
                &f.formats,
                false,
                "Books/new",
                cursor,
                STEP_BOOKS,
            )
            .unwrap()
    };
    let (first, more) = take(0);
    assert!(more && first.len() == STEP_BOOKS);
    let (second, _) = take(STEP_BOOKS as u64);
    // Asked for again (a failed step is retried): the same books.
    let (again, _) = take(STEP_BOOKS as u64);
    assert_eq!(second, again);
    // A restart passes the first books and gives the next ones.
    let fresh = Resume::default();
    let (restarted, _) = fresh
        .books(
            &f.storage,
            &f.formats,
            false,
            "Books/new",
            STEP_BOOKS as u64,
            STEP_BOOKS,
        )
        .unwrap();
    assert_eq!(second, restarted);
    assert!(first.iter().all(|b| !second.contains(b)));
}
