//! What the power menu leaves on the panel (#146): *Switch off* and *Sleep*
//! show the sleep screen first, as the power button's short press does -
//! e-ink keeps it without power, so a device that is off does not look as
//! if it were on. Where the sleep screen is the page as it is, a message
//! says that the device is off.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_power::Power;
use paperpaw_plugin_sleep::Sleep;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
};

/// An X4 as the emulator's profile has it: buttons, no touch.
const X4: DeviceInfo = DeviceInfo::new(
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

struct Test {
    kernel: Kernel,
    now: u64,
}

impl Test {
    /// An X4 with the sleep screen as `config` sets it (lines of `[sleep]`).
    fn new(language: &str, config: &str) -> Self {
        let mut kernel = Kernel::builder(X4)
            .storage(MemoryStorage::new())
            .user_config(format!(
                "[core]\nlanguage = {language}\n[sleep]\n{config}\n"
            ))
            .sleep(true)
            .power_off(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Power)
            .plugin(Sleep)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self { kernel, now: 1_000 }
    }

    fn at(&mut self, millis: u64) -> Instant {
        self.now += millis;
        Instant(self.now)
    }

    /// The power button held, which opens the menu.
    fn open_menu(&mut self) {
        let name = "power";
        let at = self.at(200);
        self.kernel.handle(
            at,
            InputEvent::Button {
                name,
                state: ButtonState::Pressed,
            },
        );
        let at = self.at(1500);
        self.kernel.tick(at);
        self.kernel.handle(
            at,
            InputEvent::Button {
                name,
                state: ButtonState::Released,
            },
        );
        assert_eq!(self.on_top(), Some("core.choice-dialog"));
    }

    fn on_top(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }

    fn texts(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label.to_string())
            .collect()
    }

    /// Moves to the option `text` of the menu and confirms it.
    fn choose(&mut self, text: &str) -> Effects {
        self.kernel.render(&mut |_, _| {});
        let wanted = self
            .kernel
            .describe()
            .iter()
            .filter(|p| matches!(p.role, Role::Row | Role::Button))
            .position(|p| p.label == text)
            .expect("the option is on the menu");
        for _ in 0..wanted {
            let at = self.at(100);
            self.kernel.run_action(at, actions::DOWN);
        }
        let at = self.at(100);
        self.kernel.run_action(at, actions::CONFIRM)
    }
}

#[test]
fn switching_off_shows_the_sleep_screen_first() {
    let mut t = Test::new("en", "show = sleep.show.cat");
    t.open_menu();
    let effects = t.choose("Switch off");
    assert!(effects.power_off(), "it switches off");
    assert_eq!(t.on_top(), Some("sleep.screen"), "the picture is on top");
    assert!(
        !t.texts().iter().any(|t| t.contains("Sleeping")),
        "a cat, no words"
    );
}

#[test]
fn sleeping_from_the_menu_shows_the_sleep_screen_too() {
    let mut t = Test::new("en", "show = sleep.show.cat");
    t.open_menu();
    let effects = t.choose("Sleep");
    assert!(effects.sleep());
    assert_eq!(t.on_top(), Some("sleep.screen"));
}

#[test]
fn off_with_the_page_kept_says_it_is_off() {
    for show in ["sleep.show.page", "sleep.show.overlay"] {
        let mut t = Test::new("en", &format!("show = {show}"));
        t.open_menu();
        assert!(t.choose("Switch off").power_off());
        assert_eq!(t.on_top(), Some("sleep.screen"), "{show}");
        let texts = t.texts();
        assert!(
            texts.iter().any(|t| t == "Switched off")
                && texts
                    .iter()
                    .any(|t| t == "Hold the power button to switch on"),
            "{show}: {texts:?}"
        );
    }
}

#[test]
fn asleep_with_the_page_kept_the_page_stays() {
    let mut t = Test::new("en", "show = sleep.show.page");
    t.open_menu();
    assert!(t.choose("Sleep").sleep());
    let texts = t.texts();
    assert!(
        !texts.iter().any(|t| t.starts_with("Switched off")),
        "{texts:?}"
    );
}

#[test]
fn the_message_is_german_too() {
    let mut t = Test::new("de", "show = sleep.show.message");
    t.open_menu();
    assert!(t.choose("Ausschalten").power_off());
    let texts = t.texts();
    assert!(
        texts.iter().any(|t| t == "Ausgeschaltet")
            && texts.iter().any(|t| t == "Zum Einschalten Ein/Aus halten"),
        "{texts:?}"
    );
}

#[test]
fn the_message_mode_says_sleeping_when_asleep_and_off_when_off() {
    let mut t = Test::new("en", "show = sleep.show.message");
    t.open_menu();
    assert!(t.choose("Sleep").sleep());
    assert!(t.texts().iter().any(|t| t == "Sleeping"));
    let mut t = Test::new("en", "show = sleep.show.message");
    t.open_menu();
    assert!(t.choose("Switch off").power_off());
    let texts = t.texts();
    assert!(!texts.iter().any(|t| t == "Sleeping"), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("Switched off")));
}
