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
//! the better state to be in; unplugged, the choice counts from then),
//! except for the choice *Now*, which is how the X4 slept before: switched
//! off at once, USB or not. A battery that cannot be read three looks in a
//! row counts as a low one. While it sleeps, the sleep screen can be drawn
//! again (`command.show_again`, [`Board::show_again_at`]): at its time, never
//! sooner than [`SHOW_MIN_MS`] after the last, only on the cable or with a
//! battery above [`LOW_PERCENT`]; the panel is put down and the sleep goes on.
//! [`run`] is the whole sleep, written against
//! what the board does ([`Board`]) so that its order (radios checked off
//! before the sleep, the watchdogs put back on both ways out, bounded
//! early wakes) is tested on a computer.

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
/// How many looks in a row may find the battery unreadable before it counts
/// as a low one (a failing reading must not keep a reader asleep until the
/// cell is flat).
pub const UNREAD_LOOKS: u8 = 3;
/// How many naps in a row may end early (not by the clock, nor by a press
/// of the power button) before the reader is switched off instead of
/// spinning with nobody looking. A glitch of the button counts like one.
pub const MAX_EARLY: u8 = 5;
/// How long the power button may be held when going to sleep (ms): a
/// reader in a tight case is switched off rather than woken at once.
pub const HELD_MS: u64 = 10_000;
/// The least time between two shows of the sleep screen while asleep
/// (`command.show_again`, ms): a refresh costs about a hundred times a look.
pub const SHOW_MIN_MS: u64 = 5 * 60_000;
/// The watchdog of the sleep (ms): longer than the longest nap
/// ([`CHECK_MS`]), so that one that never ends is a reset, not a flat
/// battery.
pub const SLEEP_WATCHDOG_MS: u64 = CHECK_MS + 5 * 60_000;

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
    /// A radio could not be switched off: no light sleep with it on.
    Radio,
    /// The power button was held down going to sleep (a case, a sleeve).
    Held,
    /// The sleep kept ending early, or the button kept glitching.
    Restless,
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
    /// Looks in a row that could not read the battery.
    unread: u8,
    /// Naps in a row that ended early.
    early: u8,
}

impl Asleep {
    /// Put to sleep at `now_ms` with the reader's choice.
    pub fn new(off: Off, now_ms: u64) -> Self {
        Self {
            off,
            battery_since_ms: now_ms,
            was_on_usb: false,
            critical_seen: false,
            unread: 0,
            early: 0,
        }
    }

    /// What to do now, at the start of the sleep and at each look.
    pub fn next(&mut self, look: Look) -> Next {
        if self.off == Off::Now {
            // As the X4 slept before, with the cable in or not.
            return Next::SwitchOff(Why::Chosen);
        }
        if look.usb {
            // Charging: the time on battery starts afresh when unplugged,
            // and what was seen of the battery before is forgotten.
            self.battery_since_ms = look.now_ms;
            self.was_on_usb = true;
            self.critical_seen = false;
            self.unread = 0;
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
        // A battery that cannot be read for a while is taken for a low one.
        self.unread = if look.percent.is_none() {
            self.unread.saturating_add(1)
        } else {
            0
        };
        let low = look.percent.is_some_and(|p| p <= LOW_PERCENT) || self.unread >= UNREAD_LOOKS;
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

    /// A nap ended not by the clock's time nor by a press of the power
    /// button (`early`, or a glitch of the button): `Some` once that has
    /// happened [`MAX_EARLY`] times in a row, and the reader is to be
    /// switched off. A nap that ended on time starts the count anew.
    fn restless(&mut self, early: bool) -> Option<Why> {
        self.early = if early {
            self.early.saturating_add(1)
        } else {
            0
        };
        (self.early >= MAX_EARLY).then_some(Why::Restless)
    }
}

/// How a nap ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Napped {
    /// The power button, still down after a moment: the reader is woken.
    Pressed,
    /// The power button went down and up again within a moment: no press.
    Glitch,
    /// Anything else: the clock, or something that ended the sleep.
    Ended,
}

/// What the board does for a sleep ([`run`]); the X4's firmware has it
/// (`shells/xteink/src/firmware.rs`), the tests a record of the calls.
pub trait Board {
    /// Milliseconds since the start.
    fn now_ms(&mut self) -> u64;
    /// What it looks at: the time, the cable, a fresh reading of the
    /// battery (`None` if it could not be read, never the last one kept).
    fn look(&mut self) -> Look;
    /// The panel powered down; it keeps its picture.
    fn panel_down(&mut self);
    /// Whether the power button is down.
    fn power_down(&mut self) -> bool;
    /// Takes the escape's request, if Power was held long enough for it
    /// (`crate::escape`, G12): `true` then, and the reader is switched off
    /// (taken, its readings no longer reset the processor).
    fn take_escape(&mut self) -> bool;
    /// Waits `ms` with the readings going, the watchdog fed.
    fn wait(&mut self, ms: u64);
    /// Wi-Fi and Bluetooth asked to go off, a busy task waited for a
    /// while.
    fn leave_radios(&mut self);
    /// Both radios are really off now.
    fn radios_off(&mut self) -> bool;
    /// The readings stop (the power button and the clock are the sources
    /// that end a nap).
    fn pause_inputs(&mut self);
    /// The readings go on.
    fn resume_inputs(&mut self);
    /// The watchdogs for the sleep: the timer group's off, the RTC's armed
    /// for [`SLEEP_WATCHDOG_MS`], which is longer than a nap.
    fn watchdog_for_sleep(&mut self);
    /// The watchdogs as they are awake, for a few seconds.
    fn watchdog_awake(&mut self);
    /// Tells the watchdog the reader is alive.
    fn feed(&mut self);
    /// Sleeps lightly until `until_ms`, or until the power button.
    fn nap(&mut self, until_ms: u64) -> Napped;
    /// Says why the sleep went the way it did (for the owner's log).
    fn say(&mut self, what: &str);
    /// When the sleep screen is to be shown again (`command.show_again`), by
    /// the clock of [`Board::now_ms`]; `None` where nothing is asked for.
    fn show_again_at(&mut self) -> Option<u64> {
        None
    }
    /// Shows the sleep screen again, with the watchdogs awake, the readings
    /// going and the panel up for as long as it takes (the caller puts the
    /// sleep back); `true` where the power button was pressed meanwhile (the
    /// reader is awake then).
    fn show_again(&mut self) -> bool {
        false
    }
}

/// Whether the battery lets the screen be drawn again: on the cable, or
/// above [`LOW_PERCENT`] (a battery that cannot be read, never).
const fn refresh_allowed(look: Look) -> bool {
    look.usb || matches!(look.percent, Some(percent) if percent > LOW_PERCENT)
}

/// When the sleep screen is shown next, by the clock: its time, no sooner than
/// [`SHOW_MIN_MS`] after the last show, and none where the battery does not
/// allow it (`allowed`).
fn show_due(board: &mut impl Board, allowed: bool, last: Option<u64>) -> Option<u64> {
    let at = board.show_again_at().filter(|_| allowed)?;
    Some(last.map_or(at, |last| at.max(last.saturating_add(SHOW_MIN_MS))))
}

/// The sleep screen shown again, then asleep as before; `true` where the
/// power button was pressed meanwhile: awake then, the readings going.
fn show_again(board: &mut impl Board) -> bool {
    board.watchdog_awake();
    board.resume_inputs();
    if board.show_again() {
        return true;
    }
    board.panel_down();
    board.pause_inputs();
    board.watchdog_for_sleep();
    false
}

/// How a light sleep ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The power button woke the reader: awake where it was.
    Woken,
    /// The reader is to be switched off, now, with the watchdogs as they
    /// are awake.
    SwitchOff(Why),
}

/// What the reason `why` of a switch-off from the sleep draws first: the off
/// screen (the slot `core.off`) with this reason (`off-reason`), or nothing.
/// Only the time of *Switch off after sleeping* does: the sleep screen is
/// shown already for the choice *Now*, and a low or critical battery, a radio
/// that would not go off, a held button or a restless sleep cut the power
/// without a frame (a refresh that is not paid for).
#[must_use]
pub const fn off_screen(why: Why) -> Option<&'static str> {
    match why {
        Why::Time => Some("timer"),
        Why::Chosen | Why::Battery | Why::Radio | Why::Held | Why::Restless => None,
    }
}

/// What the main loop does with the kernel's requests to sleep and to
/// switch off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    /// Sleep (lightly first, then switched off after a while).
    Sleep,
    /// Switch the device off now: a plugin asked (`Effect::PowerOff`).
    SwitchOff,
}

/// What to do now with a request to `sleep` and one to switch off
/// (`power_off`): nothing while an update installs (the loop comes by
/// again), a switch-off before a sleep (it is the stronger request).
pub fn wanted(sleep: bool, power_off: bool, installing: bool) -> Option<Wanted> {
    if installing {
        None
    } else if power_off {
        Some(Wanted::SwitchOff)
    } else if sleep {
        Some(Wanted::Sleep)
    } else {
        None
    }
}

/// What a sleep does once the plugins have heard `core.suspend` and the
/// pending work is finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// A press came meanwhile: awake after all; the requests are dropped.
    Awake,
    /// Sleep lightly, to be switched off after the chosen time.
    Light,
    /// The power goes now.
    SwitchOff,
}

/// The step after `core.suspend` and the finished work. `lightly`: a real
/// sleep first is wanted (the sleep, not a switch-off request), `power_off`:
/// a plugin asked to switch off meanwhile (while it answered `core.suspend`,
/// or in the work finished): that wins, or the reader would sleep and be
/// switched off the moment it woke. `pressed`: a press came meanwhile.
pub fn after_suspend(lightly: bool, power_off: bool, pressed: bool) -> After {
    if pressed {
        After::Awake
    } else if power_off || !lightly {
        After::SwitchOff
    } else {
        After::Light
    }
}

/// How often the power button is looked at while it is waited for (ms).
const RELEASE_POLL_MS: u64 = 10;

/// Waits until the power button is let go (`true`): held, it would wake the
/// reader at once. `false`, switched off, if it is held for [`HELD_MS`] (a
/// case, a sleeve) or long enough for the escape to ask (G12, from the
/// press that put the reader to sleep): its readings go on until the sleep
/// pauses them and would reset the processor if the request were left.
fn button_released(board: &mut impl Board) -> bool {
    let until = board.now_ms().saturating_add(HELD_MS);
    while board.power_down() {
        if board.take_escape() || board.now_ms() > until {
            return false;
        }
        board.wait(RELEASE_POLL_MS);
    }
    true
}

/// One sleep of a reader put to sleep on battery or on USB: checks that it
/// can sleep lightly (the power button let go, the radios off), naps until
/// the power button or the time of [`Asleep::next`], and ends either way
/// with the watchdogs put back.
pub fn run(off: Off, board: &mut impl Board) -> Ended {
    let mut asleep = Asleep::new(off, board.now_ms());
    let first = board.look();
    let Next::Nap { until_ms } = asleep.next(first) else {
        board.say("switched off at once");
        return Ended::SwitchOff(Why::Chosen);
    };
    board.panel_down();
    if !button_released(board) {
        board.say("the power button is held: switching off");
        return Ended::SwitchOff(Why::Held);
    }
    board.leave_radios();
    // A radio left on (a join under way that did not end in time) would go
    // into the sleep with its driver started: the old way out instead.
    if !board.radios_off() {
        board.say("a radio is still on: switching off, no light sleep");
        return Ended::SwitchOff(Why::Radio);
    }
    board.pause_inputs();
    board.watchdog_for_sleep();
    let mut until = until_ms;
    let mut allowed = refresh_allowed(first);
    let mut shown_at = None;
    let ended = loop {
        board.feed();
        let wake = show_due(board, allowed, shown_at).map_or(until, |due| until.min(due));
        match board.nap(wake) {
            Napped::Pressed => {
                board.say("the power button woke it");
                break Ended::Woken;
            }
            Napped::Glitch => {
                if let Some(why) = asleep.restless(true) {
                    board.say("the power button keeps glitching: switching off");
                    break Ended::SwitchOff(why);
                }
            }
            Napped::Ended => {
                let early = board.now_ms().saturating_add(1000) < wake;
                if let Some(why) = asleep.restless(early) {
                    board.say("light sleep keeps ending early: switching off");
                    break Ended::SwitchOff(why);
                }
                board.feed();
                let look = board.look();
                match asleep.next(look) {
                    Next::Nap { until_ms } => {
                        until = until_ms;
                        allowed = refresh_allowed(look);
                        let now = board.now_ms();
                        if show_due(board, allowed, shown_at).is_some_and(|due| due <= now) {
                            board.say("showing the sleep screen again");
                            if show_again(board) {
                                board.say("the power button woke it while shown");
                                break Ended::Woken;
                            }
                            shown_at = Some(board.now_ms());
                        }
                    }
                    Next::SwitchOff(why) => {
                        board.say(match why {
                            Why::Battery => "the battery is low: switching off",
                            _ => "the time has passed: switching off",
                        });
                        break Ended::SwitchOff(why);
                    }
                }
            }
        }
    };
    // Both ways out: the watchdogs as awake, before anything else (a
    // card write for the switch-off, the radio, the first frame).
    board.watchdog_awake();
    if ended == Ended::Woken {
        board.resume_inputs();
    }
    ended
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;
    use crate::escape::{self, Escape, Step};

    /// The reason of the switch-off a sleeping reader ends in, for the
    /// look at `percent` after `after_ms` (on battery, the choice 15 minutes).
    fn why_off(percent: u8, looks: &[u64]) -> Why {
        let mut asleep = Asleep::new(Off::of(M15), 0);
        for &now_ms in looks {
            let look = Look {
                now_ms,
                usb: false,
                percent: Some(percent),
            };
            if let Next::SwitchOff(why) = asleep.next(look) {
                return why;
            }
        }
        panic!("never switched off");
    }

    #[test]
    fn the_off_screen_is_drawn_for_the_timer_and_not_for_a_critical_battery() {
        let minute = 60_000;
        // The time of the choice passed on a healthy battery: drawn, told
        // `timer`.
        let timer = why_off(80, &[0, 10 * minute, 15 * minute]);
        assert_eq!(timer, Why::Time);
        assert_eq!(off_screen(timer), Some("timer"));
        // A critical battery (seen twice, a minute apart) cuts the power
        // without a frame.
        let critical = why_off(2, &[10 * minute, 11 * minute]);
        assert_eq!(critical, Why::Battery);
        assert_eq!(off_screen(critical), None);
        for why in [Why::Chosen, Why::Radio, Why::Held, Why::Restless] {
            assert_eq!(off_screen(why), None, "{why:?}");
        }
    }

    #[test]
    fn a_switch_off_request_beats_a_sleep_request() {
        assert_eq!(wanted(false, false, false), None);
        assert_eq!(wanted(true, false, false), Some(Wanted::Sleep));
        assert_eq!(wanted(false, true, false), Some(Wanted::SwitchOff));
        assert_eq!(wanted(true, true, false), Some(Wanted::SwitchOff));
    }

    #[test]
    fn a_press_meanwhile_cancels_both_requests() {
        for lightly in [false, true] {
            for power_off in [false, true] {
                assert_eq!(after_suspend(lightly, power_off, true), After::Awake);
            }
        }
    }

    #[test]
    fn a_switch_off_asked_while_going_to_sleep_is_honoured_now() {
        // The sleep was begun lightly; the plugin answered core.suspend with
        // a switch-off: no light sleep, or it would go off at the wake.
        assert_eq!(after_suspend(true, true, false), After::SwitchOff);
        assert_eq!(after_suspend(false, true, false), After::SwitchOff);
    }

    #[test]
    fn a_plain_sleep_goes_light_and_a_switch_off_request_goes_off() {
        assert_eq!(after_suspend(true, false, false), After::Light);
        assert_eq!(after_suspend(false, false, false), After::SwitchOff);
    }

    #[test]
    fn nothing_ends_while_an_update_installs() {
        assert_eq!(wanted(true, true, true), None);
        assert_eq!(wanted(false, true, true), None);
    }

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
        // The time due would be past the end of the clock: saturated, so it
        // is due when the clock gets there, not wrapped to a time long gone.
        let mut asleep = Asleep::new(Off::of(H2), u64::MAX - 5);
        let at = |now_ms| Look {
            now_ms,
            usb: false,
            percent: Some(50),
        };
        assert_eq!(asleep.next(at(u64::MAX)), Next::SwitchOff(Why::Time));
        // Before that it naps, to a time that is not before now.
        let mut asleep = Asleep::new(Off::of(H2), u64::MAX - 5);
        match asleep.next(at(u64::MAX - 5)) {
            Next::Nap { until_ms } => assert!(until_ms >= u64::MAX - 5, "{until_ms}"),
            Next::SwitchOff(why) => panic!("switched off for {why:?}"),
        }
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_wrap() {
        let mut asleep = Asleep::new(Off::of(M30), 100 * MIN);
        // A look earlier than the start of the sleep: not due, and the nap
        // ends later than now, not at a wrapped time.
        match asleep.next(look(50, 80)) {
            Next::Nap { until_ms } => assert!(until_ms > 50 * MIN, "{until_ms}"),
            Next::SwitchOff(why) => panic!("switched off for {why:?}"),
        }
    }

    #[test]
    fn now_is_how_the_x4_slept_before_even_on_usb() {
        let mut asleep = Asleep::new(Off::of(NOW), 0);
        let usb = Look {
            now_ms: 0,
            usb: true,
            percent: Some(100),
        };
        assert_eq!(asleep.next(usb), Next::SwitchOff(Why::Chosen));
        // Other choices still stay asleep on the cable.
        let mut asleep = Asleep::new(Off::of(M15), 0);
        assert!(matches!(asleep.next(usb), Next::Nap { .. }));
    }

    #[test]
    fn a_battery_that_cannot_be_read_for_three_looks_counts_as_low() {
        let unread = |min: u64| Look {
            now_ms: min * MIN,
            usb: false,
            percent: None,
        };
        let mut asleep = Asleep::new(Off::of(NEVER), 0);
        assert!(matches!(asleep.next(unread(0)), Next::Nap { .. }));
        assert!(matches!(asleep.next(unread(10)), Next::Nap { .. }));
        // The third in a row: low, and ten minutes of sleep have passed.
        assert_eq!(asleep.next(unread(20)), Next::SwitchOff(Why::Battery));
        // A good reading in between starts the count anew.
        let mut asleep = Asleep::new(Off::of(NEVER), 0);
        assert!(matches!(asleep.next(unread(0)), Next::Nap { .. }));
        assert!(matches!(asleep.next(unread(10)), Next::Nap { .. }));
        assert!(matches!(asleep.next(look(20, 80)), Next::Nap { .. }));
        assert!(matches!(asleep.next(unread(30)), Next::Nap { .. }));
        assert!(matches!(asleep.next(unread(40)), Next::Nap { .. }));
        assert_eq!(asleep.next(unread(50)), Next::SwitchOff(Why::Battery));
    }

    #[test]
    fn a_critical_reading_before_the_cable_is_not_remembered_after_it() {
        let mut asleep = Asleep::new(Off::of(NEVER), 0);
        assert!(matches!(asleep.next(look(1, 2)), Next::Nap { .. }));
        let usb = Look {
            now_ms: 11 * MIN,
            usb: true,
            percent: Some(50),
        };
        assert!(matches!(asleep.next(usb), Next::Nap { .. }));
        // Unplugged with a bad reading: once more to be sure, not off.
        assert!(matches!(asleep.next(look(30, 2)), Next::Nap { .. }));
        assert_eq!(asleep.next(look(31, 2)), Next::SwitchOff(Why::Battery));
    }

    /// A board that records what was done to it and plays a script of
    /// naps.
    struct Fake {
        now: u64,
        calls: Vec<String>,
        naps: Vec<(Napped, u64)>,
        batteries: Vec<Option<u8>>,
        usb: bool,
        held: bool,
        /// The escape's readings of the power button (every 5 ms while it
        /// waits), as the input interrupt makes them.
        escape: Escape,
        asked: bool,
        taken: bool,
        /// The readings reset the processor (`Step::Force`).
        forced: bool,
        radios_stay_on: bool,
        asleep: bool,
        /// When the sleep screen is wanted again; cleared once it is shown.
        show_at: Option<u64>,
        /// The power button is pressed while the sleep screen is shown again.
        pressed_while_shown: bool,
    }

    impl Fake {
        fn new(naps: Vec<(Napped, u64)>) -> Self {
            Self {
                now: 0,
                calls: Vec::new(),
                naps,
                batteries: Vec::new(),
                usb: false,
                held: false,
                escape: Escape::new(),
                asked: false,
                taken: false,
                forced: false,
                radios_stay_on: false,
                asleep: false,
                show_at: None,
                pressed_while_shown: false,
            }
        }

        fn at(&self, call: &str) -> Option<usize> {
            self.calls.iter().position(|c| c == call)
        }

        fn last(&self, call: &str) -> Option<usize> {
            self.calls.iter().rposition(|c| c == call)
        }
        /// The escape's readings until `to` (ms), the button as `held`.
        fn read_until(&mut self, to: u64) {
            while self.now < to {
                self.now += 5;
                match self.escape.update(self.now, self.held, true, self.taken) {
                    Step::Idle => {}
                    Step::Ask => self.asked = true,
                    Step::Force => self.forced = true,
                }
            }
        }
    }

    impl Board for Fake {
        fn now_ms(&mut self) -> u64 {
            self.now
        }
        fn look(&mut self) -> Look {
            let percent = if self.batteries.is_empty() {
                Some(80)
            } else {
                self.batteries.remove(0)
            };
            Look {
                now_ms: self.now,
                usb: self.usb,
                percent,
            }
        }
        fn panel_down(&mut self) {
            self.calls.push("panel".into());
        }
        fn power_down(&mut self) -> bool {
            self.held
        }
        fn take_escape(&mut self) -> bool {
            let take = self.asked && !self.taken;
            self.taken |= take;
            take
        }
        fn wait(&mut self, ms: u64) {
            self.read_until(self.now + ms);
        }
        fn leave_radios(&mut self) {
            self.calls.push("leave".into());
        }
        fn radios_off(&mut self) -> bool {
            self.calls.push("radios".into());
            !self.radios_stay_on
        }
        fn pause_inputs(&mut self) {
            self.calls.push("pause".into());
        }
        fn resume_inputs(&mut self) {
            self.calls.push("resume".into());
        }
        fn watchdog_for_sleep(&mut self) {
            self.calls.push("wd-sleep".into());
            self.asleep = true;
        }
        fn watchdog_awake(&mut self) {
            self.calls.push("wd-awake".into());
            self.asleep = false;
        }
        fn feed(&mut self) {
            self.calls.push("feed".into());
        }
        fn nap(&mut self, until_ms: u64) -> Napped {
            assert!(self.asleep, "a nap with the watchdogs awake or none");
            assert!(
                until_ms - self.now <= CHECK_MS,
                "a nap longer than the watchdog's room"
            );
            self.calls.push("nap".into());
            let (how, took) = self.naps.remove(0);
            self.now += took;
            how
        }
        fn say(&mut self, what: &str) {
            self.calls.push(format!("say {what}"));
        }
        fn show_again_at(&mut self) -> Option<u64> {
            self.show_at
        }
        fn show_again(&mut self) -> bool {
            assert!(!self.asleep, "drawn with the watchdogs of the sleep");
            self.calls.push("show".into());
            self.show_at = None;
            self.pressed_while_shown
        }
    }

    #[test]
    fn the_sleep_screen_is_shown_again_at_its_time_and_the_sleep_goes_on() {
        // Wanted at 15 minutes: the first nap ends at the look (10), the
        // next at the time asked for (15), then the button.
        let mut board = Fake::new(vec![
            (Napped::Ended, 10 * MIN),
            (Napped::Ended, 5 * MIN),
            (Napped::Pressed, 0),
        ]);
        board.show_at = Some(15 * MIN);
        assert_eq!(run(Off::of(NEVER), &mut board), Ended::Woken);
        assert_eq!(board.calls.iter().filter(|c| *c == "show").count(), 1);
        let show = board.at("show").unwrap();
        // Not before its time: two naps went before it, the last after.
        assert_eq!(
            board.calls[..show].iter().filter(|c| *c == "nap").count(),
            2
        );
        assert!(board.calls[show - 1..show].contains(&"resume".into()));
        // Asleep again as before: the panel down, the readings stopped.
        let after: Vec<_> = board.calls[show + 1..].iter().map(String::as_str).collect();
        assert_eq!(&after[..3], ["panel", "pause", "wd-sleep"]);
        assert!(after.contains(&"nap"));
    }

    /// The power button pressed while the sleep screen is drawn again wakes
    /// the reader there, as in a nap: no nap after it, the readings going
    /// and the watchdogs awake.
    #[test]
    fn a_press_while_the_sleep_screen_is_shown_again_wakes_the_reader() {
        let mut board = Fake::new(vec![
            (Napped::Ended, 10 * MIN),
            (Napped::Ended, 5 * MIN),
            (Napped::Ended, 10 * MIN),
        ]);
        board.show_at = Some(15 * MIN);
        board.pressed_while_shown = true;
        assert_eq!(run(Off::of(NEVER), &mut board), Ended::Woken);
        let show = board.at("show").unwrap();
        let after: Vec<_> = board.calls[show + 1..].iter().map(String::as_str).collect();
        assert!(!after.contains(&"nap"), "{after:?}");
        assert!(!after.contains(&"pause"), "{after:?}");
        assert_eq!(after.last(), Some(&"resume"));
        assert!(!board.asleep);
    }

    #[test]
    fn nothing_is_shown_again_that_was_not_asked_for() {
        let mut board = Fake::new(vec![(Napped::Ended, 10 * MIN), (Napped::Pressed, 0)]);
        assert_eq!(run(Off::of(NEVER), &mut board), Ended::Woken);
        assert_eq!(board.at("show"), None);
    }

    #[test]
    fn the_watchdogs_are_put_back_on_both_ways_out() {
        // Woken by the button after one nap.
        let mut board = Fake::new(vec![(Napped::Pressed, 3 * MIN)]);
        assert_eq!(run(Off::of(M30), &mut board), Ended::Woken);
        let sleep = board.at("wd-sleep").unwrap();
        assert!(sleep < board.at("nap").unwrap());
        assert!(board.at("feed").unwrap() < board.at("nap").unwrap());
        assert!(board.last("wd-awake").unwrap() > board.last("nap").unwrap());
        assert!(board.at("resume").unwrap() > board.last("wd-awake").unwrap());
        assert!(!board.asleep, "the sleep watchdog is gone");

        // Switched off by the time: the watchdogs as awake before the
        // caller writes to the card and cuts the power.
        let mut board = Fake::new(vec![(Napped::Ended, 10 * MIN), (Napped::Ended, 5 * MIN)]);
        assert_eq!(run(Off::of(M15), &mut board), Ended::SwitchOff(Why::Time));
        assert!(!board.asleep);
        assert_eq!(board.calls.last().map(String::as_str), Some("wd-awake"));
        assert_eq!(board.at("resume"), None, "no readings for a switch-off");
    }

    #[test]
    fn every_nap_is_fed_first() {
        let mut board = Fake::new(vec![
            (Napped::Ended, 10 * MIN),
            (Napped::Ended, 10 * MIN),
            (Napped::Pressed, MIN),
        ]);
        run(Off::of(NEVER), &mut board);
        let mut fed = false;
        for call in &board.calls {
            match call.as_str() {
                "feed" => fed = true,
                "nap" => {
                    assert!(fed, "a nap without a feed before it");
                    fed = false;
                }
                _ => {}
            }
        }
    }

    #[test]
    fn a_radio_left_on_is_no_light_sleep() {
        let mut board = Fake::new(vec![]);
        board.radios_stay_on = true;
        assert_eq!(run(Off::of(M30), &mut board), Ended::SwitchOff(Why::Radio));
        assert!(board.at("leave").unwrap() < board.at("radios").unwrap());
        assert_eq!(board.at("nap"), None);
        assert_eq!(board.at("wd-sleep"), None);
        assert_eq!(board.at("pause"), None, "the readings went on");
        assert!(board.calls.iter().any(|c| c.contains("radio")), "said");
    }

    #[test]
    fn a_held_button_is_a_switch_off_not_a_wake() {
        let mut board = Fake::new(vec![]);
        board.held = true;
        assert_eq!(run(Off::of(M30), &mut board), Ended::SwitchOff(Why::Held));
        assert_eq!(board.at("nap"), None);
        assert_eq!(board.at("leave"), None);
    }

    #[test]
    fn power_held_through_the_sleep_entry_ends_held_not_forced() {
        // Power pressed at 0 to sleep and held on (to switch off, or a
        // sleeve): `core.suspend` and the work finished take 2 s, then the
        // sleep waits for the button. The escape asks at 8 s, from the
        // press; the wait takes it, and the reader is switched off the clean
        // way, not reset 1.5 s later by the readings.
        let mut board = Fake::new(vec![]);
        board.held = true;
        board.read_until(2_000);
        assert_eq!(run(Off::of(M30), &mut board), Ended::SwitchOff(Why::Held));
        assert!(board.taken, "the escape's request is taken");
        assert!(!board.forced, "the processor is not reset");
        assert!(board.now < escape::HOLD_MS + escape::GRACE_MS);
        assert_eq!(board.at("nap"), None);
    }

    #[test]
    fn now_with_the_cable_in_does_not_start_to_sleep() {
        let mut board = Fake::new(vec![]);
        board.usb = true;
        assert_eq!(run(Off::of(NOW), &mut board), Ended::SwitchOff(Why::Chosen));
        assert_eq!(board.at("panel"), None);
        assert_eq!(board.at("nap"), None);
    }

    #[test]
    fn naps_that_keep_ending_early_end_in_a_switch_off() {
        // A clock that does not advance: every nap ends "early".
        let naps = (0..10).map(|_| (Napped::Ended, 0)).collect();
        let mut board = Fake::new(naps);
        assert_eq!(
            run(Off::of(NEVER), &mut board),
            Ended::SwitchOff(Why::Restless)
        );
        assert_eq!(board.calls.iter().filter(|c| *c == "nap").count(), 5);
        assert!(!board.asleep);
    }

    #[test]
    fn a_nap_that_ends_on_time_starts_the_count_anew() {
        let mut naps = Vec::new();
        for _ in 0..4 {
            naps.push((Napped::Ended, 0));
        }
        naps.push((Napped::Ended, 10 * MIN));
        for _ in 0..4 {
            naps.push((Napped::Ended, 0));
        }
        naps.push((Napped::Pressed, 0));
        let mut board = Fake::new(naps);
        assert_eq!(run(Off::of(NEVER), &mut board), Ended::Woken);
    }

    #[test]
    fn a_glitching_button_cannot_spin_the_reader_awake() {
        let naps = (0..20).map(|_| (Napped::Glitch, 0)).collect();
        let mut board = Fake::new(naps);
        assert_eq!(
            run(Off::of(NEVER), &mut board),
            Ended::SwitchOff(Why::Restless)
        );
        assert_eq!(board.calls.iter().filter(|c| *c == "nap").count(), 5);
        assert!(!board.asleep);
    }

    #[test]
    fn a_battery_that_cannot_be_read_switches_off_even_for_never() {
        let mut board = Fake::new(
            (0..10)
                .map(|_| (Napped::Ended, 10 * MIN))
                .collect::<Vec<_>>(),
        );
        board.batteries = vec![Some(80); 1]
            .into_iter()
            .chain((0..9).map(|_| None))
            .collect();
        assert_eq!(
            run(Off::of(NEVER), &mut board),
            Ended::SwitchOff(Why::Battery)
        );
    }
}
