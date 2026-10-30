//! The device check's steps and what came of them, apart from how they
//! are shown: what each step tests, the verdict its measurements suggest,
//! and the report written to the card after each answer. The report is
//! also where an unfinished check goes on from, e.g. after the device
//! slept with its power cut.
//!
//! The report is in English whatever the language set: it is pasted into
//! pull requests. Each step is a line `<step>: <outcome>`, with a note in
//! parentheses where there is one.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

use paperpaw_sdk::memory::MemoryUse;
use paperpaw_sdk::{Battery, Capabilities, DeviceInfo};

/// Where the report is written.
pub const REPORT: &str = paperpaw_sdk::data_path!("device-check.txt");

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
    pub fn missing(self, device: &DeviceInfo, caps: &Capabilities) -> Option<&'static str> {
        match self {
            Step::Buttons if device.input.keys.is_empty() => Some("no buttons"),
            Step::Touch if !device.input.touch => Some("no touch screen"),
            Step::Light if !caps.frontlight => Some("no front light"),
            Step::Wifi if !caps.wifi => Some("Wi-Fi not managed by PaperPaw"),
            Step::Sleep if !caps.sleep => Some("sleep not managed by PaperPaw"),
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
    pub device: &'a DeviceInfo,
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
    pub fn report(&self, about: &About<'_>) -> String {
        let d = about.device;
        let mut out = String::from("PaperPaw device check\n");
        let display = &d.display;
        let _ = writeln!(
            out,
            "Device: {} ({}), {} dpi, {:?}",
            d.name, d.id, display.dpi, display.format
        );
        let _ = writeln!(out, "PaperPaw: {}", about.version);
        if let Some(when) = about.when {
            let _ = writeln!(out, "Date: {when}");
        }
        out.push('\n');
        for step in Step::ALL {
            match self.get(step) {
                Some(Mark::Done(outcome, note)) if note.is_empty() => {
                    let _ = writeln!(out, "{}: {}", step.key(), outcome.key());
                }
                Some(Mark::Done(outcome, note)) => {
                    let _ = writeln!(out, "{}: {} ({note})", step.key(), outcome.key());
                }
                Some(Mark::Asleep) => {
                    let _ = writeln!(out, "{}: {ASLEEP}", step.key());
                }
                None => {}
            }
        }
        let (pass, fail, skip) = self.counts();
        let _ = writeln!(
            out,
            "\nResult: {pass} passed, {fail} failed, {skip} skipped"
        );
        out
    }

    /// The marks of a report written before; lines it does not know are
    /// passed over.
    pub fn parse(report: &str) -> Results {
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
                results.set(step, Mark::Done(outcome, String::from(note)));
            }
        }
        results
    }
}

/// The buttons asked for in turn, and what came of each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Buttons {
    /// The buttons to press, the device's.
    pub names: Vec<&'static str>,
    /// Those pressed when asked.
    pub pressed: Vec<&'static str>,
    /// Those given up on: another one was held instead.
    pub failed: Vec<&'static str>,
}

impl Buttons {
    pub fn new(names: &[&'static str]) -> Self {
        Self {
            names: names.to_vec(),
            ..Self::default()
        }
    }

    /// The button asked for now; `None` once all were.
    pub fn asked(&self) -> Option<&'static str> {
        self.names
            .iter()
            .copied()
            .find(|n| !self.pressed.contains(n) && !self.failed.contains(n))
    }

    /// Takes button `name` pressed (`long`: held). The one asked for
    /// counts as pressed; another held gives the one asked up as broken;
    /// another pressed briefly is a slip and changes nothing. Whether it
    /// changed anything.
    pub fn press(&mut self, name: &str, long: bool) -> bool {
        let Some(asked) = self.asked() else {
            return false;
        };
        if name == asked {
            self.pressed.push(asked);
        } else if long {
            self.failed.push(asked);
        } else {
            return false;
        }
        true
    }

    /// The outcome and note once all were asked.
    pub fn verdict(&self) -> (Outcome, String) {
        if self.failed.is_empty() {
            (Outcome::Pass, self.pressed.join(", "))
        } else {
            (
                Outcome::Fail,
                format!("not working: {}", self.failed.join(", ")),
            )
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
pub fn battery(battery: Option<Battery>, charging_seen: [bool; 2]) -> (Outcome, String) {
    let Some(b) = battery else {
        return (Outcome::Skip, String::from("no battery reported"));
    };
    let mut note = format!("{} %", b.percent);
    let volts_fit = match b.millivolts {
        Some(mv) => {
            let _ = write!(note, ", {mv} mV");
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
    let _ = write!(note, ", {seen}");
    let outcome = if b.percent <= 100 && volts_fit {
        Outcome::Pass
    } else {
        Outcome::Fail
    };
    (outcome, note)
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
    pub fn verdict(&self) -> (Outcome, String) {
        if let Some(error) = &self.error {
            return (Outcome::Fail, error.clone());
        }
        let mut note = format!("{} KiB", self.bytes / 1024);
        if let (Some(w), Some(r)) = (self.write_us, self.read_us) {
            let _ = write!(
                note,
                ", write {} KiB/s, read {} KiB/s",
                speed(self.bytes, w),
                speed(self.bytes, r)
            );
        }
        (Outcome::Pass, note)
    }
}

/// The memory's verdict: a stack with less than this left at its deepest
/// is near running over.
pub const STACK_SPARE: u32 = 1024;

pub fn memory(memory: Option<MemoryUse>) -> (Outcome, String) {
    let Some(m) = memory else {
        return (Outcome::Skip, String::from("not reported"));
    };
    let kib = |bytes: u32| bytes / 1024;
    let mut note = format!(
        "heap {} KiB free of {} KiB",
        kib(m.heap_free),
        kib(m.heap_size)
    );
    let mut outcome = Outcome::Pass;
    if let (Some(free), Some(size)) = (m.stack_free, m.stack_size) {
        let _ = write!(note, ", stack {free} B never used of {size} B");
        if free < STACK_SPARE {
            outcome = Outcome::Fail;
        }
    }
    (outcome, note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_platform::Rotation;
    use paperpaw_sdk::{DisplayInfo, InputCaps, MemoryClass, PixelFormat};

    const DEVICE: DeviceInfo = DeviceInfo::new(
        "x",
        "X",
        DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
        InputCaps::new(true, false, false, &["confirm"]),
        MemoryClass::Small,
    );

    fn about(when: Option<&str>) -> About<'_> {
        About {
            device: &DEVICE,
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
        let report = results.report(&about(Some("2026-10-23T10:00:00.000Z")));
        assert!(report.starts_with("PaperPaw device check\nDevice: X (x), 220 dpi, Mono1\nPaperPaw: 1.2.3\nDate: 2026-10-23T10:00:00.000Z\n"));
        assert!(report.contains("\nscreen: pass (2 gray levels)\nbuttons: fail\nsleep: asleep\n"));
        assert!(report.ends_with("Result: 1 passed, 1 failed, 0 skipped\n"));
        assert_eq!(Results::parse(&report), results);
    }

    #[test]
    fn what_a_report_does_not_know_is_passed_over() {
        let text = "screen: maybe\nunknown: pass\nbuttons pass\nmemory: skip (a (nested) note)\n";
        let results = Results::parse(text);
        assert_eq!(results.get(Step::Screen), None);
        assert_eq!(results.get(Step::Buttons), None);
        assert_eq!(
            results.get(Step::Memory),
            Some(&Mark::Done(Outcome::Skip, String::from("a (nested) note")))
        );
        assert!(Results::parse("").is_empty());
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
        let caps = Capabilities::default();
        let missing: Vec<Step> = Step::ALL
            .into_iter()
            .filter(|s| s.missing(&DEVICE, &caps).is_some())
            .collect();
        assert_eq!(missing, [Step::Touch, Step::Light, Step::Wifi, Step::Sleep]);
        let mut all = caps;
        all.frontlight = true;
        all.wifi = true;
        all.sleep = true;
        let mut no_keys = DEVICE;
        no_keys.input.keys = &[];
        assert_eq!(Step::Buttons.missing(&no_keys, &all), Some("no buttons"));
        assert_eq!(Step::Light.missing(&DEVICE, &all), None);
    }

    #[test]
    fn buttons_are_asked_in_turn_and_a_held_one_gives_up_the_asked() {
        let mut b = Buttons::new(&["back", "confirm", "power"]);
        assert_eq!(b.asked(), Some("back"));
        assert!(!b.press("confirm", false), "a slip changes nothing");
        assert!(b.press("back", false));
        assert!(b.press("back", true), "held instead of confirm");
        assert_eq!(b.asked(), Some("power"));
        assert!(b.press("power", true), "the one asked, held");
        assert_eq!(b.asked(), None);
        assert!(!b.press("power", false));
        assert_eq!(
            b.verdict(),
            (Outcome::Fail, String::from("not working: confirm"))
        );
        let mut good = Buttons::new(&["up", "down"]);
        good.press("up", false);
        good.press("down", false);
        assert_eq!(good.verdict(), (Outcome::Pass, String::from("up, down")));
    }

    #[test]
    fn a_battery_passes_with_values_a_battery_can_have() {
        let b = |percent, millivolts| Battery::new(percent, false, millivolts);
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
            test.verdict(),
            (
                Outcome::Pass,
                String::from("128 KiB, write 256 KiB/s, read 1024 KiB/s")
            )
        );
        test.write_us = None;
        assert_eq!(test.verdict().1, "128 KiB");
        test.error = Some(String::from("read back differs"));
        assert_eq!(test.verdict().0, Outcome::Fail);
        assert_eq!(speed(1024, 0), 1_000_000, "no division by zero");
    }

    #[test]
    fn memory_fails_with_a_stack_near_running_over() {
        let m = |stack_free: Option<u32>| {
            let heap = MemoryUse::new(100 * 1024, 256 * 1024);
            match stack_free {
                Some(free) => heap.with_stack(free, 16 * 1024),
                None => heap,
            }
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
