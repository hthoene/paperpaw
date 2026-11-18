//! The panel and a provider that answers later (todo 98, `document.md` 7),
//! driven by the scripted host: a page answered `pending` makes the panel wait
//! and start the timer that ends the wait (work `dictionary.answers`, its reply
//! buffer the op's 64 bytes), and the provider's `event.lookup_ready` makes it
//! look the word up again, without waiting any longer.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventLookupReady, EventScreenOpened, EventWorkDue, Record, ServiceEnd, ServiceLookupEntry,
    ServiceUiHandle, ServiceUiParam, record_encode_one, records_decode,
};
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_plugin_dictionary::{
    NATIVE_ENTRY,
    declarations::{ANSWERS_KEY, POPUP_TOKEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// What the plugin asked of the host.
#[derive(Default)]
struct Seen {
    /// The cursors of the lookups.
    asks: Vec<u32>,
    /// Work declared.
    declared: usize,
    /// The provider kept its answer.
    ready: bool,
}

fn setup(seen: &Rc<RefCell<Seen>>) -> ScriptedHost {
    let host = ScriptedHost::new();
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    // No card: no dictionary of its own, only the provider answers.
    host.service("core.files", 1, 0, |_, _| Err(-1));
    host.service("core.ui", 1, 0, |op, _| match op {
        16 => Ok(stream(&[Record::ServiceUiHandle(ServiceUiHandle {
            handle: 7,
        })])),
        17 => Ok(stream(&[
            Record::ServiceUiParam(ServiceUiParam {
                key: "word",
                value: "owl",
            }),
            Record::ServiceEnd(ServiceEnd {
                flags: 0,
                total: 1,
                generation: 0,
                next: 0,
                cursor: None,
            }),
        ])),
        _ => Err(-1),
    });
    let lookups = seen.clone();
    host.service("core.lookup", 1, 0, move |op, input| {
        let records = records_decode(input).map_err(|_| -1)?;
        let Some(Record::ServiceLookupAsk(ask)) = records.iter().next() else {
            return Err(-1);
        };
        if op != 1 || ask.text != "owl" {
            return Err(-1);
        }
        let mut seen = lookups.borrow_mut();
        seen.asks.push(ask.cursor);
        let (text, flags) = match (ask.cursor, seen.ready) {
            // Bit 1: pending, the provider fetches.
            (0, false) => ("", 2),
            (0, true) => ("A bird of the night.", 0),
            _ => return Err(ERR_NOT_FOUND),
        };
        Ok(stream(&[Record::ServiceLookupEntry(ServiceLookupEntry {
            provider: "summary",
            title: "Wikipedia",
            text,
            flags,
            opens: "",
        })]))
    });
    let work = seen.clone();
    host.service("core.work", 1, 0, move |op, _| {
        if op == 1 {
            work.borrow_mut().declared += 1;
            return Ok(Vec::new());
        }
        Err(-1)
    });
    // The op's `max_reply`: a smaller reply buffer is refused, as on a device.
    host.reply_bound("core.work", 1, 64);
    host
}

#[test]
fn a_later_answer_is_waited_for_and_looked_up_again_when_it_is_kept() {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let host = setup(&seen);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "dictionary.popup",
        instance: 1,
        slot: "core.lookup",
        flags: 0,
        handover: 7,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(POPUP_TOKEN, &opened).unwrap().status, 0);
    assert_eq!(seen.borrow().asks, [0, 1], "the provider's page, then none");
    assert_eq!(seen.borrow().declared, 1, "the wait's timer is declared");
    // The provider kept its answer: the word is asked again, from the start.
    seen.borrow_mut().ready = true;
    let ready = event_batch(&Record::EventLookupReady(EventLookupReady {
        now_ms: 1_000,
        provider: "summary",
    }))
    .unwrap();
    plugin.event(POPUP_TOKEN, &ready).unwrap();
    assert_eq!(seen.borrow().asks, [0, 1, 0, 1]);
    assert_eq!(seen.borrow().declared, 1, "nothing more is waited for");
    // Another provider's answer, or the same one again, asks nothing.
    plugin.event(POPUP_TOKEN, &ready).unwrap();
    assert_eq!(seen.borrow().asks.len(), 4);
    // The timer comes due: nothing waits, nothing is asked.
    let due = event_batch(&Record::EventWorkDue(EventWorkDue {
        now_ms: 20_000,
        token: WORK_TOKEN,
        why: 0,
        key: ANSWERS_KEY,
    }))
    .unwrap();
    assert_eq!(plugin.event(WORK_TOKEN, &due).unwrap().status, 0);
    assert_eq!(seen.borrow().asks.len(), 4);
}
