//! *Screens* with the real kernel: a slot two plugins offer a screen for
//! is listed, a plugin chosen for it is used and kept in `[slots]`, and
//! *Automatic* goes back to the plugins' order; a screen newly offered
//! is asked about only where it binds the slot as high as the one used.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Kernel, KernelBuilder};
use paperpaw_platform::Rotation;
use paperpaw_plugin_plugin_manager::PluginManager;
use paperpaw_sdk::builtin::{CONFIG_FILE, actions, slots};
use paperpaw_sdk::{
    Canvas, DeviceInfo, DisplayInfo, Env, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, PixelFormat, Plugin, Priority, RegisterError, Registrar, Screen, ScreenSpec,
    Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

/// A plugin offering a home screen.
struct Home(&'static str, &'static str);

impl Plugin for Home {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.0), self.1, Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        bind_home(r, self.0, Priority::DEFAULT)
    }
}

/// A plugin offering a home screen only where there is no other.
struct Fallback(&'static str, &'static str);

impl Plugin for Fallback {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.0), self.1, Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        bind_home(r, self.0, Priority::FALLBACK)
    }
}

fn bind_home(r: &mut Registrar, plugin: &str, priority: Priority) -> Result<(), RegisterError> {
    let screen = Id::parse(&format!("{plugin}.home")).unwrap();
    r.add_screen(ScreenSpec::new(
        screen.clone(),
        "home",
        Box::new(|_, _| Box::new(Blank)),
    ))?;
    r.bind_slot(slots::HOME, screen, priority)
}

/// A kernel started with `plugins` beside the manager, its last start
/// having seen no slot with several screens (as after an update that
/// brings a plugin along).
fn started(plugins: impl FnOnce(KernelBuilder) -> KernelBuilder) -> Kernel {
    let known = format!("{}/screens.txt", Env::plugin_dir("plugin-manager"));
    let builder = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new().with(&known, ""))
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(PluginManager);
    let mut kernel = plugins(builder).build().unwrap();
    kernel.start(Instant(0));
    kernel
}

#[test]
fn a_new_fallback_is_not_offered_for_the_slot() {
    let mut kernel = started(|b| {
        b.plugin(Home("first", "First"))
            .plugin(Fallback("low", "Low"))
    });
    let shown = shown(&mut kernel);
    assert!(!shown.contains("now?"), "{shown}");
}

#[test]
fn a_new_screen_bound_as_high_is_offered_once() {
    let mut kernel = started(|b| {
        b.plugin(Home("first", "First"))
            .plugin(Home("second", "Second"))
    });
    let shown = shown(&mut kernel);
    assert!(shown.contains("now?"), "{shown}");
}

fn shown(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .into_iter()
        .map(|p| p.label)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn config(kernel: &Kernel) -> String {
    paperpaw_sdk::read_file(kernel.storage(), CONFIG_FILE, 1 << 16)
        .map(|data| String::from_utf8(data).unwrap())
        .unwrap_or_default()
}

#[test]
fn a_slot_offered_twice_is_chosen_and_kept() {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(PluginManager)
        .plugin(Home("first", "First"))
        .plugin(Home("second", "Second"))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let mut now = 100;
    let mut run = |kernel: &mut Kernel, action: Id| {
        now += 100;
        kernel.run_action(Instant(now), action);
    };
    run(&mut kernel, Id::new_static("plugin-manager.screens.open"));
    let listed = shown(&mut kernel);
    assert!(listed.contains("Automatic"), "{listed}");
    // The home slot's row (first), then its candidates.
    run(&mut kernel, actions::CONFIRM);
    let candidates = shown(&mut kernel);
    assert!(
        candidates.contains("First") && candidates.contains("Second"),
        "{candidates}"
    );
    // Automatic, First, Second: the first plugin chosen.
    run(&mut kernel, actions::DOWN);
    run(&mut kernel, actions::CONFIRM);
    assert!(
        config(&kernel).contains("first.home"),
        "{}",
        config(&kernel)
    );
    let listed = shown(&mut kernel);
    assert!(listed.contains("First"), "{listed}");
    // Back to automatic: the choice is gone.
    run(&mut kernel, actions::CONFIRM);
    run(&mut kernel, actions::CONFIRM);
    assert!(
        !config(&kernel).contains("first.home"),
        "{}",
        config(&kernel)
    );
}
