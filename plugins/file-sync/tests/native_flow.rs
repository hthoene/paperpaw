//! The real plugin driven by the scripted host: it makes itself and registers
//! its settings, screens, actions, menu entries, widgets and events, all of
//! which the host accepts.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_file_sync::NATIVE_ENTRY;
use paperpaw_scripted_host::ScriptedHost;

#[test]
fn the_plugin_registers_what_it_declares() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let _p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // The settings (the group and `when`, then each pair's), the screens, the
    // three widgets (batches of their options), navigation, and the events in
    // four batches.
    assert_eq!(host.registrations().len(), 16);
}

/// A run that cannot go on (here the password cannot be read) ends, said:
/// the callback does not fail (that would drop the notice), and the next run
/// starts anew.
#[test]
fn a_run_that_breaks_off_is_said_and_ends() {
    use paperpaw_core::native_host::{environment::SettingValueRef, registrations::*};
    use paperpaw_native::host_glue::ERR_IO;
    use paperpaw_plugin_file_sync::declarations::SYNC_TOKEN;
    use paperpaw_scripted_host::event_batch;

    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    host.service("core.ui", 1, 0, |_, _| Ok(Vec::new()));
    host.reply_bound("core.ui", 1, 64);
    host.service("core.secrets", 1, 0, |_, _| Err(ERR_IO));
    host.set_setting("file-sync.folder-1", SettingValueRef::Text("Notes"));
    host.set_setting(
        "file-sync.server-1",
        SettingValueRef::Text("https://cloud.example.org/dav/Notes/"),
    );
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let sync = event_batch(&Record::EventActionRun(EventActionRun {
        now_ms: 0,
        action: "file-sync.sync",
        flags: 0,
    }))
    .unwrap();
    for _ in 0..2 {
        let run = p.event(SYNC_TOKEN, &sync).unwrap();
        assert_eq!(run.status, 0);
        assert!(run.batches_contain(b"Syncing broke off"));
    }
}
