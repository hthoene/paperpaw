//! Plugins that fail to register, with code or as data folders: each is
//! reported and left out whole (none of its contributions stay), while
//! the kernel and every other plugin go on. A plugin that panics is not
//! caught: plugins run without a sandbox (ADR 0024), and the firmware's
//! safe mode deals with them, so that is not tested here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use std::borrow::Cow;

use common::{BASE, DEVICE, Harness, press};
use paperpaw_core::sdk::builtin::{actions, slots};
use paperpaw_core::sdk::{
    ActionSpec, Command, Id, Manifest, MemoryStorage, MenuEntry, Params, Plugin, PluginState,
    Priority, RegisterError, Registrar, Screen, ScreenSpec, Setting, SettingSpec,
};
use paperpaw_core::{Kernel, Warning, WarningSource};
use paperpaw_platform::Instant;
use proptest::prelude::*;
use proptest::test_runner::Config;

/// How a plugin goes wrong.
#[derive(Clone, Copy, Debug)]
enum Fault {
    /// Returns an error after all its other contributions.
    Late,
    /// A key map with a broken line after a good one.
    Keymap,
    /// A setting whose default is out of its range.
    Default,
    /// Contributes an id of another plugin.
    Namespace,
    /// The same action twice.
    Twice,
}

const LEVEL: Setting<i64> = Setting::new("bad.level");

/// A plugin (`bad`) that registers a screen for home, an action, a
/// setting, a subscription, a menu entry and a key binding, then fails as
/// `fault` says.
struct Faulty {
    fault: Fault,
    harness: Harness,
}

impl Plugin for Faulty {
    fn manifest(&self) -> Manifest {
        common::manifest("bad")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let id = |name: &str| Id::parse(&format!("bad.{name}")).unwrap();
        r.add_screen(ScreenSpec::new(
            id("screen"),
            "x",
            Box::new(|_, _| -> Box<dyn Screen> { unreachable!("never opened") }),
        ))?;
        r.bind_slot(slots::HOME, id("screen"), Priority(i16::MAX))?;
        r.add_action(ActionSpec::new(id("act"), "x"))?;
        r.add_setting(SettingSpec::number(
            LEVEL.static_str(),
            "x",
            1,
            5,
            if matches!(self.fault, Fault::Default) {
                9
            } else {
                2
            },
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        let log = self.harness.log.clone();
        r.subscribe(
            common::LATER,
            Box::new(move |_, _, _| log.borrow_mut().push(String::from("faulty heard"))),
        )?;
        r.add_menu_entry(
            MenuEntry::new(paperpaw_core::sdk::builtin::menus::MAIN, id("act")).order(1),
        )?;
        r.add_keymap(Cow::Borrowed(
            "[keymap.global]\nbutton.page-forward = bad.act\n",
        ))?;
        match self.fault {
            Fault::Late => Err(RegisterError::Invalid {
                message: Cow::Borrowed("gave up"),
            }),
            Fault::Keymap => r.add_keymap(Cow::Borrowed(
                "[keymap.global]\nbutton.page-back = core.home\nbutton.page-back = not an id!\n",
            )),
            Fault::Default => Ok(()),
            Fault::Namespace => r.add_action(ActionSpec::new(actions::HOME, "x")),
            Fault::Twice => r.add_action(ActionSpec::new(id("act"), "x")),
        }
    }
}

fn faulty(harness: &Harness, fault: Fault) -> (Kernel, Vec<Warning>) {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Faulty {
            fault,
            harness: harness.clone(),
        })
        .plugin(harness.plugin())
        .build()
        .unwrap();
    let warnings = kernel.take_warnings();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    (kernel, warnings)
}

#[test]
fn a_plugin_that_fails_leaves_none_of_its_contributions() {
    for fault in [
        Fault::Late,
        Fault::Keymap,
        Fault::Default,
        Fault::Namespace,
        Fault::Twice,
    ] {
        let harness = Harness::new();
        let (mut kernel, warnings) = faulty(&harness, fault);
        // Reported once, as the plugin's.
        let bad: Vec<&Warning> = warnings
            .iter()
            .filter(|w| w.source == WarningSource::Plugin(Id::new_static("bad")))
            .collect();
        assert_eq!(bad.len(), 1, "{fault:?}: {warnings:?}");
        assert_eq!(
            kernel.plugins().get("bad").map(|p| p.state),
            Some(PluginState::Failed),
            "{fault:?}"
        );
        // Its screen did not take home; its setting, action and menu
        // entry are not there; it hears nothing; its key bindings do not
        // apply, not even the good line of a broken key map.
        assert_eq!(kernel.active_screen(), Some(&BASE), "{fault:?}");
        assert!(kernel.settings().get("bad.level").is_none(), "{fault:?}");
        assert!(!kernel.actions().contains("bad.act"), "{fault:?}");
        assert!(!kernel.known_screens().any(|s| s.as_str() == "bad.screen"));
        harness.run(
            &mut kernel,
            10,
            [Command::Emit(common::LATER, Params::new())],
        );
        assert!(harness.lines("faulty").is_empty(), "{fault:?}");
        press(&mut kernel, 1000, "page-forward");
        press(&mut kernel, 2000, "page-back");
        assert_eq!(
            harness.lines("base:action"),
            ["base:action core.next-page", "base:action core.prev-page"],
            "{fault:?}"
        );
    }
}

#[test]
fn a_broken_key_map_is_reported_with_its_line() {
    let harness = Harness::new();
    let (_, warnings) = faulty(&harness, Fault::Keymap);
    let bad = warnings
        .iter()
        .find(|w| w.source == WarningSource::Plugin(Id::new_static("bad")))
        .unwrap();
    assert_eq!(bad.line, Some(3), "{bad:?}");
}

#[test]
fn a_failing_plugin_switched_off_is_listed_without_a_warning() {
    let harness = Harness::new();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Faulty {
            fault: Fault::Late,
            harness: harness.clone(),
        })
        .plugin(harness.plugin())
        .user_config("[plugins]\nbad = off\n")
        .build()
        .unwrap();
    assert!(kernel.take_warnings().is_empty());
    let info = kernel.plugins().get("bad").unwrap();
    assert_eq!(info.state, PluginState::Off);
    assert!(!info.enabled);
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen(), Some(&BASE));
}

/// A plugin with a plain id other than its manifest's, or the same id as
/// one loaded before.
struct Named {
    id: &'static str,
}

impl Plugin for Named {
    fn manifest(&self) -> Manifest {
        common::manifest(self.id)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.bind_slot(slots::HOME, Id::new_static("drv.full"), Priority(i16::MAX))
    }
}

#[test]
fn plugins_with_taken_dotted_or_reserved_ids_are_refused() {
    let harness = Harness::new();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .plugin(Named { id: "drv" })
        .plugin(Named { id: "core" })
        .plugin(Named { id: "drv.sub" })
        .build()
        .unwrap();
    let warnings = kernel.take_warnings();
    let refused: Vec<String> = warnings
        .iter()
        .filter_map(|w| match &w.source {
            WarningSource::Plugin(id) => Some(id.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(refused, ["drv", "core", "drv.sub"], "{warnings:?}");
    // The first `drv` and the kernel are the ones that stay.
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen(), Some(&BASE));
    assert_eq!(
        kernel
            .plugins()
            .iter()
            .filter(|p| p.manifest.id.as_str() == "drv")
            .count(),
        1
    );
    assert!(kernel.plugins().get("drv.sub").is_none());
}

/// A storage with these data plugin folders: `(folder, file, content)`.
fn data_plugins(files: &[(&str, &str, &[u8])]) -> MemoryStorage {
    files
        .iter()
        .fold(MemoryStorage::new(), |storage, (folder, file, data)| {
            storage.with(&format!("plugins/{folder}/{file}"), data.to_vec())
        })
}

#[test]
fn broken_data_plugins_are_reported_and_good_ones_still_load() {
    let huge = "#".repeat(5000);
    let storage = data_plugins(&[
        (
            "no-ini",
            "keymap.ini",
            b"[keymap.global]\nbutton.up = core.home\n",
        ),
        ("broken-ini", "plugin.ini", b"[plugin\nid = broken\n"),
        ("future", "plugin.ini", b"[plugin]\nformats = 99\n"),
        ("formats-word", "plugin.ini", b"[plugin]\nformats = many\n"),
        ("dotted", "plugin.ini", b"[plugin]\nid = a.b\n"),
        ("bad-version", "plugin.ini", b"[plugin]\nversion = one\n"),
        ("core", "plugin.ini", b"[plugin]\nname = Not the kernel\n"),
        ("drv", "plugin.ini", b"[plugin]\nname = Not the driver\n"),
        ("latin1", "plugin.ini", b"[plugin]\nname = Gr\xFC\xDFe\n"),
        ("huge", "plugin.ini", huge.as_bytes()),
        ("bad-keys", "plugin.ini", b"[plugin]\nname = Keys\n"),
        (
            "bad-keys",
            "keymap.ini",
            b"[keymap.global]\nbutton.up = core.home\nbutton.x = !\n",
        ),
        ("bad-default", "plugin.ini", b""),
        (
            "bad-default",
            "settings.ini",
            b"[core]\nlong-press-ms = 1\n",
        ),
        ("bad-presets", "plugin.ini", b""),
        ("bad-presets", "presets.ini", b"[Bad Preset]\nkind = x\n"),
        ("bad-css", "plugin.ini", b""),
        ("bad-css", "styles.css", b"\xFF\xFE not text"),
        ("good", "plugin.ini", b"[plugin]\nname = Good\n"),
        ("good", "settings.ini", b"[core]\nui-size = 13\n"),
        (
            "good",
            "keymap.ini",
            b"[keymap.global]\nbutton.up = core.menu\n",
        ),
        (".hidden", "plugin.ini", b"[plugin]\nname = Hidden\n"),
    ]);
    let harness = Harness::new();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .storage(storage)
        .build()
        .unwrap();
    let warnings = kernel.take_warnings();
    let text: Vec<String> = warnings.iter().map(ToString::to_string).collect();
    for name in [
        "no-ini",
        "broken-ini",
        "future",
        "formats-word",
        "a.b",
        "bad-version",
        "latin1",
        "huge",
        // The host's own id: refused as a manifest, before any duplicate check.
        "plugins/core/plugin.ini: invalid id `core`",
    ] {
        assert!(
            text.iter().any(|w| w.contains(name)),
            "{name} not reported: {text:#?}"
        );
    }
    for id in ["drv", "bad-keys", "bad-default", "bad-presets"] {
        assert!(
            warnings
                .iter()
                .any(|w| w.source == WarningSource::Plugin(Id::parse(id).unwrap())),
            "{id} not reported: {text:#?}"
        );
    }
    assert!(kernel.plugins().get("hidden").is_none());
    assert_eq!(
        kernel.plugins().get("good").map(|p| p.state),
        Some(PluginState::Running)
    );
    assert_eq!(kernel.settings().int("core.ui-size"), 13);
    assert_eq!(kernel.settings().int("core.long-press-ms"), 500);
    kernel.start(Instant(0));
    common::render(&mut kernel);
    assert_eq!(kernel.active_screen(), Some(&BASE));
    // The good plugin's binding applies; the broken one's good line not.
    press(&mut kernel, 1000, "up");
    assert_eq!(kernel.active_screen(), Some(&common::OVERLAY));
}

#[test]
fn commands_for_plugins_that_are_not_data_plugins_are_refused() {
    let harness = Harness::new();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .storage(MemoryStorage::new().with("books/a.txt", b"keep".to_vec()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    harness.run(
        &mut kernel,
        10,
        [
            Command::RemovePlugin(Id::new_static("drv")),
            Command::RemovePlugin(Id::new_static("core")),
            Command::RemovePlugin(Id::new_static("nobody")),
            Command::LoadPlugin(String::from("plugins/nothing-here")),
            Command::LoadPlugin(String::new()),
            Command::LoadPlugin(String::from("../../..")),
            Command::SetPluginEnabled(Id::new_static("core"), false),
            Command::SetPluginEnabled(Id::new_static("nobody"), true),
        ],
    );
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 8, "{warnings:#?}");
    assert!(kernel.plugins().get("drv").is_some_and(|p| p.enabled));
    assert!(kernel.plugins().get("core").is_some_and(|p| p.enabled));
    assert!(kernel.storage().exists("books/a.txt"));
    press(&mut kernel, 1000, "page-forward");
    assert_eq!(harness.lines("base:action"), ["base:action core.next-page"]);
}

/// Files of a data plugin: any text in each of its files.
fn plugin_file() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        "(\\[[a-z.]{0,10}\\]\n|[a-z.\\-]{0,10} = [ -~]{0,16}\n|#[ -~]{0,8}\n){0,8}"
            .prop_map(String::into_bytes),
        prop::collection::vec(any::<u8>(), 0..256),
    ]
}

proptest! {
    #![proptest_config(Config { failure_persistence: None, ..Config::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)) })]

    /// Any content in a data plugin's files starts a kernel with a screen.
    #[test]
    fn any_data_plugin_starts_a_kernel_with_a_screen(
        manifest in plugin_file(),
        keymap in plugin_file(),
        settings in plugin_file(),
        presets in plugin_file(),
        catalog in plugin_file(),
    ) {
        let storage = data_plugins(&[
            ("any", "plugin.ini", &manifest),
            ("any", "keymap.ini", &keymap),
            ("any", "settings.ini", &settings),
            ("any", "presets.ini", &presets),
            ("any", "i18n/en.ini", &catalog),
        ]);
        let harness = Harness::new();
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(harness.plugin())
            .storage(storage)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        common::render(&mut kernel);
        prop_assert!(kernel.active_screen().is_some());
        press(&mut kernel, 1000, "menu");
        prop_assert!(kernel.active_screen().is_some());
    }
}

/// A storage that refuses to write the passwords and keys file while
/// `.1` says so, and keeps everything else.
#[derive(Clone, Default)]
struct NoSecrets(
    std::rc::Rc<MemoryStorage>,
    std::rc::Rc<std::cell::Cell<bool>>,
);

impl paperpaw_platform::Storage for NoSecrets {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        if self.1.get() && path == paperpaw_core::sdk::secrets::FILE {
            return Err(paperpaw_platform::StorageError::Full);
        }
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.create_dir(path)
    }
}

static SECRET: paperpaw_core::sdk::SettingExtra = paperpaw_core::sdk::SettingExtra::new().secret();

/// A plugin (`keeper`) with a secret setting, `keeper.key`.
struct Keeper;

impl Plugin for Keeper {
    fn manifest(&self) -> Manifest {
        common::manifest("keeper")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_setting(
            SettingSpec::text(
                "keeper.key",
                "keeper.key",
                "",
                paperpaw_core::sdk::builtin::groups::OTHER,
            )
            .with_extra(&SECRET),
        )
    }
}

/// A secret setting kept in `paperpaw.ini` before stays there while the
/// passwords and keys file cannot be written: it is moved only once it
/// is saved with the secrets.
#[test]
fn a_secret_setting_stays_in_the_file_until_the_secrets_are_saved() {
    use paperpaw_core::sdk::builtin::CONFIG_FILE;
    use paperpaw_core::sdk::secrets::FILE;
    use paperpaw_platform::Storage as _;
    let storage = NoSecrets::default();
    storage.1.set(true);
    storage
        .0
        .write(CONFIG_FILE, b"[keeper]\nkey = old-k\n")
        .unwrap();
    let start = |storage: &NoSecrets| {
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(Keeper)
            .storage(storage.clone())
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel
    };
    let config =
        |storage: &NoSecrets| String::from_utf8(storage.0.get(CONFIG_FILE).unwrap()).unwrap();
    let kernel = start(&storage);
    assert_eq!(kernel.secrets().get("keeper.key"), Some("old-k"));
    assert!(!kernel.secrets().saved("keeper.key"));
    assert!(!storage.0.exists(FILE));
    assert!(config(&storage).contains("old-k"), "{}", config(&storage));
    drop(kernel);
    // Writable again: moved at the next start.
    storage.1.set(false);
    let kernel = start(&storage);
    assert!(kernel.secrets().saved("keeper.key"));
    assert!(storage.0.exists(FILE));
    assert!(!config(&storage).contains("old-k"), "{}", config(&storage));
}
