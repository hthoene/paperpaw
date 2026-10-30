//! The settings pages with the real kernel, by the buttons of an X4:
//! covered by a full screen, they are not made anew for each setting
//! changed meanwhile, but once when they show again.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_settings::{SCREEN, SettingsPlugin};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::widgets::{Widget, WidgetSpec};
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, Rect, RegisterError, Registrar,
    Screen, ScreenSpec, SettingGroupSpec, SettingKind, SettingSpec, SettingValue, Version,
};

/// The system's allocator, counting the blocks this thread takes.
struct Counting;

thread_local! {
    static BLOCKS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = BLOCKS.try_with(|blocks| blocks.set(blocks.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        unsafe { System.dealloc(block, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The heap blocks `f` takes.
fn blocks(f: impl FnOnce()) -> usize {
    let before = BLOCKS.with(Cell::get);
    f();
    BLOCKS.with(Cell::get) - before
}

/// An X4: buttons, no touch.
const X4: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

/// A full screen of another plugin, opened above the settings.
const COVER: Id = Id::new_static("probe.cover");
const GROUP: Id = Id::new_static("probe.group");
/// A number of [`GROUP`].
const SIZE: &str = "probe.size";
/// Previews [`GROUP`] on its page.
const PREVIEW: Id = Id::new_static("probe.preview");

struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

struct Sample;

impl Widget for Sample {
    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        40
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}
}

/// A group of settings with a preview, which counts how often it is
/// made: once for each time its page is.
struct Probe(Rc<Cell<usize>>);

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            COVER,
            "probe",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.add_setting_group(SettingGroupSpec::new(GROUP, "probe.group"))?;
        r.add_setting(SettingSpec::new(
            Id::new_static(SIZE),
            SIZE,
            SettingKind::Int { min: 1, max: 9 },
            SettingValue::Int(5),
            GROUP,
        ))?;
        let made = self.0.clone();
        r.add_widget(
            WidgetSpec::new(PREVIEW, "probe.preview", move |_| {
                made.set(made.get() + 1);
                Box::new(Sample)
            })
            .previews(GROUP),
        )?;
        Ok(())
    }
}

struct Test {
    kernel: Kernel,
    /// How often the preview was made.
    made: Rc<Cell<usize>>,
    now: u64,
}

impl Test {
    fn new() -> Self {
        let made = Rc::new(Cell::new(0));
        let mut kernel = Kernel::builder(X4)
            .storage(MemoryStorage::new())
            .plugin(SettingsPlugin)
            .plugin(Probe(made.clone()))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            made,
            now: 0,
        }
    }

    fn later(&mut self) -> Instant {
        self.now += 200;
        Instant(self.now)
    }

    /// Opens the settings, of `group` if given.
    fn open_settings(&mut self, group: Option<&Id>) {
        let params = group.map_or_else(Params::new, |group| {
            Params::new().with(params::ID, group.as_str().to_string())
        });
        let now = self.later();
        self.kernel.open_slot(now, slots::SETTINGS, params);
        self.kernel.render(&mut |_, _| {});
        assert_eq!(self.kernel.active_screen(), Some(&SCREEN));
    }

    fn cover(&mut self) {
        let now = self.later();
        self.kernel.open_screen(now, COVER, Params::new());
        self.kernel.render(&mut |_, _| {});
        assert_eq!(self.kernel.active_screen(), Some(&COVER));
    }

    fn set_size(&mut self, size: i64) {
        let now = self.later();
        self.kernel
            .set_setting(now, SIZE, &size.to_string())
            .unwrap();
        self.kernel.render(&mut |_, _| {});
    }

    fn press(&mut self, name: &'static str) {
        let at = self.later();
        for (later, state) in [(0, ButtonState::Pressed), (50, ButtonState::Released)] {
            self.kernel
                .handle(at.add_millis(later), InputEvent::Button { name, state });
        }
        self.kernel.render(&mut |_, _| {});
    }
}

#[test]
fn a_covered_group_page_is_made_anew_once_it_shows() {
    let mut test = Test::new();
    test.open_settings(Some(&GROUP));
    let made = test.made.get();
    assert!(made > 0, "the page previews its group");
    // Shown, the page follows each change at once.
    test.set_size(6);
    assert_eq!(test.made.get(), made + 1);
    test.cover();
    for size in [7, 8, 9] {
        test.set_size(size);
    }
    assert_eq!(test.made.get(), made + 1, "not while covered");
    test.press("back");
    assert_eq!(test.kernel.active_screen(), Some(&SCREEN));
    assert_eq!(test.made.get(), made + 2, "once it shows again");
}

#[test]
fn a_covered_group_list_does_nothing_for_a_change() {
    // What a change costs with nothing of the settings open ...
    let mut alone = Test::new();
    alone.cover();
    alone.set_size(6);
    let base = blocks(|| alone.set_size(7));
    // ... and with the list of groups below the screen on top.
    let mut test = Test::new();
    test.open_settings(None);
    test.cover();
    test.set_size(6);
    let covered = blocks(|| test.set_size(7));
    assert!(covered <= base + 2, "{covered} blocks, {base} without it");
    test.press("back");
    assert_eq!(test.kernel.active_screen(), Some(&SCREEN));
}
