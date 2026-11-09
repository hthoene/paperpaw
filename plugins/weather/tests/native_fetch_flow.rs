//! The real plugin driven by the scripted host through the fetching of the
//! weather: a placed widget is a reason to look, the request as given to the
//! host, the answer read in pieces, what the widgets are told, the places'
//! change, a failed fetch, no network, the offline mode and the work that looks
//! again. The widgets' drawing and the place screen have tests of their own.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::{
    SNAPSHOT_BYTES, SNAPSHOT_VERSION, SettingValueRef,
};
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventHttpDone, EventSettingChanged, EventStarted, EventWidgetMade,
    EventWorkDue, Record, ServiceDevPower, ServiceDevWifi, ServiceNetAnswer, ServiceNetBytes,
    ServiceNetStarted, record_encode_one, records_decode,
};
use paperpaw_plugin_weather::{
    NATIVE_ENTRY,
    declarations::{
        FORECAST_TOKEN, HTTP_TOKEN, LIFECYCLE_TOKEN, NOW_TOKEN, REFRESH_TOKEN,
        WORK_TOKEN,
    },
};
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const FORECAST: &str = include_str!("../testdata/forecast.json");
const BERLIN: &str = "52.52,13.42";

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// What the services were asked, in order, and how the world is.
#[derive(Default)]
struct World {
    log: Rc<RefCell<Vec<String>>>,
    /// The Wi-Fi state: 3 is online.
    wifi: Rc<Cell<u32>>,
    /// The power level: 0 normal, 1 saving, 2 offline.
    level: Rc<Cell<u32>>,
}

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
    // Mono1: kind 0, code 0, 1 bit.
    bytes[42] = 1;
    host.set_env(&bytes);
}

fn world(host: &ScriptedHost, status: u32) -> World {
    let world = World::default();
    world.wifi.set(3);
    host.add_catalogs_in("i18n").unwrap();
    host.set_setting("weather.coordinates", SettingValueRef::Text(BERLIN));
    host.set_setting("weather.place", SettingValueRef::Text("Berlin"));
    host.set_setting("weather.every", SettingValueRef::Text("weather.every.1"));
    snapshot(host, 1_000, Some(1_800_000_000));
    // `core.net`: every request answers with `status` and the forecast.
    let (log, next) = (world.log.clone(), RefCell::new(0_u32));
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                let agent = rows.iter().find_map(|r| match r {
                    Record::ServiceNetHeader(h) if h.name == "User-Agent" => Some(h.value),
                    _ => None,
                });
                log.borrow_mut().push(format!(
                    "request {} {} {} token {} max {}",
                    request.method,
                    request.url,
                    agent.unwrap_or(""),
                    request.token,
                    request.max_body
                ));
                *next.borrow_mut() += 1;
                Ok(stream(&[Record::ServiceNetStarted(ServiceNetStarted {
                    id: *next.borrow(),
                })]))
            }
            (2, _) => {
                log.borrow_mut().push("cancel".to_string());
                Ok(Vec::new())
            }
            (3, _) => Ok(stream(&[Record::ServiceNetAnswer(ServiceNetAnswer {
                status,
                flags: 0,
                body_len: u32::try_from(FORECAST.len()).unwrap(),
                etag: "",
                content_type: "",
                final_url: "",
            })])),
            (4, Some(Record::ServiceNetRead(read))) => {
                let from = (read.offset as usize).min(FORECAST.len());
                Ok(stream(&[Record::ServiceNetBytes(ServiceNetBytes {
                    data: &FORECAST.as_bytes()[from..],
                })]))
            }
            _ => Err(-1),
        }
    });
    let log = world.log.clone();
    host.service("core.work", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        log.borrow_mut().push(format!("work op {op}"));
        if let (1, Some(Record::ServiceWorkSpec(spec))) = (op, batch.iter().next()) {
            log.borrow_mut().push(format!(
                "work {} token {} needs {} flags {} moments {} after {}",
                spec.key, spec.token, spec.needs, spec.flags, spec.moments, spec.after_ms
            ));
            return Ok(Vec::new());
        }
        Err(-1)
    });
    host.reply_bound("core.work", 1, 64);
    let (wifi, level) = (world.wifi.clone(), world.level.clone());
    host.service("core.device", 1, 0, move |op, input| match op {
        8 => Ok(stream(&[Record::ServiceDevWifi(ServiceDevWifi {
            state: wifi.get(),
            ssid: "",
            address: "",
            host_name: "",
            guests: 0,
            flags: 0,
        })])),
        12 => {
            let base = match records_decode(input).map_err(|_| -1)?.iter().next() {
                Some(Record::ServiceDevInterval(interval)) => interval.base_ms,
                _ => return Err(-1),
            };
            Ok(stream(&[Record::ServiceDevPower(ServiceDevPower {
                level: level.get(),
                mode: "",
                flags: 0,
                stretch: 1,
                every_ms: base,
            })]))
        }
        _ => Err(-1),
    });
    world
}

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

fn made(p: &Plug<'_>, widget: &str, instance: u32) -> Callback {
    event(
        p,
        NOW_TOKEN,
        Record::EventWidgetMade(EventWidgetMade {
            now_ms: 0,
            widget,
            instance,
            place: "",
        }),
    )
}

fn done(p: &Plug<'_>, id: u32, status: u32, reason: u32) -> Callback {
    event(
        p,
        HTTP_TOKEN,
        Record::EventHttpDone(EventHttpDone {
            now_ms: 0,
            id,
            token: 1,
            status,
            reason,
            flags: 0,
            body_len: if status == 200 {
                u32::try_from(FORECAST.len()).unwrap()
            } else {
                0
            },
        }),
    )
}

fn requests(world: &World) -> Vec<String> {
    world
        .log
        .borrow()
        .iter()
        .filter(|l| l.starts_with("request"))
        .cloned()
        .collect()
}

fn started(p: &Plug<'_>) -> Callback {
    event(
        p,
        LIFECYCLE_TOKEN,
        Record::EventStarted(EventStarted { now_ms: 0 }),
    )
}

fn action(p: &Plug<'_>, token: u32, name: &str) -> Callback {
    event(
        p,
        token,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: name,
        }),
    )
}

fn changed(p: &Plug<'_>, key: &str) -> Callback {
    event(
        p,
        LIFECYCLE_TOKEN,
        Record::EventSettingChanged(EventSettingChanged {
            now_ms: 0,
            key,
            old: &[],
        }),
    )
}

fn work_due(p: &Plug<'_>) -> Callback {
    event(
        p,
        WORK_TOKEN,
        Record::EventWorkDue(EventWorkDue {
            now_ms: 0,
            token: WORK_TOKEN,
            why: 3,
            key: "weather.refresh",
        }),
    )
}

fn works(world: &World) -> Vec<String> {
    world
        .log
        .borrow()
        .iter()
        .filter(|l| l.starts_with("work weather"))
        .cloned()
        .collect()
}

#[test]
fn a_placed_widget_makes_the_plugin_fetch_and_the_answer_is_shown() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // Nothing is placed yet: the start fetches nothing, but sets the timer.
    assert_eq!(started(&p).status, 0);
    assert!(requests(&world).is_empty());
    assert_eq!(
        works(&world),
        ["work weather.refresh token 5 needs 1 flags 1 moments 3 after 300000"],
        "network work, only when cheap, at the wake and when online"
    );
    let placed = made(&p, "weather.now", 4);
    assert_eq!(placed.status, 0);
    let all = requests(&world);
    assert_eq!(all.len(), 1, "{:?}", world.log.borrow());
    assert!(
        all[0].starts_with(
            "request GET https://api.open-meteo.com/v1/forecast?latitude=52.52&longitude=13.42&"
        ),
        "{}",
        all[0]
    );
    assert!(
        all[0].ends_with("PaperPaw/0.1 (e-reader) token 1 max 16384"),
        "{}",
        all[0]
    );
    // The widgets are told that it is being fetched, and (not asked for) nobody else.
    assert_eq!(placed.batches.len(), 1);
    let end = done(&p, 1, 200, 0);
    assert_eq!(end.status, 0);
    // Told again: the new weather shows.
    assert_eq!(end.batches.len(), 1);
    // Fresh: a look at the wake fetches nothing, but sets the timer again.
    assert_eq!(work_due(&p).status, 0);
    assert_eq!(requests(&world).len(), 1);
    assert_eq!(works(&world).len(), 3);
}

#[test]
fn what_is_due_by_the_clock_is_fetched_by_the_work() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    done(&p, 1, 200, 0);
    // An hour and a minute later (the interval is 1 h).
    snapshot(&host, 1_000 + 3_661_000, Some(1_800_000_000 + 3_661));
    assert_eq!(work_due(&p).status, 0);
    assert_eq!(requests(&world).len(), 2);
}

#[test]
fn a_failed_fetch_is_not_tried_again_for_a_quarter_of_an_hour() {
    let host = ScriptedHost::new();
    let world = world(&host, 500);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    let end = done(&p, 1, 500, 0);
    assert_eq!(end.status, 0);
    // Not asked for: no notice, the widgets are told.
    assert_eq!(end.batches.len(), 1);
    snapshot(&host, 1_000 + 600_000, Some(1_800_000_600));
    work_due(&p);
    assert_eq!(requests(&world).len(), 1, "ten minutes later: not yet");
    snapshot(&host, 1_000 + 1_000_000, Some(1_800_001_000));
    work_due(&p);
    assert_eq!(requests(&world).len(), 2, "sixteen minutes later: again");
}

#[test]
fn a_request_that_ends_without_an_answer_is_a_failure() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    // Reason 1: no network came.
    assert_eq!(done(&p, 1, 0, 1).status, 0);
    snapshot(&host, 1_000 + 60_000, Some(1_800_000_060));
    work_due(&p);
    assert_eq!(requests(&world).len(), 1);
}

#[test]
fn the_reader_asking_fetches_at_once_and_is_told() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    done(&p, 1, 200, 0);
    // Fresh, and still fetched when asked: the notice and the widgets.
    let asked = action(&p, REFRESH_TOKEN, "weather.refresh");
    assert_eq!(asked.status, 0);
    assert_eq!(requests(&world).len(), 2);
    assert_eq!(asked.batches.len(), 2, "the widgets and the notice");
    // Ending in a failure says so.
    let end = done(&p, 2, 500, 0);
    assert_eq!(end.batches.len(), 2, "the widgets and the notice");
}

#[test]
fn a_second_ask_while_one_is_under_way_waits_for_it() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    action(&p, REFRESH_TOKEN, "weather.refresh");
    assert_eq!(requests(&world).len(), 1, "one under way already");
    // Its end is told apart by the id: another one's is ignored.
    assert_eq!(done(&p, 9, 200, 0).batches.len(), 0);
}

#[test]
fn without_a_place_nothing_is_fetched_and_asking_opens_the_place_screen() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    host.set_setting("weather.coordinates", SettingValueRef::Text(""));
    host.set_setting("weather.place", SettingValueRef::Text(""));
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(made(&p, "weather.now", 4).status, 0);
    assert_eq!(started(&p).status, 0);
    assert!(requests(&world).is_empty());
    assert!(works(&world).is_empty(), "no timer without a place");
    // The scripted host has no screen stage: opening the place's screen is
    // refused with its status (the screen opens on the real host); what counts
    // is that nothing is fetched.
    let asked = action(&p, REFRESH_TOKEN, "weather.refresh");
    assert_eq!(asked.status, paperpaw_native::host_glue::ERR_UNSUPPORTED);
    assert!(requests(&world).is_empty());
}

#[test]
fn the_radio_is_not_switched_on_for_the_weather() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    world.wifi.set(1);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    assert!(requests(&world).is_empty(), "the radio is on, not joined");
    world.wifi.set(3);
    work_due(&p);
    assert_eq!(requests(&world).len(), 1, "once there is a network");
}

#[test]
fn in_the_offline_mode_nothing_is_fetched_by_itself_but_asking_is_allowed() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    world.level.set(2);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    work_due(&p);
    assert!(requests(&world).is_empty());
    action(&p, REFRESH_TOKEN, "weather.refresh");
    assert_eq!(requests(&world).len(), 1);
}

#[test]
fn a_new_place_replaces_the_fetch_under_way() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    host.set_setting("weather.coordinates", SettingValueRef::Text("48.14,11.58"));
    host.set_setting("weather.place", SettingValueRef::Text("Munich"));
    assert_eq!(changed(&p, "weather.coordinates").status, 0);
    let all = requests(&world);
    assert_eq!(all.len(), 2, "{:?}", world.log.borrow());
    assert!(all[1].contains("latitude=48.14&longitude=11.58"), "{}", all[1]);
    assert!(world.log.borrow().iter().any(|l| l == "cancel"));
    // The first one's end is ignored; the second one's counts.
    assert_eq!(done(&p, 1, 200, 0).batches.len(), 0);
    assert_eq!(done(&p, 2, 200, 0).batches.len(), 1);
}

#[test]
fn removing_the_place_stops_fetching() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    done(&p, 1, 200, 0);
    host.set_setting("weather.coordinates", SettingValueRef::Text(""));
    host.set_setting("weather.place", SettingValueRef::Text(""));
    let end = changed(&p, "weather.coordinates");
    assert_eq!(end.status, 0);
    assert_eq!(requests(&world).len(), 1, "nothing for no place");
    assert_eq!(end.batches.len(), 1, "the widgets are told");
}

#[test]
fn a_unit_changed_shows_the_widgets_anew_without_a_fetch() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "weather.now", 4);
    done(&p, 1, 200, 0);
    let end = changed(&p, "weather.temperature");
    assert_eq!(end.status, 0);
    assert_eq!(end.batches.len(), 1);
    assert_eq!(requests(&world).len(), 1);
    // Another plugin's setting is nothing to it.
    assert_eq!(changed(&p, "core.language").batches.len(), 0);
}

#[test]
fn the_forecast_widget_is_a_reason_to_look_too() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    event(
        &p,
        FORECAST_TOKEN,
        Record::EventWidgetMade(EventWidgetMade {
            now_ms: 0,
            widget: "weather.forecast",
            instance: 6,
            place: "",
        }),
    );
    assert_eq!(requests(&world).len(), 1);
}

#[test]
fn a_widget_of_another_plugin_is_no_reason() {
    let host = ScriptedHost::new();
    let world = world(&host, 200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    made(&p, "widgets.day", 7);
    assert!(requests(&world).is_empty());
}
