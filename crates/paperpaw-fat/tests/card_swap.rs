//! A card taken out and put back while PaperPaw runs (#145): the old
//! card's state never reaches the card in the slot, the new card is read
//! as it is, and a card that stays out fails without end or harm.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Card, names, read};
use paperpaw_fat::fatfs::{FileSystem, FsOptions};
use paperpaw_fat::{
    BLOCK, BlockDisk, Blocks, CardSwap, DiskError, FatStorage, LOST_AFTER, MOUNT_TRIES, PROBE_MS,
    RETRY_MS, Restart, SharedDisk, Swappable, Swapped, VolumeError, Watched,
};
use paperpaw_platform::{Storage, StorageError};

/// The slot of a device and the card in it, as a card behaves: taken out
/// it loses its power, put in it answers nothing until it is started.
#[derive(Default)]
struct Slot {
    card: Option<Rc<RefCell<Vec<u8>>>>,
    started: bool,
    /// Reads and writes that reached a card.
    reads: u32,
    writes: u32,
    /// The next reads and writes that fail, the card there (a glitch).
    fail_next: u32,
}

thread_local! {
    /// The shell's clock, as the tests move it (ms).
    static NOW: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn clock() -> u64 {
    NOW.with(std::cell::Cell::get)
}

/// Time passes, as while a card is taken out and put back.
fn later() {
    NOW.with(|now| now.set(now.get() + PROBE_MS));
}

#[derive(Clone, Default)]
struct Sim(Rc<RefCell<Slot>>);

impl Sim {
    fn with(card: &Rc<RefCell<Vec<u8>>>) -> Self {
        let sim = Self::default();
        sim.put_in(card);
        sim.0.borrow_mut().started = true;
        sim
    }

    fn take_out(&self) {
        let mut slot = self.0.borrow_mut();
        slot.card = None;
        slot.started = false;
    }

    fn put_in(&self, card: &Rc<RefCell<Vec<u8>>>) {
        let mut slot = self.0.borrow_mut();
        slot.card = Some(card.clone());
        slot.started = false;
    }

    fn writes(&self) -> u32 {
        self.0.borrow().writes
    }

    /// The card in the slot, where it answers.
    fn answering(&self) -> Result<Rc<RefCell<Vec<u8>>>, DiskError> {
        let mut slot = self.0.borrow_mut();
        if slot.fail_next > 0 {
            slot.fail_next -= 1;
            return Err(DiskError);
        }
        match &slot.card {
            Some(card) if slot.started => Ok(card.clone()),
            _ => Err(DiskError),
        }
    }
}

impl Blocks for Sim {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let card = self.answering()?;
        let at = index as usize * BLOCK;
        block.copy_from_slice(card.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        self.0.borrow_mut().reads += 1;
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let card = self.answering()?;
        let at = index as usize * BLOCK;
        card.borrow_mut()
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        self.0.borrow_mut().writes += 1;
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.answering()?.borrow().len() / BLOCK) as u32)
    }
}

impl Restart for Sim {
    fn restart(&mut self) -> Result<(), DiskError> {
        let mut slot = self.0.borrow_mut();
        if slot.card.is_none() {
            return Err(DiskError);
        }
        slot.started = true;
        Ok(())
    }
}

/// The card's FAT file system as storage, as the X4 mounts it
/// (`shells/xteink/src/volume.rs`).
fn mount(blocks: Watched<Sim>) -> Result<Box<dyn Storage>, VolumeError> {
    let disk = SharedDisk::new(BlockDisk::volume(blocks)?);
    let fs = FileSystem::new(disk.clone(), FsOptions::new()).map_err(|_| VolumeError::NoFat)?;
    Ok(Box::new(
        FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk),
    ))
}

/// A FAT card of 4 MB with `files` on it, as a computer left it.
fn card_with(files: &[(&str, &[u8])]) -> Rc<RefCell<Vec<u8>>> {
    let card = Card::new(4 << 20);
    for (path, data) in files {
        card.storage.write(path, data).unwrap();
    }
    card.storage.sync().unwrap();
    let bytes = card.bytes.0.borrow().clone();
    Rc::new(RefCell::new(bytes))
}

fn mounted(sim: &Sim) -> (CardSwap<Sim, VolumeError>, Swappable) {
    CardSwap::mount(sim.clone(), mount, clock).unwrap()
}

/// The storage used after a while, as after a card was swapped: the card
/// counts as lost at that use, though the caches would answer it.
fn until_lost(storage: &Swappable, swap: &CardSwap<Sim, VolumeError>) {
    later();
    let _ = storage.exists("a.txt");
    assert!(swap.lost(), "the card never counted as lost");
}

#[test]
fn a_card_swapped_for_another_is_read_anew_and_never_written_with_the_old_ones_state() {
    let old = card_with(&[("books/old.epub", b"old book")]);
    let sim = Sim::with(&old);
    let (mut swap, storage) = mounted(&sim);
    assert_eq!(names(&storage, "books"), ["old.epub"]);
    // Written, but kept back in the caches: never synced to the old card.
    storage.write("books/old.epub.note", b"pending").unwrap();
    sim.take_out();
    until_lost(&storage, &swap);
    let new = card_with(&[("books/new.epub", b"new book"), ("other.txt", b"x")]);
    let before = new.borrow().clone();
    sim.put_in(&new);
    // Everything kept of the old card goes nowhere: writes, a sync.
    assert!(storage.write("books/old.epub.note", b"again").is_err());
    let _ = storage.sync();
    assert_eq!(*new.borrow(), before, "the old mount wrote to the new card");
    let writes = sim.writes();
    assert_eq!(swap.check(0), Swapped::Remounted);
    assert_eq!(sim.writes(), writes, "mounting wrote to the card");
    assert_eq!(*new.borrow(), before);
    // The new card, as it is.
    assert_eq!(names(&storage, "books"), ["new.epub"]);
    assert_eq!(read(&storage, "books/new.epub"), b"new book");
    assert_eq!(
        storage.size("books/old.epub.note"),
        Err(StorageError::NotFound)
    );
    // Written to from now on, as any card.
    storage.write("books/new.epub.note", b"mine").unwrap();
    storage.sync().unwrap();
    let after = Card::mount(common::Bytes(Rc::new(RefCell::new(new.borrow().clone()))));
    assert_eq!(read(after.storage, "books/new.epub.note"), b"mine");
    assert_eq!(read(after.storage, "other.txt"), b"x");
}

#[test]
fn the_same_card_put_back_changed_shows_what_a_computer_added() {
    let card = card_with(&[("books/a.epub", b"a")]);
    let sim = Sim::with(&card);
    let (mut swap, storage) = mounted(&sim);
    assert_eq!(names(&storage, "books"), ["a.epub"]);
    sim.take_out();
    // On a computer: books added.
    {
        let elsewhere = Card::mount(common::Bytes(card.clone()));
        elsewhere.storage.write("books/b.epub", b"b").unwrap();
        elsewhere.storage.write("books/c.epub", b"c").unwrap();
        elsewhere.storage.sync().unwrap();
        let bytes = elsewhere.bytes.0.borrow().clone();
        // `Card` clears its bytes as it goes: kept first.
        drop(elsewhere);
        *card.borrow_mut() = bytes;
    }
    sim.put_in(&card);
    until_lost(&storage, &swap);
    assert_eq!(swap.check(0), Swapped::Remounted);
    assert_eq!(names(&storage, "books"), ["a.epub", "b.epub", "c.epub"]);
}

#[test]
fn a_card_that_stays_out_fails_and_is_tried_again_now_and_then() {
    let card = card_with(&[("a.txt", b"a")]);
    let sim = Sim::with(&card);
    let (mut swap, storage) = mounted(&sim);
    sim.take_out();
    until_lost(&storage, &swap);
    assert_eq!(swap.check(0), Swapped::Waiting);
    // Not tried again before its time.
    assert_eq!(swap.check(RETRY_MS - 1), Swapped::Waiting);
    assert_eq!(swap.check(RETRY_MS), Swapped::Waiting);
    // The storage fails, without reaching a card.
    assert!(storage.read_at("a.txt", 0, &mut [0; 1]).is_err());
    assert!(storage.write("b.txt", b"b").is_err());
    assert!(!storage.exists("a.txt"));
    // Put back later: mounted then.
    sim.put_in(&card);
    assert_eq!(swap.check(2 * RETRY_MS), Swapped::Remounted);
    assert_eq!(read(&storage, "a.txt"), b"a");
}

#[test]
fn a_second_swap_in_one_run_is_given_up_and_reaches_no_card() {
    let first = card_with(&[("one.txt", b"1")]);
    let sim = Sim::with(&first);
    let (mut swap, storage) = mounted(&sim);
    sim.take_out();
    until_lost(&storage, &swap);
    let second = card_with(&[("two.txt", b"2")]);
    sim.put_in(&second);
    assert_eq!(swap.check(0), Swapped::Remounted);
    assert_eq!(read(&storage, "two.txt"), b"2");
    sim.take_out();
    until_lost(&storage, &swap);
    let third = card_with(&[("three.txt", b"3")]);
    let before = third.borrow().clone();
    sim.put_in(&third);
    for at in 0..5 {
        assert_eq!(swap.check(at * RETRY_MS), Swapped::GivenUp);
    }
    // Said as such, so that the library asks for a restart.
    assert_eq!(storage.write("x.txt", b"x"), Err(StorageError::CARD_GONE));
    assert_eq!(storage.sync(), Err(StorageError::CARD_GONE));
    assert_eq!(storage.size("two.txt"), Err(StorageError::CARD_GONE));
    assert_eq!(storage.list("", &mut |_| {}), Err(StorageError::CARD_GONE));
    assert_eq!(*third.borrow(), before);
}

#[test]
fn a_few_failures_do_not_count_as_a_card_taken_out() {
    // A file far larger than the caches: each read below reaches the card.
    let big: Vec<u8> = (0..256 * 1024u32).map(|i| (i / 512) as u8).collect();
    let card = card_with(&[("big.bin", &big)]);
    let sim = Sim::with(&card);
    let (mut swap, storage) = mounted(&sim);
    let mut at = 0u64;
    let mut far = || {
        at = (at + 37 * 512) % (256 * 1024);
        at
    };
    // Fewer failures in a row than a lost card's, then the card answers.
    for round in 0..4 {
        sim.0.borrow_mut().fail_next = LOST_AFTER - 1;
        let mut answered = false;
        for _ in 0..LOST_AFTER + 2 {
            let offset = far();
            let mut byte = [0u8; 1];
            if storage.read_at("big.bin", offset, &mut byte) == Ok(1) {
                assert_eq!(byte[0], (offset / 512) as u8);
                answered = true;
            }
        }
        assert!(answered && !swap.lost(), "round {round}");
    }
    // The probe after a pause reads a second time.
    sim.0.borrow_mut().fail_next = 1;
    later();
    assert_eq!(storage.size("big.bin"), Ok(big.len() as u64));
    assert!(!swap.lost());
    assert_eq!(swap.check(0), Swapped::No);
    // As many in a row as a lost card's: lost.
    sim.0.borrow_mut().fail_next = LOST_AFTER;
    for _ in 0..LOST_AFTER {
        let _ = storage.read_at("big.bin", far(), &mut [0; 1]);
    }
    assert!(swap.lost());
}

#[test]
fn a_card_swapped_while_asleep_is_found_on_waking() {
    let old = card_with(&[("a.txt", b"old")]);
    let sim = Sim::with(&old);
    let (mut swap, storage) = mounted(&sim);
    assert_eq!(read(&storage, "a.txt"), b"old");
    assert!(!swap.probe());
    sim.take_out();
    let new = card_with(&[("a.txt", b"new")]);
    sim.put_in(&new);
    // The caches would still give the old card's file: the probe reads
    // the card itself.
    assert!(swap.probe());
    assert_eq!(swap.check(0), Swapped::Remounted);
    assert_eq!(read(&storage, "a.txt"), b"new");
}

#[test]
fn a_card_put_back_without_a_file_system_is_given_up_after_some_tries() {
    let card = card_with(&[("a.txt", b"a")]);
    let sim = Sim::with(&card);
    let (mut swap, storage) = mounted(&sim);
    sim.take_out();
    until_lost(&storage, &swap);
    let blank = Rc::new(RefCell::new(vec![0u8; 4 << 20]));
    sim.put_in(&blank);
    let mut outcomes = Vec::new();
    for n in 0..u64::from(MOUNT_TRIES) + 2 {
        outcomes.push(swap.check(n * RETRY_MS));
    }
    let tries = usize::from(MOUNT_TRIES);
    assert!(outcomes[..tries - 1].iter().all(|o| *o == Swapped::Waiting));
    assert!(outcomes[tries - 1..].iter().all(|o| *o == Swapped::GivenUp));
    assert!(blank.borrow().iter().all(|&b| b == 0), "written to");
    assert_eq!(
        storage.read_at("a.txt", 0, &mut [0; 1]),
        Err(StorageError::CARD_GONE)
    );
}

/// A storage that writes as it goes, as a file system that unmounts.
struct WritesAsItGoes(Box<dyn Storage>);

impl Drop for WritesAsItGoes {
    fn drop(&mut self) {
        let _ = self.0.write("dropped.txt", b"from the old mount");
        let _ = self.0.sync();
    }
}

impl Storage for WritesAsItGoes {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
}

fn mount_writing_as_it_goes(blocks: Watched<Sim>) -> Result<Box<dyn Storage>, VolumeError> {
    Ok(Box::new(WritesAsItGoes(mount(blocks)?)))
}

#[test]
fn the_old_mount_going_writes_nothing_to_the_card_started_again() {
    let old = card_with(&[("a.txt", b"old")]);
    let sim = Sim::with(&old);
    let (mut swap, storage) =
        CardSwap::mount(sim.clone(), mount_writing_as_it_goes, clock).unwrap();
    assert_eq!(read(&storage, "a.txt"), b"old");
    sim.take_out();
    until_lost(&storage, &swap);
    let new = card_with(&[("a.txt", b"new")]);
    let before = new.borrow().clone();
    sim.put_in(&new);
    // The old mount is dropped once the new card answers: its writes then
    // are refused by its generation alone.
    assert_eq!(swap.check(0), Swapped::Remounted);
    assert_eq!(*new.borrow(), before, "the old mount wrote to the new card");
    assert_eq!(storage.size("dropped.txt"), Err(StorageError::NotFound));
    assert_eq!(read(&storage, "a.txt"), b"new");
}
