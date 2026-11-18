//! The real plugin driven by the scripted host through a round: what a start
//! declares (the end of the phase, the minute's tick), what the tick tells the
//! widgets, the end of a phase (the notice, the next phase, auto-start, the daily
//! goal, the front light's blink), a stale end that does nothing, and a wake
//! that finds the end again by the wall clock. The widgets' drawing and the
//! screen have tests of their own beside them.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::{SNAPSHOT_BYTES, SNAPSHOT_VERSION, SettingValueRef};
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventResume, EventSettingChanged, EventStarted, EventWidgetMade, EventWorkDue,
    Record, records_decode,
};
use paperpaw_plugin_pomodoro::{
    NATIVE_ENTRY,
    declarations::{LIFECYCLE_TOKEN, OPEN_TOKEN, SKIP_TOKEN, TOGGLE_TOKEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use paperpaw_sdk::{Id, SettingSpec};
use std::cell::RefCell;
use std::rc::Rc;

const START_UNIX: i64 = 1_800_000_000;

/// The environment the plugin reads: the host's clock and the time of day.
fn snapshot(host: &ScriptedHost, monotonic_ms: u64, unix: Option<i64>) {
    let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
    bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
    // Presence: bit 2 is the Unix time.
    bytes[8..12].copy_from_slice(&u32::from(unix.is_some()).wrapping_mul(4).to_le_bytes());
    bytes[16..24].copy_from_slice(&monotonic_ms.to_le_bytes());
    bytes[24..32].copy_from_slice(&unix.unwrap_or(0).to_le_bytes());
    bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
    bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
    bytes[42] = 1;
    host.set_env(&bytes);
}

/// The time passes: the host's clock and the Unix time move `seconds` on.
fn at(host: &ScriptedHost, seconds: u64) {
    host.set_clock((1_000 + seconds * 1_000) * 1_000);
    snapshot(
        host,
        1_000 + seconds * 1_000,
        Some(START_UNIX + i64::try_from(seconds).unwrap()),
    );
}

/// What `core.work` was asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

fn world(host: &ScriptedHost) -> Log {
    let log: Log = Rc::default();
    host.add_catalogs_in("i18n").unwrap();
    at(host, 0);
    let seen = log.clone();
    host.service("core.work", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceWorkSpec(spec))) => seen
                .borrow_mut()
                .push(format!("declare {} after {}", spec.key, spec.after_ms)),
            (2, Some(Record::ServiceWorkKey(key))) => {
                seen.borrow_mut().push(format!("cancel {}", key.key));
            }
            _ => return Err(-1),
        }
        Ok(Vec::new())
    });
    host.reply_bound("core.work", 1, 64);
    host.reply_bound("core.work", 2, 64);
    log
}

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

fn started(p: &Plug<'_>) -> Callback {
    event(
        p,
        LIFECYCLE_TOKEN,
        Record::EventStarted(EventStarted { now_ms: 0 }),
    )
}

fn resumed(p: &Plug<'_>) -> Callback {
    event(
        p,
        LIFECYCLE_TOKEN,
        Record::EventResume(EventResume { now_ms: 0 }),
    )
}

fn action(p: &Plug<'_>, token: u32) -> Callback {
    event(
        p,
        token,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "",
            flags: 0,
        }),
    )
}

fn made(p: &Plug<'_>, widget: &str, instance: u32) -> Callback {
    event(
        p,
        LIFECYCLE_TOKEN,
        Record::EventWidgetMade(EventWidgetMade {
            now_ms: 0,
            widget,
            instance,
            place: "",
        }),
    )
}

fn due(p: &Plug<'_>, key: &str) -> Callback {
    event(
        p,
        WORK_TOKEN,
        Record::EventWorkDue(EventWorkDue {
            now_ms: 0,
            token: WORK_TOKEN,
            why: 3,
            key,
        }),
    )
}

fn setting(host: &ScriptedHost, key: &str, value: SettingValueRef<'_>) {
    host.set_setting(key, value);
}

/// What was asked of `core.work` since the last call.
fn asked(log: &Log) -> Vec<String> {
    log.borrow_mut().drain(..).collect()
}

#[test]
fn a_round_is_declared_ticked_and_ended_with_a_notice() {
    let host = ScriptedHost::new();
    let log = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(started(&p).status, 0);
    assert!(asked(&log).is_empty(), "nothing runs, nothing is asked");

    // Start: the phase ends in 25 minutes; nobody watches, so no tick.
    let start = action(&p, TOGGLE_TOKEN);
    assert_eq!(start.status, 0);
    assert_eq!(
        asked(&log),
        [
            "declare pomodoro.done after 1500000",
            "cancel pomodoro.tick"
        ]
    );
    // A widget is placed: the minutes are watched, on the minute.
    assert_eq!(made(&p, "pomodoro.timer", 4).status, 0);
    assert_eq!(
        asked(&log),
        [
            "declare pomodoro.done after 1500000",
            "declare pomodoro.tick after 60000"
        ]
    );
    // A minute on: the widget is told, once, and the next tick declared.
    at(&host, 60);
    let tick = due(&p, "pomodoro.tick");
    assert_eq!(tick.status, 0);
    assert_eq!(tick.batches.len(), 1, "the widget changed");
    assert_eq!(
        asked(&log),
        [
            "declare pomodoro.done after 1440000",
            "declare pomodoro.tick after 60000"
        ]
    );
    // The end: told, and the break stands ready, not running.
    at(&host, 1_500);
    let end = due(&p, "pomodoro.done");
    assert_eq!(end.status, 0);
    assert!(
        end.batches_contain(b"Time for a break"),
        "{:?}",
        end.batches
    );
    assert_eq!(
        asked(&log),
        ["cancel pomodoro.done", "cancel pomodoro.tick"]
    );
    // A tap on the break starts it: five minutes.
    action(&p, TOGGLE_TOKEN);
    assert_eq!(asked(&log)[0], "declare pomodoro.done after 300000");
}

#[test]
fn an_end_that_is_stale_does_nothing() {
    let host = ScriptedHost::new();
    let log = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    action(&p, TOGGLE_TOKEN);
    asked(&log);
    // The work of an earlier start comes due ten seconds in: not this phase's end.
    at(&host, 10);
    let stale = due(&p, "pomodoro.done");
    assert_eq!(stale.status, 0);
    assert!(stale.batches.is_empty(), "no notice");
    assert_eq!(asked(&log)[0], "declare pomodoro.done after 1490000");
    // Paused: nothing is declared any more.
    action(&p, TOGGLE_TOKEN);
    assert_eq!(
        asked(&log),
        ["cancel pomodoro.done", "cancel pomodoro.tick"]
    );
}

#[test]
fn auto_start_goes_on_by_itself() {
    let host = ScriptedHost::new();
    let log = world(&host);
    setting(&host, "pomodoro.auto", SettingValueRef::Bool(true));
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    action(&p, TOGGLE_TOKEN);
    asked(&log);
    at(&host, 1_500);
    assert_eq!(due(&p, "pomodoro.done").status, 0);
    assert_eq!(asked(&log)[0], "declare pomodoro.done after 300000");
    // The break over, the next round runs on by itself too: 25 minutes.
    at(&host, 1_800);
    due(&p, "pomodoro.done");
    assert_eq!(asked(&log)[0], "declare pomodoro.done after 1500000");
}

#[test]
fn the_daily_goal_is_told_when_reached() {
    let host = ScriptedHost::new();
    let log = world(&host);
    setting(&host, "pomodoro.goal", SettingValueRef::Int(1));
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    action(&p, TOGGLE_TOKEN);
    asked(&log);
    at(&host, 1_500);
    let end = due(&p, "pomodoro.done");
    assert!(
        end.batches_contain(b"Daily goal reached: 1 round"),
        "{:?}",
        end.batches
    );
}

#[test]
fn skipping_counts_nothing_and_stands_at_the_next_phase() {
    let host = ScriptedHost::new();
    let log = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    action(&p, TOGGLE_TOKEN);
    asked(&log);
    let skip = action(&p, SKIP_TOKEN);
    assert_eq!(skip.status, 0);
    assert!(skip.batches.is_empty(), "no notice for a skip");
    assert_eq!(
        asked(&log),
        ["cancel pomodoro.done", "cancel pomodoro.tick"]
    );
}

#[test]
fn the_light_blinks_and_comes_back() {
    let mut host = ScriptedHost::new();
    host.settings_mut()
        .define(SettingSpec::number(
            "core.frontlight",
            "Front light",
            0,
            100,
            0,
            Id::new_static("pomodoro.group"),
        ))
        .unwrap();
    let log = world(&host);
    setting(
        &host,
        "pomodoro.signal",
        SettingValueRef::Text("pomodoro.signal.light"),
    );
    setting(&host, "core.frontlight", SettingValueRef::Int(30));
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    action(&p, TOGGLE_TOKEN);
    asked(&log);
    at(&host, 1_500);
    let end = due(&p, "pomodoro.done");
    assert!(end.batches_contain(b"core.frontlight"), "to the other end");
    assert_eq!(end.status, 0);
    let work = asked(&log);
    assert!(
        work.contains(&"declare pomodoro.light after 1000".to_string()),
        "{work:?}"
    );
    let back = due(&p, "pomodoro.light");
    assert_eq!(back.status, 0);
    assert!(
        back.batches_contain(b"core.frontlight"),
        "and back as it was"
    );
}

#[test]
fn a_wake_finds_the_end_by_the_wall_clock() {
    let host = ScriptedHost::new();
    let log = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    action(&p, TOGGLE_TOKEN);
    asked(&log);
    // Ten minutes asleep; the host's clock began again at one second.
    host.set_clock(1_000_000);
    snapshot(&host, 1_000, Some(START_UNIX + 600));
    assert_eq!(resumed(&p).status, 0);
    assert_eq!(asked(&log)[0], "declare pomodoro.done after 900000");
    // Asleep past the end: told at the wake.
    snapshot(&host, 2_000, Some(START_UNIX + 1_600));
    let late = resumed(&p);
    assert!(
        late.batches_contain(b"Time for a break"),
        "{:?}",
        late.batches
    );
}

#[test]
fn what_is_not_ours_is_left_alone() {
    let host = ScriptedHost::new();
    let log = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    assert_eq!(made(&p, "weather.now", 9).status, 0);
    assert!(asked(&log).is_empty());
    let other = event(
        &p,
        LIFECYCLE_TOKEN,
        Record::EventSettingChanged(EventSettingChanged {
            now_ms: 0,
            key: "weather.place",
            old: &[],
        }),
    );
    assert!(other.batches.is_empty());
    // The action that opens the screen opens it.
    let open = action(&p, OPEN_TOKEN);
    assert!(open.batches_contain(b"pomodoro.screen"));
}
