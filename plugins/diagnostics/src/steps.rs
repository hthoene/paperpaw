//! The device check's steps and what came of them, apart from how they
//! are shown: what each step tests, the verdict its measurements suggest,
//! and the report written to the card after each answer. The report is
//! also where an unfinished check goes on from, e.g. after the device
//! slept with its power cut.
//!
//! The report is in English whatever the language set: it is pasted into
//! pull requests. Each step is a line `<step>: <outcome>`, with a note in
//! parentheses where there is one.

use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::{join, push},
    text::{copy, format, write_to},
};

/// Where the report is written, in the plugin's data folder.
pub const REPORT: &str = "device-check.txt";

/// What the device has, as far as the steps care.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Facts {
    /// The device has buttons.
    pub buttons: bool,
    /// The device has a touch screen.
    pub touch: bool,
    /// The device has a front light.
    pub frontlight: bool,
    /// PaperPaw manages the Wi-Fi.
    pub wifi: bool,
    /// PaperPaw puts the device to sleep.
    pub sleep: bool,
}

/// The battery as read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryReading {
    /// Charge in percent.
    pub percent: u8,
    /// Voltage, where known.
    pub millivolts: Option<u16>,
}

/// The heap and stack as read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryReading {
    /// Free heap in bytes.
    pub heap_free: u32,
    /// Heap size in bytes.
    pub heap_size: u32,
    /// Stack never used, where known.
    pub stack_free: Option<u32>,
    /// Stack size, where known.
    pub stack_size: Option<u32>,
}

/// The parts of a device the check goes through, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Gray levels, ghosting and the refresh modes.
    Screen,
    /// Every button, asked for by name.
    Buttons,
    /// Taps in the corners and at the edges.
    Touch,
    /// The front light's brightness and warmth.
    Light,
    /// The battery's charge, voltage and charging.
    Battery,
    /// Writing to the card and reading back, with their speed.
    Card,
    /// The time of day.
    Clock,
    /// Looking for networks and joining one.
    Wifi,
    /// Sleeping and waking again.
    Sleep,
    /// The heap and the stack free.
    Memory,
}

impl Step {
    pub const ALL: [Step; 10] = [
        Step::Screen,
        Step::Buttons,
        Step::Touch,
        Step::Light,
        Step::Battery,
        Step::Card,
        Step::Clock,
        Step::Wifi,
        Step::Sleep,
        Step::Memory,
    ];

    /// Its name in the report and in translation keys.
    pub fn key(self) -> &'static str {
        match self {
            Step::Screen => "screen",
            Step::Buttons => "buttons",
            Step::Touch => "touch",
            Step::Light => "light",
            Step::Battery => "battery",
            Step::Card => "card",
            Step::Clock => "clock",
            Step::Wifi => "wifi",
            Step::Sleep => "sleep",
            Step::Memory => "memory",
        }
    }

    pub fn index(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    fn from_key(key: &str) -> Option<Step> {
        Step::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Why the step does not apply to a device, which has no such part:
    /// the note of the skip. `None` where it applies.
    pub fn missing(self, facts: &Facts) -> Option<&'static str> {
        match self {
            Step::Buttons if !facts.buttons => Some("no buttons"),
            Step::Touch if !facts.touch => Some("no touch screen"),
            Step::Light if !facts.frontlight => Some("no front light"),
            Step::Wifi if !facts.wifi => Some("Wi-Fi not managed by PaperPaw"),
            Step::Sleep if !facts.sleep => Some("sleep not managed by PaperPaw"),
            _ => None,
        }
    }
}

/// What came of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    /// Not tried, e.g. where the device lacks the part.
    Skip,
}

impl Outcome {
    pub const ALL: [Outcome; 3] = [Outcome::Fail, Outcome::Skip, Outcome::Pass];

    pub fn key(self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "fail",
            Outcome::Skip => "skip",
        }
    }

    fn from_key(key: &str) -> Option<Outcome> {
        Outcome::ALL.into_iter().find(|o| o.key() == key)
    }
}

/// A step's mark in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// Answered, with a note on what was measured (may be empty).
    Done(Outcome, String),
    /// The device was sent to sleep; whatever shows the check next means
    /// it woke.
    Asleep,
}

/// The word of [`Mark::Asleep`] in the report.
const ASLEEP: &str = "asleep";

/// The marks of all steps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Results {
    marks: [Option<Mark>; Step::ALL.len()],
}

/// What the report says about the device, above the steps.
#[derive(Debug, Clone, Copy)]
pub struct About<'a> {
    /// The device's name.
    pub name: &'a str,
    /// The device's id.
    pub id: &'a str,
    /// Pixels per inch.
    pub dpi: u16,
    /// The panel's pixel format, as the report names it.
    pub format: &'a str,
    pub version: &'a str,
    /// When, as `2026-10-23 14:02 UTC`, where the time is known.
    pub when: Option<&'a str>,
}

impl Results {
    pub fn get(&self, step: Step) -> Option<&Mark> {
        self.marks[step.index()].as_ref()
    }

    pub fn set(&mut self, step: Step, mark: Mark) {
        self.marks[step.index()] = Some(mark);
    }

    /// The outcome of a step answered.
    pub fn outcome(&self, step: Step) -> Option<Outcome> {
        match self.get(step) {
            Some(Mark::Done(outcome, _)) => Some(*outcome),
            _ => None,
        }
    }

    /// The first step not answered yet, where the check goes on; `None`
    /// when all are.
    pub fn next_open(&self) -> Option<Step> {
        Step::ALL.into_iter().find(|s| self.outcome(*s).is_none())
    }

    /// Whether nothing is marked.
    pub fn is_empty(&self) -> bool {
        self.marks.iter().all(Option::is_none)
    }

    /// Steps passed, failed and skipped.
    pub fn counts(&self) -> (usize, usize, usize) {
        let count = |o| {
            Step::ALL
                .iter()
                .filter(|s| self.outcome(**s) == Some(o))
                .count()
        };
        (
            count(Outcome::Pass),
            count(Outcome::Fail),
            count(Outcome::Skip),
        )
    }

    /// The report as written to the card.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the text cannot grow.
    pub fn report(&self, about: &About<'_>) -> Result<String, Error> {
        let mut out = copy("PaperPaw device check\n")?;
        write_to(
            &mut out,
            format_args!(
                "Device: {} ({}), {} dpi, {}\n",
                about.name, about.id, about.dpi, about.format
            ),
        )?;
        write_to(&mut out, format_args!("PaperPaw: {}\n", about.version))?;
        if let Some(when) = about.when {
            write_to(&mut out, format_args!("Date: {when}\n"))?;
        }
        write_to(&mut out, format_args!("\n"))?;
        for step in Step::ALL {
            match self.get(step) {
                Some(Mark::Done(outcome, note)) if note.is_empty() => {
                    write_to(
                        &mut out,
                        format_args!("{}: {}\n", step.key(), outcome.key()),
                    )?;
                }
                Some(Mark::Done(outcome, note)) => {
                    write_to(
                        &mut out,
                        format_args!("{}: {} ({note})\n", step.key(), outcome.key()),
                    )?;
                }
                Some(Mark::Asleep) => {
                    write_to(&mut out, format_args!("{}: {ASLEEP}\n", step.key()))?;
                }
                None => {}
            }
        }
        let (pass, fail, skip) = self.counts();
        write_to(
            &mut out,
            format_args!("\nResult: {pass} passed, {fail} failed, {skip} skipped\n"),
        )?;
        Ok(out)
    }

    /// The marks of a report written before; lines it does not know are
    /// passed over.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when a note cannot be copied.
    pub fn parse(report: &str) -> Result<Results, Error> {
        let mut results = Results::default();
        for line in report.lines() {
            let Some((key, rest)) = line.split_once(": ") else {
                continue;
            };
            let Some(step) = Step::from_key(key) else {
                continue;
            };
            let (word, note) = match rest.split_once(" (") {
                Some((word, note)) => (word, note.strip_suffix(')').unwrap_or(note)),
                None => (rest, ""),
            };
            if word == ASLEEP {
                results.set(step, Mark::Asleep);
            } else if let Some(outcome) = Outcome::from_key(word) {
                results.set(step, Mark::Done(outcome, copy(note)?));
            }
        }
        Ok(results)
    }
}

/// The buttons asked for in turn, and what came of each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Buttons {
    /// The buttons to press, the device's.
    pub names: Vec<String>,
    /// Those pressed when asked.
    pub pressed: Vec<String>,
    /// Those given up on: another one was held instead.
    pub failed: Vec<String>,
}

impl Buttons {
    pub fn new(names: Vec<String>) -> Self {
        Self {
            names,
            ..Self::default()
        }
    }

    /// The button asked for now; `None` once all were.
    pub fn asked(&self) -> Option<&str> {
        self.names
            .iter()
            .find(|n| !self.pressed.contains(n) && !self.failed.contains(n))
            .map(String::as_str)
    }

    /// Takes button `name` pressed (`long`: held). The one asked for
    /// counts as pressed; another held gives the one asked up as broken;
    /// another pressed briefly is a slip and changes nothing. Whether it
    /// changed anything.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the answer cannot be kept.
    pub fn press(&mut self, name: &str, long: bool) -> Result<bool, Error> {
        let Some(asked) = self.asked().map(copy).transpose()? else {
            return Ok(false);
        };
        if name == asked {
            push(&mut self.pressed, asked)?;
        } else if long {
            push(&mut self.failed, asked)?;
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    /// The outcome and note once all were asked.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the note cannot be made.
    pub fn verdict(&self) -> Result<(Outcome, String), Error> {
        if self.failed.is_empty() {
            Ok((Outcome::Pass, join(&self.pressed, ", ")?))
        } else {
            Ok((
                Outcome::Fail,
                format(format_args!("not working: {}", join(&self.failed, ", ")?))?,
            ))
        }
    }
}

/// The places to tap in the touch step: the corners and the middles of
/// the edges, as fractions of the screen (in tenths), inset by half a
/// tenth so a finger can reach them.
pub const TOUCH_TARGETS: [(u8, u8); 8] = [
    (0, 0),
    (5, 0),
    (10, 0),
    (0, 5),
    (10, 5),
    (0, 10),
    (5, 10),
    (10, 10),
];

/// The battery's verdict: values a battery can have pass.
///
/// # Errors
/// [`Error::NoMemory`] when the note cannot be made.
pub fn battery(
    battery: Option<BatteryReading>,
    charging_seen: [bool; 2],
) -> Result<(Outcome, String), Error> {
    let Some(b) = battery else {
        return Ok((Outcome::Skip, copy("no battery reported")?));
    };
    let mut note = format(format_args!("{} %", b.percent))?;
    let volts_fit = match b.millivolts {
        Some(mv) => {
            write_to(&mut note, format_args!(", {mv} mV"))?;
            (2800..=4500).contains(&mv)
        }
        None => true,
    };
    let seen = match charging_seen {
        [true, true] => "charging on and off",
        [true, false] => "charging on only",
        [false, true] => "charging off only",
        [false, false] => "charging not known",
    };
    write_to(&mut note, format_args!(", {seen}"))?;
    let outcome = if b.percent <= 100 && volts_fit {
        Outcome::Pass
    } else {
        Outcome::Fail
    };
    Ok((outcome, note))
}

/// Speed in KiB per second of `bytes` in `micros`.
pub fn speed(bytes: usize, micros: u64) -> u64 {
    (bytes as u64 * 1_000_000 / 1024) / micros.max(1)
}

/// What the card's test measured: microseconds to write and to read
/// back, where the device has a clock, and an error where one came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardTest {
    pub bytes: usize,
    pub write_us: Option<u64>,
    pub read_us: Option<u64>,
    pub error: Option<String>,
}

impl CardTest {
    /// # Errors
    /// [`Error::NoMemory`] when the note cannot be made.
    pub fn verdict(&self) -> Result<(Outcome, String), Error> {
        if let Some(error) = &self.error {
            return Ok((Outcome::Fail, copy(error)?));
        }
        let mut note = format(format_args!("{} KiB", self.bytes / 1024))?;
        if let (Some(w), Some(r)) = (self.write_us, self.read_us) {
            write_to(
                &mut note,
                format_args!(
                    ", write {} KiB/s, read {} KiB/s",
                    speed(self.bytes, w),
                    speed(self.bytes, r)
                ),
            )?;
        }
        Ok((Outcome::Pass, note))
    }
}

/// The memory's verdict: a stack with less than this left at its deepest
/// is near running over.
pub const STACK_SPARE: u32 = 1024;

///
/// # Errors
/// [`Error::NoMemory`] when the note cannot be made.
pub fn memory(memory: Option<MemoryReading>) -> Result<(Outcome, String), Error> {
    let Some(m) = memory else {
        return Ok((Outcome::Skip, copy("not reported")?));
    };
    let kib = |bytes: u32| bytes / 1024;
    let mut note = format(format_args!(
        "heap {} KiB free of {} KiB",
        kib(m.heap_free),
        kib(m.heap_size)
    ))?;
    let mut outcome = Outcome::Pass;
    if let (Some(free), Some(size)) = (m.stack_free, m.stack_size) {
        write_to(
            &mut note,
            format_args!(", stack {free} B never used of {size} B"),
        )?;
        if free < STACK_SPARE {
            outcome = Outcome::Fail;
        }
    }
    Ok((outcome, note))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(reading: Option<BatteryReading>, seen: [bool; 2]) -> (Outcome, String) {
        super::battery(reading, seen).unwrap()
    }

    fn memory(reading: Option<MemoryReading>) -> (Outcome, String) {
        super::memory(reading).unwrap()
    }

    fn about(when: Option<&str>) -> About<'_> {
        About {
            name: "X",
            id: "x",
            dpi: 220,
            format: "Mono1",
            version: "1.2.3",
            when,
        }
    }

    #[test]
    fn a_report_reads_back_as_written() {
        let mut results = Results::default();
        results.set(
            Step::Screen,
            Mark::Done(Outcome::Pass, String::from("2 gray levels")),
        );
        results.set(Step::Buttons, Mark::Done(Outcome::Fail, String::new()));
        results.set(Step::Sleep, Mark::Asleep);
        let report = results
            .report(&about(Some("2026-10-23T10:00:00.000Z")))
            .unwrap();
        assert!(report.starts_with("PaperPaw device check\nDevice: X (x), 220 dpi, Mono1\nPaperPaw: 1.2.3\nDate: 2026-10-23T10:00:00.000Z\n"));
        assert!(report.contains("\nscreen: pass (2 gray levels)\nbuttons: fail\nsleep: asleep\n"));
        assert!(report.ends_with("Result: 1 passed, 1 failed, 0 skipped\n"));
        assert_eq!(Results::parse(&report).unwrap(), results);
    }

    #[test]
    fn what_a_report_does_not_know_is_passed_over() {
        let text = "screen: maybe\nunknown: pass\nbuttons pass\nmemory: skip (a (nested) note)\n";
        let results = Results::parse(text).unwrap();
        assert_eq!(results.get(Step::Screen), None);
        assert_eq!(results.get(Step::Buttons), None);
        assert_eq!(
            results.get(Step::Memory),
            Some(&Mark::Done(Outcome::Skip, String::from("a (nested) note")))
        );
        assert!(Results::parse("").unwrap().is_empty());
    }

    #[test]
    fn the_check_goes_on_at_the_first_step_not_answered() {
        let mut results = Results::default();
        assert_eq!(results.next_open(), Some(Step::Screen));
        results.set(Step::Screen, Mark::Done(Outcome::Pass, String::new()));
        results.set(Step::Touch, Mark::Done(Outcome::Skip, String::new()));
        assert_eq!(results.next_open(), Some(Step::Buttons));
        for step in Step::ALL {
            results.set(step, Mark::Done(Outcome::Pass, String::new()));
        }
        results.set(Step::Sleep, Mark::Asleep);
        assert_eq!(results.next_open(), Some(Step::Sleep));
        results.set(Step::Sleep, Mark::Done(Outcome::Pass, String::new()));
        assert_eq!(results.next_open(), None);
        assert_eq!(results.counts(), (10, 0, 0));
    }

    #[test]
    fn steps_for_parts_a_device_lacks_are_skipped() {
        let device = Facts {
            buttons: true,
            ..Facts::default()
        };
        let missing: Vec<Step> = Step::ALL
            .into_iter()
            .filter(|s| s.missing(&device).is_some())
            .collect();
        assert_eq!(missing, [Step::Touch, Step::Light, Step::Wifi, Step::Sleep]);
        let all = Facts {
            buttons: true,
            touch: true,
            frontlight: true,
            wifi: true,
            sleep: true,
        };
        let no_keys = Facts {
            buttons: false,
            ..all
        };
        assert_eq!(Step::Buttons.missing(&no_keys), Some("no buttons"));
        assert_eq!(Step::Light.missing(&all), None);
    }

    #[test]
    fn buttons_are_asked_in_turn_and_a_held_one_gives_up_the_asked() {
        let names = |list: &[&str]| list.iter().map(|n| String::from(*n)).collect::<Vec<_>>();
        let mut b = Buttons::new(names(&["back", "confirm", "power"]));
        assert_eq!(b.asked(), Some("back"));
        assert!(
            !b.press("confirm", false).unwrap(),
            "a slip changes nothing"
        );
        assert!(b.press("back", false).unwrap());
        assert!(b.press("back", true).unwrap(), "held instead of confirm");
        assert_eq!(b.asked(), Some("power"));
        assert!(b.press("power", true).unwrap(), "the one asked, held");
        assert_eq!(b.asked(), None);
        assert!(!b.press("power", false).unwrap());
        assert_eq!(
            b.verdict().unwrap(),
            (Outcome::Fail, String::from("not working: confirm"))
        );
        let mut good = Buttons::new(names(&["up", "down"]));
        good.press("up", false).unwrap();
        good.press("down", false).unwrap();
        assert_eq!(
            good.verdict().unwrap(),
            (Outcome::Pass, String::from("up, down"))
        );
    }

    #[test]
    fn a_battery_passes_with_values_a_battery_can_have() {
        let b = |percent, millivolts| BatteryReading {
            percent,
            millivolts,
        };
        assert_eq!(
            battery(Some(b(85, Some(4012))), [true, true]),
            (
                Outcome::Pass,
                String::from("85 %, 4012 mV, charging on and off")
            )
        );
        assert_eq!(
            battery(Some(b(85, Some(1200))), [false, true]).0,
            Outcome::Fail
        );
        assert_eq!(battery(Some(b(120, None)), [false, false]).0, Outcome::Fail);
        assert_eq!(
            battery(Some(b(50, None)), [false, false]).1,
            "50 %, charging not known"
        );
        assert_eq!(battery(None, [false, false]).0, Outcome::Skip);
    }

    #[test]
    fn the_card_notes_its_speed_or_its_error() {
        let mut test = CardTest {
            bytes: 128 * 1024,
            write_us: Some(500_000),
            read_us: Some(125_000),
            error: None,
        };
        assert_eq!(
            test.verdict().unwrap(),
            (
                Outcome::Pass,
                String::from("128 KiB, write 256 KiB/s, read 1024 KiB/s")
            )
        );
        test.write_us = None;
        assert_eq!(test.verdict().unwrap().1, "128 KiB");
        test.error = Some(String::from("read back differs"));
        assert_eq!(test.verdict().unwrap().0, Outcome::Fail);
        assert_eq!(speed(1024, 0), 1_000_000, "no division by zero");
    }

    #[test]
    fn memory_fails_with_a_stack_near_running_over() {
        let m = |stack_free: Option<u32>| MemoryReading {
            heap_free: 100 * 1024,
            heap_size: 256 * 1024,
            stack_free,
            stack_size: stack_free.map(|_| 16 * 1024),
        };
        assert_eq!(
            memory(Some(m(Some(4096)))),
            (
                Outcome::Pass,
                String::from("heap 100 KiB free of 256 KiB, stack 4096 B never used of 16384 B")
            )
        );
        assert_eq!(memory(Some(m(Some(512)))).0, Outcome::Fail);
        assert_eq!(memory(Some(m(None))).1, "heap 100 KiB free of 256 KiB");
        assert_eq!(memory(None).0, Outcome::Skip);
    }
}
