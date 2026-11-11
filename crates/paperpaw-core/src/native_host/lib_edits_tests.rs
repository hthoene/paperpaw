#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The staged transactions on the card: stage, commit, receipt, refusals,
//! power loss.
extern crate std;
use super::*;
use crate::native_host::lib_host::LibraryHost;
use crate::native_host::lib_reads::tests::{Card, card};
use crate::native_host::lib_service::{BookRef, Caller, LibQuery, LibWrite};
use crate::native_host::registrations as wire;
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_platform::Storage;
use std::vec::Vec;

const BOOK: &str = "books/a.epub";
const OWNER: u64 = 7;

/// The card's facts and a clock the test sets.
struct Clocked {
    inner: Rc<crate::native_host::lib_reads::tests::Env>,
    now: Cell<u64>,
}

impl CatalogEnv for Clocked {
    fn walking(&self) -> Option<u32> {
        self.inner.walking()
    }
    fn today(&self) -> Option<u32> {
        self.inner.today()
    }
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
}

/// The rows of a receipt (edit, id, action) and the generation it carries.
type Receipt = (Vec<(u32, u32, u32)>, u32);

struct Fixture {
    card: Card,
    clock: Rc<Clocked>,
    host: LibraryHost,
    next: Cell<u32>,
}

fn fixture() -> Fixture {
    let card = card();
    let clock = Rc::new(Clocked {
        inner: card.env.clone(),
        now: Cell::new(1_000),
    });
    let host = LibraryHost::new(card.storage.clone(), clock.clone(), None);
    Fixture {
        card,
        clock,
        host,
        next: Cell::new(1),
    }
}

fn edit(action: u32) -> wire::ServiceLibEdit<'static> {
    wire::ServiceLibEdit {
        action,
        id: 0,
        kind: 1,
        start_section: 1,
        start_offset: 5,
        end_section: 1,
        end_offset: 9,
        flags: 0,
        percent: 10,
        conversion: 0,
        created: 11,
        updated: 12,
        client_key: "",
        text: "hello",
        note: "",
        text_bytes: 0,
        note_bytes: 0,
    }
}

fn who() -> Caller<'static> {
    Caller {
        owner: OWNER,
        plugin: "koreader",
        storage: Ok(()),
    }
}

impl Fixture {
    fn annotations(&self) -> Vec<(u32, u32, u32, std::string::String, std::string::String)> {
        let mut rows = Vec::new();
        self.host
            .annotations(BookRef::Path(BOOK), 0, 0, &mut |record| {
                if let wire::Record::ServiceLibAnnotation(row) = record {
                    rows.push((
                        row.id,
                        row.kind,
                        row.start_offset,
                        row.text.into(),
                        row.client_key.into(),
                    ));
                }
                core::ops::ControlFlow::Continue(())
            })
            .unwrap();
        rows
    }

    fn generation(&self) -> u32 {
        self.host
            .annotations(BookRef::Path(BOOK), 0, 0, &mut |_| {
                core::ops::ControlFlow::Continue(())
            })
            .unwrap()
            .generation
    }

    /// One transaction: stages `edits` (with the parts of the first edit's
    /// text) and commits it on the generation `base`.
    fn transact(
        &self,
        base: u32,
        edits: &[wire::ServiceLibEdit<'_>],
        parts: &[(u32, u32, &str, bool)],
    ) -> Result<(Committed, u32), i32> {
        let transaction = self.next.get();
        self.next.set(transaction + 1);
        let book = self.host.resolve_book(BookRef::Path(BOOK))?;
        for (index, edit) in edits.iter().enumerate() {
            self.host.stage(
                transaction,
                &StageItem::Edit {
                    index: index as u32,
                    edit,
                },
            )?;
        }
        let mut offsets = [0_u32; 8];
        for &(field, which, data, last) in parts {
            let offset = &mut offsets[which as usize * 4 + field as usize];
            self.host.stage(
                transaction,
                &StageItem::Part {
                    edit: which,
                    field,
                    offset: *offset,
                    data,
                    last,
                },
            )?;
            *offset += data.len() as u32;
        }
        let request = CommitRequest {
            transaction,
            owner: OWNER,
            book,
            base_generation: base,
            edits: edits.len() as u32,
        };
        let done = self.host.commit(&request);
        match done {
            Ok(committed) => Ok((committed, transaction)),
            Err(error) => {
                self.host.abort_staged(transaction);
                Err(error)
            }
        }
    }

    fn receipt(&self, transaction: u32, cursor: u32) -> Result<Receipt, i32> {
        let mut rows = Vec::new();
        let end = self
            .host
            .receipt(&who(), transaction, cursor, &mut |record| {
                if let wire::Record::ServiceLibReceipt(row) = record {
                    rows.push((row.edit, row.id, row.action));
                }
                core::ops::ControlFlow::Continue(())
            })?;
        Ok((rows, end.generation))
    }

    fn file(&self) -> std::string::String {
        let path = paperpaw_sdk::annotations::file(BOOK);
        let size = self.card.storage.size(&path).unwrap() as usize;
        let mut bytes = std::vec![0_u8; size];
        self.card.storage.read_at(&path, 0, &mut bytes).unwrap();
        std::string::String::from_utf8(bytes).unwrap()
    }

    fn staged_files(&self) -> Vec<std::string::String> {
        let mut names = Vec::new();
        let _ = self.card.storage.list(stage::DIR, &mut |entry| {
            names.push(entry.name.into());
        });
        names
    }
}

#[test]
fn a_transaction_stages_commits_and_leaves_a_receipt() {
    let t = fixture();
    assert_eq!(t.generation(), 1);
    let mut second = edit(0);
    second.kind = 0;
    second.start_offset = 2;
    second.end_offset = 2;
    second.text = "";
    let mut third = edit(3);
    third.client_key = "kr-1";
    third.start_offset = 30;
    third.end_offset = 40;
    // The first edit's text comes in parts.
    let mut first = edit(0);
    first.text = "";
    first.text_bytes = 11;
    let (committed, transaction) = t
        .transact(
            1,
            &[first, second, third],
            &[(0, 0, "hello ", false), (0, 0, "world", true)],
        )
        .unwrap();
    assert_eq!(
        (
            committed.annotation_generation,
            committed.edits,
            committed.added
        ),
        (3, 3, 3)
    );
    // In reading order: the bookmark, the highlight, the keyed one.
    assert_eq!(
        t.annotations(),
        [
            (2, 0, 2, "".into(), "".into()),
            (1, 1, 5, "hello world".into(), "".into()),
            (3, 1, 30, "hello".into(), "kr-1".into()),
        ]
    );
    assert_eq!(t.generation(), 3);
    assert!(
        t.file()
            .starts_with("# format 3\n# next-id 4\n# generation 3\n")
    );
    // The ids of the edits, in the order of the edits, and by pages.
    assert_eq!(
        t.receipt(transaction, 0).unwrap(),
        (std::vec![(0, 1, 0), (1, 2, 0), (2, 3, 0)], 3)
    );
    assert_eq!(t.receipt(transaction, 2).unwrap().0, [(2, 3, 0)]);
    // The staged file is gone, the receipt stays, for the plugin that made it.
    assert_eq!(t.staged_files().len(), 1);
    let other = Caller { owner: 8, ..who() };
    assert_eq!(
        t.host.receipt(&other, transaction, 0, &mut |_| {
            core::ops::ControlFlow::Continue(())
        }),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn replace_remove_and_upsert_change_the_rows_they_name() {
    let t = fixture();
    let mut keyed = edit(0);
    keyed.client_key = "kr-1";
    let mut other = edit(0);
    other.start_offset = 50;
    other.end_offset = 60;
    other.text = "second";
    t.transact(1, &[keyed, other], &[]).unwrap();
    let generation = t.generation();
    assert_eq!(generation, 3);
    // Replace the second (new text and a new place), upsert the keyed one by
    // its key, remove nothing yet.
    let mut replace = edit(1);
    replace.id = 2;
    replace.start_offset = 1;
    replace.end_offset = 2;
    replace.text = "moved";
    let mut upsert = edit(3);
    upsert.client_key = "kr-1";
    upsert.text = "updated";
    let (committed, transaction) = t.transact(generation, &[replace, upsert], &[]).unwrap();
    assert_eq!((committed.annotation_generation, committed.added), (4, 0));
    assert_eq!(
        t.annotations(),
        [
            (2, 1, 1, "moved".into(), "".into()),
            (1, 1, 5, "updated".into(), "kr-1".into()),
        ]
    );
    assert_eq!(t.receipt(transaction, 0).unwrap().0, [(0, 2, 1), (1, 1, 3)]);
    let mut remove = edit(2);
    remove.id = 1;
    remove.text = "";
    t.transact(4, &[remove], &[]).unwrap();
    assert_eq!(t.annotations(), [(2, 1, 1, "moved".into(), "".into())]);
}

#[test]
fn a_commit_that_is_refused_changes_nothing() {
    let t = fixture();
    let mut keyed = edit(0);
    keyed.client_key = "kr-1";
    t.transact(1, &[keyed], &[]).unwrap();
    let before = t.file();
    // The book moved on since the plugin read it.
    assert_eq!(t.transact(1, &[edit(0)], &[]).map(|_| ()), Err(ERR_STATE));
    // An id that is not there; an id named twice; a key the book has; a key
    // named twice in one transaction.
    let mut gone = edit(2);
    gone.id = 9;
    assert_eq!(t.transact(3, &[gone], &[]).map(|_| ()), Err(ERR_NOT_FOUND));
    let mut twice = edit(2);
    twice.id = 1;
    assert_eq!(
        t.transact(3, &[twice, twice], &[]).map(|_| ()),
        Err(ERR_INVALID)
    );
    assert_eq!(t.transact(3, &[keyed], &[]).map(|_| ()), Err(ERR_INVALID));
    let mut again = edit(0);
    again.client_key = "kr-2";
    assert_eq!(
        t.transact(3, &[again, again], &[]).map(|_| ()),
        Err(ERR_INVALID)
    );
    // A text longer than a line is no row.
    let mut long = edit(0);
    long.text = "";
    long.text_bytes = 9_000;
    let nine = "x".repeat(3_000);
    assert_eq!(
        t.transact(
            3,
            &[long],
            &[
                (0, 0, &nine, false),
                (0, 0, &nine, false),
                (0, 0, &nine, true)
            ]
        )
        .map(|_| ()),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(t.file(), before);
    assert!(
        t.staged_files()
            .iter()
            .all(|name| name.ends_with("receipt"))
    );
    assert_eq!(t.staged_files().len(), 1);
}

#[test]
fn a_version_2_file_is_converted_by_the_first_commit() {
    let t = fixture();
    let path = paperpaw_sdk::annotations::file(BOOK);
    t.card
        .storage
        .write(
            &path,
            b"# format 2\nh\t1\t10\t1\t20\t5\t100\t200\t\t\t\tfirst\tnote\nb\t3\t0\t3\t0\t50\t1\t1\t\t\t\tsecond\t\n",
        )
        .unwrap();
    assert_eq!(t.generation(), 1);
    let mut remove = edit(2);
    remove.id = 1;
    remove.text = "";
    let (committed, _) = t.transact(1, &[remove, edit(0)], &[]).unwrap();
    assert_eq!((committed.annotation_generation, committed.added), (3, 1));
    let file = t.file();
    assert!(
        file.starts_with("# format 3\n# next-id 4\n# generation 3\n"),
        "{file}"
    );
    assert_eq!(
        t.annotations(),
        [
            (3, 1, 5, "hello".into(), "".into()),
            (2, 0, 0, "second".into(), "".into())
        ]
    );
}

#[test]
fn a_power_cut_leaves_the_old_file_and_the_next_start_removes_the_staged_files() {
    let t = fixture();
    t.transact(1, &[edit(0)], &[]).unwrap();
    let before = t.file();
    let transaction = 90;
    let book = t.host.resolve_book(BookRef::Path(BOOK)).unwrap();
    t.host
        .stage(
            transaction,
            &StageItem::Edit {
                index: 0,
                edit: &edit(0),
            },
        )
        .unwrap();
    assert!(t.staged_files().iter().any(|name| name.starts_with("5a.")));
    // The device restarts: the staged file and the receipt are gone, the book's
    // annotations are as they were, and a commit of the lost transaction says so.
    let restarted = LibraryHost::new(t.card.storage.clone(), t.clock.clone(), None);
    assert!(t.staged_files().is_empty());
    let request = CommitRequest {
        transaction,
        owner: OWNER,
        book,
        base_generation: 3,
        edits: 1,
    };
    assert_eq!(restarted.commit(&request).map(|_| ()), Err(ERR_INVALID));
    assert_eq!(t.file(), before);
}

#[test]
fn a_commit_that_cannot_rename_leaves_the_old_file_and_no_receipt() {
    struct NoRename(Rc<paperpaw_sdk::MemoryStorage>);
    impl Storage for NoRename {
        fn read_at(
            &self,
            p: &str,
            o: u64,
            b: &mut [u8],
        ) -> Result<usize, paperpaw_platform::StorageError> {
            self.0.read_at(p, o, b)
        }
        fn size(&self, p: &str) -> Result<u64, paperpaw_platform::StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.0.append(p, d)
        }
        fn list(
            &self,
            d: &str,
            v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), paperpaw_platform::StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), paperpaw_platform::StorageError> {
            self.0.remove(p)
        }
        fn rename(&self, _: &str, _: &str) -> Result<(), paperpaw_platform::StorageError> {
            Err(paperpaw_platform::StorageError::Full)
        }
    }
    let card = card();
    let clock = Rc::new(Clocked {
        inner: card.env.clone(),
        now: Cell::new(1),
    });
    let host = LibraryHost::new(Rc::new(NoRename(card.storage.clone())), clock, None);
    let book = host.resolve_book(BookRef::Path(BOOK)).unwrap();
    host.stage(
        1,
        &StageItem::Edit {
            index: 0,
            edit: &edit(0),
        },
    )
    .unwrap();
    let request = CommitRequest {
        transaction: 1,
        owner: OWNER,
        book,
        base_generation: 1,
        edits: 1,
    };
    assert_eq!(host.commit(&request).map(|_| ()), Err(ERR_NO_SPACE));
    assert!(
        card.storage
            .size(&paperpaw_sdk::annotations::file(BOOK))
            .is_err()
    );
    assert!(
        card.storage
            .size(".paperpaw/library/edits/1.receipt")
            .is_err()
    );
    assert!(card.storage.size(".paperpaw/library/edits/1.new").is_err());
}

#[test]
fn a_receipt_ends_at_the_next_begin_at_stop_and_after_ten_minutes() {
    let t = fixture();
    let (_, first) = t.transact(1, &[edit(0)], &[]).unwrap();
    assert!(t.receipt(first, 0).is_ok());
    // The plugin begins again (or stops): its receipts go.
    t.host.receipts_ended(OWNER);
    assert_eq!(t.receipt(first, 0).map(|_| ()), Err(ERR_NOT_FOUND));
    let (_, second) = t.transact(3, &[edit(0)], &[]).unwrap();
    t.clock.now.set(1_000 + 600_001);
    assert_eq!(t.receipt(second, 0).map(|_| ()), Err(ERR_NOT_FOUND));
    assert!(t.staged_files().is_empty());
}

#[test]
fn a_book_the_move_job_holds_cannot_begin_a_transaction() {
    let card = card();
    let moves = Rc::new(crate::native_host::lib_moves_service::LibraryMoves::new(
        card.storage.clone(),
    ));
    moves.begin("books/c.epub", "moved/c.epub", false).unwrap();
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), Some(moves));
    assert_eq!(
        host.resolve_book(BookRef::Path("books/c.epub")),
        Err(ERR_BUSY)
    );
    assert!(host.resolve_book(BookRef::Path("books/a.epub")).is_ok());
    assert_eq!(
        host.resolve_book(BookRef::Path("books/none.epub")),
        Err(ERR_NOT_FOUND)
    );
}

/// A card that answers a read with at most 7 bytes, as FAT may in the middle
/// of a file.
struct Short(Rc<paperpaw_sdk::MemoryStorage>);

impl Storage for Short {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        let end = buf.len().min(7);
        self.0.read_at(path, offset, &mut buf[..end])
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.rename(from, to)
    }
}

#[test]
fn a_card_that_answers_reads_short_does_not_fail_a_commit_or_its_receipt() {
    let card = card();
    let clock = Rc::new(Clocked {
        inner: card.env.clone(),
        now: Cell::new(1_000),
    });
    let host = LibraryHost::new(Rc::new(Short(card.storage.clone())), clock.clone(), None);
    let t = Fixture {
        card,
        clock,
        host,
        next: Cell::new(1),
    };
    let mut keyed = edit(3);
    keyed.client_key = "kr-1";
    keyed.start_offset = 30;
    keyed.end_offset = 40;
    keyed.text = "a note of a good length to cross a few short reads";
    let (committed, transaction) = t.transact(1, &[edit(0), keyed], &[]).unwrap();
    assert_eq!((committed.edits, committed.added), (2, 2));
    assert_eq!(t.receipt(transaction, 0).unwrap().0, [(0, 1, 0), (1, 2, 0)]);
    assert_eq!(t.annotations().len(), 2);
}

#[test]
fn the_devices_monotonic_clock_is_the_one_of_the_transactions_when_the_shell_has_one() {
    struct Device(Cell<u64>);
    impl crate::native_host::shell_wiring::WorkDevice for Device {
        fn now_ms(&self) -> u64 {
            self.0.get()
        }
        fn network_up(&self) -> bool {
            false
        }
        fn radio_on(&self) -> bool {
            false
        }
    }
    let card = card();
    let device = Rc::new(Device(Cell::new(5_000)));
    // The catalog's own clock is 0 (none): the device's is the one.
    let host =
        LibraryHost::new(card.storage.clone(), card.env.clone(), None).with_clock(device.clone());
    assert_eq!(host.now_ms(), 5_000);
    device.0.set(7_000);
    assert_eq!(host.now_ms(), 7_000);
}

#[test]
fn a_host_without_a_clock_ends_an_idle_transaction_after_60_calls() {
    let card = card();
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let first = host.now_ms();
    let mut last = first;
    for _ in 0..60 {
        last = host.now_ms();
    }
    // The idle time of the transactions is 10 minutes.
    assert!(last - first >= 600_000, "{first} {last}");
}
