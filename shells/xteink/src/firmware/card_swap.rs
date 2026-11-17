//! The card taken out and put back while PaperPaw runs (#145): the shell's
//! part of `paperpaw_fat::CardSwap`. A card found lost (reads and writes
//! failing, or the look at it after a pause or on waking) is started
//! again and mounted anew, never written with what was kept of it; the
//! kernel then walks the library again, as after a USB drive.

use log::{info, warn};
use paperpaw_fat::Swapped;

use super::Firmware;
use crate::now;

/// How the card came through a light sleep (see [`Firmware::card_asleep`]).
#[cfg(feature = "asleep-light")]
pub(super) enum Asleep {
    /// It answers: it kept its power, nothing changed it.
    Kept,
    /// Taken out and back (or another): mounted anew.
    Swapped,
    /// Taken out and not back yet.
    Gone,
}

impl Firmware {
    /// Writes back what the storage kept back; a failure is said, not retried.
    pub(super) fn write_back(&mut self) {
        self.state.unsynced = None;
        if self.kernel.storage().sync().is_err() {
            warn!("card: not all written back");
        }
    }

    /// Between the kernel's steps: where the card is lost, starts it again
    /// and mounts it anew (every few seconds while it is out), and tells
    /// the kernel once it is.
    #[inline(never)]
    pub(super) fn watch_card(&mut self) {
        if self.remount_card() {
            let effects = self.kernel.usb_drive_ended(now());
            self.apply(effects);
        }
    }

    /// On waking from a light sleep, before the kernel hears it: how the
    /// card came through it. A card taken out loses its power and answers
    /// no read until it is started again, so one that answers is the card
    /// as the sleep left it; one that does not is mounted anew where it is
    /// back (the kernel to be told a card swapped).
    #[cfg(feature = "asleep-light")]
    #[inline(never)]
    pub(super) fn card_asleep(&mut self) -> Asleep {
        let lost = self.card_swap.as_mut().is_some_and(|swap| swap.probe());
        match lost {
            false => Asleep::Kept,
            true if self.remount_card() => Asleep::Swapped,
            true => Asleep::Gone,
        }
    }

    /// Where the card is lost, tries to mount it anew: `true` if it was.
    /// What came of it is logged as it changes.
    fn remount_card(&mut self) -> bool {
        let Some(swap) = self.card_swap.as_mut() else {
            return false;
        };
        let swapped = swap.check(now().0);
        let said = (swapped != Swapped::No).then_some(swapped);
        if said != self.state.card_said {
            match swapped {
                Swapped::No => {}
                Swapped::Remounted => info!("SD card: taken out and put back; mounted anew"),
                Swapped::Waiting => warn!("SD card: gone; looking for it every few seconds"),
                Swapped::GivenUp => {
                    warn!("SD card: gone for this run; nothing is read or written until a restart");
                }
            }
        }
        // A remount is said each time; the states between, once.
        self.state.card_said = if swapped == Swapped::Remounted {
            None
        } else {
            said
        };
        swapped == Swapped::Remounted
    }
}
