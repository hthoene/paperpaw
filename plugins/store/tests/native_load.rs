//! The fetch of the list and of a plugin's page, against a scripted host
//! (slice 5, host-free).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_device::Facts;
use paperpaw_plugin_store::native_load::{
    Details, Done, Ended, LIST_PART, LIST_TOKEN, Loader, PAGE_TOKEN, QUEUE_PAGE_TOKEN, Trouble,
    Wire,
};
use paperpaw_plugin_store::native_model::Revoked;
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_sdk::{MemoryStorage, Storage};

struct Script {
    storage: MemoryStorage,
    facts: Option<Facts>,
    facts_error: Option<Error>,
    get: Result<u32, Error>,
    asked: Vec<(String, String, u32)>,
    cancelled: Vec<u32>,
    revoked: usize,
}

impl Script {
    fn new() -> Self {
        Self {
            storage: MemoryStorage::new(),
            facts_error: None,
            facts: Some(Facts::new("x4", "code.ppn", "en", true, Vec::new()).unwrap()),
            get: Ok(7),
            asked: Vec::new(),
            cancelled: Vec::new(),
            revoked: 0,
        }
    }
}

impl Wire for Script {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }
    fn facts(&mut self) -> Result<Option<Facts>, Error> {
        if let Some(error) = self.facts_error {
            return Err(error);
        }
        Ok(self.facts.clone())
    }
    fn catalog(&mut self) -> Result<(String, bool), Error> {
        Ok(("https://lib.example/plugins.json".into(), false))
    }
    fn get(&mut self, url: &str, file: &str, token: u32, _max: u64) -> Result<RequestId, Error> {
        self.asked.push((url.into(), file.into(), token));
        self.get.map(|n| RequestId::new(n).unwrap())
    }
    fn cancel(&mut self, token: u32) {
        self.cancelled.push(token);
    }
    fn final_url(&mut self, _id: RequestId) -> Option<String> {
        None
    }
    fn small_memory(&self) -> bool {
        false
    }
    fn revoke(&mut self, revoked: &[Revoked]) -> Result<(), Error> {
        self.revoked += revoked.len();
        Ok(())
    }
}

const LIST: &str = r#"{"format": 2, "plugins": [{"id": "plugin-a", "name": "A", "summary": "Does a well.", "group": "tools",
 "kind": "data", "author": "X", "licence": "MIT", "added": "2024-01-02",
 "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30, "date": "2024-01-02"}}]}"#;

fn done(token: u32, reason: Reason, status: u32) -> Done {
    Done::new(RequestId::new(7).unwrap(), token, reason, status)
}

#[test]
fn the_list_is_asked_for_the_device_and_kept_when_it_comes() {
    let mut wire = Script::new();
    let mut loader = Loader::default();
    loader.open(&wire.storage).unwrap();
    loader.start_list(&mut wire).unwrap();
    assert_eq!(loader.trouble(), Trouble::Loading);
    let (url, file, token) = wire.asked.first().unwrap().clone();
    assert!(url.starts_with("https://lib.example/plugins.json"), "{url}");
    assert!(url.ends_with("&order=fair"), "{url}");
    assert_eq!((file.as_str(), token), (LIST_PART, LIST_TOKEN));
    wire.storage.write(LIST_PART, LIST.as_bytes()).unwrap();
    let ended = loader
        .ended(&mut wire, &done(LIST_TOKEN, Reason::Ok, 200))
        .unwrap();
    assert_eq!(ended, Ended::ListChanged);
    assert_eq!(loader.trouble(), Trouble::None);
    assert_eq!(loader.index().unwrap().len(), 1);
}

#[test]
fn a_failed_list_is_offline_without_an_answer_and_failed_with_one() {
    for (reason, status, trouble) in [
        (Reason::NoNetwork, 0, Trouble::Offline),
        // The host tells a connection it had no memory for as too large.
        (Reason::TooLarge, 0, Trouble::Memory),
        (Reason::Ok, 500, Trouble::Failed),
    ] {
        let mut wire = Script::new();
        let mut loader = Loader::default();
        loader.start_list(&mut wire).unwrap();
        let ended = loader
            .ended(&mut wire, &done(LIST_TOKEN, reason, status))
            .unwrap();
        assert_eq!(ended, Ended::ListUnchanged);
        assert_eq!(loader.trouble(), trouble);
        assert!(loader.index().is_none());
    }
}

#[test]
fn a_list_that_is_no_list_leaves_the_index_there_is() {
    let mut wire = Script::new();
    let mut loader = Loader::default();
    loader.start_list(&mut wire).unwrap();
    wire.storage.write(LIST_PART, LIST.as_bytes()).unwrap();
    loader
        .ended(&mut wire, &done(LIST_TOKEN, Reason::Ok, 200))
        .unwrap();
    loader.start_list(&mut wire).unwrap();
    wire.storage.write(LIST_PART, b"not json").unwrap();
    let ended = loader
        .ended(&mut wire, &done(LIST_TOKEN, Reason::Ok, 200))
        .unwrap();
    assert_eq!(ended, Ended::ListUnchanged);
    assert_eq!(loader.index().unwrap().len(), 1);
    assert_eq!(loader.trouble(), Trouble::None);
    assert!(!wire.storage.exists(LIST_PART), "the part is removed");
}

#[test]
fn nothing_is_asked_where_the_library_does_not_know_the_device() {
    let mut wire = Script::new();
    wire.facts = None;
    let mut loader = Loader::default();
    loader.start_list(&mut wire).unwrap();
    assert!(wire.asked.is_empty());
    // The start says so instead of loading for ever.
    assert_eq!(loader.trouble(), Trouble::Unsupported);
}

#[test]
fn no_network_service_is_offline_and_a_refused_allocation_is_an_error() {
    let mut wire = Script::new();
    let mut loader = Loader::default();
    wire.get = Err(Error::Unsupported);
    loader.start_list(&mut wire).unwrap();
    assert_eq!(loader.trouble(), Trouble::Offline);
    wire.get = Err(Error::NoMemory);
    assert_eq!(loader.start_list(&mut wire), Err(Error::NoMemory));
}

#[test]
fn another_request_ended_is_ignored() {
    let mut wire = Script::new();
    let mut loader = Loader::default();
    loader.start_list(&mut wire).unwrap();
    let other = Done::new(RequestId::new(9).unwrap(), LIST_TOKEN, Reason::Ok, 200);
    assert_eq!(loader.ended(&mut wire, &other).unwrap(), Ended::Ignored);
    assert_eq!(loader.trouble(), Trouble::Loading);
}

#[test]
fn a_page_of_a_bad_name_is_not_asked_for_and_a_failed_one_says_so() {
    let mut wire = Script::new();
    let mut loader = Loader::default();
    loader.start_page(&mut wire, "bad name").unwrap();
    assert!(wire.asked.is_empty());
    assert_eq!(loader.details(), (Some("bad name"), Details::Failed));
    loader.start_page(&mut wire, "plugin-a").unwrap();
    assert_eq!(loader.details(), (Some("plugin-a"), Details::Asked));
    let (_, file, token) = wire.asked.first().unwrap().clone();
    assert_eq!(token, PAGE_TOKEN);
    assert_eq!(file, "plugin-plugin-a.part");
    let ended = loader
        .ended(&mut wire, &done(PAGE_TOKEN, Reason::NoNetwork, 0))
        .unwrap();
    assert_eq!(ended, Ended::Page);
    assert_eq!(loader.details(), (Some("plugin-a"), Details::Offline));
    loader.forget_page(&mut wire);
    assert_eq!(loader.details(), (None, Details::None));
}

/// What the library answers for the page of plugin `id`.
fn page_of(id: &str) -> String {
    format!(
        r#"{{"format": 2, "plugin": {{"id": "{id}", "name": "A", "summary": "Does a well.",
  "group": "tools", "kind": "data", "author": "X", "licence": "MIT",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30,
    "files": [{{"path": "plugin.ini", "url": "plugin.ini", "size": 30,
      "sha256": "{zero}"}}]}}}}}}"#,
        zero = "0".repeat(64)
    )
}

/// #486: the queue and a page screen shared one page; a screen opened while
/// the queue waited took it, and the queue (or the screen) waited for ever.
#[test]
fn the_queues_page_and_a_screens_page_are_fetched_apart() {
    let mut wire = Script::new();
    let mut loader = Loader::default();
    loader.start_queue_page(&mut wire, "plugin-a").unwrap();
    loader.start_page(&mut wire, "plugin-b").unwrap();
    assert_eq!(loader.queue_details(), (Some("plugin-a"), Details::Asked));
    assert_eq!(loader.details(), (Some("plugin-b"), Details::Asked));
    // Each into a file of its own, each told with its own number.
    let asked: Vec<_> = wire
        .asked
        .iter()
        .map(|(_, file, token)| (file.as_str(), *token))
        .collect();
    assert_eq!(
        asked,
        [
            ("queue-plugin-a.part", QUEUE_PAGE_TOKEN),
            ("plugin-plugin-b.part", PAGE_TOKEN)
        ]
    );
    assert!(wire.cancelled.is_empty());
    // Another page screen's page ends only the screen's request.
    loader.start_page(&mut wire, "plugin-c").unwrap();
    assert_eq!(wire.cancelled, [PAGE_TOKEN]);
    assert_eq!(loader.queue_details(), (Some("plugin-a"), Details::Asked));
    wire.storage
        .write("queue-plugin-a.part", page_of("plugin-a").as_bytes())
        .unwrap();
    let ended = loader
        .ended(&mut wire, &done(QUEUE_PAGE_TOKEN, Reason::Ok, 200))
        .unwrap();
    assert_eq!(ended, Ended::QueuePage);
    assert_eq!(loader.queue_details(), (Some("plugin-a"), Details::Here));
    assert!(loader.queue_page("plugin-a").is_some());
    // The screens read only their own page; the screen's is still coming.
    assert!(loader.page("plugin-a").is_none());
    assert_eq!(loader.details(), (Some("plugin-c"), Details::Asked));
    // Its install took what it needs: the queue's page goes, the screen's stays.
    loader.release_queue_page("plugin-a");
    assert_eq!(loader.queue_details(), (None, Details::None));
    assert_eq!(loader.details(), (Some("plugin-c"), Details::Asked));
}

/// #486: leaving the last page screen dropped the page without ending its
/// request: the answer was ignored and its part (up to 512 KiB) stayed.
#[test]
fn a_page_let_go_ends_its_request_and_its_part_goes() {
    let mut wire = Script::new();
    let mut loader = Loader::default();
    loader.start_page(&mut wire, "plugin-a").unwrap();
    loader.start_queue_page(&mut wire, "plugin-b").unwrap();
    wire.storage.write("plugin-plugin-a.part", b"{").unwrap();
    wire.cancelled.clear();
    loader.forget_page(&mut wire);
    assert_eq!(wire.cancelled, [PAGE_TOKEN]);
    assert!(!wire.storage.exists("plugin-plugin-a.part"));
    assert_eq!(loader.details(), (None, Details::None));
    // The queue's page is the queue's.
    assert_eq!(loader.queue_details(), (Some("plugin-b"), Details::Asked));
    wire.storage.write("queue-plugin-b.part", b"{").unwrap();
    loader.forget_queue_page(&mut wire);
    assert_eq!(wire.cancelled, [PAGE_TOKEN, QUEUE_PAGE_TOKEN]);
    assert!(!wire.storage.exists("queue-plugin-b.part"));
    // A page asked for anew lets the one before go the same way.
    loader.start_page(&mut wire, "plugin-a").unwrap();
    wire.storage.write("plugin-plugin-a.part", b"{").unwrap();
    wire.cancelled.clear();
    loader.start_page(&mut wire, "plugin-b").unwrap();
    assert_eq!(wire.cancelled, [PAGE_TOKEN]);
    assert!(!wire.storage.exists("plugin-plugin-a.part"));
    loader.forget_page(&mut wire);
    // A page that is here asks the host for nothing when it goes.
    loader.start_page(&mut wire, "plugin-a").unwrap();
    loader
        .ended(&mut wire, &done(PAGE_TOKEN, Reason::NoNetwork, 0))
        .unwrap();
    wire.cancelled.clear();
    loader.forget_page(&mut wire);
    assert!(wire.cancelled.is_empty());
}

/// #486: a request the host refused (too many at once, no network service)
/// read as a page the library no longer offers, and the queue waited for an
/// answer that never came.
#[test]
fn a_page_the_host_refuses_to_ask_for_is_no_connection() {
    for refusal in [Error::Busy, Error::Denied, Error::Unsupported] {
        let mut wire = Script::new();
        wire.get = Err(refusal);
        let mut loader = Loader::default();
        loader.start_page(&mut wire, "plugin-a").unwrap();
        assert_eq!(loader.details(), (Some("plugin-a"), Details::Offline));
        loader.start_queue_page(&mut wire, "plugin-a").unwrap();
        assert_eq!(loader.queue_details(), (Some("plugin-a"), Details::Offline));
    }
    // A library that does not know the device has no page of it.
    let mut wire = Script::new();
    wire.facts = None;
    let mut loader = Loader::default();
    loader.start_queue_page(&mut wire, "plugin-a").unwrap();
    assert!(wire.asked.is_empty());
    assert_eq!(loader.queue_details(), (Some("plugin-a"), Details::Failed));
}

#[test]
fn a_fetch_that_cannot_go_on_leaves_no_part_and_no_loading() {
    let mut wire = Script::new();
    let mut loader = Loader::default();
    loader.start_list(&mut wire).unwrap();
    wire.storage.write(LIST_PART, LIST.as_bytes()).unwrap();
    wire.facts_error = Some(Error::NoMemory);
    let ended = loader.ended(&mut wire, &done(LIST_TOKEN, Reason::Ok, 200));
    assert_eq!(ended, Err(Error::NoMemory));
    assert_eq!(loader.trouble(), Trouble::Failed);
    assert!(!wire.storage.exists(LIST_PART));
    wire.facts_error = None;
    loader.start_page(&mut wire, "plugin-a").unwrap();
    wire.facts_error = Some(Error::NoMemory);
    let ended = loader.ended(&mut wire, &done(PAGE_TOKEN, Reason::Ok, 200));
    assert_eq!(ended, Err(Error::NoMemory));
    assert_eq!(loader.details(), (Some("plugin-a"), Details::Failed));
}

#[test]
fn a_list_offline_mode_held_back_is_told_apart_and_forgotten_on_the_next_ask() {
    // #459: the start then offers to go online (Beta 3), not only *No
    // connection* with a tap that tries again in vain.
    let mut wire = Script::new();
    let mut loader = Loader::default();
    loader.start_list(&mut wire).unwrap();
    loader
        .ended(&mut wire, &done(LIST_TOKEN, Reason::Offline, 0))
        .unwrap();
    assert_eq!(loader.trouble(), Trouble::Offline);
    assert!(loader.held_offline());
    loader.start_list(&mut wire).unwrap();
    assert!(!loader.held_offline());
    loader
        .ended(&mut wire, &done(LIST_TOKEN, Reason::NoNetwork, 0))
        .unwrap();
    assert!(!loader.held_offline(), "no network is not Offline mode");
}
