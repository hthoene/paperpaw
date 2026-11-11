#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The pending asks through the service: kept for an asker that subscribes to
//! `event.lookup_ready`, ended by the ready command, the deadline or a stop.
extern crate std;
use super::*;
use crate::abi7::services_wire;
use alloc::{string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_platform::Instant;
use paperpaw_sdk::Settings;
use wire::{ServiceLookupAsk as Ask, ServiceLookupEntry as Entry};

const WIKI: u64 = 500;
const TRANSLATE: u64 = 501;

/// Two network providers, "wiki" and "translate", that answer `pending`
/// until told otherwise; a third, "dict", answers at once.
struct Host {
    subscribed: RefCell<Vec<u64>>,
    now: Cell<u64>,
    log: RefCell<Vec<String>>,
    ready: Cell<bool>,
}
impl LookupHost for Host {
    fn ask(&self, _frame: &dyn Frame, _: u64, ask: &Ask<'_>, out: &mut [u8]) -> Result<usize, i32> {
        let (provider, pending) = match ask.cursor {
            0 => ("wiki", !self.ready.get()),
            1 => ("translate", !self.ready.get()),
            2 => ("dict", false),
            _ => return Err(paperpaw_native::host_glue::ERR_NOT_FOUND),
        };
        let more = u32::from(ask.cursor < 2);
        wire::record_encode_one(
            &Record::ServiceLookupEntry(Entry {
                provider,
                title: "Word",
                text: if pending { "" } else { "definition" },
                flags: more | if pending { PENDING } else { 0 },
            }),
            out,
        )
    }
    fn provider_owner(&self, name: &str) -> Option<u64> {
        match name {
            "wiki" => Some(WIKI),
            "translate" => Some(TRANSLATE),
            _ => None,
        }
    }
    fn subscribed(&self, asker: u64) -> bool {
        self.subscribed.borrow().contains(&asker)
    }
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn log(&self, line: &str) {
        self.log.borrow_mut().push(line.into());
    }
}

struct Rig {
    host: Rc<Host>,
    hosts: crate::native_host::host_services::HostServices,
    kernel: crate::Kernel,
}
impl Frame for Rig {
    fn settings(&self) -> &Settings {
        self.kernel.shared.env(Instant(0)).settings()
    }
    fn translator(&self) -> &Translator {
        unreachable!("the lookup backend never translates")
    }
}
fn rig(subscribed: &[u64]) -> Rig {
    let host = Rc::new(Host {
        subscribed: RefCell::new(subscribed.into()),
        now: Cell::new(1_000),
        log: RefCell::new(Vec::new()),
        ready: Cell::new(false),
    });
    let hosts = crate::native_host::host_services::HostServices::new();
    let shared: Rc<dyn LookupHost> = host.clone();
    hosts.wire_lookup(shared, Default::default()).unwrap();
    Rig {
        host,
        hosts,
        kernel: crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap(),
    }
}
impl Rig {
    fn ask(&self, asker: u64, text: &str, cursor: u32) -> (String, u32, String) {
        let op = services_wire::service_schema("core.lookup", 1)
            .unwrap()
            .op(1)
            .unwrap();
        let mut input = std::vec![0; 4096];
        let used = wire::records_encode_counted(
            &[Record::ServiceLookupAsk(Ask {
                text,
                language: "en",
                context: "",
                cursor,
            })],
            1,
            op.max_input,
            &mut input,
        )
        .unwrap();
        let caller = Caller {
            owner: asker,
            plugin: "reader",
            features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
            hosts: &self.hosts,
        };
        let mut out = std::vec![0; op.max_reply];
        let length = backend(self, &caller, op, &input[..used], &mut out).unwrap();
        let Record::ServiceLookupEntry(entry) = wire::record_decode_one(&out[..length]).unwrap()
        else {
            panic!("an entry")
        };
        (entry.provider.into(), entry.flags, entry.text.into())
    }
    fn service(&self) -> &LookupServices {
        self.hosts.lookup().unwrap()
    }
    fn ready(&self, provider: u64, text: &str) -> Vec<u64> {
        let mut told = Vec::new();
        self.service()
            .ready(provider, "en", text, &mut |asker| told.push(asker));
        told.sort_unstable();
        told
    }
}

#[test]
fn a_pending_ask_is_kept_for_a_subscribed_asker_and_ready_tells_it_once() {
    let rig = rig(&[7, 8]);
    // The wiki provider answers pending, the asker 7 asks; so does 8.
    assert_eq!(rig.ask(7, "tree", 0), ("wiki".into(), 0b11, String::new()));
    assert_eq!(rig.ask(8, "tree", 0).1, 0b11);
    assert_eq!(rig.service().pending_len(), 2);
    // The provider kept its result: both askers are told, once.
    assert_eq!(rig.ready(WIKI, "tree"), [7, 8]);
    assert_eq!(rig.service().pending_len(), 0);
    assert_eq!(rig.ready(WIKI, "tree"), Vec::<u64>::new());
    assert_eq!(rig.service().unmatched(), 1);
    // The asker asks again and gets the answer, without `pending`.
    rig.host.ready.set(true);
    assert_eq!(
        rig.ask(7, "tree", 0),
        ("wiki".into(), 1, "definition".into())
    );
    assert_eq!(rig.service().pending_len(), 0);
}

#[test]
fn an_asker_that_does_not_subscribe_has_nothing_recorded() {
    let rig = rig(&[]);
    // The asker sees `pending` and may ask again itself.
    assert_eq!(rig.ask(7, "tree", 0), ("wiki".into(), 0b11, String::new()));
    assert_eq!(rig.service().pending_len(), 0);
    assert_eq!(rig.ready(WIKI, "tree"), Vec::<u64>::new());
}

#[test]
fn a_ready_command_matches_its_provider_and_its_text_only() {
    let rig = rig(&[7]);
    rig.ask(7, "tree", 0);
    rig.ask(7, "tree", 1);
    rig.ask(7, "leaf", 0);
    assert_eq!(rig.service().pending_len(), 3);
    // Another provider, another text, another language: no match.
    assert_eq!(rig.ready(TRANSLATE, "leaf"), Vec::<u64>::new());
    let mut told = Vec::new();
    rig.service()
        .ready(WIKI, "de", "tree", &mut |a| told.push(a));
    assert!(told.is_empty());
    assert_eq!(rig.ready(WIKI, "tree"), [7]);
    assert_eq!(rig.service().pending_len(), 2);
}

#[test]
fn a_full_table_answers_the_empty_entry_and_logs_it() {
    let rig = rig(&[7]);
    // The asker holds four; the fifth ask is the empty entry with its
    // provider's name and the host's `more` bit, no `pending`.
    for n in 0..4 {
        assert_eq!(rig.ask(7, &std::format!("w{n}"), 0).1, 0b11);
    }
    assert!(rig.host.log.borrow().is_empty());
    assert_eq!(rig.ask(7, "w4", 0), ("wiki".into(), 1, String::new()));
    assert_eq!(rig.service().pending_len(), 4);
    assert_eq!(rig.host.log.borrow().len(), 1);
    // Another ask meeting the full table adds no second line this minute.
    assert_eq!(rig.ask(7, "w5", 0).1, 1);
    assert_eq!(rig.host.log.borrow().len(), 1);
    // The last provider's page keeps its own bits (none more).
    assert_eq!(rig.ask(7, "w4", 2).1, 0);
    // Asking again for one that is kept refreshes it and is no refusal.
    assert_eq!(rig.ask(7, "w0", 0).1, 0b11);
}

#[test]
fn a_deadline_ends_an_ask_and_a_stop_ends_the_asks_of_and_to_a_plugin() {
    let rig = rig(&[7, 8]);
    rig.ask(7, "tree", 0);
    rig.ask(8, "tree", 1);
    // 61 s on: both ended without an event.
    rig.host.now.set(1_000 + 61_000);
    assert_eq!(rig.ready(WIKI, "tree"), Vec::<u64>::new());
    assert_eq!(rig.ready(TRANSLATE, "tree"), Vec::<u64>::new());
    assert_eq!(rig.service().pending_len(), 0);
    // An asker stops: its asks end; a provider stops: the asks to it end.
    rig.ask(7, "a", 0);
    rig.ask(8, "b", 0);
    rig.ask(8, "c", 1);
    rig.service().stopped(7);
    assert_eq!(rig.service().pending_len(), 2);
    rig.service().stopped(TRANSLATE);
    assert_eq!(rig.ready(WIKI, "b"), [8]);
    assert_eq!(rig.service().pending_len(), 0);
}

#[test]
fn unmatched_ready_commands_log_one_line_a_minute() {
    let rig = rig(&[]);
    for _ in 0..5 {
        rig.ready(WIKI, "none");
    }
    assert_eq!(rig.service().unmatched(), 5);
    assert_eq!(rig.host.log.borrow().len(), 1);
    rig.host.now.set(1_000 + 61_000);
    rig.ready(WIKI, "none");
    assert_eq!(rig.host.log.borrow().len(), 2);
}

#[test]
fn the_event_record_carries_the_time_and_the_provider() {
    let mut out = [0_u8; 96];
    let used = ready_event(123_456, "wiki", &mut out).unwrap();
    let Record::EventLookupReady(event) = wire::record_decode_one(&out[..used]).unwrap() else {
        panic!("an event")
    };
    assert_eq!((event.now_ms, event.provider), (123_456, "wiki"));
    assert_eq!(wire::EVENT_LOOKUP_READY_TAG, 110);
}

#[test]
fn delivery_runs_outside_the_table_so_it_may_stop_a_plugin_or_ask_again() {
    let rig = rig(&[7, 8]);
    rig.ask(7, "tree", 0);
    rig.ask(8, "tree", 0);
    rig.ask(8, "leaf", 0);
    let mut told = Vec::new();
    rig.service().ready(WIKI, "en", "tree", &mut |asker| {
        told.push(asker);
        // Stopping the asker 8 while it is told must take effect now.
        rig.service().stopped(8);
    });
    told.sort_unstable();
    assert_eq!(told, [7, 8]);
    // The other ask of 8 ended with the stop; nothing is left.
    assert_eq!(rig.service().pending_len(), 0);
}
