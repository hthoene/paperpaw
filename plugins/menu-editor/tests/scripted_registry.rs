//! The registry-change event (75) over the scripted host: with no screen open
//! the plugin has nothing to refresh and reads nothing; with its screen open it
//! reads the rows from the registry again. The redraw it then queues is a screen
//! command that the scripted host cannot take (no screen stack, BLOCKED H1).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{EventRegistryChanged, EventScreenOpened, Record};
use paperpaw_plugin_menu_editor::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};

/// The tokens of `declarations.rs`: the registry subscription and the menus screen.
const EVENTS: u32 = 2;
const SCREEN: u32 = 10;
const INSTANCE: u32 = 7;

fn opened() -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "menu-editor.menus",
        instance: INSTANCE,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

fn changed(what: u32) -> Vec<u8> {
    event_batch(&Record::EventRegistryChanged(EventRegistryChanged {
        now_ms: 0,
        what,
    }))
    .unwrap()
}

#[test]
fn a_registry_change_reads_the_rows_of_an_open_screen_again() {
    let host = ScriptedHost::new();
    // The plugin's texts and the core's (action names, `core.action.nothing`).
    for folder in [
        concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/paperpaw-core/i18n"
        ),
    ] {
        host.add_catalogs_in(folder).unwrap();
    }
    let _dialogs = host.dialogs();
    let registry = host.registry();
    // Every list op the screen reads answers an empty page.
    for op in 0..64 {
        registry.set_rows(op, Vec::new());
    }
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    // Nothing is open: nothing to refresh, nothing staged.
    let idle = plugin.event(EVENTS, &changed(1)).unwrap();
    assert_eq!(idle.status, 0);
    assert!(idle.commands.is_empty(), "{:?}", idle.commands);
    assert_eq!(
        registry.pages(),
        0,
        "nothing open: the registry is not read"
    );
    // The list screen opens, then the registry changes under it.
    let done = plugin.event(SCREEN, &opened()).unwrap();
    assert_eq!(done.status, 0);
    let read = registry.pages();
    assert!(read > 0, "opening reads the registry");
    let done = plugin.event(EVENTS, &changed(1)).unwrap();
    // The rows are read again from the registry (the refresh). The redraw that
    // follows is a screen command: the scripted host has no screen stack to
    // own it and refuses it with `Unsupported` (-1, BLOCKED H1); a host that
    // takes it answers 0.
    assert!(
        registry.pages() > read,
        "a registry change reads the rows again"
    );
    assert!(matches!(done.status, 0 | -1), "{}", done.status);
}
