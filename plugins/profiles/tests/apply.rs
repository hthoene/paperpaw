//! Reading profiles with the real kernel, by touch and by buttons only:
//! saving the current settings as a profile (named on the keyboard),
//! applying one changes the settings and marks it in use, *Next profile*
//! goes round them, renaming cleans the name, updating and removing, and
//! hostile files (values a setting does not take, unknown settings) are
//! applied as far as they are valid. What is written (`profiles.ini`,
//! `last.txt`) is checked on the storage.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat, Point,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_plugin_profiles::Profiles;
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::semantics::{self, Part, Role};
use paperpaw_sdk::{Env, Id, MemoryStorage, Params};

/// An X4 as the emulator's profile has it: buttons, no touch.
const BUTTONS: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// A touch screen as large as the X4's.
const TOUCH: DeviceInfo = BUTTONS
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

const NIGHT: &str = "core.night-mode";

fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir("profiles"))
}

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new(device: DeviceInfo) -> Self {
        Self::on(device, MemoryStorage::new())
    }

    fn on(device: DeviceInfo, storage: MemoryStorage) -> Self {
        let kernel = Kernel::builder(device)
            .storage(storage)
            .user_config("[core]\nlanguage = en\n")
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Keyboard)
            .plugin(Profiles)
            .build()
            .unwrap();
        let mut device = Self { kernel, now: 0 };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn on_top(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }

    fn parts(&mut self) -> Vec<Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    fn open(&mut self) {
        self.run(Id::new_static("profiles.open"));
        assert_eq!(self.on_top(), Some("profiles.list"));
    }

    fn set(&mut self, id: &str, value: &str) {
        self.now += 100;
        let effects = self
            .kernel
            .set_setting(Instant(self.now), id, value)
            .unwrap();
        self.settle(effects);
    }

    fn night(&self) -> bool {
        self.kernel.settings().bool(NIGHT)
    }

    fn read(&self, name: &str) -> Option<String> {
        let data = paperpaw_sdk::read_file(self.kernel.storage(), &file(name), 1 << 16).ok()?;
        Some(String::from_utf8(data).unwrap())
    }

    fn finger(&mut self, phase: PointerPhase, point: Point) {
        self.now += 50;
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            point,
            PointerTool::Finger,
            1000,
        ));
        let effects = self.kernel.handle(Instant(self.now), event);
        if phase == PointerPhase::Up {
            self.settle(effects);
        }
    }

    /// The part whose text begins with `start`.
    fn part(&mut self, start: &str) -> Option<Part> {
        self.parts()
            .into_iter()
            .find(|p| matches!(p.role, Role::Row | Role::Button) && p.label.starts_with(start))
    }

    fn at(&mut self, start: &str) -> Point {
        let area = self
            .part(start)
            .unwrap_or_else(|| panic!("nothing says {start:?}"))
            .area;
        Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        )
    }

    fn tap(&mut self, start: &str) {
        let at = self.at(start);
        self.now += 1000;
        self.finger(PointerPhase::Down, at);
        self.finger(PointerPhase::Up, at);
    }

    fn hold(&mut self, start: &str) {
        let at = self.at(start);
        self.now += 1000;
        self.finger(PointerPhase::Down, at);
        self.now += 1500;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
        self.finger(PointerPhase::Up, at);
    }

    /// Moves the buttons' focus down to the row that begins with `start`.
    fn focus(&mut self, start: &str) {
        for _ in 0..40 {
            let parts = self.parts();
            if semantics::focused(&parts).is_some_and(|p| p.label.starts_with(start)) {
                return;
            }
            self.run(actions::DOWN);
        }
        panic!("no row {start:?}");
    }

    /// Saves the settings as a new profile, keeping the name offered.
    fn add(&mut self) {
        if self.kernel.device().input.touch {
            self.tap("Save as new profile");
            self.tap("OK");
        } else {
            self.focus("Save as new profile");
            self.run(actions::CONFIRM);
            // On the keyboard: Back held finishes (the focus is on a
            // letter, where a held Select shows its capital).
            self.run(paperpaw_plugin_keyboard::DONE);
        }
        assert_eq!(self.on_top(), Some("profiles.list"));
    }

    /// The rows of the list, as they say.
    fn rows(&mut self) -> Vec<String> {
        self.parts()
            .into_iter()
            .filter(|p| p.role == Role::Row)
            .map(|p| p.label)
            .collect()
    }

    /// Text typed for the question `event`, about profile `index`.
    fn answer(&mut self, event: &'static str, text: &str, index: usize) {
        let typed = Typed::new(
            text.to_string(),
            Params::new().with(params::PATH, index.to_string()),
        );
        self.now += 100;
        let effects =
            self.kernel
                .send_event(Instant(self.now), Id::new_static(event), typed.to_params());
        self.settle(effects);
    }
}

#[test]
fn a_profile_saved_and_applied_by_touch() {
    let mut device = Device::new(TOUCH);
    device.open();
    assert!(device.parts().iter().any(|p| p.label == "No profiles yet"));
    assert!(!device.night());
    device.add();
    let rows = device.rows();
    assert!(rows[0].starts_with("Profile 1, In use"), "{rows:?}");
    let saved = device.read("profiles.ini").unwrap();
    assert!(
        saved.starts_with("# format 2\n[profile.1]\nname = Profile 1\n"),
        "{saved}"
    );
    assert!(saved.contains("core.night-mode = off"), "{saved}");
    // Folders and languages are not a way of reading.
    assert!(!saved.contains("core.language"), "{saved}");
    assert_eq!(device.read("last.txt").as_deref(), Some("Profile 1"));

    // Night mode on: no longer as the profile has it.
    device.set(NIGHT, "on");
    assert!(!device.rows()[0].contains("In use"));
    device.add();
    let rows = device.rows();
    assert!(rows[1].starts_with("Profile 2, In use"), "{rows:?}");

    device.tap("Profile 1");
    assert!(!device.night());
    assert_eq!(device.read("last.txt").as_deref(), Some("Profile 1"));
    let rows = device.rows();
    assert!(
        rows[0].contains("In use") && !rows[1].contains("In use"),
        "{rows:?}"
    );
}

#[test]
fn a_profile_saved_and_applied_by_buttons() {
    let mut device = Device::new(BUTTONS);
    device.open();
    device.add();
    device.set(NIGHT, "on");
    device.add();
    device.focus("Profile 1");
    device.run(actions::CONFIRM);
    assert!(!device.night());
    device.focus("Profile 2");
    device.run(actions::CONFIRM);
    assert!(device.night());
    assert_eq!(device.read("last.txt").as_deref(), Some("Profile 2"));
}

#[test]
fn next_profile_goes_round() {
    let mut device = Device::new(TOUCH);
    // None yet: nothing happens.
    device.run(Id::new_static("profiles.next"));
    assert_eq!(device.read("last.txt"), None);
    device.open();
    device.add();
    device.set(NIGHT, "on");
    device.add();
    device.run(actions::BACK);
    // Profile 2 was saved last: next is the first.
    device.run(Id::new_static("profiles.next"));
    assert!(!device.night());
    device.run(Id::new_static("profiles.next"));
    assert!(device.night());
    device.run(Id::new_static("profiles.next"));
    assert!(!device.night());
    assert_eq!(device.read("last.txt").as_deref(), Some("Profile 1"));
}

#[test]
fn next_profile_starts_at_the_first_when_the_last_is_gone() {
    let storage = MemoryStorage::new()
        .with(
            &file("profiles.ini"),
            "# format 2\n[profile.1]\nname = Day\ncore.night-mode = off\n\n\
             [profile.2]\nname = Night\ncore.night-mode = on\n",
        )
        .with(&file("last.txt"), "Removed long ago");
    let mut device = Device::on(TOUCH, storage);
    device.set(NIGHT, "on");
    device.run(Id::new_static("profiles.next"));
    assert!(!device.night());
    assert_eq!(device.read("last.txt").as_deref(), Some("Day"));
}

#[test]
fn renaming_cleans_the_name_and_a_name_in_use_is_refused() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.add();
    device.hold("Profile 1");
    assert_eq!(device.on_top(), Some("core.choice-dialog"));
    device.tap("Rename");
    assert_eq!(device.on_top(), Some("keyboard.screen"));
    device.run(actions::BACK);
    // As if typed: brackets go, lines and spaces become one space.
    device.answer("profiles.renamed", "  Late\n [night]   reading ", 0);
    assert!(device.rows()[0].starts_with("Late night reading"));
    let saved = device.read("profiles.ini").unwrap();
    assert!(saved.contains("name = Late night reading\n"), "{saved}");
    // Nothing but spaces: the name stays.
    device.answer("profiles.renamed", " \n\t[ ] ", 0);
    assert!(device.rows()[0].starts_with("Late night reading"));
    // A rename for a profile not there: nothing.
    device.answer("profiles.renamed", "Ghost", 7);
    assert!(!device.rows().iter().any(|r| r.starts_with("Ghost")));
    // A new profile of a name taken, whatever its case, is refused: the
    // one of that name stays as it was.
    device.set(NIGHT, "on");
    for name in ["Late night reading", "LATE NIGHT reading"] {
        device.answer("profiles.named", name, 0);
        let rows = device.rows();
        assert_eq!(
            rows.iter()
                .filter(|r| r.to_lowercase().starts_with("late night"))
                .count(),
            1,
            "{rows:?}"
        );
    }
    assert_eq!(device.read("profiles.ini").unwrap(), saved);
    device.tap("Late night reading");
    assert!(!device.night());
}

#[test]
fn updating_and_removing_a_profile() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.add();
    device.add();
    device.set(NIGHT, "on");
    device.hold("Profile 1");
    device.tap("Save the current settings");
    device.tap("Replace");
    assert_eq!(device.on_top(), Some("profiles.list"));
    let saved = device.read("profiles.ini").unwrap();
    let first = saved.split("[profile.2]").next().unwrap();
    assert!(first.contains("core.night-mode = on"), "{saved}");
    device.hold("Profile 1");
    device.tap("Remove");
    device.tap("Remove");
    let rows = device.rows();
    assert!(rows[0].starts_with("Profile 2"), "{rows:?}");
    device.hold("Profile 2");
    device.tap("Remove");
    device.tap("Remove");
    assert!(device.parts().iter().any(|p| p.label == "No profiles yet"));
    let saved = device.read("profiles.ini").unwrap();
    assert_eq!(saved, "# format 2\n");
}

#[test]
fn updating_and_removing_a_profile_ask_first() {
    for device in [TOUCH, BUTTONS] {
        let mut device = Device::new(device);
        device.open();
        device.add();
        device.set(NIGHT, "on");
        let saved = device.read("profiles.ini").unwrap();
        let touch = device.kernel.device().input.touch;
        for (option, keep) in [("Save the current settings", "Cancel"), ("Remove", "Keep")] {
            if touch {
                device.hold("Profile 1");
                device.tap(option);
            } else {
                device.focus("Profile 1");
                device.run(actions::MENU);
                device.focus(option);
                device.run(actions::CONFIRM);
            }
            assert_eq!(device.on_top(), Some("core.confirm-dialog"));
            let parts = device.parts();
            assert!(
                parts.iter().any(|p| p.label.contains("Profile 1")),
                "{parts:?}"
            );
            // Not the answer: nothing changes.
            if touch {
                device.tap(keep);
            } else {
                device.run(actions::CONFIRM);
            }
            assert_eq!(device.on_top(), Some("profiles.list"));
            assert_eq!(device.read("profiles.ini").unwrap(), saved);
        }
        // Yes, by buttons too: up to it and confirm.
        if !touch {
            device.focus("Profile 1");
            device.run(actions::MENU);
            device.focus("Remove");
            device.run(actions::CONFIRM);
            device.run(actions::UP);
            device.run(actions::CONFIRM);
            assert_eq!(
                device.read("profiles.ini").unwrap(),
                "# format 2
"
            );
        }
    }
}

#[test]
fn hostile_values_are_applied_as_far_as_they_are_valid() {
    let storage = MemoryStorage::new().with(
        &file("profiles.ini"),
        "# format 2\n[profile.1]\nname = Odd\n\
         core.night-mode = maybe\n\
         no.such-setting = 3\n\
         core.full-refresh-interval = -99999999999999999999\n\
         core.rotation = sideways\n\
         core.night-pictures = on\n",
    );
    let mut device = Device::on(TOUCH, storage);
    let before = device.night();
    device.open();
    // Not in use: some of it can be applied.
    assert!(!device.rows()[0].contains("In use"));
    device.tap("Odd");
    assert_eq!(device.night(), before);
    assert!(device.kernel.settings().bool("core.night-pictures"));
    // All that could be applied is: now it is in use.
    assert!(device.rows()[0].contains("In use"), "{:?}", device.rows());
}

#[test]
fn a_profile_with_nothing_valid_is_never_in_use() {
    let storage = MemoryStorage::new().with(
        &file("profiles.ini"),
        "# format 2\n[profile.1]\nname = Empty\n\n[profile.2]\nname = Unknown\nno.such = 1\n",
    );
    let mut device = Device::on(BUTTONS, storage);
    device.open();
    let rows = device.rows();
    assert!(!rows[0].contains("In use"), "{rows:?}");
    device.focus("Empty");
    device.run(actions::CONFIRM);
    assert_eq!(device.read("last.txt").as_deref(), Some("Empty"));
    assert!(!device.rows()[0].contains("In use"));
}

#[test]
fn a_profile_file_of_version_1_is_shown_and_saved_as_version_2() {
    let storage = MemoryStorage::new().with(
        &file("profiles.ini"),
        include_str!("../testdata/profiles.ini.v1"),
    );
    let mut device = Device::on(TOUCH, storage);
    device.open();
    let rows = device.rows();
    assert!(rows[0].starts_with("Night"), "{rows:?}");
    assert!(rows[1].starts_with("Large print"), "{rows:?}");
    device.add();
    let saved = device.read("profiles.ini").unwrap();
    assert!(
        saved.starts_with("# format 2\n[profile.1]\nname = Night\n"),
        "{saved}"
    );
    assert!(saved.contains("name = Large print"), "{saved}");
    assert!(saved.contains("name = Profile 3"), "{saved}");
}

#[test]
fn storage_errors_are_survived() {
    // The plugin's folder is a file: nothing can be written there.
    let storage = MemoryStorage::new().with(".paperpaw/plugins/profiles", "not a folder");
    let mut device = Device::on(TOUCH, storage);
    device.open();
    device.add();
    device.run(Id::new_static("profiles.next"));
    assert!(device.kernel.storage().exists(".paperpaw/plugins/profiles"));
}
