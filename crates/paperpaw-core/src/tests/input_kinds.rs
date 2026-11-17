//! Inputs of more kinds: double presses and taps, taps of
//! two fingers, a layer's own zones, inputs raised by plugins, key map
//! layers put in front, guards.

use super::*;
use crate::Items;
use paperpaw_sdk::{Gesture, GestureKind, Params};

/// A kernel showing plugin `a`'s screen, with its key map `keymap`.
fn kernel_with(log: &Log, keymap: &'static str) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin {
            keymap: Some(keymap),
            ..TestPlugin::new("a", log)
        })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

/// Finger `id` going down or lifted at `at`.
fn finger(kernel: &mut Kernel, at: u64, id: u8, phase: PointerPhase, (x, y): (i32, i32)) {
    let event = PointerEvent::new(id, phase, Point::new(x, y), PointerTool::Finger, 1000);
    kernel.handle(Instant(at), InputEvent::Pointer(event));
}

/// A press bound twice waits for a second: two make the double press,
/// one alone is a press once the time for a second is over.
#[test]
fn a_button_pressed_twice_is_a_double_press() {
    let log = log();
    let mut kernel = kernel_with(&log, "[keymap.a.screen]\ndouble.confirm = a.special\n");
    press(&mut kernel, 1000, "confirm");
    assert!(log.borrow().is_empty(), "the first press waits");
    press(&mut kernel, 1150, "confirm");
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    let effects = press(&mut kernel, 3000, "confirm");
    assert!(log.borrow().is_empty());
    // Held for a long press, it counts as it is let go.
    let wake = effects.wake_at.unwrap();
    assert!(wake <= Instant(3350), "{wake:?}");
    kernel.tick(wake);
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
    // Another button meanwhile: the press waiting comes first.
    log.borrow_mut().clear();
    press(&mut kernel, 5000, "confirm");
    press(&mut kernel, 5100, "back");
    assert_eq!(log.borrow()[0], "a:core.confirm");
}

/// Two taps close together: a double tap where one is bound, else two
/// taps at once, so that pages turn as fast as they are tapped.
#[test]
fn two_quick_taps_are_a_double_tap_where_one_is_bound() {
    let log = log();
    let keymap = "[keymap.a.screen]\ndouble-tap.center = a.special\ntap.center = core.confirm\ntap.left = core.confirm\n";
    let mut kernel = kernel_with(&log, keymap);
    tap(&mut kernel, 1000, 45, 45);
    assert!(log.borrow().is_empty(), "the first tap waits");
    tap(&mut kernel, 1150, 46, 44);
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    tap(&mut kernel, 3000, 5, 45);
    tap(&mut kernel, 3150, 5, 45);
    assert_eq!(*log.borrow(), ["a:core.confirm", "a:core.confirm"]);
    log.borrow_mut().clear();
    tap(&mut kernel, 5000, 45, 45);
    kernel.tick(Instant(5500));
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
}

/// Logs the gestures it gets.
struct Taps {
    log: Log,
}

impl crate::testing::Fake for Taps {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        let taken = gesture.kind == GestureKind::Tap;
        if taken {
            let kind = alloc::format!("{:?}", gesture.kind);
            self.log.borrow_mut().push(kind);
        }
        if taken { Handled::Yes } else { Handled::No }
    }
}

fn taps_plugin(kept0: Log) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let log = kept0.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("taps.screen"),
            "taps",
            Box::new(move |_, _| Box::new(Taps { log: log.clone() })),
        ))?;
        r.bind_slot(
            slots::HOME,
            Id::new_static("taps.screen"),
            Priority::DEFAULT,
        )
    }
}

/// What [`taps_plugin`] registers under.
fn taps_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("taps"), "Taps", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// Two quick taps reach a screen as two taps.
#[test]
fn screens_get_two_quick_taps_as_taps() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(taps_plugin_manifest(), taps_plugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    tap(&mut kernel, 1000, 45, 45);
    tap(&mut kernel, 1150, 45, 45);
    assert_eq!(*log.borrow(), ["Tap", "Tap"]);
}

/// Two fingers tapped together run `tap2`; taps in a layer's own zone
/// run what it binds there, before the grid's.
#[test]
fn two_fingers_and_own_zones() {
    let log = log();
    let keymap = "[zones.a.screen]\ncorner = 80 0 20 20\n[keymap.a.screen]\ntap2 = a.special\ntap.corner = core.confirm\n";
    let mut kernel = kernel_with(&log, keymap);
    finger(&mut kernel, 1000, 1, PointerPhase::Down, (30, 45));
    finger(&mut kernel, 1010, 2, PointerPhase::Down, (60, 45));
    finger(&mut kernel, 1080, 1, PointerPhase::Up, (30, 45));
    finger(&mut kernel, 1090, 2, PointerPhase::Up, (60, 45));
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    tap(&mut kernel, 2000, 85, 5);
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
    // Outside the corner, the grid's: the top right turns the page.
    log.borrow_mut().clear();
    tap(&mut kernel, 4000, 85, 25);
    assert_eq!(*log.borrow(), ["a:core.next-page"]);
}

/// A screen that lays a key map layer for itself (`lay.on`, `lay.off`)
/// or tries one for the screen below it (`lay.other`); it logs the
/// actions it gets.
struct Layered(Log);

impl crate::testing::Fake for Layered {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.borrow_mut().push(alloc::format!("lay:{action}"));
        let book = Id::new_static("book.mine");
        match action.as_str() {
            "lay.on" => cx.set_key_layer(Some(book)),
            "lay.off" => cx.set_key_layer(None),
            "lay.other" => {
                let below = cx.env().stack()[0].key;
                cx.command(Command::SetKeyLayer(below, Some(book)));
            }
            _ if *action == actions::CONFIRM => {}
            _ => return Handled::No,
        }
        Handled::Yes
    }
}

fn layer_plugin(kept: Log) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("lay.screen"),
            "layered",
            Box::new(move |_, _| Box::new(Layered(log.clone()))),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("lay.screen"), Priority(20))?;
        for action in ["lay.on", "lay.off", "lay.other", "lay.hit"] {
            r.add_action(ActionSpec::new(Id::parse(action).unwrap(), action))?;
        }
        r.add_keymap(Cow::Borrowed(
            "[keymap.book.mine]\nbutton.confirm = lay.hit\n",
        ))
    }
}

/// What [`layer_plugin`] registers under.
fn layer_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("lay"), "Layers", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn layered(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(layer_plugin_manifest(), layer_plugin(log.clone()))
        .test_plugin(TestPlugin::new("b", log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    kernel
}

/// Inputs raised by command run what they are bound to; a layer a screen
/// lays for itself binds before the slot's and the global one. A layer
/// asked for by command, or by a screen for another, is not laid.
#[test]
fn inputs_raised_and_layers_put_in_front() {
    let log = log();
    let mut kernel = layered(&log);
    kernel.apply(Command::Input(String::from("button.confirm")));
    kernel.finish();
    assert_eq!(*log.borrow(), ["lay:core.confirm"]);
    log.borrow_mut().clear();
    kernel.run_action(Instant(10), Id::new_static("lay.on"));
    kernel.apply(Command::Input(String::from("button.confirm")));
    kernel.finish();
    assert_eq!(*log.borrow(), ["lay:lay.on", "lay:lay.hit"]);
    log.borrow_mut().clear();
    kernel.run_action(Instant(20), Id::new_static("lay.off"));
    press(&mut kernel, 2000, "confirm");
    assert_eq!(*log.borrow(), ["lay:lay.off", "lay:core.confirm"]);
    // By command, also with the screen's own key: not laid.
    let key = paperpaw_sdk::ScreenKey::new(kernel.stack[0].serial);
    kernel.apply(Command::SetKeyLayer(key, Some(Id::new_static("book.mine"))));
    kernel.finish();
    assert_eq!(kernel.stack[0].key_layer, None);
    // By a screen for the one below it: not laid.
    kernel.open_screen(
        Instant(30),
        Id::new_static("lay.screen"),
        Params::new().with("n", "2"),
    );
    kernel.run_action(Instant(40), Id::new_static("lay.other"));
    assert_eq!(kernel.stack[0].key_layer, None);
    kernel.apply(Command::Input(String::from("no.such-input")));
    kernel.finish();
    let warnings = kernel.take_warnings();
    assert!(warnings.iter().any(|w| w.message.contains("no.such-input")));
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("laid by its screen"))
    );
}

/// A lock of its own plugin: it guards the device and logs the actions
/// it gets.
struct Lock(Log);

impl crate::testing::Fake for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn flags(&self) -> u32 {
        crate::testing::GUARDS
    }

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.0.borrow_mut().push(alloc::format!("lock:{action}"));
        Handled::Yes
    }
}

fn lock_plugin(kept: Log) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("lock.screen"),
            "lock",
            Box::new(move |_, _| Box::new(Lock(log.clone()))),
        ))
    }
}

/// What [`lock_plugin`] registers under.
fn lock_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("lock"), "Lock", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn lock_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", log))
        .items(lock_plugin_manifest(), lock_plugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

/// While a screen of another plugin guards the device, an input raised by
/// command does not reach it; the guard's own presses do.
#[test]
fn an_input_raised_by_another_plugin_does_not_reach_a_guard() {
    let log = log();
    let mut kernel = lock_kernel(&log);
    kernel.open_screen(Instant(10), Id::new_static("lock.screen"), Params::new());
    // An input raised by another plugin does not reach the lock: what is
    // typed into it is the reader's alone.
    log.borrow_mut().clear();
    kernel.apply(Command::Input(String::from("button.menu")));
    kernel.finish();
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    press(&mut kernel, 2000, "menu");
    assert_eq!(*log.borrow(), ["lock:core.menu"]);
}

/// A layer put in front is the screen's that laid it: it applies while
/// that screen is on top, not over another, and goes with it.
#[test]
fn a_key_layer_lives_with_its_screen() {
    let log = log();
    let mut kernel = layered(&log);
    kernel.run_action(Instant(5), Id::new_static("lay.on"));
    kernel.open_screen(Instant(10), Id::new_static("b.screen"), Params::new());
    log.borrow_mut().clear();
    press(&mut kernel, 1000, "confirm");
    assert_eq!(*log.borrow(), ["b:core.confirm"]);
    let last = kernel.stack.len() - 1;
    kernel.close_from(last);
    log.borrow_mut().clear();
    press(&mut kernel, 2000, "confirm");
    assert_eq!(*log.borrow(), ["lay:lay.hit"]);
    // Made anew, the screen has no layer.
    kernel.close_from(0);
    kernel.open_screen(Instant(3000), Id::new_static("lay.screen"), Params::new());
    log.borrow_mut().clear();
    press(&mut kernel, 4000, "confirm");
    assert_eq!(*log.borrow(), ["lay:core.confirm"]);
}

/// A press waiting for a second is resolved for the screen it began on,
/// also when another came up meanwhile; nothing where that one closed.
#[test]
fn a_waiting_press_keeps_its_screen() {
    let log = log();
    let keymap = "[keymap.a.screen]\ndouble.confirm = core.home\nbutton.confirm = a.special\n";
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin {
            keymap: Some(keymap),
            priority: Priority(10),
            ..TestPlugin::new("a", &log)
        })
        .test_plugin(TestPlugin::new("b", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, 1000, "confirm");
    kernel.open_screen(Instant(1100), Id::new_static("b.screen"), Params::new());
    log.borrow_mut().clear();
    kernel.tick(Instant(1400));
    assert_eq!(*log.borrow(), ["b:a.special"], "a.screen's binding");
    log.borrow_mut().clear();
    // Pressed on b: b's binding. Pressed on a, a closed meanwhile:
    // nothing.
    press(&mut kernel, 2000, "confirm");
    kernel.tick(Instant(2400));
    assert_eq!(*log.borrow(), ["b:core.confirm"]);
    let top = kernel.stack.len() - 1;
    kernel.close_from(top);
    log.borrow_mut().clear();
    press(&mut kernel, 3000, "confirm");
    kernel.open_screen(Instant(3100), Id::new_static("b.screen"), Params::new());
    kernel.close_from(0);
    kernel.tick(Instant(3400));
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

/// A layer is not laid below a screen that guards the device.
#[test]
fn no_key_layer_is_laid_below_a_guard() {
    let log = log();
    let mut kernel = lock_kernel(&log);
    kernel.open_screen(Instant(1010), Id::new_static("lock.screen"), Params::new());
    let below = paperpaw_sdk::ScreenKey::new(kernel.stack[0].serial);
    kernel.apply(Command::SetKeyLayer(
        below,
        Some(Id::new_static("book.mine")),
    ));
    kernel.finish();
    assert_eq!(kernel.stack[0].key_layer, None);
}

struct WayScreen(Log);

impl crate::testing::Fake for WayScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.0.borrow_mut().push(alloc::format!("{action}"));
        Handled::Yes
    }
}

/// A screen bound to Home; it logs the actions it gets.
fn way_plugin(kept0: Log) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let log = kept0.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("way.screen"),
            "way",
            Box::new(move |_, _| Box::new(WayScreen(log.clone()))),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("way.screen"), Priority(20))?;
        for action in ["way.left", "way.right"] {
            r.add_action(ActionSpec::new(Id::parse(action).unwrap(), action))?;
        }
        r.add_keymap(Cow::Borrowed(
            "[keymap.way.screen]\ntap.left = way.left\ntap.right = way.right\n\
             swipe.left = way.left\nswipe.right = way.right\n",
        ))
    }
}

/// What [`way_plugin`] registers under.
fn way_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("way"), "Ways", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// What a tap at the right and a swipe to the left run on a screen in the
/// interface of `language` (`ar` goes right to left).
fn right_side(language: &str) -> Vec<String> {
    let log = log();
    let arabic =
        "[meta]\nlanguage = ar\nname = العربية\ndirection = rtl\n\n[messages]\nway.x = س\n";
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("plugins/arabic/plugin.ini", "[plugin]\nname = Arabic\n")
        .with("plugins/arabic/i18n/ar.ini", arabic);
    let mut kernel = Kernel::builder(DEVICE)
        .items(way_plugin_manifest(), way_plugin(log.clone()))
        .storage(storage)
        .user_config(alloc::format!("[core]\nlanguage = {language}\n"))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    log.borrow_mut().clear();
    tap(&mut kernel, 1000, 80, 45);
    swipe(&mut kernel, 3000, (60, 45), (20, 45));
    log.take()
}

/// Screens go as the interface does: in Arabic, a tap at the right and a
/// swipe to the left run what the left side binds.
#[test]
fn screens_follow_the_interfaces_direction() {
    assert_eq!(right_side("en"), ["way.right", "way.left"]);
    assert_eq!(right_side("ar"), ["way.left", "way.right"]);
}

/// A pen above the screen (reserved) reaches no screen, and a tilted pen
/// is a pen like another.
#[test]
fn a_hovering_pen_reaches_no_screen() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            crate::tests::edge_holder_plugin_manifest(),
            crate::tests::edge_holder_plugin(log.clone(), true),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let pen = |phase| {
        InputEvent::Pointer(
            PointerEvent::new(0, phase, Point::new(45, 45), PointerTool::Pen, 500)
                .with_tilt(30, -120),
        )
    };
    let effects = kernel.handle(Instant(100), pen(PointerPhase::Hover));
    assert_eq!(effects.refresh(), None);
    assert!(log.borrow().is_empty());
    kernel.handle(Instant(200), pen(PointerPhase::Down));
    assert_eq!(*log.borrow(), ["holder:down"]);
    let InputEvent::Pointer(tilted) = pen(PointerPhase::Down) else {
        unreachable!()
    };
    assert_eq!((tilted.tilt_x, tilted.tilt_y), (30, -90));
}
