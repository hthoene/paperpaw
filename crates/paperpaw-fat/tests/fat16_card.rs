//! A FAT16 card (up to 2 GB) mounts and holds files with the feature set the firmware
//! builds `paperpaw-fat` with: the dependency alone turns FAT16 on, the dev-dependency
//! only adds formatting (#54).

#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_fat::fatfs::{FatType, FormatVolumeOptions, Seek, SeekFrom, Write, format_volume};
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, mount};
use paperpaw_platform::Storage;

#[derive(Clone)]
struct Disk(Rc<RefCell<Vec<u8>>>);

impl Blocks for Disk {
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

#[test]
fn a_fat16_card_mounts_and_keeps_files() {
    let disk = Disk(Rc::new(RefCell::new(vec![0u8; 64 << 20])));
    let mut raw = BlockDisk::new(disk.clone()).unwrap();
    format_volume(
        &mut raw,
        FormatVolumeOptions::new().fat_type(FatType::Fat16),
    )
    .unwrap();
    raw.flush().unwrap();
    raw.seek(SeekFrom::Start(0)).unwrap();
    let storage = mount(disk).expect("a FAT16 card mounts");
    storage.write("books/a.txt", b"hello").unwrap();
    storage.sync().unwrap();
    let mut back = [0u8; 5];
    assert_eq!(storage.read_at("books/a.txt", 0, &mut back).unwrap(), 5);
    assert_eq!(&back, b"hello");
}
