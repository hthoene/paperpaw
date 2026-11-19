//! The SD card's FAT file system as PaperPaw's storage, on any card's
//! blocks (the X4's on SPI, the ESP32-S3 devices' on SDMMC).

use alloc::boxed::Box;

use log::{info, warn};
use paperpaw_core::CardTrouble;
use paperpaw_fat::{Blocks, CardSwap, Restart, Swapped, VolumeError, Watched};
use paperpaw_platform::Storage;

/// The card mounted: the storage for the kernel, and what watches the
/// card being taken out and put back.
pub struct Mounted {
    pub storage: Box<dyn Storage>,
    pub swap: Box<dyn Swap>,
    /// The card is exFAT: used, and the reader told once that it is an
    /// experiment (`CardTrouble::ExFat`).
    pub exfat: bool,
}

/// Whether the card mounted last is exFAT.
static EXFAT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

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
    /// The card itself for `with`, its file system dropped first (see
    /// `CardSwap::lend`); the next `check` mounts it anew. `false` where
    /// it could not be lent.
    #[cfg(feature = "usb-drive")]
    fn lend(&mut self, with: &mut dyn FnMut(&mut dyn Blocks)) -> bool;
}

impl<B: Restart + 'static> Swap for CardSwap<B, CardTrouble> {
    fn check(&mut self, now_ms: u64) -> Swapped {
        CardSwap::check(self, now_ms)
    }

    fn probe(&mut self) -> bool {
        CardSwap::probe(self)
    }

    #[cfg(feature = "usb-drive")]
    fn lend(&mut self, with: &mut dyn FnMut(&mut dyn Blocks)) -> bool {
        CardSwap::lend(self, |card| with(card)).is_some()
    }
}

/// The started `card`'s file system as PaperPaw's storage (see [`mount`]),
/// mounted anew should the card be taken out and put back.
pub fn mount_swappable<B: Restart + 'static>(card: B) -> Result<Mounted, CardTrouble> {
    let (swap, storage) = CardSwap::mount(card, mount::<Watched<B>>, clock_ms)?;
    Ok(Mounted {
        storage: Box::new(storage),
        swap: Box::new(swap),
        exfat: EXFAT.load(core::sync::atomic::Ordering::Relaxed),
    })
}

/// The shell's clock, in ms.
fn clock_ms() -> u64 {
    crate::now().0
}

/// The FAT file system on `blocks`; without one PaperPaw can read, why.
pub fn mount<B: Blocks + 'static>(blocks: B) -> Result<Box<dyn Storage>, CardTrouble> {
    // The disk shared with the storage, which writes back its changed
    // blocks (folders, the FAT) on sync; the storage owns its file system,
    // so a card mounted anew gives back what the old mount took (#151).
    match paperpaw_fat::mount(blocks) {
        Ok(storage) => {
            let exfat = storage.kind() == paperpaw_fat::fatfs::VolumeKind::ExFat;
            if exfat {
                info!("SD card: exFAT (experimental)");
            }
            EXFAT.store(exfat, core::sync::atomic::Ordering::Relaxed);
            Ok(Box::new(storage))
        }
        Err(e) => Err(match e {
            VolumeError::NoFat => {
                warn!("SD card: neither FAT nor exFAT");
                CardTrouble::NoFat
            }
            VolumeError::Disk => {
                warn!("SD card: not readable");
                CardTrouble::Unreadable
            }
        }),
    }
}
