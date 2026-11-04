//! The SD card's FAT file system as PaperPaw's storage, on any card's
//! blocks (the X4's on SPI, the ESP32-S3 devices' on SDMMC).

use alloc::boxed::Box;

use log::warn;
use paperpaw_core::CardTrouble;
use paperpaw_fat::fatfs::{FileSystem, FsOptions};
use paperpaw_fat::{
    BlockDisk, Blocks, CardSwap, FatStorage, Restart, SharedDisk, Swapped, VolumeError, Watched,
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

/// The FAT file system on `blocks`; without one PaperPaw can read, why.
pub fn mount<B: Blocks + 'static>(blocks: B) -> Result<Box<dyn Storage>, CardTrouble> {
    // The disk shared with the storage, which writes back its changed
    // blocks (folders, the FAT) on sync.
    let fs = BlockDisk::volume(blocks)
        .map(SharedDisk::new)
        .and_then(|disk| {
            FileSystem::new(disk.clone(), FsOptions::new())
                .map(|fs| (fs, disk))
                .map_err(|_| VolumeError::NoFat)
        });
    match fs {
        Ok((fs, disk)) => Ok(Box::new(
            FatStorage::owned(fs).with_disk(disk),
        )),
        Err(e) => Err(match e {
            VolumeError::ExFat => {
                warn!("SD card: exFAT; PaperPaw reads FAT32");
                CardTrouble::ExFat
            }
            VolumeError::NoFat => {
                warn!("SD card: no FAT file system");
                CardTrouble::NoFat
            }
            VolumeError::Disk => {
                warn!("SD card: not readable");
                CardTrouble::Unreadable
            }
        }),
    }
}
