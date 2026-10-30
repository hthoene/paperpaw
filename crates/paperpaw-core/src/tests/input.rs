//! Input recognition: presses dropped while a frame was not shown yet,
//! long presses, fingers resting together, the edges.

use super::*;

/// A button going down or up at `at`.
fn button(kernel: &mut Kernel, at: u64, name: &'static str, state: ButtonState) {
    kernel.handle(Instant(at), InputEvent::Button { name, state });
}

/// A finger going down, moving or lifted at `at`.
fn finger(
    kernel: &mut Kernel,
    at: u64,
    id: u8,
    phase: PointerPhase,
    (x, y): (i32, i32),
) -> crate::Effects {
    let event = PointerEvent::new(id, phase, Point::new(x, y), PointerTool::Finger, 1000);
    kernel.handle(Instant(at), InputEvent::Pointer(event))
}

/// Whether nothing is held: no button and no finger waits for its long
/// press.
fn nothing_held(kernel: &Kernel) -> bool {
    kernel
        .input
        .recognizer
        .next_deadline(&kernel.thresholds())
        .is_none()
}

/// A kernel whose home screen showed at 1000: presses made before are
/// dropped.
fn shown_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel.shown(Instant(1000));
    kernel
}

#[test]
fn a_dropped_press_pressed_again_after_the_frame_does_not_stick() {
    let log = log();
    let mut kernel = shown_kernel(&log);
    // Pressed while the screen came up: dropped. Pressed again once it
    // showed (its release missed, or the press came twice): taken, and
    // let go it is a short press, not one held on to a long press.
    button(&mut kernel, 500, "back", ButtonState::Pressed);
    button(&mut kernel, 1200, "back", ButtonState::Pressed);
    button(&mut kernel, 1250, "back", ButtonState::Released);
    kernel.tick(Instant(3000));
    assert_eq!(*log.borrow(), ["a:core.back"]);
    assert!(nothing_held(&kernel));
}

#[test]
fn a_held_press_pressed_again_before_the_frame_keeps_its_release() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // Pressed before anything was dropped; the press comes again, late,
    // from before the screen showed: the release still ends the press.
    button(&mut kernel, 900, "back", ButtonState::Pressed);
    kernel.shown(Instant(1000));
    button(&mut kernel, 950, "back", ButtonState::Pressed);
    button(&mut kernel, 1100, "back", ButtonState::Released);
    kernel.tick(Instant(3000));
    assert_eq!(*log.borrow(), ["a:core.back"]);
    assert!(nothing_held(&kernel));
}

#[test]
fn a_press_dropped_after_the_clock_went_back_does_not_stick() {
    let log = log();
    let mut kernel = shown_kernel(&log);
    kernel.shown(Instant(2000));
    button(&mut kernel, 5000, "back", ButtonState::Pressed);
    // The clock went back: the press again, then its release, seem older
    // than the frames shown.
    button(&mut kernel, 100, "back", ButtonState::Pressed);
    button(&mut kernel, 150, "back", ButtonState::Released);
    kernel.tick(Instant(9000));
    assert!(!log.borrow().iter().any(|l| l == "a:core.home"));
    assert!(nothing_held(&kernel));
}

#[test]
fn a_finger_down_again_before_the_frame_does_not_stick() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // Down before anything was dropped; down again (its lift missed)
    // from before the screen showed: dropped, and the first touch is
    // over, no long press of it later.
    finger(&mut kernel, 900, 0, PointerPhase::Down, (45, 45));
    kernel.shown(Instant(1000));
    finger(&mut kernel, 950, 0, PointerPhase::Down, (45, 45));
    finger(&mut kernel, 1100, 0, PointerPhase::Up, (45, 45));
    kernel.tick(Instant(3000));
    assert!(!kernel.settings().read(&settings::NIGHT_MODE));
    assert!(nothing_held(&kernel));
}

/// With the page buttons swapped while reading, a page button pressed
/// again from before the frame showed still ends with its release: the
/// press held is the swapped one's, and later presses turn pages.
#[test]
fn a_swapped_page_button_pressed_again_keeps_its_release() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .user_config(
            "[slots]
core.reader = a.screen
[core]
swap-page-buttons = on
",
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_slot(
        Instant(10),
        slots::READER,
        paperpaw_sdk::Params::new().with("book", "a.txt"),
    );
    render(&mut kernel);
    log.borrow_mut().clear();
    button(&mut kernel, 900, "page-forward", ButtonState::Pressed);
    kernel.shown(Instant(1000));
    button(&mut kernel, 950, "page-forward", ButtonState::Pressed);
    button(&mut kernel, 1100, "page-forward", ButtonState::Released);
    assert!(nothing_held(&kernel));
    button(&mut kernel, 2000, "page-forward", ButtonState::Pressed);
    button(&mut kernel, 2050, "page-forward", ButtonState::Released);
    assert_eq!(*log.borrow(), ["a:core.prev-page", "a:core.prev-page"]);
    assert!(kernel.input.swapped.is_empty());
}

/// A small generator of pseudo random numbers, the same every run.
struct Dice(u32);

impl Dice {
    fn roll(&mut self, sides: u32) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.0 >> 16) % sides
    }
}

#[test]
fn any_order_of_presses_leaves_the_buttons_working() {
    let mut dice = Dice(7);
    for _ in 0..300 {
        let log = log();
        let mut kernel = shown_kernel(&log);
        // Presses and releases of two buttons in any order, at times
        // going back and forth around frames shown.
        for _ in 0..dice.roll(10) {
            let at = u64::from(dice.roll(40)) * 100;
            let name = ["back", "confirm"][dice.roll(2) as usize];
            match dice.roll(5) {
                0 | 1 => button(&mut kernel, at, name, ButtonState::Pressed),
                2 | 3 => button(&mut kernel, at, name, ButtonState::Released),
                _ => kernel.shown(Instant(at)),
            }
        }
        // Pressed and let go after all that: nothing stays held.
        for name in ["back", "confirm"] {
            button(&mut kernel, 10_000, name, ButtonState::Pressed);
            button(&mut kernel, 10_050, name, ButtonState::Released);
        }
        assert!(nothing_held(&kernel));
        assert!(kernel.input.dropped_buttons.is_empty());
    }
}

#[test]
fn two_fingers_resting_wake_the_shell_no_more() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // Two fingers down and held: no long press, so nothing to wake for.
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (30, 45));
    finger(&mut kernel, 1010, 1, PointerPhase::Down, (60, 45));
    for at in [1600, 2000, 5000] {
        let wake = kernel.tick(Instant(at)).wake_at();
        assert!(wake.is_none_or(|w| w > Instant(at)), "{wake:?} at {at}");
    }
    // One lifted, the other rests on: still no long press of it.
    finger(&mut kernel, 5100, 1, PointerPhase::Up, (60, 45));
    kernel.tick(Instant(6000));
    assert!(!kernel.settings().read(&settings::NIGHT_MODE));
    assert!(nothing_held(&kernel));
}

/// A home screen that notes the gestures and actions it gets.
struct GesturePlugin(Log);

struct GestureScreen(Log);

impl Screen for GestureScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.0.borrow_mut().push(alloc::format!("{action}"));
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: paperpaw_sdk::Gesture, _: &mut Cx<'_, '_>) -> Handled {
        let start = gesture.start;
        let noted = alloc::format!("{:?} {},{}", gesture.kind, start.x, start.y);
        self.0.borrow_mut().push(noted);
        Handled::Yes
    }
}

impl Plugin for GesturePlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("g"), "Gestures", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("g.screen"),
            "g",
            Box::new(move |_, _| Box::new(GestureScreen(log.clone()))),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("g.screen"), Priority::DEFAULT)?;
        // Every edge swipes in: the strips along them are the kernel's.
        r.add_keymap(Cow::Borrowed(
            "[keymap.g.screen]
edge.top = core.quick-panel
edge.bottom = core.home
edge.left = core.back
edge.right = core.back
",
        ))
    }
}

#[test]
fn a_finger_held_at_an_edge_is_a_long_press() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(GesturePlugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // Held still on the first line, in the strip along the top: a long
    // press when its time comes, as anywhere else; lifted, nothing more.
    let effects = finger(&mut kernel, 100, 0, PointerPhase::Down, (45, 5));
    assert_eq!(effects.wake_at(), Some(Instant(600)));
    kernel.tick(Instant(600));
    assert_eq!(*log.borrow(), ["LongPress 45,5"]);
    finger(&mut kernel, 700, 0, PointerPhase::Up, (45, 5));
    assert_eq!(*log.borrow(), ["LongPress 45,5"]);
    assert!(nothing_held(&kernel));
    // At a side, where a thumb rests holding the device: no long press,
    // and lifted after as long, nothing.
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (88, 45));
    kernel.tick(Instant(1500));
    finger(&mut kernel, 1600, 0, PointerPhase::Up, (88, 45));
    assert_eq!(*log.borrow(), ["LongPress 45,5"]);
    log.borrow_mut().clear();

    // Swipes in from the edges still apply what they are bound to, also
    // after resting a moment; a tap at an edge is a tap.
    swipe(&mut kernel, 2000, (45, 88), (45, 30));
    finger(&mut kernel, 3000, 0, PointerPhase::Down, (2, 45));
    kernel.tick(Instant(3300));
    finger(&mut kernel, 3300, 0, PointerPhase::Move, (70, 45));
    kernel.tick(Instant(3600));
    finger(&mut kernel, 3700, 0, PointerPhase::Up, (70, 45));
    tap(&mut kernel, 4000, 88, 45);
    assert_eq!(*log.borrow(), ["core.home", "core.back", "Tap 88,45"]);
    assert!(nothing_held(&kernel));
}

/// A thumb resting at an edge is no second finger: a long press elsewhere
/// still comes, and the thumb makes none of its own, also at the top.
#[test]
fn a_thumb_resting_at_an_edge_leaves_long_presses_elsewhere() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(GesturePlugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    for (at, thumb) in [(0, (88, 45)), (2000, (45, 5))] {
        finger(&mut kernel, at + 100, 1, PointerPhase::Down, thumb);
        finger(&mut kernel, at + 200, 0, PointerPhase::Down, (45, 45));
        kernel.tick(Instant(at + 700));
        finger(&mut kernel, at + 800, 0, PointerPhase::Up, (45, 45));
        finger(&mut kernel, at + 900, 1, PointerPhase::Up, thumb);
    }
    assert_eq!(*log.borrow(), ["LongPress 45,45", "LongPress 45,45"]);
    assert!(nothing_held(&kernel));
}

#[test]
fn right_held_opens_the_quick_panel_and_pressed_still_goes_right() {
    let log = log();
    let mut kernel = shown_kernel(&log);
    // The quick panel by the buttons, as down from the top edge.
    let held = crate::keymap::InputKey::LongButton(Cow::Borrowed("right"));
    assert_eq!(kernel.resolve(&held), Some(&actions::QUICK_PANEL));
    // Pressed briefly: right, as before (when it is let go).
    button(&mut kernel, 1200, "right", ButtonState::Pressed);
    button(&mut kernel, 1250, "right", ButtonState::Released);
    kernel.tick(Instant(3000));
    assert_eq!(*log.borrow(), ["a:core.right"]);
    assert!(nothing_held(&kernel));
}

/// A screen is seen while the refresh that shows it goes on (a full one
/// on the X4 takes 1.6 s): a press made then counts, not only one made
/// once the refresh is over.
#[test]
fn a_press_while_a_new_screen_refreshes_counts() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // The refresh began at 1000 and ended at 2600; the presses made
    // meanwhile are handled after it.
    kernel.showing(Instant(1000));
    kernel.shown(Instant(2600));
    // Before it began to show: dropped.
    press(&mut kernel, 900, "page-forward");
    assert!(log.borrow().is_empty());
    // While it showed: taken.
    press(&mut kernel, 1200, "page-forward");
    assert_eq!(*log.borrow(), ["a:core.next-page"]);
}
