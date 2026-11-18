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
use paperpaw_fat::fatfs::{FatType, FormatVolumeOptions, Write as _, format_volume};
use paperpaw_fat::{
    AGAIN_MS, BLOCK, BlockDisk, Blocks, CALM_MS, CardSwap, DiskError, LOST_AFTER, MOUNT_TRIES,
    PROBE_MS, RETRY_MS, Restart, Swappable, Swapped, VolumeError, Watched,
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
    Ok(Box::new(paperpaw_fat::mount(blocks)?))
}

thread_local! {
    /// The serial number of the card made next: each card its own, as
    /// formatting gives them.
    static SERIAL: std::cell::Cell<u32> = const { std::cell::Cell::new(0x5a5a_0001) };
}

fn serial() -> u32 {
    SERIAL.with(|next| next.replace(next.get() + 1))
}

/// A FAT card of 4 MB (FAT12: no FS information sector) with `files` on
/// it, as a computer left it.
fn card_with(files: &[(&str, &[u8])]) -> Rc<RefCell<Vec<u8>>> {
    let card = Card::new(4 << 20);
    for (path, data) in files {
        card.storage.write(path, data).unwrap();
    }
    card.storage.sync().unwrap();
    let mut bytes = card.bytes.0.borrow().clone();
    // Its serial number, where FAT12 and FAT16 keep it.
    bytes[0x27..0x2b].copy_from_slice(&serial().to_le_bytes());
    Rc::new(RefCell::new(bytes))
}

/// A FAT32 card of 40 MB with `files` on it, as a computer left it.
fn card32_with(files: &[(&str, &[u8])]) -> Rc<RefCell<Vec<u8>>> {
    let bytes = common::Bytes(Rc::new(RefCell::new(vec![0u8; 40 << 20])));
    let mut disk = BlockDisk::new(bytes.clone()).unwrap();
    let options = FormatVolumeOptions::new()
        .fat_type(FatType::Fat32)
        .volume_id(serial());
    format_volume(&mut disk, options).unwrap();
    disk.flush().unwrap();
    let card = Card::mount(bytes);
    for (path, data) in files {
        card.storage.write(path, data).unwrap();
    }
    card.storage.sync().unwrap();
    // `Card` clears its bytes as it goes: kept first.
    let bytes = card.bytes.0.borrow().clone();
    Rc::new(RefCell::new(bytes))
}

/// The card loses its power and has it back (a loose contact, or taken
/// out and put back), found at the next use.
fn power_lost(sim: &Sim, card: &Rc<RefCell<Vec<u8>>>, storage: &Swappable, swap: &Swap) {
    sim.take_out();
    sim.put_in(card);
    until_lost(storage, swap);
}

type Swap = CardSwap<Sim, VolumeError>;

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
fn every_swap_in_one_run_is_mounted_anew_and_the_old_mount_writes_nothing_as_it_goes() {
    let first = card_with(&[("one.txt", b"1")]);
    let sim = Sim::with(&first);
    let (mut swap, storage) = mounted(&sim);
    sim.take_out();
    until_lost(&storage, &swap);
    let second = card_with(&[("two.txt", b"2")]);
    sim.put_in(&second);
    assert_eq!(swap.check(0), Swapped::Remounted);
    assert_eq!(read(&storage, "two.txt"), b"2");
    // Kept back in the caches, never written to the second card.
    storage.append("two.txt", b"dirty").unwrap();
    sim.take_out();
    until_lost(&storage, &swap);
    let third = card_with(&[("three.txt", b"3")]);
    let before = third.borrow().clone();
    let writes = sim.writes();
    sim.put_in(&third);
    assert_eq!(swap.check(RETRY_MS), Swapped::Remounted);
    assert_eq!(sim.writes(), writes, "the old mount wrote as it went");
    assert_eq!(*third.borrow(), before);
    assert_eq!(read(&storage, "three.txt"), b"3");
    assert_eq!(storage.size("two.txt"), Err(StorageError::NotFound));
    // Everything the mounts took is given back with the storage.
    drop(storage);
    drop(swap);
    assert_eq!(Rc::strong_count(&sim.0), 1, "an old mount kept the slot");
}

/// The bytes this thread has taken from the heap and not given back.
mod heap {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static LIVE: Cell<isize> = const { Cell::new(0) };
    }

    struct Counting;

    #[global_allocator]
    static COUNTING: Counting = Counting;

    fn count(bytes: usize, sign: isize) {
        let _ = LIVE.try_with(|live| live.set(live.get() + sign * bytes as isize));
    }

    // SAFETY: every call is passed on to the system's allocator as it came.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count(layout.size(), 1);
            // SAFETY: as the caller's.
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            count(layout.size(), -1);
            // SAFETY: as the caller's.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            count(layout.size(), -1);
            count(new_size, 1);
            // SAFETY: as the caller's.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    pub fn live() -> isize {
        LIVE.with(Cell::get)
    }
}

#[test]
fn a_card_swapped_again_and_again_takes_the_heap_of_the_first_mount() {
    let files: [(&str, &[u8]); 2] = [("books/a.txt", b"one"), ("books/b.txt", b"two")];
    let cards = [card_with(&files), card_with(&files)];
    let sim = Sim::with(&cards[0]);
    // The same use of each mount: a folder listed, files read and written.
    let used = |storage: &Swappable| {
        assert_eq!(names(storage, "books"), ["a.txt", "b.txt"]);
        assert_eq!(read(storage, "books/a.txt"), b"one");
        storage.write("books/c.txt", b"three").unwrap();
        storage.remove("books/c.txt").unwrap();
        storage.sync().unwrap();
    };
    let (mut swap, storage) = mounted(&sim);
    used(&storage);
    let first = heap::live();
    for round in 1..=8 {
        sim.take_out();
        sim.put_in(&cards[round % 2]);
        // Found at a use, or by the look at the card on waking.
        if round % 2 == 0 {
            assert!(swap.probe(), "swap {round} while asleep not found");
        } else {
            until_lost(&storage, &swap);
        }
        let at = round as u64 * RETRY_MS;
        assert_eq!(swap.check(at), Swapped::Remounted, "swap {round}");
        used(&storage);
        assert_eq!(heap::live(), first, "heap after swap {round}");
    }
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

#[test]
fn a_card_that_failed_for_a_while_and_never_left_keeps_its_mount_and_tells_no_one() {
    let card = card_with(&[("a.txt", b"a")]);
    let sim = Sim::with(&card);
    let (mut swap, storage) = mounted(&sim);
    // Appended, kept back in the caches: a new mount would lose it.
    storage.append("log.txt", b"kept back").unwrap();
    // What the shell does of a check: only `Remounted` has the kernel walk
    // the library and start the time to sleep anew.
    let mut told = 0;
    for round in 0..20u64 {
        // The card is in, started, and fails for a while: by the look at
        // it after a pause (twice), or by reads in a row.
        sim.0.borrow_mut().fail_next = 2;
        until_lost(&storage, &swap);
        let said = swap.check(round);
        told += u32::from(said == Swapped::Remounted);
        assert_eq!(said, Swapped::Kept, "round {round}");
        assert!(!swap.lost());
        assert_eq!(swap.check(round), Swapped::No);
    }
    assert_eq!(told, 0, "a card that never left was mounted anew");
    // Its mount is the one it had: what it kept back reaches the card.
    assert_eq!(read(&storage, "log.txt"), b"kept back");
    storage.sync().unwrap();
    let after = Card::mount(common::Bytes(Rc::new(RefCell::new(card.borrow().clone()))));
    assert_eq!(read(after.storage, "log.txt"), b"kept back");
}

#[test]
fn the_same_card_back_as_it_was_left_is_not_walked_and_is_left_longer_each_time() {
    let card = card32_with(&[("books/a.epub", b"a")]);
    let sim = Sim::with(&card);
    let (mut swap, storage) = mounted(&sim);
    assert_eq!(names(&storage, "books"), ["a.epub"]);
    let mut told = 0;
    let mut at = 0u64;
    // A loose contact: the card loses its power again and again.
    for (round, wait) in AGAIN_MS.iter().chain(&[AGAIN_MS[3]; 2]).enumerate() {
        power_lost(&sim, &card, &storage, &swap);
        let said = swap.check(at);
        told += u32::from(said == Swapped::Remounted);
        assert_eq!(said, Swapped::Back, "round {round}");
        assert_eq!(read(&storage, "books/a.epub"), b"a");
        // Lost again at once: left for longer each time.
        power_lost(&sim, &card, &storage, &swap);
        at += wait;
        assert_eq!(swap.check(at - 1), Swapped::Waiting, "round {round}");
    }
    assert_eq!(told, 0, "the same card was told as another one");
    // Another card in its place is told, and ends the row.
    let other = card32_with(&[("books/b.epub", b"b")]);
    sim.take_out();
    sim.put_in(&other);
    assert_eq!(swap.check(at), Swapped::Remounted);
    assert_eq!(names(&storage, "books"), ["b.epub"]);
    power_lost(&sim, &other, &storage, &swap);
    assert_eq!(swap.check(at + RETRY_MS), Swapped::Back);
    // A mount that held for long ends the row too.
    power_lost(&sim, &other, &storage, &swap);
    assert_eq!(swap.check(at + RETRY_MS + CALM_MS), Swapped::Back);
    power_lost(&sim, &other, &storage, &swap);
    let then = at + RETRY_MS + CALM_MS + RETRY_MS;
    assert_eq!(swap.check(then - 1), Swapped::Waiting);
    assert_eq!(swap.check(then), Swapped::Back);
}

#[test]
fn the_same_card_written_elsewhere_or_by_this_mount_since_is_told() {
    let card = card32_with(&[("books/a.epub", b"a")]);
    let sim = Sim::with(&card);
    let (mut swap, storage) = mounted(&sim);
    assert_eq!(names(&storage, "books"), ["a.epub"]);
    // On a computer: a book added. It takes clusters and says so in the FS
    // information sector (Linux moves its hint, 4 bytes at 492; Windows
    // and macOS write their count).
    sim.take_out();
    {
        let elsewhere = Card::mount(common::Bytes(card.clone()));
        elsewhere.storage.write("books/b.epub", b"b").unwrap();
        elsewhere.storage.sync().unwrap();
        let mut bytes = elsewhere.bytes.0.borrow().clone();
        drop(elsewhere);
        let hint = BLOCK + 492;
        bytes[hint] = bytes[hint].wrapping_add(1);
        *card.borrow_mut() = bytes;
    }
    sim.put_in(&card);
    until_lost(&storage, &swap);
    assert_eq!(swap.check(0), Swapped::Remounted);
    assert_eq!(names(&storage, "books"), ["a.epub", "b.epub"]);
    // What this mount wrote to the sector is the card as it was left: the
    // card back after it is the same card.
    storage.write("books/c.epub", b"c").unwrap();
    // The free space, counted (a part of the FAT at each ask), is kept in
    // the sector by the sync.
    assert!((0..8).any(|_| storage.free_space().is_some()));
    storage.sync().unwrap();
    power_lost(&sim, &card, &storage, &swap);
    assert_eq!(swap.check(AGAIN_MS[0]), Swapped::Back);
    assert_eq!(names(&storage, "books"), ["a.epub", "b.epub", "c.epub"]);
}
