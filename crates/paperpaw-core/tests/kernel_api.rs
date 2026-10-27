//! What shells tell the kernel and ask of it besides input: the battery
//! and the wall clock (also readings that make no sense), sleeping and
//! waking, passwords and keys kept on the storage, slots opened and
//! events sent from outside.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{BASE, DEVICE, Harness, LATER};
use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::{events, slots};
use paperpaw_core::sdk::secrets::{FILE as SECRETS, SecretChange};
use paperpaw_core::sdk::{
    Battery, DirEntry, Manifest, MemoryStorage, Params, Plugin, RegisterError, Registrar, Storage,
    StorageError, SystemStatus, WallClock,
};
use paperpaw_platform::Instant;
use proptest::prelude::*;
use proptest::test_runner::Config;

/// Logs the kernel's events about the device.
struct Listener {
    harness: Harness,
}

impl Plugin for Listener {
    fn manifest(&self) -> Manifest {
        common::manifest("listener")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for event in [
            events::STARTED.id().clone(),
            events::STATUS_CHANGED.id().clone(),
            events::BATTERY_MEASURED.id().clone(),
            events::SUSPEND.id().clone(),
            events::RESUME.id().clone(),
            events::STOPPING.id().clone(),
        ] {
            let log = self.harness.log.clone();
            r.subscribe(
                event.clone(),
                Box::new(move |_, _, _| log.borrow_mut().push(format!("heard {event}"))),
            )?;
        }
        Ok(())
    }
}

/// A storage that counts how often it is told to write back what it kept
/// back.
#[derive(Clone, Default)]
struct Synced {
    files: Rc<MemoryStorage>,
    syncs: Rc<Cell<u32>>,
}

impl Storage for Synced {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.syncs.set(self.syncs.get() + 1);
        Ok(())
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.files.create_dir(path)
    }
}

fn kernel(harness: &Harness, storage: impl Storage + 'static) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .plugin(Listener {
            harness: harness.clone(),
        })
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    kernel
}

fn heard(harness: &Harness) -> Vec<String> {
    let heard = harness.lines("heard");
    harness.take();
    heard
}

fn battery(percent: u8, millivolts: u16) -> Option<Battery> {
    Some(Battery {
        percent,
        charging: false,
        millivolts: Some(millivolts),
    })
}

fn clock(at: u64, unix_millis: i64, offset: Option<i32>) -> Option<WallClock> {
    Some(WallClock {
        at: Instant(at),
        unix_millis,
        utc_offset_minutes: offset,
    })
}

#[test]
fn status_lines_are_told_only_of_what_they_show() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    let status = |battery, clock| SystemStatus { clock, battery };
    let day = 1_760_000_000_000;
    kernel.set_status(
        Instant(10),
        status(battery(80, 4000), clock(10, day, Some(60))),
    );
    assert_eq!(
        heard(&harness),
        ["heard core.status-changed", "heard core.battery-measured"]
    );
    // The voltage moved, the clock was corrected by a few seconds.
    kernel.set_status(
        Instant(20),
        status(battery(80, 3990), clock(20, day + 5_010, Some(60))),
    );
    assert_eq!(heard(&harness), ["heard core.battery-measured"]);
    // The percent, the time zone, a jump of the clock, the clock gone.
    for (at, next) in [
        (
            30,
            status(battery(79, 3990), clock(20, day + 5_010, Some(60))),
        ),
        (
            40,
            status(battery(79, 3990), clock(20, day + 5_010, Some(120))),
        ),
        (
            50,
            status(battery(79, 3990), clock(20, day + 3_600_000, Some(120))),
        ),
        (60, status(battery(79, 3990), None)),
    ] {
        kernel.set_status(Instant(at), next);
        assert_eq!(
            heard(&harness),
            ["heard core.status-changed", "heard core.battery-measured"],
            "at {at}"
        );
    }
    // Nothing measured, nothing changed: nothing heard.
    kernel.set_status(Instant(70), status(None, None));
    kernel.set_status(Instant(80), status(None, None));
    assert_eq!(heard(&harness), ["heard core.status-changed"]);
}

#[test]
fn wall_clock_readings_that_make_no_sense_do_not_panic() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    for (now, reading) in [
        (10, clock(0, i64::MAX, None)),
        (20, clock(0, i64::MIN, None)),
        (u64::MAX, clock(0, 0, Some(i32::MAX))),
        (30, clock(u64::MAX, i64::MAX, Some(i32::MIN))),
    ] {
        let status = SystemStatus {
            clock: reading,
            battery: None,
        };
        kernel.set_status(Instant(now), status);
    }
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

#[test]
fn battery_readings_out_of_range_do_not_panic() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    for (at, percent, millivolts) in [(10, 255, u16::MAX), (20, 0, 0), (30, 101, 1)] {
        kernel.set_status(
            Instant(at),
            SystemStatus {
                clock: None,
                battery: battery(percent, millivolts),
            },
        );
        common::render(&mut kernel);
    }
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

#[test]
fn sleeping_and_waking_are_heard_and_counted() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    for i in 0..3 {
        kernel.suspend(Instant(1000 + i * 100));
        kernel.resume(Instant(1050 + i * 100));
    }
    // Woken twice without sleeping, slept twice without waking.
    kernel.resume(Instant(2000));
    kernel.suspend(Instant(2100));
    kernel.suspend(Instant(2200));
    let heard = heard(&harness);
    assert_eq!(heard.iter().filter(|l| l.ends_with("suspend")).count(), 5);
    assert_eq!(heard.iter().filter(|l| l.ends_with("resume")).count(), 4);
    assert_eq!(kernel.usage().sleeps, 5);
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

/// A device whose sleep restarts it (the X4s) wakes by a start: the shell
/// says so, and plugins hear the waking after the start, once.
#[test]
fn a_start_that_ends_a_sleep_is_a_waking_too() {
    for (after_sleep, expected) in [
        (false, vec!["heard core.started"]),
        (true, vec!["heard core.started", "heard core.resume"]),
    ] {
        let harness = Harness::new();
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(harness.plugin())
            .plugin(Listener {
                harness: harness.clone(),
            })
            .after_sleep(after_sleep)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        common::render(&mut kernel);
        assert_eq!(heard(&harness), expected, "after a sleep: {after_sleep}");
        // It is no sleep counted, and it is not told again.
        assert_eq!(kernel.usage().sleeps, 0);
        kernel.suspend(Instant(1000));
        kernel.resume(Instant(1100));
        assert_eq!(
            heard(&harness),
            ["heard core.suspend", "heard core.resume"],
            "after a sleep: {after_sleep}"
        );
    }
}

#[test]
fn stopping_is_heard_and_written_back_but_no_sleep() {
    let harness = Harness::new();
    let storage = Synced::default();
    let mut kernel = kernel(&harness, storage.clone());
    let synced = storage.syncs.get();
    kernel.stop(Instant(1000));
    assert_eq!(heard(&harness), ["heard core.stopping"]);
    assert_eq!(kernel.usage().sleeps, 0);
    assert_eq!(storage.syncs.get(), synced + 1);
}

#[test]
fn a_slot_opened_by_the_shell_for_something_else_replaces_its_screen() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    let book = |path: &str| Params::new().with("path", String::from(path));
    kernel.open_slot(Instant(10), slots::HOME, Params::new());
    assert!(harness.take().is_empty(), "the same: nothing to do");
    kernel.open_slot(Instant(20), slots::HOME, book("a.epub"));
    assert_eq!(harness.take(), ["base:close", "base:show"]);
    kernel.open_slot(Instant(30), slots::HOME, book("a.epub"));
    assert!(harness.take().is_empty(), "the same book again");
    assert_eq!(kernel.screens().count(), 1);
    // A slot nobody fills: a warning, nothing else.
    kernel.open_slot(Instant(40), slots::READER, book("b.epub"));
    assert_eq!(
        kernel.active_screen().map(|s| s.as_str()),
        Some("core.placeholder")
    );
    let warnings = kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn events_sent_by_the_shell_reach_plugins_once() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    kernel.send_event(Instant(10), LATER, Params::new().with("k", "v"));
    assert_eq!(harness.lines("event"), ["event drv.later at 10 k=v"]);
}

#[test]
fn secrets_kept_are_read_at_the_next_start_and_removed_files_go() {
    let harness = Harness::new();
    let mut first = kernel(&harness, MemoryStorage::new());
    first.set_secret(SecretChange::keep("drv.key", "Key", "s3cret\tvalue\n"));
    first.set_secret(SecretChange::keep("drv.other", "Other", ""));
    let saved = paperpaw_core::sdk::read_file(first.storage(), SECRETS, 4096).unwrap();
    assert!(
        !String::from_utf8_lossy(&saved).contains("s3cret"),
        "kept out of sight"
    );
    let second = kernel(&harness, MemoryStorage::new().with(SECRETS, saved));
    assert_eq!(second.secrets().get("drv.key"), Some("s3cret\tvalue\n"));
    assert_eq!(second.secrets().get("drv.other"), Some(""));
    // All removed: the file goes.
    first.set_secret(SecretChange::remove("drv.key"));
    first.set_secret(SecretChange::remove("drv.other"));
    first.set_secret(SecretChange::remove("drv.never-kept"));
    assert!(!first.storage().exists(SECRETS));
}

#[test]
fn a_damaged_secrets_file_keeps_what_can_be_read() {
    let harness = Harness::new();
    let mut good = kernel(&harness, MemoryStorage::new());
    good.set_secret(SecretChange::keep("drv.key", "Key", "value"));
    let line = paperpaw_core::sdk::read_file(good.storage(), SECRETS, 4096).unwrap();
    let mut damaged = b"no tabs at all\n\t\t\t\nid\tlabel\tc\tnot hex\nid\tlabel\tc\tff\n".to_vec();
    damaged.extend_from_slice(&[0xff, 0xfe, b'\n']);
    damaged.extend_from_slice(&line);
    let kernel = kernel(&harness, MemoryStorage::new().with(SECRETS, damaged));
    assert_eq!(kernel.secrets().get("drv.key"), Some("value"));
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

proptest! {
    #![proptest_config(Config { failure_persistence: None, ..Config::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)) })]

    /// Any value kept as a secret is read back the same.
    #[test]
    fn any_secret_is_read_back_the_same(value in any::<String>(), label in "[^\t\r\n]{0,20}") {
        let harness = Harness::new();
        let mut first = kernel(&harness, MemoryStorage::new());
        first.set_secret(SecretChange::keep("drv.key", &label, &value));
        let saved = paperpaw_core::sdk::read_file(first.storage(), SECRETS, 1 << 20).unwrap();
        let second = kernel(&harness, MemoryStorage::new().with(SECRETS, saved));
        prop_assert_eq!(second.secrets().get("drv.key"), Some(value.as_str()));
    }
}

/// Sets looks for the parts of screens; its home screen tells which look
/// its style has.
struct Looks {
    name: &'static str,
    looks: Vec<(
        &'static paperpaw_core::sdk::ui::look::Painters,
        paperpaw_core::sdk::Priority,
    )>,
    seen: Rc<std::cell::RefCell<String>>,
}

struct LookScreen(Rc<std::cell::RefCell<String>>);

impl paperpaw_core::sdk::Screen for LookScreen {
    fn draw(&mut self, _: &mut paperpaw_core::sdk::Canvas<'_>, env: &paperpaw_core::sdk::Env<'_>) {
        let style = paperpaw_core::sdk::ui::UiStyle::new(env);
        *self.0.borrow_mut() = format!("{:?}", style.look());
    }

    fn on_action(
        &mut self,
        _: &paperpaw_core::sdk::Id,
        _: &mut paperpaw_core::sdk::Cx<'_, '_>,
    ) -> paperpaw_core::sdk::Handled {
        paperpaw_core::sdk::Handled::No
    }
}

impl Plugin for Looks {
    fn manifest(&self) -> Manifest {
        common::manifest(self.name)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (painters, priority) in &self.looks {
            r.set_ui_look(painters, *priority)?;
        }
        if self.name == "looks-a" {
            let screen = paperpaw_core::sdk::Id::new_static("looks-a.home");
            let seen = self.seen.clone();
            r.add_screen(paperpaw_core::sdk::ScreenSpec::new(
                screen.clone(),
                "looks-a.home",
                Box::new(move |_, _| Box::new(LookScreen(seen.clone()))),
            ))?;
            r.bind_slot(slots::HOME, screen, paperpaw_core::sdk::Priority(1000))?;
        }
        Ok(())
    }
}

#[test]
fn the_look_of_the_highest_priority_is_used_of_equal_ones_the_later() {
    use paperpaw_core::sdk::Priority;
    use paperpaw_core::sdk::ui::look::{FocusPart, PaintCx, Painters, SwitchPart};
    fn switch(_: &mut PaintCx<'_, '_>, _: &SwitchPart) -> bool {
        true
    }
    fn focus(_: &mut PaintCx<'_, '_>, _: &FocusPart) -> bool {
        true
    }
    static PLAIN: Painters = Painters::new();
    static SWITCHES: Painters = Painters::new().switch(switch);
    static FOCUS: Painters = Painters::new().focus(focus);
    let seen = Rc::new(std::cell::RefCell::new(String::new()));
    let looks = |name, looks| Looks {
        name,
        looks,
        seen: seen.clone(),
    };
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(looks("looks-a", vec![(&PLAIN, Priority::DEFAULT)]))
        .plugin(looks("looks-b", vec![(&SWITCHES, Priority::PREFERRED)]))
        .plugin(looks(
            "looks-c",
            vec![(&FOCUS, Priority::PREFERRED), (&PLAIN, Priority::FALLBACK)],
        ))
        .storage(MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    assert_eq!(*seen.borrow(), format!("{:?}", FOCUS));
}

/// Fills the choice slot with a screen of its own.
struct Asker {
    bind: bool,
}

impl Plugin for Asker {
    fn manifest(&self) -> Manifest {
        common::manifest("asker")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        if self.bind {
            let screen = paperpaw_core::sdk::Id::new_static("asker.choice");
            let seen = Rc::new(std::cell::RefCell::new(String::new()));
            r.add_screen(paperpaw_core::sdk::ScreenSpec::new(
                screen.clone(),
                "asker.choice",
                Box::new(move |_, _| Box::new(LookScreen(seen.clone()))),
            ))?;
            r.bind_slot(slots::CHOICE, screen, paperpaw_core::sdk::Priority::DEFAULT)?;
        }
        Ok(())
    }
}

/// Every plugin's questions go to slot `core.choice`: the kernel's
/// dialog, or the screen another plugin binds there.
#[test]
fn questions_are_asked_where_the_choice_slot_says() {
    use paperpaw_core::sdk::event::Event;
    use paperpaw_core::sdk::ui::choice::Picked;
    const CHOSEN: Event<Picked> = Event::new("asker.chosen");
    for (bind, expected) in [(false, "core.choice-dialog"), (true, "asker.choice")] {
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(Asker { bind })
            .storage(MemoryStorage::new())
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let paperpaw_core::sdk::Command::OpenSlot(slot, params) =
            paperpaw_core::sdk::ask::choice("Delete?", &CHOSEN)
                .options("asker.option.", &["delete", "cancel"])
                .command()
        else {
            panic!("a slot");
        };
        kernel.open_slot(Instant(100), slot, params);
        common::render(&mut kernel);
        assert_eq!(kernel.active_screen().map(|s| s.as_str()), Some(expected));
    }
}

/// The yes or no question every plugin asks (`ask::confirm`): asked on
/// the kernel's box, the focus on no; yes by the buttons, no by a tap on
/// it, nothing on *Back* or a tap outside the box.
#[test]
fn a_yes_or_no_question_is_answered_by_buttons_and_by_touch() {
    use paperpaw_core::sdk::event::Event;
    use paperpaw_core::sdk::replies::Confirmed;
    use paperpaw_core::sdk::semantics::Role;
    use paperpaw_platform::Rect;
    const ANSWERED: Event<Confirmed> = Event::new("drv.reply");
    let harness = Harness::new();
    let mut kernel = harness.kernel();
    let ask = |kernel: &mut Kernel, at: u64| {
        let paperpaw_core::sdk::Command::OpenSlot(slot, params) =
            paperpaw_core::sdk::ask::confirm("Delete?", &ANSWERED)
                .with("book", "a.epub")
                .command()
        else {
            panic!("a slot");
        };
        kernel.open_slot(Instant(at), slot, params);
        common::render(kernel);
        assert_eq!(
            kernel.active_screen().map(|s| s.as_str()),
            Some("core.confirm-dialog")
        );
    };
    let buttons = |kernel: &mut Kernel| -> Vec<(String, Rect, bool)> {
        kernel
            .describe()
            .into_iter()
            .filter(|p| p.role == Role::Button)
            .map(|p| (p.label, p.area, p.focused))
            .collect()
    };
    let closed = |kernel: &Kernel| {
        assert_eq!(kernel.active_screen().map(|s| s.as_str()), Some("drv.base"));
    };
    let answers = || {
        let lines = harness.lines("event drv.reply");
        harness.take();
        lines
            .iter()
            .map(|l| {
                assert!(l.contains("book=a.epub"), "{l}");
                l.contains("answer=yes")
            })
            .collect::<Vec<bool>>()
    };
    // Asked: yes and no, the focus on no.
    ask(&mut kernel, 100);
    let shown = buttons(&mut kernel);
    let labels: Vec<&str> = shown.iter().map(|(l, _, _)| l.as_str()).collect();
    assert_eq!(labels, ["Yes", "No"]);
    assert!(!shown[0].2 && shown[1].2, "{shown:?}");
    // Yes with the buttons: left to it, then confirm.
    common::press(&mut kernel, 200, "left");
    assert!(buttons(&mut kernel)[0].2);
    common::press(&mut kernel, 300, "confirm");
    closed(&kernel);
    assert_eq!(answers(), [true]);
    // No by a tap on it.
    ask(&mut kernel, 400);
    let no = buttons(&mut kernel)[1].1;
    let (x, y) = (
        i32::from(no.x) + i32::from(no.width) / 2,
        i32::from(no.y) + i32::from(no.height) / 2,
    );
    common::tap(&mut kernel, 500, x, y);
    closed(&kernel);
    assert_eq!(answers(), [false]);
    // Back: closed, no answer.
    ask(&mut kernel, 700);
    common::press(&mut kernel, 800, "back");
    closed(&kernel);
    assert_eq!(answers(), []);
    // A tap outside the box, at the top of the screen: closed, no answer.
    ask(&mut kernel, 900);
    common::tap(&mut kernel, 1000, 60, 1);
    closed(&kernel);
    assert_eq!(answers(), []);
}

/// A name typed for a setting, and a number whose check refuses large
/// values.
struct Checked;

fn at_most_five(
    _: &paperpaw_core::sdk::Settings,
    value: &paperpaw_core::sdk::SettingValue,
) -> Result<(), &'static str> {
    match value {
        paperpaw_core::sdk::SettingValue::Int(n) if *n > 5 => Err("checked.too-large"),
        _ => Ok(()),
    }
}

static AT_MOST_FIVE: paperpaw_core::sdk::SettingExtra =
    paperpaw_core::sdk::SettingExtra::new().check(at_most_five);
static CHECKED_SETTINGS: [paperpaw_core::sdk::SettingSpec; 2] = [
    paperpaw_core::sdk::SettingSpec::text(
        "checked.name",
        "checked.name",
        "",
        paperpaw_core::sdk::builtin::groups::OTHER,
    ),
    paperpaw_core::sdk::SettingSpec::number(
        "checked.count",
        "checked.count",
        1,
        9,
        3,
        paperpaw_core::sdk::builtin::groups::OTHER,
    )
    .with_extra(&AT_MOST_FIVE),
];

impl Plugin for Checked {
    fn manifest(&self) -> Manifest {
        common::manifest("checked")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&CHECKED_SETTINGS)
    }
}

/// Text typed for a setting is set by the kernel, whichever screen asked;
/// a value the setting's check refuses is not set, and the reader is told.
#[test]
fn typed_settings_are_set_and_refused_values_told() {
    use paperpaw_core::sdk::builtin::{events, params};
    use paperpaw_core::sdk::event::EventData;
    let harness = Harness::default();
    let mut kernel = kernel_with(&harness, Checked);
    let typed =
        paperpaw_core::sdk::Typed::new("  Ann ", Params::new().with(params::ID, "checked.name"));
    kernel.send_event(
        Instant(100),
        events::SETTING_TYPED.id().clone(),
        typed.to_params(),
    );
    common::render(&mut kernel);
    assert_eq!(kernel.settings().text("checked.name"), "Ann");
    // An answer while still typing sets nothing.
    let typing =
        paperpaw_core::sdk::Typed::new("An", Params::new().with(params::ID, "checked.name"))
            .while_typing();
    kernel.send_event(
        Instant(150),
        events::SETTING_TYPED.id().clone(),
        typing.to_params(),
    );
    common::render(&mut kernel);
    assert_eq!(kernel.settings().text("checked.name"), "Ann");
    // Refused: a notice shows, until its time is up.
    let effects = kernel
        .set_setting(Instant(200), "checked.count", "7")
        .unwrap();
    assert_eq!(kernel.settings().int("checked.count"), 3);
    assert_eq!(effects.wake_at(), Some(Instant(4_200)));
    common::render(&mut kernel);
    let effects = kernel
        .set_setting(Instant(300), "checked.count", "4")
        .unwrap();
    assert_eq!(kernel.settings().int("checked.count"), 4);
    assert_eq!(effects.wake_at(), Some(Instant(4_200)), "no new notice");
}

fn kernel_with(harness: &Harness, plugin: impl Plugin + 'static) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .plugin(plugin)
        .storage(MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    kernel
}

/// Two buttons, the focus on one of them, moved by *Down*; and the parts
/// where the focus goes, heard.
struct Focused {
    heard: Rc<std::cell::RefCell<Vec<String>>>,
    /// Its buttons are a PIN's keys.
    secret: bool,
}

struct TwoButtons(usize, bool);

impl paperpaw_core::sdk::Screen for TwoButtons {
    fn draw(
        &mut self,
        canvas: &mut paperpaw_core::sdk::Canvas<'_>,
        _: &paperpaw_core::sdk::Env<'_>,
    ) {
        use paperpaw_core::sdk::semantics::Role;
        for (i, label) in ["First", "Second"].iter().enumerate() {
            let area = paperpaw_core::sdk::Rect::new(0, 40 * i as u16, 100, 40);
            canvas
                .describe(Role::Button, label, area)
                .focused(self.0 == i);
        }
    }

    fn secret(&self) -> bool {
        self.1
    }

    fn on_action(
        &mut self,
        action: &paperpaw_core::sdk::Id,
        cx: &mut paperpaw_core::sdk::Cx<'_, '_>,
    ) -> paperpaw_core::sdk::Handled {
        if *action == paperpaw_core::sdk::builtin::actions::DOWN {
            self.0 = (self.0 + 1) % 2;
            cx.request_redraw();
            return paperpaw_core::sdk::Handled::Yes;
        }
        paperpaw_core::sdk::Handled::No
    }
}

impl Plugin for Focused {
    fn manifest(&self) -> Manifest {
        common::manifest("focused")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let screen = paperpaw_core::sdk::Id::new_static("focused.home");
        r.add_screen(paperpaw_core::sdk::ScreenSpec::new(
            screen.clone(),
            "focused.home",
            {
                let secret = self.secret;
                Box::new(move |_, _| Box::new(TwoButtons(0, secret)))
            },
        ))?;
        r.bind_slot(slots::HOME, screen, paperpaw_core::sdk::Priority(1000))?;
        let heard = self.heard.clone();
        r.on(events::FOCUS_CHANGED, move |_, focus, _| {
            heard
                .borrow_mut()
                .push(format!("{} {} {}", focus.role, focus.label, focus.y));
        })
    }
}

/// A plugin that listens hears where the focus goes, once each time.
#[test]
fn plugins_hear_where_the_focus_goes() {
    let heard = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Focused {
            heard: heard.clone(),
            secret: false,
        })
        .storage(MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    common::press(&mut kernel, 100, "down");
    common::press(&mut kernel, 300, "down");
    assert_eq!(
        *heard.borrow(),
        ["button Second 40", "button First 0"],
        "the start's focus is told with the first move"
    );
}

/// The keys of a PIN or a password are never told: the screen is secret.
#[test]
fn the_focus_on_a_secret_screen_is_not_told() {
    let heard = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Focused {
            heard: heard.clone(),
            secret: true,
        })
        .storage(MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    common::press(&mut kernel, 100, "down");
    common::press(&mut kernel, 300, "down");
    assert!(heard.borrow().is_empty(), "{:?}", heard.borrow());
}

/// Listens to the answers typed for settings: it must never hear a
/// password.
struct Spy {
    heard: Rc<std::cell::RefCell<Vec<String>>>,
}

impl Plugin for Spy {
    fn manifest(&self) -> Manifest {
        common::manifest("spy")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let heard = self.heard.clone();
        r.on(
            paperpaw_core::sdk::builtin::events::SETTING_TYPED,
            move |_, typed, _| {
                heard.borrow_mut().push(typed.text.clone());
            },
        )
    }
}

/// A password typed reaches the plugin that asked for it alone; an
/// answer that is no password reaches everyone who listens.
#[test]
fn passwords_typed_reach_the_asker_alone() {
    use paperpaw_core::sdk::builtin::{events, params};
    use paperpaw_core::sdk::event::EventData;
    let heard = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Checked)
        .plugin(Spy {
            heard: heard.clone(),
        })
        .storage(MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let answer = |text: &str, mode: Option<&str>| {
        let mut context = Params::new().with(params::ID, "checked.name");
        if let Some(mode) = mode {
            context.set(params::MODE, mode);
        }
        paperpaw_core::sdk::Typed::new(text, context).to_params()
    };
    kernel.send_event(
        Instant(100),
        events::SETTING_TYPED.id().clone(),
        answer("s3cret", Some("password")),
    );
    // The kernel, which asked, set it; the spy heard nothing.
    assert_eq!(kernel.settings().text("checked.name"), "s3cret");
    assert!(heard.borrow().is_empty(), "{:?}", heard.borrow());
    kernel.send_event(
        Instant(200),
        events::SETTING_TYPED.id().clone(),
        answer("Ann", None),
    );
    assert_eq!(*heard.borrow(), ["Ann"]);
}

/// An input filter that counts what it sees.
struct Watcher {
    seen: Rc<std::cell::Cell<u32>>,
}

impl Plugin for Watcher {
    fn manifest(&self) -> Manifest {
        common::manifest("watcher")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let seen = self.seen.clone();
        r.add_input_filter(paperpaw_core::sdk::filters::InputFilterSpec::new(
            paperpaw_core::sdk::Id::new_static("watcher.filter"),
            "watcher.filter",
            Box::new(move |_, _| {
                seen.set(seen.get() + 1);
                paperpaw_core::sdk::filters::InputVerdict::Pass
            }),
        ))
    }
}

/// While a PIN or a password is entered, other plugins' input filters
/// see none of its keys.
#[test]
fn input_filters_see_no_key_of_a_secret() {
    for secret in [false, true] {
        let seen = Rc::new(std::cell::Cell::new(0));
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(Focused {
                heard: Rc::new(std::cell::RefCell::new(Vec::new())),
                secret,
            })
            .plugin(Watcher { seen: seen.clone() })
            .storage(MemoryStorage::new())
            .build()
            .unwrap();
        kernel.start(Instant(0));
        common::render(&mut kernel);
        common::press(&mut kernel, 100, "down");
        assert_eq!(seen.get() > 0, !secret, "secret {secret}");
    }
}

static VAULT_SETTINGS: [paperpaw_core::sdk::SettingSpec; 3] = [
    paperpaw_core::sdk::SettingSpec::text(
        "vault.key",
        "vault.key",
        "",
        paperpaw_core::sdk::builtin::groups::OTHER,
    )
    .with_extra(&KEPT_SECRET),
    paperpaw_core::sdk::SettingSpec::text(
        "vault.box.note",
        "vault.box.note",
        "",
        paperpaw_core::sdk::builtin::groups::OTHER,
    ),
    // Keeps the device safe: only its plugin and the settings editor
    // change it.
    paperpaw_core::sdk::SettingSpec::text(
        "vault.pin",
        "vault.pin",
        "",
        paperpaw_core::sdk::builtin::groups::OTHER,
    )
    .guarded(),
];
static KEPT_SECRET: paperpaw_core::sdk::SettingExtra =
    paperpaw_core::sdk::SettingExtra::new().secret();

/// What the plugins heard changed: `<plugin> <params>`.
type Heard = Rc<std::cell::RefCell<Vec<String>>>;

/// A plugin with a secret setting (`vault.key`) and one a widget's
/// placements have (`vault.box.note`), when named `vault`; it forgets the
/// secret on `vault.forget`. Any name logs the settings it hears changed.
struct Vault {
    name: &'static str,
    heard: Heard,
}

impl Plugin for Vault {
    fn manifest(&self) -> Manifest {
        common::manifest(self.name)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        if self.name == "vault" {
            r.add_settings(&VAULT_SETTINGS)?;
            r.subscribe(
                paperpaw_core::sdk::Id::new_static("vault.forget"),
                Box::new(|_, _, out| {
                    out.push(paperpaw_core::sdk::Command::ResetSetting(
                        paperpaw_core::sdk::Id::new_static("vault.key"),
                    ));
                }),
            )?;
            r.subscribe(
                paperpaw_core::sdk::Id::new_static("vault.ask"),
                Box::new(|_, _, out| out.push(ask_keyboard("vault.key"))),
            )?;
        }
        if self.name == "spy" {
            // Tries to overwrite the vault's secret: set, typed for, reset.
            r.subscribe(
                paperpaw_core::sdk::Id::new_static("spy.try"),
                Box::new(|_, _, out| {
                    use paperpaw_core::sdk::builtin::params;
                    use paperpaw_core::sdk::event::EventData;
                    let key = paperpaw_core::sdk::Id::new_static("vault.key");
                    out.push(paperpaw_core::sdk::Command::SetSetting(
                        key.clone(),
                        paperpaw_core::sdk::SettingValue::Text("spy-set".into()),
                    ));
                    let typed = paperpaw_core::sdk::Typed::new(
                        "spy-typed",
                        Params::new().with(params::ID, "vault.key"),
                    );
                    out.push(paperpaw_core::sdk::Command::Emit(
                        events::SETTING_TYPED.id().clone(),
                        typed.to_params(),
                    ));
                    out.push(paperpaw_core::sdk::Command::ResetSetting(key));
                    // Asked of the keyboard, read from the clipboard, for
                    // the secret and for the guarded setting.
                    for id in ["vault.key", "vault.pin"] {
                        out.push(ask_keyboard(id));
                        out.push(paperpaw_core::sdk::Command::SetClipboard("spy-clip".into()));
                        out.push(paperpaw_core::sdk::Command::ReadClipboard(
                            events::SETTING_TYPED.id().clone(),
                            Params::new().with(params::ID, id),
                        ));
                    }
                    let typed = paperpaw_core::sdk::Typed::new(
                        "spy-typed",
                        Params::new().with(params::ID, "vault.pin"),
                    );
                    out.push(paperpaw_core::sdk::Command::Emit(
                        events::SETTING_TYPED.id().clone(),
                        typed.to_params(),
                    ));
                }),
            )?;
        }
        let (name, heard) = (self.name, self.heard.clone());
        r.subscribe(
            events::SETTING_CHANGED.id().clone(),
            Box::new(move |_, params, _| heard.borrow_mut().push(format!("{name} {params:?}"))),
        )
    }
}

/// Asks the keyboard for text for setting `id`, answered to the kernel.
fn ask_keyboard(id: &str) -> paperpaw_core::sdk::Command {
    use paperpaw_core::sdk::builtin::{params, slots};
    paperpaw_core::sdk::Command::OpenSlot(
        slots::KEYBOARD,
        Params::new()
            .with(params::ID, id)
            .with(params::REPLY, events::SETTING_TYPED.as_str()),
    )
}

/// A keyboard that answers at once what it is asked, with `keyed`.
struct Keys;

impl Plugin for Keys {
    fn manifest(&self) -> Manifest {
        common::manifest("keys")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        use paperpaw_core::sdk::builtin::slots;
        let screen = paperpaw_core::sdk::Id::new_static("keys.screen");
        r.add_screen(paperpaw_core::sdk::ScreenSpec::new(
            screen.clone(),
            "keys",
            Box::new(|_, asked| {
                Box::new(Answering(Some(asked.clone()))) as Box<dyn paperpaw_core::sdk::Screen>
            }),
        ))?;
        r.bind_slot(
            slots::KEYBOARD,
            screen,
            paperpaw_core::sdk::Priority::DEFAULT,
        )?;
        Ok(())
    }
}

/// The keyboard's screen: answers as it shows, then closes.
struct Answering(Option<Params>);

impl paperpaw_core::sdk::Screen for Answering {
    fn draw(&mut self, _: &mut paperpaw_core::sdk::Canvas<'_>, _: &paperpaw_core::sdk::Env<'_>) {}

    fn on_action(
        &mut self,
        _: &paperpaw_core::sdk::Id,
        _: &mut paperpaw_core::sdk::Cx<'_, '_>,
    ) -> paperpaw_core::sdk::Handled {
        paperpaw_core::sdk::Handled::No
    }

    fn on_show(&mut self, cx: &mut paperpaw_core::sdk::Cx<'_, '_>) {
        use paperpaw_core::sdk::builtin::params;
        use paperpaw_core::sdk::event::EventData;
        let Some(asked) = self.0.take() else {
            return;
        };
        let reply = asked
            .get(params::REPLY)
            .and_then(|r| paperpaw_core::sdk::Id::parse(r).ok());
        if let Some(reply) = reply {
            let typed = paperpaw_core::sdk::Typed::new("keyed", asked.clone());
            cx.command(paperpaw_core::sdk::Command::Emit(reply, typed.to_params()));
        }
        cx.command(paperpaw_core::sdk::Command::Back);
    }
}

/// A kernel with the vault and a spy beside it, on `storage`.
fn vault(storage: MemoryStorage) -> (Kernel, Heard) {
    let heard = Heard::default();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Vault {
            name: "vault",
            heard: heard.clone(),
        })
        .plugin(Vault {
            name: "spy",
            heard: heard.clone(),
        })
        .plugin(Keys)
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    (kernel, heard)
}

/// `text` typed for setting `id`, as the settings' editor sends it.
fn type_for(kernel: &mut Kernel, now: u64, id: &str, text: &str) {
    use paperpaw_core::sdk::builtin::params;
    use paperpaw_core::sdk::event::EventData;
    let typed = paperpaw_core::sdk::Typed::new(text, Params::new().with(params::ID, id));
    kernel.send_event(
        Instant(now),
        events::SETTING_TYPED.id().clone(),
        typed.to_params(),
    );
}

/// A secret setting typed is kept with the secrets, never in the
/// settings every plugin reads; its owner alone hears that it changed,
/// and not to what.
#[test]
fn a_secret_setting_is_kept_with_the_secrets() {
    let (mut kernel, heard) = vault(MemoryStorage::new());
    type_for(&mut kernel, 100, "vault.key", "k-123");
    assert_eq!(kernel.settings().text("vault.key"), "");
    assert_eq!(kernel.secrets().get("vault.key"), Some("k-123"));
    let told = heard.borrow().clone();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].starts_with("vault ") && told[0].contains("vault.key"),
        "{told:?}"
    );
    assert!(!told[0].contains("k-123"), "{told:?}");
    // Set directly, the same.
    heard.borrow_mut().clear();
    kernel
        .set_setting(Instant(200), "vault.key", "k-456")
        .unwrap();
    assert_eq!(kernel.settings().text("vault.key"), "");
    assert_eq!(kernel.secrets().get("vault.key"), Some("k-456"));
    let told = heard.borrow().clone();
    assert!(told.len() == 1 && told[0].starts_with("vault "), "{told:?}");
}

/// Another plugin cannot overwrite a plugin's secret or guarded setting:
/// not set, reset, typed for, nor asked of the keyboard or the
/// clipboard; the plugin itself asks the keyboard for its own, which
/// answers.
#[test]
fn another_plugin_cannot_overwrite_a_secret_setting() {
    let (mut kernel, _) = vault(MemoryStorage::new());
    type_for(&mut kernel, 100, "vault.key", "k-123");
    kernel.send_event(
        Instant(200),
        paperpaw_core::sdk::Id::new_static("spy.try"),
        Params::new(),
    );
    assert_eq!(kernel.secrets().get("vault.key"), Some("k-123"));
    assert_eq!(kernel.settings().text("vault.pin"), "");
    common::render(&mut kernel);
    assert_eq!(kernel.secrets().get("vault.key"), Some("k-123"));
    assert_eq!(kernel.settings().text("vault.pin"), "");
    // Its own, asked of the keyboard by the vault: kept.
    kernel.send_event(
        Instant(300),
        paperpaw_core::sdk::Id::new_static("vault.ask"),
        Params::new(),
    );
    common::render(&mut kernel);
    assert_eq!(kernel.secrets().get("vault.key"), Some("keyed"));
}

/// Reset, a secret setting goes from the secrets; its owner hears it.
#[test]
fn a_secret_setting_reset_goes_from_the_secrets() {
    let (mut kernel, heard) = vault(MemoryStorage::new());
    type_for(&mut kernel, 100, "vault.key", "k-123");
    heard.borrow_mut().clear();
    kernel.send_event(
        Instant(200),
        paperpaw_core::sdk::Id::new_static("vault.forget"),
        Params::new(),
    );
    assert_eq!(kernel.secrets().get("vault.key"), None);
    assert!(!kernel.storage().exists(SECRETS));
    let told = heard.borrow().clone();
    assert!(told.len() == 1 && told[0].starts_with("vault "), "{told:?}");
}

/// A secret setting kept in `paperpaw.ini` before is moved to the
/// secrets at the start, and out of the file.
#[test]
fn a_secret_setting_in_the_file_moves_to_the_secrets() {
    use paperpaw_core::sdk::builtin::CONFIG_FILE;
    let ini = "[vault]\nkey = old-k\nbox.note = kept\n";
    let (kernel, _) = vault(MemoryStorage::new().with(CONFIG_FILE, ini.as_bytes().to_vec()));
    assert_eq!(kernel.secrets().get("vault.key"), Some("old-k"));
    assert_eq!(kernel.settings().text("vault.key"), "");
    assert_eq!(kernel.settings().text("vault.box.note"), "kept");
    let file = paperpaw_core::sdk::read_file(kernel.storage(), CONFIG_FILE, 4096).unwrap();
    let file = String::from_utf8(file).unwrap();
    assert!(!file.contains("old-k"), "{file}");
    assert!(file.contains("kept"), "{file}");
}

/// Typed for a widget's placement, the placement's value is set, not the
/// setting's shared one.
#[test]
fn text_typed_for_a_placement_is_its_own() {
    let (mut kernel, _) = vault(MemoryStorage::new());
    type_for(&mut kernel, 100, "vault.box@2.note", "second");
    let settings = kernel.settings();
    assert_eq!(settings.text("vault.box.note"), "");
    let placed = settings.with_placement(Some("vault.box@2"), || {
        String::from(settings.text("vault.box.note"))
    });
    assert_eq!(placed, "second");
}
