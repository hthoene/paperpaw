//! The firmware as the light sleep sees it ([`paperpaw_x4::sleeping::run`]).

use log::info;

use super::Firmware;
use crate::usb_power::Sense;
use crate::{input, now, power};

/// The board as [`paperpaw_x4::sleeping::run`] sees it: the firmware for
/// the length of one sleep.
pub(super) struct Light<'a> {
    pub(super) firmware: &'a mut Firmware,
}

impl paperpaw_x4::sleeping::Board for Light<'_> {
    fn now_ms(&mut self) -> u64 {
        now().0
    }

    fn look(&mut self) -> paperpaw_x4::sleeping::Look {
        paperpaw_x4::sleeping::Look {
            now_ms: now().0,
            // Charging, not a computer talking: a reader on a cable with a full
            // cell is switched off after its time, as one on a battery.
            usb: self.firmware.usb_power.charging(),
            percent: input::percent_now(),
        }
    }

    fn panel_down(&mut self) {
        let _ = self.firmware.panel.power_down();
    }

    fn button_released(&mut self) -> bool {
        let until = now().0 + paperpaw_x4::sleeping::HELD_MS;
        // Released first: held, the button would wake it at once.
        while input::power_down() {
            if now().0 > until {
                return false;
            }
            power::feed();
            self.firmware.delay.delay_millis(10);
        }
        true
    }

    fn leave_radios(&mut self) {
        self.firmware.network.leave();
    }

    fn radios_off(&mut self) -> bool {
        !self.firmware.network.radio_on()
    }

    fn pause_inputs(&mut self) {
        input::pause();
    }

    fn resume_inputs(&mut self) {
        input::resume();
    }

    fn watchdog_for_sleep(&mut self) {
        power::watchdogs_for_sleep(paperpaw_x4::sleeping::SLEEP_WATCHDOG_MS);
        info!(
            "sleeping: RTC watchdog paused in sleep: {}",
            power::watchdog_pauses_in_sleep()
        );
    }

    fn watchdog_awake(&mut self) {
        power::watchdogs_awake();
    }

    fn feed(&mut self) {
        power::feed();
    }

    fn nap(&mut self, until_ms: u64) -> paperpaw_x4::sleeping::Napped {
        use paperpaw_x4::sleeping::Napped;
        if !power::sleep_asleep(until_ms) {
            return Napped::Ended;
        }
        // A press, not a glitch: still down after a moment.
        self.firmware.delay.delay_millis(20);
        if input::power_pin_low() {
            Napped::Pressed
        } else {
            Napped::Glitch
        }
    }

    fn say(&mut self, what: &str) {
        info!("sleeping: {what}");
    }
}
