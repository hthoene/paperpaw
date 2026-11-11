//! The page of the networks over a scripted `core.device` and
//! `core.secrets`, with the plugin made and driven by the scripted host: what
//! it asks the device (the radio, then the networks in reach) and the secrets
//! (the names kept), and that no refusal of the host or of memory makes it
//! abort.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, Record, ServiceDevNetwork, ServiceDevWifi, ServiceEnd, ServiceSecEntry,
    record_encode_one,
};
use paperpaw_plugin_wifi::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use std::cell::RefCell;
use std::rc::Rc;

const DEVICE_WIFI: u32 = 8;
const DEVICE_NETWORKS: u32 = 9;
const SECRETS_LIST: u32 = 2;
const NETWORKS_TOKEN: u32 = 11;
/// The radio is on, not scanning.
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
    // The plugin's own texts, as the kernel reads them: the rows are drawn.
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    host.service("core.ui", 1, 0, |_, _| Ok(Vec::new()));
    let seen = log.clone();
    host.service("core.device", 1, 0, move |op, _| {
        seen.borrow_mut().push(format!("device {op}"));
        match op {
            DEVICE_WIFI => Ok(stream(&[Record::ServiceDevWifi(ServiceDevWifi {
                state: ON,
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
            SECRETS_LIST => Ok(stream(&[
                Record::ServiceSecEntry(ServiceSecEntry {
                    flags: 0,
                    plugin: "wifi",
                    name: "Home",
                    label: "Wi-Fi Home",
                }),
                last_page(1, 0),
            ])),
            _ => Err(-1),
        }
    });
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
