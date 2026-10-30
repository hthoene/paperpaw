//! The touch lock with the real kernel: on a touch screen, taps and
//! swipes turn no page while it is on and the buttons still do; a tap
//! and a held tap unlock (as the notice says), so do two taps and a hold,
//! a hand resting does not, and the power button held
//! unlocks where it is bound; with the lock off, both do what they always
//! do.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    ButtonState, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_lock::Lock;
use paperpaw_plugin_lock::touch::{HINT_MS, HOLD_MS, LOCKED, TOGGLE};
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, MemoryStorage, Plugin, Priority, RegisterError,
    Registrar, Screen, ScreenSpec, Version,
};

const PAGES: Id = Id::new_static("test.pages");

const fn device(touch: bool) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, touch, false, &[]),
        MemoryClass::Large,
    )
}

/// The pages turned, by whichever input.
type Turned = Rc<RefCell<u32>>;

/// A home screen that counts the pages turned forward.
struct Pages(Turned);

struct PagesScreen(Turned);

impl Screen for PagesScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::NEXT_PAGE {
            *self.0.borrow_mut() += 1;
            return Handled::Yes;
        }
        // A switch of the setting, as the quick panel's and Settings' are.
        if *action == actions::CONFIRM {
            let on = cx.env().settings().read(&LOCKED);
            cx.command(paperpaw_sdk::Command::set(&LOCKED, !on));
            return Handled::Yes;
        }
        Handled::No
    }
}

impl Plugin for Pages {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("test"), "Pages", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let turned = self.0.clone();
        r.add_screen(ScreenSpec::new(
            PAGES,
            "test.pages",
            Box::new(move |_, _| Box::new(PagesScreen(turned.clone()))),
        ))?;
        r.bind_slot(slots::HOME, PAGES, Priority::PREFERRED)
    }
}

struct Test {
    kernel: Kernel,
    now: u64,
    turned: Turned,
}

impl Test {
    fn new(touch: bool) -> Self {
        let turned = Turned::default();
        let mut kernel = Kernel::builder(device(touch))
            .storage(MemoryStorage::new())
            .plugin(paperpaw_plugin_fonts_default::DefaultFonts)
            .plugin(Lock)
            // Shows the notices, and the quick panel.
            .plugin(paperpaw_plugin_menu::Menu)
            .plugin(Pages(turned.clone()))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            now: 0,
            turned,
        }
    }

    fn at(&mut self, millis: u64) -> Instant {
        self.now += millis;
        Instant(self.now)
    }

    fn turned(&self) -> u32 {
        *self.turned.borrow()
    }

    fn locked(&self) -> bool {
        self.kernel.settings().read(&LOCKED)
    }

    fn toggle(&mut self) {
        let at = self.at(200);
        self.kernel.run_action(at, TOGGLE);
    }

    fn pointer(&mut self, millis: u64, phase: PointerPhase, (x, y): (i32, i32)) {
        let at = self.at(millis);
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ));
        self.kernel.handle(at, event);
    }

    /// A tap at the right edge: the next page, where touch works.
    fn tap_right(&mut self) {
        self.pointer(200, PointerPhase::Down, (440, 400));
        self.pointer(60, PointerPhase::Up, (440, 400));
    }

    /// A swipe to the left: the next page too.
    fn swipe_left(&mut self) {
        self.pointer(200, PointerPhase::Down, (400, 400));
        self.pointer(30, PointerPhase::Move, (200, 400));
        self.pointer(30, PointerPhase::Up, (80, 400));
    }

    /// A finger held still in the middle for `millis`.
    fn hold(&mut self, millis: u64) {
        self.pointer(200, PointerPhase::Down, (240, 400));
        self.pointer(millis / 2, PointerPhase::Move, (245, 404));
        self.pointer(millis - millis / 2, PointerPhase::Up, (245, 404));
    }

    /// A quick tap in the middle, then the finger down again and kept
    /// down `millis`, not moving: tap twice, holding the second tap.
    fn tap_and_hold(&mut self, millis: u64) {
        self.pointer(200, PointerPhase::Down, (240, 400));
        self.pointer(80, PointerPhase::Up, (240, 400));
        self.pointer(300, PointerPhase::Down, (242, 401));
        self.pointer(millis, PointerPhase::Up, (242, 401));
    }

    /// Two quick taps in the middle, then the finger kept down `millis`.
    fn taps_and_hold(&mut self, millis: u64) {
        for _ in 0..2 {
            self.pointer(200, PointerPhase::Down, (240, 400));
            self.pointer(80, PointerPhase::Up, (240, 400));
        }
        self.hold(millis);
    }

    /// Whether the screen says `text` now (a notice too).
    fn says(&mut self, text: &str) -> bool {
        self.kernel.render(&mut |_, _| {});
        let labels: Vec<String> = self
            .kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        labels.join(" ").contains(text)
    }

    /// Opens the quick panel and taps its *Lock touch* switch.
    fn quick_panel_switch(&mut self) {
        let at = self.at(200);
        self.kernel.run_action(at, actions::QUICK_PANEL);
        self.kernel.render(&mut |_, _| {});
        let parts = self.kernel.describe();
        let switch = parts
            .iter()
            .find(|p| p.label.contains("Lock touch"))
            .unwrap_or_else(|| panic!("no switch in {parts:?}"));
        let (x, y) = (
            i32::from(switch.area.x) + i32::from(switch.area.width) / 2,
            i32::from(switch.area.y) + i32::from(switch.area.height) / 2,
        );
        self.pointer(200, PointerPhase::Down, (x, y));
        self.pointer(60, PointerPhase::Up, (x, y));
    }

    /// Time passes: a notice showing goes.
    fn wait(&mut self, millis: u64) {
        let at = self.at(millis);
        self.kernel.tick(at);
    }

    /// Switches the setting on a screen, as the reader does in the quick
    /// panel and in Settings.
    fn switch(&mut self) {
        let at = self.at(200);
        self.kernel.run_action(at, actions::CONFIRM);
    }

    fn button(&mut self, name: &'static str, held: u64) {
        let at = self.at(200);
        self.kernel.handle(
            at,
            InputEvent::Button {
                name,
                state: ButtonState::Pressed,
            },
        );
        // Held: the kernel's beat sees it long meanwhile.
        let at = self.at(held);
        self.kernel.tick(at);
        self.kernel.handle(
            at,
            InputEvent::Button {
                name,
                state: ButtonState::Released,
            },
        );
    }
}

#[test]
fn locked_touch_turns_no_page_but_the_buttons_do() {
    let mut t = Test::new(true);
    assert!(!t.locked(), "off by default");
    t.tap_right();
    assert_eq!(t.turned(), 1, "a tap turns the page while unlocked");
    t.toggle();
    assert!(t.locked());
    t.tap_right();
    t.swipe_left();
    assert_eq!(t.turned(), 1, "taps and swipes do nothing while locked");
    t.button("page-forward", 50);
    assert_eq!(t.turned(), 2, "the buttons still work");
    t.toggle();
    t.tap_right();
    assert_eq!(t.turned(), 3, "unlocked, a tap works again");
}

/// The way out as the notice tells it: down, up, down, held past
/// HOLD_MS, then lifted.
#[test]
fn a_tap_and_a_held_tap_unlock() {
    let mut t = Test::new(true);
    t.toggle();
    t.tap_and_hold(HOLD_MS / 2);
    assert!(t.locked(), "held too short");
    t.wait(2_000);
    t.tap_and_hold(HOLD_MS + 100);
    assert!(!t.locked(), "a tap and a held tap unlock");
    assert_eq!(t.turned(), 0, "and turn no page");
}

#[test]
fn two_taps_and_a_hold_unlock_a_resting_hand_does_not() {
    let mut t = Test::new(true);
    t.toggle();
    t.hold(10_000);
    assert!(t.locked(), "a hand resting, then lifted, does not unlock");
    t.taps_and_hold(HOLD_MS / 2);
    assert!(t.locked(), "a short hold after the taps does not");
    t.taps_and_hold(HOLD_MS + 100);
    assert!(!t.locked(), "two taps and a hold unlock");
    assert_eq!(t.turned(), 0, "and turn no page");
}

#[test]
fn the_power_button_held_unlocks_only_while_locked() {
    let mut t = Test::new(true);
    t.toggle();
    t.button("power", 1500);
    assert!(!t.locked(), "the power button held unlocks");
    t.button("power", 1500);
    assert!(!t.locked(), "and does not lock again");
}

#[test]
fn with_buttons_only_nothing_changes() {
    let mut t = Test::new(false);
    t.button("page-forward", 50);
    assert_eq!(t.turned(), 1);
    assert!(!t.locked());
}

const LOCKED_TEXT: &str = "Touch is locked. To unlock, tap twice";
const UNLOCKED_TEXT: &str = "Touch is unlocked.";

/// However touch is locked (Settings or the quick panel, which set the
/// setting; the toggle, from a button or a rule), the reader is told how
/// to unlock it, and told when it is unlocked.
#[test]
fn locking_touch_says_how_to_unlock_however_it_is_switched() {
    let mut t = Test::new(true);
    t.switch();
    assert!(t.locked());
    assert!(
        t.says(LOCKED_TEXT),
        "switched as in Settings and the quick panel"
    );
    t.wait(10_000);
    assert!(!t.says(LOCKED_TEXT));
    t.switch();
    assert!(t.says(UNLOCKED_TEXT));
    t.wait(10_000);
    t.quick_panel_switch();
    assert!(t.locked());
    assert!(t.says(LOCKED_TEXT), "the quick panel's switch");
    t.wait(10_000);
    t.toggle();
    assert!(t.says(UNLOCKED_TEXT));
    t.wait(10_000);
    t.toggle();
    assert!(t.says(LOCKED_TEXT), "the toggle");
    t.wait(10_000);
    t.toggle();
    assert!(t.says(UNLOCKED_TEXT));
}

/// While touch is locked, taps and swipes say how to unlock, now and
/// then (not at every touch, never between the way out's steps), and
/// still turn no page; the way out works as before.
#[test]
fn a_touch_while_locked_says_how_to_unlock_now_and_then() {
    let mut t = Test::new(true);
    t.toggle();
    t.wait(HINT_MS);
    assert!(!t.says(LOCKED_TEXT));
    t.tap_right();
    assert!(!t.says(LOCKED_TEXT), "a first tap may begin the way out");
    t.wait(2_000);
    t.tap_right();
    assert!(
        t.says(LOCKED_TEXT),
        "a tap a while later says how to unlock"
    );
    t.wait(5_000);
    assert!(!t.says(LOCKED_TEXT));
    t.swipe_left();
    assert!(!t.says(LOCKED_TEXT), "not again so soon");
    t.wait(HINT_MS);
    t.swipe_left();
    assert!(t.says(LOCKED_TEXT), "a swipe, a while later, says it again");
    assert_eq!(t.turned(), 0);
    t.wait(HINT_MS);
    t.taps_and_hold(HOLD_MS + 100);
    assert!(!t.locked(), "the way out still unlocks");
    assert!(t.says(UNLOCKED_TEXT));
}

/// The X4 Pro's home key (below the screen) held unlocks too.
#[test]
fn the_home_key_held_unlocks() {
    let mut t = Test::new(true);
    t.toggle();
    t.button("home", 1_500);
    assert!(!t.locked());
}
