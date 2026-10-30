//! Entries that show what they are now, as their plugin says, in the bar,
//! the list and the panel alike: a switch on, a label and a badge of their
//! own, and a faint one that the focus passes and that does nothing when
//! tapped.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    ButtonState, InputEvent, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::semantics::{Part, Role};
use paperpaw_sdk::{
    ActionSpec, Canvas, Cx, DeviceInfo, DisplayInfo, Env, Handled, Id, InputCaps, Instant,
    ItemState, Manifest, MemoryClass, MemoryStorage, MenuEntry, Params, PixelFormat, Plugin, Point,
    RegisterError, Registrar, Screen, ScreenSpec, Version,
};

/// An X4: buttons, no touch.
const X4: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

/// A touch screen.
const BOOX: DeviceInfo = X4.with_input(InputCaps::new(false, true, false, &[]));

/// The screen the menus open over, as a book.
const BASE: Id = Id::new_static("probe.base");
const MENU: Id = Id::new_static("probe.menu");
/// Faint, labelled "Not now"; first, so that the focus starts past it.
const FAINT: Id = Id::new_static("probe.faint");
/// Shown as a switch that is on.
const ON: Id = Id::new_static("probe.on");
/// With a badge.
const COUNTED: Id = Id::new_static("probe.counted");
const PLAIN: Id = Id::new_static("probe.plain");

thread_local! {
    /// The actions run, in order.
    static RAN: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

fn ran() -> Vec<&'static str> {
    RAN.with(|r| r.borrow().clone())
}

struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// Four entries saying what they are now.
struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            BASE,
            "probe",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        for (order, id) in [FAINT, ON, COUNTED, PLAIN].into_iter().enumerate() {
            let name: &'static str = match order {
                0 => "probe.faint",
                1 => "probe.on",
                2 => "probe.counted",
                _ => "probe.plain",
            };
            r.add_action(
                ActionSpec::new(id.clone(), name)
                    .handler(move |_, _, _| RAN.with(|r| r.borrow_mut().push(name))),
            )?;
            r.add_menu_entry(MenuEntry::new(MENU, id).order(order as i16))?;
        }
        r.set_item_state(FAINT, |_| ItemState::new().enabled(false).label("Not now"))?;
        r.set_item_state(ON, |_| ItemState::new().checked(true))?;
        r.set_item_state(COUNTED, |_| ItemState::new().badge("3"))
    }
}

struct Test {
    kernel: Kernel,
    now: u64,
}

impl Test {
    fn new(device: DeviceInfo) -> Self {
        RAN.with(|r| r.borrow_mut().clear());
        let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
        // Without the hints over it all.
        let builder = Kernel::builder(device)
            .storage(storage)
            .user_config("[help]\nshow = false\n");
        let mut kernel = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs)
            .plugin(Probe)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.open_screen(Instant(0), BASE, Params::new());
        Self { kernel, now: 0 }
    }

    /// Opens [`MENU`] in `slot`, as `mode` if given.
    fn open(&mut self, slot: Id, mode: Option<&str>) {
        let mut with = Params::new().with(params::MENU, MENU.as_str().to_string());
        if let Some(mode) = mode {
            with = with.with(params::MODE, mode.to_string());
        }
        self.now += 200;
        self.kernel.open_slot(Instant(self.now), slot, with);
        self.kernel.render(&mut |_, _| {});
    }

    fn press(&mut self, name: &'static str) {
        self.now += 200;
        let at = Instant(self.now);
        for (later, state) in [(0, ButtonState::Pressed), (50, ButtonState::Released)] {
            self.kernel
                .handle(at.add_millis(later), InputEvent::Button { name, state });
        }
        self.kernel.render(&mut |_, _| {});
    }

    fn tap(&mut self, point: Point) {
        for (later, phase) in [(0, PointerPhase::Down), (50, PointerPhase::Up)] {
            self.now += later + 100;
            let event = InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                point,
                PointerTool::Finger,
                1000,
            ));
            self.kernel.handle(Instant(self.now), event);
        }
        self.kernel.render(&mut |_, _| {});
    }

    fn parts(&mut self) -> Vec<Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    /// The part showing `label`.
    fn part(&mut self, label: &str) -> Part {
        let parts = self.parts();
        parts
            .iter()
            .find(|p| p.label.starts_with(label))
            .cloned()
            .unwrap_or_else(|| panic!("{label}: {parts:?}"))
    }

    /// The label of the focused part, the last one (the screen on top).
    fn focused(&mut self) -> Option<String> {
        self.parts()
            .into_iter()
            .rev()
            .find(|p| p.focused)
            .map(|p| p.label)
    }
}

#[test]
fn the_bar_shows_labels_and_badges_and_its_focus_passes_faint_entries() {
    let mut test = Test::new(X4);
    test.open(slots::MENU, Some("bar"));
    let texts: Vec<String> = test
        .parts()
        .into_iter()
        .filter(|p| p.role == Role::Button)
        .map(|p| p.label)
        .collect();
    assert!(texts.iter().any(|t| t == "Not now"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "probe.counted · 3"), "{texts:?}");
    // The focus starts past the faint one, on the switch, and goes on
    // round past it.
    test.press("down");
    test.press("confirm");
    assert_eq!(ran(), ["probe.counted"]);
    test.open(slots::MENU, Some("bar"));
    for _ in 0..3 {
        test.press("down");
    }
    test.press("confirm");
    assert_eq!(ran(), ["probe.counted", "probe.on"]);
}

#[test]
fn a_faint_entry_tapped_does_nothing() {
    for (slot, mode) in [
        (slots::MENU, Some("bar")),
        (slots::MENU, None),
        (slots::PANEL, None),
    ] {
        let mut test = Test::new(BOOX);
        test.open(slot.clone(), mode);
        let area = test.part("Not now").area;
        let screen = test.kernel.active_screen().cloned();
        test.tap(Point {
            x: i32::from(area.x) + i32::from(area.width) / 2,
            y: i32::from(area.y) + i32::from(area.height) / 2,
        });
        assert!(ran().is_empty(), "{slot} {mode:?}: {:?}", ran());
        assert_eq!(test.kernel.active_screen(), screen.as_ref(), "{slot}");
        // Another one tapped runs.
        let area = test.part("probe.plain").area;
        test.tap(Point {
            x: i32::from(area.x) + i32::from(area.width) / 2,
            y: i32::from(area.y) + i32::from(area.height) / 2,
        });
        assert_eq!(ran(), ["probe.plain"], "{slot} {mode:?}");
    }
}

#[test]
fn the_list_passes_faint_entries_and_shows_switches_and_badges() {
    let mut test = Test::new(X4);
    test.open(slots::MENU, None);
    assert_eq!(test.part("probe.on").on, Some(true));
    let counted = test.part("probe.counted").label;
    assert!(counted.contains('3'), "{counted}");
    // The faint one is told as doing nothing now; the others are not.
    assert!(test.part("Not now").disabled);
    assert!(!test.part("probe.on").disabled);
    // From the switch, the focus goes round past the faint one.
    let focused = test.focused().unwrap();
    assert!(focused.starts_with("probe.on"), "{focused}");
    for _ in 0..3 {
        test.press("down");
    }
    let focused = test.focused().unwrap();
    assert!(focused.starts_with("probe.on"), "{focused}");
    test.press("up");
    test.press("confirm");
    assert_eq!(ran(), ["probe.plain"]);
}

#[test]
fn the_panel_shows_switches_labels_badges_and_passes_faint_entries() {
    let mut test = Test::new(X4);
    test.open(slots::PANEL, None);
    assert_eq!(test.part("probe.on").on, Some(true));
    assert_eq!(test.part("Not now").on, None);
    assert!(test.part("Not now").disabled);
    assert!(!test.part("probe.counted · 3").disabled);
    assert_eq!(test.focused().as_deref(), Some("probe.on"));
    test.press("down");
    assert_eq!(test.focused().as_deref(), Some("probe.counted · 3"));
    test.press("down");
    test.press("down");
    assert_eq!(test.focused().as_deref(), Some("probe.on"), "round");
    test.press("up");
    test.press("confirm");
    assert_eq!(ran(), ["probe.plain"]);
}
