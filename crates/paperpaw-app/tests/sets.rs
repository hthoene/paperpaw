//! The plugins each shell builds in: the X4's firmware leaves out those
//! the store offers as files, but never what a beginner needs to read,
//! get online, update, install more and recover; the list of crates the
//! firmware writes into its image (ADR 0022) is what is registered.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;
use native_activation::BuildNative as _;

use paperpaw_core::{Kernel, KernelBuilder};
use paperpaw_platform::{DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation};
use paperpaw_sdk::{MemoryStorage, PluginState};

const DEVICE: DeviceInfo = DeviceInfo::new(
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

/// What a beginner needs on a fresh device, and what gets the rest back.
/// (The library, `files`, the update and Wi-Fi are native plugins: the app
/// registers none until the host activates them, `NATIVE_PLUGINS`.)
const ALWAYS: [&str; 6] = [
    "fonts-default",
    "format-txt",
    "reader",
    "home",
    "menu",
    "store",
];

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

fn builder() -> KernelBuilder {
    Kernel::builder(DEVICE)
        .storage(MemoryStorage::new().with(".paperpaw/plugins/setup/done", ""))
        .user_config("[core]\nlanguage = en\n")
}

/// The plugins registered, with their states.
fn plugins(kernel: &Kernel) -> Vec<(String, PluginState)> {
    kernel
        .plugins()
        .iter()
        .filter(|p| p.manifest.id.as_str() != "core")
        .map(|p| (p.manifest.id.to_string(), p.state))
        .collect()
}

/// The crate a plugin comes from; none for the app's own (Pebble), which no
/// plugin crate lists.
fn crate_of(id: &str) -> Option<String> {
    if id == "pebble" {
        return None;
    }
    Some(format!("paperpaw-plugin-{id}"))
}

#[test]
#[cfg_attr(
    not(all(
        feature = "native-linked-reading",
        not(feature = "native-linked-store")
    )),
    ignore = "the lean firmware set is the native plugins with `native-linked-reading`, without the store's plugins (#244)"
)]
fn the_lean_set_keeps_what_a_beginner_needs() {
    let kernel = paperpaw_app::install_with(builder(), fonts(), paperpaw_app::StorePlugins::Left)
        .build_native();
    let plugins = plugins(&kernel);
    for id in ALWAYS {
        assert!(
            plugins
                .iter()
                .any(|(p, state)| p == id && *state == PluginState::Running),
            "{id} is missing: {plugins:?}"
        );
    }
    // The app's own theme, off as it ships.
    assert!(
        plugins
            .iter()
            .any(|(p, state)| p == "pebble" && *state == PluginState::Off),
        "{plugins:?}"
    );
    for id in paperpaw_app::STORE_PLUGINS {
        assert!(
            plugins.iter().all(|(p, _)| p != id),
            "{id} is built in: {plugins:?}"
        );
    }
}

#[test]
#[cfg_attr(
    not(all(
        feature = "native-linked",
        feature = "native-linked-reading",
        feature = "native-linked-store"
    )),
    ignore = "every plugin crate is installed only with `native-linked`, `native-linked-reading` and `native-linked-store` (#244)"
)]
fn the_crates_listed_are_those_installed() {
    let kernel = native_activation::build(paperpaw_app::install(builder(), fonts()));
    let mut installed: Vec<String> = plugins(&kernel)
        .iter()
        .filter_map(|(id, _)| crate_of(id))
        .collect();
    // Wi-Fi is native only (#244): the build has its crate, the interface-6 app
    // does not register it.
    installed.push(String::from("paperpaw-plugin-wifi"));
    installed.sort();
    installed.dedup();
    let listed: Vec<&str> = paperpaw_app::CRATES.split(',').collect();
    assert_eq!(installed, listed);
}

/// The plugins the store offers as files too are linked, and off unless used
/// (Android and the X4's card: a new reader has none of them running).
#[cfg(all(feature = "store-plugins", feature = "native-linked-store"))]
#[test]
fn linked_store_plugins_are_off_unless_used() {
    const USED: [&str; 2] = ["vocabulary", "widgets"];
    let kernel = native_activation::build_with(
        builder(),
        paperpaw_app::StorePlugins::Off(&|id| USED.contains(&id)),
    );
    let plugins = plugins(&kernel);
    for id in paperpaw_app::STORE_PLUGINS {
        let state = plugins
            .iter()
            .find(|(p, _)| p == id)
            .map(|(_, state)| *state)
            .unwrap_or_else(|| panic!("{id} is not linked"));
        let want = if USED.contains(&id) {
            PluginState::Running
        } else {
            PluginState::Off
        };
        assert_eq!(state, want, "{id}");
    }
}

#[test]
fn a_reader_who_ran_paperpaw_before_keeps_the_plugins_as_they_were() {
    // A new reader: the store plugins off (Android).
    assert!(!paperpaw_app::ran_before(&MemoryStorage::new()));
    // One with PaperPaw's folder, its configuration, or either under the
    // name before 0.4: as they were, whatever plugin they used.
    for path in [
        ".paperpaw/plugins/setup/done",
        "paperpaw.ini",
        ".quireink/secrets.txt",
        "quireink.ini",
    ] {
        let storage = MemoryStorage::new().with(path, "");
        assert!(paperpaw_app::ran_before(&storage), "{path}");
    }
}

/// Android, for a reader who ran PaperPaw before and switched a plugin off: it
/// stays off; one who did not has it on.
#[cfg(all(feature = "store-plugins", feature = "native-linked-store"))]
#[test]
fn a_linked_store_plugin_switched_off_before_stays_off() {
    let storage = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[core]\nlanguage = en\n\n[plugins]\nvocabulary = off\n",
        );
    assert!(paperpaw_app::ran_before(&storage));
    let kernel = native_activation::build_with(
        Kernel::builder(DEVICE).storage(storage).user_config(""),
        paperpaw_app::StorePlugins::Off(&|_| true),
    );
    let off = plugins(&kernel);
    let state = |id: &str| off.iter().find(|(p, _)| p == id).map(|(_, s)| *s);
    assert_eq!(state("vocabulary"), Some(PluginState::Off));
    assert_eq!(state("widgets"), Some(PluginState::Running));
    let kernel =
        native_activation::build_with(builder(), paperpaw_app::StorePlugins::Off(&|_| true));
    let on = plugins(&kernel);
    let state = |id: &str| on.iter().find(|(p, _)| p == id).map(|(_, s)| *s);
    assert_eq!(state("vocabulary"), Some(PluginState::Running));
}

/// The list the shells use names every plugin that `native-linked-store` links
/// beyond the lean set, and no other.
#[cfg(feature = "native-linked-store")]
#[test]
fn every_store_plugin_is_linked_with_the_store_and_none_without() {
    let all = native_activation::build_with(builder(), paperpaw_app::StorePlugins::Left);
    let linked: Vec<&str> = paperpaw_app::NATIVE_PLUGINS
        .iter()
        .map(|(id, _)| *id)
        .collect();
    for id in paperpaw_app::STORE_PLUGINS {
        assert!(linked.contains(&id), "{id} is not linked");
        // Left to the store: not hosted, whatever the configuration says.
        assert_eq!(
            paperpaw_app::native_default_on(id, paperpaw_app::StorePlugins::Left),
            None,
            "{id}"
        );
        assert!(plugins(&all).iter().all(|(p, _)| p != id), "{id} is hosted");
    }
}
