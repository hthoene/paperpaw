//! The SD card's file system, FAT or exFAT, as PaperPaw's storage, on any
//! card's blocks (the X4's on SPI, the ESP32-S3 devices' on SDMMC).

use alloc::boxed::Box;

use log::warn;
use paperpaw_core::CardTrouble;
use paperpaw_fat::fatfs::{FileSystem, FsOptions};
use paperpaw_fat::{BlockDisk, Blocks, ExFatStorage, FatStorage, Kind, SharedDisk, VolumeError};
use paperpaw_platform::Storage;

/// The FAT or exFAT file system on `blocks`; without one PaperPaw can
/// read, why. Never formats the card.
pub fn mount<B: Blocks + 'static>(blocks: B) -> Result<Box<dyn Storage>, CardTrouble> {
    let (disk, kind) = BlockDisk::find(blocks).map_err(|e| match e {
        VolumeError::Ntfs | VolumeError::ExFat => {
            warn!("SD card: NTFS; PaperPaw reads FAT32 and exFAT");
            CardTrouble::Ntfs
        }
        VolumeError::NoFat => {
            warn!("SD card: no FAT or exFAT file system");
            CardTrouble::NoFat
        }
        VolumeError::Disk => {
            warn!("SD card: not readable");
            CardTrouble::Unreadable
        }
    })?;
    // The disk shared with the storage, which writes back its changed
    // blocks (folders, the FAT, the bitmap) on sync.
    let disk = SharedDisk::new(disk);
    match kind {
        Kind::Fat => match FileSystem::new(disk.clone(), FsOptions::new()) {
            Ok(fs) => Ok(Box::new(
                FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk),
            )),
            Err(_) => {
                warn!("SD card: FAT volume not readable");
                Err(CardTrouble::Unreadable)
            }
        },
        Kind::ExFat => match ExFatStorage::mount(disk) {
            Ok(storage) => Ok(Box::new(storage)),
            Err(_) => {
                warn!("SD card: exFAT volume not readable");
                Err(CardTrouble::Unreadable)
            }
        },
    }
}
