//! A plugin with code added to the standard app (`set_plugins`), as a
//! module built around a plugin being written adds it: it runs on the
//! device and `paperpaw-dev` checks it. A test binary of its own, as the
//! plugins added hold for every device started after.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::borrow::Cow;

use paperpaw_core::KernelBuilder;
use paperpaw_core::sdk::builtin::actions;
use paperpaw_core::sdk::{
    Canvas, Command, Cx, Env, Handled, Id, Manifest, Screen, ScreenSpec, Uses, Version,
};
use paperpaw_web::emulator::Emulator;

/// The words of the plugin's screen, by language.
const EN: &str = "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nhello.title = Hello\n";
const DE: &str = "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\nhello.title = Hallo\n";

/// A screen with nothing on it that the buttons close.
struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM || *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        Handled::No
    }
}

/// A plugin with a screen, added as a module's `pp_plugins` adds its own.
fn hello(app: KernelBuilder) -> KernelBuilder {
    app.items(
        Manifest::new(Id::new_static("hello"), "Hello", Version::new(0, 1, 0)).uses(Uses::NOTHING),
        |r| {
            r.add_screen(ScreenSpec::new(
                Id::new_static("hello.screen"),
                "hello.title",
                Box::new(|_, _| Box::new(Blank)),
            ))?;
            r.add_translations(Cow::Borrowed(EN))?;
            r.add_translations(Cow::Borrowed(DE))
        },
    )
}

#[test]
fn a_plugin_with_code_is_added_and_checked() {
    let mut emulator = Emulator::new("xteink-x4", "en", false).unwrap();
    emulator.start().unwrap();
    emulator.advance(5_000);
    assert!(
        emulator.check(&["lint"], &["hello"]).is_err(),
        "nothing added yet"
    );

    // Added before the device starts, as a module's `pp_plugins` does
    // (one device at a time: the heap budget counts every kernel).
    drop(emulator);
    paperpaw_web::set_plugins(hello);
    let mut emulator = Emulator::new("xteink-x4", "en", false).unwrap();
    emulator.start().unwrap();
    emulator.advance(10_000);
    let json = emulator
        .check(&["lint", "reach", "perf"], &["hello"])
        .unwrap();
    // Each check ran, perf with the app's heap alone (this device's own
    // is no part of it).
    for ran in ["lint.pseudo", "reach.searched", "perf.calibration"] {
        assert!(json.contains(ran), "{ran}: {json}");
    }
    assert!(!json.contains("perf.heap\""), "{json}");
    assert!(!json.contains("dev.run"), "a check could not run: {json}");
    // Only plugins the app has, only checks of plugins.
    assert!(emulator.check(&["lint"], &["no-such-plugin"]).is_err());
    assert!(emulator.check(&["snapshot"], &["hello"]).is_err());
    // The device runs on after the check.
    assert!(emulator.button("right", true, 20_000));
    emulator.advance(25_000);
}
