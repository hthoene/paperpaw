//! The queue of plugins to fetch and the requests of other plugins, kept on
//! the card in the interface-6 format (slice 5, host-free).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_queue::{Mark, QUEUE, QUIET, Queue, note, report};
use paperpaw_plugin_store::native_request::{
    DECLINED_MS, Last, REQUEST, RESULT, Request, Verdict, decide, entries, finished, names,
    outcome, wanted,
};
use paperpaw_sdk::{MemoryStorage, Storage};

fn ids(queue: &Queue) -> Vec<(&str, Mark, bool)> {
    queue
        .lines()
        .iter()
        .map(|l| (l.id.as_str(), l.mark, l.turn))
        .collect()
}

#[test]
fn a_beta_3_queue_is_read_and_written_as_it_was() {
    let storage = MemoryStorage::new();
    storage
        .write(QUEUE, b"> &fonts-pack\n~setup-a\nplain\n> \nbad name\n")
        .unwrap();
    let queue = Queue::read(&storage).unwrap();
    assert_eq!(
        ids(&queue),
        [
            ("fonts-pack", Mark::Requested, true),
            ("setup-a", Mark::Quiet, false),
            ("plain", Mark::Said, false),
            ("", Mark::Said, true),
            ("bad name", Mark::Said, false),
        ]
    );
    assert!(queue.requested() && queue.requested_waiting() && queue.quiet() && queue.has_quiet());
    queue.keep(&storage).unwrap();
    let text = String::from_utf8(paperpaw_sdk::read_file(&storage, QUEUE, 1000).unwrap()).unwrap();
    assert_eq!(text, "> &fonts-pack\n~setup-a\nplain\n> \nbad name\n");
}

#[test]
fn the_queue_goes_on_one_turn_at_a_time() {
    let storage = MemoryStorage::new();
    let mut queue = Queue::default();
    queue.add(["a", "b", "a", "c"], Mark::Said).unwrap();
    // Each once, behind those waiting; a plugin already there is not put twice.
    queue.add(["b", "d"], Mark::Quiet).unwrap();
    assert_eq!(queue.lines().len(), 4);
    assert!(!queue.quiet(), "none has its turn yet");
    assert!(queue.has_quiet());
    assert_eq!(queue.advance(), Some("a"));
    queue.keep(&storage).unwrap();
    // A restart cut the download off: its turn comes again, first.
    let mut again = Queue::read(&storage).unwrap();
    again.interrupted();
    assert_eq!(again.advance(), Some("a"));
    assert_eq!(again.advance(), Some("b"));
    assert_eq!(again.advance(), Some("c"));
    assert_eq!(again.advance(), Some("d"));
    assert!(again.quiet(), "d was put in quietly and has its turn");
    assert_eq!(again.advance(), None);
    // Empty: no file.
    again.keep(&storage).unwrap();
    assert!(storage.size(QUEUE).is_err());
}

#[test]
fn keeping_an_empty_queue_with_no_file_is_fine() {
    let storage = MemoryStorage::new();
    assert!(storage.size(QUEUE).is_err());
    Queue::default().keep(&storage).unwrap();
    // And with a file: it goes.
    storage.write(QUEUE, b"a\n").unwrap();
    Queue::default().keep(&storage).unwrap();
    assert!(storage.size(QUEUE).is_err());
}

#[test]
fn what_is_no_plugin_name_goes_when_its_turn_would_come() {
    let mut queue = Queue::default();
    queue
        .add(["Bad Name", "good-one", "-dash", ""], Mark::Said)
        .unwrap();
    assert_eq!(queue.advance(), Some("good-one"));
    assert_eq!(queue.lines().len(), 1);
}

#[test]
fn the_quiet_plugins_are_told_once_when_the_last_is_done() {
    let storage = MemoryStorage::new();
    note(&storage, "Alpha", true).unwrap();
    note(&storage, "Be\nta", false).unwrap();
    note(&storage, "Gamma", true).unwrap();
    let mut queue = Queue::default();
    queue.add(["x"], Mark::Quiet).unwrap();
    assert_eq!(report(&storage, &queue).unwrap(), None, "one still waits");
    assert!(storage.size(QUIET).is_ok());
    let empty = Queue::default();
    let said = report(&storage, &empty).unwrap().unwrap();
    assert_eq!(said.done, "Alpha, Gamma");
    assert_eq!(said.not, "Be ta");
    assert!(storage.size(QUIET).is_err(), "told once");
    assert_eq!(report(&storage, &empty).unwrap(), None);
}

#[test]
fn a_queue_that_cannot_be_read_is_an_empty_one() {
    let storage = MemoryStorage::new();
    assert!(Queue::read(&storage).unwrap().lines().is_empty());
    storage.write(QUEUE, &[0xff, 0xfe, b'\n']).unwrap();
    assert!(Queue::read(&storage).unwrap().lines().is_empty());
    storage.write(QUEUE, &vec![b'a'; 20_000]).unwrap();
    assert!(Queue::read(&storage).unwrap().lines().is_empty());
}

#[test]
fn an_ask_is_taken_as_the_store_takes_it() {
    let installed = |id: &str| id == "have";
    let list: Vec<String> = ["a", "have", "a", "b"]
        .iter()
        .map(ToString::to_string)
        .collect();
    let got = wanted(list.iter().map(String::as_str), &installed).unwrap();
    assert_eq!(got, ["a", "b"]);
    // At most 32, however many are named.
    let many: Vec<String> = (0..200).map(|n| format!("p{n}")).collect();
    let got = wanted(many.iter().map(String::as_str), &installed).unwrap();
    assert_eq!(got.len(), 32);
    // Only four times as many are looked at: the device's own are skipped first.
    let all_have: Vec<String> = (0..200).map(|_| "have".to_string()).collect();
    assert!(
        wanted(all_have.iter().map(String::as_str), &installed)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_request_is_kept_answered_and_closed() {
    let storage = MemoryStorage::new();
    assert_eq!(Request::open(&storage).unwrap(), None);
    let ids: Vec<String> = ["pack-a", "pack-b", "pack-c"]
        .iter()
        .map(ToString::to_string)
        .collect();
    Request::start(&storage, "reader", &ids).unwrap();
    let open = Request::open(&storage).unwrap().unwrap();
    assert_eq!((open.by.as_str(), open.ids.len()), ("reader", 3));
    outcome(&storage, "pack-a", true).unwrap();
    outcome(&storage, "pack-b", false).unwrap();
    let end = finished(&storage).unwrap().unwrap();
    assert_eq!(end.by, "reader");
    assert_eq!(end.installed, ["pack-a"]);
    assert_eq!(end.missing, ["pack-b", "pack-c"]);
    assert!(storage.size(REQUEST).is_err() && storage.size(RESULT).is_err());
    assert_eq!(finished(&storage).unwrap(), None);
    // Again: what was noted goes.
    outcome(&storage, "pack-a", true).unwrap();
    Request::again(&storage, "reader", &ids).unwrap();
    assert!(storage.size(RESULT).is_err());
    assert!(Request::open(&storage).unwrap().is_some());
    Request::close(&storage);
    assert!(Request::open(&storage).unwrap().is_none());
    // An asker that is no id, or no first line: no request.
    storage.write(REQUEST, b"Bad Id!\na\n").unwrap();
    assert_eq!(Request::open(&storage).unwrap(), None);
}

#[test]
fn an_ask_is_decided_as_interface_6_did() {
    let w: Vec<String> = vec!["a".into()];
    let last = |declined| Last {
        by: "reader".into(),
        ids: w.clone(),
        at_ms: 1_000,
        declined,
    };
    assert_eq!(
        decide("reader", &[], None, 0, false, false),
        Verdict::NothingMissing
    );
    assert_eq!(
        decide(
            "reader",
            &w,
            Some(&last(true)),
            1_000 + DECLINED_MS - 1,
            false,
            false
        ),
        Verdict::DeclinedAgain
    );
    // The window is over: asked anew.
    assert_eq!(
        decide(
            "reader",
            &w,
            Some(&last(true)),
            1_000 + DECLINED_MS,
            false,
            false
        ),
        Verdict::Ask { close_stale: false }
    );
    assert_eq!(
        decide("reader", &w, Some(&last(false)), 2_000, true, false),
        Verdict::SameOpen
    );
    // Not open any more: a dialog is shown again.
    assert_eq!(
        decide("reader", &w, Some(&last(false)), 2_000, false, false),
        Verdict::Ask { close_stale: false }
    );
    // Another asker, or other ids: not the same.
    assert_eq!(
        decide("other", &w, Some(&last(true)), 2_000, false, false),
        Verdict::Ask { close_stale: false }
    );
    // One at a time: another's request open with plugins waiting is Busy; a
    // dialog closed without an answer is closed where nothing waits.
    assert_eq!(decide("other", &w, None, 0, true, true), Verdict::Busy);
    assert_eq!(
        decide("other", &w, None, 0, true, false),
        Verdict::Ask { close_stale: true }
    );
}

#[test]
fn the_dialog_names_plugins_by_the_listing_not_by_the_ask() {
    use paperpaw_plugin_store::native_index::take;
    use paperpaw_plugin_store::native_item::Device;
    use paperpaw_plugin_store::native_list::Hooks;
    use paperpaw_sdk::native::{Fingerprint, INTERFACE};
    fn nobody(_: &str) -> Option<[u8; 32]> {
        None
    }
    fn none() -> Vec<String> {
        Vec::new()
    }
    let storage = MemoryStorage::new();
    let ask: Vec<String> = ["pack-a", "unlisted"]
        .iter()
        .map(ToString::to_string)
        .collect();
    // No index: every id stands for itself.
    let got = entries(&storage, &ask).unwrap();
    assert_eq!(names(&got).unwrap(), "pack-a, unlisted");
    let list = r#"{"format": 2, "plugins": [{"id": "pack-a", "name": "Letters A", "summary": "s",
 "group": "look", "kind": "data", "author": "A", "licence": "MIT",
 "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30}}]}"#;
    storage.write("list.json", list.as_bytes()).unwrap();
    let device = Device {
        name: "x4",
        fingerprint: Some(Fingerprint::of(INTERFACE, 1)),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    };
    let hooks = Hooks {
        author_of: &nobody,
        stopped: &none,
        host_checks: false,
    };
    take(&storage, "list.json", device, hooks).unwrap().unwrap();
    let got = entries(&storage, &ask).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].1.as_ref().map(|e| e.size), Some(30));
    assert!(got[1].1.is_none());
    assert_eq!(names(&got).unwrap(), "Letters A, unlisted");
}
