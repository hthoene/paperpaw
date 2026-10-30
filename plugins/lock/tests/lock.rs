//! The screen lock with the real kernel and the sleep screen: set up and
//! unlocked with the buttons of an X4 and by touch, a locked device going
//! to sleep and waking, wrong tries, and the lock changed or removed
//! elsewhere.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    ButtonState, InputEvent, Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_plugin_lock::{Lock, NOW};
use paperpaw_plugin_sleep::Sleep;
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DisplayInfo, Env, EventData, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, Priority, RegisterError, Registrar,
    Screen, ScreenSpec, Storage, Version,
};

const LOCK: Id = Id::new_static("lock.screen");
const SLEEPING: Id = Id::new_static("sleep.screen");
const KEYBOARD: Id = Id::new_static("keyboard.screen");
const PANEL: Id = Id::new_static("test.panel");

/// An X4: buttons, no touch.
const X4: DeviceInfo = device(false);
/// A Boox: touch and a few buttons.
const BOOX: DeviceInfo = device(true);

const fn device(touch: bool) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, touch, false, &[]),
        MemoryClass::Large,
    )
}

/// A quick panel that opens where asked, to see whether it does.
struct Panel;

struct PanelScreen;

impl Screen for PanelScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Panel {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("test"), "Panel", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            PANEL,
            "test.panel",
            Box::new(|_, _| Box::new(PanelScreen)),
        ))?;
        r.bind_slot(slots::PANEL, PANEL, Priority::DEFAULT)
    }
}

struct Test {
    kernel: Kernel,
    now: u64,
    /// The key of the keypad chosen with the buttons.
    focus: usize,
}

impl Test {
    fn new(device: DeviceInfo) -> Self {
        Self::with(device, MemoryStorage::new())
    }

    fn with(device: DeviceInfo, storage: MemoryStorage) -> Self {
        let mut kernel = Kernel::builder(device)
            .storage(storage)
            .random(random)
            .sleep(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFonts)
            .plugin(Lock)
            .plugin(Sleep)
            .plugin(Keyboard)
            .plugin(Panel)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            now: 0,
            focus: 0,
        }
    }

    /// The same device started anew, with what its card keeps.
    fn restart(self) -> Self {
        let device = *self.kernel.device();
        let mut storage = MemoryStorage::new();
        copy(self.kernel.storage(), "", &mut storage);
        Self::with(device, storage)
    }

    fn at(&mut self) -> Instant {
        self.now += 200;
        Instant(self.now)
    }

    fn top(&self) -> Option<&Id> {
        self.kernel.active_screen()
    }

    fn open(&self, screen: &Id) -> bool {
        self.kernel.screens().any(|s| s == screen)
    }

    fn render(&mut self) {
        self.kernel.render(&mut |_, _| {});
    }

    /// A short press of button `name`.
    fn press(&mut self, name: &'static str) -> Effects {
        let at = self.at();
        let pressed = self.kernel.handle(
            at,
            InputEvent::Button {
                name,
                state: ButtonState::Pressed,
            },
        );
        let released = self.kernel.handle(
            at.add_millis(50),
            InputEvent::Button {
                name,
                state: ButtonState::Released,
            },
        );
        self.render();
        if pressed.sleep() { pressed } else { released }
    }

    fn run(&mut self, action: Id) -> Effects {
        let at = self.at();
        let effects = self.kernel.run_action(at, action);
        self.render();
        effects
    }

    fn pointer(&mut self, at: Instant, phase: PointerPhase, (x, y): (i32, i32)) {
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ));
        self.kernel.handle(at, event);
    }

    fn tap(&mut self, point: (i32, i32)) {
        let at = self.at();
        self.pointer(at, PointerPhase::Down, point);
        self.pointer(at.add_millis(60), PointerPhase::Up, point);
        self.render();
    }

    fn swipe(&mut self, from: (i32, i32), to: (i32, i32)) {
        let at = self.at();
        self.pointer(at, PointerPhase::Down, from);
        self.pointer(at.add_millis(30), PointerPhase::Move, to);
        self.pointer(at.add_millis(60), PointerPhase::Up, to);
        self.render();
    }

    /// Taps the button labelled `label`, where the screen shows it.
    fn tap_button(&mut self, label: &str) {
        let part = self
            .kernel
            .describe()
            .into_iter()
            .find(|p| p.role == Role::Button && p.label == label)
            .unwrap_or_else(|| panic!("no button `{label}` in {:?}", self.kernel.describe()));
        let area = part.area;
        let centre = (
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        );
        self.tap(centre);
    }

    /// Moves the keypad's focus to key `index` with the buttons.
    fn key(&mut self, index: usize) {
        while self.focus != index {
            self.press("right");
            self.focus = (self.focus + 1) % 12;
        }
    }

    /// Enters `code` on the keypad with the buttons, then *OK*.
    fn enter(&mut self, code: &str) {
        for digit in code.chars() {
            let index = match digit {
                '0' => 10,
                d => d.to_digit(10).unwrap() as usize - 1,
            };
            self.key(index);
            self.press("confirm");
        }
        self.key(11);
        self.press("confirm");
    }

    /// Enters `code` on the keypad by touch, then *OK*.
    fn tap_code(&mut self, code: &str) {
        for digit in code.chars() {
            self.tap_button(&digit.to_string());
        }
        self.tap_button("OK");
    }

    /// A lock screen opens: its keypad's focus starts on the first key.
    fn opened(&mut self) {
        self.focus = 0;
    }

    /// Sets up a PIN with the buttons, on the lock's own page.
    fn set_up_pin(&mut self, code: &str) {
        let at = self.at();
        self.kernel.open_screen(at, LOCK, Params::new());
        // Off, PIN, password: the PIN.
        self.press("down");
        self.press("confirm");
        self.opened();
        self.enter(code);
        self.enter(code);
        assert!(!self.open(&LOCK), "set up and closed");
        assert_eq!(
            self.kernel.settings().text("lock.kind"),
            "lock.kind.pin",
            "set up"
        );
    }

    /// Answers the keyboard for the lock, as typing there would.
    fn typed(&mut self, text: &str) {
        let at = self.at();
        let typed = Typed::new(text.to_string(), Params::new());
        self.kernel
            .send_event(at, Id::new_static("lock.typed"), typed.to_params());
        self.render();
    }

    /// Sets up a password, typed on the keyboard.
    fn set_up_password(&mut self, password: &str) {
        let at = self.at();
        self.kernel.open_screen(at, LOCK, Params::new());
        self.tap_button_row("Password");
        for _ in 0..2 {
            assert_eq!(self.top(), Some(&KEYBOARD), "asked for");
            self.press("back");
            self.typed(password);
        }
        assert!(!self.open(&LOCK), "set up and closed");
    }

    /// Taps the row of a list that says `label`.
    fn tap_button_row(&mut self, label: &str) {
        let part = self
            .kernel
            .describe()
            .into_iter()
            .find(|p| p.label == label)
            .unwrap_or_else(|| panic!("no row `{label}`"));
        let area = part.area;
        self.tap((
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        ));
    }

    fn lock_now(&mut self) {
        self.run(NOW);
        assert!(self.open(&LOCK), "locked");
        self.opened();
    }

    /// The device sleeps by the power button, then wakes.
    fn sleep_and_wake(&mut self) {
        let effects = self.press("power");
        assert!(effects.sleep(), "asleep");
        assert_eq!(self.top(), Some(&SLEEPING));
        let at = self.at();
        self.kernel.suspend(at);
        let at = self.at();
        self.kernel.resume(at);
        self.render();
    }
}

/// Random bytes, other ones every time.
fn random(buf: &mut [u8]) -> bool {
    static NEXT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(7);
    for byte in buf {
        *byte = NEXT.fetch_add(37, std::sync::atomic::Ordering::Relaxed);
    }
    true
}

/// The lock's hash of `code` with `salt`: a salted SHA-1, 2000 times.
fn hashed(salt: &str, code: &str) -> String {
    use paperpaw_sdk::digest::sha1::{Sha1, hex};
    let mut sha = Sha1::new();
    sha.update(salt.as_bytes());
    sha.update(code.as_bytes());
    let mut digest = sha.finish();
    for _ in 1..2000 {
        let mut sha = Sha1::new();
        sha.update(salt.as_bytes());
        sha.update(&digest);
        digest = sha.finish();
    }
    hex(&digest)
}

/// Copies every file of `dir` from `from` to `to`.
fn copy(from: &dyn Storage, dir: &str, to: &mut MemoryStorage) {
    let mut entries = Vec::new();
    from.list(dir, &mut |entry| {
        entries.push((entry.name.to_string(), entry.is_dir));
    })
    .unwrap();
    for (name, is_dir) in entries {
        let path = if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        };
        if is_dir {
            copy(from, &path, to);
        } else {
            let data = paperpaw_sdk::read_file(from, &path, 1 << 20).unwrap();
            to.write(&path, &data).unwrap();
        }
    }
}

#[test]
fn a_locked_x4_sleeps_by_the_power_button_and_by_itself() {
    let mut test = Test::new(X4);
    test.set_up_pin("2580");
    test.lock_now();

    // The power button.
    let effects = test.press("power");
    assert!(effects.sleep(), "the power button puts it to sleep");
    assert_eq!(test.top(), Some(&SLEEPING));
    let at = test.at();
    test.kernel.suspend(at);
    let at = test.at();
    test.kernel.resume(at);
    assert_eq!(test.top(), Some(&LOCK), "still locked when it wakes");

    // By itself, after a minute without use.
    let at = test.at();
    test.kernel
        .set_setting(at, "core.sleep-after", "1")
        .unwrap();
    test.now += 61_000;
    let effects = test.kernel.tick(Instant(test.now));
    assert!(effects.sleep(), "asleep by itself");
    assert_eq!(test.top(), Some(&SLEEPING));
}

#[test]
fn refresh_and_night_mode_pass_the_lock() {
    let mut test = Test::new(X4);
    test.set_up_pin("2580");
    test.lock_now();
    test.run(actions::TOGGLE_NIGHT_MODE);
    assert!(test.kernel.settings().bool("core.night-mode"));
    // Anything else stays the lock's.
    test.run(actions::QUICK_PANEL);
    assert_eq!(test.top(), Some(&LOCK));
}

#[test]
fn unlocking_after_waking_leaves_the_device_usable_by_buttons() {
    let mut test = Test::new(X4);
    test.set_up_pin("2580");
    test.sleep_and_wake();
    assert_eq!(test.top(), Some(&LOCK), "locked as it woke");
    test.opened();
    test.enter("2580");
    assert!(!test.open(&LOCK), "unlocked");
    assert!(!test.open(&SLEEPING), "the sleep screen went too");
    // The buttons work again: the quick panel opens.
    test.run(actions::QUICK_PANEL);
    assert_eq!(test.top(), Some(&PANEL));
}

#[test]
fn unlocking_after_waking_leaves_the_device_usable_by_touch() {
    let mut test = Test::new(BOOX);
    test.set_up_pin("2580");
    test.sleep_and_wake();
    assert_eq!(test.top(), Some(&LOCK), "locked as it woke");
    test.tap_code("2580");
    assert!(!test.open(&LOCK), "unlocked");
    assert!(!test.open(&SLEEPING), "the sleep screen went too");
    // A swipe down from the top opens the quick panel again.
    test.swipe((240, 2), (240, 400));
    assert_eq!(test.top(), Some(&PANEL));
}

#[test]
fn nothing_opens_over_a_password_being_typed_to_unlock() {
    let mut test = Test::new(BOOX);
    test.set_up_password("secret words");
    test.lock_now();
    assert_eq!(test.top(), Some(&KEYBOARD), "the keyboard asks at once");

    // A swipe down from the top edge, and the quick panel's button.
    test.swipe((240, 2), (240, 400));
    assert!(!test.open(&PANEL), "no quick panel by the edge");
    test.run(actions::QUICK_PANEL);
    test.run(actions::SETTINGS);
    assert_eq!(test.top(), Some(&KEYBOARD), "nor by a button");

    // A wrong password, then the right one.
    test.press("back");
    test.typed("other words");
    assert_eq!(test.top(), Some(&LOCK), "wrong");
    test.typed("secret words");
    assert!(!test.open(&LOCK), "unlocked");
}

#[test]
fn the_wait_after_wrong_tries_holds_when_turned_and_started_anew() {
    let mut test = Test::new(X4);
    test.set_up_pin("2580");
    test.lock_now();
    for _ in 0..5 {
        test.enter("1111");
    }
    // Turned: the lock screen is made anew, the wait goes on.
    let at = test.at();
    test.kernel.resize(at, 800, 480);
    test.opened();
    test.enter("2580");
    assert_eq!(test.top(), Some(&LOCK), "waiting, also turned");

    // Started anew: the wait begins again.
    let mut test = test.restart();
    assert_eq!(test.top(), Some(&LOCK), "locked at the start");
    test.opened();
    test.enter("2580");
    assert_eq!(test.top(), Some(&LOCK), "waiting, also started anew");

    // Half a minute later the right code unlocks, and the tries are gone.
    test.now += 31_000;
    test.kernel.tick(Instant(test.now));
    test.opened();
    test.enter("2580");
    assert!(!test.open(&LOCK), "unlocked after the wait");
    assert!(
        !test
            .kernel
            .storage()
            .exists(".paperpaw/plugins/lock/tries.txt"),
        "the tries forgotten"
    );
}

#[test]
fn the_kind_changed_elsewhere_goes_back() {
    let mut test = Test::new(X4);
    test.set_up_pin("2580");
    // As a plain list of choices, or *Reset all settings*, would.
    let at = test.at();
    test.kernel
        .set_setting(at, "lock.kind", "lock.kind.off")
        .unwrap();
    assert_eq!(test.kernel.settings().text("lock.kind"), "lock.kind.pin");
    test.lock_now();
}

/// A code kept with a salt made from it (as before salts were random:
/// of the device's id, the time and the code) opens the lock as it did,
/// and is kept anew with a random salt then; a wrong one changes
/// nothing. One kept now has a random salt from the start.
#[test]
fn codes_kept_with_a_salt_of_the_code_are_kept_anew_at_random() {
    let mut test = Test::new(X4);
    test.set_up_pin("2580");
    let stored = |test: &Test| test.kernel.secrets().get("lock.code").unwrap().to_string();
    let fresh = stored(&test);
    assert!(fresh.starts_with("pin:v2."), "{fresh}");
    let old = "0123456789abcdef";
    let value = format!("pin:{old}:{}", hashed(old, "2580"));
    test.kernel.set_secret(
        paperpaw_sdk::secrets::SecretChange::keep("lock.code", "lock.secret", &value)
            .changeable(false),
    );
    test.lock_now();
    test.enter("1111");
    assert!(test.open(&LOCK), "a wrong code opens nothing");
    assert!(stored(&test).contains(old), "and changes nothing");
    test.enter("2580");
    assert!(!test.open(&LOCK), "the code kept before opens");
    let renewed = stored(&test);
    assert!(
        renewed.starts_with("pin:v2.") && !renewed.contains(old),
        "{renewed}"
    );
    assert_ne!(renewed, fresh, "a salt of its own");
    test.lock_now();
    test.enter("2580");
    assert!(!test.open(&LOCK), "and opens kept anew");
}

#[test]
fn a_lock_whose_code_is_gone_lets_the_device_be_used() {
    let mut test = Test::new(X4);
    test.set_up_pin("2580");
    test.lock_now();
    // The code removed while locked (from the list of passwords).
    test.kernel
        .set_secret(paperpaw_sdk::secrets::SecretChange::remove("lock.code"));
    test.press("confirm");
    assert!(!test.open(&LOCK), "nothing to ask for");
}

#[test]
fn a_password_lock_whose_code_is_gone_closes_with_its_keyboard() {
    let mut test = Test::new(BOOX);
    test.set_up_password("secret words");
    test.lock_now();
    assert_eq!(test.top(), Some(&KEYBOARD));
    test.kernel
        .set_secret(paperpaw_sdk::secrets::SecretChange::remove("lock.code"));
    // Any event: the lock finds its code gone while its keyboard is up.
    let at = test.at();
    test.kernel
        .send_event(at, Id::new_static("test.anything"), Params::new());
    assert!(!test.open(&LOCK), "nothing to ask for");
    assert!(!test.open(&KEYBOARD));
}
