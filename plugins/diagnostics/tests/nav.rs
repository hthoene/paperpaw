//! The device check on a device of buttons alone, as the X4's: the arrow
//! buttons move between the controls of every page, *Select* chooses and
//! *Back* leaves, also on the buttons page until its *Start* is chosen;
//! then every button counts as the one asked for.: it goes through the steps a
//! device has and skips the others, takes the buttons by name, fills the
//! touch targets, tries the card, and writes the report after each
//! answer; it goes on where it was when opened again. A sleep counts once
//! the device went to sleep and woke, not when it started again instead
//! or never slept; the reader's own light comes back after a check that
//! ended while trying it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel, KernelBuilder};
use paperpaw_platform::{
    ButtonState, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Rotation,
};
use paperpaw_plugin_diagnostics::steps::REPORT;
use paperpaw_plugin_diagnostics::{CHECK, Diagnostics, OPEN_CHECK};
use paperpaw_sdk::{MemoryStorage, Storage};

const WIDTH: u16 = 480;
const HEIGHT: u16 = 800;
const DPI: u16 = 220;
/// As the X4's firmware lists them.
const KEYS: [&str; 7] = [
    "back",
    "confirm",
    "left",
    "right",
    "page-back",
    "page-forward",
    "power",
];

/// Buttons and touch; no light, no Wi-Fi, no sleep of PaperPaw's own.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test",
    DisplayInfo::new(WIDTH, HEIGHT, DPI, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &KEYS),
    MemoryClass::Large,
);

struct Device {
    kernel: Kernel,
    now: u64,
    /// Each refresh the shell was asked for: whether in grays, and how
    /// much of the screen it covers (in percent).
    refreshes: Vec<(bool, u32)>,
}

impl Device {
    /// A device whose card holds `report` already, if any.
    fn new(report: Option<&str>) -> Self {
        let files: Vec<(&str, &str)> = report.map(|r| (REPORT, r)).into_iter().collect();
        Self::with(&files, |builder| builder)
    }

    /// A device whose card holds `files`, its kernel as `setup` makes it.
    fn with(files: &[(&str, &str)], setup: impl FnOnce(KernelBuilder) -> KernelBuilder) -> Self {
        let storage = MemoryStorage::new();
        for (path, text) in files {
            storage.write(path, text.as_bytes()).unwrap();
        }
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[core]\nlanguage = en\n")
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Diagnostics);
        let kernel = setup(builder).build().unwrap();
        let mut device = Self {
            kernel,
            now: 0,
            refreshes: Vec::new(),
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device
    }

    /// Notes the refresh `effects` ask for, if any.
    fn note(&mut self, effects: &Effects) {
        if let Some((area, _)) = effects.refresh() {
            let screen = u32::from(WIDTH) * u32::from(HEIGHT);
            let share = u32::from(area.width) * u32::from(area.height) * 100 / screen;
            self.refreshes.push((effects.grays(), share));
        }
    }

    /// Runs the timers due soon, then draws what changed, as a device
    /// would.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            self.note(&effects);
            let Some(at) = effects.wake_at() else {
                break;
            };
            if at.0 > self.now + 1000 {
                break;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
        self.kernel.render(&mut |_, _| {});
    }

    fn run(&mut self, action: paperpaw_sdk::Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn button(&mut self, name: &'static str, long: bool) {
        self.now += 1000;
        let pressed = InputEvent::Button {
            name,
            state: ButtonState::Pressed,
        };
        let effects = self.kernel.handle(Instant(self.now), pressed);
        self.note(&effects);
        if long {
            self.now += 1500;
            let effects = self.kernel.tick(Instant(self.now));
            self.settle(effects);
        }
        self.now += 50;
        let released = InputEvent::Button {
            name,
            state: ButtonState::Released,
        };
        let effects = self.kernel.handle(Instant(self.now), released);
        self.settle(effects);
    }

    /// The texts on the screen.
    fn texts(&mut self) -> Vec<String> {
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    fn shows(&mut self, text: &str) -> bool {
        self.texts().iter().any(|t| t == text)
    }

    fn report(&self) -> String {
        paperpaw_sdk::files::read_text(self.kernel.storage(), REPORT, 1 << 16).unwrap()
    }

    fn open_check(&mut self) {
        self.run(OPEN_CHECK);
        assert_eq!(self.kernel.active_screen(), Some(&CHECK));
    }
}

impl Device {
    /// The labels of the controls with the focus.
    fn focus(&mut self) -> String {
        self.kernel
            .describe()
            .into_iter()
            .filter(|p| p.focused)
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join("|")
    }

    fn press(&mut self, names: &[&'static str]) {
        for name in names {
            self.button(name, false);
        }
    }
}

#[test]
fn the_first_page_is_gone_over_with_the_arrows_and_the_page_buttons() {
    let mut d = Device::new(None);
    d.open_check();
    // It opens on the first tool, not on the answer it suggests: a first
    // arrow goes on from there, a first Select flips the pattern, and the
    // page is not passed unseen.
    assert_eq!(d.focus(), "Flip pattern");
    // The controls in a ring: Flip pattern, Full refresh, Fail, Skip, Pass.
    let mut seen = Vec::new();
    for _ in 0..5 {
        d.press(&["right"]);
        seen.push(d.focus());
    }
    assert_eq!(
        seen,
        ["Full refresh", "Fail", "Skip", "Pass", "Flip pattern"]
    );
    d.press(&["left"]);
    assert_eq!(d.focus(), "Pass");
    d.press(&["page-back"]);
    assert_eq!(d.focus(), "Skip");
    d.press(&["page-forward", "page-forward"]);
    assert_eq!(d.focus(), "Flip pattern");
    d.press(&["confirm"]);
    assert!(d.shows("Screen"), "Select on a tool stays: {:?}", d.texts());
    assert_eq!(d.focus(), "Flip pattern");
    d.press(&["left", "confirm"]);
    assert!(d.shows("Buttons"), "{:?}", d.texts());
}

#[test]
fn the_buttons_page_is_moved_over_and_left_before_it_is_started() {
    let mut d = Device::new(None);
    d.open_check();
    d.press(&["left", "confirm"]);
    assert!(d.shows("Buttons"), "{:?}", d.texts());
    assert_eq!(d.focus(), "Start");
    // Nothing is asked for yet: the arrows move, none counts.
    d.press(&["right"]);
    assert_eq!(d.focus(), "Fail");
    d.press(&["right", "right"]);
    assert_eq!(d.focus(), "Pass");
    d.press(&["right"]);
    assert_eq!(d.focus(), "Start");
    // Left from the first goes round to the last.
    d.press(&["left", "page-back", "page-forward"]);
    assert_eq!(d.focus(), "Pass");
    d.press(&["right"]);
    assert_eq!(d.focus(), "Start");
    assert!(!d.shows("Press now"), "{:?}", d.texts());
    // Skip, chosen with Select, goes on at once.
    d.press(&["right", "right", "confirm"]);
    assert!(d.shows("Battery"), "{:?}", d.texts());
    assert!(
        d.report().lines().any(|l| l == "buttons: skip"),
        "{}",
        d.report()
    );
}

#[test]
fn back_leaves_the_buttons_page_before_it_is_started() {
    let mut d = Device::new(None);
    d.open_check();
    d.press(&["left", "confirm", "back"]);
    assert_ne!(d.kernel.active_screen(), Some(&CHECK));
    // It goes on where it was: the buttons page, not started.
    d.open_check();
    assert!(d.shows("Buttons"), "{:?}", d.texts());
    assert_eq!(d.focus(), "Start");
    assert!(!d.shows("Press now"), "{:?}", d.texts());
}

#[test]
fn started_it_asks_for_each_button_as_the_device_lists_them() {
    let mut d = Device::new(None);
    d.open_check();
    d.press(&["left", "confirm", "confirm"]);
    assert!(d.shows("Press now"), "{:?}", d.texts());
    assert_eq!(d.focus(), "", "no control is chosen while they are asked");
    // Back is the first asked: it counts, and does not leave.
    d.press(&["back"]);
    assert_eq!(d.kernel.active_screen(), Some(&CHECK));
    // A slip changes nothing.
    d.press(&["power"]);
    assert!(d.shows("Buttons"), "{:?}", d.texts());
    d.press(&[
        "confirm",
        "left",
        "right",
        "page-back",
        "page-forward",
        "power",
    ]);
    assert!(d.shows("Battery"), "{:?}", d.texts());
    assert!(
        d.report()
            .lines()
            .any(|l| l
                == "buttons: pass (back, confirm, left, right, page-back, page-forward, power)"),
        "{}",
        d.report()
    );
}

#[test]
fn started_a_button_held_gives_the_one_asked_up() {
    let mut d = Device::new(None);
    d.open_check();
    d.press(&["left", "confirm", "confirm"]);
    // Back does not work: Select held instead.
    d.button("confirm", true);
    d.press(&[
        "confirm",
        "left",
        "right",
        "page-back",
        "page-forward",
        "power",
    ]);
    assert!(d.shows("Battery"), "{:?}", d.texts());
    assert!(
        d.report()
            .lines()
            .any(|l| l == "buttons: fail (not working: back)"),
        "{}",
        d.report()
    );
}

impl Device {
    /// The refreshes asked for since the last call, those a pause held
    /// back included.
    fn refreshed(&mut self) -> Vec<(bool, u32)> {
        self.now += 3000;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
        core::mem::take(&mut self.refreshes)
    }
}

#[test]
fn a_key_on_the_first_page_does_not_ask_for_the_whole_screen_in_grays() {
    let mut d = Device::new(None);
    d.open_check();
    // The page opens in grays, its stripes as the panel shows them.
    let opened = d.refreshed();
    assert!(opened.iter().any(|r| r.0), "{opened:?}");
    // Moving the focus changes a button or two, in black and white: on
    // the X4 a page in grays is a refresh of all the screen for any
    // change, taking seconds, and a small change after it is not shown.
    for key in ["right", "right", "left", "page-forward", "page-back"] {
        d.press(&[key]);
        let asked = d.refreshed();
        assert!(!asked.is_empty(), "{key} shows something");
        assert!(asked.iter().all(|r| !r.0), "{key}: {asked:?}");
    }
    // Flipping the pattern too, and flipping it back.
    d.press(&["left"]);
    assert_eq!(d.focus(), "Flip pattern");
    d.refreshed();
    for _ in 0..2 {
        d.press(&["confirm"]);
        let asked = d.refreshed();
        assert!(asked.iter().all(|r| !r.0), "{asked:?}");
    }
}

#[test]
fn a_full_refresh_shows_the_stripes_in_grays_again() {
    let mut d = Device::new(None);
    d.open_check();
    d.press(&["right", "right", "left"]);
    assert_eq!(d.focus(), "Full refresh");
    d.refreshed();
    d.press(&["confirm"]);
    let asked = d.refreshed();
    assert!(asked.iter().any(|r| r.0), "{asked:?}");
    // And the next key is in black and white again.
    d.press(&["right"]);
    let asked = d.refreshed();
    assert!(asked.iter().all(|r| !r.0), "{asked:?}");
}
