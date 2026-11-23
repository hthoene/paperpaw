//! The kernel's sleep, wake and stop.

use crate::{Ask, Effects, Instant, Kernel, events, meter};

impl Kernel {
    /// The device goes to sleep or the app to the background; plugins hear
    /// [`events::SUSPEND`].
    pub fn suspend(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        // Fingers and keys held under a native filter's capture are let go.
        self.shared.native_input.clear();
        self.shared.now = now;
        self.suspended_at = Some(now);
        self.shared.asleep = true;
        self.developer_suspend();
        self.shared.usage.sleeps = self.shared.usage.sleeps.saturating_add(1);
        self.output.emit(&events::SUSPEND, ());
        self.end_at_suspend();
        let effects = self.finish();
        // Standby: the plugins that may sleep and run idle are parked again
        // (#248, D3, Decision 166); each is made again at its next use.
        self.native_repark_idle(usize::MAX);
        // Asleep, no word is hyphenated: open tables give their heap back.
        self.shared.hyphenation.close_tables();
        self.keep_timing();
        // What plugins saved, and what storage kept back, written.
        let _ = self.shared.storage.sync();
        effects
    }

    /// PaperPaw stops: the device restarts or the app closes for good;
    /// plugins hear [`events::STOPPING`]. Unlike [`Kernel::suspend`] it is
    /// no sleep: none is counted, and no work waits for it.
    pub fn stop(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.now = now;
        self.developer_suspend();
        self.output.emit(&events::STOPPING, ());
        let effects = self.finish();
        self.keep_timing();
        // What plugins saved, and what storage kept back, written.
        let _ = self.shared.storage.sync();
        effects
    }

    /// Back from [`Kernel::suspend`]. How long the device slept, for
    /// plugins ([`Place::slept_for`](crate::Place::slept_for)), is the
    /// time between the two by the shell's clock: `now` counts the time
    /// asleep.
    pub fn resume(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.native_input.clear();
        self.woke(now);
        self.finish()
    }

    /// [`Kernel::resume`], for a shell that knows that nothing changed the
    /// storage while the device slept: a memory card that kept its power
    /// through a light sleep (a card taken out loses it, and is told with
    /// [`Kernel::usb_drive_ended`] once it is mounted anew). Nothing can
    /// have been copied onto it, so the library is not walked again for
    /// this wake, and a walk under way goes on where it was (#305).
    pub fn resume_storage_kept(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.native_input.clear();
        self.woke(now);
        self.storage_kept = true;
        self.finish()
    }

    /// What waking does before the frame: the time asleep, plugins told.
    fn woke(&mut self, now: Instant) {
        self.forget_presses();
        self.storage_kept = false;
        self.shared.asleep = false;
        self.shared.show_again = None;
        if let Some(at) = self.suspended_at.take() {
            let slept = now.0.saturating_sub(at.0);
            self.shared.slept_for = Some(core::time::Duration::from_millis(slept));
        }
        self.shared.now = now;
        self.last_activity = now;
        self.shown_since = now; // the time asleep is no screen's
        self.close_shut_out();
        self.count_free_again(now);
        self.native_work_woke();
        self.output.emit(&events::RESUME, ());
    }

    /// The shell found no cable for [`Effects::usb_drive`]: the card was
    /// never lent. As on waking, and the reader is told at once.
    pub fn usb_drive_refused(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.woke(now);
        self.with_env(|env, out| {
            out.push(Ask::Notify(alloc::string::String::from(
                env.i18n().tr("core.usb-drive.none"),
            )));
        });
        self.finish()
    }

    /// The card is PaperPaw's again after a computer had it as a USB drive
    /// ([`Effects::usb_drive`]): as on waking ([`Kernel::resume`]), and
    /// plugins hear [`events::USB_DRIVE_ENDED`]; the library is walked
    /// anew, as files may have come, gone or changed.
    pub fn usb_drive_ended(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.woke(now);
        // A computer may have changed the card, or another card is in.
        self.card_changed();
        self.output.emit(&events::USB_DRIVE_ENDED, ());
        self.output.emit(&events::LIBRARY_CHANGED, ());
        self.finish()
    }

    /// The card lost its power and has it back, and is the card it was,
    /// as it was left (a loose contact; the shell's `Swapped::Back`): what
    /// the kernel kept of it is read again, as what was not written back
    /// is gone. Unlike [`Kernel::usb_drive_ended`] nothing wakes: no
    /// library is walked, no plugin told, and the time to sleep stays.
    pub fn card_back(&mut self) {
        let _running = self.shared.meter.enter(meter::CORE);
        self.card_changed();
    }

    /// Closes the screens above a guard that it did not open (a sleep
    /// screen): input goes to the guard again.
    pub(crate) fn close_shut_out(&mut self) {
        while self.top_shut_out() {
            let top = self.stack.len() - 1;
            let key = paperpaw_sdk::ScreenKey::new(self.stack[top].serial);
            let before = self.stack.len();
            self.close_screen(key);
            if self.stack.len() == before {
                break;
            }
        }
    }
}
