//! The exFAT write gate (#5 R2 step 2): writes only over a verified
//! journal; a card marked in use without one is read, never written.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "write-gate regression assertions"
)]
mod common;
use common::{Bytes, Recorded, exfat, exfat_card, read};
use paperpaw_fat::{BlockDisk, Blocks, ExFatStorage, SharedDisk};
use paperpaw_platform::{Storage, StorageError};
use std::{cell::RefCell, rc::Rc};

const NEEDS_CHECK: StorageError = StorageError::CARD_NEEDS_CHECK;

fn journaled<B: Blocks + 'static>(card: B) -> ExFatStorage<B> {
    let (disk, _) = BlockDisk::find(card).unwrap();
    ExFatStorage::mount_journaled(SharedDisk::new(disk), 7).unwrap()
}

fn copy(bytes: &[u8]) -> Bytes {
    Bytes(Rc::new(RefCell::new(bytes.to_vec())))
}

fn set_dirty(bytes: &Bytes) {
    bytes.0.borrow_mut()[106] |= 2;
}

/// A card with a book in a folder and a file at the root, synced clean.
fn library() -> Bytes {
    let bytes = exfat_card();
    let card = exfat(&bytes);
    card.create_dir("books").unwrap();
    card.write("books/a.epub", b"book a").unwrap();
    card.write("kept.txt", b"safe content").unwrap();
    card.sync().unwrap();
    drop(card);
    assert_eq!(bytes.0.borrow()[106] & 2, 0);
    bytes
}

/// Every read works and every write is refused with `why`, the card
/// unchanged.
fn reads_only(bytes: &Bytes, why: StorageError) {
    let before = bytes.0.borrow().clone();
    let card = journaled(bytes.clone());
    assert_eq!(card.write_refusal(), Some(why));
    assert_eq!(read(&card, "kept.txt"), b"safe content");
    assert_eq!(read(&card, "books/a.epub"), b"book a");
    let mut names = Vec::new();
    card.list("", &mut |e| names.push(e.name.to_string()))
        .unwrap();
    names.sort();
    assert_eq!(names, ["books", "kept.txt"]);
    assert_eq!(card.write("kept.txt", b"new"), Err(why));
    assert_eq!(card.write("new.txt", b"new"), Err(why));
    assert_eq!(card.append("kept.txt", b"more"), Err(why));
    assert_eq!(card.remove("kept.txt"), Err(why));
    assert_eq!(card.rename("kept.txt", "moved.txt"), Err(why));
    assert_eq!(card.create_dir("more"), Err(why));
    assert_eq!(card.remove_dir_all("books"), Err(why));
    assert_eq!(card.sync(), Ok(()));
    assert_eq!(read(&card, "kept.txt"), b"safe content");
    drop(card);
    assert!(*bytes.0.borrow() == before, "a refused card was written");
}

#[test]
fn a_clean_card_gets_a_journal_and_writes_as_usual() {
    let bytes = library();
    let card = journaled(bytes.clone());
    assert_eq!(card.write_refusal(), None);
    card.write("kept.txt", b"new content").unwrap();
    card.sync().unwrap();
    // The journal is the storage's own: not listed, not taken by name.
    let mut names = Vec::new();
    card.list("", &mut |e| names.push(e.name.to_string()))
        .unwrap();
    names.sort();
    assert_eq!(names, ["books", "kept.txt"]);
    assert_eq!(
        card.write("PP$INTENT", b"x"),
        Err(StorageError::InvalidPath)
    );
    assert_eq!(card.remove("pp$intent"), Err(StorageError::InvalidPath));
    drop(card);
    assert_eq!(bytes.0.borrow()[106] & 2, 0);
    // Verified at the next mount, also where a cut left the card marked.
    set_dirty(&bytes);
    let card = journaled(bytes.clone());
    assert_eq!(card.write_refusal(), None);
    assert_eq!(read(&card, "kept.txt"), b"new content");
    card.write("kept.txt", b"newer").unwrap();
    assert_eq!(read(&card, "kept.txt"), b"newer");
}

#[test]
fn a_card_marked_in_use_without_a_journal_is_read_and_never_written() {
    let bytes = library();
    set_dirty(&bytes);
    for _ in 0..3 {
        reads_only(&bytes, NEEDS_CHECK);
    }
    // The plain mount (journal off) is not affected by the gate.
    assert_eq!(exfat(&bytes).write_refusal(), None);
}

#[test]
fn a_damaged_journal_name_refuses_writes_and_keeps_the_card() {
    let bytes = library();
    drop(journaled(bytes.clone()));
    let name: Vec<u8> = "PP$INTENT"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    {
        let mut data = bytes.0.borrow_mut();
        let text = data.windows(name.len()).position(|b| b == name).unwrap();
        data[text - 66 + 2] ^= 1;
    }
    reads_only(&bytes, NEEDS_CHECK);
}

/// A cut at every block the first journaled mount writes: the next one
/// writes over a verified journal, or makes the first one on a card left
/// as it was, or reads only and leaves the card as it is. Never a second
/// journal, never a changed file.
#[test]
fn a_cut_during_the_first_mount_never_loses_or_doubles_anything() {
    let bytes = library();
    let mut image = bytes.0.borrow().clone();
    let initial = image.clone();
    let recorded = Recorded {
        data: Rc::new(RefCell::new(image.clone())),
        writes: Rc::new(RefCell::new(Vec::new())),
    };
    let card = journaled(recorded.clone());
    assert_eq!(card.write_refusal(), None);
    drop(card);
    let free_before = exfat(&copy(&initial)).free_space().unwrap();
    let journal = free_before - exfat(&copy(&recorded.data.borrow())).free_space().unwrap();
    assert!(journal > 0);
    let (mut opened, mut refused) = (0, 0);
    for write in std::iter::once(None).chain(recorded.writes.borrow().iter().map(Some)) {
        if let Some((index, block)) = write {
            let at = *index as usize * 512;
            image[at..at + 512].copy_from_slice(block);
        }
        let cut = copy(&image);
        let card = journaled(cut.clone());
        let lost = free_before - card.free_space().unwrap();
        assert!(lost <= journal, "a second journal: {lost} > {journal}");
        match card.write_refusal() {
            None => {
                assert_eq!(read(&card, "kept.txt"), b"safe content");
                card.write("kept.txt", b"after").unwrap();
                card.sync().unwrap();
                drop(card);
                assert_eq!(read(&exfat(&cut), "kept.txt"), b"after");
                opened += 1;
            }
            Some(why) => {
                assert_eq!(why, NEEDS_CHECK);
                drop(card);
                reads_only(&cut, NEEDS_CHECK);
                refused += 1;
            }
        }
    }
    eprintln!("first-mount cuts: {opened} wrote, {refused} read only");
    assert!(opened > 0 && refused > 0);
}

#[derive(Default)]
struct WriteFault {
    writes: usize,
    fail: Option<usize>,
    hit: bool,
}

/// Fails exactly one block write, then behaves like the card again.
struct OneShotWrite {
    bytes: Bytes,
    fault: Rc<RefCell<WriteFault>>,
}

impl Blocks for OneShotWrite {
    fn read_block(
        &mut self,
        index: u32,
        block: &mut [u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.bytes.read_block(index, block)
    }

    fn write_block(
        &mut self,
        index: u32,
        block: &[u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        let mut state = self.fault.borrow_mut();
        let write = state.writes;
        state.writes += 1;
        if state.fail == Some(write) {
            state.hit = true;
            return Err(paperpaw_fat::DiskError);
        }
        drop(state);
        self.bytes.write_block(index, block)
    }

    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        self.bytes.blocks()
    }
}

/// One failed write during the first journaled mount: writes are refused
/// for that mount; later mounts write only over a verified journal or on
/// an untouched card, and never take a second one.
#[test]
fn a_write_failure_during_the_first_mount_closes_writes_without_a_second_journal() {
    let initial = {
        let bytes = library();
        // Bigger than the block cache, so reads evict every slot.
        let card = exfat(&bytes);
        card.write("big.bin", &[0x5a; 64 * 1024]).unwrap();
        card.sync().unwrap();
        drop(card);
        bytes.0.borrow().clone()
    };
    let free_before = exfat(&copy(&initial)).free_space().unwrap();
    let journal = {
        let fresh = copy(&initial);
        drop(journaled(fresh.clone()));
        free_before - exfat(&fresh).free_space().unwrap()
    };
    let mut checked = 0;
    for fail in 0..1000 {
        let card = copy(&initial);
        let fault = Rc::new(RefCell::new(WriteFault {
            fail: Some(fail),
            ..WriteFault::default()
        }));
        let storage = journaled(OneShotWrite {
            bytes: card.clone(),
            fault: fault.clone(),
        });
        let refusal = storage.write_refusal();
        if !fault.borrow().hit {
            assert_eq!(refusal, None);
            break;
        }
        // Refused for this mount, also a write after it. Reads that evict
        // every cache slot write nothing (Senior M1): what the failed
        // setup left in the cache is gone, reads see the card.
        assert!(refusal.is_some(), "write {fail}");
        let (writes, on_card) = (fault.borrow().writes, card.0.borrow().clone());
        assert_eq!(read(&storage, "big.bin"), vec![0x5a; 64 * 1024]);
        assert_eq!(read(&storage, "kept.txt"), b"safe content");
        assert_eq!(read(&storage, "books/a.epub"), b"book a");
        assert_eq!(storage.write("x.txt", b"x"), Err(refusal.unwrap()));
        assert_eq!(storage.sync(), Ok(()));
        drop(storage);
        assert_eq!(
            fault.borrow().writes,
            writes,
            "write {fail}: refused mode wrote"
        );
        assert!(
            *card.0.borrow() == on_card,
            "write {fail}: refused mode changed the card"
        );
        let untouched = *card.0.borrow() == initial;
        for _ in 0..3 {
            let before = card.0.borrow().clone();
            let again = journaled(card.clone());
            match again.write_refusal() {
                None => {}
                Some(why) => {
                    assert_eq!(why, NEEDS_CHECK, "write {fail}");
                    assert!(!untouched, "write {fail}: an untouched card refused");
                    drop(again);
                    assert!(*card.0.borrow() == before, "write {fail}");
                }
            }
        }
        let lost = free_before - exfat(&card).free_space().unwrap();
        assert!(lost <= journal, "write {fail}: {lost} > {journal}");
        assert_eq!(read(&exfat(&card), "kept.txt"), b"safe content");
        assert_eq!(read(&exfat(&card), "books/a.epub"), b"book a");
        checked += 1;
        assert!(fail < 999, "the sweep must reach a mount without failure");
    }
    assert!(checked > 0);
    eprintln!("first-mount one-shot write failures checked: {checked}");
}
