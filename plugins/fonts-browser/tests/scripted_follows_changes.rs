//! The pages follow what changes outside them (as interface 6's did), driven
//! by the scripted host: a reading or status font chosen elsewhere, or a font
//! installed or removed (`event.plugins_changed`), reads the installed fonts
//! again while a page of the plugin is open; another setting, or any change
//! with no page open (a store install), reads nothing.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::SettingValueRef;
use paperpaw_core::native_host::registrations::{
    EventPluginsChanged, EventScreenOpened, EventScreenShown, EventSettingChanged, Record,
    ServiceDevDevice, record_encode_one,
};
use paperpaw_plugin_fonts_browser::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};

/// `declarations.rs`: the tokens of `event.setting_changed` and
/// `event.plugins_changed`.
const SETTING: u32 = 5;
const PLUGINS: u32 = 6;
/// `declarations.rs`: the token of the screen `fonts-browser.font`.
const FONT: u32 = 10;
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
    // A small device (packs, not font files), the catalog's address.
    host.service("core.device", 1, 0, |op, _| {
        if op != 1 {
            return Err(-1);
        }
        let mut out = vec![0; 512];
        let used = record_encode_one(
            &Record::ServiceDevDevice(ServiceDevDevice {
                id: "test",
                model: "Test",
                catalog: "",
                class: 0,
                caps: 0,
                controls: 0,
                tls_sessions: 0,
                start: 0,
                slept_ms: 0,
                flags: 0,
            }),
            &mut out,
        )
        .map_err(|_| -1)?;
        out.truncate(used);
        Ok(out)
    });
    host.set_setting(
        "fonts-browser.catalog",
        SettingValueRef::Text("https://paperpaw.org/api/v1/fonts.json"),
    );
    // One handle: each `registry()` puts a new one in place.
    let registry = host.registry();
    registry.set_rows(OP_PLUGINS, Vec::new());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let plugins = event_batch(&Record::EventPluginsChanged(EventPluginsChanged {
        now_ms: 0,
    }))
    .unwrap();
    // No page open: a store install or a font chosen reads nothing.
    plugin.event(PLUGINS, &plugins).unwrap();
    plugin
        .event(SETTING, &changed("core.reading-font"))
        .unwrap();
    assert_eq!(registry.pages(), 0);
    // A font's page opens (it reads the installed fonts once).
    let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "fonts-browser.font",
        instance: 1,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(FONT, &opened).unwrap().status, 0);
    let shown = event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen: "fonts-browser.font",
        width: 480,
        height: 800,
        instance: Some(1),
        last_input: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(FONT, &shown).unwrap().status, 0);
    assert_eq!(registry.pages(), 1);
    // Another setting: nothing is read.
    plugin.event(SETTING, &changed("core.language")).unwrap();
    assert_eq!(registry.pages(), 1);
    // The reading font, then the status font: read again each time.
    plugin
        .event(SETTING, &changed("core.reading-font"))
        .unwrap();
    assert_eq!(registry.pages(), 2);
    plugin.event(SETTING, &changed("core.status-font")).unwrap();
    assert_eq!(registry.pages(), 3);
    // A font installed or removed.
    plugin.event(PLUGINS, &plugins).unwrap();
    assert_eq!(registry.pages(), 4);
}
