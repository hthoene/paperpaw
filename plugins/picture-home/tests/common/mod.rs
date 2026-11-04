//! What the tests of this crate share: the standard app on a storage the
//! test looks into, on a device with buttons only (as the X4) or with
//! touch only, driven by actions, taps, long presses and drags, with a
//! keyboard that answers what the test says it types.

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::rc::Rc;

pub use paperpaw_core::{Effects, Kernel};
pub use paperpaw_platform::{
    ButtonState, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
pub use paperpaw_sdk::builtin::{actions, params, slots};
pub use paperpaw_sdk::semantics::{Part, Role};
pub use paperpaw_sdk::{DirEntry, Id, MemoryStorage, Params, Storage, StorageError};

use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Handled, Manifest, Plugin, Priority, RegisterError, Registrar,
    Screen, ScreenSpec, Version,
};

/// An X4 as the emulator's profile has it: buttons, no touch.
pub const X4: DeviceInfo = DeviceInfo::new(
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

/// A touch screen as large as the X4's, without buttons.
pub const TOUCH: DeviceInfo = X4
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

/// Both devices, for tests that go the same way on each.
pub const DEVICES: [DeviceInfo; 2] = [X4, TOUCH];

/// The configuration every test starts from: English, and on touch
/// screens without the hints over it all.
pub const BASE_CONFIG: &str = "[core]\nlanguage = en\n[help]\nshow = false\n";

/// Where the first-run setup notes that it was done.
pub const SETUP_DONE: &str = ".paperpaw/plugins/setup/done";

/// A storage the test looks into while kernels have it.
#[derive(Clone)]
pub struct Shared(pub Rc<dyn Storage>);

impl Shared {
    /// A storage in memory with `files`, the configuration
    /// [`BASE_CONFIG`] and `config` after it, past the first-run setup.
    pub fn memory(config: &str, files: &[(&str, &[u8])]) -> Self {
        let storage = Self::bare(config, files);
        storage.write(SETUP_DONE, b"").unwrap();
        storage
    }

    /// The same, before the first-run setup.
    pub fn bare(config: &str, files: &[(&str, &[u8])]) -> Self {
        let mut memory = MemoryStorage::new();
        for (path, data) in files {
            memory = memory.with(path, *data);
        }
        let storage = Self(Rc::new(memory));
        storage.set_config(config);
        storage
    }

    /// Writes `paperpaw.ini`: [`BASE_CONFIG`], then `config`.
    pub fn set_config(&self, config: &str) {
        let text = format!("{BASE_CONFIG}{config}");
        self.write(paperpaw_sdk::builtin::CONFIG_FILE, text.as_bytes())
            .unwrap();
    }

    /// The text of the file at `path`, if there is one.
    pub fn text(&self, path: &str) -> Option<String> {
        paperpaw_sdk::read_file(self, path, 1 << 20)
            .ok()
            .map(|data| String::from_utf8_lossy(&data).into_owned())
    }

    /// The configuration file as it is now.
    pub fn config(&self) -> String {
        self.text(paperpaw_sdk::builtin::CONFIG_FILE)
            .unwrap_or_default()
    }

    /// The names in folder `dir`, sorted (folders end with `/`).
    pub fn names(&self, dir: &str) -> Vec<String> {
        let mut names = Vec::new();
        let _ = self.list(dir, &mut |e| {
            names.push(if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.to_string()
            });
        });
        names.sort();
        names
    }
}

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
}

thread_local! {
    /// What the keyboard answers next, one text a question.
    static TYPED: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
    /// The questions asked of the keyboard: what it was opened with.
    static ASKED: RefCell<Vec<Params>> = const { RefCell::new(Vec::new()) };
}

/// The keyboard: shown, it answers with the next text the test typed
/// ([`App::will_type`]), as the real one does when its text is done; with
/// nothing typed it stays open, as if the reader were still typing.
pub struct TestKeyboard;

const KEYBOARD_SCREEN: Id = Id::new_static("test-keyboard.screen");

impl Plugin for TestKeyboard {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("test-keyboard"),
            "Test keyboard",
            Version::new(0, 1, 0),
        )
        .description("Types what the test says.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            KEYBOARD_SCREEN,
            "test-keyboard",
            Box::new(|_, params| {
                ASKED.with(|asked| asked.borrow_mut().push(params.clone()));
                Box::new(Keyboard {
                    params: params.clone(),
                })
            }),
        ))?;
        r.bind_slot(slots::KEYBOARD, KEYBOARD_SCREEN, Priority(100))?;
        Ok(())
    }
}

struct Keyboard {
    params: Params,
}

impl Screen for Keyboard {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let Some(text) = TYPED.with(|typed| typed.borrow_mut().pop_front()) else {
            return;
        };
        cx.command(Command::Back);
        let reply = self
            .params
            .get(params::REPLY)
            .and_then(|r| Id::parse(r).ok());
        if let Some(reply) = reply {
            let typed = Typed::new(text, self.params.clone());
            cx.command(Command::Emit(
                reply,
                paperpaw_sdk::event::EventData::to_params(&typed),
            ));
        }
    }
}

/// The standard app with the test keyboard, over a storage the test keeps.
pub struct App {
    pub kernel: Kernel,
    pub storage: Shared,
    pub now: u64,
    /// The screen as last drawn: whether each pixel is black, row by row.
    ink: Vec<bool>,
}

impl App {
    /// The app on `device` over `storage`, started.
    pub fn new(device: DeviceInfo, storage: Shared) -> Self {
        Self::at(device, storage, 0)
    }

    fn at(device: DeviceInfo, storage: Shared, now: u64) -> Self {
        TYPED.with(|typed| typed.borrow_mut().clear());
        ASKED.with(|asked| asked.borrow_mut().clear());
        let builder = Kernel::builder(device).storage(storage.clone());
        let mut kernel = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs)
            .plugin(TestKeyboard)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(now));
        let display = device.display;
        let mut app = Self {
            kernel,
            storage,
            now,
            ink: vec![false; usize::from(display.width) * usize::from(display.height)],
        };
        app.settle(effects);
        app
    }

    /// A new kernel over the same storage, as after a restart.
    pub fn restart(self) -> Self {
        let device = *self.kernel.device();
        let storage = self.storage.clone();
        let now = self.now + 1000;
        drop(self);
        Self::at(device, storage, now)
    }

    /// Lets timers run until nothing is due within a second.
    pub fn settle(&mut self, mut effects: Effects) {
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

    /// Runs `action` as a button bound to it would.
    pub fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// Runs `action` `times` times.
    pub fn repeat(&mut self, action: Id, times: usize) {
        for _ in 0..times {
            self.run(action.clone());
        }
    }

    /// Opens the screen bound to `slot`.
    pub fn open_slot(&mut self, slot: Id, params: Params) {
        self.now += 100;
        let effects = self.kernel.open_slot(Instant(self.now), slot, params);
        self.settle(effects);
    }

    /// Opens screen `screen`.
    pub fn open_screen(&mut self, screen: Id, params: Params) {
        self.now += 100;
        let effects = self.kernel.open_screen(Instant(self.now), screen, params);
        self.settle(effects);
    }

    /// Sets setting `id` as the settings screen would.
    pub fn set(&mut self, id: &str, value: &str) {
        self.now += 100;
        let effects = self
            .kernel
            .set_setting(Instant(self.now), id, value)
            .unwrap();
        self.settle(effects);
    }

    /// Sends `event` as a plugin would.
    pub fn send(&mut self, event: impl Into<Id>, params: Params) {
        self.now += 100;
        let effects = self
            .kernel
            .send_event(Instant(self.now), event.into(), params);
        self.settle(effects);
    }

    /// A physical button pressed and let go at once.
    pub fn press(&mut self, name: &'static str) {
        self.button(name, ButtonState::Pressed);
        self.now += 50;
        self.button(name, ButtonState::Released);
    }

    /// A physical button held down a second.
    pub fn hold(&mut self, name: &'static str) {
        self.button(name, ButtonState::Pressed);
        self.now += 1000;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
        self.button(name, ButtonState::Released);
    }

    fn button(&mut self, name: &'static str, state: ButtonState) {
        self.now += 50;
        let effects = self
            .kernel
            .handle(Instant(self.now), InputEvent::Button { name, state });
        self.settle(effects);
    }

    /// A finger at `point`, coming down, moving or lifted.
    pub fn finger(&mut self, phase: PointerPhase, point: Point) {
        self.now += 50;
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            point,
            PointerTool::Finger,
            1000,
        ));
        let effects = self.kernel.handle(Instant(self.now), event);
        // Timers wait while the finger is down: a long press is not meant.
        if phase == PointerPhase::Up {
            self.settle(effects);
        }
    }

    /// A tap at `point`.
    pub fn tap(&mut self, point: Point) {
        self.finger(PointerPhase::Down, point);
        self.finger(PointerPhase::Up, point);
    }

    /// A finger held at `point` a second, then lifted.
    pub fn long_press(&mut self, point: Point) {
        self.finger(PointerPhase::Down, point);
        self.now += 1000;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
        self.finger(PointerPhase::Up, point);
    }

    /// A finger put down at `from`, moved to `to` in steps, lifted there.
    pub fn drag(&mut self, from: Point, to: Point) {
        self.finger(PointerPhase::Down, from);
        for step in 1..=8 {
            let x = from.x + (to.x - from.x) * step / 8;
            let y = from.y + (to.y - from.y) * step / 8;
            self.finger(PointerPhase::Move, Point::new(x, y));
        }
        self.finger(PointerPhase::Up, to);
    }

    /// The middle of the part of the screen that says `label` (ignoring
    /// case; else the first that begins with it).
    pub fn find(&mut self, label: &str) -> Option<Point> {
        let parts = self.parts();
        let area = paperpaw_sdk::semantics::find(&parts, label)?.area;
        Some(center(area))
    }

    /// A tap on the part of the screen that says `label`.
    pub fn tap_on(&mut self, label: &str) {
        let at = self.find(label).unwrap_or_else(|| {
            panic!("nothing says {label:?}:\n{}", self.described());
        });
        self.tap(at);
    }

    /// A finger held on the part of the screen that says `label`.
    pub fn long_press_on(&mut self, label: &str) {
        let at = self.find(label).unwrap_or_else(|| {
            panic!("nothing says {label:?}:\n{}", self.described());
        });
        self.long_press(at);
    }

    /// The header of the screen on top (its first heading).
    fn header(&mut self) -> paperpaw_sdk::Rect {
        let parts = self.parts();
        parts
            .iter()
            .find(|p| p.role == Role::Heading && p.area.y == 0)
            .unwrap_or_else(|| panic!("no header:\n{}", self.described()))
            .area
    }

    /// A tap on the back arrow at the left of the header (touch screens).
    pub fn tap_back(&mut self) {
        let header = self.header();
        let side = i32::from(header.height);
        self.tap(Point::new(side / 2, i32::from(header.y) + side / 2));
    }

    /// A tap on the arrow to the next page at the right of the header
    /// (touch screens, lists of several pages).
    pub fn tap_next_page(&mut self) {
        let header = self.header();
        let x = header.right() as i32 - i32::from(header.height) / 3;
        self.tap(Point::new(x, i32::from(header.y + header.height / 2)));
    }

    /// Draws what changed, kept in [`App::dark`].
    pub fn render(&mut self) {
        let width = usize::from(self.kernel.device().display.width);
        let ink = &mut self.ink;
        self.kernel.render(&mut |y, view| {
            for row in 0..view.height {
                for x in 0..view.width {
                    let at = usize::from(y + row) * width + usize::from(x);
                    if let Some(pixel) = ink.get_mut(at) {
                        *pixel = view.pixel(x, row) == paperpaw_platform::Color::BLACK;
                    }
                }
            }
        });
    }

    /// Whether the pixel at `x`, `y` shows black, as drawn now.
    pub fn dark(&mut self, x: i32, y: i32) -> bool {
        self.render();
        let display = self.kernel.device().display;
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return false;
        };
        x < usize::from(display.width)
            && self
                .ink
                .get(y * usize::from(display.width) + x)
                .copied()
                .unwrap_or(false)
    }

    /// The parts the screens shown describe, as drawn now.
    pub fn parts(&mut self) -> Vec<Part> {
        self.render();
        self.kernel.describe()
    }

    /// The labels of the parts shown.
    pub fn labels(&mut self) -> Vec<String> {
        self.parts().into_iter().map(|p| p.label).collect()
    }

    /// Whether some part shown says `text` somewhere in its label.
    pub fn shows(&mut self, text: &str) -> bool {
        self.labels().iter().any(|l| l.contains(text))
    }

    /// The label of the part with the focus, if any.
    pub fn focused(&mut self) -> Option<String> {
        let parts = self.parts();
        paperpaw_sdk::semantics::focused(&parts).map(|p| p.label.clone())
    }

    /// The screen on top.
    pub fn screen(&self) -> String {
        self.kernel
            .active_screen()
            .map_or_else(String::new, |id| id.as_str().to_string())
    }

    /// What the screen shows: its id, then a line for each part.
    pub fn described(&mut self) -> String {
        let parts = self.parts();
        let mut text = format!("screen {}\n", self.screen());
        for part in parts {
            let mut state = String::new();
            if part.focused {
                state.push_str(" (focused)");
            }
            match part.on {
                Some(true) => state.push_str(" (on)"),
                Some(false) => state.push_str(" (off)"),
                None => {}
            }
            let label = part.label.replace('\n', " / ");
            writeln!(text, "{:?} \"{label}\"{state}", part.role).unwrap();
        }
        text
    }

    /// The keyboard answers its next question with `text`.
    pub fn will_type(&self, text: &str) {
        TYPED.with(|typed| typed.borrow_mut().push_back(text.to_string()));
    }

    /// What the keyboard was opened with, question by question.
    pub fn asked(&self) -> Vec<Params> {
        ASKED.with(|asked| asked.borrow().clone())
    }

    /// Back until the home screen shows (at most ten times).
    pub fn home(&mut self) {
        for _ in 0..10 {
            if self.screen() == "home.dashboard" {
                return;
            }
            self.run(actions::BACK);
        }
    }
}

/// The middle of `area`.
pub fn center(area: paperpaw_sdk::Rect) -> Point {
    Point::new(
        i32::from(area.x) + i32::from(area.width) / 2,
        i32::from(area.y) + i32::from(area.height) / 2,
    )
}
