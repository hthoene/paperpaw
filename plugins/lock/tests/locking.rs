//! The screen lock with the real kernel, on a device with buttons only
//! (like the X4) and on a touch screen: a PIN, a pattern and a password
//! set up, asked for when PaperPaw starts and when the device wakes up,
//! wrong and right codes, PINs with leading zeros, many wrong tries and
//! the wait after them, changing and removing the lock (only with the
//! code in use), and that the code is kept as a salted hash, never as
//! it is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    ButtonState, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_plugin_lock::Lock;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::{self, Part, Role};
use paperpaw_sdk::{
    Canvas, Cx, DirEntry, Env, Handled, Id, Manifest, MemoryStorage, Params, Plugin, RegisterError,
    Registrar, Screen, ScreenSpec, Storage, StorageError, Version,
};
use proptest::prelude::*;

/// An X4 as the emulator's profile has it: buttons, no touch.
const BUTTONS: DeviceInfo = DeviceInfo::new(
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

/// A touch screen as large as the X4's.
const TOUCH: DeviceInfo = BUTTONS
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

const LOCK_SCREEN: &str = "lock.screen";
/// Where the plugin keeps the code among the secrets.
const SECRET: &str = "lock.code";
/// Wrong tries before the next one waits, and how long.
const TRIES: usize = 5;
const WAIT_MS: u64 = 30_000;

/// A storage the test looks into while kernels have it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

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
}

impl Shared {
    /// Every file on the storage, with its contents.
    fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut found = Vec::new();
        let mut dirs = vec![String::new()];
        while let Some(dir) = dirs.pop() {
            let mut entries = Vec::new();
            let _ = self.0.list(&dir, &mut |e| {
                entries.push((e.name.to_string(), e.is_dir));
            });
            for (name, is_dir) in entries {
                let path = if dir.is_empty() {
                    name
                } else {
                    format!("{dir}/{name}")
                };
                if is_dir {
                    dirs.push(path);
                } else {
                    found.push((path.clone(), self.0.get(&path).unwrap_or_default()));
                }
            }
        }
        found
    }

    /// Whether `text` is in any file, as it is.
    fn contains(&self, text: &str) -> bool {
        self.files()
            .iter()
            .any(|(_, data)| data.windows(text.len()).any(|w| w == text.as_bytes()))
    }
}

/// What another plugin heard: the events it subscribed to and those its
/// screen was told of, with what they carried.
type Heard = Rc<RefCell<Vec<String>>>;

/// Another plugin, honest but curious: it subscribes to the lock's
/// events and keeps a screen open.
struct Spy(Heard);

const SPY_SCREEN: Id = Id::new_static("spy.screen");

struct SpyScreen(Heard);

impl Screen for SpyScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, _: &mut Cx<'_, '_>) {
        self.0.borrow_mut().push(format!("{event} {params:?}"));
    }
}

impl Plugin for Spy {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("spy"), "Spy", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let heard = self.0.clone();
        r.subscribe(
            Id::new_static("lock.typed"),
            Box::new(move |_, params, _| heard.borrow_mut().push(format!("{params:?}"))),
        )?;
        let heard = self.0.clone();
        r.add_screen(ScreenSpec::new(
            SPY_SCREEN,
            "Spy",
            Box::new(move |_, _| Box::new(SpyScreen(heard.clone()))),
        ))
    }
}

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
    heard: Heard,
}

impl Device {
    /// PaperPaw started on `device` with `storage`.
    fn start(device: DeviceInfo, storage: Shared) -> Self {
        Self::start_with(device, storage, "")
    }

    fn start_with(device: DeviceInfo, storage: Shared, config: &str) -> Self {
        let heard = Heard::default();
        let kernel = Kernel::builder(device)
            .storage(storage.clone())
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Keyboard)
            .plugin(Lock)
            .plugin(Spy(heard.clone()))
            .build()
            .unwrap();
        let mut device = Self {
            kernel,
            storage,
            now: 1_000,
            heard,
        };
        let effects = device.kernel.start(Instant(device.now));
        device.settle(effects);
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

    /// Time passes.
    fn wait(&mut self, millis: u64) {
        self.now += millis;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
    }

    fn locked(&self) -> bool {
        self.kernel.screens().any(|id| id.as_str() == LOCK_SCREEN)
    }

    fn on_top(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn parts(&mut self) -> Vec<Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    /// The texts shown (headings, titles, messages).
    fn texts(&mut self) -> Vec<String> {
        self.parts()
            .into_iter()
            .filter(|p| matches!(p.role, Role::Text | Role::Heading))
            .map(|p| p.label)
            .collect()
    }

    fn says(&mut self, text: &str) -> bool {
        self.texts().iter().any(|t| t.contains(text))
    }

    fn finger(&mut self, phase: PointerPhase, point: Point) {
        self.now += 50;
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            point,
            PointerTool::Finger,
            1000,
        ));
        let effects = self.kernel.handle(Instant(self.now), event);
        if phase == PointerPhase::Up {
            self.settle(effects);
        }
    }

    fn center(part: &Part) -> Point {
        Point::new(
            i32::from(part.area.x) + i32::from(part.area.width) / 2,
            i32::from(part.area.y) + i32::from(part.area.height) / 2,
        )
    }

    /// A tap on the part that says exactly `label` (a row or a button).
    fn tap(&mut self, label: &str) {
        let part = self
            .parts()
            .into_iter()
            .rev()
            .find(|p| p.role != Role::Text && p.label == label)
            .unwrap_or_else(|| panic!("nothing says {label:?}"));
        let at = Self::center(&part);
        self.finger(PointerPhase::Down, at);
        self.finger(PointerPhase::Up, at);
    }

    /// Moves the buttons' focus with *Next* to `label` and confirms.
    fn press(&mut self, label: &str) {
        for _ in 0..80 {
            let parts = self.parts();
            if semantics::focused(&parts).map(|p| p.label.as_str()) == Some(label) {
                self.run(actions::CONFIRM);
                return;
            }
            self.run(actions::NEXT);
        }
        panic!("nothing to focus says {label:?}");
    }

    /// Enters a PIN on the keypad and OK, by touch or by buttons.
    fn enter_pin(&mut self, pin: &str) {
        let touch = self.kernel.device().input.touch;
        for digit in pin.chars().map(String::from).chain([String::from("OK")]) {
            if touch {
                self.tap(&digit);
            } else {
                self.press(&digit);
            }
        }
    }

    /// Opens the lock's page in the settings.
    fn open_setup(&mut self) {
        self.now += 100;
        let effects = self.kernel.open_screen(
            Instant(self.now),
            Id::new_static(LOCK_SCREEN),
            Params::new(),
        );
        self.settle(effects);
    }

    /// Chooses a kind on the setup's list.
    fn choose(&mut self, kind: &str) {
        if self.kernel.device().input.touch {
            self.tap(kind);
        } else {
            self.press(kind);
        }
    }

    /// Sets a PIN up through the settings, as the reader does.
    fn set_pin(&mut self, pin: &str) {
        self.open_setup();
        self.choose("PIN");
        self.enter_pin(pin);
        self.enter_pin(pin);
        assert_eq!(self.kind(), "lock.kind.pin");
    }

    /// The lock's kind, as the setting has it.
    fn kind(&self) -> String {
        self.kernel.settings().text("lock.kind").to_string()
    }

    /// The code kept among the secrets.
    fn kept(&self) -> Option<String> {
        self.kernel
            .secrets()
            .get(SECRET)
            .map(std::string::ToString::to_string)
    }

    /// The device sleeps and wakes up.
    fn sleep_and_wake(&mut self) {
        self.now += 100;
        let effects = self.kernel.suspend(Instant(self.now));
        self.settle(effects);
        self.now += 60_000;
        let effects = self.kernel.resume(Instant(self.now));
        self.settle(effects);
    }

    /// The dots of a pattern, in order (buttons without a label).
    fn dots(&mut self) -> Vec<Point> {
        self.parts()
            .iter()
            .filter(|p| p.role == Role::Button && p.label.is_empty())
            .map(Self::center)
            .collect()
    }

    /// Draws a pattern through `dots` (indices `0` to `8`, row by row).
    fn draw(&mut self, dots: &[usize]) {
        let points = self.dots();
        assert_eq!(points.len(), 9);
        self.finger(PointerPhase::Down, points[dots[0]]);
        for &dot in &dots[1..] {
            self.finger(PointerPhase::Move, points[dot]);
        }
        self.finger(PointerPhase::Up, points[*dots.last().unwrap()]);
    }

    /// Types `text` on the keyboard by touch and finishes.
    fn type_text(&mut self, text: &str) {
        assert_eq!(self.on_top(), Some("keyboard.screen"));
        for c in text.chars() {
            if c.is_uppercase() {
                self.tap("Shift");
            }
            self.tap(&c.to_string());
        }
        self.tap("OK");
    }
}

/// The kept code's parts: kind, salt and hash.
fn split(kept: &str) -> (String, String, String) {
    let mut parts = kept.splitn(3, ':').map(String::from);
    (
        parts.next().unwrap(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    )
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn no_lock_by_default() {
    let device = Device::start(TOUCH, Shared::default());
    assert!(!device.locked());
    assert_eq!(device.kind(), "lock.kind.off");
}

#[test]
fn pin_by_touch_is_asked_for_at_start_and_kept_as_a_hash() {
    let storage = Shared::default();
    let mut device = Device::start(TOUCH, storage.clone());
    device.set_pin("0012");
    assert!(!device.locked());
    let (kind, salt, hash) = split(&device.kept().unwrap());
    assert_eq!(kind, "pin");
    // A salt of its own, not of the code: `v2.` and 16 random letters.
    let letters = salt.strip_prefix("v2.").unwrap_or_else(|| panic!("{salt}"));
    assert!(
        letters.len() == 16 && letters.bytes().all(|b| b.is_ascii_alphanumeric()),
        "{salt}"
    );
    assert!(is_hex(&hash, 40), "{hash}");
    assert!(!device.storage.contains("pin:0012"));

    // PaperPaw starts again: locked.
    let mut device = Device::start(TOUCH, storage);
    assert!(device.locked());
    assert_eq!(device.on_top(), Some(LOCK_SCREEN));
    assert!(device.says("Enter the PIN"));
    // The leading zeros count.
    device.enter_pin("12");
    assert!(device.locked());
    assert!(device.says("That was not right."));
    device.enter_pin("012");
    assert!(device.locked());
    device.enter_pin("00012");
    assert!(device.locked());
    device.enter_pin("0012");
    assert!(!device.locked());
}

#[test]
fn pin_by_buttons_only() {
    let storage = Shared::default();
    let mut device = Device::start(BUTTONS, storage.clone());
    device.set_pin("9087");
    let mut device = Device::start(BUTTONS, storage);
    assert!(device.locked());
    device.enter_pin("9088");
    assert!(device.locked());
    device.enter_pin("9087");
    assert!(!device.locked());
}

#[test]
fn back_takes_the_last_digit_and_never_unlocks() {
    let storage = Shared::default();
    Device::start(BUTTONS, storage.clone()).set_pin("1234");
    let mut device = Device::start(BUTTONS, storage);
    device.press("1");
    device.press("2");
    device.press("5");
    device.run(actions::BACK);
    device.press("3");
    device.press("4");
    device.press("OK");
    assert!(!device.locked());

    let storage = Shared::default();
    Device::start(BUTTONS, storage.clone()).set_pin("1234");
    let mut device = Device::start(BUTTONS, storage);
    // Back, home and menu on an empty code: still locked.
    for action in [actions::BACK, actions::BACK, actions::HOME, actions::MENU] {
        device.run(action.clone());
        assert_eq!(device.on_top(), Some(LOCK_SCREEN), "{action}");
    }
}

/// The plugin says a screen that would open above the lock (a rule, a
/// timer) closes it and the lock opens again at once; but a screen
/// opened above it is shown and used above the lock, which stays below.
#[test]
fn a_screen_opened_while_locked_does_not_get_past_the_lock() {
    let storage = Shared::default();
    Device::start(TOUCH, storage.clone()).set_pin("1234");
    let mut device = Device::start(TOUCH, storage);
    assert!(device.locked());
    // A full screen, as a rule or a timer might open.
    device.now += 100;
    let effects = device.kernel.open_screen(
        Instant(device.now),
        Id::new_static("keyboard.choose"),
        Params::new(),
    );
    device.settle(effects);
    assert_eq!(device.on_top(), Some(LOCK_SCREEN));
    device.enter_pin("1234");
    assert!(!device.locked());
}

#[test]
fn many_wrong_tries_make_the_next_one_wait() {
    let storage = Shared::default();
    Device::start(TOUCH, storage.clone()).set_pin("2468");
    let mut device = Device::start(TOUCH, storage);
    for _ in 0..TRIES {
        device.enter_pin("1111");
    }
    assert!(device.says("Too many tries."));
    // While waiting, even the right PIN is not taken.
    device.enter_pin("2468");
    assert!(device.locked());
    device.wait(WAIT_MS / 2);
    device.enter_pin("2468");
    assert!(device.locked());
    // After the wait, it is.
    device.wait(WAIT_MS);
    assert!(!device.says("Too many tries"));
    device.enter_pin("2468");
    assert!(!device.locked());
}

/// The seconds left are counted up from whole seconds plus one: the
/// wait of 30 s shows as 31 s when it begins (and 1 s as 2 s).
#[test]
fn the_wait_shows_the_seconds_left() {
    let storage = Shared::default();
    Device::start(TOUCH, storage.clone()).set_pin("2468");
    let mut device = Device::start(TOUCH, storage);
    for _ in 0..TRIES {
        device.enter_pin("1111");
    }
    assert!(device.says("Wait 30 s."), "{:?}", device.texts());
    // Counted down (the screen is drawn again each second) and never to
    // 0 s, then the PIN is asked for again.
    let mut last = String::new();
    for _ in 0..200 {
        device.wait(250);
        let texts = device.texts();
        match texts.iter().find(|t| t.contains("Wait")) {
            Some(wait) => {
                assert!(!wait.ends_with("Wait 0 s."), "{wait:?}");
                last = wait.clone();
            }
            None => break,
        }
    }
    assert!(
        last.ends_with("Wait 1 s.") || last.ends_with("Wait 2 s."),
        "{last:?}"
    );
    assert!(!device.says("Wait"), "{:?}", device.texts());
}

#[test]
fn the_wait_comes_again_after_five_more_wrong_tries() {
    let storage = Shared::default();
    Device::start(BUTTONS, storage.clone()).set_pin("2468");
    let mut device = Device::start(BUTTONS, storage);
    for _ in 0..TRIES {
        device.enter_pin("1357");
    }
    device.wait(WAIT_MS + 1000);
    for _ in 0..TRIES - 1 {
        device.enter_pin("1357");
        assert!(!device.says("Too many tries"));
    }
    device.enter_pin("1357");
    assert!(device.says("Too many tries"));
}

#[test]
fn waking_up_locks_unless_switched_off() {
    let storage = Shared::default();
    let mut device = Device::start(TOUCH, storage.clone());
    device.set_pin("5555");
    assert!(!device.locked());
    device.sleep_and_wake();
    assert!(device.locked());
    device.enter_pin("5555");
    assert!(!device.locked());
    // Sleeping again while locked opens one lock screen, not two.
    device.sleep_and_wake();
    device.sleep_and_wake();
    let open = device
        .kernel
        .screens()
        .filter(|id| id.as_str() == LOCK_SCREEN)
        .count();
    assert_eq!(open, 1);
    device.enter_pin("5555");
    assert!(!device.locked());

    // Not when waking, only at the start.
    let mut device = Device::start_with(TOUCH, storage, "[lock]\non-wake = off\n");
    device.enter_pin("5555");
    device.sleep_and_wake();
    assert!(!device.locked());
}

#[test]
fn lock_now_locks_at_once() {
    let mut device = Device::start(BUTTONS, Shared::default());
    // Without a lock set up, nothing.
    device.run(Id::new_static("lock.now"));
    assert!(!device.locked());
    device.set_pin("4321");
    device.run(Id::new_static("lock.now"));
    assert!(device.locked());
    device.enter_pin("4321");
    assert!(!device.locked());
}

#[test]
fn a_new_pin_needs_four_digits_and_the_same_twice() {
    let mut device = Device::start(TOUCH, Shared::default());
    device.open_setup();
    device.choose("PIN");
    device.enter_pin("123");
    assert!(device.says("At least 4"));
    device.enter_pin("1234");
    assert!(device.says("Once more"));
    device.enter_pin("1235");
    assert!(device.says("They were not the same"));
    // From the start: new, then again.
    device.enter_pin("6789");
    device.enter_pin("6789");
    assert_eq!(device.kind(), "lock.kind.pin");
    assert_ne!(device.on_top(), Some(LOCK_SCREEN));
    device.run(Id::new_static("lock.now"));
    device.enter_pin("1234");
    assert!(device.locked());
    device.enter_pin("6789");
    assert!(!device.locked());
}

#[test]
fn changing_or_removing_the_lock_needs_the_code_in_use() {
    let mut device = Device::start(TOUCH, Shared::default());
    device.set_pin("1470");
    device.open_setup();
    assert!(device.says("Enter the current PIN"));
    device.enter_pin("0741");
    // Still asked for it, no list of kinds.
    assert!(device.parts().iter().all(|p| p.label != "Off"));
    device.enter_pin("1470");
    device.choose("Off");
    assert_eq!(device.kind(), "lock.kind.off");
    assert_eq!(device.kept(), None);
    // Nothing asked for any more.
    device.run(Id::new_static("lock.now"));
    assert!(!device.locked());
    device.sleep_and_wake();
    assert!(!device.locked());
}

#[test]
fn the_kept_code_and_the_setting_must_agree() {
    // A code kept for a PIN, the setting changed by hand to a password:
    // no lock that cannot be opened.
    let storage = Shared::default();
    Device::start(TOUCH, storage.clone()).set_pin("1234");
    let device = Device::start_with(TOUCH, storage, "[lock]\nkind = lock.kind.password\n");
    assert!(!device.locked());
}

#[test]
fn a_pattern_drawn_with_a_finger() {
    let storage = Shared::default();
    let mut device = Device::start(TOUCH, storage.clone());
    device.open_setup();
    device.choose("Pattern");
    // Too few dots.
    device.draw(&[0, 1, 2]);
    assert!(device.says("At least 4"));
    device.draw(&[0, 1, 2, 5, 8]);
    device.draw(&[0, 1, 2, 5, 8]);
    assert_eq!(device.kind(), "lock.kind.pattern");
    let (kind, _, hash) = split(&device.kept().unwrap());
    assert_eq!(kind, "pattern");
    assert!(is_hex(&hash, 40));

    let mut device = Device::start(TOUCH, storage);
    assert!(device.says("Draw the pattern"));
    // The same dots in another order are another pattern.
    device.draw(&[8, 5, 2, 1, 0]);
    assert!(device.locked());
    device.draw(&[0, 1, 2, 5]);
    assert!(device.locked());
    // Passing a dot again counts it once: the same pattern.
    device.draw(&[0, 1, 0, 2, 5, 8]);
    assert!(!device.locked());
}

#[test]
fn patterns_are_offered_only_on_touch_screens() {
    let mut device = Device::start(BUTTONS, Shared::default());
    device.open_setup();
    let rows: Vec<String> = device
        .parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label)
        .collect();
    assert_eq!(rows, ["Off", "PIN", "Password"]);
}

#[test]
fn a_password_typed_on_the_keyboard_is_never_kept_in_clear() {
    let storage = Shared::default();
    let mut device = Device::start(TOUCH, storage.clone());
    device.open_setup();
    device.choose("Password");
    device.type_text("Secretpass");
    device.type_text("Secretpass");
    assert_eq!(device.kind(), "lock.kind.password");
    let kept = device.kept().unwrap();
    assert!(!kept.contains("Secretpass"), "{kept}");
    assert!(!storage.contains("Secretpass"));
    assert!(!storage.contains("ecretpas"));

    let mut device = Device::start(TOUCH, storage);
    assert!(device.locked());
    // The keyboard is asked at once, for a password (shown as dots).
    device.type_text("secretpass");
    assert!(device.locked());
    assert!(device.says("That was not right."));
    device.tap("Type the password");
    device.type_text("Secretpass");
    assert!(!device.locked());
}

/// While the lock guards the device, what is typed to unlock it goes to
/// the lock and its keyboard alone: no other plugin's subscription, nor
/// a screen below the lock, hears it.
#[test]
fn a_password_typed_to_unlock_reaches_no_other_plugin() {
    let storage = Shared::default();
    let mut device = Device::start(TOUCH, storage.clone());
    device.open_setup();
    device.choose("Password");
    device.type_text("Secretpass");
    device.type_text("Secretpass");

    let mut device = Device::start(TOUCH, storage);
    assert!(device.locked());
    device.now += 100;
    let effects = device
        .kernel
        .open_screen(Instant(device.now), SPY_SCREEN, Params::new());
    device.settle(effects);
    assert!(device.kernel.screens().any(|id| *id == SPY_SCREEN));
    device.heard.borrow_mut().clear();
    device.type_text("wrongpass");
    device.tap("Type the password");
    device.type_text("Secretpass");
    assert!(!device.locked());
    let heard = device.heard.borrow();
    assert!(
        !heard
            .iter()
            .any(|h| h.contains("pass") || h.contains("lock.typed")),
        "{heard:?}"
    );
}

/// A plugin whose power mode would switch the lock off, and that wears
/// it at start.
struct Thrifty;

static THRIFTY: [paperpaw_sdk::presets::Preset; 1] = [paperpaw_sdk::presets::Preset::fixed(
    "thrifty.mode",
    "core.power-mode",
    "Thrifty",
    "",
    "lock.kind = lock.kind.off
lock.on-wake = off
core.sleep-after = 3",
)];

impl Plugin for Thrifty {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("thrifty"), "Thrifty", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_presets(&THRIFTY)?;
        r.subscribe(
            paperpaw_sdk::builtin::events::STARTED.id().clone(),
            Box::new(|_, _, out| {
                out.push(paperpaw_sdk::Command::Wear(paperpaw_sdk::power::Wear::new(
                    Id::new_static("thrifty.mode"),
                )));
            }),
        )
    }
}

/// A power mode wears its values over the reader's own, but not the
/// lock's settings.
#[test]
fn a_power_mode_leaves_the_lock_as_it_is() {
    let storage = Shared::default();
    Device::start(BUTTONS, storage.clone()).set_pin("1234");
    let kernel = Kernel::builder(BUTTONS)
        .storage(storage.clone())
        .user_config(String::from(
            "[core]
language = en
",
        ))
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Keyboard)
        .plugin(Lock)
        .plugin(Thrifty)
        .build()
        .unwrap();
    let mut device = Device {
        kernel,
        storage,
        now: 1_000,
        heard: Heard::default(),
    };
    let effects = device.kernel.start(Instant(device.now));
    device.settle(effects);
    assert_eq!(device.kernel.power().mode.as_str(), "thrifty.mode");
    assert_eq!(device.kernel.settings().int("core.sleep-after"), 3);
    assert_eq!(device.kind(), "lock.kind.pin");
    assert!(device.kernel.settings().bool("lock.on-wake"));
    assert!(device.locked());
}

/// A PIN lock whose kept code is `value`, PaperPaw started over it.
fn with_kept_code(value: &str) -> Device {
    const PIN: &str = "[lock]\nkind = lock.kind.pin\n";
    let storage = Shared::default();
    let mut device = Device::start_with(TOUCH, storage.clone(), PIN);
    device.kernel.set_secret(
        paperpaw_sdk::secrets::SecretChange::keep(SECRET, "lock.secret", value).changeable(false),
    );
    Device::start_with(TOUCH, storage, PIN)
}

#[test]
fn a_kept_code_without_a_kind_locks_nothing() {
    assert!(!with_kept_code("pin").locked());
    assert!(!with_kept_code("").locked());
    // Another kind than the setting's: no lock either.
    assert!(!with_kept_code("password:00:00").locked());
}

/// A damaged kept code (no salt, no hash) still opens the lock screen,
/// as the kinds agree, but no code ever matches it: the device is locked
/// for good, where a code without a kind locks nothing.
#[test]
fn a_damaged_kept_code_does_not_lock_for_good() {
    for damaged in ["pin:", "pin:salt", "pin::", "pin:salt:"] {
        let device = with_kept_code(damaged);
        assert!(!device.locked(), "{damaged:?} locks for good");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(24)))]

    /// Any PIN of four to sixteen digits unlocks, and any other does not.
    #[test]
    fn any_pin_unlocks_and_no_other(pin in "[0-9]{4,16}", other in "[0-9]{1,16}") {
        let storage = Shared::default();
        Device::start(TOUCH, storage.clone()).set_pin(&pin);
        let mut device = Device::start(TOUCH, storage);
        if other != pin {
            device.enter_pin(&other);
            prop_assert!(device.locked());
        }
        device.enter_pin(&pin);
        prop_assert!(!device.locked());
    }
}

/// While the device is locked, no other plugin changes how it is set up:
/// a setting that would switch the lock off, its secret, the plugin
/// itself. The lock holds.
mod setup_under_the_lock {
    use super::*;
    use paperpaw_sdk::builtin::events;
    use paperpaw_sdk::secrets::SecretChange;
    use paperpaw_sdk::{
        Command, Manifest, Plugin, RegisterError, Registrar, SettingValue, Version,
    };

    /// Hears the clock and tries to undo the lock.
    struct Intruder;

    impl Plugin for Intruder {
        fn manifest(&self) -> Manifest {
            Manifest::new(
                Id::new_static("intruder"),
                "Intruder",
                Version::new(1, 0, 0),
            )
        }

        fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
            r.on(events::STATUS_CHANGED, |_, _, out| {
                out.push(Command::SetSetting(
                    Id::new_static("lock.kind"),
                    SettingValue::Text("lock.kind.off".into()),
                ));
                out.push(Command::ResetSetting(Id::new_static("lock.kind")));
                out.push(Command::set_secret(SecretChange::remove("lock.code")));
                out.push(Command::SetPluginEnabled(Id::new_static("lock"), false));
                out.push(Command::RemovePlugin(Id::new_static("lock")));
                out.push(Command::LoadPlugin(String::from("plugins/intruder")));
            })
        }
    }

    #[test]
    fn nothing_changes_the_setup_under_the_lock() {
        let storage = Shared::default();
        Device::start(BUTTONS, storage.clone()).set_pin("1234");
        let kept = Device::start(BUTTONS, storage.clone()).kept();
        let kernel = Kernel::builder(BUTTONS)
            .storage(storage.clone())
            .user_config(String::from("[core]\nlanguage = en\n"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Keyboard)
            .plugin(Lock)
            .plugin(Intruder)
            .build()
            .unwrap();
        let mut device = Device {
            kernel,
            storage,
            now: 1_000,
            heard: Heard::default(),
        };
        let effects = device.kernel.start(Instant(device.now));
        device.settle(effects);
        assert!(device.locked());
        let kind = device.kind();
        device.now += 100;
        let status = paperpaw_platform::SystemStatus::new(
            Some(paperpaw_platform::WallClock::new(
                Instant(device.now),
                1_700_000_000_000,
                None,
            )),
            None,
        );
        let effects = device.kernel.set_status(Instant(device.now), status);
        device.settle(effects);
        device.run(actions::NEXT);
        assert!(device.locked(), "on top: {:?}", device.on_top());
        assert_eq!(device.kind(), kind);
        assert_eq!(device.kept(), kept);
    }
}

/// A sleep screen of another plugin, over the lock as the device sleeps,
/// gets no input, and closes as the device wakes: it sees nothing of the
/// PIN typed into the lock.
mod sleep_screen_over_the_lock {
    use super::*;
    use paperpaw_sdk::builtin::slots;
    use paperpaw_sdk::{
        Canvas, Cx, Env, Handled, Manifest, Plugin, Priority, RegisterError, Registrar, Screen,
        ScreenSpec, Version,
    };
    use std::cell::RefCell;

    type Seen = Rc<RefCell<Vec<String>>>;

    struct Watcher(Seen);

    impl Screen for Watcher {
        fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

        fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
            self.0.borrow_mut().push(action.as_str().to_string());
            Handled::Yes
        }
    }

    struct Sleepy(Seen);

    impl Plugin for Sleepy {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static("sleepy"), "Sleepy", Version::new(1, 0, 0))
        }

        fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
            let seen = self.0.clone();
            r.add_screen(ScreenSpec::new(
                Id::new_static("sleepy.screen"),
                "sleepy",
                Box::new(move |_, _| Box::new(Watcher(seen.clone()))),
            ))?;
            r.bind_slot(slots::SLEEP, Id::new_static("sleepy.screen"), Priority(100))
        }
    }

    #[test]
    fn a_sleep_screen_sees_nothing_of_the_pin() {
        let storage = Shared::default();
        Device::start(BUTTONS, storage.clone()).set_pin("1234");
        let seen = Seen::default();
        let kernel = Kernel::builder(BUTTONS)
            .storage(storage.clone())
            .sleep(true)
            .user_config(String::from("[core]\nlanguage = en\n"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Keyboard)
            .plugin(Lock)
            .plugin(Sleepy(seen.clone()))
            .build()
            .unwrap();
        let mut device = Device {
            kernel,
            storage,
            now: 1_000,
            heard: Heard::default(),
        };
        let effects = device.kernel.start(Instant(device.now));
        device.settle(effects);
        assert!(device.locked());
        device.run(actions::SLEEP);
        assert_eq!(device.on_top(), Some("sleepy.screen"));
        device.run(actions::NEXT);
        device.run(actions::DOWN);
        assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
        device.now += 100;
        let effects = device.kernel.resume(Instant(device.now));
        device.settle(effects);
        assert_eq!(device.on_top(), Some(LOCK_SCREEN));
        device.enter_pin("1234");
        assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
        assert!(!device.locked());
    }

    /// A shell that shows the sleep screen and never says the device woke
    /// does not lock the reader out: a button closes the sleep screen and
    /// the lock takes input again.
    #[test]
    fn a_button_closes_a_sleep_screen_left_over_the_lock() {
        let storage = Shared::default();
        Device::start(BUTTONS, storage.clone()).set_pin("1234");
        let seen = Seen::default();
        let kernel = Kernel::builder(BUTTONS)
            .storage(storage.clone())
            .sleep(true)
            .user_config(String::from(
                "[core]
language = en
",
            ))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Keyboard)
            .plugin(Lock)
            .plugin(Sleepy(seen.clone()))
            .build()
            .unwrap();
        let mut device = Device {
            kernel,
            storage,
            now: 1_000,
            heard: Heard::default(),
        };
        let effects = device.kernel.start(Instant(device.now));
        device.settle(effects);
        device.run(actions::SLEEP);
        assert_eq!(device.on_top(), Some("sleepy.screen"));
        for state in [ButtonState::Pressed, ButtonState::Released] {
            device.now += 50;
            let event = InputEvent::Button {
                name: "confirm",
                state,
            };
            let effects = device.kernel.handle(Instant(device.now), event);
            device.settle(effects);
        }
        assert_eq!(device.on_top(), Some(LOCK_SCREEN));
        assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
        device.enter_pin("1234");
        assert!(!device.locked());
    }
}
