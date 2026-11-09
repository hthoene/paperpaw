#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! The card's `MoveHost`: a move through the real state machine over a card
//! that keeps names, a journal that survives, and the refusals.
extern crate std;
use super::*;
use crate::native_host::{lib_moves_journal::JOURNAL, lib_service::moves::Moves};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NOT_FOUND};
use paperpaw_platform::DirEntry;
use paperpaw_sdk::MemoryStorage;
use std::{string::String, vec::Vec};

/// A card with a case-insensitive name rule and the identity of what an
/// entry holds (its files' names below it and their bytes): the same before
/// and after a rename.
struct Card {
    inner: MemoryStorage,
    folds: bool,
    tells: bool,
}

impl Card {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            inner: MemoryStorage::new(),
            folds: true,
            tells: true,
        })
    }

    fn put(&self, path: &str, bytes: &[u8]) {
        self.inner.write(path, bytes).unwrap();
    }

    fn files_below(&self, path: &str, out: &mut Vec<String>) {
        let mut names = Vec::new();
        let listed = self.inner.list(path, &mut |entry: DirEntry<'_>| {
            names.push((String::from(entry.name), entry.is_dir));
        });
        if listed.is_err() {
            return;
        }
        for (name, is_dir) in names {
            let full = if path.is_empty() {
                name.clone()
            } else {
                std::format!("{path}/{name}")
            };
            if is_dir {
                self.files_below(&full, out);
            } else {
                out.push(full);
            }
        }
    }
}

impl Storage for Card {
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
        self.inner.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.inner.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.inner.rename(f, t)
    }
    fn ignores_case(&self) -> bool {
        self.folds
    }
    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.tells.then(|| a.eq_ignore_ascii_case(b))
    }
}

impl EntryIdentity for Card {
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        let mut files = Vec::new();
        if self.inner.exists(path) {
            files.push(String::from(path));
        } else {
            self.files_below(path, &mut files);
        }
        if files.is_empty() {
            return Ok(None);
        }
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for file in &files {
            // The names below the entry, not the entry's own: a rename keeps it.
            let rest = file.strip_prefix(path).unwrap_or("");
            let mut bytes = std::vec![0u8; self.inner.size(file)? as usize];
            self.inner.read_at(file, 0, &mut bytes)?;
            for b in rest.bytes().chain(bytes) {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        Ok(Some(hash))
    }
}

/// The catalog side, recording what the machine asks of it.
#[derive(Default)]
struct Carry {
    calls: RefCell<Vec<String>>,
}

impl MoveCarry for Carry {
    fn discard_replaced(&self, to: &str) -> Result<(), i32> {
        self.calls.borrow_mut().push(std::format!("discard {to}"));
        Ok(())
    }
    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32> {
        self.calls
            .borrow_mut()
            .push(std::format!("{phase:?} {from}>{to}"));
        Ok(match phase {
            Phase::Books(_) => Phase::Assignments,
            Phase::Assignments => Phase::Settings,
            Phase::Settings => Phase::Events(0),
            Phase::Events(_) | Phase::Done => Phase::Done,
        })
    }
    fn carry_book(&self, _: &str, _: &str, book: &str) -> Result<(), i32> {
        self.calls.borrow_mut().push(std::format!("book {book}"));
        Ok(())
    }
}

fn host(card: &Rc<Card>, carry: &Rc<Carry>) -> CardMoves {
    CardMoves::new(card.clone(), Some(card.clone()), carry.clone())
}

fn library() -> Rc<Card> {
    let card = Card::new();
    card.put("Books/old/a.epub", b"AAA");
    card.put("Books/old/sub/b.epub", b"BBBB");
    card.put("Books/other.epub", b"C");
    card
}

fn run(moves: &Moves, host: &CardMoves) {
    for _ in 0..100 {
        if !moves.step(host).unwrap() {
            return;
        }
    }
    panic!("the job ends");
}

#[test]
fn a_folder_move_journals_renames_carries_every_phase_and_clears_the_entry() {
    let (card, carry) = (library(), Rc::new(Carry::default()));
    let (host, moves) = (host(&card, &carry), Moves::default());
    moves.begin(&host, "Books/old", "Books/new", false).unwrap();
    // The journal was written before the rename and holds the move.
    assert!(card.inner.exists(JOURNAL));
    assert!(card.inner.exists("Books/new/a.epub") && !card.inner.exists("Books/old/a.epub"));
    assert!(host.open_entry(&mut |e| assert_eq!((e.from, e.to), ("Books/old", "Books/new"))).unwrap());
    run(&moves, &host);
    assert!(!card.inner.exists(JOURNAL), "cleared when everything is carried");
    let calls = carry.calls.borrow();
    assert!(calls[0].starts_with("Books(") && calls.last().unwrap().starts_with("Events("));
    assert!(calls.iter().any(|c| c.starts_with("Assignments")));
    assert!(calls.iter().any(|c| c.starts_with("Settings")));
}

#[test]
fn a_cut_right_after_the_rename_is_finished_at_start_from_the_journal_alone() {
    let (card, carry) = (library(), Rc::new(Carry::default()));
    {
        let (host, moves) = (host(&card, &carry), Moves::default());
        moves.begin(&host, "Books/old", "Books/new", false).unwrap();
        // The power goes: the job's memory is lost; the card has the entry.
    }
    let (host, moves) = (host(&card, &carry), Moves::default());
    moves.start(&host).unwrap();
    run(&moves, &host);
    assert!(!card.inner.exists(JOURNAL));
    assert!(carry.calls.borrow().iter().any(|c| c.starts_with("Books(")));
}

#[test]
fn a_cut_before_the_rename_drops_the_entry_and_nothing_is_carried() {
    let (card, carry) = (library(), Rc::new(Carry::default()));
    let host = host(&card, &carry);
    // The entry was written, the rename never happened.
    let identity = card.entry_identity("Books/old").unwrap().unwrap();
    host.write_entry(&Entry {
        from: "Books/old",
        to: "Books/new",
        replace: false,
        identity,
    })
    .unwrap();
    let moves = Moves::default();
    moves.start(&host).unwrap();
    run(&moves, &host);
    assert!(!card.inner.exists(JOURNAL));
    assert!(card.inner.exists("Books/old/a.epub"));
    assert!(carry.calls.borrow().is_empty(), "nothing moved, nothing carried");
}

#[test]
fn without_an_identity_from_the_shell_every_move_is_refused_before_anything_changes() {
    let (card, carry) = (library(), Rc::new(Carry::default()));
    let host = CardMoves::new(card.clone(), None, carry);
    let moves = Moves::default();
    assert_eq!(
        moves.begin(&host, "Books/old", "Books/new", false),
        Err(ERR_UNSUPPORTED)
    );
    assert!(!card.inner.exists(JOURNAL) && card.inner.exists("Books/old/a.epub"));
}

#[test]
fn a_storage_that_cannot_tell_same_entry_refuses_and_one_that_can_does_not() {
    let carry = Rc::new(Carry::default());
    let card = Rc::new(Card {
        inner: MemoryStorage::new(),
        folds: true,
        tells: false,
    });
    card.put("Books/a.epub", b"A");
    let host = CardMoves::new(card.clone(), Some(card.clone()), carry);
    assert_eq!(host.same_entry("Books/a.epub", "books/A.EPUB"), Err(ERR_UNSUPPORTED));
    let told = library();
    let host = CardMoves::new(told.clone(), Some(told.clone()), Rc::new(Carry::default()));
    assert_eq!(host.same_entry("Books/old", "books/OLD"), Ok(true));
    assert_eq!(host.same_entry("Books/old", "Books/other.epub"), Ok(false));
}

#[test]
fn a_missing_source_is_not_found_and_the_journal_is_not_written() {
    let (card, carry) = (library(), Rc::new(Carry::default()));
    let (host, moves) = (host(&card, &carry), Moves::default());
    assert_eq!(
        moves.begin(&host, "Books/gone", "Books/new", false),
        Err(ERR_NOT_FOUND)
    );
    assert!(!card.inner.exists(JOURNAL));
    // A second move waits while a job is open.
    moves.begin(&host, "Books/old", "Books/new", false).unwrap();
    assert_eq!(
        moves.begin(&host, "Books/other.epub", "Books/x.epub", false),
        Err(ERR_BUSY)
    );
}

#[test]
fn folders_spelling_and_within_follow_the_storage() {
    let (card, carry) = (library(), Rc::new(Carry::default()));
    let host = host(&card, &carry);
    assert_eq!(host.is_folder("Books/old"), Ok(true));
    assert_eq!(host.is_folder("Books/other.epub"), Ok(false));
    assert_eq!(host.is_folder("Books/nothing"), Ok(false));
    assert_eq!(host.spelled_as("Books/old"), Ok(true));
    assert_eq!(host.spelled_as("Books/OLD"), Ok(false), "the stored spelling is exact");
    assert_eq!(host.spelled_as("Books/none"), Ok(false));
    assert_eq!(host.spelled_as("Nowhere/x"), Ok(false));
    // Case folds as the storage says; the top is every path's folder.
    assert!(host.within("books/OLD/a.epub", "Books/old"));
    assert!(host.within("Books/old", "Books/old"));
    assert!(!host.within("Books/older", "Books/old"));
    assert!(!host.within("Books", "Books/old"));
    assert!(host.within("Books/old", ""));
    // A folder, on a card that keeps case, is exact.
    let exact = Rc::new(Card {
        inner: MemoryStorage::new(),
        folds: false,
        tells: true,
    });
    let host = CardMoves::new(exact.clone(), None, Rc::new(Carry::default()));
    assert!(!host.within("books/old", "Books"));
    assert!(host.within("Books/old", "Books"));
}
