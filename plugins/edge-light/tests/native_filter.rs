//! The real plugin driven by the scripted host as the kernel drives it: every finger
//! goes to its input filter as a stage call, and its verdict is what the kernel acts
//! on. A swipe along the edge answers *preview* of the level the finger is at (the
//! light follows it without a card write) and, when the finger lifts, *run*
//! `edge-light.step`, whose action sets the light once; the vertical swipe the
//! finger ends with is taken; a swipe anywhere else, and a tap, pass.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::{
    SNAPSHOT_BYTES, SNAPSHOT_VERSION, SettingValueRef, settings_decode,
};
use paperpaw_core::native_host::registrations::{
    EventActionRun, InputGesture, InputPointer, Record, ServiceDevDevice, StageCall,
    record_encode_one,
};
use paperpaw_plugin_edge_light::{
    NATIVE_ENTRY,
    declarations::{FILTER_TOKEN, STEP_TOKEN},
};
use paperpaw_scripted_host::{Callback, Plugin, ScriptedHost, event_batch};
use paperpaw_sdk::native::stage_schemas::core_input_filter_v1::{FilterInput, FilterVerdict};
use paperpaw_sdk::{Id, SettingSpec};

const PASS: u32 = 0;
const TAKE: u32 = 1;
const RUN: u32 = 2;
const PREVIEW: u32 = 6;

const DOWN: u32 = 0;
const MOVE: u32 = 1;
const UP: u32 = 2;

fn snapshot(host: &ScriptedHost) {
    let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
    bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
    bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
    bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
    bytes[42] = 1;
    host.set_env(&bytes);
}

/// A device with a touch screen (bit 0) and a front light (bit 11).
fn device(host: &ScriptedHost, caps: u32) {
    host.service("core.device", 1, 0, move |op, _| {
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
                caps: u64::from(caps),
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
}

fn world(caps: u32) -> ScriptedHost {
    let mut host = ScriptedHost::new();
    for key in ["core.frontlight", "core.frontlight-warmth"] {
        host.settings_mut()
            .define(SettingSpec::number(
                key,
                "Light",
                0,
                100,
                0,
                Id::new_static("edge-light.group"),
            ))
            .unwrap();
    }
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    snapshot(&host);
    device(&host, caps);
    host.set_setting("core.frontlight", SettingValueRef::Int(40));
    host.set_setting("core.frontlight-warmth", SettingValueRef::Int(30));
    host
}

/// What the filter answered one input: the verdict's kind and action, and the
/// setting and level of a preview.
fn ask_all(plugin: &Plugin<'_>, record: &Record<'_>) -> (u32, String, String, Option<i64>) {
    let input = event_batch(record).unwrap();
    let mut payload = vec![0; 512];
    let used = FilterInput::new(&input, 0).encode(&mut payload).unwrap();
    let call = event_batch(&Record::StageCall(StageCall {
        stage: "core.input-filter",
        schema: 1,
        payload: &payload[..used],
    }))
    .unwrap();
    let (done, reply) = plugin.stage_call(FILTER_TOKEN, &call, 256).unwrap();
    assert_eq!(done.status, 0);
    let verdict = FilterVerdict::decode(&reply).unwrap();
    let level =
        (!verdict.value.is_empty()).then(|| match settings_decode(verdict.value).unwrap() {
            SettingValueRef::Int(level) => level,
            other => panic!("{other:?}"),
        });
    (
        verdict.kind,
        verdict.action.to_string(),
        verdict.key.to_string(),
        level,
    )
}

fn ask(plugin: &Plugin<'_>, record: &Record<'_>) -> (u32, String) {
    let (kind, action, ..) = ask_all(plugin, record);
    (kind, action)
}

/// The setting and level a preview shows (anything else is no preview).
fn preview(plugin: &Plugin<'_>, phase: u32, x: i32, y: i32) -> Option<(String, i64)> {
    let (kind, _, key, level) = ask_all(
        plugin,
        &Record::InputPointer(InputPointer {
            now_ms: 0,
            x,
            y,
            phase,
            contact: Some(0),
        }),
    );
    (kind == PREVIEW).then(|| (key, level.unwrap()))
}

fn finger(plugin: &Plugin<'_>, phase: u32, x: i32, y: i32) -> (u32, String) {
    ask(
        plugin,
        &Record::InputPointer(InputPointer {
            now_ms: 0,
            x,
            y,
            phase,
            contact: Some(0),
        }),
    )
}

fn swipe_up(plugin: &Plugin<'_>) -> (u32, String) {
    ask(
        plugin,
        &Record::InputGesture(InputGesture {
            now_ms: 0,
            kind: 4,
            x: 10,
            y: 700,
        }),
    )
}

fn step(plugin: &Plugin<'_>) -> Callback {
    let batch = event_batch(&Record::EventActionRun(EventActionRun {
        now_ms: 0,
        action: "",
        flags: 0,
    }))
    .unwrap();
    plugin.event(STEP_TOKEN, &batch).unwrap()
}

#[test]
fn a_swipe_up_the_left_edge_previews_the_level_and_keeps_it_when_the_finger_lifts() {
    let host = world(1 | 1 << 11);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(finger(&plugin, DOWN, 10, 700).0, PASS);
    assert_eq!(finger(&plugin, MOVE, 10, 690).0, PASS);
    // It is a swipe from here: the filter takes the finger, the level is where it was.
    assert_eq!(
        preview(&plugin, MOVE, 10, 660),
        Some(("core.frontlight".into(), 40))
    );
    // 240 pixels up are 50 %: 40 to 90. Nothing is written yet.
    assert_eq!(
        preview(&plugin, MOVE, 10, 420),
        Some(("core.frontlight".into(), 90))
    );
    // The same place again shows nothing new.
    assert_eq!(finger(&plugin, MOVE, 10, 421).0, PASS);
    // Lifted: the level is kept, once, by the action.
    assert_eq!(
        finger(&plugin, UP, 10, 421),
        (RUN, "edge-light.step".into())
    );
    let done = step(&plugin);
    assert_eq!(done.status, 0);
    assert!(done.batches_contain(b"core.frontlight"));
    assert!(
        done.batches_contain(b"Brightness 90 %"),
        "{:?}",
        done.batches
    );
    // The vertical swipe the kernel makes of it is taken: no page turns.
    assert_eq!(swipe_up(&plugin).0, TAKE);
    assert_eq!(swipe_up(&plugin).0, PASS);
}

#[test]
fn a_swipe_that_ends_where_it_began_or_is_cancelled_keeps_nothing() {
    let host = world(1 | 1 << 11);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    finger(&plugin, DOWN, 10, 700);
    assert!(preview(&plugin, MOVE, 10, 660).is_some());
    assert!(preview(&plugin, MOVE, 10, 420).is_some());
    // Back to where it began: the level is the one kept.
    assert_eq!(preview(&plugin, MOVE, 10, 660).unwrap().1, 40);
    assert_eq!(finger(&plugin, UP, 10, 660).0, PASS);
    // A cancel after a change keeps nothing either.
    finger(&plugin, DOWN, 10, 700);
    assert!(preview(&plugin, MOVE, 10, 660).is_some());
    assert!(preview(&plugin, MOVE, 10, 420).is_some());
    assert_eq!(finger(&plugin, 3, 10, 420).0, PASS);
    assert!(step(&plugin).batches.is_empty());
}

#[test]
fn the_right_edge_sets_the_warmth() {
    let host = world(1 | 1 << 11);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    finger(&plugin, DOWN, 470, 300);
    finger(&plugin, MOVE, 470, 340);
    // 96 pixels down are 20 %: 30 to 10.
    assert_eq!(
        preview(&plugin, MOVE, 470, 436),
        Some(("core.frontlight-warmth".into(), 10))
    );
    assert_eq!(finger(&plugin, UP, 470, 436).0, RUN);
    let done = step(&plugin);
    assert_eq!(done.status, 0);
    assert!(done.batches_contain(b"core.frontlight-warmth"));
    assert!(done.batches_contain(b"Warmth 10 %"), "{:?}", done.batches);
}

#[test]
fn a_swipe_away_from_the_edge_is_not_stolen() {
    let host = world(1 | 1 << 11);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    for (phase, x, y) in [
        (DOWN, 240, 700),
        (MOVE, 240, 500),
        (MOVE, 240, 300),
        (UP, 240, 300),
    ] {
        assert_eq!(finger(&plugin, phase, x, y).0, PASS);
    }
    assert_eq!(swipe_up(&plugin).0, PASS);
}

#[test]
fn a_tap_and_a_sideways_swipe_from_the_strip_pass() {
    let host = world(1 | 1 << 11);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    for (phase, x, y) in [(DOWN, 10, 400), (UP, 10, 400)] {
        assert_eq!(finger(&plugin, phase, x, y).0, PASS);
    }
    for (phase, x, y) in [
        (DOWN, 10, 400),
        (MOVE, 80, 405),
        (MOVE, 90, 700),
        (UP, 90, 700),
    ] {
        assert_eq!(finger(&plugin, phase, x, y).0, PASS);
    }
    assert_eq!(swipe_up(&plugin).0, PASS);
}

#[test]
fn a_device_without_a_front_light_or_touch_is_left_alone() {
    for caps in [1, 1 << 11, 0] {
        let host = world(caps);
        let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
        for (phase, y) in [(DOWN, 700), (MOVE, 660), (MOVE, 400), (UP, 400)] {
            assert_eq!(finger(&plugin, phase, 10, y).0, PASS);
        }
        assert_eq!(swipe_up(&plugin).0, PASS);
    }
}
