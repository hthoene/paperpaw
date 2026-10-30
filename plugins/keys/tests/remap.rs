//! Changing what buttons and gestures do, with the real kernel, on a
//! device with buttons only (like the X4) and on a touch screen: a
//! binding chosen on the screens is written to `paperpaw.ini` under
//! `[keymap.<layer>]` and takes effect at once; *As standard* removes it
//! again, *Nothing* switches an input off, a place other than everywhere
//! writes its own layer and wins there, the same input bound twice keeps
//! the last, and the search finds actions by their names.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    ButtonState, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_plugin_keys::Keys;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::{self, Part, Role};
use paperpaw_sdk::{
    ActionSpec, Commands, Env, Id, Manifest, MemoryStorage, Plugin, RegisterError, Registrar,
    Version,
};

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

/// Counts how often it runs.
const COUNT: Id = Id::new_static("counter.count");

struct Counter(Rc<Cell<u32>>);

impl Plugin for Counter {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("counter"), "Counter", Version::new(0, 1, 0))
            .description("Counts.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let count = self.0.clone();
        r.add_action(
            ActionSpec::new(COUNT, "Count things")
                .handler(move |_: &Env<'_>, _, _: &mut Commands| count.set(count.get() + 1)),
        )
    }
}

struct Device {
    kernel: Kernel,
    now: u64,
    count: Rc<Cell<u32>>,
}

impl Device {
    fn new(device: DeviceInfo) -> Self {
        Self::with_config(device, "")
    }

    fn with_config(device: DeviceInfo, config: &str) -> Self {
        let count = Rc::new(Cell::new(0));
        let kernel = Kernel::builder(device)
            .storage(MemoryStorage::new())
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Keyboard)
            .plugin(Keys)
            .plugin(Counter(count.clone()))
            .build()
            .unwrap();
        let mut device = Self {
            kernel,
            now: 0,
            count,
        };
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

    /// The user's configuration file as it is now.
    fn config(&self) -> String {
        let data = paperpaw_sdk::read_file(self.kernel.storage(), "paperpaw.ini", 1 << 16)
            .unwrap_or_default();
        String::from_utf8(data).unwrap()
    }

    /// The lines of section `[section]` of the configuration file.
    fn section(&self, section: &str) -> Vec<String> {
        let config = self.config();
        let header = format!("[{section}]");
        config
            .lines()
            .skip_while(|l| l.trim() != header)
            .skip(1)
            .take_while(|l| !l.starts_with('['))
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .collect()
    }

    /// Opens the list of inputs.
    fn open(&mut self) {
        self.run(Id::new_static("keys.open"));
        assert_eq!(self.on_top(), Some("keys.list"));
    }

    fn touch(&self) -> bool {
        self.kernel.device().input.touch
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

    fn tap_at(&mut self, at: Point) {
        self.now += 1000;
        self.finger(PointerPhase::Down, at);
        self.finger(PointerPhase::Up, at);
    }

    fn center(part: &Part) -> Point {
        Point::new(
            i32::from(part.area.x) + i32::from(part.area.width) / 2,
            i32::from(part.area.y) + i32::from(part.area.height) / 2,
        )
    }

    /// The row whose text begins with `start` on the page shown.
    fn row(&mut self, start: &str) -> Option<Part> {
        self.parts()
            .into_iter()
            .find(|p| matches!(p.role, Role::Row | Role::Button) && p.label.starts_with(start))
    }

    /// Chooses the row whose text begins with `start`: by touch, turning
    /// pages until it shows; by buttons, moving the focus down to it.
    fn choose(&mut self, start: &str) {
        for _ in 0..400 {
            if self.touch() {
                if let Some(row) = self.row(start) {
                    let at = Self::center(&row);
                    return self.tap_at(at);
                }
                self.run(actions::NEXT_PAGE);
            } else {
                let parts = self.parts();
                let focused = semantics::focused(&parts).map(|p| p.label.clone());
                if focused.is_some_and(|f| f.starts_with(start)) {
                    return self.run(actions::CONFIRM);
                }
                self.run(actions::DOWN);
            }
        }
        panic!("no row {start:?}");
    }

    /// What the row of input `name` says it does, looked up from the
    /// list's start.
    fn does(&mut self, name: &str) -> String {
        self.close();
        self.open();
        let prefix = format!("{name}, ");
        for _ in 0..40 {
            if let Some(row) = self.row(&prefix) {
                return row.label[prefix.len()..].to_string();
            }
            self.run(actions::NEXT_PAGE);
        }
        panic!("no input {name:?}");
    }

    /// Binds input `name` (as the list says it) to the action whose name
    /// begins with `action`.
    fn bind(&mut self, name: &str, action: &str) {
        self.choose(&format!("{name}, "));
        assert_eq!(self.on_top(), Some("keys.pick"));
        self.choose(action);
    }

    /// A press of button `name`: short, or held a second and a half.
    fn button(&mut self, name: &'static str, long: bool) {
        self.now += 1000;
        let pressed = InputEvent::Button {
            name,
            state: ButtonState::Pressed,
        };
        // Timers wait while the button is down: a long press is not
        // meant unless asked for.
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

    /// Leaves the screens of this plugin.
    fn close(&mut self) {
        for _ in 0..4 {
            if !self.on_top().is_some_and(|s| s.starts_with("keys.")) {
                return;
            }
            self.run(actions::BACK);
        }
    }
}

#[test]
fn a_held_button_remapped_by_buttons_is_saved_and_works() {
    let mut device = Device::new(BUTTONS);
    device.open();
    device.bind("Hold: Back", "Count things");
    assert_eq!(device.on_top(), Some("keys.list"));
    assert_eq!(
        device.section("keymap.global"),
        ["long.back = counter.count"]
    );
    // The list says it.
    assert_eq!(device.does("Hold: Back"), "Count things");
    device.close();
    device.button("back", true);
    assert_eq!(device.count.get(), 1);
    // A short press does not.
    device.button("back", false);
    assert_eq!(device.count.get(), 1);
}

#[test]
fn a_tap_zone_remapped_by_touch_with_the_search_is_saved_and_works() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.bind("Tap: centre", "Search the actions");
    // The search asks the keyboard; only what matches is listed.
    assert_eq!(device.on_top(), Some("keyboard.screen"));
    for key in ["c", "o", "u", "n", "t", "OK"] {
        let key = device
            .parts()
            .into_iter()
            .find(|p| p.role == Role::Button && p.label == key)
            .unwrap();
        device.tap_at(Device::center(&key));
    }
    assert_eq!(device.on_top(), Some("keys.pick"));
    assert!(device.row("As standard").is_none());
    assert!(device.row("Count things").is_some());
    device.choose("Count things");
    assert_eq!(device.on_top(), Some("keys.list"));
    assert_eq!(
        device.section("keymap.global"),
        ["tap.center = counter.count"]
    );
    device.close();
    device.tap_at(Point::new(240, 400));
    assert_eq!(device.count.get(), 1);
}

#[test]
fn as_standard_removes_the_binding_again() {
    let mut device = Device::new(BUTTONS);
    device.open();
    device.bind("Hold: Back", "Count things");
    device.bind("Hold: Back", "As standard");
    assert!(device.section("keymap.global").is_empty());
    assert_eq!(device.does("Hold: Back"), "Home");
    device.close();
    device.button("back", true);
    assert_eq!(device.count.get(), 0);
}

/// A binding made on the keys page replaces one of the same input from
/// the shell's configuration (`--config`, the device's own file):
/// "Nothing" switches the button off at once.
#[test]
fn nothing_switches_an_input_off() {
    let mut device = Device::with_config(BUTTONS, "[keymap.global]\nbutton.left = counter.count\n");
    device.button("left", false);
    assert_eq!(device.count.get(), 1);
    device.open();
    device.bind("Left", "Nothing");
    assert_eq!(
        device.section("keymap.global"),
        ["button.left = core.nothing"]
    );
    device.close();
    device.button("left", false);
    assert_eq!(device.count.get(), 1);
}

#[test]
fn the_same_input_bound_twice_keeps_the_last() {
    let mut device = Device::new(BUTTONS);
    device.open();
    device.bind("Right", "Count things");
    device.bind("Right", "Home");
    assert_eq!(
        device.section("keymap.global"),
        ["button.right = core.home"]
    );
    // Two inputs to one action: both do it.
    device.bind("Right", "Count things");
    device.bind("Left", "Count things");
    let mut lines = device.section("keymap.global");
    lines.sort();
    assert_eq!(
        lines,
        [
            "button.left = counter.count",
            "button.right = counter.count"
        ]
    );
    device.close();
    device.button("left", false);
    device.button("right", false);
    assert_eq!(device.count.get(), 2);
}

#[test]
fn a_place_writes_its_own_layer_which_applies_only_there() {
    let mut device = Device::new(BUTTONS);
    device.open();
    device.choose("Applies");
    assert_eq!(device.on_top(), Some("keys.place"));
    device.choose("While reading");
    assert_eq!(device.on_top(), Some("keys.list"));
    assert!(device.row("Applies, While reading").is_some());
    device.bind("Hold: Back", "Count things");
    assert_eq!(
        device.section("keymap.core.reader"),
        ["long.back = counter.count"]
    );
    assert!(device.section("keymap.global").is_empty());
    // Not here, where the global binding applies.
    device.close();
    device.button("back", true);
    assert_eq!(device.count.get(), 0);
}

#[test]
fn gestures_of_a_place_switch_round_on_touch_screens() {
    let mut device = Device::new(TOUCH);
    device.open();
    // Everywhere: no choice of how gestures are taken.
    assert!(device.row("Gestures here").is_none());
    device.choose("Applies");
    device.choose("In the library");
    let policy = |device: &mut Device| device.row("Gestures here").unwrap().label;
    assert!(policy(&mut device).contains("As standard"));
    device.choose("Gestures here");
    assert!(policy(&mut device).contains("The tool's own"));
    assert_eq!(
        device.section("keymap.core.library"),
        ["gestures.policy = core.gestures.own"]
    );
    device.choose("Gestures here");
    assert!(policy(&mut device).contains("Off"));
    device.choose("Gestures here");
    assert!(policy(&mut device).contains("As standard"));
    assert_eq!(
        device.section("keymap.core.library"),
        ["gestures.policy = core.gestures.standard"]
    );
}

#[test]
fn only_the_device_buttons_are_listed() {
    let mut device = Device::new(BUTTONS);
    device.open();
    let mut names = Vec::new();
    for _ in 0..8 {
        names.extend(
            device
                .parts()
                .into_iter()
                .filter(|p| p.role == Role::Row)
                .map(|p| p.label),
        );
        device.run(actions::NEXT_PAGE);
    }
    assert!(names.iter().any(|n| n.starts_with("Select, ")));
    assert!(names.iter().any(|n| n.starts_with("Hold: Down, ")));
    // The device has no page buttons nor menu: they are not offered.
    assert!(!names.iter().any(|n| n.starts_with("Page forward")));
    assert!(!names.iter().any(|n| n.starts_with("Menu")));
    // No gestures without touch.
    assert!(!names.iter().any(|n| n.starts_with("Tap: ")));
}

#[test]
fn a_binding_to_an_unknown_action_shows_its_id() {
    let mut device = Device::with_config(
        BUTTONS,
        "[keymap.global]\nbutton.left = gone.plugin-action\n",
    );
    device.open();
    assert_eq!(device.does("Left"), "gone.plugin-action");
    device.close();
    // Pressing it does nothing harmful.
    device.button("left", false);
    assert_eq!(device.count.get(), 0);
}

#[test]
fn a_binding_for_one_screen_wins_on_that_screen_only() {
    let mut device = Device::new(BUTTONS);
    device.open();
    device.bind("Hold: Up", "Home");
    // Any screen can be the place, also one not listed first.
    device.choose("Applies");
    device.choose("Keyboard layouts");
    assert!(device.row("Applies, Keyboard layouts").is_some());
    device.bind("Hold: Up", "Count things");
    assert_eq!(device.section("keymap.global"), ["long.up = core.home"]);
    assert_eq!(
        device.section("keymap.keyboard.choose"),
        ["long.up = counter.count"]
    );
    device.close();
    // Elsewhere the global binding.
    device.button("up", true);
    assert_eq!(device.count.get(), 0);
    // On that screen its own.
    device.now += 100;
    let effects = device.kernel.open_screen(
        Instant(device.now),
        Id::new_static("keyboard.choose"),
        paperpaw_sdk::Params::new(),
    );
    device.settle(effects);
    device.button("up", true);
    assert_eq!(device.count.get(), 1);
}
