//! First-time hints with the real kernel: shown over a screen the first
//! time it opens, once; closed by a tap or a button; switched off with
//! *No more hints*; shown again when asked; for the device's input only.

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
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::help::{self, HelpHint, Zone};
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, MemoryStorage, Params, Plugin, Priority, RegisterError,
    Registrar, Screen, ScreenSpec, Version,
};

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

const TOUCH: DeviceInfo = BUTTONS
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

const HOME: Id = Id::new_static("probe.home");
const OTHER: Id = Id::new_static("probe.other");

/// A home screen without hints, and a screen with some.
struct Probe;

struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            cx.command(paperpaw_sdk::Command::Back);
            return Handled::Yes;
        }
        Handled::No
    }
}

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            HOME,
            "probe",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.add_screen(ScreenSpec::new(
            OTHER,
            "probe",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)?;
        help::add(
            r,
            OTHER.as_str(),
            &[
                HelpHint::touch("probe.next").zone(Zone::new(667, 0, 333, 1000)),
                HelpHint::any("probe.both"),
                HelpHint::buttons("probe.buttons"),
            ],
        )?;
        r.add_translations(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\n\
             probe.next = Tap here: next\nprobe.both = For all\nprobe.buttons = For buttons\n",
        )
    }
}

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new(device: DeviceInfo, config: &str) -> Self {
        Self::on(device, config, MemoryStorage::new())
    }

    fn on(device: DeviceInfo, config: &str, storage: impl paperpaw_sdk::Storage + 'static) -> Self {
        let mut kernel = Kernel::builder(device)
            .storage(storage)
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Probe)
            .plugin(paperpaw_plugin_help::Help)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut device = Self { kernel, now: 0 };
        device.settle(effects);
        device
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..1000 {
            match effects.wake_at() {
                Some(at) if at.0 <= self.now + 1000 => {
                    self.now = self.now.max(at.0);
                    effects = self.kernel.tick(Instant(self.now));
                }
                _ => return,
            }
        }
    }

    fn open(&mut self, screen: Id) {
        self.open_with(screen, Params::new());
    }

    fn open_with(&mut self, screen: Id, params: Params) {
        self.now += 100;
        let effects = self.kernel.open_screen(Instant(self.now), screen, params);
        self.settle(effects);
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn on_top(&self) -> &str {
        self.kernel
            .active_screen()
            .map(Id::as_str)
            .unwrap_or_default()
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .iter()
            .map(|p| format!("{}\n", p.label))
            .collect()
    }

    fn tap(&mut self, x: i32, y: i32) {
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 50;
            let event = InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                Point::new(x, y),
                PointerTool::Finger,
                1000,
            ));
            let effects = self.kernel.handle(Instant(self.now), event);
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    fn button(&mut self, label: &str) -> (i32, i32) {
        let part = self
            .kernel
            .describe()
            .into_iter()
            .find(|p| p.label == label)
            .unwrap_or_else(|| panic!("no {label}"));
        let a = part.area;
        (
            i32::from(a.x) + i32::from(a.width) / 2,
            i32::from(a.y) + i32::from(a.height) / 2,
        )
    }

    /// Whether hints show (`help.show`).
    fn hints_on(&self) -> bool {
        self.kernel.settings().read(&paperpaw_plugin_help::SHOW)
    }
}

#[test]
fn hints_show_once_over_a_screen_on_a_touch_screen() {
    let mut device = Device::new(TOUCH, "");
    assert_eq!(
        device.on_top(),
        "probe.home",
        "no hints for the home screen here"
    );
    device.open(OTHER);
    assert_eq!(device.on_top(), "help.overlay");
    let shown = device.shown();
    // In its zone, as wide as that.
    assert!(
        shown.contains(
            "Tap
here:
next"
        ),
        "{shown}"
    );
    assert!(shown.contains("For all"), "{shown}");
    assert!(!shown.contains("For buttons"), "{shown}");
    assert!(
        shown.contains("How it works here") && shown.contains("Got it"),
        "{shown}"
    );
    // A tap anywhere closes them, the screen below as it was.
    device.tap(100, 100);
    assert_eq!(device.on_top(), "probe.other");
    device.run(actions::BACK);
    device.open(OTHER);
    assert_eq!(device.on_top(), "probe.other", "once");
}

#[test]
fn no_more_hints_switches_them_off() {
    let mut device = Device::new(TOUCH, "");
    device.open(OTHER);
    device.shown();
    let (x, y) = device.button("No more hints");
    device.tap(x, y);
    assert_eq!(device.on_top(), "probe.other");
    assert!(!device.hints_on());
}

#[test]
fn with_buttons_alone_their_hints_and_their_choice() {
    let mut device = Device::new(BUTTONS, "");
    device.open(OTHER);
    let shown = device.shown();
    assert!(
        shown.contains("For buttons") && !shown.contains("Tap here"),
        "{shown}"
    );
    device.run(actions::RIGHT);
    device.run(actions::CONFIRM);
    assert_eq!(device.on_top(), "probe.other");
    assert!(!device.hints_on());

    // Switched off at first: none.
    let mut device = Device::new(BUTTONS, "[help]\nshow = false\n");
    device.open(OTHER);
    assert_eq!(device.on_top(), "probe.other");
}

/// A card that cannot be written keeps no hints as seen: closed, they
/// stay closed (they opened again over the screen each time, and nothing
/// got past them).
#[test]
fn hints_closed_on_a_card_that_cannot_be_written_stay_closed() {
    let mut device = Device::on(BUTTONS, "", paperpaw_platform::NoStorage);
    device.open(OTHER);
    assert_eq!(device.on_top(), "help.overlay");
    device.run(actions::CONFIRM);
    assert_eq!(device.on_top(), "probe.other");
    device.run(actions::BACK);
    device.open(OTHER);
    assert_eq!(device.on_top(), "probe.other");
}

#[test]
fn hints_show_again_when_asked() {
    let mut device = Device::new(BUTTONS, "");
    device.open(OTHER);
    device.run(actions::CONFIRM);
    device.run(actions::BACK);
    device.open(Id::new_static("help.again"));
    assert!(device.shown().contains("The hints show again"));
    device.run(actions::BACK);
    device.open(OTHER);
    assert_eq!(device.on_top(), "help.overlay");
}

/// Another screen opened over the hints (a plugin's question, a notice)
/// covers what they explain: they close, the screen on top stays, and
/// they show again when the screen they explain does.
#[test]
fn a_screen_opened_over_the_hints_closes_them_for_next_time() {
    let mut device = Device::new(TOUCH, "");
    device.open(OTHER);
    assert_eq!(device.on_top(), "help.overlay");
    // Another of it (the one below is gone back to, not opened).
    device.open_with(HOME, Params::new().with("n", "2"));
    assert_eq!(device.on_top(), "probe.home", "the one opened stays");
    let open: Vec<String> = device.kernel.screens().map(|id| id.to_string()).collect();
    assert!(!open.iter().any(|s| s == "help.overlay"), "{open:?}");
    // Back to the screen they explain: they show again.
    device.run(actions::BACK);
    assert_eq!(device.on_top(), "help.overlay", "shown again");
}
