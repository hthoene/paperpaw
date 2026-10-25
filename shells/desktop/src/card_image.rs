//! A card image, a file holding a whole card (a FAT or exFAT volume, with
//! or without a partition table, as `dd` copies a card), as the emulated
//! device's storage: mounted by PaperPaw's own FAT and exFAT storages, as
//! the X4 mounts its card (`--card`). What it writes goes to the file.

use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::rc::Rc;

use paperpaw_fat::fatfs::{FileSystem, FsOptions};
use paperpaw_fat::{
    BLOCK, BlockDisk, Blocks, DiskError, ExFatStorage, FatStorage, Kind, SharedDisk,
};
use paperpaw_platform::Storage;

/// The blocks of a card image, a file or bytes in memory.
#[derive(Debug, Clone)]
pub enum ImageBlocks {
    File(Rc<RefCell<File>>),
    Memory(Rc<RefCell<Vec<u8>>>),
}

impl Blocks for ImageBlocks {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = u64::from(index) * BLOCK as u64;
        match self {
            ImageBlocks::File(file) => {
                let mut file = file.borrow_mut();
                file.seek(SeekFrom::Start(at)).map_err(|_| DiskError)?;
                file.read_exact(block).map_err(|_| DiskError)
            }
            ImageBlocks::Memory(bytes) => {
                let bytes = bytes.borrow();
                let at = at as usize;
                block.copy_from_slice(bytes.get(at..at + BLOCK).ok_or(DiskError)?);
                Ok(())
            }
        }
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = u64::from(index) * BLOCK as u64;
        match self {
            ImageBlocks::File(file) => {
                let mut file = file.borrow_mut();
                file.seek(SeekFrom::Start(at)).map_err(|_| DiskError)?;
                file.write_all(block).map_err(|_| DiskError)
            }
            ImageBlocks::Memory(bytes) => {
                let mut bytes = bytes.borrow_mut();
                let at = at as usize;
                bytes
                    .get_mut(at..at + BLOCK)
                    .ok_or(DiskError)?
                    .copy_from_slice(block);
                Ok(())
            }
        }
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        let len = match self {
            ImageBlocks::File(file) => file.borrow().metadata().map_err(|_| DiskError)?.len(),
            ImageBlocks::Memory(bytes) => bytes.borrow().len() as u64,
        };
        u32::try_from(len / BLOCK as u64).map_err(|_| DiskError)
    }
}

/// The file system on the card `blocks`, FAT or exFAT, as PaperPaw's
/// storage. Never formats anything.
pub fn mount(blocks: ImageBlocks) -> std::io::Result<Box<dyn Storage>> {
    let invalid = |what: String| std::io::Error::new(std::io::ErrorKind::InvalidData, what);
    let (disk, kind) = BlockDisk::find(blocks).map_err(|e| invalid(format!("{e:?}")))?;
    let disk = SharedDisk::new(disk);
    Ok(match kind {
        Kind::Fat => {
            let fs = FileSystem::new(disk.clone(), FsOptions::new())
                .map_err(|e| invalid(format!("FAT: {e:?}")))?;
            // Lives as long as the emulator, as on a device.
            Box::new(FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk))
        }
        Kind::ExFat => {
            Box::new(ExFatStorage::mount(disk).map_err(|e| invalid(format!("exFAT: {e:?}")))?)
        }
    })
}

/// The card image in the file at `path`, read and written in place.
pub fn open(path: &Path) -> std::io::Result<ImageBlocks> {
    let file = File::options().read(true).write(true).open(path)?;
    Ok(ImageBlocks::File(Rc::new(RefCell::new(file))))
}
