//! A linked plugin's settings are taken from the table its build script made,
//! not copied onto the heap: a table that differs from what the plugin
//! declares costs the memory it should save, and the debug build says so.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
#![cfg(feature = "native-linked")]

mod native_activation;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::MemoryStorage;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &["back", "page-forward", "page-back"]),
    MemoryClass::Small,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

#[test]
fn every_linked_plugin_takes_its_settings_from_its_table() {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage);
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    let _ = kernel.start(Instant(0));
    // The debug build says when a table does not equal what a plugin declared.
    #[cfg(debug_assertions)]
    {
        let unmatched: Vec<_> = kernel
            .take_warnings()
            .into_iter()
            .filter(|warning| format!("{warning:?}").contains("table does not match"))
            .collect();
        assert!(unmatched.is_empty(), "{unmatched:?}");
    }
    // A plugin with settings has its table (one that is not listed costs
    // memory): the ones that declare settings today.
    #[cfg_attr(
        not(feature = "native-linked-store"),
        allow(
            unused_mut,
            reason = "the store's plugins are added only with the feature"
        )
    )]
    let mut with_settings = vec![
        "keyboard",
        "lock",
        "help",
        "opds",
        "power",
        "bluetooth",
        "menu",
        "settings",
        "file-manager",
        "sleep",
    ];
    #[cfg(feature = "native-linked-store")]
    with_settings.extend(["aids", "fonts-browser", "picture-home"]);
    #[cfg(feature = "native-linked-reading")]
    with_settings.extend(["home", "reader", "dictionary"]);
    #[cfg(all(feature = "native-linked-reading", feature = "native-linked-store"))]
    with_settings.extend(["news", "vocabulary"]);
    for id in with_settings {
        assert!(
            !paperpaw_app::fixed_tables(id).settings.is_empty(),
            "{id} has no table"
        );
    }
    // The plugins that declare setting groups have their table of them (an
    // empty or stale one only costs memory, silently).
    #[cfg_attr(
        not(feature = "native-linked-store"),
        allow(
            unused_mut,
            reason = "the store's plugins are added only with the feature"
        )
    )]
    let mut with_groups = vec!["bluetooth", "keyboard", "lock", "power"];
    #[cfg(feature = "native-linked-store")]
    with_groups.extend(["widgets", "picture-home"]);
    #[cfg(feature = "native-linked-reading")]
    // (Reader declares no group of its own: its settings sit in core groups.)
    with_groups.push("home");
    #[cfg(all(feature = "native-linked-reading", feature = "native-linked-store"))]
    with_groups.extend(["news", "vocabulary"]);
    for id in with_groups {
        assert!(
            !paperpaw_app::fixed_tables(id).groups.is_empty(),
            "{id} has no table of groups"
        );
    }
}

/// Every linked plugin's ids and titles stay where the program keeps them
/// (its id table and its image): none is copied onto the heap, the saving of
/// #248 (about 9 KiB on the X4 for 22 plugins). A plugin added without its id
/// table, or a title the host copies again, shows here by kind and id.
#[test]
fn no_linked_plugin_keeps_a_copy_of_its_ids_or_titles() {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage);
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    let _ = kernel.start(Instant(0));
    assert!(
        kernel
            .actions()
            .iter()
            .any(|action| action.native().is_some()),
        "linked plugins with actions are registered"
    );
    let owned = kernel.owned_native_texts();
    assert!(owned.is_empty(), "{} copies: {owned:#?}", owned.len());
}

#[test]
fn every_linked_plugins_action_keeps_the_entry_of_its_titles_table() {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage);
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    let _ = kernel.start(Instant(0));
    // Each native action's id is the one of its plugin's titles table, not a
    // row of its own (#248, B): one that is not costs the memory it should
    // save.
    let mut native = 0;
    for action in kernel.actions().iter() {
        if action.native().is_none() {
            continue;
        }
        native += 1;
        let id = action.id();
        let titles = paperpaw_app::fixed_tables(id.namespace()).titles;
        assert!(
            titles
                .iter()
                .any(|kept| core::ptr::eq(&kept.id, id) && kept.title == action.title()),
            "{} keeps a row of its own",
            id.as_str()
        );
    }
    assert!(native > 10, "{native} native actions");
}

#[test]
fn every_linked_plugins_screen_keeps_the_entry_of_its_titles_table() {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage);
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    let _ = kernel.start(Instant(0));
    // Each linked plugin's screen keeps the id of its plugin's titles table,
    // not a row of its own (#248, B). Only the plugins registered native
    // (`NATIVE_PLUGINS`): one that is still interface 6 in this build (home,
    // notes-vault until activation, #243) has a table for the flip, and its
    // interface-6 screens, with the same ids, are rows of their own.
    let native = |plugin: &str| {
        paperpaw_app::NATIVE_PLUGINS
            .iter()
            .any(|(linked, _)| *linked == plugin)
    };
    let mut kept = 0;
    let mut plugins: Vec<&str> = Vec::new();
    for id in kernel.known_screens() {
        if !native(id.namespace()) {
            continue;
        }
        let titles = paperpaw_app::fixed_tables(id.namespace()).titles;
        if titles.iter().any(|entry| entry.id == *id) {
            if !plugins.contains(&id.namespace()) {
                plugins.push(id.namespace());
            }
            assert!(
                titles.iter().any(|entry| core::ptr::eq(&entry.id, id)),
                "{} keeps a row of its own",
                id.as_str()
            );
            kept += 1;
        }
    }
    assert!(!plugins.is_empty(), "no native plugin's screens checked");
    assert!(kept > 10, "{kept} screens kept");
}

/// Each linked native plugin's row in the plugin list points to its manifest
/// kept in the program, not a copy on the heap (#248, B; Senior M1, #1372).
/// The flip lists them so (`list`, with the build's table of manifests).
#[test]
fn every_linked_plugins_row_points_to_its_kept_manifest() {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage);
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    let _ = kernel.start(Instant(0));
    let mut kept = 0;
    for (id, _) in paperpaw_app::NATIVE_PLUGINS.iter() {
        let info = kernel.plugins().get(id).expect("listed");
        assert!(
            info.manifest.is_fixed(),
            "{id} holds a copy of its manifest"
        );
        kept += 1;
    }
    assert!(kept > 10, "{kept} rows kept");
}
