//! The plugins each shell builds in: the X4's firmware leaves out those
//! the store offers as files, but never what a beginner needs to read,
//! get online, update, install more and recover; the list of crates the
//! firmware writes into its image (ADR 0022) is what is registered.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

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
const ALWAYS: [&str; 16] = [
    "fonts-default",
    "format-epub",
    "format-txt",
    "reader",
    "files",
    "home",
    "menu",
    "settings",
    "setup",
    "wifi",
    "keyboard",
    "store",
    "plugin-manager",
    "update",
    "transfer",
    "about",
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
        .user_config("[help]\nshow = false\n[core]\nlanguage = en\n")
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

/// The crate a plugin comes from: the formats of `format-epub` share it;
/// none for the app's own (Pebble), which no plugin crate lists.
fn crate_of(id: &str) -> Option<String> {
    let id = match id {
        "format-html" | "format-fb2" | "format-markdown" | "format-cbz" => "format-epub",
        "pebble" => return None,
        id => id,
    };
    Some(format!("paperpaw-plugin-{id}"))
}

#[test]
fn the_lean_set_keeps_what_a_beginner_needs() {
    let kernel = paperpaw_app::install_with(builder(), fonts(), paperpaw_app::StorePlugins::Left)
        .build()
        .unwrap();
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
fn the_crates_listed_are_those_installed() {
    let kernel = paperpaw_app::install(builder(), fonts()).build().unwrap();
    let mut installed: Vec<String> = plugins(&kernel)
        .iter()
        .filter_map(|(id, _)| crate_of(id))
        .collect();
    installed.sort();
    installed.dedup();
    let listed: Vec<&str> = paperpaw_app::CRATES.split(',').collect();
    assert_eq!(installed, listed);
}

#[cfg(feature = "store-plugins")]
#[test]
fn built_in_store_plugins_are_off_unless_used() {
    let used = ["stats"];
    let kernel = paperpaw_app::install_with(
        builder(),
        fonts(),
        paperpaw_app::StorePlugins::Off(&|id| used.contains(&id)),
    )
    .build()
    .unwrap();
    let plugins = plugins(&kernel);
    for id in paperpaw_app::STORE_PLUGINS {
        let state = plugins
            .iter()
            .find(|(p, _)| p == id)
            .map(|(_, state)| *state)
            .unwrap_or_else(|| panic!("{id} is not built in"));
        let want = if used.contains(&id) {
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

#[cfg(feature = "store-plugins")]
#[test]
fn a_store_plugin_switched_off_before_stays_off() {
    // Android, for a reader who ran PaperPaw before and switched one off.
    let storage = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[core]\nlanguage = en\n\n[plugins]\nrules = off\n",
        );
    let ran_before = paperpaw_app::ran_before(&storage);
    let kernel = paperpaw_app::install_with(
        Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[help]\nshow = false\n"),
        fonts(),
        paperpaw_app::StorePlugins::Off(&|_| ran_before),
    )
    .build()
    .unwrap();
    let plugins = plugins(&kernel);
    let state = |id: &str| plugins.iter().find(|(p, _)| p == id).map(|(_, s)| *s);
    assert_eq!(state("aids"), Some(PluginState::Running));
    assert_eq!(state("stats"), Some(PluginState::Running));
    assert_eq!(state("rules"), Some(PluginState::Off));
    // Off as it always shipped.
    assert_eq!(state("browser"), Some(PluginState::Off));
}

#[cfg(feature = "store-plugins")]
#[test]
fn every_store_plugin_is_one_of_the_app() {
    // The list the shells use names every plugin the feature builds in.
    let all = paperpaw_app::install(builder(), fonts()).build().unwrap();
    let lean = paperpaw_app::install_with(builder(), fonts(), paperpaw_app::StorePlugins::Left)
        .build()
        .unwrap();
    let lean: Vec<String> = plugins(&lean).into_iter().map(|(id, _)| id).collect();
    let mut offered: Vec<String> = plugins(&all)
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| !lean.contains(id) && id != "diagnostics")
        .collect();
    offered.sort();
    assert_eq!(offered, paperpaw_app::STORE_PLUGINS);
}
