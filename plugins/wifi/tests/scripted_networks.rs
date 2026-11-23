//! The page of the networks over a scripted `core.device` and
//! `core.secrets`, with the plugin made and driven by the scripted host: what
//! it asks the device (the radio, then the networks in reach) and the secrets
//! (the names kept), that no refusal of the host or of memory makes it
//! abort, that a network kept is forgotten with the buttons alone, and that
//! the radio going off leaves the page open saying so (an event's callback
//! cannot leave a screen).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::commands::commands_decode;
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, EventScreenShown, InputAction, Record, ServiceDevNetwork, ServiceDevWifi,
    ServiceEnd, ServiceSecEntry, record_encode_one,
};
use paperpaw_plugin_wifi::NATIVE_ENTRY;
use paperpaw_scripted_host::{Callback, Plugin, ScriptedHost, Slot, event_batch, sweep};
use std::cell::RefCell;
use std::rc::Rc;

const DEVICE_WIFI: u32 = 8;
const DEVICE_NETWORKS: u32 = 9;
const SECRETS_GET: u32 = 1;
const SECRETS_LIST: u32 = 2;
const SECRETS_KEEP: u32 = 3;
const SECRETS_REMOVE: u32 = 4;
const NETWORKS_TOKEN: u32 = 11;
/// The radio is off, or on and not scanning.
const OFF: u32 = 0;
const ON: u32 = 1;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// The end of the last page of a paged list: the device's lists carry a
/// generation, the secrets' none.
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

/// What the services were asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

fn script(host: &ScriptedHost, log: &Log) {
    script_with(host, log, ON, KEPT);
}

/// The networks kept: Home.
const KEPT: &[&str] = &["Home"];

/// The services, the radio in `state` and the networks `kept`.
fn script_with(host: &ScriptedHost, log: &Log, state: u32, kept: &'static [&'static str]) {
    // The plugin's own texts, as the kernel reads them: the rows are drawn.
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    host.service("core.ui", 1, 0, |_, _| Ok(Vec::new()));
    let seen = log.clone();
    host.service("core.device", 1, 0, move |op, _| {
        seen.borrow_mut().push(format!("device {op}"));
        match op {
            DEVICE_WIFI => Ok(stream(&[Record::ServiceDevWifi(ServiceDevWifi {
                state,
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
        }
    });
    let seen = log.clone();
    host.service("core.secrets", 1, 0, move |op, _| {
        seen.borrow_mut().push(format!("secrets {op}"));
        match op {
            SECRETS_LIST => {
                let mut records: Vec<Record<'_>> = kept
                    .iter()
                    .map(|&name| {
                        Record::ServiceSecEntry(ServiceSecEntry {
                            flags: 0,
                            plugin: "wifi",
                            name,
                            label: name,
                        })
                    })
                    .collect();
                records.push(last_page(kept.len(), 0));
                Ok(stream(&records))
            }
            SECRETS_REMOVE => Ok(Vec::new()),
            _ => Err(-1),
        }
    });
    // The shell refuses a smaller `out` than an op's reply bound
    // (`docs/abi/secrets.md`: `get`, `keep` and `remove` 1,280, `list` 4,096).
    for op in [SECRETS_GET, SECRETS_KEEP, SECRETS_REMOVE] {
        host.reply_bound("core.secrets", op, 1_280);
    }
}

fn opened() -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "wifi.networks",
        instance: 1,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

/// The rows are drawn from the plugin's catalogs (the scripted host answers
/// `i18n.copy` and `i18n.format`): the callback ends well.
fn finished(status: i32) -> bool {
    status == 0
}

fn run(host: &ScriptedHost) -> i32 {
    // A refusal while the plugin is made is the factory's status.
    let plugin = match host.load(NATIVE_ENTRY.entry()) {
        Ok(plugin) => plugin,
        Err(status) => return status,
    };
    plugin
        .event(NETWORKS_TOKEN, &opened())
        .map_or_else(|status| status, |done| done.status)
}

#[test]
fn the_networks_page_asks_the_radio_then_the_networks_then_the_names_kept() {
    let host = ScriptedHost::new();
    let log: Log = Log::default();
    script(&host, &log);
    let status = run(&host);
    assert!(finished(status), "{status} {:?}", log.borrow());
    let log = log.borrow();
    // The radio first (twice: the page and the rows read it), the networks
    // of one page, the secrets list for the names kept.
    assert_eq!(log.first().map(String::as_str), Some("device 8"), "{log:?}");
    assert!(log.contains(&"device 9".to_string()), "{log:?}");
    assert!(log.contains(&"secrets 2".to_string()), "{log:?}");
}

#[test]
fn no_refusal_of_the_host_makes_it_abort() {
    for slot in [Slot::ServiceCall, Slot::ServiceOpen] {
        let results = sweep(slot, -5, |host, _| {
            script(host, &Log::default());
            run(host)
        });
        assert!(finished(results[0].1));
        assert!(results.len() >= 2, "{}", results.len());
        // A refused call is an error status (nothing staged), never a hang.
        assert!(results.iter().skip(1).all(|(_, status)| *status <= 0));
    }
}

fn shown() -> Vec<u8> {
    event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen: "wifi.networks",
        width: 480,
        height: 800,
        instance: Some(1),
        last_input: 0,
    }))
    .unwrap()
}

fn action(name: &str) -> Vec<u8> {
    event_batch(&Record::InputAction(InputAction {
        now_ms: 0,
        action: name,
    }))
    .unwrap()
}

/// The commands a callback submitted, as text.
fn sent(done: &Callback) -> Vec<String> {
    let mut sent = Vec::new();
    for bytes in &done.batches {
        for command in commands_decode(bytes).unwrap().iter() {
            sent.push(format!("{command:?}"));
        }
    }
    sent
}

/// The networks page opened and shown.
fn showing(host: &ScriptedHost) -> Plugin<'_> {
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert!(finished(
        plugin.event(NETWORKS_TOKEN, &opened()).unwrap().status
    ));
    plugin.event(NETWORKS_TOKEN, &shown()).unwrap();
    plugin
}

#[test]
fn select_held_or_the_menu_button_forgets_the_network_kept_that_has_the_focus() {
    for button in ["core.long-confirm", "core.menu"] {
        let host = ScriptedHost::new();
        let log: Log = Log::default();
        script(&host, &log);
        let plugin = showing(&host);
        // The focus is on Home, which is kept. Before, only a long press of
        // a touch screen forgot it: the list takes neither button.
        log.borrow_mut().clear();
        plugin.input(NETWORKS_TOKEN, &action(button)).unwrap();
        assert!(
            log.borrow().contains(&format!("secrets {SECRETS_REMOVE}")),
            "{button}: {:?}",
            log.borrow()
        );
    }
}

#[test]
fn the_key_bar_names_forget_on_a_network_kept_only() {
    // The focus on Home, kept: the menu button says Forget, as in Beta 3.
    let host = ScriptedHost::new();
    script(&host, &Log::default());
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    plugin.event(NETWORKS_TOKEN, &opened()).unwrap();
    let done = plugin.event(NETWORKS_TOKEN, &shown()).unwrap();
    let hints: Vec<String> = sent(&done)
        .into_iter()
        .filter(|c| c.starts_with("SetHint"))
        .collect();
    assert!(
        hints.len() == 1 && hints[0].contains("core.menu") && hints[0].contains("Forget"),
        "{hints:?}"
    );
    // Home not kept: no Forget, and the menu button is left to the host.
    let host = ScriptedHost::new();
    let log: Log = Log::default();
    script_with(&host, &log, ON, &[]);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    plugin.event(NETWORKS_TOKEN, &opened()).unwrap();
    let done = plugin.event(NETWORKS_TOKEN, &shown()).unwrap();
    assert!(
        sent(&done).iter().all(|c| !c.contains("Forget")),
        "{:?}",
        sent(&done)
    );
    log.borrow_mut().clear();
    let done = plugin.input(NETWORKS_TOKEN, &action("core.menu")).unwrap();
    assert_eq!(done.status, 0, "left to the host");
    assert!(!log.borrow().contains(&format!("secrets {SECRETS_REMOVE}")));
}

#[test]
fn the_page_stays_open_and_says_so_while_the_radio_is_off() {
    let host = ScriptedHost::new();
    script_with(&host, &Log::default(), OFF, KEPT);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert!(finished(
        plugin.event(NETWORKS_TOKEN, &opened()).unwrap().status
    ));
    // Before, the page went back from the event, which the host refuses
    // there (only a key's callback leaves a screen): the callback failed and
    // the commands it staged were lost with it.
    let done = plugin.event(NETWORKS_TOKEN, &shown()).unwrap();
    let sent = sent(&done);
    assert!(sent.iter().all(|c| !c.starts_with("Close")), "{sent:?}");
    assert!(sent.iter().any(|c| c.starts_with("Redraw")), "{sent:?}");
}
