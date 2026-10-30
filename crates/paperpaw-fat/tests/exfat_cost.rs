//! What exFAT costs in time and memory, measured on the host: blocks read
//! to count the free space of big cards (the allocation bitmap), and the
//! heap the storage takes against the FAT storage on the same work.
//! Prints a table; asserts only the bounds the design claims.
//!
//! The big cards are sparse images made by `mkfs.exfat` (not in the
//! repository), so the test is `#[ignore]`d:
//!
//! ```sh
//! truncate -s 512G /tmp/big.img && mkfs.exfat -c 128K /tmp/big.img
//! PAPERPAW_EXFAT_BIG=/tmp/big.img cargo test -p paperpaw-fat --test exfat_cost \
//!     -- --ignored --nocapture
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::io::{Read, Seek, SeekFrom, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, ExFatStorage, SharedDisk};
use paperpaw_platform::Storage;

mod common;

struct Count;
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator, counting the bytes.
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = NOW.fetch_add(layout.size(), Relaxed) + layout.size();
        PEAK.fetch_max(now, Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        NOW.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNT: Count = Count;

/// The heap in use now, and the highest since the last [`mark`].
fn heap() -> (usize, usize) {
    (NOW.load(Relaxed), PEAK.load(Relaxed))
}

/// Starts a measurement: the peak is the heap now.
fn mark() -> usize {
    let now = NOW.load(Relaxed);
    PEAK.store(now, Relaxed);
    now
}

/// A card in a file, counting the blocks read and written.
#[derive(Clone)]
struct FileCard {
    file: Rc<RefCell<std::fs::File>>,
    reads: Rc<Cell<u64>>,
    writes: Rc<Cell<u64>>,
}

impl Blocks for FileCard {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.reads.set(self.reads.get() + 1);
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(u64::from(index) * BLOCK as u64))
            .map_err(|_| DiskError)?;
        file.read_exact(block).map_err(|_| DiskError)
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.writes.set(self.writes.get() + 1);
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(u64::from(index) * BLOCK as u64))
            .map_err(|_| DiskError)?;
        file.write_all(block).map_err(|_| DiskError)
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        let bytes = self.file.borrow().metadata().map_err(|_| DiskError)?.len();
        u32::try_from(bytes / BLOCK as u64).map_err(|_| DiskError)
    }
}

/// The same small work on any storage: a folder, files, a listing, a read.
fn work(s: &dyn Storage) {
    s.create_dir("books").unwrap();
    for n in 0..20 {
        s.write(&format!("books/book-{n:02}.txt"), &[b'x'; 4096])
            .unwrap();
    }
    let mut listed = 0;
    s.list("books", &mut |_| listed += 1).unwrap();
    assert!(listed >= 20);
    let mut buf = [0u8; 4096];
    s.read_at("books/book-07.txt", 0, &mut buf).unwrap();
    s.write("books/progress.ini", b"[a]\nb=1\n").unwrap();
    s.sync().unwrap();
}

#[test]
#[ignore = "needs a big sparse image: see the module's documentation"]
fn what_exfat_costs_on_big_cards() {
    let path = std::env::var("PAPERPAW_EXFAT_BIG").expect("PAPERPAW_EXFAT_BIG");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let size = file.metadata().unwrap().len();
    let card = FileCard {
        file: Rc::new(RefCell::new(file)),
        reads: Rc::default(),
        writes: Rc::default(),
    };
    let base = mark();
    let (disk, _) = BlockDisk::find(card.clone()).unwrap();
    let storage = ExFatStorage::mount(SharedDisk::new(disk)).unwrap();
    let mount = (heap().1 - base, card.reads.get());
    let kept = heap().0.saturating_sub(base);
    let total = storage.total_space().unwrap();
    let (r0, t0) = (card.reads.get(), Instant::now());
    let free = storage.free_space().unwrap();
    let scan = (card.reads.get() - r0, t0.elapsed());
    let (r1, t1) = (card.reads.get(), Instant::now());
    let again = storage.free_space().unwrap();
    let second = (card.reads.get() - r1, t1.elapsed());
    assert_eq!(free, again);
    let base = mark();
    let (w0, r2) = (card.writes.get(), card.reads.get());
    work(&storage);
    let worked = (
        heap().1 - base,
        card.reads.get() - r2,
        card.writes.get() - w0,
    );
    // A file of 64 MiB, in pieces: the cost of finding free clusters.
    let base = mark();
    let r3 = card.reads.get();
    for _ in 0..4096 {
        storage.append("big.bin", &[7u8; 16 * 1024]).unwrap();
    }
    storage.sync().unwrap();
    let appended = (heap().1 - base, card.reads.get() - r3);
    println!(
        "| {} GiB ({} GiB in clusters) | mount: peak heap {} B, kept {} B, {} reads | free space: {} reads, {:?} on the host (a second query: {} reads) | work: heap {} B, {} reads, {} writes | 64 MiB appended: heap {} B, {} reads |",
        size >> 30,
        total >> 30,
        mount.0,
        kept,
        mount.1,
        scan.0,
        scan.1,
        second.0,
        worked.0,
        worked.1,
        worked.2,
        appended.0,
        appended.1,
    );
    // The start of the reader: a library of 500 files in one folder, the
    // card mounted anew, the folder listed (its first look settles it).
    for n in 0..500 {
        storage
            .write(&format!("library/book-{n:03}.epub"), b"b")
            .unwrap();
    }
    storage.sync().unwrap();
    drop(storage);
    let (r4, t4) = (card.reads.get(), Instant::now());
    let (disk, _) = BlockDisk::find(card.clone()).unwrap();
    let storage = ExFatStorage::mount(SharedDisk::new(disk)).unwrap();
    let mounted = (card.reads.get() - r4, t4.elapsed());
    let mut listed = 0;
    storage.list("library", &mut |_| listed += 1).unwrap();
    println!(
        "| {} GiB: start with 500 books: mount {} reads ({:?}), first listing of the folder {} more reads ({:?} in all, {listed} files) |",
        size >> 30,
        mounted.0,
        mounted.1,
        card.reads.get() - r4 - mounted.0,
        t4.elapsed(),
    );
    // The bitmap is streamed through one block: nothing of the card's size
    // is kept (the design's claim).
    assert!(mount.0 < 64 * 1024, "{} B to mount", mount.0);
    assert!(worked.0 < 64 * 1024, "{} B for a little work", worked.0);
}

#[test]
fn what_exfat_takes_of_the_heap_beyond_fat() {
    // Equal work on a small exFAT and a small FAT card, both in memory.
    let exfat_bytes = common::exfat_card();
    let base = mark();
    let (disk, _) = BlockDisk::find(exfat_bytes.clone()).unwrap();
    let storage = ExFatStorage::mount(SharedDisk::new(disk)).unwrap();
    work(&storage);
    let exfat = heap().1 - base;
    drop(storage);
    let formatted = common::Card::new(8 * 1024 * 1024);
    let copy = common::Bytes(Rc::new(RefCell::new(formatted.bytes.0.borrow().clone())));
    let base = mark();
    let fat = common::Card::mount(copy);
    work(fat.storage);
    let fat_peak = heap().1 - base;
    println!(
        "| exFAT mount and work: peak heap {exfat} B | FAT mount and work: {fat_peak} B | difference {} B |",
        exfat as i64 - fat_peak as i64
    );
    // The extra heap the design claims is about 8 KiB; far more is a leak
    // of the card's size into memory.
    assert!(exfat < 128 * 1024, "{exfat} B");
}
