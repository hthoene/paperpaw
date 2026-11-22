//! The card shown to a computer as a USB drive (#32): the main loop's side
//! of [`crate::usb_drive`]. The kernel asked (`Effect::UsbDrive`) with the
//! notice shown; the plugins save and hear `core.suspend` as before a
//! sleep, the card's file system is dropped and the card lent to the
//! drive, and when the computer ejects it (or the reader presses Back, or
//! the cable goes) the card is mounted anew and the kernel walks the
//! library again (`Kernel::usb_drive_ended`).

use log::{info, warn};
use paperpaw_fat::Swapped;
use paperpaw_platform::buttons;

use super::Firmware;
use crate::usb_power::Sense;
use crate::{input, now, power};

/// Asked each turn of the drive's loop: whether the reader asks to end it
/// (Back, Home on the X4 Pro, the power button, or power held 8 s: the
/// main loop acts on that one afterwards). The watchdog is fed here.
fn reader_ends() -> bool {
    power::feed();
    let mut ends = input::escape_requested();
    while let Some(change) = input::next() {
        ends |= match change {
            input::Change::Button(name, true, _) => name == buttons::BACK || name == buttons::POWER,
            input::Change::Touch(input::Touch::Home(true), _) => true,
            _ => false,
        };
    }
    ends
}

impl Firmware {
    /// Shows the card to a computer until it ends, then takes it back.
    #[inline(never)]
    pub(super) fn usb_drive(&mut self) {
        self.state.usb_drive = false;
        // No cable, no computer: say so at once, not after a minute's wait
        // (#437). A charger alone still waits, for `NO_COMPUTER_MS`.
        if !self.usb_power.cable_now() {
            info!("USB drive: no cable");
            let effects = self.kernel.usb_drive_refused(now());
            return self.apply(effects);
        }
        self.settle_frame();
        // Plugins save and hear `core.suspend`, as before a sleep.
        self.pause_trial();
        let effects = self.kernel.suspend(now());
        self.apply(effects);
        #[cfg(feature = "bluetooth")]
        self.network
            .bluetooth(paperpaw_core::BtCall::Radio { on: false });
        self.finish_work();
        // What the work wrote is on the card before the computer has it.
        self.write_back();
        // Power held 8 s meanwhile (G12): switched off, the card never lent
        // (the reset the escape ends in would come while a computer has it).
        if self.state.escape {
            self.power_off();
        }
        // Presses so far are of no interest: only the ones that end it.
        while input::next().is_some() {}
        info!("USB drive: the card goes to the computer");
        let mut ended = None;
        let lent = self.card_swap.as_mut().is_some_and(|swap| {
            swap.lend(&mut |card| {
                ended = Some(crate::usb_drive::run(card, reader_ends));
            })
        });
        let Some(why) = ended.filter(|_| lent) else {
            warn!("USB drive: the card could not be lent");
            if let Some(ota) = &mut self.ota {
                ota.resume_trial(now().0);
            }
            let effects = self.kernel.resume(now());
            return self.apply(effects);
        };
        // Power held 8 s ended it (G12): taken at once, which gives the
        // switch-off the escape's time for a clean one, not the grace a reset
        // ends (the card is mounted anew first, the frame after it would take
        // most of that).
        let escape = self.take_escape();
        info!("USB drive: ended ({why:?}); the card is mounted anew");
        // The clock and the battery shown anew soon, the light as it was.
        self.state.status_read = None;
        #[cfg(feature = "x4-pro")]
        {
            self.state.light_timer = paperpaw_x4::light_timer::LightTimer::new(now().0);
        }
        if let Some(ota) = &mut self.ota {
            ota.resume_trial(now().0);
        }
        // Any card is mounted as new: a computer had it. Where it is not
        // back yet, the kernel runs on and `watch_card` tells it once it is.
        let remounted = self.remount_card();
        if escape {
            // Asleep since the card was lent: off as from a sleep.
            self.power_off();
        }
        let effects = match remounted {
            Swapped::Remounted | Swapped::Back => self.kernel.usb_drive_ended(now()),
            _ => self.kernel.resume(now()),
        };
        self.apply(effects);
    }
}
