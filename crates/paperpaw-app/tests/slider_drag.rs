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

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rect, RefreshMode, Rotation, Storage,
    StorageError,
};
use paperpaw_sdk::builtin::{actions, menus};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::slider::Slider;
use paperpaw_sdk::{
    Manifest, MemoryStorage, MenuEntry, Plugin, RegisterError, Registrar, SettingSpec,
    SettingValue, Version,
};

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

impl Plugin for Light {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            paperpaw_sdk::Id::new_static("test"),
            "Light",
            Version::new(0, 1, 0),
        )
        .description("A light set with a slider.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
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
            .user_config("[core]\nlanguage = en\n[help]\nshow = false\n");
        let mut kernel = paperpaw_app::install(builder, fonts())
            .plugin(Light)
            .build()
            .unwrap();
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
        self.settle(effects)
    }

    /// The slider's track and the value it shows.
    fn slider(&mut self) -> (Rect, String) {
        let part = self
            .kernel
            .describe()
            .into_iter()
            .find(|p| p.role == Role::Slider)
            .expect("a slider in the quick panel");
        (part.area, part.label)
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
    // On touch screens a slider is a fingertip high and runs across most
    // of the screen.
    assert!(track.height >= 55, "{track:?}");
    assert!(track.width >= 240, "{track:?}");
    let slider = Slider::new(0, 100).step(5);
    let y = i32::from(track.y) + 10;
    // Far right of the knob: it does not jump there.
    let start = track.right() as i32 - 20;

    app.count(true);
    app.finger(PointerPhase::Down, start, y);
    assert_eq!(app.slider().1, "0 %");
    // Less far than a tap may wander (3 mm): nothing yet.
    assert_eq!(app.finger(PointerPhase::Move, start + 20, y + 5), None);
    // Moved 60 to the right, in three moves: the knob as far, the value
    // shown at once with a quick refresh.
    let mut refreshes = Vec::new();
    for x in [start + 30, start + 45, start + 60] {
        refreshes.push(app.finger(PointerPhase::Move, x, y + 15));
    }
    let moved = slider.value_at(track, slider.knob_x(track, 0) + 60);
    assert!(moved >= 20, "{moved}");
    assert_eq!(app.slider().1, format!("{moved} %"));
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
    let lifted = app.finger(PointerPhase::Up, start + 60, y + 15);
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

    // A tap beside the slider's + moves it a step.
    app.finger(PointerPhase::Down, 475, y);
    app.finger(PointerPhase::Up, 475, y);
    assert_eq!(app.kernel.settings().int(LIGHT.as_str()), 5);
}
