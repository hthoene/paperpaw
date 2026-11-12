#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! What carrying a book's own settings costs on a real FAT card
//! (`paperpaw-fat`, blocks in memory, the storage as on the X4): 500 books
//! moved one by one, a tenth of them with a settings file of the reader,
//! among the folders of 30 plugins. The card's block reads and writes are
//! counted (the emulator uses the computer's file system, so this is the
//! closer measure of the X4; its time per block is measured on the device).
//!
//! Run: `cargo test -p paperpaw-core --lib fat_tests::moving_500 -- --ignored --nocapture`.
extern crate std;
use super::*;
use crate::native_host::book_data_steps::move_data;
use core::cell::{Cell, RefCell};
use paperpaw_fat::{
    BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk,
    fatfs::{FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write},
};
use paperpaw_platform::Storage;
use std::{boxed::Box, format, println, rc::Rc, string::String, vec, vec::Vec};

/// Books moved.
const BOOKS: usize = 500;
/// One book in this many has a settings file of the reader.
const WITH_SETTINGS: usize = 10;
/// Plugin folders on the card (the reader among them).
const PLUGINS: usize = 30;

/// A card in memory that counts the blocks read and written.
#[derive(Clone)]
struct Ram {
    bytes: Rc<RefCell<Vec<u8>>>,
    reads: Rc<Cell<u64>>,
    writes: Rc<Cell<u64>>,
}

impl Blocks for Ram {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.bytes.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        self.reads.set(self.reads.get() + 1);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.bytes
            .borrow_mut()
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.bytes.borrow().len() / BLOCK) as u32)
    }
}

/// A formatted 32 MB card and the storage on it, as on the X4.
fn card() -> (Ram, FatStorage<SharedDisk<Ram>>) {
    let ram = Ram {
        bytes: Rc::new(RefCell::new(vec![0u8; 32 << 20])),
        reads: Rc::default(),
        writes: Rc::default(),
    };
    let mut disk = BlockDisk::new(ram.clone()).unwrap();
    paperpaw_fat::fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
    disk.flush().unwrap();
    let mut disk = BlockDisk::new(ram.clone()).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    let shared = SharedDisk::new(disk);
    let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
    let storage = FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared);
    (ram, storage)
}

/// The path of book `n`, before (`old`) or after a move.
fn book(n: usize, old: bool) -> String {
    format!("{}/b{n:03}.epub", if old { "in" } else { "out" })
}

/// Blocks read and written while `run` runs.
fn counted(ram: &Ram, run: impl FnOnce()) -> (u64, u64) {
    let (reads, writes) = (ram.reads.get(), ram.writes.get());
    run();
    (ram.reads.get() - reads, ram.writes.get() - writes)
}

#[test]
#[ignore = "measurement (#257): run with --ignored --nocapture"]
fn moving_500_books_one_by_one_on_a_fat_card() {
    let (ram, storage) = card();
    for n in 0..PLUGINS {
        let plugin = if n == 0 {
            String::from("reader")
        } else {
            format!("plugin-{n:02}")
        };
        storage
            .create_dir(&format!(".paperpaw/plugins/{plugin}"))
            .unwrap();
    }
    for n in (0..BOOKS).step_by(WITH_SETTINGS) {
        let path = file("reader", &book(n, true)).unwrap();
        storage.write(&path, b"[core]\nscope-only = on\n").unwrap();
    }
    let (mut with, mut without) = ((0, 0, 0), (0, 0, 0));
    let started = std::time::Instant::now();
    for n in 0..BOOKS {
        let (from, to) = (book(n, true), book(n, false));
        let (reads, writes) = counted(&ram, || move_data(&storage, &[], &from, &to).unwrap());
        let sum = if n % WITH_SETTINGS == 0 {
            &mut with
        } else {
            &mut without
        };
        *sum = (sum.0 + 1, sum.1 + reads, sum.2 + writes);
    }
    let took = started.elapsed();
    let moved = (0..BOOKS)
        .step_by(WITH_SETTINGS)
        .filter(|&n| storage.exists(&file("reader", &book(n, false)).unwrap()))
        .count();
    assert_eq!(moved, BOOKS / WITH_SETTINGS);
    for (what, (books, reads, writes)) in [("with a file", with), ("without", without)] {
        println!(
            "{what}: {books} books, {:.1} blocks read and {:.1} written per book",
            reads as f64 / books as f64,
            writes as f64 / books as f64,
        );
    }
    println!("{BOOKS} moves, {PLUGINS} plugin folders: {took:?} on this computer");
}

#[test]
fn a_move_on_a_fat_card_reaches_the_new_key() {
    let (_, storage) = card();
    let (old, new) = (
        file("reader", &book(1, true)).unwrap(),
        file("reader", &book(1, false)).unwrap(),
    );
    storage.write(&old, b"[core]\n").unwrap();
    move_data(&storage, &[], &book(1, true), &book(1, false)).unwrap();
    assert!(!storage.exists(&old), "gone from the old key");
    assert_eq!(
        paperpaw_sdk::read_file(&storage, &new, 64).unwrap(),
        b"[core]\n"
    );
}
