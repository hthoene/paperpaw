//! A slider dragged in the quick panel, as on an X4 Pro: a finger comes
//! down on its band, moves and is lifted. The knob moves as far as the
//! finger does from where it came down, in the slider's steps and within
//! its range; the value shows at once without being kept, each move draws
//! the slider's row alone with a quick refresh, and lifting the finger
//! keeps the value and refreshes the panel cleanly. Nothing is read from
//! or written to the card while the finger moves: the X4 Pro's card is
//! slow, and the light follows the value shown.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;
use native_activation::BuildNative as _;

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::kit::menus::MenuEntry;
use paperpaw_core::{Effects, Kernel, Staging};
use paperpaw_platform::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rect, RefreshMode, Rotation, Storage,
    StorageError,
};
use paperpaw_plugin_menu::native_panel as panel;
use paperpaw_sdk::builtin::{actions, menus};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{Manifest, MemoryStorage, RegisterError, SettingSpec, SettingValue, Version};

/// An X4 Pro as the emulator's profile has it: touch and buttons, frames
/// drawn in bands of 96 rows.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4-pro",
    "Test X4 Pro",
    DisplayInfo::new(480, 800, 219, PixelFormat::Gray2, Rotation::Deg0, None),
    InputCaps::new(
        true,
        true,
        false,
        &["page-back", "page-forward", "home", "power"],
    ),
    MemoryClass::Small,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// A light as the X4 Pro's shell registers it: percent in fives.
const LIGHT: Setting<i64> = Setting::new("test.light");

struct Light;

/// A percentage set by a slider, in steps of 5.
static LIGHT_EXTRA: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .unit("core.unit.percent")
    .step(5)
    .slider();

impl Light {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            paperpaw_sdk::Id::new_static("test"),
            "Light",
            Version::new(0, 1, 0),
        )
        .description("A light set with a slider.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Staging) -> Result<(), RegisterError> {
        r.add_setting(
            SettingSpec::number(
                LIGHT.static_str(),
                "test.light",
                0,
                100,
                0,
                paperpaw_sdk::builtin::groups::OTHER,
            )
            .with_extra(&LIGHT_EXTRA),
        )?;
        r.add_menu_entry(MenuEntry::new(menus::QUICK, LIGHT.into()).order(20))
    }
}

/// The card, with every call on it counted while `counting`.
struct Counted {
    inner: MemoryStorage,
    calls: Rc<RefCell<Option<Vec<String>>>>,
}

impl Counted {
    fn note(&self, op: &str, path: &str) {
        if let Some(calls) = self.calls.borrow_mut().as_mut() {
            calls.push(format!("{op} {path}"));
        }
    }
}

impl Storage for Counted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.note("read", path);
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.note("size", path);
        self.inner.size(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.note("exists", path);
        self.inner.exists(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note("write", path);
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note("append", path);
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.note("list", dir);
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.note("remove", path);
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.note("rename", from);
        self.inner.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.note("create_dir", path);
        self.inner.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.note("remove_dir_all", path);
        self.inner.remove_dir_all(path)
    }
}

struct App {
    kernel: Kernel,
    now: u64,
    calls: Rc<RefCell<Option<Vec<String>>>>,
}

impl App {
    fn new() -> Self {
        let calls = Rc::new(RefCell::new(None));
        let storage = Counted {
            inner: MemoryStorage::new().with(".paperpaw/plugins/setup/done", Vec::new()),
            calls: calls.clone(),
        };
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .frame_rows(96)
            .row_align(8)
            .user_config(format!(
                "[core]\nlanguage = en\n{}",
                native_activation::QUIET
            ));
        let mut kernel = paperpaw_app::install(builder, fonts())
            .items(Light.manifest(), |r| Light.register(r))
            .build_native();
        let effects = kernel.start(Instant(0));
        let mut app = Self {
            kernel,
            now: 0,
            calls,
        };
        app.settle(effects);
        app
    }

    /// Lets timers due within a second run, drawing what changes as a
    /// device would; the refresh asked for.
    fn settle(&mut self, mut effects: Effects) -> Option<RefreshMode> {
        let mut refresh = None;
        for _ in 0..1000 {
            refresh = refresh.max(effects.refresh().map(|(_, mode)| mode));
            self.kernel.render(&mut |_, _| {});
            let Some(at) = effects.wake_at().filter(|at| at.0 <= self.now + 1000) else {
                break;
            };
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
        refresh
    }

    fn finger(&mut self, phase: PointerPhase, x: i32, y: i32) -> Option<RefreshMode> {
        self.now += 30;
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ));
        let effects = self.kernel.handle(Instant(self.now), event);
        // Timers wait while the finger is down: a long press is not meant.
        if phase == PointerPhase::Down {
            return effects.refresh().map(|(_, mode)| mode);
        }
        self.settle(effects)
    }

    /// The slider's track and the value it shows.
    fn slider(&mut self) -> (Rect, String) {
        let parts = self.kernel.describe();
        let part = parts
            .iter()
            .find(|p| p.role == Role::Slider)
            .unwrap_or_else(|| panic!("a slider in the quick panel: {parts:?}"));
        (part.area, part.label.clone())
    }

    /// Where the slider's `+` is: at the right end of the row above the
    /// track, level with the title shown there (the buttons are not parts,
    /// and the value text is folded into the label of a home button behind
    /// the panel where one lies under it).
    fn plus_above(&mut self, track: Rect) -> (i32, i32) {
        let parts = self.kernel.describe();
        let title = parts
            .iter()
            .filter(|p| p.role == Role::Text && p.label == LIGHT.as_str() && p.area.y < track.y)
            .max_by_key(|p| p.area.y)
            .map(|p| p.area)
            .unwrap_or_else(|| panic!("the title above the slider: {parts:?}"));
        let y = i32::from(title.y) + i32::from(title.height) / 2;
        (track.right() as i32 - 10, y)
    }

    fn count(&mut self, on: bool) -> Vec<String> {
        let mut calls = self.calls.borrow_mut();
        let counted = calls.take().unwrap_or_default();
        *calls = on.then(Vec::new);
        counted
    }
}

#[test]
fn a_slider_dragged_reads_nothing_and_follows_the_finger() {
    let mut app = App::new();
    let effects = app.kernel.run_action(Instant(100), actions::QUICK_PANEL);
    app.settle(effects);
    let (track, shown) = app.slider();
    assert_eq!(shown, "0 %");
    // On touch screens a slider is a fingertip high, in a row of its own
    // across the panel under its title, minus and plus (#435).
    assert!(track.height >= 40, "{track:?}");
    assert!(track.width >= 240, "{track:?}");
    let range = (0, 100, 5);
    let y = i32::from(track.y) + 10;
    // Right of the knob: it does not jump there.
    let start = track.right() as i32 - 20;

    app.count(true);
    app.finger(PointerPhase::Down, start, y);
    assert_eq!(app.slider().1, "0 %");
    // Less far than a tap may wander (3 mm): nothing yet.
    assert_eq!(app.finger(PointerPhase::Move, start + 15, y + 5), None);
    // Moved on in three moves: the knob as far from where it was as the
    // finger from where it came down, the value shown at once with a quick
    // refresh.
    let mut refreshes = Vec::new();
    // A tenth, a fifth and a third of the track.
    let (w, third) = (i32::from(track.width), i32::from(track.width) / 3);
    for dx in [w / 10, w / 5, third] {
        refreshes.push(app.finger(PointerPhase::Move, start + dx, y + 15));
        let want = panel::value_at(track, panel::knob_x(track, 0, (0, 100)) + dx, range);
        assert_eq!(app.slider().1, format!("{want} %"), "{dx}");
    }
    let moved = panel::value_at(track, panel::knob_x(track, 0, (0, 100)) + third, range);
    assert!((20..100).contains(&moved), "{moved}");
    assert!(
        refreshes.iter().all(|r| *r == Some(RefreshMode::Fast)),
        "{refreshes:?}"
    );
    // Shown, not kept yet.
    let kept = app.kernel.settings().global(LIGHT.as_str());
    assert_eq!(kept, Some(&SettingValue::Int(0)));
    // Nothing on the card while the finger is down, drawing included.
    assert_eq!(app.count(false), Vec::<String>::new());
    // Lifted: kept, and the panel drawn cleanly.
    let lifted = app.finger(PointerPhase::Up, start + third, y + 15);
    assert_eq!(lifted, Some(RefreshMode::Partial));
    assert_eq!(app.kernel.settings().int(LIGHT.as_str()), moved);
    assert_eq!(app.slider().1, format!("{moved} %"));

    // Far past either end, no further than the range; again nothing on
    // the card until it is lifted.
    app.count(true);
    app.finger(PointerPhase::Down, start, y);
    app.finger(PointerPhase::Move, start + 2000, y);
    assert_eq!(app.slider().1, "100 %");
    app.finger(PointerPhase::Move, start - 2000, y);
    assert_eq!(app.slider().1, "0 %");
    assert_eq!(app.count(false), Vec::<String>::new());
    app.finger(PointerPhase::Up, start - 2000, y);
    assert_eq!(app.kernel.settings().int(LIGHT.as_str()), 0);

    // A tap on the slider's + (in the row above the track) moves it a step.
    let (px, py) = app.plus_above(track);
    app.finger(PointerPhase::Down, px, py);
    app.finger(PointerPhase::Up, px, py);
    assert_eq!(app.kernel.settings().int(LIGHT.as_str()), 5);
    // A tap on the track sets the value there.
    let middle = i32::from(track.x) + i32::from(track.width) / 2;
    app.finger(PointerPhase::Down, middle, y);
    app.finger(PointerPhase::Up, middle, y);
    assert_eq!(
        app.kernel.settings().int(LIGHT.as_str()),
        panel::value_at(track, middle, range)
    );
}
