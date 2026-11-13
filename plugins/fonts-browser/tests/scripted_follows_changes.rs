//! The pages follow what changes outside them (as interface 6's did), driven
//! by the scripted host: a reading or status font chosen elsewhere, or a font
//! installed or removed (`event.plugins_changed`), reads the installed fonts
//! again; another setting reads nothing.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{EventPluginsChanged, EventSettingChanged, Record};
use paperpaw_plugin_fonts_browser::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};

/// `declarations.rs`: the tokens of `event.setting_changed` and
/// `event.plugins_changed`.
const SETTING: u32 = 5;
const PLUGINS: u32 = 6;
/// `core.registry`'s list of plugins (`OP_PLUGINS`).
const OP_PLUGINS: u32 = 5;

fn changed(key: &str) -> Vec<u8> {
    event_batch(&Record::EventSettingChanged(EventSettingChanged {
        now_ms: 0,
        key,
        old: b"",
    }))
    .unwrap()
}

#[test]
fn a_font_chosen_installed_or_removed_elsewhere_is_read_again() {
    let host = ScriptedHost::new();
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    // One handle: each `registry()` puts a new one in place.
    let registry = host.registry();
    registry.set_rows(OP_PLUGINS, Vec::new());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    // Another setting: nothing is read.
    plugin.event(SETTING, &changed("core.language")).unwrap();
    assert_eq!(registry.pages(), 0);
    // The reading font, then the status font: read again each time.
    plugin
        .event(SETTING, &changed("core.reading-font"))
        .unwrap();
    assert_eq!(registry.pages(), 1);
    plugin.event(SETTING, &changed("core.status-font")).unwrap();
    assert_eq!(registry.pages(), 2);
    // A font installed or removed.
    let plugins = event_batch(&Record::EventPluginsChanged(EventPluginsChanged {
        now_ms: 0,
    }))
    .unwrap();
    plugin.event(PLUGINS, &plugins).unwrap();
    assert_eq!(registry.pages(), 3);
}
