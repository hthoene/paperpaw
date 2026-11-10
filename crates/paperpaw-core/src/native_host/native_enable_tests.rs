#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::{Kernel, config::read_config, tests::DEVICE};
use paperpaw_sdk::{
    Id, Manifest, MemoryStorage, PluginKind, PluginState, RequiredFor, Storage, Version,
};

fn kernel(file: &str, overrides: &str) -> Kernel {
    let storage = MemoryStorage::new();
    if !file.is_empty() {
        storage.write("paperpaw.ini", file.as_bytes()).unwrap();
    }
    Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(overrides)
        .build()
        .unwrap()
}

fn info(id: &'static str, state: PluginState, enabled: bool) -> PluginInfo {
    PluginInfo::new(
        Manifest::new(Id::new_static(id), id, Version::new(1, 0, 0)),
        state,
        enabled,
        RequiredFor::default(),
        PluginKind::BuiltIn,
        None,
        None,
    )
}

#[test]
fn a_plugin_shipped_off_is_off_until_the_configuration_says_on() {
    let off = kernel("", "");
    assert!(!off.native_enabled("mail", false));
    assert!(off.native_enabled("mail", true));
    let on = kernel("[plugins]\nmail = on\n", "");
    assert!(on.native_enabled("mail", false));
    assert!(!on.native_enabled("notes-vault", false));
    // The shell's overrides count as the file does.
    assert!(kernel("", "[plugins]\nmail = on\n").native_enabled("mail", false));
}

#[test]
fn off_in_the_configuration_wins_over_everything() {
    let k = kernel("[plugins]\nmail = off\n", "");
    assert!(!k.native_enabled("mail", true));
    let k = kernel("[plugins]\nmail = on\nmail = off\n", "");
    assert!(!k.native_enabled("mail", true));
    // Off in one, on in the other: off.
    let k = kernel("[plugins]\nmail = on\n", "[plugins]\nmail = off\n");
    assert!(!k.native_enabled("mail", true));
    let k = kernel("[plugins]\nmail = off\n", "[plugins]\nmail = on\n");
    assert!(!k.native_enabled("mail", false));
    // Another section, another plugin, a value that is neither: nothing said.
    let k = kernel(
        "[other]\nmail = off\n[plugins]\nmailx = off\nmail = maybe\n",
        "",
    );
    assert!(k.native_enabled("mail", true) && !k.native_enabled("mail", false));
}

#[test]
fn a_native_plugin_is_listed_off_or_running_and_not_twice() {
    let mut k = kernel("", "");
    k.list_native(info("mail", PluginState::Off, false))
        .unwrap();
    k.list_native(info("clock", PluginState::Running, true))
        .unwrap();
    let mail = k.plugins().get("mail").unwrap();
    assert_eq!((mail.state, mail.enabled), (PluginState::Off, false));
    assert!(!mail.restart_needed());
    assert_eq!(
        k.list_native(info("mail", PluginState::Off, false)),
        Err(ERR_INVALID)
    );
    assert_eq!(
        k.list_native(info("core", PluginState::Running, true)),
        Err(ERR_DENIED)
    );
}

/// The plugin manager's switch: saved in `[plugins]` for the next start, the
/// row asks for a restart, the next start's decision follows.
#[test]
fn a_switch_is_saved_and_decides_the_next_start() {
    let mut k = kernel("", "");
    k.list_native(info("mail", PluginState::Off, false))
        .unwrap();
    k.list_native(info("clock", PluginState::Running, true))
        .unwrap();
    let id = Id::new_static("mail");
    k.set_plugin_enabled(&id, true);
    assert!(k.plugins().get("mail").unwrap().restart_needed());
    let saved = read_config(k.shared.storage.as_ref()).unwrap().unwrap();
    assert!(saved.contains("mail = on"), "{saved}");
    assert!(k.native_enabled("mail", false));
    // And back off, and a running one off: both stay as they run until the
    // restart; the decision for it follows the file.
    k.set_plugin_enabled(&id, false);
    assert!(!k.plugins().get("mail").unwrap().restart_needed());
    assert!(!k.native_enabled("mail", true));
    k.set_plugin_enabled(&Id::new_static("clock"), false);
    assert!(k.plugins().get("clock").unwrap().restart_needed());
    assert!(!k.native_enabled("clock", true));
}

#[test]
fn a_removed_native_plugin_leaves_the_list() {
    let mut k = kernel("", "");
    k.list_native(info("mail", PluginState::Off, false))
        .unwrap();
    k.list_native(info("clock", PluginState::Running, true))
        .unwrap();
    assert!(k.unload_plugin(&Id::new_static("mail")));
    assert!(k.unload_plugin(&Id::new_static("clock")));
    assert!(k.plugins().get("mail").is_none() && k.plugins().get("clock").is_none());
    // A plugin of the same id may be listed again (an install after a removal).
    k.list_native(info("mail", PluginState::Off, false))
        .unwrap();
}
