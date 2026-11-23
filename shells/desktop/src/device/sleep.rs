//! The device asleep and switched off, as the Xteink firmware has them
//! (`shells/xteink/src/firmware/sleep.rs`): the radios go off for a sleep, a
//! switch-off ends the run (the power button starts it anew), and a wake
//! tells the kernel how the card came through.

use paperpaw_core::{BtCall, WifiCommand};

use super::{Device, card};
use crate::budget;

impl Device {
    /// The sleep screen is shown: plugins save and hear `core.suspend`, the
    /// radios go off (the light sleep leaves them, the kernel hears it when
    /// the device wakes), and the device sleeps, or is switched off
    /// (`power_off`), the power button then starting it anew. A switch-off
    /// leaves the sleep's mark on the card, as the firmware does
    /// (`paperpaw_x4::slept`): the next start is told it follows a sleep.
    pub(super) fn fall_asleep(&mut self, power_off: bool) {
        self.asleep = true;
        self.card_left_asleep = false;
        let what = if power_off { "power off" } else { "asleep" };
        eprintln!("{}: {what}", self.profile.device.id);
        let now = self.now();
        let effects = budget::track(|| self.kernel.suspend(now));
        self.execute(effects);
        if self.profile.wifi {
            self.wifi.apply(WifiCommand::Off, now);
        }
        if self.profile.bluetooth > 0 {
            self.bluetooth.apply(BtCall::Radio { on: false }, now);
        }
        if power_off {
            self.powered_off = true;
            paperpaw_x4::slept::mark(self.kernel.storage());
        }
    }

    /// The power button pressed while asleep: a switched-off device starts
    /// anew; one asleep wakes, and the kernel hears how the card came
    /// through (`firmware/sleep.rs`, `wake_lightly`): taken out and back, as
    /// after a USB drive (a computer may have had it); still out, a plain
    /// wake; never out, kept as the sleep left it, so no library is walked
    /// again (#305).
    pub(super) fn wake(&mut self) {
        self.asleep = false;
        if std::mem::take(&mut self.powered_off) {
            eprintln!("{}: power on", self.profile.device.id);
            return self.start_again(false);
        }
        eprintln!("{}: awake", self.profile.device.id);
        let now = self.now();
        let swapped = std::mem::take(&mut self.card_left_asleep);
        let effects = budget::track(|| {
            if card::is_out() {
                self.kernel.resume(now)
            } else if swapped {
                self.kernel.usb_drive_ended(now)
            } else {
                self.kernel.resume_storage_kept(now)
            }
        });
        self.execute(effects);
    }

    /// The card is taken out of its slot (`true`) or put back (scripts'
    /// `card-out` and `card-in`): while it is out everything asked of the
    /// storage fails with [`paperpaw_platform::StorageError::CARD_GONE`],
    /// which the kernel meets, as on a device. Put back while awake, it is
    /// the card it was and the kernel reads again what it kept of it
    /// (`Kernel::card_back`, the firmware's `watch_card`); taken out while
    /// asleep, the wake tells it.
    pub fn card_out(&mut self, out: bool) {
        let back = card::is_out() && !out;
        card::set_out(out);
        if self.asleep {
            self.card_left_asleep |= out;
        } else if back {
            let kernel = &mut self.kernel;
            budget::track(|| kernel.card_back());
        }
    }
}
