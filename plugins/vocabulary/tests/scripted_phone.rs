//! The phone page through the production loader with a scripted host: when
//! the memory runs out while the address is being made, the server that was
//! started is stopped again, never left running with nobody told how to reach
//! it. Every allocation after the event is read is refused in turn.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, EventServer, Record, ServiceEnd, ServiceFsEntry, ServiceFsProgress,
    ServiceSecBytes, record_encode_one, records_decode,
};
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_plugin_vocabulary::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

/// The plugin's tokens for the server's events, and the server's own number.
const SERVER_TOKEN: u32 = 9;
const SERVER: u32 = 1;
const PHONE_TOKEN: u32 = 16;
const FILES_LIST: u32 = 1;
const FILES_REMOVE_TREE: u32 = 5;
const SECRETS_RANDOM: u32 = 5;
const NET_SERVE: u32 = 5;
const NET_STOP: u32 = 6;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// The server `server` is up at an address; the plugin's status, how often it
/// stopped its server, and how many services it opened and called.
fn drive(refuse: Option<(Slot, usize)>, server: u32) -> (i32, usize, [usize; 2]) {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let stops = Rc::new(RefCell::new(0));
    let seen = stops.clone();
    host.service("core.secrets", 1, 0, |op, input| match op {
        SECRETS_RANDOM => {
            let asked = records_decode(input).unwrap().iter().find_map(|r| match r {
                Record::ServiceSecRandom(random) => Some(random.len as usize),
                _ => None,
            });
            let data: Vec<u8> = (0..asked.unwrap_or(0))
                .map(|i| u8::try_from((i * 37 + 11) % 256).unwrap())
                .collect();
            Ok(stream(&[Record::ServiceSecBytes(ServiceSecBytes {
                flags: 1,
                data: &data,
            })]))
        }
        _ => Err(-1),
    });
    host.service("core.net", 1, 0, move |op, _| {
        if op == NET_STOP {
            *seen.borrow_mut() += 1;
        }
        Ok(Vec::new())
    });
    host.reply_bound("core.secrets", SECRETS_RANDOM, 1_280);
    for op in [NET_SERVE, NET_STOP] {
        host.reply_bound("core.net", op, 1_536);
    }
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    host.reset_counts();
    if let Some((slot, nth)) = refuse {
        host.refuse_nth(slot, nth, ERR_NO_MEMORY);
    }
    let batch = event_batch(&Record::EventServer(EventServer {
        now_ms: 3_100,
        server,
        state: 0,
        reason: 0,
        address: "192.168.1.20:8080",
    }))
    .unwrap();
    let done = plugin.event(SERVER_TOKEN, &batch).unwrap();
    let calls = [host.calls(Slot::ServiceOpen), host.calls(Slot::ServiceCall)];
    (done.status, *stops.borrow(), calls)
}

#[test]
fn memory_that_runs_out_while_the_address_is_made_stops_the_server() {
    let (status, stops, calls) = drive(None, SERVER);
    assert_eq!((status, stops), (0, 0), "a clean start stops nothing");
    // The key is drawn from the secrets: a service opened and called.
    assert!(calls[0] >= 1 && calls[1] >= 1, "{calls:?}");
    // The host out of memory at each call in turn, as at each open.
    for (slot, count) in [(Slot::ServiceOpen, calls[0]), (Slot::ServiceCall, calls[1])] {
        for nth in 1..=count {
            let (status, stops, _) = drive(Some((slot, nth)), SERVER);
            assert!(status < 0, "call {nth} refused: the event still ends well");
            assert_eq!(stops, 1, "call {nth} refused: the server was left running");
        }
    }
}

fn entry(name: &str, flags: u32) -> Record<'_> {
    Record::ServiceFsEntry(ServiceFsEntry {
        name,
        flags,
        // A folder has no size.
        size: u64::from(flags == 0),
        modified: None,
    })
}

/// What a power cut left in the data folder (the page, a long answer) is
/// removed when the screen opens; the decks and what only looks alike stay.
#[test]
fn opening_the_screen_removes_what_an_earlier_serving_left() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let seen = log.clone();
    host.service("core.files", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (FILES_LIST, Some(Record::ServiceFsList(list))) => {
                seen.borrow_mut().push(format!("list {:?}", list.path));
                Ok(stream(&[
                    entry("decks.txt", 0),
                    entry("phone.html", 0),
                    entry("phone-3.json", 0),
                    entry("phone-x.json", 0),
                    entry("phone-9.json", 1),
                    Record::ServiceEnd(ServiceEnd {
                        flags: 1,
                        total: 0,
                        generation: 0,
                        next: 0,
                        cursor: None,
                    }),
                ]))
            }
            (FILES_REMOVE_TREE, Some(Record::ServiceFsList(gone))) => {
                seen.borrow_mut().push(format!("remove {}", gone.path));
                Ok(stream(&[Record::ServiceFsProgress(ServiceFsProgress {
                    removed: 1,
                    flags: 1,
                    next: 0,
                })]))
            }
            _ => Err(-1),
        }
    });
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 1_000,
        screen: "vocabulary.phone",
        instance: 1,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    // What the rest of the opening does on a host with no storage is not the
    // point: the files service was asked, and what was left was removed.
    plugin.event(PHONE_TOKEN, &opened).unwrap();
    assert_eq!(
        *log.borrow(),
        ["list \"\"", "remove phone.html", "remove phone-3.json"]
    );
}
