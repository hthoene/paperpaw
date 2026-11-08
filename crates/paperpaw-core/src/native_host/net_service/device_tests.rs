#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The production `core.net` host over a fake shell and a memory card: what
//! core derives itself (roots and paths, sizes, claims, the staged file of a
//! saved answer), the translation to the shell's plain types, and the views of
//! the owner directory.
extern crate std;
use super::*;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    event_tags::EventTags,
    host_services::HostServices,
    net_service::{
        self, DONE_SAVED, NetProfile, table,
        tests::{self as fixtures, Event, OWNER},
    },
    owner_directory::OwnerDirectory,
    shared::Shared,
    shell_wiring::{NetInputs, NetLimits, NetPowerEnd, ShellHttp, ShellNetwork},
};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_IO, ERR_STATE};
use paperpaw_native::metadata::Feature;
use paperpaw_sdk::MemoryStorage;
use std::{string::ToString, vec};

static WIRED: &[Feature<'static>] = &[Feature {
    key: "uses.storage",
    revision: 1,
}];

type Started = (u32, std::string::String, Option<std::string::String>);

#[derive(Default)]
struct Shell {
    up: Cell<bool>,
    started: RefCell<Vec<Started>>,
    aborted: RefCell<Vec<u32>>,
    listen_with: RefCell<Option<Result<&'static str, ServerReason>>>,
    demand: RefCell<Vec<bool>>,
}

impl ShellHttp for Shell {
    fn start(&self, job: &HttpJob<'_>) -> Result<(), i32> {
        self.started.borrow_mut().push((
            job.id,
            job.url.to_string(),
            job.save_path.map(ToString::to_string),
        ));
        Ok(())
    }
    fn abort(&self, id: u32) {
        self.aborted.borrow_mut().push(id);
    }
    fn listen(&self, _: u32, _: u32, address: &mut [u8]) -> Result<usize, ServerReason> {
        let text = (*self.listen_with.borrow()).unwrap_or(Ok("192.168.1.2:80"))?;
        let room = address.get_mut(..text.len()).ok_or(ServerReason::Refused)?;
        room.copy_from_slice(text.as_bytes());
        Ok(text.len())
    }
    fn unlisten(&self, _: u32) {}
    fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn close_served(&self, _: u32, _: u32) {}
}

impl ShellNetwork for Shell {
    fn now_ms(&self) -> u64 {
        1_000
    }
    fn network_up(&self) -> bool {
        self.up.get()
    }
    fn network_demand(&self, wanted: bool) {
        self.demand.borrow_mut().push(wanted);
    }
    fn power_end(&self, _: &str, _: bool) -> Option<NetPowerEnd> {
        None
    }
}

struct Fixture {
    card: Rc<MemoryStorage>,
    shell: Rc<Shell>,
    device: NetDevice,
    hosts: HostServices,
    owners: Shared<OwnerDirectory>,
}

fn fixture(limits: NetLimits) -> Fixture {
    let card = Rc::new(MemoryStorage::new());
    let shell = Rc::new(Shell::default());
    shell.up.set(true);
    let hosts = HostServices::new();
    let owners = hosts.owners().unwrap().try_clone().unwrap();
    let device = NetInputs::new(card.clone(), shell.clone(), shell.clone(), limits)
        .device(hosts.in_use().clone(), owners.try_clone().unwrap());
    // The plugin `mail` runs and subscribed to every event.
    owners.admit(OWNER, "mail").unwrap();
    owners.subscribe(OWNER, EventTags::collect(|_| true));
    Fixture {
        card,
        shell,
        device,
        hosts,
        owners,
    }
}

fn x4() -> NetLimits {
    NetLimits::new(49_152, 2).memory_bodies(2).cable(true)
}

fn caller(f: &Fixture, bits: u32) -> Caller<'_> {
    Caller {
        owner: OWNER,
        plugin: "mail",
        features: AdmittedFeatures::test(bits, WIRED),
        hosts: &f.hosts,
    }
}

fn request(f: &Fixture, input: &[u8]) -> Result<u32, i32> {
    table::request(&f.device, &caller(f, 1), input)
}

fn events(f: &Fixture) -> Vec<Event> {
    fixtures::drain(&f.device, OWNER)
}

fn finish(f: &Fixture, id: u32, done: HttpDone<'_>) -> Result<(), i32> {
    f.device.report_finished(id, done)
}

fn saved(status: u32) -> HttpDone<'static> {
    let mut done = HttpDone::new(status, HttpReason::Ok);
    done.saved = true;
    done
}

#[test]
fn limits_follow_the_pool_and_are_the_shells_numbers() {
    let none = NetLimits::new(0, 6);
    assert_eq!(
        (none.memory_bodies, none.max_body, none.cable),
        (8, 1_048_576, false)
    );
    let pooled = NetLimits::new(49_152, 2);
    assert_eq!((pooled.memory_bodies, pooled.max_body), (2, 45_056));
    let f = fixture(x4());
    assert_eq!(f.device.profile(), NetProfile::X4);
    let f = fixture(NetLimits::new(0, 6));
    assert_eq!(f.device.table().pool().capacity(), 0);
}

#[test]
fn roots_follow_the_files_rules() {
    let f = fixture(x4());
    let (none, storage) = (caller(&f, 0), caller(&f, 1));
    let d = &f.device;
    assert_eq!(d.root_access(&none, 0, "a/b.txt", true), Ok(()));
    assert_eq!(d.root_access(&none, 2, "x.dat", false), Ok(()));
    assert_eq!(d.root_access(&none, 2, "x.dat", true), Err(ERR_DENIED));
    assert_eq!(d.root_access(&none, 0, ".received/1/2", false), Ok(()));
    assert_eq!(
        d.root_access(&none, 0, ".Received/1/2", true),
        Err(ERR_DENIED)
    );
    assert_eq!(
        d.root_access(&none, 1, "Books/a.epub", true),
        Err(ERR_DENIED)
    );
    assert_eq!(d.root_access(&storage, 1, "Books/a.epub", true), Ok(()));
    assert_eq!(
        d.root_access(&storage, 1, "Plugins/x", false),
        Err(ERR_INVALID)
    );
    assert_eq!(
        d.root_access(&storage, 1, ".paperpaw/secrets.txt", false),
        Err(ERR_INVALID)
    );
    assert_eq!(d.root_access(&storage, 3, "a", false), Err(ERR_UNSUPPORTED));
    assert_eq!(d.root_access(&storage, 1, "a//b", false), Err(ERR_INVALID));
    assert_eq!(d.root_access(&storage, 1, "../x", false), Err(ERR_INVALID));
    // The staged name is core's.
    assert_eq!(
        d.root_access(&storage, 1, "Books/a.epub.pp-staged", true),
        Err(ERR_INVALID)
    );
}

#[test]
fn a_file_to_send_is_found_by_its_joined_path() {
    let f = fixture(x4());
    f.card.write("Books/a.epub", &[0; 12]).unwrap();
    let who = caller(&f, 1);
    assert_eq!(f.device.file_len(&who, 1, "Books/a.epub"), Ok(12));
    assert_eq!(f.device.file_len(&who, 1, "Books/none"), Err(ERR_NOT_FOUND));
    assert_eq!(f.device.file_len(&who, 1, "Books"), Err(ERR_NOT_FOUND));
    assert_eq!(f.device.file_len(&who, 9, "x"), Err(ERR_UNSUPPORTED));
    // Root 0 of the plugin of the call.
    f.card
        .write(".paperpaw/plugins/mail/data.bin", &[0; 5])
        .unwrap();
    assert_eq!(f.device.file_len(&who, 0, "data.bin"), Ok(5));
}

#[test]
fn names_are_one_file_by_spelling_and_a_short_name_is_taken_for_the_same() {
    let f = fixture(x4());
    assert!(f.device.same_file(1, "Books/A.epub", "Books/A.epub"));
    assert!(!f.device.same_file(1, "Books/a.epub", "Books/b.epub"));
    assert!(
        f.device
            .same_file(1, "Books/LONGNA~1.EPU", "Books/other.epub")
    );
}

#[test]
fn a_saved_answer_is_staged_then_renamed_whole_and_the_paths_are_held_meanwhile() {
    let f = fixture(x4());
    f.card.write("Books/a.epub", b"old").unwrap();
    let id = request(&f, &fixtures::save_to(1, 1, "Books/a.epub")).unwrap();
    // The shell is told the staged path, never the final name.
    let (started, url, save) = f.shell.started.borrow()[0].clone();
    assert_eq!((started, url.as_str()), (id, "https://example.org/a"));
    assert_eq!(save.as_deref(), Some("Books/a.epub.pp-staged"));
    // Both paths are in use (`core.files` answers `ERR_BUSY` for them).
    let in_use = f.hosts.in_use();
    assert!(in_use.holds("Books/a.epub", false));
    assert!(in_use.holds("Books/a.epub.pp-staged", false));
    assert!(in_use.holds("Books", false), "a folder holding one");
    // The shell writes the staged file; the old file is whole until the end.
    f.card.write("Books/a.epub.pp-staged", b"new body").unwrap();
    assert_eq!(f.card.size("Books/a.epub"), Ok(3));
    assert_eq!(finish(&f, id, saved(200)), Ok(()));
    assert_eq!(f.card.size("Books/a.epub"), Ok(8));
    assert!(f.card.size("Books/a.epub.pp-staged").is_err());
    let told = events(&f);
    assert!(matches!(told[0], Event::Done { flags, reason: 0, .. } if flags & DONE_SAVED != 0));
    assert!(!f.hosts.in_use().holds("Books/a.epub", false));
}

#[test]
fn a_failed_or_partial_answer_leaves_the_old_file_and_no_staged_file() {
    for (status, reason) in [(0, HttpReason::Timeout), (500, HttpReason::Ok)] {
        let f = fixture(x4());
        f.card.write("Books/a.epub", b"old").unwrap();
        let id = request(&f, &fixtures::save_to(1, 1, "Books/a.epub")).unwrap();
        f.card.write("Books/a.epub.pp-staged", b"half").unwrap();
        let mut done = HttpDone::new(status, reason);
        done.saved = true;
        assert_eq!(finish(&f, id, done), Ok(()));
        assert_eq!(f.card.size("Books/a.epub"), Ok(3));
        assert!(f.card.size("Books/a.epub.pp-staged").is_err());
        assert!(!f.hosts.in_use().holds("Books/a.epub", false));
        let told = events(&f);
        assert!(matches!(told[0], Event::Done { flags, .. } if flags & DONE_SAVED == 0));
    }
}

#[test]
fn an_abort_removes_the_staged_file() {
    let f = fixture(x4());
    let id = request(&f, &fixtures::save_to(1, 1, "Books/a.epub")).unwrap();
    f.card.write("Books/a.epub.pp-staged", b"half").unwrap();
    table::cancel(&f.device, OWNER, 1).unwrap();
    assert_eq!(*f.shell.aborted.borrow(), [id]);
    assert!(f.card.size("Books/a.epub.pp-staged").is_err());
    assert!(!f.hosts.in_use().holds("Books/a.epub", false));
}

#[test]
fn a_busy_table_is_retried_without_renaming_twice() {
    let f = fixture(x4());
    f.card.write("Books/a.epub", b"old").unwrap();
    let id = request(&f, &fixtures::save_to(1, 1, "Books/a.epub")).unwrap();
    f.card.write("Books/a.epub.pp-staged", b"new").unwrap();
    {
        let _held = f.device.table().lock().unwrap();
        assert_eq!(finish(&f, id, saved(200)), Err(ERR_BUSY));
    }
    // The rename is done; a file written meanwhile at the target is not
    // replaced again by the retry.
    assert_eq!(f.card.size("Books/a.epub"), Ok(3));
    f.card.write("Books/a.epub", b"changed").unwrap();
    assert_eq!(finish(&f, id, saved(200)), Ok(()));
    assert_eq!(f.card.size("Books/a.epub"), Ok(7));
    assert!(matches!(events(&f)[0], Event::Done { reason: 0, .. }));
}

#[test]
fn a_body_with_more_room_than_the_request_reserved_is_refused() {
    let f = fixture(x4());
    let id = request(&f, &fixtures::big(1, 100)).unwrap();
    let held = f.device.table().pool().used();
    let mut done = HttpDone::new(200, HttpReason::Ok);
    done.body = Vec::with_capacity(101);
    assert_eq!(finish(&f, id, done), Ok(()));
    let told = events(&f);
    assert!(matches!(
        told[0],
        Event::Done { reason: r, body_len: 0, .. } if r == net_service::reason::TOO_LARGE
    ));
    assert!(
        f.device.table().pool().used() < held,
        "the reservation is given back"
    );
    // A body that fits is kept.
    let id = request(&f, &fixtures::big(2, 100)).unwrap();
    let mut done = HttpDone::new(200, HttpReason::Ok);
    done.body = Vec::with_capacity(100);
    done.body.extend_from_slice(b"abc");
    assert_eq!(finish(&f, id, done), Ok(()));
    assert!(matches!(events(&f)[0], Event::Done { body_len: 3, .. }));
}

#[test]
fn paths_are_held_once_for_core_net_and_core_files() {
    let f = fixture(x4());
    let in_use = f.hosts.in_use();
    // Another writer holds a path: a save to it is `ERR_BUSY`, and nothing is held.
    in_use.claim(99, "Books/b.epub").unwrap();
    assert_eq!(
        request(&f, &fixtures::save_to(1, 1, "Books/b.epub")),
        Err(ERR_BUSY)
    );
    assert!(!in_use.holds("Books/a.epub", false));
    // One's own claims, again, are fine and counted.
    f.device.claim_save(OWNER, 1, "Books/a.epub").unwrap();
    f.device.claim_save(OWNER, 1, "Books/a.epub").unwrap();
    f.device.release_save(OWNER, 1, "Books/a.epub");
    assert!(in_use.holds("Books/a.epub", false), "one claim is left");
    f.device.release_save(OWNER, 1, "Books/a.epub");
    assert!(!in_use.holds("Books/a.epub", false));
    // A case-insensitive card takes another spelling for the path.
    f.device.claim_save(OWNER, 1, "Books/c.epub").unwrap();
    assert!(in_use.holds("BOOKS/C.EPUB", true));
    assert!(!in_use.holds("BOOKS/C.EPUB", false));
}

#[test]
fn an_owner_the_kernel_does_not_know_is_refused_safely() {
    let f = fixture(x4());
    f.owners.withdraw(OWNER);
    let d = &f.device;
    assert_eq!(d.claim_save(OWNER, 0, "a.txt"), Err(ERR_STATE));
    assert!(!d.subscribed(OWNER, 99));
    assert_eq!(d.current_run(OWNER), None);
    assert_eq!(d.showing_screen(OWNER), None);
    assert_eq!(d.power_end(OWNER, false), None);
    d.clear_received(OWNER, None);
    d.clear_received(OWNER, Some(3));
    d.remove_received(OWNER, 3, 4);
    // Without a subscription nothing is admitted.
    assert!(request(&f, &fixtures::get(1)).is_err());
    assert!(f.shell.started.borrow().is_empty());
}

#[test]
fn received_bodies_are_cleared_under_the_plugin_folder() {
    let f = fixture(x4());
    for path in [
        ".paperpaw/plugins/mail/.received/3/4",
        ".paperpaw/plugins/mail/.received/3/5",
        ".paperpaw/plugins/mail/.received/9/1",
        ".paperpaw/plugins/mail/keep.txt",
    ] {
        f.card.write(path, b"x").unwrap();
    }
    f.device.remove_received(OWNER, 3, 4);
    assert!(f.card.size(".paperpaw/plugins/mail/.received/3/4").is_err());
    assert!(f.card.size(".paperpaw/plugins/mail/.received/3/5").is_ok());
    f.device.clear_received(OWNER, Some(3));
    assert!(f.card.size(".paperpaw/plugins/mail/.received/3/5").is_err());
    assert!(f.card.size(".paperpaw/plugins/mail/.received/9/1").is_ok());
    f.device.clear_received(OWNER, None);
    assert!(f.card.size(".paperpaw/plugins/mail/.received/9/1").is_err());
    assert!(f.card.size(".paperpaw/plugins/mail/keep.txt").is_ok());
}

#[test]
fn a_server_address_comes_through_a_buffer_and_a_refusal_is_a_reason() {
    let f = fixture(x4());
    let mut address = String::new();
    assert_eq!(f.device.listen(1, 8080, &mut address), Ok(()));
    assert_eq!(address, "192.168.1.2:80");
    for (reason, code) in [
        (ServerReason::NoNetwork, net_service::reason::NO_NETWORK),
        (ServerReason::Offline, net_service::reason::OFFLINE),
        (ServerReason::Saving, net_service::reason::SAVING),
        (ServerReason::NetworkOff, net_service::reason::NETWORK_OFF),
        (ServerReason::Refused, net_service::reason::REFUSED),
    ] {
        *f.shell.listen_with.borrow_mut() = Some(Err(reason));
        assert_eq!(f.device.listen(1, 8080, &mut String::new()), Err(code));
    }
    // An address longer than the event holds is the shell's refusal.
    *f.shell.listen_with.borrow_mut() = Some(Ok(
        "a-very-long-address-that-does-not-fit-in-sixty-four-bytes.example.org:65535",
    ));
    assert_eq!(
        f.device.listen(1, 8080, &mut String::new()),
        Err(net_service::reason::REFUSED)
    );
}

#[test]
fn reports_for_unknown_ids_are_ignored_and_the_staged_list_stays_empty() {
    let f = fixture(x4());
    assert_eq!(finish(&f, 99, HttpDone::new(200, HttpReason::Ok)), Ok(()));
    assert_eq!(finish(&f, 99, saved(200)), Ok(()));
    assert!(f.device.staged.borrow().is_empty());
    let _ = vec![ERR_IO];
}

#[test]
fn a_path_that_ends_inside_a_letter_is_not_cut_in_two() {
    let f = fixture(x4());
    let storage = caller(&f, 1);
    // The last ten bytes start inside the two-byte "é".
    for path in [
        "éaaaaaaaaa",
        "dir/éaaaaaaaaa",
        "aéaaaaaaaaa",
        "日本語日本語日本語",
    ] {
        assert_eq!(f.device.root_access(&storage, 1, path, true), Ok(()));
    }
    assert_eq!(
        f.device.root_access(&storage, 1, "é.PP-STAGED", true),
        Err(ERR_INVALID)
    );
}

#[test]
fn saves_and_received_files_are_cleaned_up_after_the_plugin_is_gone() {
    let f = fixture(x4());
    f.device.claim_save(OWNER, 0, "a.txt").unwrap();
    f.card
        .write(".paperpaw/plugins/mail/a.txt.pp-staged", b"x")
        .unwrap();
    f.card
        .write(".paperpaw/plugins/mail/.received/3/4", b"x")
        .unwrap();
    // The owner is out of the registry before the cleanup runs.
    f.owners.withdraw(OWNER);
    f.device.release_save(OWNER, 0, "a.txt");
    assert!(
        f.card
            .size(".paperpaw/plugins/mail/a.txt.pp-staged")
            .is_err()
    );
    assert!(
        !f.hosts
            .in_use()
            .holds(".paperpaw/plugins/mail/a.txt", false)
    );
    f.device.clear_received(OWNER, None);
    assert!(f.card.size(".paperpaw/plugins/mail/.received/3/4").is_err());
    // The id is let go with the last cleanup.
    assert_eq!(f.device.claim_save(OWNER, 0, "a.txt"), Err(ERR_STATE));
}

#[test]
fn debug_output_leaves_out_what_the_plugin_sent_and_got() {
    let done = HttpDone {
        body: b"secret-body".to_vec(),
        etag: "secret-etag",
        final_url: "https://x.test/?token=secret-url",
        ..HttpDone::new(200, HttpReason::Ok)
    };
    let text = std::format!("{done:?}");
    assert!(!text.contains("secret"), "{text}");
    assert!(text.contains("body_len"));
}
