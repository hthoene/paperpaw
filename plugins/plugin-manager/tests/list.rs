//! The plugin manager with the real kernel: its list follows plugins
//! installed while it is open (e.g. from a store opened from it).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_plugin_manager::{OPEN, PluginManager};
use paperpaw_sdk::{
    ActionSpec, Command, Commands, DeviceInfo, DisplayInfo, Env, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, PixelFormat, Plugin, RegisterError, Registrar, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// Installs the data plugin downloaded into `plugins/.airy.part`, as a
/// store does.
const INSTALL: Id = Id::new_static("installer.install");

struct Installer;

impl Plugin for Installer {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("installer"),
            "Installer",
            Version::new(0, 1, 0),
        )
        .description("Installs a plugin.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(INSTALL, "installer.install").handler(
            |_: &Env<'_>, _, out: &mut Commands| {
                out.push(Command::InstallPlugin(
                    Id::new_static("airy"),
                    String::from("plugins/.airy.part"),
                ));
            },
        ))
    }
}

/// What the screen shows, as one text.
fn shown(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .into_iter()
        .map(|p| p.label)
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn a_plugin_installed_while_the_list_is_open_is_listed() {
    let storage = MemoryStorage::new().with(
        "plugins/.airy.part/plugin.ini",
        "[plugin]\nid = airy\nname = Airy\ndescription = Roomy.\nversion = 1.0.0\n",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(PluginManager)
        .plugin(Installer)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(100), OPEN);
    let shown_before = shown(&mut kernel);
    assert!(!shown_before.contains("Airy"), "{shown_before}");
    kernel.run_action(Instant(200), INSTALL);
    assert!(kernel.plugins().get("airy").is_some());
    let shown_after = shown(&mut kernel);
    assert!(shown_after.contains("Airy"), "{shown_after}");
    assert!(shown_after.contains("Roomy."), "{shown_after}");
    // Not installed by a store: local; built-in plugins say neither.
    assert!(shown_after.contains("Local"), "{shown_after}");
    assert_eq!(shown_after.matches("Local").count(), 1, "{shown_after}");
    assert!(!shown_after.contains("From the store"), "{shown_after}");
    // Installed by a store at that version: the store's.
    paperpaw_sdk::install::remember_store_install(kernel.storage(), "airy", Version::new(1, 0, 0));
    kernel.run_action(Instant(300), paperpaw_sdk::builtin::actions::BACK);
    kernel.run_action(Instant(400), OPEN);
    let shown = shown(&mut kernel);
    assert!(shown.contains("From the store."), "{shown}");
    assert!(!shown.contains("Local"), "{shown}");
}

/// Says how it works, at length.
struct Explained;

impl Plugin for Explained {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("explained"),
            "Explained",
            Version::new(0, 1, 0),
        )
        .description("Says how it works.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(std::borrow::Cow::Borrowed(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\n\
             explained.plugin.how = \"First this.\\n\\nThen that.\"\n",
        ))
    }
}

/// Holding each of the first rows of the list in turn: the text each
/// was shown in, the first line (the plugin's name) of each, where one was.
fn hold_each(kernel: &mut Kernel) -> Vec<String> {
    const HOW: &str = ".paperpaw/plugins/plugin-manager/how.txt";
    let mut shown = Vec::new();
    for step in 0..8 {
        let at = Instant(1000 + step * 100);
        kernel.run_action(at, paperpaw_sdk::builtin::actions::LONG_CONFIRM);
        if let Ok(data) = paperpaw_sdk::read_file(kernel.storage(), HOW, 1 << 16) {
            shown.push(String::from_utf8(data).unwrap());
            kernel.storage().remove(HOW).unwrap();
            // Back from the page it opened, where a reader opened it.
            kernel.run_action(at, paperpaw_sdk::builtin::actions::BACK);
        }
        kernel.run_action(at, paperpaw_sdk::builtin::actions::DOWN);
    }
    shown
}

#[test]
fn holding_a_plugin_shows_how_it_works() {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(PluginManager)
        .plugin(Explained)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(100), OPEN);
    let shown = hold_each(&mut kernel);
    let explained = String::from("Explained\n\nFirst this.\n\nThen that.\n");
    assert!(shown.contains(&explained), "{shown:?}");
}

#[test]
fn holding_a_plugin_that_says_nothing_more_keeps_the_list() {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(PluginManager)
        .plugin(Installer)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(100), OPEN);
    // The installer says nothing of how it works: a hold tells so (a
    // notice) and the list stays; the default fonts do.
    let texts = hold_each(&mut kernel);
    assert!(
        texts.iter().all(|s| !s.starts_with("Installer")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|s| s.starts_with("Default fonts")),
        "{texts:?}"
    );
    let shown = shown(&mut kernel);
    assert!(shown.starts_with("Plugins"), "{shown}");
}
