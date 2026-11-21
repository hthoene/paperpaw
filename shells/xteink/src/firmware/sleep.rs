//! Resting between loops and sleeping: the processor's nap, the sleep the
//! kernel asked for (light, then switched off) and the wake after it.

use log::info;

use super::{Firmware, POLL_MS, SAMPLE_MS, SLEEP_WAIT_MS, STATUS_MS, SYNC_MS};
#[cfg(feature = "asleep-light")]
use super::{card_swap, sleeping_board};
#[cfg(feature = "asleep-light")]
use crate::device;
use crate::usb_power::Sense;
use crate::{input, now, power};
use esp_hal::gpio::{Event, WakeupConfig};

impl Firmware {
    /// On battery the processor sleeps until there is something to do or
    /// the buttons are to be read again; on USB it stays in reach of the
    /// computer (see `power`).
    pub(super) fn rest(&mut self, battery: bool, installing: bool) {
        #[cfg(feature = "remote")]
        if self
            .state
            .sleep_on_usb_until
            .is_some_and(|until| until <= now())
        {
            // Started again: whatever the light sleeps did to the USB link,
            // the computer finds the X4 again.
            power::mark(power::at::TRIED);
            crate::safe_mode::clean_exit();
            esp_hal::system::software_reset();
        }
        // A press under way: read again soon, awake (the readings stop in
        // a light sleep), so that it counts at once.
        if input::settling() {
            self.delay.delay_millis(SAMPLE_MS as u32);
            return;
        }
        let idle = self.idle_ms();
        // A doze waits for a press or a command only: what the kernel and
        // the panel would wake for (a stale time, long past) would make
        // this return at once, every loop.
        let dozing = self.state.stay.is_dozing();
        let mut until = now().0
            + if battery {
                idle
            } else if self.network.moving() || installing || self.cable_moving() {
                // Data on the way, or an update: on at once.
                0
            } else {
                POLL_MS
            };
        if let Some(at) = self.state.wake_at
            && !dozing
        {
            until = until.min(at.0);
        }
        if let Some(worked) = self.state.unsynced {
            let due = worked + SYNC_MS;
            if now().0 >= due {
                self.write_back();
            } else {
                until = until.min(due);
            }
        }
        if !dozing {
            if let Some(read) = self.state.status_read {
                until = until.min(read.0 + STATUS_MS);
            }
            if let Some(at) = self.state.panel_idle_at {
                until = until.min(at);
            }
            if let Some(due) = self.frame_due() {
                until = until.min(due);
            }
        }
        if !power::nap_until(until) {
            self.delay
                .delay_millis(until.saturating_sub(now().0) as u32);
        }
    }

    /// Whether light sleeps are tried on USB (`idle sleep` over USB).
    pub(super) fn trying_sleep(&self) -> bool {
        #[cfg(feature = "remote")]
        return self.state.sleep_on_usb_until.is_some();
        #[cfg(not(feature = "remote"))]
        false
    }

    /// Plugins save what is pending, the screen stays as it is, and the X4
    /// sleeps: its power is cut where possible, otherwise the processor
    /// enters deep sleep. The power button starts it again where it was.
    /// USB or a charging indication must not keep a sleeping reader awake.
    /// `lightly`: a real sleep first, switched off after a while (#51); not
    /// where a plugin asked to switch off (`Effect::PowerOff`): the power
    /// goes at once, the next start is a cold one. Either way plugins have
    /// heard `core.suspend` and saved first.
    #[inline(never)]
    pub(super) fn sleep(&mut self, lightly: bool) {
        self.settle_frame();
        // The lows of the run are kept before the sleep (test image).
        #[cfg(feature = "test-image")]
        crate::high_water::before_sleep();
        // A press over USB that put it to sleep is let go first.
        #[cfg(feature = "remote")]
        self.release_held();
        // A doze that ends in a sleep (the cable pulled) suspended already.
        if !self.state.stay.suspended_already() {
            self.pause_trial();
            let effects = self.kernel.suspend(now());
            self.apply(effects);
        }
        // Bluetooth down now: the main loop does not come by once the
        // pending work is finished and the processor sleeps.
        #[cfg(feature = "bluetooth")]
        self.network
            .bluetooth(paperpaw_core::BtCall::Radio { on: false });
        let pressed = !self.finish_work();
        // What the work finished wrote is on the card before a light sleep
        // of hours (`slept::mark` writes back once more at the power-off).
        self.write_back();
        // A plugin may have asked to switch off while it heard core.suspend
        // or in the work finished: that is honoured now, not after a wake.
        // Power held 8 s (G12) is out, whatever else was pressed meanwhile.
        let next = if self.state.escape {
            paperpaw_x4::sleeping::After::SwitchOff
        } else {
            paperpaw_x4::sleeping::after_suspend(lightly, self.state.power_off, pressed)
        };
        if next == paperpaw_x4::sleeping::After::Awake {
            // A press meanwhile: awake after all.
            info!("sleep: a press, awake");
            self.state.sleep = false;
            self.state.power_off = false;
            if let Some(ota) = &mut self.ota {
                ota.resume_trial(now().0);
            }
            let effects = self.kernel.resume(now());
            return self.apply(effects);
        }
        // A real sleep first (the processor lightly asleep, the power button
        // waking it at once), switched off only after a while (#51).
        #[cfg(feature = "asleep-light")]
        if next == paperpaw_x4::sleeping::After::Light && self.sleep_lightly() {
            return;
        }
        self.power_off()
    }

    /// The power goes: only the power button starts the reader again, and
    /// that is a start (a light sleep ended by the button is not).
    #[expect(
        clippy::expect_used,
        reason = "the power button is taken back only here; without it the X4 could not wake"
    )]
    #[inline(never)]
    pub(super) fn power_off(&mut self) -> ! {
        // Its own choice, maybe before the first frame: not a failed start.
        crate::safe_mode::clean_exit();
        // The next start ends this sleep: the mark says so, for plugins to
        // hear the device wake (`core.resume`). A switch-off a plugin asked
        // for leaves it too: the start is a cold one, but plugins that
        // count sleeps hear `core.resume` (with the time asleep unknown).
        self.prepare_start_picture();
        paperpaw_x4::slept::mark(self.kernel.storage());
        let _ = self.panel.sleep();
        #[cfg(feature = "x4-pro")]
        if let Some(light) = self.light.as_mut() {
            light.off();
        }
        info!(
            "sleeping: hardware shutdown (USB {}, charging {})",
            self.usb_power.cable(),
            self.usb_power.charging()
        );
        // The power goes, the radio's with it: the network left first, and
        // before the wait below, which has no end while the button is held
        // (a case or a sleeve pressing it): the radio would listen all that
        // time.
        self.network.leave();
        // Released first: held, the button would wake it at once. A hold
        // meanwhile (a sleeve pressing it) is not the escape.
        input::escape_off();
        self.wait_for_power_release();
        let mut power = input::stop().expect("the power button");
        // The X4 Classic and the Picco keep their power on and hold their
        // lines, as their stock firmware sleeps (see their boards).
        #[cfg(any(feature = "x4-classic", feature = "picco"))]
        crate::board::hold_for_sleep(&mut self.power_latch);
        #[cfg(not(any(feature = "x4-classic", feature = "picco")))]
        {
            // The X4 Pro holds its latch through resets: let go first.
            #[cfg(feature = "esp32s3")]
            self.power_latch.set_pad_hold(false);
            self.power_latch.set_low();
        }
        power.listen(Event::LowLevel);
        let _ = power.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true));
        power::deep();
    }

    /// Sleeps lightly until the power button wakes the reader (`true`:
    /// awake again, where it was), or until it is time to switch it off
    /// (`false`: the caller does). The panel keeps its picture, the radios
    /// are checked off, the readings rest and the watchdogs are the sleep's;
    /// every [`paperpaw_x4::sleeping::CHECK_MS`] it wakes for a moment to
    /// look at the battery. The order of it is
    /// [`paperpaw_x4::sleeping::run`], tested on a computer.
    #[cfg(feature = "asleep-light")]
    #[inline(never)]
    pub(super) fn sleep_lightly(&mut self) -> bool {
        use paperpaw_x4::sleeping::{Ended, Off};
        // The developer switch back to how the X4 slept before.
        if !self.kernel.settings().read(&device::LIGHT_SLEEP) {
            return false;
        }
        // The front light goes off for the sleep (its PWM stops in a light
        // sleep, the LEDs must not be left half lit); `wake_lightly` and the
        // next frame bring it back as the settings say.
        #[cfg(feature = "x4-pro")]
        if let Some(light) = self.light.as_mut() {
            light.off();
        }
        let choice = self.kernel.settings().read(&device::POWER_OFF_AFTER);
        let off = Off::of(choice);
        info!(
            "sleeping: light sleep, off {off:?}, USB {}, charging {}",
            self.usb_power.cable(),
            self.usb_power.charging()
        );
        let started = now().0;
        let ended = paperpaw_x4::sleeping::run(off, &mut sleeping_board::Light { firmware: self });
        info!(
            "sleeping: {ended:?} after {} s",
            now().0.saturating_sub(started) / 1000
        );
        self.draw_off_screen(ended);
        if ended == Ended::Woken {
            self.wake_lightly();
        }
        ended == Ended::Woken
    }

    /// Awake again after a light sleep: plugins hear it (`core.resume`),
    /// the screen is drawn as it was before the sleep screen.
    #[cfg(feature = "asleep-light")]
    pub(super) fn wake_lightly(&mut self) {
        self.state.sleep = false;
        self.state.power_off = false;
        // The clock and the battery shown anew soon.
        self.state.status_read = None;
        // The press that woke it is input: the front light, off for the
        // sleep, comes back as set and its timer starts anew.
        #[cfg(feature = "x4-pro")]
        {
            self.state.light_timer = paperpaw_x4::light_timer::LightTimer::new(now().0);
        }
        if let Some(ota) = &mut self.ota {
            ota.resume_trial(now().0);
        }
        // A card swapped while asleep is mounted anew first: waking is
        // then told as after a USB drive (which resumes too). One that kept
        // its power is as the sleep left it: the kernel walks no library
        // again for this wake (#305).
        let effects = match self.card_asleep() {
            card_swap::Asleep::Swapped => self.kernel.usb_drive_ended(now()),
            card_swap::Asleep::Kept => self.kernel.resume_storage_kept(now()),
            card_swap::Asleep::Gone => self.kernel.resume(now()),
        };
        self.apply(effects);
    }

    /// Before sleeping: requests under way (a position pushed) go on until
    /// they are answered, [`SLEEP_WAIT_MS`] at the longest; the radio goes
    /// with the power then. `false` if a press came meanwhile (it waits
    /// for the main loop): no sleep then. Power held 8 s meanwhile (G12,
    /// the press that put it to sleep held on) ends it with the escape
    /// taken (`state.escape`): the caller switches off.
    pub(super) fn finish_work(&mut self) -> bool {
        let until = now().0 + SLEEP_WAIT_MS;
        if self.kernel.holds_sleep() {
            info!("sleep: finishing work first");
        }
        while self.kernel.holds_sleep() && now().0 < until {
            power::feed();
            if self.take_escape() {
                return true;
            }
            if input::waiting() {
                return false;
            }
            self.io();
            for effects in self.network.step(&mut self.kernel, now()) {
                self.apply(effects);
            }
            self.tick();
            self.delay.delay_millis(POLL_MS as u32);
        }
        true
    }

    pub(super) fn wait_for_power_release(&self) {
        while input::power_down() {
            power::feed();
            self.delay.delay_millis(10);
        }
    }
}
