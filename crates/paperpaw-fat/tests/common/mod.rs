//! A card of blocks in memory, formatted with FAT, and a `FatStorage` on
//! it as the X4 has one: through a `BlockDisk` shared with the storage.
//! The file system lives for ever (`Box::leak`), so the card's bytes are
//! freed when the test's `Card` handle is dropped.

// Not every test file uses every helper.
#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_fat::fatfs::{FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, Write};
use paperpaw_fat::{
    BLOCK, BlockDisk, Blocks, DiskError, ExFatStorage, FatStorage, Kind, SharedDisk,
};

/// The bytes of a card, shared.
#[derive(Clone)]
pub struct Bytes(pub Rc<RefCell<Vec<u8>>>);

impl Blocks for Bytes {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.0.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.0
            .borrow_mut()
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.0.borrow().len() / BLOCK) as u32)
    }
}

pub type Storage = FatStorage<SharedDisk<Bytes>>;

/// A card and the storage on it; the card's bytes go when it is dropped.
pub struct Card {
    pub bytes: Bytes,
    pub storage: &'static Storage,
}

impl Drop for Card {
    fn drop(&mut self) {
        let mut bytes = self.bytes.0.borrow_mut();
        bytes.clear();
        bytes.shrink_to_fit();
    }
}

impl Card {
    /// A card of `size` bytes, formatted.
    pub fn new(size: usize) -> Self {
        let bytes = Bytes(Rc::new(RefCell::new(vec![0u8; size])));
        let mut disk = BlockDisk::new(bytes.clone()).unwrap();
        paperpaw_fat::fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
        disk.flush().unwrap();
        Self::mount(bytes)
    }

    /// The storage on the card `bytes` as it is (after a reset).
    pub fn mount(bytes: Bytes) -> Self {
        let mut disk = BlockDisk::new(bytes.clone()).unwrap();
        disk.seek(SeekFrom::Start(0)).unwrap();
        let shared = SharedDisk::new(disk);
        let fs = FileSystem::new(shared.clone(), FsOptions::new()).unwrap();
        let storage = FatStorage::new(Box::leak(Box::new(fs))).with_disk(shared);
        Self {
            bytes,
            storage: Box::leak(Box::new(storage)),
        }
    }

    /// The same card mounted anew from a copy of its bytes as they are
    /// now (what a reset would find).
    pub fn remounted(&self) -> Card {
        let copy = self.bytes.0.borrow().clone();
        Card::mount(Bytes(Rc::new(RefCell::new(copy))))
    }
}

/// Everything in file `path`.
pub fn read(storage: &dyn paperpaw_platform::Storage, path: &str) -> Vec<u8> {
    let size = storage.size(path).unwrap() as usize;
    let mut data = vec![0u8; size];
    let mut at = 0;
    while at < size {
        let n = storage.read_at(path, at as u64, &mut data[at..]).unwrap();
        assert!(n > 0, "{path} ends early at {at} of {size}");
        at += n;
    }
    data
}

/// The names in folder `dir`, sorted, folders with a `/` after them.
pub fn names(storage: &dyn paperpaw_platform::Storage, dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    storage
        .list(dir, &mut |e| {
            out.push(if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.to_string()
            })
        })
        .unwrap();
    out.sort();
    out
}

/// An image kept sparse (`tests/images`): `PPSPARSE`, its size (8 bytes),
/// then each block that is not all zeros as its number (4 bytes) and its
/// 512 bytes.
pub fn image(sparse: &[u8]) -> Bytes {
    assert_eq!(&sparse[..8], b"PPSPARSE");
    let size = u64::from_le_bytes(sparse[8..16].try_into().unwrap()) as usize;
    let mut bytes = vec![0u8; size];
    for chunk in sparse[16..].chunks(516) {
        let index = u32::from_le_bytes(chunk[..4].try_into().unwrap()) as usize;
        bytes[index * 512..(index + 1) * 512].copy_from_slice(&chunk[4..]);
    }
    Bytes(Rc::new(RefCell::new(bytes)))
}

/// An exFAT card of 8 MB as `mkfs.exfat` made it, empty.
pub fn exfat_card() -> Bytes {
    image(include_bytes!("../images/small.sparse"))
}

/// The storage on the exFAT card `bytes`, as the X4 mounts it.
pub fn exfat(bytes: &Bytes) -> ExFatStorage<Bytes> {
    let (disk, kind) = BlockDisk::find(bytes.clone()).unwrap();
    assert_eq!(kind, Kind::ExFat);
    ExFatStorage::mount(SharedDisk::new(disk)).unwrap()
}
