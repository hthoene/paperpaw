//! The widget's tap over the scripted host: where the statistics do not count
//! it offers to switch them on or to get them, and the answer is done (the
//! command is staged), `Not now` and a cancelled question do nothing, and a
//! second tap while the question is open asks nothing more. What a tap does
//! where they count (opens the settings on the goal) is a screen command the
//! scripted host does not stage (BLOCKED H1).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventWidgetMade, InputWidget, Record, ServiceRegPlugin,
};
use paperpaw_plugin_reading_goal::NATIVE_ENTRY;
use paperpaw_scripted_host::{Answer, Dialogs, ScriptedHost, event_batch, row};

#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the tests read the tokens only")]
mod declared;

const OP_PLUGINS: u32 = 5;
const INSTANCE: u32 = 4;

struct World {
    host: ScriptedHost,
    dialogs: Dialogs,
}

/// A host with the registry listing `stats` in `state` (0 running, 1 off), or
/// not listing it.
fn world(stats: Option<u32>) -> World {
    let host = ScriptedHost::new();
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    let dialogs = host.dialogs();
    let registry = host.registry();
    let mut rows = vec![
        row(&Record::ServiceRegPlugin(ServiceRegPlugin {
            id: "reading-goal",
            name: "Reading goal",
            version: "1.0.0",
            kind: 0,
            state: 0,
            flags: 0,
            problem: 0,
            problem_arg: 0,
            update: "",
        }))
        .unwrap(),
    ];
    if let Some(state) = stats {
        rows.push(
            row(&Record::ServiceRegPlugin(ServiceRegPlugin {
                id: "stats",
                name: "Reading statistics",
                version: "1.0.0",
                kind: 0,
                state,
                flags: 0,
                problem: 0,
                problem_arg: 0,
                update: "",
            }))
            .unwrap(),
        );
    }
    registry.set_rows(OP_PLUGINS, rows);
    World { host, dialogs }
}

fn made(plugin: &paperpaw_scripted_host::Plugin<'_>) {
    let batch = event_batch(&Record::EventWidgetMade(EventWidgetMade {
        now_ms: 0,
        widget: "reading-goal.widget",
        instance: INSTANCE,
        place: "",
    }))
    .unwrap();
    assert_eq!(
        plugin.event(declared::WIDGET_TOKEN, &batch).unwrap().status,
        0
    );
}

fn tap(plugin: &paperpaw_scripted_host::Plugin<'_>) -> i32 {
    let batch = event_batch(&Record::InputWidget(InputWidget {
        now_ms: 0,
        instance: INSTANCE,
        target: 1,
        x: 1,
        y: 1,
        kind: 0,
    }))
    .unwrap();
    plugin.input(declared::WIDGET_TOKEN, &batch).unwrap().status
}

#[test]
fn off_statistics_are_offered_to_be_switched_on() {
    let w = world(Some(1));
    let plugin = w.host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&plugin);
    assert_eq!(tap(&plugin), 1);
    let question = w.dialogs.last().unwrap();
    // A choice (kind 1): switch on, or not now.
    assert_eq!(question.kind, 1);
    assert_eq!(
        question.title,
        "The cat needs Reading statistics to count the minutes you read."
    );
    assert_eq!(
        question.choices,
        "Switch them on (PaperPaw starts anew)\nNot now"
    );
    // The first choice: stage the switch-on of `stats`.
    let answer = w
        .dialogs
        .answer(question.request, &Answer::Choice(0))
        .unwrap();
    let done = plugin.event(declared::WIDGET_TOKEN, &answer).unwrap();
    assert!(done.batches_contain(b"stats"), "{done:?}");
}

#[test]
fn missing_statistics_are_offered_from_the_store() {
    let w = world(None);
    let plugin = w.host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&plugin);
    assert_eq!(tap(&plugin), 1);
    let question = w.dialogs.last().unwrap();
    assert_eq!(question.choices, "Get them from the store\nNot now");
    let answer = w
        .dialogs
        .answer(question.request, &Answer::Choice(0))
        .unwrap();
    let done = plugin.event(declared::WIDGET_TOKEN, &answer).unwrap();
    assert!(done.batches_contain(b"store.open"), "{done:?}");
}

#[test]
fn not_now_and_a_cancelled_question_do_nothing() {
    for answer in [Answer::Choice(1), Answer::Cancelled] {
        let w = world(Some(1));
        let plugin = w.host.load(NATIVE_ENTRY.entry()).unwrap();
        made(&plugin);
        assert_eq!(tap(&plugin), 1);
        let question = w.dialogs.last().unwrap();
        let answered = w.dialogs.answer(question.request, &answer).unwrap();
        let done = plugin.event(declared::WIDGET_TOKEN, &answered).unwrap();
        assert_eq!(done.status, 0);
        assert!(done.batches.is_empty(), "{done:?}");
    }
}

#[test]
fn a_second_tap_while_the_question_is_open_asks_nothing_more() {
    let w = world(Some(1));
    let plugin = w.host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&plugin);
    assert_eq!(tap(&plugin), 1);
    assert_eq!(tap(&plugin), 1);
    assert_eq!(w.dialogs.questions().len(), 1);
}
