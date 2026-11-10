//! Which linked native plugins are on (`paperpaw_app::native_default_on`) and
//! how the plugin manager sees them: the plugins the store offers as files keep
//! the state they ship in, as their interface-6 forms had it in
//! `install_with`; an off plugin is listed and not hosted.
#![cfg(feature = "native-linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use paperpaw_app::{NATIVE_PLUGINS, STORE_PLUGINS, StorePlugins, native_default_on};
use paperpaw_core::{Kernel, KernelBuilder};
use paperpaw_platform::{DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation};
use paperpaw_sdk::{MemoryStorage, PluginState};

#[test]
fn a_plugin_the_store_does_not_offer_is_always_on() {
    for id in ["menu", "settings", "files", "about"] {
        assert!(native_default_on(id, StorePlugins::Left), "{id}");
        assert!(native_default_on(id, StorePlugins::On), "{id}");
    }
}

#[test]
fn where_the_store_installs_them_none_is_on() {
    for id in STORE_PLUGINS {
        assert!(!native_default_on(id, StorePlugins::Left), "{id}");
    }
}

#[test]
fn built_in_and_on_the_store_plugins_are_on_except_the_ones_shipped_off() {
    for id in STORE_PLUGINS {
        let shipped_off = matches!(id, "mail" | "notes-vault" | "picture-home");
        assert_eq!(
            native_default_on(id, StorePlugins::On),
            !shipped_off,
            "{id}"
        );
    }
}

#[test]
fn mail_is_always_shipped_off() {
    for store in [StorePlugins::On, StorePlugins::Off(&|_| true)] {
        assert!(!native_default_on("mail", store));
    }
}

#[test]
fn off_unless_used_before_follows_the_function() {
    let used = |id: &str| id == "aids";
    assert!(native_default_on("aids", StorePlugins::Off(&used)));
    assert!(!native_default_on("notes-vault", StorePlugins::Off(&used)));
}

#[test]
fn every_linked_plugin_has_a_version_of_its_crate() {
    for (id, _) in NATIVE_PLUGINS {
        let version = paperpaw_app::native_version(id).unwrap_or_else(|| panic!("{id}"));
        assert!(
            paperpaw_core::sdk::Version::parse(version).is_some(),
            "{id}: {version}"
        );
    }
    assert_eq!(paperpaw_app::native_version("no-such-plugin"), None);
}

#[cfg(feature = "native-linked-store")]
#[test]
fn the_ids_of_the_table_are_the_plugins_ids_and_there_are_no_two() {
    let ids: Vec<&str> = NATIVE_PLUGINS.iter().map(|(id, _)| *id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len());
    // The store plugins whose ports are finished are in the table, behind
    // their feature.
    for id in ["aids", "mail", "notes-vault", "widgets"] {
        assert!(ids.contains(&id), "{id}");
        assert!(STORE_PLUGINS.contains(&id), "{id}");
    }
    // The ones that are not finished, or that the interface-6 reader or
    // vocabulary still needs, are not.
    for id in [
        "home",
        "reader",
        "news",
        "picture-home",
        "vocabulary",
        "dictionary",
        "format-txt",
        "format-xtc",
        "format-docx",
        "format-images",
        "format-mobi",
        "sleep",
    ] {
        assert!(!ids.contains(&id), "{id}");
    }
}

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

fn kernel(config: &str) -> Kernel {
    let builder: KernelBuilder = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new().with(".paperpaw/plugins/setup/done", ""))
        .user_config(format!("[core]\nlanguage = en\n{config}"));
    native_activation::build(paperpaw_app::install(
        builder,
        #[cfg(any(feature = "packs", not(feature = "outline")))]
        paperpaw_app::Fonts::Packs,
        #[cfg(all(feature = "outline", not(feature = "packs")))]
        paperpaw_app::Fonts::Outline,
    ))
}

fn state(kernel: &Kernel, id: &str) -> Option<(PluginState, bool)> {
    kernel
        .plugins()
        .iter()
        .find(|plugin| plugin.manifest.id.as_str() == id)
        .map(|plugin| (plugin.state, plugin.enabled))
}

#[test]
fn the_plugin_manager_lists_the_linked_plugins_and_a_shipped_off_one_is_off() {
    let kernel = kernel("");
    // Listed with a name of its own, whether hosted or not.
    let menu = kernel
        .plugins()
        .iter()
        .find(|plugin| plugin.manifest.id.as_str() == "menu")
        .expect("menu is listed");
    assert_eq!(menu.manifest.name.as_str(), "Menu");
    assert_eq!(
        Some(menu.manifest.version.to_string()),
        paperpaw_app::native_version("menu").map(String::from)
    );
    #[cfg(feature = "native-linked-store")]
    {
        // Mail ships off: listed, not hosted, switched off.
        assert_eq!(state(&kernel, "mail"), Some((PluginState::Off, false)));
        assert_eq!(
            state(&kernel, "notes-vault"),
            Some((PluginState::Off, false))
        );
    }
}

#[cfg(feature = "native-linked-store")]
#[test]
fn a_shipped_off_plugin_switched_on_in_the_configuration_is_hosted() {
    let kernel = kernel("[plugins]\nmail = on\n");
    let (state, enabled) = state(&kernel, "mail").expect("mail is listed");
    assert!(enabled);
    // Hosted, or failed to be: the test host backs few services.
    assert!(matches!(state, PluginState::Running | PluginState::Failed));
}

#[cfg(feature = "native-linked-store")]
#[test]
fn a_plugin_switched_off_in_the_configuration_is_not_hosted() {
    let kernel = kernel("[plugins]\nwidgets = off\n");
    assert_eq!(state(&kernel, "widgets"), Some((PluginState::Off, false)));
}
