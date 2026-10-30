//! A rule for the battery reaching a level, with the real kernel: made on
//! the device as a reader makes it, then the battery rising and falling
//! with charging on and off. The rule applies once each time the level is
//! reached, not again while it wavers around it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::{Battery, Rotation, SystemStatus};
use paperpaw_plugin_rules::{OPEN, Rules};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    ActionSpec, Commands, DeviceInfo, DisplayInfo, Env, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, RegisterError, Registrar, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

const RULES: &str = ".paperpaw/plugins/rules/rules.txt";
/// The action the rules run, counted.
const COUNT: Id = Id::new_static("counter.count");

/// Counts how often [`COUNT`] runs.
struct Counter(Rc<Cell<u32>>);

impl Plugin for Counter {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("counter"), "Counter", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let count = self.0.clone();
        r.add_action(ActionSpec::new(COUNT, "counter.count").handler(
            move |_: &Env<'_>, _, _: &mut Commands| {
                count.set(count.get() + 1);
            },
        ))
    }
}

struct Test {
    kernel: Kernel,
    count: Rc<Cell<u32>>,
    now: u64,
}

impl Test {
    fn new(rules: &str) -> Self {
        let count = Rc::new(Cell::new(0));
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new().with(RULES, rules))
            .plugin(Rules)
            .plugin(Counter(count.clone()))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            count,
            now: 0,
        }
    }

    fn at(&mut self) -> Instant {
        self.now += 60_000;
        Instant(self.now)
    }

    /// The shell reads the battery at `percent`, charging or not.
    fn battery(&mut self, percent: u8, charging: bool) {
        let at = self.at();
        self.kernel.set_status(
            at,
            SystemStatus::new(None, Some(Battery::new(percent, charging, None))),
        );
    }

    /// The reader answers what the rules asked.
    fn answer(&mut self, event: &str, params: Params) {
        let at = self.at();
        self.kernel
            .send_event(at, Id::parse(event).unwrap(), params);
    }

    fn pick(&mut self, event: &str, path: &str, option: &str) {
        let picked = Picked::new(path.to_string(), option.to_string());
        self.answer(event, picked.to_params());
    }

    fn rules(&self) -> String {
        let data = paperpaw_sdk::read_file(self.kernel.storage(), RULES, 4096).unwrap();
        String::from_utf8(data).unwrap()
    }
}

#[test]
fn made_on_the_device_and_reached_once_while_charging() {
    let mut test = Test::new("");
    let at = test.at();
    test.kernel.run_action(at, OPEN);

    // When the battery reaches 80 %: run the counter, only while charging.
    test.pick("rules.trigger-chosen", "", "battery-at");
    test.pick("rules.level-chosen", "", "80");
    test.pick("rules.effect-chosen", "", "action");
    test.answer(
        "rules.picked",
        Params::new().with("id", COUNT.as_str().to_string()),
    );
    test.pick("rules.option-chosen", "0", "condition");
    test.pick("rules.condition-chosen", "", "charging");
    assert_eq!(
        test.rules(),
        "battery-at 80\taction\tcounter.count\ton\tcharging\n"
    );

    // Charging from 70 %.
    for percent in [70, 75, 79] {
        test.battery(percent, true);
    }
    assert_eq!(test.count.get(), 0);
    test.battery(80, true);
    assert_eq!(test.count.get(), 1, "reached");
    for percent in [81, 85, 79, 80, 78, 81] {
        test.battery(percent, true);
    }
    assert_eq!(test.count.get(), 1, "once, wavering or not");

    // Unplugged, used, the level reached again on battery: not while
    // charging, so nothing.
    for percent in [76, 60, 80, 85] {
        test.battery(percent, false);
    }
    assert_eq!(test.count.get(), 1, "only while charging");

    // Used down, charged up again: once more.
    test.battery(40, false);
    for percent in [42, 70, 82, 90] {
        test.battery(percent, true);
    }
    assert_eq!(test.count.get(), 2, "reached again");
}

#[test]
fn a_level_already_reached_does_not_count() {
    let mut test = Test::new("battery-at 80\taction\tcounter.count\ton\n");
    // The first reading is above the level: it was reached before.
    for percent in [85, 86, 84] {
        test.battery(percent, true);
    }
    assert_eq!(test.count.get(), 0);
    test.battery(70, false);
    test.battery(100, true);
    assert_eq!(test.count.get(), 1, "rising past it in one step");

    // Paused: nothing.
    let mut test = Test::new("battery-at 80\taction\tcounter.count\toff\n");
    test.battery(70, true);
    test.battery(80, true);
    assert_eq!(test.count.get(), 0);
}

#[test]
fn another_level_on_the_keyboard() {
    let mut test = Test::new("");
    let at = test.at();
    test.kernel.run_action(at, OPEN);
    test.pick("rules.trigger-chosen", "", "battery-at");
    test.pick("rules.level-chosen", "", "other");
    let typed = |text: &str| Typed::new(text.to_string(), Params::new()).to_params();
    // Not a level: no rule.
    test.answer("rules.level-entered", typed("120"));
    test.pick("rules.effect-chosen", "", "message");
    test.answer("rules.message-entered", typed("Unplug now"));
    assert_eq!(test.rules(), "");
    test.answer("rules.level-entered", typed("85 %"));
    test.pick("rules.effect-chosen", "", "message");
    test.answer("rules.message-entered", typed("Unplug now"));
    assert_eq!(test.rules(), "battery-at 85\tmessage\tUnplug now\ton\n");
}
