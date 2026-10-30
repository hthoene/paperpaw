//! Inputs of more kinds: double presses and taps, taps of
//! two fingers, a layer's own zones, inputs raised by plugins, key map
//! layers put in front, filters that cancel or replace, plugins' own
//! recognizers.

use super::*;
use crate::gestures;
use paperpaw_sdk::keys::{
    GestureRecognizer, GestureSpec, InputMode, InputSpec, PointerVerdict, Recognition,
};
use paperpaw_sdk::{Gesture, GestureKind, Params};

/// A kernel showing plugin `a`'s screen, with its key map `keymap`.
fn kernel_with(log: &Log, keymap: &'static str) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin {
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

/// Logs the gestures it gets; takes double taps where told to (`None`:
/// says it takes none).
struct Taps {
    log: Log,
    doubles: Option<bool>,
}

impl Screen for Taps {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn takes_double_taps(&self) -> bool {
        self.doubles.is_some()
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        let taken = match gesture.kind {
            GestureKind::Tap => true,
            GestureKind::DoubleTap => self.doubles == Some(true),
            _ => false,
        };
        if taken {
            let kind = alloc::format!("{:?}", gesture.kind);
            self.log.borrow_mut().push(kind);
        }
        if taken { Handled::Yes } else { Handled::No }
    }
}

struct TapsPlugin(Log, Option<bool>);

impl Plugin for TapsPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("taps"), "Taps", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (log, doubles) = (self.0.clone(), self.1);
        r.add_screen(ScreenSpec::new(
            Id::new_static("taps.screen"),
            "taps",
            Box::new(move |_, _| {
                Box::new(Taps {
                    log: log.clone(),
                    doubles,
                })
            }),
        ))?;
        r.bind_slot(
            slots::HOME,
            Id::new_static("taps.screen"),
            Priority::DEFAULT,
        )
    }
}

/// A screen that takes double taps gets the second tap as one, as a tap
/// where it does not take it; other screens get taps.
#[test]
fn screens_get_double_taps_or_taps() {
    let cases = [
        (Some(true), ["Tap", "DoubleTap"]),
        (Some(false), ["Tap", "Tap"]),
        (None, ["Tap", "Tap"]),
    ];
    for (doubles, seen) in cases {
        let log = log();
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(TapsPlugin(log.clone(), doubles))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        tap(&mut kernel, 1000, 45, 45);
        tap(&mut kernel, 1150, 45, 45);
        assert_eq!(*log.borrow(), seen);
    }
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

impl Screen for Layered {
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

struct LayerPlugin(Log);

impl Plugin for LayerPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("lay"), "Layers", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
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

fn layered(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(LayerPlugin(log.clone()))
        .plugin(TestPlugin::new("b", log))
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

/// Cancels the back button and the second finger; makes the menu button
/// confirm.
struct Filters;

impl Plugin for Filters {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("filters"), "Filters", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        use paperpaw_sdk::filters::{InputFilterSpec, InputVerdict};
        r.add_input_filter(InputFilterSpec::new(
            Id::new_static("filters.input"),
            "",
            Box::new(|_, event| match *event {
                InputEvent::Button { name: "back", .. } => InputVerdict::Cancel,
                InputEvent::Button {
                    name: "menu",
                    state,
                } => InputVerdict::Replace(InputEvent::Button {
                    name: "confirm",
                    state,
                }),
                InputEvent::Pointer(p) if p.id == 2 && p.phase == PointerPhase::Move => {
                    InputVerdict::Cancel
                }
                _ => InputVerdict::Pass,
            }),
        ))
    }
}

/// A filter cancels an input: nothing comes of it, nothing waits for a
/// long press. It replaces one: the other runs. What the reader used last
/// is known.
#[test]
fn filters_cancel_and_replace_inputs() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .plugin(Filters)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, 1000, "back");
    press(&mut kernel, 1200, "menu");
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
    assert_eq!(kernel.shared.last_input, InputMode::Buttons);
    // A finger held, cancelled as it moves: no long press comes of it.
    log.borrow_mut().clear();
    finger(&mut kernel, 2000, 2, PointerPhase::Down, (45, 45));
    finger(&mut kernel, 2100, 2, PointerPhase::Move, (45, 46));
    kernel.tick(Instant(4000));
    finger(&mut kernel, 4100, 2, PointerPhase::Up, (45, 46));
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    assert_eq!(kernel.shared.last_input, InputMode::Touch);
}

/// Recognizes a flick off the right side, and a finger held a while
/// (before the kernel's long press); a finger coming down at the far left
/// is its own.
#[derive(Default)]
struct Flick {
    held_until: Option<Instant>,
}

impl GestureRecognizer for Flick {
    fn pointer(&mut self, event: &PointerEvent, now: Instant) -> PointerVerdict {
        match event.phase {
            PointerPhase::Down => {
                self.held_until = Some(now.add_millis(300));
                if event.point.x < 40 {
                    PointerVerdict::Capture
                } else {
                    PointerVerdict::Observe
                }
            }
            PointerPhase::Move => {
                self.held_until = None;
                PointerVerdict::Observe
            }
            PointerPhase::Up if event.point.x > 80 => {
                self.held_until = None;
                PointerVerdict::Recognized
            }
            _ => {
                self.held_until = None;
                PointerVerdict::Pass
            }
        }
    }

    fn tick(&mut self, _: Instant) -> Recognition {
        self.held_until = None;
        Recognition::Recognized
    }

    fn next_tick(&self) -> Option<Instant> {
        self.held_until
    }
}

struct FlickPlugin;

impl Plugin for FlickPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("g"), "Gestures", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_gesture(GestureSpec::new(
            Id::new_static("g.flick"),
            "g.flick",
            Box::new(|| Box::new(Flick::default())),
        ))?;
        r.add_input(InputSpec::new(Id::new_static("g.chord"), "g.chord"))?;
        r.add_input(
            InputSpec::new(Id::new_static("g.light"), "g.light")
                .need(paperpaw_sdk::MenuNeed::Frontlight),
        )?;
        r.add_keymap(Cow::Borrowed(
            "[keymap.global]\ngesture.g.flick = a.special\ninput.g.chord = core.confirm\n",
        ))
    }
}

fn flick_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", log))
        .plugin(FlickPlugin)
        .plugin(LockPlugin(log.clone()))
        .plugin(Filters)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

/// A plugin's recognizer sees the fingers beside the kernel and runs
/// what its gesture is bound to, also when the time it asked for comes:
/// then the finger is its own, and the kernel makes no long press of it.
/// A finger it captures is not the kernel's. A plugin's input is raised
/// by command; inputs the device cannot make are not offered.
#[test]
fn plugins_recognize_gestures_and_add_inputs() {
    let log = log();
    let mut kernel = flick_kernel(&log);
    // The gesture is listed to bind too; the input for a front light is
    // not, as the device has none.
    let keys: Vec<String> = kernel.shared.inputs.iter().map(InputSpec::key).collect();
    assert_eq!(keys, ["input.g.chord", "gesture.g.flick"]);
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (60, 45));
    finger(&mut kernel, 1100, 0, PointerPhase::Up, (85, 45));
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    finger(&mut kernel, 2000, 0, PointerPhase::Down, (45, 45));
    let effects = kernel.tick(Instant(2100));
    assert!(effects.wake_at.is_some_and(|at| at <= Instant(2300)));
    kernel.tick(Instant(2300));
    kernel.tick(Instant(3000));
    finger(&mut kernel, 3100, 0, PointerPhase::Up, (45, 45));
    assert_eq!(*log.borrow(), ["a:a.special"], "no long tap besides");
    log.borrow_mut().clear();
    // Captured on the left: no tap of the kernel's.
    finger(&mut kernel, 4000, 0, PointerPhase::Down, (30, 45));
    finger(&mut kernel, 4050, 0, PointerPhase::Up, (30, 45));
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    kernel.apply(Command::Input(String::from("input.g.chord")));
    kernel.finish();
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
}

/// A finger a filter cancels is gone for the recognizers too: no gesture
/// comes of it when its time comes.
#[test]
fn a_cancelled_finger_is_gone_for_recognizers() {
    let log = log();
    let mut kernel = flick_kernel(&log);
    finger(&mut kernel, 1000, 2, PointerPhase::Down, (45, 45));
    finger(&mut kernel, 1050, 2, PointerPhase::Move, (45, 46));
    kernel.tick(Instant(1300));
    kernel.tick(Instant(2000));
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

/// A recognizer asking for a time past wakes the device at most twenty
/// times a second.
#[test]
fn recognizers_wake_the_device_seldom() {
    let log = log();
    let mut kernel = flick_kernel(&log);
    // It asks for its time 300 ms after the finger came down (its own:
    // the kernel waits for nothing): past by then, as the device slept.
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (30, 45));
    let wake = kernel.handle(
        Instant(5000),
        InputEvent::Button {
            name: "nothing",
            state: ButtonState::Released,
        },
    );
    let at = wake.wake_at.unwrap();
    assert!(at >= Instant(5000 + gestures::RECOGNIZER_TICK_MS), "{at:?}");
}

/// A lock of its own plugin: it guards the device and logs the actions
/// it gets.
struct Lock(Log);

impl Screen for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn guards(&self) -> bool {
        true
    }

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.0.borrow_mut().push(alloc::format!("lock:{action}"));
        Handled::Yes
    }
}

struct LockPlugin(Log);

impl Plugin for LockPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("lock"), "Lock", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("lock.screen"),
            "lock",
            Box::new(move |_, _| Box::new(Lock(log.clone()))),
        ))
    }
}

/// While a screen of another plugin guards the device, recognizers see
/// no finger (not the path of an unlock pattern, nor where a PIN is
/// tapped); an input raised by command reaches the guard first.
#[test]
fn recognizers_see_nothing_under_a_guard() {
    let log = log();
    let mut kernel = flick_kernel(&log);
    kernel.open_screen(Instant(10), Id::new_static("lock.screen"), Params::new());
    log.borrow_mut().clear();
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (60, 45));
    finger(&mut kernel, 1100, 0, PointerPhase::Up, (85, 45));
    assert!(kernel.input.recognizers[0].fingers.is_empty());
    assert!(
        !log.borrow().contains(&String::from("a:a.special")),
        "{:?}",
        log.borrow()
    );
    // An input raised by another plugin does not reach the lock: what is
    // typed into it is the reader's alone.
    log.borrow_mut().clear();
    kernel.apply(Command::Input(String::from("button.menu")));
    kernel.finish();
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    // Another plugin's input filter sees nothing of what is typed into
    // the lock (it would make *Menu* confirm).
    press(&mut kernel, 2000, "menu");
    assert_eq!(*log.borrow(), ["lock:core.menu"]);
}

/// On a screen that takes gestures itself, recognizers see no finger and
/// gesture bindings do not run.
#[test]
fn recognizers_follow_the_screens_policy() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin {
            keymap: Some("[keymap.a.screen]\ngestures.policy = core.gestures.own\n"),
            ..TestPlugin::new("a", &log)
        })
        .plugin(FlickPlugin)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (60, 45));
    finger(&mut kernel, 1100, 0, PointerPhase::Up, (85, 45));
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
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
        .plugin(TestPlugin {
            keymap: Some(keymap),
            priority: Priority(10),
            ..TestPlugin::new("a", &log)
        })
        .plugin(TestPlugin::new("b", &log))
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

/// A screen that guards the device takes the fingers others' recognizers
/// follow: they see them cancelled and wake the device no more; a layer
/// is not laid below the guard.
#[test]
fn a_guard_takes_the_fingers_of_recognizers() {
    let log = log();
    let mut kernel = flick_kernel(&log);
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (45, 45));
    assert!(!kernel.input.recognizers[0].fingers.is_empty());
    kernel.open_screen(Instant(1010), Id::new_static("lock.screen"), Params::new());
    assert!(kernel.input.recognizers[0].fingers.is_empty());
    assert_eq!(kernel.recognizers_due(), None, "nothing to wake for");
    let below = paperpaw_sdk::ScreenKey::new(kernel.stack[0].serial);
    kernel.apply(Command::SetKeyLayer(
        below,
        Some(Id::new_static("book.mine")),
    ));
    kernel.finish();
    assert_eq!(kernel.stack[0].key_layer, None);
}
