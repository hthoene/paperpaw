//! The setup with a phone through the production loader with a scripted host:
//! the own network is made from a strong random draw (a weak one makes none),
//! the radio is asked to scan and to host, the page is served when the own
//! network is open, a request for it is answered with the networks in reach
//! and the page's token, a form without the token chooses nothing, a path that
//! is none is 404, a device that serves a page already is told, and closing
//! the screen stops serving and leaves the own network. The scripted host
//! cannot stage Wi-Fi commands (`DEV-staging`): what the plugin sent is read
//! from the bytes of its batches, as in `scripted_radio.rs`.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::commands::commands_decode;
use paperpaw_core::native_host::environment::SettingValueRef;
use paperpaw_core::native_host::registrations::{
    EventScreenClosed, EventScreenOpened, EventScreenShown, EventServedRequest, EventServer,
    EventWifiChanged, EventWifiScanned, Record, ServiceDevDevice, ServiceDevNetwork,
    ServiceDevWifi, ServiceEnd, ServiceSecBytes, ServiceUiScreen, record_encode_one,
    records_decode,
};
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_plugin_wifi::NATIVE_ENTRY;
use paperpaw_scripted_host::{Plugin, ScriptedHost, event_batch};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const DEVICE_DEVICE: u32 = 1;
const DEVICE_WIFI: u32 = 8;
const DEVICE_NETWORKS: u32 = 9;
const SECRETS_RANDOM: u32 = 5;
const NET_SERVE: u32 = 5;
const NET_STOP: u32 = 6;
const NET_RESPOND: u32 = 7;
const WORK_DECLARE: u32 = 1;
const UI_STACK: u32 = 1;
const PHONE: u32 = 12;
const WIFI_SCANNED: u32 = 8;
const WIFI_CHANGED: u32 = 6;
const SERVER_EVENT: u32 = 16;
const SERVED_REQUEST: u32 = 17;
const OFF: u32 = 0;
const ON: u32 = 1;
const HOSTING: u32 = 4;
/// The command the scripted stage refuses when it is submitted.
const DEV_REFUSED: i32 = -1;

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

/// What the world is like and what the plugin did to it.
#[derive(Default)]
struct World {
    radio: Cell<u32>,
    /// Whether the draw of the own network is fully random.
    strong: Cell<bool>,
    /// The server is refused as busy.
    busy: Cell<bool>,
    /// Calls of `core.net`: "serve <token> <port>", "stop <token>".
    net: RefCell<Vec<String>>,
    /// Answers: (request, status, body).
    answers: RefCell<Vec<(u32, u32, String)>>,
    /// Work declared: (key, at_ms).
    works: RefCell<Vec<(String, u64)>>,
}

fn script(host: &ScriptedHost, world: &Rc<World>) {
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    for (key, value) in [("wifi.on-demand", true), ("wifi.while-charging", false)] {
        host.set_setting(key, SettingValueRef::Bool(value));
    }
    host.set_setting("wifi.idle-off", SettingValueRef::Text("wifi.idle-off.1m"));
    host.set_setting(
        "wifi.join-first",
        SettingValueRef::Text("wifi.join-first.picked"),
    );
    host.set_setting("core.language", SettingValueRef::Text("en"));
    script_ui(host);
    script_device(host, world);
    script_secrets(host, world);
    script_net(host, world);
    script_work(host, world);
}

/// The stack: the setup screen on top.
fn script_ui(host: &ScriptedHost) {
    host.service("core.ui", 1, 0, |op, _| match op {
        UI_STACK => Ok(stream(&[
            Record::ServiceUiScreen(ServiceUiScreen {
                screen: "wifi.phone",
                instance: 1,
                slot: "",
                owner: "wifi",
                flags: 0,
            }),
            last_page(1, 1),
        ])),
        _ => Ok(Vec::new()),
    });
}

/// The device: its facts, the radio and the networks in reach.
fn script_device(host: &ScriptedHost, world: &Rc<World>) {
    let w = world.clone();
    host.service("core.device", 1, 0, move |op, _| match op {
        DEVICE_DEVICE => Ok(stream(&[Record::ServiceDevDevice(ServiceDevDevice {
            id: "x4",
            model: "Reader",
            catalog: "x4",
            class: 0,
            caps: 0,
            controls: 0,
            tls_sessions: 0,
            start: 0,
            slept_ms: 0,
            flags: 0,
        })])),
        DEVICE_WIFI => Ok(stream(&[Record::ServiceDevWifi(ServiceDevWifi {
            state: w.radio.get(),
            ssid: "",
            address: "",
            host_name: "reader",
            guests: 0,
            flags: 0,
        })])),
        DEVICE_NETWORKS => Ok(stream(&[
            Record::ServiceDevNetwork(ServiceDevNetwork {
                ssid: "Home",
                signal: -50,
                flags: 1,
            }),
            Record::ServiceDevNetwork(ServiceDevNetwork {
                ssid: "Cafe",
                signal: -80,
                flags: 0,
            }),
            last_page(2, 1),
        ])),
        _ => Err(-1),
    });
}

/// The secrets: the random draw.
fn script_secrets(host: &ScriptedHost, world: &Rc<World>) {
    let w = world.clone();
    host.service("core.secrets", 1, 0, move |op, input| match op {
        SECRETS_RANDOM => {
            let asked = records_decode(input).unwrap().iter().find_map(|r| match r {
                Record::ServiceSecRandom(random) => Some(random.len as usize),
                _ => None,
            });
            let data: Vec<u8> = (0..asked.unwrap_or(0))
                .map(|i| u8::try_from((i * 37 + 11) % 256).unwrap())
                .collect();
            Ok(stream(&[Record::ServiceSecBytes(ServiceSecBytes {
                flags: u32::from(w.strong.get()),
                data: &data,
            })]))
        }
        _ => Err(-1),
    });
}

/// The network: what is served, stopped and answered.
fn script_net(host: &ScriptedHost, world: &Rc<World>) {
    let w = world.clone();
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (NET_SERVE, Some(Record::ServiceNetServe(serve))) => {
                if w.busy.get() {
                    return Err(ERR_BUSY);
                }
                w.net
                    .borrow_mut()
                    .push(format!("serve {} {}", serve.token, serve.port));
                Ok(Vec::new())
            }
            (NET_STOP, Some(Record::ServiceNetToken(token))) => {
                w.net.borrow_mut().push(format!("stop {}", token.token));
                Ok(Vec::new())
            }
            (NET_RESPOND, Some(Record::ServiceNetResponse(response))) => {
                let body = rows.iter().find_map(|r| match r {
                    Record::ServiceNetBody(b) => Some(String::from_utf8_lossy(b.data).to_string()),
                    _ => None,
                });
                w.answers.borrow_mut().push((
                    response.request,
                    response.status,
                    body.unwrap_or_default(),
                ));
                Ok(Vec::new())
            }
            _ => {
                w.net.borrow_mut().push(format!("other {op} {rows:?}"));
                Err(-1)
            }
        }
    });
}

/// The work declared, and the replies' bounds the shell keeps.
fn script_work(host: &ScriptedHost, world: &Rc<World>) {
    let w = world.clone();
    host.service("core.work", 1, 0, move |op, input| {
        if op == WORK_DECLARE {
            for record in records_decode(input).unwrap().iter() {
                if let Record::ServiceWorkSpec(spec) = record {
                    w.works
                        .borrow_mut()
                        .push((spec.key.to_string(), spec.at_ms));
                }
            }
        }
        Ok(Vec::new())
    });
    host.reply_bound("core.work", WORK_DECLARE, 64);
    // The shell refuses a smaller `out` than an op's reply bound
    // (`docs/abi/secrets.md` 1,280, `docs/abi/network.md` 1,536).
    host.reply_bound("core.secrets", SECRETS_RANDOM, 1_280);
    for op in [NET_SERVE, NET_STOP, NET_RESPOND] {
        host.reply_bound("core.net", op, 1_536);
    }
}

fn world(strong: bool) -> (ScriptedHost, Rc<World>) {
    let host = ScriptedHost::new();
    let world = Rc::new(World::default());
    world.radio.set(ON);
    world.strong.set(strong);
    script(&host, &world);
    (host, world)
}

fn opened() -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 1_000,
        screen: "wifi.phone",
        instance: 1,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

fn shown() -> Vec<u8> {
    event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 1_100,
        screen: "wifi.phone",
        width: 480,
        height: 800,
        instance: Some(1),
        last_input: 0,
    }))
    .unwrap()
}

fn closed() -> Vec<u8> {
    event_batch(&Record::EventScreenClosed(EventScreenClosed {
        now_ms: 90_000,
        screen: "wifi.phone",
        instance: 1,
    }))
    .unwrap()
}

fn scanned() -> Vec<u8> {
    event_batch(&Record::EventWifiScanned(EventWifiScanned {
        now_ms: 2_000,
        networks: 2,
    }))
    .unwrap()
}

fn changed(state: u32) -> Vec<u8> {
    event_batch(&Record::EventWifiChanged(EventWifiChanged {
        now_ms: 3_000,
        state,
    }))
    .unwrap()
}

fn server(state: u32, address: &str) -> Vec<u8> {
    event_batch(&Record::EventServer(EventServer {
        now_ms: 3_100,
        server: 1,
        state,
        reason: 0,
        address,
    }))
    .unwrap()
}

fn asked(request: u32, method: &str, target: &str) -> Vec<u8> {
    event_batch(&Record::EventServedRequest(EventServedRequest {
        now_ms: 4_000,
        server: 1,
        request,
        method,
        target,
        flags: 0,
        body_len: 0,
    }))
    .unwrap()
}

/// What delivering an event submitted, as text, and the callback's status.
fn send(plugin: &Plugin<'_>, token: u32, batch: &[u8]) -> (i32, Vec<String>) {
    let done = plugin.event(token, batch).unwrap();
    let mut sent = Vec::new();
    for bytes in &done.batches {
        for command in commands_decode(bytes).unwrap().iter() {
            sent.push(format!("{command:?}"));
        }
    }
    (done.status, sent)
}

/// A callback that only redraws: the scripted stage refuses what it is sent
/// (`DEV-staging`), so a redraw reads as `DEV_REFUSED`, and none as success.
fn only_redraws(status: i32, sent: &[String]) {
    assert!(sent.iter().all(|c| c.starts_with("Redraw")), "{sent:?}");
    let want = if sent.is_empty() { 0 } else { DEV_REFUSED };
    assert_eq!(status, want, "{sent:?}");
}

/// The setup up to its own network open and its page served at `address`.
fn serving<'h>(host: &'h ScriptedHost, world: &World) -> Plugin<'h> {
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let (status, _) = send(&plugin, PHONE, &opened());
    assert_eq!(status, 0);
    // The screen is shown: the radio is on, networks are looked for.
    let (status, sent) = send(&plugin, PHONE, &shown());
    assert_eq!(status, DEV_REFUSED, "{sent:?}");
    assert!(sent.iter().any(|c| c.contains("Scan")), "{sent:?}");
    // Found: the own network is asked for (its password is not in the text).
    let (status, sent) = send(&plugin, WIFI_SCANNED, &scanned());
    assert_eq!(status, DEV_REFUSED, "{sent:?}");
    assert!(sent.iter().any(|c| c.contains("Host")), "{sent:?}");
    // The own network is open: the page is served at port 80 and the idle
    // time is set as work.
    world.radio.set(HOSTING);
    let (status, sent) = send(&plugin, WIFI_CHANGED, &changed(HOSTING));
    only_redraws(status, &sent);
    assert_eq!(*world.net.borrow(), ["serve 1 80"]);
    assert!(
        world
            .works
            .borrow()
            .contains(&("wifi.phone-wake".to_string(), 3_000 + 600_000)),
        "{:?}",
        world.works.borrow()
    );
    let (status, sent) = send(&plugin, SERVER_EVENT, &server(0, "192.168.4.1:80"));
    only_redraws(status, &sent);
    plugin
}

#[test]
fn the_own_network_is_asked_for_and_the_page_served_when_it_is_open() {
    let (host, world) = world(true);
    let _plugin = serving(&host, &world);
}

#[test]
fn the_page_lists_the_networks_and_carries_the_token_of_the_screen() {
    let (host, world) = world(true);
    let plugin = serving(&host, &world);
    let (status, sent) = send(&plugin, SERVED_REQUEST, &asked(7, "GET", "/"));
    only_redraws(status, &sent);
    let answers = world.answers.borrow();
    let (request, status, body) = answers.last().unwrap();
    assert_eq!((*request, *status), (7, 200));
    assert!(
        body.contains("<form method=\"post\" action=\"/join\">"),
        "{body}"
    );
    assert!(body.contains("value=\"Home\"") && body.contains("value=\"Cafe\""));
    // The token: drawn, not empty, and never the password of the own network.
    let token = body
        .split("name=\"t\" value=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    assert!(token.len() >= 6, "{token:?}");
}

#[test]
fn a_form_without_the_token_chooses_nothing_and_a_path_that_is_none_is_404() {
    let (host, world) = world(true);
    let plugin = serving(&host, &world);
    // The form has no file in the scripted host's storage: it reads as empty,
    // so it has no token, and joining is refused with the page again.
    let (status, sent) = send(&plugin, SERVED_REQUEST, &asked(8, "POST", "/join"));
    assert_eq!(status, 0, "{sent:?}");
    assert!(sent.is_empty(), "no join was started: {sent:?}");
    let (status, sent) = send(&plugin, SERVED_REQUEST, &asked(9, "GET", "/elsewhere"));
    only_redraws(status, &sent);
    let answers = world.answers.borrow();
    assert_eq!(answers[answers.len() - 2].1, 403);
    assert_eq!(answers[answers.len() - 1].1, 404);
}

#[test]
fn a_device_that_serves_a_page_already_is_told_and_nothing_is_lost() {
    let (host, world) = world(true);
    world.busy.set(true);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    send(&plugin, PHONE, &opened());
    send(&plugin, PHONE, &shown());
    send(&plugin, WIFI_SCANNED, &scanned());
    world.radio.set(HOSTING);
    let (status, sent) = send(&plugin, WIFI_CHANGED, &changed(HOSTING));
    // The refusal is a state of the screen, not a failure of the callback:
    // it only asks for a redraw.
    only_redraws(status, &sent);
    assert!(world.net.borrow().is_empty());
}

#[test]
fn a_weak_draw_makes_no_own_network_and_nothing_is_asked_of_the_radio() {
    let (host, world) = world(false);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let (status, _) = send(&plugin, PHONE, &opened());
    assert_eq!(status, 0);
    let (status, sent) = send(&plugin, PHONE, &shown());
    only_redraws(status, &sent);
    assert!(world.net.borrow().is_empty());
}

#[test]
fn closing_the_screen_stops_the_page_and_leaves_the_own_network() {
    let (host, world) = world(true);
    let plugin = serving(&host, &world);
    let (status, sent) = send(&plugin, PHONE, &closed());
    assert_eq!(status, DEV_REFUSED, "{sent:?}");
    assert!(sent.iter().any(|c| c.contains("Leave")), "{sent:?}");
    assert!(world.net.borrow().contains(&"stop 1".to_string()));
}

#[test]
fn the_radio_off_after_the_screen_opened_asks_it_on() {
    let (host, world) = world(true);
    world.radio.set(OFF);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    send(&plugin, PHONE, &opened());
    let (status, sent) = send(&plugin, PHONE, &shown());
    assert_eq!(status, DEV_REFUSED, "{sent:?}");
    assert!(sent[0].contains("On"), "{sent:?}");
}
