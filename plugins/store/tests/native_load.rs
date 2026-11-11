//! The fetch of the list and of a plugin's page, against a scripted host
//! (slice 5, host-free).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_device::Facts;
use paperpaw_plugin_store::native_load::{
    Details, Done, Ended, LIST_PART, LIST_TOKEN, Loader, PAGE_TOKEN, Trouble, Wire,
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
    loader.forget_page();
    assert_eq!(loader.details(), (None, Details::None));
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
