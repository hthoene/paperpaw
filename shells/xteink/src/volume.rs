//! The SD card's file system, FAT or exFAT, as PaperPaw's storage, on any
//! card's blocks (the X4's on SPI, the ESP32-S3 devices' on SDMMC).

use alloc::boxed::Box;

use log::warn;
use paperpaw_core::CardTrouble;
use paperpaw_fat::fatfs::{FileSystem, FsOptions};
use paperpaw_fat::{
    BlockDisk, Blocks, CardSwap, ExFatStorage, FatStorage, Kind, Restart, SharedDisk, Swapped,
    VolumeError, Watched,
};
use paperpaw_platform::Storage;

/// The card mounted: the storage for the kernel, and what watches the
/// card being taken out and put back.
pub struct Mounted {
    pub storage: Box<dyn Storage>,
    pub swap: Box<dyn Swap>,
}

/// A card that may be taken out and put back while PaperPaw runs (#145,
/// see `paperpaw_fat::CardSwap`), whatever card it is.
pub trait Swap {
    /// Between the kernel's steps: a card found lost started again and
    /// mounted anew, where it is back.
    fn check(&mut self, now_ms: u64) -> Swapped;
    /// Looks at the card itself (on waking): `true` if it is lost.
    // Only boards with a light sleep wake without a start.
    #[cfg_attr(
        not(feature = "asleep-light"),
        expect(dead_code, reason = "used on waking from light sleep")
    )]
    fn probe(&mut self) -> bool;
}

impl<B: Restart + 'static> Swap for CardSwap<B, CardTrouble> {
    fn check(&mut self, now_ms: u64) -> Swapped {
        CardSwap::check(self, now_ms)
    }

    fn probe(&mut self) -> bool {
        CardSwap::probe(self)
    }
}

/// The started `card`'s file system as PaperPaw's storage (see [`mount`]),
/// mounted anew should the card be taken out and put back.
pub fn mount_swappable<B: Restart + 'static>(card: B) -> Result<Mounted, CardTrouble> {
    let (swap, storage) = CardSwap::mount(card, mount::<Watched<B>>, clock_ms)?;
    Ok(Mounted {
        storage: Box::new(storage),
        swap: Box::new(swap),
    })
}

/// The shell's clock, in ms.
fn clock_ms() -> u64 {
    crate::now().0
}

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
            // As before exFAT: a FAT volume rust-fatfs does not take.
            Err(_) => {
                warn!("SD card: no FAT file system");
                Err(CardTrouble::NoFat)
            }
        },
        Kind::ExFat => match ExFatStorage::mount(disk) {
            Ok(storage) => {
                let missing = storage.missing_blocks();
                if missing > 0 {
                    let past_end = storage.clusters_past_end();
                    warn!(
                        "SD card: the exFAT volume is {} KB longer than the card; its {past_end} clusters there are not used",
                        missing / 2
                    );
                }
                Ok(Box::new(storage))
            }
            Err(failure) => {
                warn!("SD card: exFAT volume not readable: {failure}");
                Err(CardTrouble::Unreadable)
            }
        },
    }
}
