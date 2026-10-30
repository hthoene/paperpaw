//! Rules with the real kernel: those that repeat without a clock, times
//! of day once the clock becomes known, rules that undo each other, and
//! a rules file that cannot be read.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::{DirEntry, Rotation, SystemStatus, WallClock};
use paperpaw_plugin_rules::{OPEN, Rules};
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    ActionSpec, Commands, DeviceInfo, DisplayInfo, Env, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, RegisterError, Registrar, SettingSpec,
    Storage, StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

const RULES: &str = ".paperpaw/plugins/rules/rules.txt";
const COUNT: Id = Id::new_static("counter.count");

/// Two switches, and an action that counts how often it runs.
struct Counter(Rc<Cell<u32>>);

static SWITCHES: [SettingSpec; 2] = [
    SettingSpec::switch("counter.a", "counter.a", false, groups::SYSTEM),
    SettingSpec::switch("counter.b", "counter.b", false, groups::SYSTEM),
];

impl Plugin for Counter {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("counter"), "Counter", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let count = self.0.clone();
        r.add_settings(&SWITCHES)?;
        r.add_action(ActionSpec::new(COUNT, "counter.count").handler(
            move |_: &Env<'_>, _, _: &mut Commands| {
                count.set(count.get() + 1);
            },
        ))
    }
}

/// A card whose rules file can stop being readable.
struct Flaky {
    inner: MemoryStorage,
    broken: Rc<Cell<bool>>,
}

impl Flaky {
    fn check(&self, path: &str) -> Result<(), StorageError> {
        if self.broken.get() && path == RULES {
            Err(StorageError::Io)
        } else {
            Ok(())
        }
    }
}

impl Storage for Flaky {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.check(path)?;
        self.inner.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.check(path)?;
        self.inner.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
}

struct Test {
    kernel: Kernel,
    count: Rc<Cell<u32>>,
    broken: Rc<Cell<bool>>,
}

impl Test {
    fn new(rules: &str) -> Self {
        Self::reading(rules, true)
    }

    /// Started with the rules file readable or not.
    fn reading(rules: &str, readable: bool) -> Self {
        let count = Rc::new(Cell::new(0));
        let broken = Rc::new(Cell::new(!readable));
        let storage = Flaky {
            inner: MemoryStorage::new().with(RULES, rules),
            broken: broken.clone(),
        };
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .plugin(Rules)
            .plugin(Counter(count.clone()))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            count,
            broken,
        }
    }

    /// The time passes to `at` (milliseconds since the start).
    fn tick(&mut self, at: u64) {
        self.kernel.tick(Instant(at));
    }

    /// The clock becomes known: `hour:minute` UTC, `at` since the start.
    fn clock(&mut self, at: u64, hour: i64, minute: i64) {
        let day = 20_000 * 86_400_000;
        let status = SystemStatus::new(
            Some(WallClock::new(
                Instant(at),
                day + (hour * 60 + minute) * 60_000,
                Some(0),
            )),
            None,
        );
        self.kernel.set_status(Instant(at), status);
    }

    fn rules(&mut self) -> String {
        // What the kernel keeps of what it read goes once a frame is drawn.
        self.kernel.render(&mut |_, _| {});
        let data = paperpaw_sdk::read_file(self.kernel.storage(), RULES, 4096).unwrap();
        String::from_utf8(data).unwrap()
    }
}

#[test]
fn rules_that_repeat_need_no_clock() {
    let mut test = Test::new("every 1\taction\tcounter.count\ton\n");
    test.tick(60_000);
    assert_eq!(test.count.get(), 1, "a minute after the start");
    test.tick(120_000);
    assert_eq!(test.count.get(), 2, "and the next");
}

#[test]
fn a_time_of_day_waits_for_the_clock() {
    let mut test = Test::new("time 22:00\taction\tcounter.count\ton\n");
    test.tick(30_000);
    // The clock becomes known at 21:59 (UTC, as the device has no zone).
    test.clock(40_000, 21, 59);
    test.tick(100_000);
    assert_eq!(test.count.get(), 1, "at 22:00");
}

#[test]
fn rules_that_undo_each_other_stop() {
    let mut test = Test::new(
        "switch-on counter.a\tswitch-on\tcounter.b\ton\n\
         switch-on counter.b\tswitch-off\tcounter.a\ton\n\
         switch-off counter.a\tswitch-off\tcounter.b\ton\n\
         switch-off counter.b\tswitch-on\tcounter.a\ton\n",
    );
    test.kernel.take_warnings();
    test.kernel
        .set_setting(Instant(1000), "counter.a", "on")
        .unwrap();
    let warnings = test.kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_rules_file_that_cannot_be_read_is_not_saved_over() {
    let kept = "every 1\taction\tcounter.count\ton\n";
    // Unreadable from the start: none of the rules is known.
    let mut test = Test::reading(kept, false);
    // A rule added in the list: when PaperPaw starts, a message.
    test.kernel.run_action(Instant(1000), OPEN);
    let picked = |option: &str| Picked::new(String::new(), option.to_string()).to_params();
    test.kernel.send_event(
        Instant(1100),
        Id::new_static("rules.trigger-chosen"),
        picked("start"),
    );
    test.kernel.send_event(
        Instant(1200),
        Id::new_static("rules.effect-chosen"),
        picked("message"),
    );
    let typed = paperpaw_sdk::replies::Typed::new("Hello".to_string(), Params::new());
    test.kernel.send_event(
        Instant(1300),
        Id::new_static("rules.message-entered"),
        typed.to_params(),
    );
    test.broken.set(false);
    assert_eq!(test.rules(), kept, "not saved over");
}
