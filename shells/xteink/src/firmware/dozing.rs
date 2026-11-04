//! Staying awake on USB, the developer's choice (#96): the main loop's side
//! of [`paperpaw_x4::stay_awake`]. While it is on and the cable is in, the
//! kernel does not sleep by itself, and a sleep the reader asks for is a
//! doze: the sleep picture stays, plugins heard `core.suspend`, and the X4
//! reads USB and the buttons awake, so the computer keeps it. The first
//! press or command wakes it (`core.resume`); a cable pulled meanwhile puts
//! it to sleep for real, two seconds after (the choice never leaves a
//! forgotten X4 awake on its battery).

use log::info;

use super::Firmware;
use crate::usb_power::Sense;
use crate::{device, now, power};

impl Firmware {
    /// Looks at the choice and the cable once a loop: the kernel is told
    /// when they change, and a doze whose cable is gone ends in a sleep.
    pub(super) fn watch_stay_awake(&mut self) {
        // The cable is looked at only where the choice is on.
        let wanted =
            self.kernel.settings().read(&device::STAY_AWAKE_USB) && self.usb_power.cable();
        if let Some(on) = self.state.stay.watch(wanted, now().0) {
            info!("stay awake on USB: {}", if on { "on" } else { "off" });
            let effects = self.kernel.set_awake_on_usb(now(), on);
            // A doze whose cable is gone sleeps for real next: what the
            // kernel would draw now must not go over the sleep picture.
            if !self.state.stay.is_dozing() {
                self.apply(effects);
            }
        }
        if self.state.stay.must_sleep() {
            info!("stay awake on USB: the cable is out, sleeping");
            self.state.sleep = true;
        }
    }

    /// A sleep asked for while staying awake: the picture stays, plugins
    /// save (`core.suspend`), the X4 goes on awake.
    pub(super) fn doze(&mut self) {
        self.settle_frame();
        #[cfg(feature = "remote")]
        self.release_held();
        self.pause_trial();
        let effects = self.kernel.suspend(now());
        self.apply(effects);
        self.state.sleep = false;
        self.state.stay.doze();
        info!("sleep: dozing on USB, any press or command wakes it");
    }

    /// One loop of a doze: the buttons and USB are read, and the first of
    /// either wakes the X4 (the press or the command is then handled as
    /// usual). Nothing else runs, so the sleep picture is not drawn over.
    pub(super) fn dozing_step(&mut self) {
        power::feed();
        power::allow(false);
        // The X3 knows the cable from its fuel gauge, which only a reading of
        // the battery asks (the reading itself rests a second): a cable
        // pulled while it dozes is noticed so.
        #[cfg(feature = "x3")]
        let _ = crate::input::millivolts();
        self.buttons();
        self.usb();
        if self.state.stay.is_dozing() {
            self.rest(false, false);
        }
    }

    /// Awake from a doze at a press or a command: plugins hear
    /// `core.resume`, the screen is drawn as it was before the sleep
    /// picture. Whether it did; nothing where the X4 does not doze.
    pub(super) fn wake_dozing(&mut self) -> bool {
        if !self.state.stay.wake() {
            return false;
        }
        info!("sleep: awake from the doze");
        self.state.sleep = false;
        self.state.power_off = false;
        self.state.status_read = None;
        if let Some(ota) = &mut self.ota {
            ota.resume_trial(now().0);
        }
        let effects = self.kernel.resume(now());
        self.apply(effects);
        true
    }
}
