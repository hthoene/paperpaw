//! The plugin made and driven by the scripted host: the action that opens the
//! to-do list and the events of its screen reach the plugin (they are decoded
//! with the kinds it lists), so the screen opens, is shown and closes.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventScreenClosed, EventScreenOpened, EventScreenShown, Record,
};
use paperpaw_plugin_widgets::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};

/// The tokens the plugin declares for the to-do list's screen and action.
const SCREEN: u32 = 10;
const OPEN: u32 = 11;

fn opened(instance: u32) -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "widgets.todo.list",
        instance,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

#[test]
fn the_open_action_reaches_the_plugin_and_opens_the_screen() {
    let host = ScriptedHost::new();
    let _dialogs = host.dialogs();
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let batch = event_batch(&Record::EventActionRun(EventActionRun {
        now_ms: 0,
        action: "widgets.todo.open",
        flags: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    let done = plugin.event(OPEN, &batch).unwrap();
    assert_eq!(done.status, 0);
    assert_eq!(done.commands.len(), 1, "one command: open the screen");
}

#[test]
fn the_screens_events_reach_the_plugin() {
    let host = ScriptedHost::new();
    let _dialogs = host.dialogs();
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    // Opened: the list is read, so the plugin asks the host for it (an event
    // that never arrived would give status 0 and no call at all).
    let done = plugin.event(SCREEN, &opened(7));
    let reached = match &done {
        Ok(done) => done.status != 0 || !done.commands.is_empty() || !done.batches.is_empty(),
        Err(_) => true,
    };
    assert!(reached, "ScreenOpened was ignored: {done:?}");
    let shown = event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen: "widgets.todo.list",
        width: 480,
        height: 800,
        instance: Some(7),
        last_input: 0,
    }))
    .unwrap();
    let _ = plugin.event(SCREEN, &shown).unwrap();
    let closed = event_batch(&Record::EventScreenClosed(EventScreenClosed {
        now_ms: 0,
        screen: "widgets.todo.list",
        instance: 7,
    }))
    .unwrap();
    assert_eq!(plugin.event(SCREEN, &closed).unwrap().status, 0);
}
