#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! The catalog side of a library move: books carried a step at a time, old
//! keys gone, replays idempotent, events, folder settings, discard.
extern crate std;
use super::*;
use alloc::{borrow::Cow, vec::Vec};
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
    let carry = EnvCarry::from_parts(&f.storage, &f.formats, &f.data, &f.settings, &mut out);
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
    let carry = EnvCarry::from_parts(&f.storage, &f.formats, &f.data, &f.settings, &mut out);
    carry.discard("Books/b.epub").unwrap();
    assert!(!f.has_place("Books/b.epub"));
    assert!(library::favourites(&f.storage).is_empty());
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
