//! The real plugin driven by the scripted host: it makes itself and registers
//! its settings, screens, actions, menu entry and events, all of which the host
//! accepts.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_picture_frame::NATIVE_ENTRY;
use paperpaw_scripted_host::ScriptedHost;

#[test]
fn the_plugin_registers_what_it_declares() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let _p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // Settings, screens, the widget (batches of its options), navigation, and the events in four batches.
    assert_eq!(host.registrations().len(), 12);
}
