//! Sleeping on the X4 and X3: the power button puts the reader to a real
//! sleep first (the processor in light sleep, the screen as it was, the
//! power button waking it at once), and it is switched off only after a
//! while asleep (#51). This is the part that decides, on a computer-tested
//! state machine; the firmware (`shells/xteink`) does the sleeping.
//!
//! Asleep on battery, the reader wakes now and then (every
//! [`CHECK_MS`]) to look at the battery and the clock, never for long:
//!
//! - the charge at or below [`CRITICAL_PERCENT`] at two looks a minute
//!   apart (one reading may be wrong): switched off, so that the cell is
//!   not drained to where it is harmed;
//! - the charge at or below [`LOW_PERCENT`]: switched off after
//!   [`LOW_OFF_MS`] at the most, instead of the reader's choice;
//! - otherwise switched off when the choice of *Switch off after
//!   sleeping* has passed, counted from the start of the sleep, and never
//!   if the reader chose that.
//!
//! On USB the reader stays asleep and the time does not run (charging is
//! the better state to be in; unplugged, the choice counts from then).

/// The reader's choices of *Switch off after sleeping*.
pub const NOW: &str = "x4.power-off-after.now";
pub const M15: &str = "x4.power-off-after.m15";
pub const M30: &str = "x4.power-off-after.m30";
pub const H1: &str = "x4.power-off-after.h1";
pub const H2: &str = "x4.power-off-after.h2";
pub const NEVER: &str = "x4.power-off-after.never";
/// In the order the settings screen lists them.
pub const CHOICES: [&str; 6] = [NOW, M15, M30, H1, H2, NEVER];
/// The choice until the reader makes another.
pub const DEFAULT: &str = M30;

/// How long between looks at the battery while asleep (ms).
pub const CHECK_MS: u64 = 10 * 60_000;
/// At or below this charge (percent) the reader is switched off at once.
pub const CRITICAL_PERCENT: u8 = 3;
/// At or below this charge (percent) it is switched off after
/// [`LOW_OFF_MS`] at the most.
pub const LOW_PERCENT: u8 = 10;
/// How long after a critical reading the battery is looked at again (ms).
pub const CONFIRM_MS: u64 = 60_000;
/// The longest a reader with a low battery stays asleep (ms).
pub const LOW_OFF_MS: u64 = 10 * 60_000;

/// When a sleeping reader is switched off, by the reader's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Off {
    /// As soon as it is put to sleep: how the X4 slept before.
    Now,
    /// After so many milliseconds asleep on battery.
    After(u64),
    /// Never: it stays asleep.
    Never,
}

impl Off {
    /// The meaning of the choice `choice` (one of [`CHOICES`]); anything
    /// else is the [`DEFAULT`].
    pub fn of(choice: &str) -> Self {
        match choice {
            NOW => Off::Now,
            M15 => Off::After(15 * 60_000),
            H1 => Off::After(60 * 60_000),
            H2 => Off::After(120 * 60_000),
            NEVER => Off::Never,
            _ => Off::After(30 * 60_000),
        }
    }
}

/// What the reader found out when it woke to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// Milliseconds since the start (the processor's clock, which counts
    /// on in light sleep).
    pub now_ms: u64,
    /// Power from the cable.
    pub usb: bool,
    /// The battery's charge, if it could be read.
    pub percent: Option<u8>,
}

/// Why the reader is switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The reader's choice was *Now*.
    Chosen,
    /// The time of the choice has passed.
    Time,
    /// The battery is low (the time is cut short) or critical.
    Battery,
}

/// What a sleeping reader does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Sleeps on, woken at `until_ms` by the clock (or by the power
    /// button, which always wakes it).
    Nap { until_ms: u64 },
    /// Switched off: the power is cut, the power button starts it.
    SwitchOff(Why),
}

/// One sleep, from being put to sleep until the power button wakes it or
/// it is switched off.
#[derive(Debug, Clone, Copy)]
pub struct Asleep {
    off: Off,
    /// Since when it has been asleep on battery.
    battery_since_ms: u64,
    /// The last look found the cable in.
    was_on_usb: bool,
    /// The last look found the battery critical.
    critical_seen: bool,
}

impl Asleep {
    /// Put to sleep at `now_ms` with the reader's choice.
    pub fn new(off: Off, now_ms: u64) -> Self {
        Self {
            off,
            battery_since_ms: now_ms,
            was_on_usb: false,
            critical_seen: false,
        }
    }

    /// What to do now, at the start of the sleep and at each look.
    pub fn next(&mut self, look: Look) -> Next {
        if look.usb {
            // Charging: the time on battery starts afresh when unplugged.
            self.battery_since_ms = look.now_ms;
            self.was_on_usb = true;
            return Next::Nap {
                until_ms: look.now_ms.saturating_add(CHECK_MS),
            };
        }
        if self.was_on_usb {
            // Seen unplugged for the first time: from now.
            self.battery_since_ms = look.now_ms;
            self.was_on_usb = false;
        }
        if look.percent.is_some_and(|p| p <= CRITICAL_PERCENT) {
            if self.critical_seen {
                return Next::SwitchOff(Why::Battery);
            }
            // Once more in a minute: one reading may be wrong.
            self.critical_seen = true;
            return Next::Nap {
                until_ms: look.now_ms.saturating_add(CONFIRM_MS),
            };
        }
        self.critical_seen = false;
        let low = look.percent.is_some_and(|p| p <= LOW_PERCENT);
        let after = match (self.off, low) {
            (Off::Now, _) => 0,
            (Off::After(ms), true) => ms.min(LOW_OFF_MS),
            (Off::After(ms), false) => ms,
            (Off::Never, true) => LOW_OFF_MS,
            (Off::Never, false) => {
                return Next::Nap {
                    until_ms: look.now_ms.saturating_add(CHECK_MS),
                };
            }
        };
        let due = self.battery_since_ms.saturating_add(after);
        if look.now_ms >= due {
            let why = match (self.off, low) {
                (Off::Now, _) => Why::Chosen,
                (_, true) if after < self.chosen_ms() => Why::Battery,
                _ => Why::Time,
            };
            return Next::SwitchOff(why);
        }
        Next::Nap {
            until_ms: due.min(look.now_ms.saturating_add(CHECK_MS)),
        }
    }

    /// The reader's choice in milliseconds (`u64::MAX` if never).
    fn chosen_ms(&self) -> u64 {
        match self.off {
            Off::Now => 0,
            Off::After(ms) => ms,
            Off::Never => u64::MAX,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn look(now_min: u64, percent: u8) -> Look {
        Look {
            now_ms: now_min * MIN,
            usb: false,
            percent: Some(percent),
        }
    }

    #[test]
    fn the_choices_mean_what_they_say() {
        assert_eq!(Off::of(NOW), Off::Now);
        assert_eq!(Off::of(M15), Off::After(15 * MIN));
        assert_eq!(Off::of(M30), Off::After(30 * MIN));
        assert_eq!(Off::of(H1), Off::After(60 * MIN));
        assert_eq!(Off::of(H2), Off::After(120 * MIN));
        assert_eq!(Off::of(NEVER), Off::Never);
        // A value from an older or newer file is the default.
        assert_eq!(Off::of("something else"), Off::of(DEFAULT));
        assert!(CHOICES.contains(&DEFAULT));
    }

    #[test]
    fn it_sleeps_in_steps_and_is_switched_off_when_the_time_has_passed() {
        let mut asleep = Asleep::new(Off::of(M30), 5 * MIN);
        assert_eq!(asleep.next(look(5, 80)), Next::Nap { until_ms: 15 * MIN });
        assert_eq!(asleep.next(look(15, 80)), Next::Nap { until_ms: 25 * MIN });
        // The last step is cut short to land on the time.
        assert_eq!(asleep.next(look(25, 79)), Next::Nap { until_ms: 35 * MIN });
        assert_eq!(
            asleep.next(look(35, 79)),
            Next::SwitchOff(Why::Time),
            "30 minutes after 5"
        );
    }

    #[test]
    fn the_last_step_ends_on_the_time() {
        let mut asleep = Asleep::new(Off::of(M15), 0);
        assert_eq!(asleep.next(look(0, 90)), Next::Nap { until_ms: 10 * MIN });
        assert_eq!(asleep.next(look(10, 90)), Next::Nap { until_ms: 15 * MIN });
        assert_eq!(asleep.next(look(15, 90)), Next::SwitchOff(Why::Time));
    }

    #[test]
    fn never_keeps_it_asleep_on_a_healthy_battery() {
        let mut asleep = Asleep::new(Off::of(NEVER), 0);
        for step in 0..1000u64 {
            let now = step * 10;
            assert_eq!(
                asleep.next(look(now, 60)),
                Next::Nap {
                    until_ms: (now + 10) * MIN
                }
            );
        }
    }

    #[test]
    fn a_reader_switched_to_now_is_not_put_to_a_light_sleep() {
        let mut asleep = Asleep::new(Off::of(NOW), 123);
        assert_eq!(
            asleep.next(Look {
                now_ms: 123,
                usb: false,
                percent: Some(100),
            }),
            Next::SwitchOff(Why::Chosen)
        );
    }

    #[test]
    fn a_low_battery_cuts_the_time_short_even_for_never() {
        let mut asleep = Asleep::new(Off::of(NEVER), 0);
        assert_eq!(asleep.next(look(0, 9)), Next::Nap { until_ms: 10 * MIN });
        assert_eq!(asleep.next(look(10, 9)), Next::SwitchOff(Why::Battery));
        let mut asleep = Asleep::new(Off::of(H2), 0);
        assert_eq!(asleep.next(look(0, 50)), Next::Nap { until_ms: 10 * MIN });
        // It dropped below 10 % on the way: 10 minutes after the start.
        assert_eq!(asleep.next(look(30, 8)), Next::SwitchOff(Why::Battery));
        // A short choice is cut short too, to the 10 minutes.
        let mut asleep = Asleep::new(Off::of(M15), 0);
        assert_eq!(asleep.next(look(5, 8)), Next::Nap { until_ms: 10 * MIN });
        assert_eq!(asleep.next(look(10, 8)), Next::SwitchOff(Why::Battery));
        // A choice shorter than that stays its own.
        let mut asleep = Asleep::new(Off::After(5 * MIN), 0);
        assert_eq!(asleep.next(look(5, 8)), Next::SwitchOff(Why::Time));
    }

    #[test]
    fn a_critical_battery_switches_it_off_at_the_second_look() {
        let mut asleep = Asleep::new(Off::of(NEVER), 0);
        assert_eq!(asleep.next(look(1, 3)), Next::Nap { until_ms: 2 * MIN });
        assert_eq!(asleep.next(look(2, 0)), Next::SwitchOff(Why::Battery));
        // A wrong reading is forgotten by the next good one.
        let mut asleep = Asleep::new(Off::of(NEVER), 0);
        assert!(matches!(asleep.next(look(1, 0)), Next::Nap { .. }));
        assert!(matches!(asleep.next(look(2, 60)), Next::Nap { .. }));
        assert!(matches!(asleep.next(look(3, 0)), Next::Nap { .. }));
        assert_eq!(asleep.next(look(4, 1)), Next::SwitchOff(Why::Battery));
        // Not read: no reason to.
        let unread = Look {
            now_ms: MIN,
            usb: false,
            percent: None,
        };
        assert_eq!(asleep.next(unread), Next::Nap { until_ms: 11 * MIN });
    }

    #[test]
    fn on_usb_it_stays_asleep_and_the_time_starts_when_unplugged() {
        let mut asleep = Asleep::new(Off::of(M30), 0);
        let on = |min: u64| Look {
            now_ms: min * MIN,
            usb: true,
            percent: Some(100),
        };
        assert_eq!(asleep.next(on(10)), Next::Nap { until_ms: 20 * MIN });
        assert_eq!(
            asleep.next(on(600)),
            Next::Nap {
                until_ms: 610 * MIN
            }
        );
        // Unplugged at hour ten: the 30 minutes count from there.
        assert_eq!(
            asleep.next(look(610, 100)),
            Next::Nap {
                until_ms: 620 * MIN
            }
        );
        assert_eq!(
            asleep.next(look(630, 100)),
            Next::Nap {
                until_ms: 640 * MIN
            }
        );
        assert_eq!(asleep.next(look(640, 100)), Next::SwitchOff(Why::Time));
        // Even a low battery does not switch a charging reader off.
        let mut asleep = Asleep::new(Off::of(M15), 0);
        let low = Look {
            now_ms: 20 * MIN,
            usb: true,
            percent: Some(2),
        };
        assert!(matches!(asleep.next(low), Next::Nap { .. }));
    }

    #[test]
    fn a_clock_that_ran_far_ahead_switches_off_rather_than_wraps() {
        let mut asleep = Asleep::new(Off::of(H2), u64::MAX - 5);
        assert!(matches!(
            asleep.next(Look {
                now_ms: u64::MAX,
                usb: false,
                percent: Some(50)
            }),
            Next::Nap { .. } | Next::SwitchOff(_)
        ));
    }
}
