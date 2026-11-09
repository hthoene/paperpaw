//! The plugin made and driven by the scripted host: its widgets are declared
//! with it (the host admits them), a screen's widget gets an instance and loses
//! it, a widget that is not the plugin's is none of its business, and a goal
//! changed with no home screen to offer *Read today* on does nothing.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventSettingChanged, EventWidgetDropped, EventWidgetMade, Record,
};
use paperpaw_plugin_stats::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};

const TODAY: u32 = 40;
const WEEK: u32 = 41;
const EVENTS: u32 = 1;

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
    // The goal and the widgets are among what it submitted.
    let declared = host.registrations();
    assert!(!declared.is_empty());
    for (token, widget, instance) in [
        (TODAY, "stats.widget.today", 5),
        (TODAY, "stats.widget.today@home.page-1/0", 6),
        (WEEK, "stats.widget.week", 7),
        // Not one of its widgets: nothing is kept, nothing refused.
        (WEEK, "other.widget", 8),
    ] {
        let done = plugin.event(token, &made(widget, instance)).unwrap();
        assert_eq!(done.status, 0, "{widget}");
        assert!(done.commands.is_empty());
    }
    for instance in [5, 6, 7, 8, 9] {
        let done = plugin.event(TODAY, &dropped(instance)).unwrap();
        assert_eq!(done.status, 0, "{instance}");
    }
}

#[test]
fn a_goal_set_with_no_home_screen_to_offer_it_on_changes_nothing() {
    let host = ScriptedHost::new();
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let changed = event_batch(&Record::EventSettingChanged(EventSettingChanged {
        now_ms: 0,
        key: "stats.goal",
        old: &[],
    }))
    .unwrap();
    let done = plugin.event(EVENTS, &changed).unwrap();
    assert_eq!(done.status, 0);
    assert!(done.commands.is_empty());
    // Another setting is none of its business either.
    let other = event_batch(&Record::EventSettingChanged(EventSettingChanged {
        now_ms: 0,
        key: "reader.size",
        old: &[],
    }))
    .unwrap();
    assert_eq!(plugin.event(EVENTS, &other).unwrap().status, 0);
}
