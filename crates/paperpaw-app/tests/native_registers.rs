//! Which linked native plugins register (`paperpaw_app::native_registers`):
//! the plugins the store offers as files keep the state they ship in, as their
//! interface-6 forms had it in `install_with`.
#![cfg(feature = "native-linked")]

use paperpaw_app::{NATIVE_PLUGINS, STORE_PLUGINS, StorePlugins, native_registers};

#[test]
fn a_plugin_the_store_does_not_offer_always_registers() {
    for id in ["menu", "settings", "files", "about"] {
        assert!(native_registers(id, StorePlugins::Left, &[]), "{id}");
        assert!(
            native_registers(id, StorePlugins::Left, &["[plugins]\nmenu = off\n"]),
            "{id}: the configuration names store plugins only"
        );
    }
}

#[test]
fn where_the_store_installs_them_none_is_linked() {
    for id in STORE_PLUGINS {
        assert!(!native_registers(id, StorePlugins::Left, &[]), "{id}");
        // Not even switched on: there is no linked plugin to switch on.
        assert!(
            !native_registers(
                id,
                StorePlugins::Left,
                &[&format!("[plugins]\n{id} = on\n")]
            ),
            "{id}"
        );
    }
}

#[test]
fn built_in_and_on_the_store_plugins_register_except_the_ones_shipped_off() {
    for id in STORE_PLUGINS {
        let shipped_off = matches!(id, "mail" | "notes-vault" | "picture-home");
        assert_eq!(
            native_registers(id, StorePlugins::On, &[]),
            !shipped_off,
            "{id}"
        );
    }
}

#[test]
fn mail_is_off_until_the_reader_switches_it_on() {
    for store in [StorePlugins::On, StorePlugins::Off(&|_| true)] {
        assert!(!native_registers("mail", store, &[]));
        assert!(!native_registers(
            "mail",
            store,
            &["[plugins]\nnotes-vault = on\n"]
        ));
        assert!(native_registers("mail", store, &["[plugins]\nmail = on\n"]));
        // An off wins, as in `install_with`.
        assert!(!native_registers(
            "mail",
            store,
            &["[plugins]\nmail = on\n", "[plugins]\nmail = off\n"]
        ));
    }
}

#[test]
fn notes_vault_is_off_by_default_and_on_when_switched_on() {
    assert!(!native_registers("notes-vault", StorePlugins::On, &[]));
    assert!(native_registers(
        "notes-vault",
        StorePlugins::On,
        &["[plugins]\nnotes-vault = on\n"]
    ));
    assert!(!native_registers(
        "notes-vault",
        StorePlugins::On,
        &[
            "[plugins]\nnotes-vault = on\n",
            "[plugins]\nnotes-vault = off\n"
        ]
    ));
}

#[test]
fn off_unless_used_before_follows_the_function() {
    let used = |id: &str| id == "aids";
    assert!(native_registers("aids", StorePlugins::Off(&used), &[]));
    assert!(!native_registers(
        "notes-vault",
        StorePlugins::Off(&used),
        &[]
    ));
    assert!(native_registers(
        "notes-vault",
        StorePlugins::Off(&used),
        &["[plugins]\nnotes-vault = on\n"]
    ));
    assert!(!native_registers(
        "aids",
        StorePlugins::Off(&used),
        &["[plugins]\naids = off\n"]
    ));
}

#[test]
fn a_switch_in_another_section_or_for_another_plugin_does_nothing() {
    let config = "[reader]\nmail = on\n[plugins]\nnews = on\n";
    assert!(!native_registers("mail", StorePlugins::On, &[config]));
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
    // The ones that are not finished, or that the interface-6 reader still
    // needs (the formats), are not.
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
    ] {
        assert!(!ids.contains(&id), "{id}");
    }
}
