#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! A place saved on a real FAT card (`paperpaw-fat`, blocks in memory, the
//! storage as on the X4): what a save writes, and what is read after a
//! power cut at each block of it (#305).
extern crate std;
use super::super::tests::card as catalog_card;
use super::*;
use crate::native_host::lib_service::LibWrite;
use alloc::rc::Rc;
use core::cell::RefCell;
use paperpaw_fat::{
    BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk,
    fatfs::{FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write},
};
use std::{boxed::Box, vec, vec::Vec};

const A: BookRef<'static> = BookRef::Path("books/a.epub");

/// A card in memory that notes every block written to it.
#[derive(Clone)]
struct Ram {
    bytes: Rc<RefCell<Vec<u8>>>,
    written: Rc<RefCell<Vec<(u32, [u8; BLOCK])>>>,
}

impl Blocks for Ram {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.bytes.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.bytes
            .borrow_mut()
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        self.written.borrow_mut().push((index, *block));
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.bytes.borrow().len() / BLOCK) as u32)
    }
}

/// The storage on the card `bytes`, as on the X4.
fn mount(bytes: Vec<u8>) -> (Ram, Rc<FatStorage<SharedDisk<Ram>>>) {
    let ram = Ram {
        bytes: Rc::new(RefCell::new(bytes)),
        written: Rc::default(),
    };
    let mut disk = BlockDisk::new(ram.clone()).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    let shared = SharedDisk::new(disk);
    let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
    let storage = FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared);
    (ram, Rc::new(storage))
}

/// A formatted 32 MB card.
fn formatted() -> Vec<u8> {
    let ram = Ram {
        bytes: Rc::new(RefCell::new(vec![0u8; 32 << 20])),
        written: Rc::default(),
    };
    let mut disk = BlockDisk::new(ram.clone()).unwrap();
    paperpaw_fat::fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
    disk.flush().unwrap();
    drop(disk);
    ram.bytes.take()
}

fn place(offset: u32) -> wire::ServiceLibPosition<'static> {
    wire::ServiceLibPosition {
        section: 7,
        offset,
        insert: 0,
        progress_ppm: 415_000,
        conversion: 255,
        updated: 1_760_000_000,
        xpointer: "",
        flags: 0b010,
    }
}

/// The offset of the place the card `bytes` holds for the book.
fn kept(bytes: Vec<u8>) -> Option<u32> {
    let (_, storage) = mount(bytes);
    positions::load(&*storage, "books/a.epub").map(|p| p.at.offset)
}

/// A page turned: the place is saved with one block written, and cut off
/// before or after it the card holds the place before or the new one. The
/// first save of a book writes the file whole, as every save did.
#[test]
fn a_place_saved_again_is_one_block_and_old_or_new_after_a_cut() {
    let env = catalog_card().env;
    let (ram, storage) = mount(formatted());
    storage.write("books/a.epub", &[7u8; 3000]).unwrap();
    storage.sync().unwrap();
    let reads = CatalogReads::new(storage.clone(), env);
    ram.written.borrow_mut().clear();
    reads.write_position(A, &place(1), false).unwrap();
    storage.sync().unwrap();
    let first = ram.written.borrow().len();
    assert!(first > 8, "{first} blocks for a file written whole");
    for turn in 2..=9u32 {
        let before = ram.bytes.borrow().clone();
        ram.written.borrow_mut().clear();
        reads.write_position(A, &place(turn), false).unwrap();
        storage.sync().unwrap();
        let written = core::mem::take(&mut *ram.written.borrow_mut());
        assert_eq!(written.len(), 1, "blocks written for save {turn}");
        // Cut off before the block: the place before. After it: the new one.
        assert_eq!(kept(before.clone()), Some(turn - 1));
        let mut after = before;
        let (index, block) = written[0];
        let at = index as usize * BLOCK;
        after[at..at + BLOCK].copy_from_slice(&block);
        assert_eq!(kept(after.clone()), Some(turn));
        // A block that is neither (not written at once): the place before.
        after[at + 20..at + 60].fill(b'#');
        assert_eq!(kept(after), Some(turn - 1));
    }
    std::println!("first save: {first} blocks written; each save after: 1");
}
