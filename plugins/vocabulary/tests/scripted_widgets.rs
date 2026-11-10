//! The plugin made and driven by the scripted host: its widgets are declared
//! with it (the host admits them), a screen's widget gets an instance and loses
//! it, and a widget that is not the plugin's is none of its business.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{EventWidgetDropped, EventWidgetMade, Record};
use paperpaw_plugin_vocabulary::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};

const WORD: u32 = 20;
const SAVED: u32 = 21;
const DUE: u32 = 22;

fn made(widget: &str, instance: u32) -> Vec<u8> {
    event_batch(&Record::EventWidgetMade(EventWidgetMade {
        now_ms: 0,
        widget,
        instance,
        place: "core.home",
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
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert!(!host.registrations().is_empty());
    for (token, widget, instance) in [
        (WORD, "vocabulary.widget.word", 5),
        (WORD, "vocabulary.widget.word@home.page-1/0", 6),
        (SAVED, "vocabulary.widget.saved", 7),
        (DUE, "vocabulary.widget.due", 10),
        (DUE, "vocabulary.widget.due@home.page-2/1", 11),
        // Not one of its widgets: nothing is kept, nothing refused.
        (SAVED, "other.widget", 8),
    ] {
        let done = plugin.event(token, &made(widget, instance)).unwrap();
        assert_eq!(done.status, 0, "{widget}");
        assert!(done.commands.is_empty());
    }
    for instance in [5, 6, 7, 8, 9, 10, 11] {
        let done = plugin.event(WORD, &dropped(instance)).unwrap();
        assert_eq!(done.status, 0, "{instance}");
    }
}
