//! The device check with the real kernel: it goes through the steps a
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
    Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_diagnostics::steps::REPORT;
use paperpaw_plugin_diagnostics::{CHECK, Diagnostics, OPEN_CHECK};
use paperpaw_sdk::builtin::{actions, settings};
use paperpaw_sdk::power::Start;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{MemoryStorage, Metrics, Storage};

const WIDTH: u16 = 480;
const HEIGHT: u16 = 800;
const DPI: u16 = 220;
const KEYS: [&str; 3] = ["page-back", "page-forward", "confirm"];

/// Buttons and touch; no light, no Wi-Fi, no sleep of PaperPaw's own.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test",
    DisplayInfo::new(WIDTH, HEIGHT, DPI, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &KEYS),
    MemoryClass::Large,
);

struct Device {
    kernel: Kernel,
    now: u64,
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
        let mut device = Self { kernel, now: 0 };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device
    }

    /// Runs the timers due soon, then draws what changed, as a device
    /// would.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
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
        let _ = self.kernel.handle(Instant(self.now), pressed);
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

    fn tap(&mut self, at: Point) {
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 50;
            let event =
                InputEvent::Pointer(PointerEvent::new(0, phase, at, PointerTool::Finger, 1000));
            let effects = self.kernel.handle(Instant(self.now), event);
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
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

    /// Taps the button labelled `label`.
    fn choose(&mut self, label: &str) {
        let part = self
            .kernel
            .describe()
            .into_iter()
            .find(|p| p.role == Role::Button && p.label == label)
            .unwrap_or_else(|| panic!("no button {label}: {:?}", self.texts()));
        let a = part.area;
        let centre = Point::new(
            i32::from(a.x) + i32::from(a.width) / 2,
            i32::from(a.y) + i32::from(a.height) / 2,
        );
        self.tap(centre);
    }

    /// Taps the touch page's targets, as the check places them.
    fn tap_targets(&mut self) {
        let inset = Metrics::new(DPI).mm(7) as i32;
        let span = |size: u16, tenths: i32| inset + (i32::from(size) - 2 * inset) * tenths / 10;
        for (x, y) in [
            (0, 0),
            (5, 0),
            (10, 0),
            (0, 5),
            (10, 5),
            (0, 10),
            (5, 10),
            (10, 10),
        ] {
            self.tap(Point::new(span(WIDTH, x), span(HEIGHT, y)));
        }
    }

    /// Moves the clock on by `ms`, running what is due.
    fn wait(&mut self, ms: u64) {
        self.now += ms;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
    }

    fn report(&self) -> String {
        paperpaw_sdk::files::read_text(self.kernel.storage(), REPORT, 1 << 16).unwrap()
    }

    fn open_check(&mut self) {
        self.run(OPEN_CHECK);
        assert_eq!(self.kernel.active_screen(), Some(&CHECK));
    }
}

#[test]
fn it_goes_through_the_steps_and_writes_the_report() {
    let mut d = Device::new(None);
    d.open_check();
    assert!(d.shows("Screen"), "{:?}", d.texts());
    d.choose("Pass");

    // Every button counts, Select and the page buttons too.
    assert!(d.shows("Buttons"), "{:?}", d.texts());
    for name in KEYS {
        d.button(name, false);
    }

    assert!(d.shows("Touch"), "{:?}", d.texts());
    d.tap_targets();

    // The light is skipped: the device has none.
    assert!(d.shows("Battery"), "{:?}", d.texts());
    d.choose("Skip");
    assert!(d.shows("Card"), "{:?}", d.texts());
    d.choose("Pass");
    assert!(d.shows("Clock"), "{:?}", d.texts());
    d.choose("Skip");
    // Wi-Fi and sleep are skipped: PaperPaw manages neither here.
    assert!(d.shows("Memory"), "{:?}", d.texts());
    d.choose("Skip");
    assert!(d.shows("Done"), "{:?}", d.texts());

    let report = d.report();
    for line in [
        "Device: Test (test), 220 dpi, Gray4",
        "screen: pass (16 gray levels)",
        "buttons: pass (page-back, page-forward, confirm)",
        "touch: pass (corners and edges)",
        "light: skip (no front light)",
        "battery: skip (no battery reported)",
        "card: pass (128 KiB)",
        "clock: skip (time of day not known)",
        "wifi: skip (Wi-Fi not managed by PaperPaw)",
        "sleep: skip (sleep not managed by PaperPaw)",
        "memory: skip (not reported)",
        "Result: 4 passed, 0 failed, 6 skipped",
    ] {
        assert!(report.lines().any(|l| l == line), "{line} in\n{report}");
    }
    // The card's test file went again.
    assert!(
        d.kernel
            .storage()
            .size(paperpaw_sdk::data_path!("device-check.bin"))
            .is_err()
    );
}

#[test]
fn a_button_held_instead_gives_the_one_asked_up() {
    let mut d = Device::new(None);
    d.open_check();
    d.choose("Pass");
    // Page back does not work: page forward held instead.
    d.button("page-forward", true);
    d.button("page-forward", false);
    // A slip, not the one asked: nothing changes.
    d.button("page-forward", false);
    d.button("confirm", false);
    assert!(d.shows("Touch"), "{:?}", d.texts());
    assert!(
        d.report()
            .lines()
            .any(|l| l == "buttons: fail (not working: page-back)"),
        "{}",
        d.report()
    );
}

#[test]
fn it_goes_on_where_it_was() {
    let mut d = Device::new(None);
    d.open_check();
    d.choose("Fail");
    d.run(actions::BACK);
    assert_ne!(d.kernel.active_screen(), Some(&CHECK));
    d.open_check();
    assert!(d.shows("Buttons"), "{:?}", d.texts());
    assert!(
        d.report()
            .lines()
            .any(|l| l == "screen: fail (16 gray levels)")
    );
}

/// Every step before the sleep answered; `sleep` the line about it.
fn before_sleep(sleep: &str) -> String {
    format!(
        "screen: pass\nbuttons: pass\ntouch: pass\nlight: skip\nbattery: skip\n\
         card: pass\nclock: skip\nwifi: skip\n{sleep}"
    )
}

fn has(report: &str, line: &str) -> bool {
    report.lines().any(|l| l == line)
}

#[test]
fn a_sleep_ended_by_the_power_counts_and_all_can_start_over() {
    // Asleep when last seen, then switched on by the power button.
    let report = before_sleep("sleep: asleep\n");
    let mut d = Device::with(&[(REPORT, &report)], |b| b.started(Start::PowerOn));
    d.open_check();
    assert!(has(&d.report(), "sleep: pass (woke)"), "{}", d.report());
    assert!(d.shows("Memory"), "{:?}", d.texts());
    d.choose("Pass");
    assert!(d.shows("Done"), "{:?}", d.texts());
    d.choose("Start over");
    assert!(d.shows("Screen"), "{:?}", d.texts());
    assert!(!d.report().contains("sleep:"), "{}", d.report());
}

#[test]
fn a_start_again_instead_of_a_wake_fails() {
    let report = before_sleep("sleep: asleep\n");
    let mut d = Device::with(&[(REPORT, &report)], |b| b.started(Start::Restart));
    d.open_check();
    assert!(
        has(&d.report(), "sleep: fail (started again instead of waking)"),
        "{}",
        d.report()
    );
}

#[test]
fn a_sleep_counts_once_the_device_went_to_sleep_and_woke() {
    let report = before_sleep("");
    let mut d = Device::with(&[(REPORT, &report)], |b| b.sleep(true));
    d.open_check();
    assert!(d.shows("Sleep"), "{:?}", d.texts());
    d.choose("Sleep now");
    // Asked, not yet asleep.
    assert!(!d.report().contains("sleep:"), "{}", d.report());
    d.now += 100;
    let effects = d.kernel.suspend(Instant(d.now));
    d.settle(effects);
    assert!(has(&d.report(), "sleep: asleep"), "{}", d.report());
    d.now += 5000;
    let effects = d.kernel.resume(Instant(d.now));
    d.settle(effects);
    assert!(has(&d.report(), "sleep: pass (woke)"), "{}", d.report());
    assert!(d.shows("Memory"), "{:?}", d.texts());
}

#[test]
fn a_device_that_does_not_go_to_sleep_fails() {
    let report = before_sleep("");
    let mut d = Device::with(&[(REPORT, &report)], |b| b.sleep(true));
    d.open_check();
    d.choose("Sleep now");
    // The shell never sends the kernel to sleep.
    d.wait(11_000);
    assert!(
        has(&d.report(), "sleep: fail (did not go to sleep)"),
        "{}",
        d.report()
    );
}

#[test]
fn the_readers_own_light_comes_back_after_a_check_that_ended_trying_it() {
    // The check ended while trying the light: the card still keeps the
    // reader's own, the settings a level tried.
    let light = paperpaw_sdk::data_path!("device-check-light.txt");
    let report = "screen: pass\nbuttons: pass\ntouch: pass\n";
    let mut d = Device::with(&[(REPORT, report), (light, "30 70\n")], |b| {
        b.frontlight(true)
    });
    let level = |d: &Device| {
        (
            d.kernel.settings().read(&settings::FRONTLIGHT),
            d.kernel.settings().read(&settings::FRONTLIGHT_WARMTH),
        )
    };
    d.open_check();
    assert!(d.shows("Front light"), "{:?}", d.texts());
    // The light step began again, the reader's own kept as the card had it.
    let kept = paperpaw_sdk::files::read_text(d.kernel.storage(), light, 64).unwrap();
    assert_eq!(kept, "30 70\n");
    for _ in 0..6 {
        d.wait(1600);
    }
    assert_eq!(level(&d), (30, 70));
    assert!(d.kernel.storage().size(light).is_err(), "kept no longer");
}
