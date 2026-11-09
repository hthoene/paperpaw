#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The staged files of the host's writers over a device that fails where a
//! card fails: before the rename, in it, and after the old file is gone.
extern crate std;
use super::*;
use crate::native_host::net_service::fail_nth;
use alloc::rc::Rc;
use core::cell::{Cell, RefCell};
use paperpaw_platform::DirEntry;
use paperpaw_sdk::MemoryStorage;
use std::vec::Vec;

/// A card over memory that logs its calls and fails on demand.
struct Card {
    files: MemoryStorage,
    log: RefCell<Vec<&'static str>>,
    sync_fails: Cell<bool>,
    /// The rename fails; with `takes_target`, only after the old file at the
    /// target is gone (the rename's replace had begun).
    rename_fails: Cell<bool>,
    takes_target: Cell<bool>,
    append_fails: Cell<bool>,
}

impl Card {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            files: MemoryStorage::new(),
            log: RefCell::new(Vec::new()),
            sync_fails: Cell::new(false),
            rename_fails: Cell::new(false),
            takes_target: Cell::new(false),
            append_fails: Cell::new(false),
        })
    }
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(path)
    }
}

impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.files.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.log.borrow_mut().push("write");
        self.files.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.log.borrow_mut().push("append");
        if self.append_fails.get() {
            return Err(StorageError::Full);
        }
        self.files.append(p, d)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.log.borrow_mut().push("sync");
        if self.sync_fails.get() {
            return Err(StorageError::Io);
        }
        Ok(())
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.log.borrow_mut().push("remove");
        self.files.remove(p)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.log.borrow_mut().push("rename");
        if self.rename_fails.get() {
            if self.takes_target.get() {
                self.files.remove(to).ok();
            }
            return Err(StorageError::Io);
        }
        self.files.rename(from, to)
    }
    fn ignores_case(&self) -> bool {
        true
    }
}

const OWNER: u64 = 7;
const TARGET: &str = "Books/Äpfel.epub";
const STAGED: &str = "Books/Äpfel.epub.pp-staged";

fn saves(card: &Rc<Card>) -> SaveFiles {
    SaveFiles::new(card.clone(), Rc::new(PathsInUse::default()))
}

/// A claimed save with `new` staged and the old file in place.
fn staged_save(card: &Rc<Card>, saves: &SaveFiles) {
    card.files.write(TARGET, b"old").unwrap();
    saves.claim(OWNER, TARGET, STAGED, true).unwrap();
    saves.begin(OWNER, TARGET, STAGED).unwrap();
    saves.append(STAGED, b"new bytes").unwrap();
}

#[test]
fn a_claim_is_refused_under_any_spelling_and_for_the_owner_too_where_exclusive() {
    let card = Card::new();
    let saves = saves(&card);
    saves.claim(OWNER, TARGET, STAGED, true).unwrap();
    // Another spelling of the same file (FAT folds non-ASCII case), either
    // name, any plugin, and the owner itself when exclusive.
    for (owner, target, staged) in [
        (8, "books/äpfel.EPUB", "books/äpfel.epub.PP-STAGED"),
        (OWNER, "BOOKS/ÄPFEL.EPUB", "BOOKS/ÄPFEL.EPUB.PP-STAGED"),
        (8, "Books/other.epub", STAGED),
        (8, TARGET, "Books/other.pp-staged"),
    ] {
        assert_eq!(saves.claim(owner, target, staged, true), Err(ERR_BUSY));
    }
    // Not exclusive: another plugin is still refused, in any spelling.
    assert_eq!(
        saves.claim(8, "books/äpfel.EPUB", "books/äpfel.epub.PP-STAGED", false),
        Err(ERR_BUSY)
    );
    // Non-exclusive: the owner's own claim is no obstacle (NET replaces its
    // own request), the claims are counted.
    assert_eq!(saves.claim(OWNER, TARGET, STAGED, false), Ok(()));
    assert!(saves.release(OWNER, TARGET));
    assert!(saves.release(OWNER, TARGET));
    assert!(!saves.release(OWNER, TARGET), "nothing left");
    assert_eq!(saves.claim(8, TARGET, STAGED, true), Ok(()));
}

#[test]
fn the_staged_file_is_flushed_before_it_is_renamed_over_the_target() {
    let card = Card::new();
    let saves = saves(&card);
    staged_save(&card, &saves);
    card.log.borrow_mut().clear();
    saves.commit(OWNER, TARGET, STAGED).unwrap();
    assert_eq!(card.log.borrow().as_slice(), &["sync", "rename"]);
    assert_eq!(card.get(TARGET).unwrap(), b"new bytes");
    assert!(card.get(STAGED).is_none());
    assert!(saves.release(OWNER, TARGET));
    assert_eq!(card.get(TARGET).unwrap(), b"new bytes", "release keeps it");
}

#[test]
fn a_flush_that_fails_renames_nothing_and_leaves_the_old_file() {
    let card = Card::new();
    let saves = saves(&card);
    staged_save(&card, &saves);
    card.sync_fails.set(true);
    card.log.borrow_mut().clear();
    assert_eq!(saves.commit(OWNER, TARGET, STAGED), Err(StorageError::Io));
    assert_eq!(
        card.log.borrow().as_slice(),
        &["sync"],
        "no rename after it"
    );
    assert_eq!(card.get(TARGET).unwrap(), b"old");
    assert!(!saves.is_kept(OWNER, TARGET), "the old file is the copy");
    assert!(saves.release(OWNER, TARGET));
    assert_eq!(card.get(TARGET).unwrap(), b"old");
    assert!(card.get(STAGED).is_none(), "the temporary goes");
}

#[test]
fn a_rename_that_fails_with_the_old_file_in_place_removes_the_temporary() {
    let card = Card::new();
    let saves = saves(&card);
    staged_save(&card, &saves);
    card.rename_fails.set(true);
    assert_eq!(saves.commit(OWNER, TARGET, STAGED), Err(StorageError::Io));
    assert!(!saves.is_kept(OWNER, TARGET));
    saves.release(OWNER, TARGET);
    assert_eq!(card.get(TARGET).unwrap(), b"old");
    assert!(card.get(STAGED).is_none());
}

#[test]
fn a_rename_that_fails_after_the_old_file_is_gone_keeps_the_temporary_as_the_only_copy() {
    let card = Card::new();
    let saves = saves(&card);
    staged_save(&card, &saves);
    card.rename_fails.set(true);
    card.takes_target.set(true);
    assert_eq!(saves.commit(OWNER, TARGET, STAGED), Err(StorageError::Io));
    assert!(card.get(TARGET).is_none(), "the old file is gone");
    assert!(saves.is_kept(OWNER, TARGET));
    // Neither the discard at the request's end nor the release removes it.
    saves.discard(OWNER, TARGET);
    assert_eq!(card.get(STAGED).unwrap(), b"new bytes");
    assert!(saves.release(OWNER, TARGET));
    assert_eq!(card.get(STAGED).unwrap(), b"new bytes", "left on the card");
    // The next save to the path replaces it.
    card.rename_fails.set(false);
    saves.claim(OWNER, TARGET, STAGED, true).unwrap();
    saves.begin(OWNER, TARGET, STAGED).unwrap();
    assert_eq!(card.get(STAGED).unwrap(), b"", "a fresh one");
    assert!(!saves.is_kept(OWNER, TARGET));
    saves.append(STAGED, b"second").unwrap();
    saves.commit(OWNER, TARGET, STAGED).unwrap();
    assert_eq!(card.get(TARGET).unwrap(), b"second");
}

#[test]
fn a_write_that_fails_leaves_the_old_file_and_the_release_clears_up() {
    let card = Card::new();
    let saves = saves(&card);
    card.files.write(TARGET, b"old").unwrap();
    saves.claim(OWNER, TARGET, STAGED, true).unwrap();
    saves.begin(OWNER, TARGET, STAGED).unwrap();
    card.append_fails.set(true);
    assert_eq!(saves.append(STAGED, b"x"), Err(StorageError::Full));
    assert!(saves.release(OWNER, TARGET));
    assert_eq!(card.get(TARGET).unwrap(), b"old");
    assert!(card.get(STAGED).is_none());
}

#[test]
fn another_owners_claim_is_not_released_or_discarded_by_mistake() {
    let card = Card::new();
    let saves = saves(&card);
    staged_save(&card, &saves);
    assert!(!saves.release(8, TARGET));
    saves.discard(8, TARGET);
    assert_eq!(card.get(STAGED).unwrap(), b"new bytes");
}

#[test]
fn an_allocation_that_fails_in_a_claim_holds_nothing() {
    for nth in 1..=2 {
        let card = Card::new();
        let saves = saves(&card);
        fail_nth(nth);
        let claimed = saves.claim(OWNER, TARGET, STAGED, true);
        fail_nth(0);
        assert_eq!(claimed, Err(ERR_NO_MEMORY), "gate {nth}");
        assert!(!saves.release(OWNER, TARGET), "gate {nth}");
        assert_eq!(saves.claim(8, TARGET, STAGED, true), Ok(()), "gate {nth}");
    }
}
