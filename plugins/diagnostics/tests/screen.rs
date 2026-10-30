//! The diagnostics screen with the real kernel, judged by what it draws:
//! it is the home screen only where the user binds it, a page turned
//! moves the progress bar and stops at the last page and the first,
//! confirm and a tap check a box and check it off again, a long confirm
//! (its own key binding) resets it all, and the number of pages comes
//! from the setting, also at one page.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    ButtonState, Color, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass,
    PixelFormat, Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_diagnostics::Diagnostics;
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, MemoryStorage, Plugin, Priority, RegisterError,
    Registrar, Screen, ScreenSpec, Version,
};

const WIDTH: u16 = 480;
const HEIGHT: u16 = 800;

/// A device with buttons and touch, so both can be tried.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test",
    DisplayInfo::new(WIDTH, HEIGHT, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(
        true,
        true,
        false,
        &["back", "confirm", "left", "right", "up", "down"],
    ),
    MemoryClass::Large,
);

/// The screen as drawn so far, row by row.
type Frame = Vec<Color>;

struct Device {
    kernel: Kernel,
    now: u64,
    frame: Frame,
}

impl Device {
    /// With its screen as the home screen, chosen as a user would.
    fn new(config: &str) -> Self {
        Self::with(
            &format!("[slots]\ncore.home = diagnostics.screen\n{config}"),
            None,
        )
    }

    fn with(config: &str, home: Option<OtherHome>) -> Self {
        let mut builder = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Diagnostics);
        if let Some(home) = home {
            builder = builder.plugin(home);
        }
        let kernel = builder.build().unwrap();
        let mut device = Self {
            kernel,
            now: 0,
            frame: vec![Color::WHITE; usize::from(WIDTH) * usize::from(HEIGHT)],
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device.draw();
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

    /// Draws what changed into the frame kept.
    fn draw(&mut self) -> &Frame {
        let frame = &mut self.frame;
        self.kernel.render(&mut |top, view| {
            for y in 0..view.height {
                for x in 0..view.width.min(WIDTH) {
                    let row = usize::from(top + y);
                    if row < usize::from(HEIGHT) {
                        frame[row * usize::from(WIDTH) + usize::from(x)] = view.pixel(x, y);
                    }
                }
            }
        });
        &self.frame
    }

    fn run(&mut self, action: Id) -> Frame {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
        self.draw().clone()
    }

    fn button(&mut self, name: &'static str, long: bool) -> Frame {
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
        self.draw().clone()
    }

    fn tap(&mut self, x: i32, y: i32) -> Frame {
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 500;
            let event = InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                Point::new(x, y),
                PointerTool::Finger,
                1000,
            ));
            let effects = self.kernel.handle(Instant(self.now), event);
            // Timers wait while the finger is down: no long press.
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
        self.draw().clone()
    }
}

/// The rows that differ between `a`
/// and `b`.
fn changed_rows(a: &Frame, b: &Frame) -> Vec<usize> {
    (0..usize::from(HEIGHT))
        .filter(|&y| {
            let row = y * usize::from(WIDTH)..(y + 1) * usize::from(WIDTH);
            a[row.clone()] != b[row]
        })
        .collect()
}

/// The darker pixels in a row band, a measure of what is filled.
fn dark(frame: &Frame, rows: std::ops::Range<usize>) -> usize {
    rows.flat_map(|y| frame[y * usize::from(WIDTH)..(y + 1) * usize::from(WIDTH)].iter())
        .filter(|c| **c != Color::WHITE)
        .count()
}

/// The bottom tenth of the screen, where the progress bar is.
fn bottom() -> std::ops::Range<usize> {
    usize::from(HEIGHT) * 9 / 10..usize::from(HEIGHT)
}

/// It binds no slot: a firmware that brings it along never offers it as
/// the home screen.
#[test]
fn it_is_the_home_screen_only_where_the_user_binds_it() {
    let device = Device::with("", None);
    assert_ne!(
        device.kernel.active_screen().map(Id::as_str),
        Some("diagnostics.screen")
    );
    let device = Device::with("", Some(OtherHome));
    assert_eq!(
        device.kernel.active_screen().map(Id::as_str),
        Some("other.home")
    );
    // Bound by the user, it is the home screen again.
    let device = Device::with("[slots]\ncore.home = diagnostics.screen\n", Some(OtherHome));
    assert_eq!(
        device.kernel.active_screen().map(Id::as_str),
        Some("diagnostics.screen")
    );
}

#[test]
fn pages_move_the_progress_bar_and_stop_at_the_ends() {
    let mut device = Device::new("[diagnostics]\npages = 3\n");
    let first = device.draw().clone();
    // Not before the first page.
    assert!(device.run(actions::PREV_PAGE) == first);
    let second = device.run(actions::NEXT_PAGE);
    let rows = changed_rows(&first, &second);
    assert!(!rows.is_empty());
    // Only at the bottom: the label and the bar.
    assert!(
        rows.iter().all(|y| *y > usize::from(HEIGHT) / 2),
        "{rows:?}"
    );
    assert!(dark(&second, bottom()) > dark(&first, bottom()));
    let third = device.run(actions::NEXT_PAGE);
    assert!(dark(&third, bottom()) > dark(&second, bottom()));
    // Not past the last page.
    assert!(device.run(actions::NEXT_PAGE) == third);
    assert!(device.run(actions::PREV_PAGE) == second);
}

#[test]
fn one_page_shows_a_full_bar_and_turns_nowhere() {
    let mut device = Device::new("[diagnostics]\npages = 1\n");
    let only = device.draw().clone();
    assert!(device.run(actions::NEXT_PAGE) == only);
    assert!(device.run(actions::PREV_PAGE) == only);
    let mut many = Device::new("[diagnostics]\npages = 10000\n");
    assert!(dark(&only, bottom()) > dark(many.draw(), bottom()));
}

#[test]
fn confirm_checks_a_box_and_a_long_confirm_resets() {
    let mut device = Device::new("");
    let start = device.draw().clone();
    let checked = device.run(actions::CONFIRM);
    assert!(checked != start);
    // Again: as before.
    assert!(device.run(actions::CONFIRM) == start);
    // Another box, a page on, then the reset by its own key binding.
    device.run(actions::RIGHT);
    device.run(actions::CONFIRM);
    device.run(actions::NEXT_PAGE);
    assert!(*device.draw() != start);
    assert!(device.button("confirm", true) == start);
}

#[test]
fn focus_goes_round_the_four_boxes() {
    let mut device = Device::new("");
    let start = device.draw().clone();
    // Right four times, left four times, up and down twice: back where
    // it began.
    for action in [actions::RIGHT, actions::LEFT, actions::UP, actions::DOWN] {
        let times = if action == actions::UP || action == actions::DOWN {
            2
        } else {
            4
        };
        let mut moved = Vec::new();
        for _ in 0..times {
            moved.push(device.run(action.clone()));
        }
        assert!(moved[0] != start, "{action}");
        assert!(*moved.last().unwrap() == start, "{action}");
    }
}

#[test]
fn a_tap_checks_the_box_under_it() {
    let mut device = Device::new("");
    let start = device.draw().clone();
    // The middle of the screen's upper half, right: the second box.
    let (x, y) = (i32::from(WIDTH) * 3 / 4, i32::from(HEIGHT) * 2 / 5);
    let checked = device.tap(x, y);
    assert!(checked != start);
    // Again: unchecked, the focus stays on it.
    let unchecked = device.tap(x, y);
    assert!(unchecked != checked && unchecked != start);
    // Confirm checks it again: the focus moved there.
    assert!(device.run(actions::CONFIRM) == checked);
}

/// A home screen of another plugin.
struct OtherHome;

struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for OtherHome {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("other"), "Other home", Version::new(0, 1, 0))
            .description("A home screen.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("other.home"),
            "Home",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("other.home"), Priority::DEFAULT)
    }
}
