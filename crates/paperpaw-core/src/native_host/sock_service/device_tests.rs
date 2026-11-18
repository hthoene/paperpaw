#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The production `core.sockets` host over a fake shell and a memory card:
//! what core derives itself (roots and paths, the exclusive claim, the staged
//! file and its commit, the owner views, the power ends) and what the shell's
//! plain reports become.
extern crate std;
use super::super::{
    PeerInfo, TlsOpen, connected, event_delivered, failed, kind, next_event, peer_closed, received,
    table, tick,
    wire::{self, Record},
};
use super::*;
use crate::native_host::{
    admitted_features::AdmittedFeatures, event_tags::EventTags, host_services::HostServices,
    shell_wiring::SockLimits,
};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_UNSUPPORTED};
use paperpaw_native::metadata::Feature;
use paperpaw_platform::{DirEntry, StorageError};
use paperpaw_sdk::MemoryStorage;

const OWNER: u64 = 7;
const DATA: &str = ".paperpaw/plugins/mail/";
static WIRED: &[Feature<'static>] = &[Feature {
    key: "uses.storage",
    revision: 1,
}];

#[derive(Default)]
struct Shell {
    up: Cell<bool>,
    connects: RefCell<Vec<u32>>,
    closes: RefCell<Vec<u32>>,
    begins: RefCell<Vec<u32>>,
    tls_ends: RefCell<Vec<u32>>,
    demand: RefCell<Vec<bool>>,
}

impl SockTransport for Shell {
    fn connect(&self, id: u32, _: &super::super::Connect<'_>) -> Result<(), i32> {
        self.connects.borrow_mut().push(id);
        Ok(())
    }
    fn send(&self, _: u32, data: &[u8]) -> Result<usize, i32> {
        Ok(data.len())
    }
    fn read(&self, _: u32, _: u32) {}
    fn close(&self, id: u32) {
        self.closes.borrow_mut().push(id);
    }
    fn broadcast(&self, _: u32, _: &[u8], _: &[u16]) -> Result<(), i32> {
        Ok(())
    }
}

impl SockTls for Shell {
    fn begin(&self, id: u32, _: &TlsOpen<'_>) -> Result<(), i32> {
        self.begins.borrow_mut().push(id);
        Ok(())
    }
    fn end(&self, id: u32) {
        self.tls_ends.borrow_mut().push(id);
    }
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
}

/// A card whose rename can fail, taking the old target first.
struct Card {
    files: MemoryStorage,
    rename_takes_target_then_fails: Cell<bool>,
}

impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.files.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.files.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.files.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.files.remove(p)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if self.rename_takes_target_then_fails.get() {
            self.files.remove(to).ok();
            return Err(StorageError::Io);
        }
        self.files.rename(from, to)
    }
    fn ignores_case(&self) -> bool {
        true
    }
}

struct Fixture {
    card: Rc<Card>,
    shell: Rc<Shell>,
    device: SockDevice,
    hosts: HostServices,
}

fn fixture(tls: bool) -> Fixture {
    let card = Rc::new(Card {
        files: MemoryStorage::new(),
        rename_takes_target_then_fails: Cell::new(false),
    });
    let shell = Rc::new(Shell::default());
    shell.up.set(true);
    let hosts = HostServices::new();
    let owners = hosts.owners().unwrap().try_clone().unwrap();
    // The kernel has filled the policy: nothing is off.
    hosts
        .policy()
        .update(0, false, false, [].into_iter(), [].into_iter());
    let limits = SockLimits::small(49_152);
    let device = SockDevice::new(
        card.clone(),
        shell.clone(),
        tls.then(|| shell.clone() as Rc<dyn SockTls>),
        shell.clone(),
        SockProfile::new(limits.sockets, limits.queue, limits.read),
        hosts
            .net_pool(limits.pool_bytes, limits.tls_sessions)
            .unwrap()
            .clone(),
        hosts.in_use().clone(),
        owners.try_clone().unwrap(),
        hosts.policy().clone(),
    );
    owners
        .admit(OWNER, &paperpaw_sdk::Id::new_static("mail"))
        .unwrap();
    owners.subscribe(OWNER, EventTags::collect(|_| true));
    Fixture {
        card,
        shell,
        device,
        hosts,
    }
}

fn caller(f: &Fixture, bits: u32) -> Caller<'_> {
    Caller {
        owner: OWNER,
        plugin: "mail",
        features: AdmittedFeatures::test(bits, WIRED),
        hosts: &f.hosts,
    }
}

fn batch(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = std::vec![0; 4096];
    let used = wire::records_encode_counted(records, 20, 4096, &mut out).unwrap();
    out.truncate(used);
    out
}

fn connect(f: &Fixture, flags: u32) -> Result<u32, i32> {
    let c = wire::ServiceSockConnect {
        token: 9,
        port: 143,
        flags,
        timeout_ms: 0,
        idle_ms: 0,
        host: "mail.example.org",
        server_name: "",
    };
    table::connect(
        &f.device,
        &caller(f, 1),
        &batch(&[Record::ServiceSockConnect(c)]),
    )
}

fn open(f: &Fixture) -> u32 {
    let id = connect(f, 0).unwrap();
    connected(&f.device, id).unwrap();
    drain(f);
    id
}

fn drain(f: &Fixture) -> Vec<(u32, u32)> {
    let mut seen = Vec::new();
    let mut out = std::vec![0; 256];
    while let Ok(Some(used)) = next_event(&f.device, OWNER, &mut out) {
        if let Record::EventSocket(e) =
            crate::native_host::codec::decode_any_single(&out[..used]).unwrap()
        {
            seen.push((e.kind, e.reason));
        }
        event_delivered(&f.device, OWNER).unwrap();
    }
    seen
}

fn read_file(f: &Fixture, id: u32, len: u64, root: u32, path: &str) -> Result<(), i32> {
    table::read(
        &f.device,
        &caller(f, 1),
        &wire::ServiceSockRead {
            socket: id,
            mode: 1,
            flags: 0,
            max: 0,
            len,
            root,
            path,
        },
    )
}

fn card_file(f: &Fixture, name: &str) -> Option<Vec<u8>> {
    f.card.files.get(&std::format!("{DATA}{name}"))
}

#[test]
fn a_file_read_stages_under_the_one_staged_name_holds_both_and_commits_whole() {
    let f = fixture(false);
    f.card
        .files
        .write(&std::format!("{DATA}book.epub"), b"old")
        .unwrap();
    let id = open(&f);
    read_file(&f, id, 10, 0, "book.epub").unwrap();
    let target = std::format!("{DATA}book.epub");
    assert!(f.hosts.in_use().holds(&target, true));
    assert!(
        f.hosts
            .in_use()
            .holds(&std::format!("{target}.pp-staged"), true)
    );
    assert_eq!(received(&f.device, id, b"01234").unwrap(), 5);
    assert_eq!(card_file(&f, "book.epub.pp-staged").unwrap(), b"01234");
    assert_eq!(card_file(&f, "book.epub").unwrap(), b"old");
    assert_eq!(received(&f.device, id, b"56789").unwrap(), 5);
    assert_eq!(card_file(&f, "book.epub").unwrap(), b"0123456789");
    assert!(card_file(&f, "book.epub.pp-staged").is_none());
    assert!(!f.hosts.in_use().holds(&target, true), "the claim is gone");
    assert_eq!(drain(&f), [(kind::SAVED, 0)]);
}

#[test]
fn a_read_to_a_file_is_exclusive_against_every_other_writer_even_the_plugins_own() {
    let f = fixture(false);
    let id = open(&f);
    let target = std::format!("{DATA}a.bin");
    // The plugin's own NET save (a counted claim) is a writer too.
    f.hosts.in_use().claim(OWNER, &target).unwrap();
    assert_eq!(read_file(&f, id, 4, 0, "a.bin"), Err(ERR_BUSY));
    f.hosts.in_use().release(OWNER, &target);
    // Another spelling of the staged name, FAT-style.
    f.hosts
        .in_use()
        .claim(8, &std::format!("{DATA}A.BIN.PP-STAGED"))
        .unwrap();
    assert_eq!(read_file(&f, id, 4, 0, "a.bin"), Err(ERR_BUSY));
    f.hosts
        .in_use()
        .release(8, &std::format!("{DATA}A.BIN.PP-STAGED"));
    assert_eq!(read_file(&f, id, 4, 0, "a.bin"), Ok(()));
    // The plugin's own storage writes to the same file are busy meanwhile.
    let adapter = crate::native_host::storage::StorageAdapter::borrowed_folder(
        &*f.card,
        DATA,
        Some(&**f.hosts.in_use()),
    )
    .unwrap();
    assert_eq!(adapter.write(0, b"a.bin", b"x"), Err(ERR_BUSY));
    assert_eq!(
        adapter.rename(0, b"a.bin.pp-staged", b"a.bin"),
        Err(ERR_BUSY)
    );
    assert_eq!(adapter.remove(0, b"a.bin.pp-staged"), Err(ERR_BUSY));
}

#[test]
fn a_commit_that_fails_after_the_old_file_is_gone_leaves_the_staged_file_in_place() {
    let f = fixture(false);
    f.card
        .files
        .write(&std::format!("{DATA}book.epub"), b"old")
        .unwrap();
    let id = open(&f);
    read_file(&f, id, 4, 0, "book.epub").unwrap();
    f.card.rename_takes_target_then_fails.set(true);
    assert_eq!(received(&f.device, id, b"abcd").unwrap(), 4);
    // The socket failed with `io`; the only copy of the bytes is the staged
    // file, which nothing removed.
    assert_eq!(drain(&f), [(kind::FAILED, reason::IO)]);
    assert!(card_file(&f, "book.epub").is_none(), "the old file is gone");
    assert_eq!(card_file(&f, "book.epub.pp-staged").unwrap(), b"abcd");
    assert!(
        !f.hosts
            .in_use()
            .holds(&std::format!("{DATA}book.epub"), true),
        "the claim is gone"
    );
    // The next read to that path replaces it.
    f.card.rename_takes_target_then_fails.set(false);
    let id = open(&f);
    read_file(&f, id, 2, 0, "book.epub").unwrap();
    assert_eq!(received(&f.device, id, b"zz").unwrap(), 2);
    assert_eq!(card_file(&f, "book.epub").unwrap(), b"zz");
    assert!(card_file(&f, "book.epub.pp-staged").is_none());
}

#[test]
fn roots_and_paths_follow_the_rules_of_files() {
    let f = fixture(false);
    let id = open(&f);
    // Root 1 needs `uses.storage` (bit 1 of the fake's features).
    let r = |path: &str, root: u32, bits: u32| {
        table::read(
            &f.device,
            &caller(&f, bits),
            &wire::ServiceSockRead {
                socket: id,
                mode: 1,
                flags: 0,
                max: 0,
                len: 4,
                root,
                path,
            },
        )
    };
    assert_eq!(r("Books/x", 1, 0), Err(ERR_DENIED));
    assert_eq!(
        r("plugins/x", 1, 1),
        Err(ERR_INVALID),
        "the kernel's folders"
    );
    assert_eq!(r(".paperpaw/x", 1, 1), Err(ERR_INVALID));
    assert_eq!(r(".received/x", 0, 1), Err(ERR_DENIED), "the host's folder");
    assert_eq!(r("a.pp-staged", 0, 1), Err(ERR_INVALID));
    assert_eq!(r("../x", 0, 1), Err(ERR_INVALID));
    assert_eq!(r("x", 2, 1), Err(ERR_INVALID), "the bundle is read-only");
    assert!(!f.hosts.in_use().holds(DATA, true));
    assert_eq!(r("Books/x", 1, 1), Ok(()));
    assert!(f.hosts.in_use().holds("Books/x", true));
}

#[test]
fn without_tls_in_the_shell_the_tls_ops_are_unsupported_and_plain_sockets_work() {
    let f = fixture(false);
    assert_eq!(connect(&f, 1), Err(ERR_UNSUPPORTED));
    assert!(f.shell.connects.borrow().is_empty());
    let id = open(&f);
    let t = wire::ServiceSockTls {
        socket: id,
        timeout_ms: 0,
        idle_ms: 0,
        server_name: "",
    };
    assert_eq!(
        table::start_tls(
            &f.device,
            &caller(&f, 1),
            &batch(&[Record::ServiceSockTls(t)])
        ),
        Err(ERR_UNSUPPORTED)
    );
    assert!(f.device.tls().is_none());
}

#[test]
fn with_tls_in_the_shell_the_handshake_runs_through_it() {
    let f = fixture(true);
    let id = connect(&f, 1).unwrap();
    connected(&f.device, id).unwrap();
    assert_eq!(*f.shell.begins.borrow(), [id]);
    super::super::secured(&f.device, id, &PeerInfo::new([1; 32], [2; 32], true)).unwrap();
    assert_eq!(drain(&f), [(kind::SECURED, 0)]);
    table::close(&f.device, OWNER, id).unwrap();
    assert_eq!(*f.shell.tls_ends.borrow(), [id]);
    let small = SockLimits::small(49_152);
    assert_eq!(
        f.hosts
            .net_pool(small.pool_bytes, small.tls_sessions)
            .unwrap()
            .tls_holder(),
        None
    );
}

/// The user's decisions as the kernel hands them to the policy.
fn decide(f: &Fixture, version: u32, offline: bool, off: &[&str]) {
    f.hosts.policy().update(
        version,
        offline,
        offline,
        off.iter().copied(),
        [].into_iter(),
    );
}

#[test]
fn a_plugin_whose_network_is_off_gets_network_off_and_the_shell_hears_nothing() {
    let f = fixture(false);
    decide(&f, 1, false, &["mail"]);
    connect(&f, 0).unwrap();
    assert!(f.shell.connects.borrow().is_empty(), "nothing was started");
    assert_eq!(drain(&f), [(kind::FAILED, reason::NETWORK_OFF)]);
}

#[test]
fn the_offline_mode_ends_a_socket_before_the_shell_hears_of_it() {
    let f = fixture(false);
    decide(&f, 1, true, &[]);
    connect(&f, 0).unwrap();
    assert!(f.shell.connects.borrow().is_empty(), "nothing was started");
    assert_eq!(drain(&f), [(kind::FAILED, reason::OFFLINE)]);
}

#[test]
fn another_plugins_setting_does_not_end_this_ones_socket() {
    let f = fixture(false);
    decide(&f, 1, false, &["news"]);
    let id = open(&f);
    tick(&f.device).unwrap();
    assert_eq!(drain(&f), []);
    assert!(f.shell.closes.borrow().is_empty());
    table::close(&f.device, OWNER, id).unwrap();
}

#[test]
fn turning_the_network_off_mid_socket_ends_it_at_the_next_tick_and_closes_the_transport() {
    let f = fixture(false);
    let id = open(&f);
    decide(&f, 1, false, &["mail"]);
    tick(&f.device).unwrap();
    assert_eq!(drain(&f), [(kind::FAILED, reason::NETWORK_OFF)]);
    assert_eq!(*f.shell.closes.borrow(), [id]);
    // Turned on again: a new socket goes out.
    decide(&f, 2, false, &[]);
    let again = connect(&f, 0).unwrap();
    assert_eq!(*f.shell.connects.borrow().last().unwrap(), again);
}

#[test]
fn the_shell_hears_the_demand_of_sockets_once_per_change() {
    let f = fixture(false);
    let id = open(&f);
    table::close(&f.device, OWNER, id).unwrap();
    drain(&f);
    assert_eq!(*f.shell.demand.borrow(), [true, false]);
    let _ = (
        tick(&f.device),
        failed(&f.device, 99, reason::IO),
        peer_closed(&f.device, 99),
    );
}

#[test]
fn a_host_that_cannot_allocate_the_owner_name_refuses_the_read_and_holds_nothing() {
    use crate::native_host::net_service::fail_nth;
    for nth in 1..=6 {
        let f = fixture(false);
        let id = open(&f);
        fail_nth(nth);
        let asked = read_file(&f, id, 4, 0, "book.epub");
        fail_nth(0);
        if asked.is_err() {
            assert!(
                !f.hosts
                    .in_use()
                    .holds(&std::format!("{DATA}book.epub"), true),
                "gate {nth}"
            );
        }
        table::owner_stopped(&f.device, OWNER).unwrap();
        assert!(
            !f.hosts.in_use().holds(DATA, true),
            "gate {nth}: nothing claimed"
        );
        assert_eq!(f.device.table().pool().used(), 0, "gate {nth}");
    }
}

#[test]
fn a_plugin_that_is_off_or_offline_creates_no_demand_and_it_ends_with_the_last_socket() {
    for (offline, off) in [(false, &["mail"][..]), (true, &[][..])] {
        let f = fixture(false);
        decide(&f, 1, offline, off);
        connect(&f, 0).unwrap();
        assert!(!f.device.table().pool().wants(), "refused: no demand");
        assert!(f.shell.demand.borrow().is_empty(), "the shell hears none");
    }
    let f = fixture(false);
    let id = open(&f);
    assert!(f.device.table().pool().wants());
    table::close(&f.device, OWNER, id).unwrap();
    drain(&f);
    assert!(
        !f.device.table().pool().wants(),
        "idle after the last socket"
    );
}
