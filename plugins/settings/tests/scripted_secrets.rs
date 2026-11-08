//! Passwords and keys over a scripted `core.secrets`, with the plugin made and
//! driven by the scripted host: what the list asks the service (all plugins'
//! secrets, page after page), that the plugin is not harmed by a service it
//! may not use, and that no refusal of the host aborts it.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, Record, ServiceEnd, ServiceSecEntry, record_encode_one,
};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_UNSUPPORTED};
use paperpaw_plugin_settings::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use std::cell::RefCell;
use std::rc::Rc;

const SECRETS_LIST: u32 = 2;
const SECRETS_TOKEN: u32 = 15;
const PAGE: usize = 2;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// (owner, name, label, changeable)
fn kept() -> Vec<(&'static str, &'static str, &'static str, bool)> {
    vec![
        ("mail", "password", "mail.secret.password", true),
        ("kosync", "key", "Sync key", false),
        ("wifi", "Home", "Wi-Fi Home", true),
        ("lock", "code", "", false),
        ("wifi", "Cafe", "Wi-Fi Cafe", true),
    ]
}

/// What the service was asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

fn script(host: &ScriptedHost, log: &Log, answer: Option<i32>) {
    host.service("core.ui", 1, 0, |_, _| Ok(Vec::new()));
    // Opened with the registry reader for every screen; this one reads none of it.
    host.service("core.registry", 1, 0, |_, _| Ok(Vec::new()));
    let seen = log.clone();
    host.service("core.secrets", 1, 0, move |op, input| {
        let batch = paperpaw_core::native_host::registrations::records_decode(input).unwrap();
        let rows: Vec<Record<'_>> = batch.iter().collect();
        if let Some(status) = answer {
            return Err(status);
        }
        match (op, rows.first()) {
            (SECRETS_LIST, Some(Record::ServiceSecList(query))) => {
                seen.borrow_mut()
                    .push(format!("list {} {}", query.flags, query.cursor));
                let all = kept();
                let from = query.cursor as usize;
                let to = (from + PAGE).min(all.len());
                let mut out: Vec<Record<'_>> = all[from..to]
                    .iter()
                    .map(|(plugin, name, label, changeable)| {
                        Record::ServiceSecEntry(ServiceSecEntry {
                            flags: u32::from(*changeable),
                            plugin,
                            name,
                            label,
                        })
                    })
                    .collect();
                let last = to == all.len();
                out.push(Record::ServiceEnd(ServiceEnd {
                    flags: u32::from(last),
                    total: u32::try_from(all.len()).unwrap(),
                    generation: 0,
                    next: u32::try_from(to).unwrap(),
                    cursor: None,
                }));
                Ok(stream(&out))
            }
            _ => Err(-1),
        }
    });
}

fn opened() -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "settings.secrets",
        instance: 1,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

/// The scripted host does not answer `i18n.copy` and `i18n.format`, nor the
/// registry the list of the settings (`ERR_UNSUPPORTED`): the rows end there,
/// after the secrets have been read.
fn finished(status: i32) -> bool {
    status == 0 || status == ERR_UNSUPPORTED
}

fn run(host: &ScriptedHost) -> i32 {
    // A refusal while the plugin is made is the factory's status.
    let plugin = match host.load(NATIVE_ENTRY.entry()) {
        Ok(plugin) => plugin,
        Err(status) => return status,
    };
    plugin
        .event(SECRETS_TOKEN, &opened())
        .map_or_else(|status| status, |done| done.status)
}

#[test]
fn the_list_asks_for_every_plugins_secrets_a_page_at_a_time() {
    let host = ScriptedHost::new();
    let log: Log = Log::default();
    script(&host, &log, None);
    let status = run(&host);
    assert!(finished(status), "{status} {:?}", log.borrow());
    // Flag 1 is every plugin's; the cursor goes on from the rows read.
    assert_eq!(*log.borrow(), ["list 1 0", "list 1 2", "list 1 4"]);
}

#[test]
fn a_service_that_will_not_list_leaves_the_list_empty_not_failed() {
    for status in [ERR_DENIED, ERR_UNSUPPORTED] {
        let host = ScriptedHost::new();
        script(&host, &Log::default(), Some(status));
        let result = run(&host);
        assert!(finished(result), "{status}: {result}");
    }
}

#[test]
fn no_refusal_of_the_host_makes_it_abort() {
    for slot in [Slot::ServiceCall, Slot::ServiceOpen] {
        let results = sweep(slot, -5, |host, _| {
            script(host, &Log::default(), None);
            run(host)
        });
        assert!(finished(results[0].1));
        assert!(results.len() >= 2, "{}", results.len());
        // A refused call is an error status (nothing staged), never a hang.
        assert!(results.iter().skip(1).all(|(_, status)| *status <= 0));
    }
}
