//! The plugin made and driven by the scripted host: its widgets are declared
//! with it (the host admits them), a screen's widget gets an instance and loses
//! it, and a widget that is not the plugin's is none of its business.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{EventWidgetDropped, EventWidgetMade, Record};
use paperpaw_plugin_home::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};

const DATE: u32 = 20;
const CLOCK: u32 = 21;
const CARD: u32 = 22;
const SHELF: u32 = 23;

fn made(widget: &str, instance: u32) -> Vec<u8> {
    event_batch(&Record::EventWidgetMade(EventWidgetMade {
        now_ms: 0,
        widget,
        instance,
        place: "home.pages",
    }))
    .unwrap()
}

fn dropped(instance: u32) -> Vec<u8> {
    event_batch(&Record::EventWidgetDropped(EventWidgetDropped {
        now_ms: 0,
        instance,
    }))
    .unwrap()
}

#[test]
fn the_widgets_are_declared_and_a_screens_widgets_come_and_go() {
    let host = ScriptedHost::new();
    // The first callback opens `core.ui`.
    let _dialogs = host.dialogs();
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert!(!host.registrations().is_empty());
    for (token, widget, instance) in [
        (DATE, "home.date", 5),
        (DATE, "home.date@home.page-1/0", 6),
        (CLOCK, "home.clock", 7),
        (CLOCK, "home.clock@home.page-2/1", 8),
        (CARD, "home.continue", 11),
        (SHELF, "home.recent@home.page-1/2", 12),
        // Not one of its widgets: nothing is kept, nothing refused.
        (CLOCK, "other.widget", 9),
    ] {
        let done = plugin.event(token, &made(widget, instance)).unwrap();
        assert_eq!(done.status, 0, "{widget}");
        assert!(done.commands.is_empty(), "{widget}");
    }
    for instance in [5, 6, 7, 8, 9, 10, 11, 12] {
        let done = plugin.event(DATE, &dropped(instance)).unwrap();
        assert_eq!(done.status, 0, "{instance}");
    }
}
