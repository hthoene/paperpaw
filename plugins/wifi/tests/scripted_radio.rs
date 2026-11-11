//! The radio's events through the production loader with a scripted host: a
//! plugin wants the network (the radio comes on), the network goes idle (the
//! idle timer is declared as work at the idle time, and when it comes due the
//! radio goes off), a screen at rest (off at once), and a join that failed
//! (the policy hears it). The facts are explicit: the radio's state, the
//! networks joined before and their passwords, the clock of the events.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::commands::commands_decode;
use paperpaw_core::native_host::environment::SettingValueRef;
use paperpaw_core::native_host::registrations::{
    EventNetworkIdle, EventNetworkWanted, EventScreenClosed, EventWifiDone, EventWifiScanned,
    EventWorkDue, Record, ServiceDevNetwork, ServiceDevWifi, ServiceEnd, ServiceSecEntry,
    ServiceSecValue, ServiceUiScreen, record_encode_one, records_decode,
};
use paperpaw_plugin_wifi::NATIVE_ENTRY;
use paperpaw_scripted_host::{Plugin, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the tests read the tokens only")]
mod declared;

const DEVICE_WIFI: u32 = 8;
const DEVICE_NETWORKS: u32 = 9;
const SECRETS_GET: u32 = 1;
const SECRETS_LIST: u32 = 2;
const UI_STACK: u32 = 1;
const WORK_DECLARE: u32 = 1;
/// `dev_wifi` states.
const OFF: u32 = 0;
const ON: u32 = 1;
/// `dev_wifi.flags`: scanning, wanted, busy.
const WANTED: u32 = 2;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

fn last_page(rows: usize, generation: u32) -> Record<'static> {
    let rows = u32::try_from(rows).unwrap();
    Record::ServiceEnd(ServiceEnd {
        flags: 1,
        total: rows,
        generation,
        next: rows,
        cursor: None,
    })
}

/// What the work service was asked to declare: (key, at_ms).
type Works = Rc<RefCell<Vec<(String, u64)>>>;

/// What the world looks like: the radio, its flags, the networks in reach and
/// the networks joined before (name, password), the screen on top.
struct World {
    radio: u32,
    flags: u32,
    reach: Vec<(&'static str, bool)>,
    kept: Vec<(&'static str, &'static str)>,
    top: &'static str,
}

impl World {
    fn new(radio: u32) -> Self {
        Self {
            radio,
            flags: 0,
            reach: Vec::new(),
            kept: vec![("Home", "secret")],
            top: "",
        }
    }
}

fn host(world: World, works: &Works) -> ScriptedHost {
    let host = ScriptedHost::new();
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    host.set_setting("wifi.on-demand", SettingValueRef::Bool(true));
    host.set_setting("wifi.while-charging", SettingValueRef::Bool(false));
    host.set_setting("wifi.idle-off", SettingValueRef::Text("wifi.idle-off.1m"));
    host.set_setting(
        "wifi.join-first",
        SettingValueRef::Text("wifi.join-first.picked"),
    );
    let top = world.top;
    host.service("core.ui", 1, 0, move |op, _| match op {
        UI_STACK => Ok(stream(&[
            Record::ServiceUiScreen(ServiceUiScreen {
                screen: "home",
                instance: 1,
                slot: top,
                owner: "home",
                flags: 0,
            }),
            last_page(1, 1),
        ])),
        _ => Ok(Vec::new()),
    });
    let (radio, flags) = (world.radio, world.flags);
    let reach = world.reach.clone();
    host.service("core.device", 1, 0, move |op, _| match op {
        DEVICE_WIFI => Ok(stream(&[Record::ServiceDevWifi(ServiceDevWifi {
            state: radio,
            ssid: "",
            address: "",
            host_name: "reader",
            guests: 0,
            flags,
        })])),
        DEVICE_NETWORKS => {
            let mut records: Vec<Record<'_>> = reach
                .iter()
                .map(|(ssid, secured)| {
                    Record::ServiceDevNetwork(ServiceDevNetwork {
                        ssid,
                        signal: -50,
                        flags: u32::from(*secured),
                    })
                })
                .collect();
            records.push(last_page(reach.len(), 1));
            Ok(stream(&records))
        }
        _ => Err(-1),
    });
    let kept = world.kept.clone();
    host.service("core.secrets", 1, 0, move |op, input| match op {
        SECRETS_LIST => {
            let mut records: Vec<Record<'_>> = kept
                .iter()
                .map(|(name, _)| {
                    Record::ServiceSecEntry(ServiceSecEntry {
                        flags: 0,
                        plugin: "wifi",
                        name,
                        label: "Wi-Fi",
                    })
                })
                .collect();
            records.push(last_page(kept.len(), 0));
            Ok(stream(&records))
        }
        SECRETS_GET => {
            let wanted = kept
                .iter()
                .find(|(name, _)| input.windows(name.len()).any(|w| w == name.as_bytes()))
                .map(|(_, password)| *password);
            match wanted {
                Some(password) => Ok(stream(&[Record::ServiceSecValue(ServiceSecValue {
                    flags: 0,
                    label: "Wi-Fi",
                    value: password,
                })])),
                None => Err(-4),
            }
        }
        _ => Err(-1),
    });
    let seen = works.clone();
    host.service("core.work", 1, 0, move |op, input| {
        if op == WORK_DECLARE {
            for record in records_decode(input).unwrap().iter() {
                if let Record::ServiceWorkSpec(spec) = record {
                    seen.borrow_mut().push((spec.key.to_string(), spec.at_ms));
                }
            }
        }
        Ok(Vec::new())
    });
    // `declare` replies with nothing: at most 64 bytes (`work.md` 1).
    host.reply_bound("core.work", WORK_DECLARE, 64);
    host
}

/// The scripted host's stage has no `core.device` context, so a Wi-Fi command
/// is refused when it is submitted and its callback ends with this status
/// (BLOCKED scripted-host DEV staging); what the plugin submitted is read
/// from the bytes of its batches.
const DEV_REFUSED: i32 = -1;

fn wanted(now_ms: u64) -> Vec<u8> {
    event_batch(&Record::EventNetworkWanted(EventNetworkWanted { now_ms })).unwrap()
}

fn idle(now_ms: u64) -> Vec<u8> {
    event_batch(&Record::EventNetworkIdle(EventNetworkIdle { now_ms })).unwrap()
}

fn due(now_ms: u64) -> Vec<u8> {
    event_batch(&Record::EventWorkDue(EventWorkDue {
        now_ms,
        token: 1,
        why: 0,
        key: "wifi.idle-off",
    }))
    .unwrap()
}

/// One of the plugin's own screens left (`screen_shown` of the others is not
/// subscribed to: the host sends 38 only to a declared screen's token).
fn closed(screen: &str, now_ms: u64) -> Vec<u8> {
    event_batch(&Record::EventScreenClosed(EventScreenClosed {
        now_ms,
        screen,
        instance: 1,
    }))
    .unwrap()
}

fn done_event(status: i32, reason: u32, state: u32, now_ms: u64) -> Vec<u8> {
    event_batch(&Record::EventWifiDone(EventWifiDone {
        now_ms,
        request: 1,
        ssid: "Home",
        status,
        reason,
        state,
    }))
    .unwrap()
}

/// What delivering an event did: the commands it submitted, as text (a
/// password in one shows as nothing).
struct Outcome {
    sent: Vec<String>,
}

fn send(plugin: &Plugin<'_>, token: u32, batch: &[u8]) -> Outcome {
    let done = plugin.event(token, batch).unwrap();
    let mut sent = Vec::new();
    for bytes in &done.batches {
        for command in commands_decode(bytes).unwrap().iter() {
            sent.push(format!("{command:?}"));
        }
    }
    // A Wi-Fi command or a notice is refused by the scripted stage (it has no
    // device or screen context), a redraw is not.
    let refused = sent.iter().any(|c| !c.starts_with("Redraw"));
    assert_eq!(
        done.status,
        if refused { DEV_REFUSED } else { 0 },
        "{sent:?}"
    );
    Outcome { sent }
}

fn only(outcome: &Outcome, command: &str) {
    assert_eq!(outcome.sent.len(), 1, "{:?}", outcome.sent);
    assert!(outcome.sent[0].starts_with(command), "{:?}", outcome.sent);
}

#[test]
fn a_plugin_wanting_the_network_brings_the_radio_on() {
    let host = host(World::new(OFF), &Works::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    only(
        &send(&plugin, declared::NETWORK_WANTED, &wanted(5_000)),
        "WifiOn",
    );
}

#[test]
fn a_network_wanted_with_none_known_tells_the_host_none_is_coming() {
    let mut world = World::new(OFF);
    world.kept = Vec::new();
    let host = host(world, &Works::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    only(
        &send(&plugin, declared::NETWORK_WANTED, &wanted(5_000)),
        "NetworkNone",
    );
}

#[test]
fn nothing_uses_the_network_the_idle_timer_is_set_and_when_due_the_radio_goes_off() {
    let works = Works::default();
    let host = host(World::new(ON), &works);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let outcome = send(&plugin, declared::NETWORK_IDLE, &idle(5_000));
    assert!(
        outcome.sent.iter().all(|c| c.starts_with("Redraw")),
        "{:?}",
        outcome.sent
    );
    // The default idle time is a minute: due at the event's time plus that.
    assert_eq!(
        works.borrow().as_slice(),
        [("wifi.idle-off".to_string(), 65_000)]
    );
    // It comes due: nothing waits, the radio goes off.
    only(&send(&plugin, declared::WORK, &due(65_000)), "WifiOff");
}

#[test]
fn a_timer_set_before_the_network_was_wanted_again_does_nothing() {
    let host = host(World::new(ON), &Works::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    send(&plugin, declared::NETWORK_IDLE, &idle(1_000));
    // Wanted again before it is due: the policy moves on a generation.
    send(&plugin, declared::NETWORK_WANTED, &wanted(30_000));
    let outcome = send(&plugin, declared::WORK, &due(61_000));
    assert!(
        outcome.sent.iter().all(|c| c.starts_with("Redraw")),
        "{:?}",
        outcome.sent
    );
}

#[test]
fn a_timer_for_work_that_still_waits_keeps_the_radio_on() {
    let mut world = World::new(ON);
    world.flags = WANTED;
    let host = host(world, &Works::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    send(&plugin, declared::NETWORK_WANTED, &wanted(1_000));
    send(&plugin, declared::NETWORK_IDLE, &idle(2_000));
    // Work still waits and the network was asked for just now: not off.
    let outcome = send(&plugin, declared::WORK, &due(10_000));
    assert!(
        outcome.sent.iter().all(|c| c.starts_with("Redraw")),
        "{:?}",
        outcome.sent
    );
}

#[test]
fn back_home_with_nothing_wanted_the_radio_goes_off_at_once() {
    let mut world = World::new(ON);
    world.top = "core.home";
    let host = host(world, &Works::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    only(
        &send(&plugin, declared::PAGE, &closed("wifi.page", 1_000)),
        "WifiOff",
    );
}

#[test]
fn a_screen_that_is_not_at_rest_leaves_the_radio_on() {
    let host = host(World::new(ON), &Works::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let outcome = send(&plugin, declared::PAGE, &closed("wifi.networks", 1_000));
    assert!(
        outcome.sent.iter().all(|c| c.starts_with("Redraw")),
        "{:?}",
        outcome.sent
    );
}

#[test]
fn networks_found_while_one_is_wanted_join_the_one_joined_before() {
    let mut world = World::new(ON);
    world.flags = WANTED;
    world.reach = vec![("Cafe", false), ("Home", true)];
    let host = host(world, &Works::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    // The network is wanted first: the policy takes the goal.
    send(&plugin, declared::NETWORK_WANTED, &wanted(5_000));
    let scanned = event_batch(&Record::EventWifiScanned(EventWifiScanned {
        now_ms: 6_000,
        networks: 2,
    }))
    .unwrap();
    let outcome = send(&plugin, declared::WIFI_SCANNED, &scanned);
    // One join: of the network joined before, not of the open one in reach;
    // the password shows nowhere.
    only(&outcome, "WifiJoin");
    assert!(outcome.sent[0].contains("Home"), "{:?}", outcome.sent);
    assert!(!outcome.sent[0].contains("secret"), "{:?}", outcome.sent);
}

#[test]
fn a_join_that_failed_is_told_and_a_success_decides_nothing() {
    let mut world = World::new(ON);
    world.flags = WANTED;
    world.reach = vec![("Home", true)];
    let host = host(world, &Works::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    send(&plugin, declared::NETWORK_WANTED, &wanted(5_000));
    // Success: the radio's change says it, nothing to decide here.
    let outcome = send(&plugin, declared::WIFI_DONE, &done_event(0, 0, 3, 7_000));
    assert!(
        outcome.sent.iter().all(|c| c.starts_with("Redraw")),
        "{:?}",
        outcome.sent
    );
    // A wrong password on the only network joined before: told, as the policy
    // says (nothing else to try); the notice is a screen command.
    let outcome = send(&plugin, declared::WIFI_DONE, &done_event(-5, 2, 0, 8_000));
    let said = outcome.sent.join(" ");
    assert!(said.contains("Wrong password for Home"), "{said}");
}
