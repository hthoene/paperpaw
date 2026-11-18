#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! `core.sockets` over a fake device and a fake transport with failure
//! injection: admission, the limits, the pool, reads, sends, events, the ends
//! and an allocation-failure sweep.
extern crate std;
use super::table::close;
use super::*;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    host_services::HostServices,
    net_pool::{NetPool, TlsHolder},
    net_service::fail_nth,
};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};
use std::{
    collections::BTreeMap,
    string::{String, ToString},
};

/// A broadcast the transport was asked for: id, datagram, ports.
type CastCall = (u32, Vec<u8>, Vec<u16>);

const OWNER: u64 = 7;
const OTHER: u64 = 8;

struct Fake {
    table: SockTable,
    profile: Cell<SockProfile>,
    clock: Cell<u64>,
    network: Cell<bool>,
    power: Cell<Option<PowerEnd>>,
    subscribed: Cell<bool>,
    screen: Cell<Option<u64>>,
    run: Cell<Option<u64>>,
    /// The work run's resources: told open, told closed, refused to open.
    run_opened: RefCell<Vec<u64>>,
    run_closed: RefCell<Vec<u64>>,
    run_open_fails: Cell<Option<i32>>,
    connects: RefCell<Vec<(u32, String, u32)>>,
    connect_fails: Cell<Option<i32>>,
    cast_fails: Cell<Option<i32>>,
    casts: RefCell<Vec<CastCall>>,
    wire: RefCell<Vec<u8>>,
    /// How many more bytes the connection takes (`usize::MAX`: any).
    room: Cell<usize>,
    send_fails: Cell<bool>,
    reads: RefCell<Vec<(u32, u32)>>,
    closes: RefCell<Vec<u32>>,
    demand: RefCell<Vec<bool>>,
    /// The card: finished files and staged files by (root, path).
    files: RefCell<BTreeMap<(u32, String), Vec<u8>>>,
    parts: RefCell<BTreeMap<(u32, String), Vec<u8>>>,
    claims: RefCell<Vec<(u64, u32, String)>>,
    /// Paths another writer holds.
    in_use: RefCell<Vec<(u32, String)>>,
    deny: Cell<Option<i32>>,
    free: Cell<u64>,
    write_fails: Cell<Option<i32>>,
    commit_fails: Cell<Option<i32>>,
    /// The fake TLS backend: absent until a test turns it on.
    tls_on: Cell<bool>,
    begins: RefCell<Vec<(u32, String, Vec<TlsPin>)>>,
    tls_ends: RefCell<Vec<u32>>,
    begin_fails: Cell<Option<i32>>,
    /// What the lease and the shell's session looked like at each `end`.
    order: RefCell<Vec<&'static str>>,
}

impl Fake {
    fn new(profile: SockProfile, pool: u32) -> Self {
        Self::with_pool(profile, Rc::new(NetPool::new(pool)))
    }
    fn with_pool(profile: SockProfile, pool: Rc<NetPool>) -> Self {
        Self {
            table: SockTable::new(pool),
            profile: Cell::new(profile),
            clock: Cell::new(1_000),
            network: Cell::new(true),
            power: Cell::new(None),
            subscribed: Cell::new(true),
            screen: Cell::new(Some(5)),
            run: Cell::new(None),
            run_opened: RefCell::new(Vec::new()),
            run_closed: RefCell::new(Vec::new()),
            run_open_fails: Cell::new(None),
            connects: RefCell::new(Vec::new()),
            connect_fails: Cell::new(None),
            cast_fails: Cell::new(None),
            casts: RefCell::new(Vec::new()),
            wire: RefCell::new(Vec::new()),
            room: Cell::new(usize::MAX),
            send_fails: Cell::new(false),
            reads: RefCell::new(Vec::new()),
            closes: RefCell::new(Vec::new()),
            demand: RefCell::new(Vec::new()),
            files: RefCell::new(BTreeMap::new()),
            parts: RefCell::new(BTreeMap::new()),
            claims: RefCell::new(Vec::new()),
            in_use: RefCell::new(Vec::new()),
            deny: Cell::new(None),
            free: Cell::new(u64::MAX),
            write_fails: Cell::new(None),
            commit_fails: Cell::new(None),
            tls_on: Cell::new(false),
            begins: RefCell::new(Vec::new()),
            tls_ends: RefCell::new(Vec::new()),
            begin_fails: Cell::new(None),
            order: RefCell::new(Vec::new()),
        }
    }
    fn x4() -> Self {
        Self::new(SockProfile::X4, 49_152)
    }
    fn advance(&self, ms: u64) {
        self.clock.set(self.clock.get() + ms);
    }
    fn used(&self) -> u32 {
        self.table.pool().used()
    }
}

impl SockTransport for Fake {
    fn connect(&self, id: u32, to: &Connect<'_>) -> Result<(), i32> {
        if let Some(error) = self.connect_fails.get() {
            return Err(error);
        }
        self.connects
            .borrow_mut()
            .push((id, to.host.to_string(), to.port));
        Ok(())
    }
    fn send(&self, _: u32, data: &[u8]) -> Result<usize, i32> {
        if self.send_fails.get() {
            return Err(paperpaw_native::host_glue::ERR_IO);
        }
        let n = data.len().min(self.room.get());
        if self.room.get() != usize::MAX {
            self.room.set(self.room.get() - n);
        }
        self.wire.borrow_mut().extend_from_slice(&data[..n]);
        Ok(n)
    }
    fn read(&self, id: u32, max: u32) {
        self.reads.borrow_mut().push((id, max));
    }
    fn broadcast(&self, id: u32, data: &[u8], ports: &[u16]) -> Result<(), i32> {
        if let Some(error) = self.cast_fails.get() {
            return Err(error);
        }
        self.casts
            .borrow_mut()
            .push((id, data.to_vec(), ports.to_vec()));
        Ok(())
    }
    fn close(&self, id: u32) {
        self.closes.borrow_mut().push(id);
    }
}

impl SockTls for Fake {
    fn begin(&self, id: u32, to: &TlsOpen<'_>) -> Result<(), i32> {
        if let Some(error) = self.begin_fails.get() {
            return Err(error);
        }
        self.begins
            .borrow_mut()
            .push((id, to.server_name.to_string(), to.pins.to_vec()));
        Ok(())
    }
    fn end(&self, id: u32) {
        // The session goes before the lease does.
        let leased = self.table.pool().tls_holder() == Some(TlsHolder::Sock(id));
        self.order.borrow_mut().push(if leased {
            "end, lease held"
        } else {
            "end, lease free"
        });
        self.tls_ends.borrow_mut().push(id);
    }
}

impl SockHost for Fake {
    fn table(&self) -> &SockTable {
        &self.table
    }
    fn tls(&self) -> Option<&dyn SockTls> {
        self.tls_on.get().then_some(self)
    }
    fn transport(&self) -> &dyn SockTransport {
        self
    }
    fn profile(&self) -> SockProfile {
        self.profile.get()
    }
    fn now_ms(&self) -> u64 {
        self.clock.get()
    }
    fn subscribed(&self, _: u64, tag: u32) -> bool {
        (tag == wire::EVENT_SOCKET_TAG || tag == wire::EVENT_BROADCAST_TAG) && self.subscribed.get()
    }
    fn power_end(&self, _: u64, _: bool) -> Option<PowerEnd> {
        self.power.get()
    }
    fn network_up(&self) -> bool {
        self.network.get()
    }
    fn network_demand(&self, wanted: bool) {
        self.demand.borrow_mut().push(wanted);
    }
    fn current_run(&self, _: u64) -> Option<u64> {
        self.run.get()
    }
    fn resource_opened(&self, run: u64) -> Result<(), i32> {
        if let Some(error) = self.run_open_fails.get() {
            return Err(error);
        }
        self.run_opened.borrow_mut().push(run);
        Ok(())
    }
    fn resource_closed(&self, run: u64) {
        self.run_closed.borrow_mut().push(run);
    }
    fn root_access(&self, _: &Caller<'_>, _: u32, _: &str) -> Result<(), i32> {
        self.deny.get().map_or(Ok(()), Err)
    }
    fn room_for(&self, extra: u64) -> bool {
        extra <= self.free.get()
    }
    fn claim_save(&self, owner: u64, root: u32, path: &str) -> Result<(), i32> {
        if self.in_use.borrow().contains(&(root, path.to_string()))
            || self
                .claims
                .borrow()
                .iter()
                .any(|c| (c.1, &c.2) == (root, &path.to_string()))
        {
            return Err(ERR_BUSY);
        }
        self.claims
            .borrow_mut()
            .push((owner, root, path.to_string()));
        Ok(())
    }
    fn release_save(&self, owner: u64, root: u32, path: &str) {
        let mut claims = self.claims.borrow_mut();
        if let Some(at) = claims
            .iter()
            .position(|c| *c == (owner, root, path.to_string()))
        {
            claims.remove(at);
        }
        self.parts.borrow_mut().remove(&(root, path.to_string()));
    }
    fn staged_write(
        &self,
        _: u64,
        root: u32,
        path: &str,
        data: &[u8],
        first: bool,
    ) -> Result<(), i32> {
        if let Some(error) = self.write_fails.get() {
            return Err(error);
        }
        let mut parts = self.parts.borrow_mut();
        let part = parts.entry((root, path.to_string())).or_default();
        if first {
            part.clear();
        }
        part.extend_from_slice(data);
        self.free
            .set(self.free.get().saturating_sub(data.len() as u64));
        Ok(())
    }
    fn staged_commit(&self, _: u64, root: u32, path: &str) -> Result<(), i32> {
        if let Some(error) = self.commit_fails.get() {
            return Err(error);
        }
        let part = self
            .parts
            .borrow_mut()
            .remove(&(root, path.to_string()))
            .ok_or(ERR_NOT_FOUND)?;
        self.files
            .borrow_mut()
            .insert((root, path.to_string()), part);
        Ok(())
    }
    fn showing_screen(&self, _: u64) -> Option<u64> {
        self.screen.get()
    }
}

fn caller<'a>(owner: u64, hosts: &'a HostServices) -> Caller<'a> {
    Caller {
        owner,
        plugin: "mail",
        features: AdmittedFeatures::NONE,
        hosts,
    }
}

fn batch(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = std::vec![0; 4096];
    let used = wire::records_encode_counted(records, 20, 4096, &mut out).unwrap();
    out.truncate(used);
    out
}

fn head(token: u32) -> wire::ServiceSockConnect<'static> {
    wire::ServiceSockConnect {
        token,
        port: 143,
        flags: 0,
        timeout_ms: 0,
        idle_ms: 0,
        host: "mail.example.org",
        server_name: "",
    }
}

fn connect_with(f: &Fake, owner: u64, c: wire::ServiceSockConnect<'_>) -> Result<u32, i32> {
    let hosts = HostServices::new();
    table::connect(
        f,
        &caller(owner, &hosts),
        &batch(&[Record::ServiceSockConnect(c)]),
    )
}

fn connect(f: &Fake, owner: u64, token: u32) -> Result<u32, i32> {
    connect_with(f, owner, head(token))
}

/// Connected and told so: the plugin may send and read.
fn open(f: &Fake, owner: u64, token: u32) -> u32 {
    let id = connect(f, owner, token).unwrap();
    connected(f, id).unwrap();
    assert_eq!(
        next(f, owner),
        Some(Ev::new(id, token, kind::CONNECTED, 0, 0))
    );
    event_delivered(f, owner).unwrap();
    id
}

fn send_bytes(f: &Fake, owner: u64, id: u32, data: &[u8]) -> Result<(), i32> {
    table::send(f, owner, &wire::ServiceSockSend { socket: id, data })
}

fn read_bytes(f: &Fake, owner: u64, id: u32, max: u32, exact: bool) -> Result<(), i32> {
    let hosts = HostServices::new();
    table::read(
        f,
        &caller(owner, &hosts),
        &wire::ServiceSockRead {
            socket: id,
            mode: 0,
            flags: u32::from(exact),
            max,
            len: 0,
            root: 0,
            path: "",
        },
    )
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Ev {
    socket: u32,
    token: u32,
    kind: u32,
    reason: u32,
    len: u32,
}

impl Ev {
    fn new(socket: u32, token: u32, kind: u32, reason: u32, len: u32) -> Self {
        Self {
            socket,
            token,
            kind,
            reason,
            len,
        }
    }
}

fn next(f: &Fake, owner: u64) -> Option<Ev> {
    let mut out = std::vec![0; 256];
    let used = next_event(f, owner, &mut out).unwrap()?;
    // What the plugin's callback gets: the SDK frames it as a batch of one and
    // decodes it (the kernel's `service_event` reads its tag the same way).
    let incoming = paperpaw_sdk::native::events::Incoming::new(&out[..used]).unwrap();
    assert!(incoming.decode().is_ok(), "the plugin decodes the event");
    match crate::native_host::codec::decode_any_single(&out[..used]).unwrap() {
        Record::EventSocket(e) => Some(Ev::new(e.socket, e.token, e.kind, e.reason, e.len)),
        other => panic!("not event 106: {other:?}"),
    }
}

fn drain(f: &Fake, owner: u64) -> Vec<Ev> {
    let mut all = Vec::new();
    while let Some(e) = next(f, owner) {
        all.push(e);
        event_delivered(f, owner).unwrap();
    }
    all
}

fn data_of(f: &Fake, owner: u64, id: u32, offset: u32) -> Result<Vec<u8>, i32> {
    let mut out = std::vec![0; 4096];
    let used = table::data(
        f,
        owner,
        &wire::ServiceSockAt { socket: id, offset },
        &mut out,
    )?;
    match wire::record_decode_one(&out[..used]).unwrap() {
        Record::ServiceSockBytes(b) => Ok(b.data.to_vec()),
        other => panic!("not sock_bytes: {other:?}"),
    }
}

// ---- admission ----

#[test]
fn a_plain_connect_starts_the_transport_and_tells_the_plugin() {
    let f = Fake::x4();
    let id = connect(&f, OWNER, 9).unwrap();
    assert_ne!(id, 0);
    assert_eq!(
        &*f.connects.borrow(),
        &[(id, "mail.example.org".to_string(), 143)]
    );
    assert_eq!(
        next(&f, OWNER),
        None,
        "nothing before the transport says so"
    );
    connected(&f, id).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 9, kind::CONNECTED, 0, 0)]
    );
    assert_eq!(&*f.demand.borrow(), &[true]);
}

#[test]
fn bad_input_is_refused_before_anything_changes() {
    let f = Fake::x4();
    let bad = |c: wire::ServiceSockConnect<'_>| connect_with(&f, OWNER, c);
    for host in ["", "a..b", ".a", "a b", "[]", "[zz]", "a_b", "[::1"] {
        assert_eq!(
            bad(wire::ServiceSockConnect { host, ..head(1) }),
            Err(ERR_INVALID),
            "{host:?}"
        );
    }
    let long = "a".repeat(253);
    assert!(
        bad(wire::ServiceSockConnect {
            host: &long,
            ..head(1)
        })
        .is_ok()
    );
    for port in [0, 65_536] {
        assert_eq!(
            bad(wire::ServiceSockConnect { port, ..head(1) }),
            Err(ERR_INVALID)
        );
    }
    assert_eq!(
        bad(wire::ServiceSockConnect {
            flags: 2,
            ..head(1)
        }),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(wire::ServiceSockConnect {
            timeout_ms: 60_001,
            ..head(1)
        }),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(wire::ServiceSockConnect {
            idle_ms: 1_800_001,
            ..head(1)
        }),
        Err(ERR_INVALID)
    );
    for host in ["192.168.0.1", "[fe80::1]", "a-b.example"] {
        assert!(
            bad(wire::ServiceSockConnect { host, ..head(1) }).is_ok(),
            "{host}"
        );
        let id = f.table.held().1.unwrap();
        close(&f, OWNER, id).unwrap();
        drain(&f, OWNER);
    }
    // Pins without TLS, a repeated head, a pin first: all invalid.
    let hosts = HostServices::new();
    let pin = wire::ServiceSockPin {
        kind: 0,
        hash: &[0; 32],
    };
    for records in [
        std::vec![
            Record::ServiceSockConnect(head(1)),
            Record::ServiceSockPin(pin)
        ],
        std::vec![
            Record::ServiceSockPin(pin),
            Record::ServiceSockConnect(head(1))
        ],
        std::vec![
            Record::ServiceSockConnect(head(1)),
            Record::ServiceSockConnect(head(2))
        ],
    ] {
        assert_eq!(
            table::connect(&f, &caller(OWNER, &hosts), &batch(&records)),
            Err(ERR_INVALID)
        );
    }
    let short = wire::ServiceSockPin {
        kind: 0,
        hash: &[0; 31],
    };
    assert_eq!(
        table::connect(
            &f,
            &caller(OWNER, &hosts),
            &batch(&[
                Record::ServiceSockConnect(head(1)),
                Record::ServiceSockPin(short)
            ])
        ),
        Err(ERR_INVALID)
    );
    assert!(
        table::connect(&f, &caller(OWNER, &hosts), &[]).is_err(),
        "an empty batch"
    );
}

#[test]
fn tls_is_unsupported_not_invented() {
    let f = Fake::x4();
    assert_eq!(
        connect_with(
            &f,
            OWNER,
            wire::ServiceSockConnect {
                flags: 1,
                ..head(1)
            }
        ),
        Err(ERR_UNSUPPORTED)
    );
    assert_eq!(f.used(), 0);
    let id = open(&f, OWNER, 1);
    assert_eq!(
        read_bytes(&f, OWNER, id, 10, false),
        Ok(()),
        "the socket is unharmed"
    );
}

#[test]
fn a_plugin_that_did_not_subscribe_gets_no_socket() {
    let f = Fake::x4();
    f.subscribed.set(false);
    assert_eq!(connect(&f, OWNER, 1), Err(ERR_STATE));
    assert_eq!(f.used(), 0);
    assert!(f.connects.borrow().is_empty());
}

#[test]
fn limits_per_plugin_and_per_device_and_the_pool_hold() {
    let f = Fake::x4();
    let a = connect(&f, OWNER, 1).unwrap();
    let b = connect(&f, OWNER, 2).unwrap();
    assert_eq!(connect(&f, OWNER, 3), Err(ERR_BUSY), "two per plugin");
    assert_eq!(connect(&f, OTHER, 4), Err(ERR_BUSY), "two per X4");
    assert_eq!(f.used(), 2 * 8_192);
    close(&f, OWNER, a).unwrap();
    assert_eq!(f.used(), 8_192, "closing gives the buffers back at once");
    assert!(connect(&f, OTHER, 5).is_ok(), "the room is free again");
    let _ = b;
    // Other devices: eight at once, pool-less.
    let g = Fake::new(SockProfile::OTHER, 0);
    for owner in 1..=4 {
        connect(&g, owner, 0).unwrap();
        connect(&g, owner, 0).unwrap();
    }
    assert_eq!(connect(&g, 5, 0), Err(ERR_BUSY));
    assert_eq!(g.used(), 0, "a device without a pool counts nothing");
}

#[test]
fn a_server_name_without_tls_is_refused() {
    let f = Fake::x4();
    let c = wire::ServiceSockConnect {
        server_name: "other.example.org",
        ..head(1)
    };
    assert_eq!(connect_with(&f, OWNER, c), Err(ERR_INVALID));
    assert_eq!(f.used(), 0);
    assert!(f.connects.borrow().is_empty());
    // With `tls` it is the name to check (TLS itself is a later slice).
    let c = wire::ServiceSockConnect {
        flags: 1,
        server_name: "other.example.org",
        ..head(1)
    };
    assert_eq!(connect_with(&f, OWNER, c), Err(ERR_UNSUPPORTED));
}

#[test]
fn a_pool_full_of_net_bodies_refuses_a_socket_and_holds_nothing() {
    let pool = Rc::new(NetPool::new(49_152));
    assert!(pool.reserve(49_152 - 8_191));
    let f = Fake::with_pool(SockProfile::X4, pool.clone());
    assert_eq!(connect(&f, OWNER, 1), Err(ERR_NO_MEMORY));
    assert_eq!(pool.used(), 49_152 - 8_191, "nothing held");
    assert!(f.connects.borrow().is_empty());
    pool.release(1);
    assert!(connect(&f, OWNER, 1).is_ok());
    drop(f);
    assert_eq!(pool.used(), 49_152 - 8_192, "the table gave back its own");
}

#[path = "tests/cast.rs"]
mod cast;
#[path = "tests/deadline.rs"]
mod deadline;
#[path = "tests/file.rs"]
mod file;
#[path = "tests/pump_bytes.rs"]
mod pump_bytes;
#[path = "tests/tls.rs"]
mod tls;

// ---- sending ----

#[test]
fn a_send_queues_goes_out_and_is_told_once_it_has_drained() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 3);
    f.room.set(100);
    send_bytes(&f, OWNER, id, &[1; 250]).unwrap();
    assert_eq!(
        f.wire.borrow().len(),
        100,
        "the connection took what it could"
    );
    assert_eq!(next(&f, OWNER), None, "not drained yet");
    f.room.set(usize::MAX);
    writable(&f, id).unwrap();
    assert_eq!(f.wire.borrow().len(), 250);
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 3, kind::SENT, 0, 0)]
    );
    // Two sends, one `sent`.
    send_bytes(&f, OWNER, id, b"a").unwrap();
    send_bytes(&f, OWNER, id, b"b").unwrap();
    assert_eq!(drain(&f, OWNER).len(), 1, "coalesced");
}

#[test]
fn a_send_that_does_not_fit_queues_nothing() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 3);
    f.room.set(0);
    send_bytes(&f, OWNER, id, &[1; 3_840]).unwrap();
    assert_eq!(
        send_bytes(&f, OWNER, id, &[1; 300]),
        Err(ERR_BUSY),
        "4 KiB queue"
    );
    send_bytes(&f, OWNER, id, &[1; 256]).unwrap();
    assert_eq!(send_bytes(&f, OWNER, id, b"x"), Err(ERR_BUSY));
    assert_eq!(send_bytes(&f, OWNER, id, &[]), Err(ERR_INVALID));
    assert_eq!(send_bytes(&f, OWNER, id, &[0; 3_841]), Err(ERR_INVALID));
    f.room.set(usize::MAX);
    writable(&f, id).unwrap();
    assert_eq!(f.wire.borrow().len(), 3_840 + 256);
}

#[test]
fn send_needs_an_open_socket_of_the_caller() {
    let f = Fake::x4();
    let id = connect(&f, OWNER, 3).unwrap();
    assert_eq!(
        send_bytes(&f, OWNER, id, b"x"),
        Err(ERR_STATE),
        "still connecting"
    );
    assert_eq!(read_bytes(&f, OWNER, id, 5, false), Err(ERR_STATE));
    connected(&f, id).unwrap();
    assert_eq!(send_bytes(&f, OTHER, id, b"x"), Err(ERR_NOT_FOUND));
    assert_eq!(send_bytes(&f, OWNER, id + 99, b"x"), Err(ERR_NOT_FOUND));
    f.send_fails.set(true);
    send_bytes(&f, OWNER, id, b"x").unwrap();
    let all = drain(&f, OWNER);
    assert_eq!(
        all.last(),
        Some(&Ev::new(id, 3, kind::FAILED, reason::IO, 0))
    );
    assert_eq!(send_bytes(&f, OWNER, id, b"x"), Err(ERR_NOT_FOUND), "ended");
    assert_eq!(f.used(), 0);
}

// ---- reading ----

#[test]
fn a_read_waits_for_data_and_the_bytes_live_through_their_callback() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    read_bytes(&f, OWNER, id, 100, false).unwrap();
    assert_eq!(
        &*f.reads.borrow(),
        &[(id, 100)],
        "the transport may report 100"
    );
    assert_eq!(
        read_bytes(&f, OWNER, id, 5, false),
        Err(ERR_BUSY),
        "one at a time"
    );
    assert_eq!(received(&f, id, b"hello").unwrap(), 5);
    assert_eq!(next(&f, OWNER), Some(Ev::new(id, 4, kind::RECEIVED, 0, 5)));
    assert_eq!(data_of(&f, OWNER, id, 0).unwrap(), b"hello");
    assert_eq!(data_of(&f, OWNER, id, 2).unwrap(), b"llo");
    assert_eq!(data_of(&f, OWNER, id, 5).unwrap(), b"", "zero at the end");
    assert_eq!(data_of(&f, OWNER, id, 99).unwrap(), b"");
    assert_eq!(data_of(&f, OTHER, id, 0), Err(ERR_NOT_FOUND));
    // A read asked for inside the callback starts when the bytes are freed.
    read_bytes(&f, OWNER, id, 7, false).unwrap();
    assert_eq!(
        f.reads.borrow().len(),
        1,
        "not before the callback returned"
    );
    assert_eq!(
        received(&f, id, b"x"),
        Err(ERR_STATE),
        "nothing arrives unasked"
    );
    event_delivered(&f, OWNER).unwrap();
    assert_eq!(data_of(&f, OWNER, id, 0), Err(ERR_STATE), "freed");
    assert_eq!(f.reads.borrow()[1], (id, 7));
}

#[test]
fn exact_reads_wait_for_all_their_bytes_and_clamp_nothing_silently() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    assert_eq!(read_bytes(&f, OWNER, id, 4_097, true), Err(ERR_INVALID));
    assert_eq!(read_bytes(&f, OWNER, id, 0, false), Err(ERR_INVALID));
    assert_eq!(read_bytes(&f, OWNER, id, 16_385, false), Err(ERR_INVALID));
    read_bytes(&f, OWNER, id, 6, true).unwrap();
    assert_eq!(received(&f, id, b"abc").unwrap(), 3);
    assert_eq!(next(&f, OWNER), None, "three of six");
    assert_eq!(received(&f, id, b"defgh").unwrap(), 3, "no more than asked");
    assert_eq!(next(&f, OWNER), Some(Ev::new(id, 4, kind::RECEIVED, 0, 6)));
    assert_eq!(data_of(&f, OWNER, id, 0).unwrap(), b"abcdef");
    event_delivered(&f, OWNER).unwrap();
    // A larger plain read reads at most the device's read part.
    read_bytes(&f, OWNER, id, 16_384, false).unwrap();
    assert_eq!(f.reads.borrow().last(), Some(&(id, 4_096)));
}

#[test]
fn the_peer_closing_gives_an_empty_read_and_then_a_close() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    read_bytes(&f, OWNER, id, 10, false).unwrap();
    peer_closed(&f, id).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![
            Ev::new(id, 4, kind::RECEIVED, 0, 0),
            Ev::new(id, 4, kind::CLOSED, 0, 0)
        ]
    );
    assert_eq!(f.used(), 0);
    assert_eq!(f.closes.borrow().as_slice(), &[id]);
    // Closed while no read was asked: the next read tells it.
    let id = open(&f, OWNER, 5);
    peer_closed(&f, id).unwrap();
    assert_eq!(next(&f, OWNER), None);
    read_bytes(&f, OWNER, id, 10, false).unwrap();
    assert_eq!(drain(&f, OWNER).len(), 2);
    // An exact read is all or nothing: the peer closing before `max` bytes
    // ends the socket `failed` `closed_early`, with no `received` and the
    // partial bytes freed.
    let id = open(&f, OWNER, 6);
    read_bytes(&f, OWNER, id, 10, true).unwrap();
    received(&f, id, b"abcd").unwrap();
    peer_closed(&f, id).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 6, kind::FAILED, reason::CLOSED_EARLY, 0)]
    );
    assert_eq!(f.used(), 0);
    // A plain read keeps the empty read as the peer's close.
    let id = open(&f, OWNER, 7);
    read_bytes(&f, OWNER, id, 10, false).unwrap();
    peer_closed(&f, id).unwrap();
    assert_eq!(drain(&f, OWNER).len(), 2);
    // Closed while no read was asked: an exact read ends at once.
    let id = open(&f, OWNER, 8);
    peer_closed(&f, id).unwrap();
    read_bytes(&f, OWNER, id, 10, true).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 8, kind::FAILED, reason::CLOSED_EARLY, 0)]
    );
    assert_eq!(f.used(), 0);
}

#[test]
fn bytes_that_end_with_the_socket_stay_readable_until_told() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    read_bytes(&f, OWNER, id, 10, false).unwrap();
    received(&f, id, b"bye").unwrap();
    close(&f, OWNER, id).unwrap();
    assert_eq!(f.used(), 4_096, "the read part stays for the unread bytes");
    assert_eq!(next(&f, OWNER), Some(Ev::new(id, 4, kind::RECEIVED, 0, 3)));
    assert_eq!(data_of(&f, OWNER, id, 0).unwrap(), b"bye");
    event_delivered(&f, OWNER).unwrap();
    assert_eq!(next(&f, OWNER), Some(Ev::new(id, 4, kind::CLOSED, 0, 0)));
    event_delivered(&f, OWNER).unwrap();
    assert_eq!(f.used(), 0);
    assert_eq!(f.table.held().0, 0, "and the socket is gone");
}

// ---- ends ----

#[test]
fn close_is_idempotent_and_every_socket_ends_exactly_once() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    close(&f, OWNER, id).unwrap();
    close(&f, OWNER, id).unwrap();
    close(&f, OWNER, 999).unwrap();
    close(&f, OTHER, id).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 4, kind::CLOSED, 0, 0)]
    );
    close(&f, OWNER, id).unwrap();
    assert_eq!(next(&f, OWNER), None);
    assert_eq!(
        f.closes.borrow().as_slice(),
        &[id],
        "the transport closed once"
    );
    assert_eq!(f.demand.borrow().last(), Some(&false));
}

#[test]
fn times_power_and_the_network_end_sockets_with_their_reasons() {
    // Connect time.
    let f = Fake::x4();
    let id = connect_with(
        &f,
        OWNER,
        wire::ServiceSockConnect {
            timeout_ms: 5_000,
            ..head(1)
        },
    )
    .unwrap();
    f.advance(4_999);
    tick(&f).unwrap();
    assert_eq!(next(&f, OWNER), None);
    f.advance(1);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 1, kind::FAILED, reason::TIMEOUT, 0)]
    );
    // Idle time: bytes either way keep a socket alive.
    let id = connect_with(
        &f,
        OWNER,
        wire::ServiceSockConnect {
            idle_ms: 10_000,
            ..head(2)
        },
    )
    .unwrap();
    connected(&f, id).unwrap();
    drain(&f, OWNER);
    f.advance(9_000);
    send_bytes(&f, OWNER, id, b"x").unwrap();
    f.advance(9_000);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 2, kind::SENT, 0, 0)]
    );
    f.advance(1_000);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 2, kind::FAILED, reason::TIMEOUT, 0)]
    );
    // The default idle time is a minute, for every socket.
    let id = open(&f, OWNER, 3);
    f.advance(59_999);
    tick(&f).unwrap();
    assert_eq!(next(&f, OWNER), None);
    f.advance(1);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 3, kind::FAILED, reason::TIMEOUT, 0)]
    );
    // Power ends it before any byte goes out.
    f.power.set(Some(PowerEnd {
        reason: reason::SAVING,
        later: true,
    }));
    let id = connect(&f, OWNER, 4).unwrap();
    assert!(f.connects.borrow().iter().all(|(c, _, _)| *c != id));
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 4, kind::FAILED, reason::SAVING, 0)]
    );
    f.power.set(None);
    // No network: it waits up to 30 s, then fails.
    f.network.set(false);
    let id = connect(&f, OWNER, 5).unwrap();
    f.advance(29_999);
    tick(&f).unwrap();
    assert_eq!(next(&f, OWNER), None);
    f.network.set(true);
    tick(&f).unwrap();
    assert!(
        f.connects.borrow().iter().any(|(c, _, _)| *c == id),
        "it went on"
    );
    f.network.set(false);
    // The network going away ends every socket.
    network_lost(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 5, kind::FAILED, reason::NO_NETWORK, 0)]
    );
    let id = connect(&f, OWNER, 6).unwrap();
    f.advance(30_000);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 6, kind::FAILED, reason::NO_NETWORK, 0)]
    );
    assert_eq!(f.used(), 0);
}

#[test]
fn a_transport_that_cannot_connect_or_loses_the_socket_says_why() {
    let f = Fake::x4();
    f.connect_fails.set(Some(ERR_NO_MEMORY));
    let id = connect(&f, OWNER, 1).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 1, kind::FAILED, reason::NO_MEMORY, 0)]
    );
    f.connect_fails
        .set(Some(paperpaw_native::host_glue::ERR_IO));
    let id = connect(&f, OWNER, 2).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 2, kind::FAILED, reason::IO, 0)]
    );
    f.connect_fails.set(None);
    let id = open(&f, OWNER, 3);
    failed(&f, id, reason::LOST).unwrap();
    failed(&f, id, reason::REFUSED).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 3, kind::FAILED, reason::LOST, 0)]
    );
    assert!(f.closes.borrow().is_empty(), "the transport had dropped it");
    assert_eq!(f.used(), 0);
}

#[test]
fn a_screen_or_a_run_that_ends_takes_its_sockets_with_it() {
    let f = Fake::x4();
    let on_screen = open(&f, OWNER, 1);
    f.screen.set(None);
    f.run.set(Some(33));
    let in_run = open(&f, OWNER, 2);
    f.run.set(None);
    // Another screen closing changes nothing; the run of another plugin neither.
    screen_closed(&f, OWNER, 6).unwrap();
    work_ended(&f, OTHER, 33).unwrap();
    assert_eq!(next(&f, OWNER), None);
    screen_closed(&f, OWNER, 5).unwrap();
    work_ended(&f, OWNER, 33).unwrap();
    let mut all = drain(&f, OWNER);
    all.sort_by_key(|e| e.socket);
    assert_eq!(
        all,
        std::vec![
            Ev::new(on_screen, 1, kind::CLOSED, reason::SCREEN_CLOSED, 0),
            Ev::new(in_run, 2, kind::CLOSED, reason::WORK_ENDED, 0)
        ]
    );
    assert_eq!(f.used(), 0);
}

#[test]
fn a_plugin_that_stops_loses_its_sockets_without_events() {
    let f = Fake::new(
        SockProfile {
            sockets: 4,
            ..SockProfile::X4
        },
        49_152,
    );
    let a = open(&f, OWNER, 1);
    let b = connect(&f, OWNER, 2).unwrap();
    let kept = open(&f, OTHER, 3);
    assert_ne!(a, kept);
    read_bytes(&f, OWNER, a, 8, false).unwrap();
    received(&f, a, b"x").unwrap();
    close(&f, OWNER, b).unwrap();
    // A socket keeps its plugin from sleeping again (#248, D3).
    assert_eq!(owner_holds(&f, OWNER), Ok(true));
    owner_stopped(&f, OWNER).unwrap();
    owner_stopped(&f, OWNER).unwrap();
    assert_eq!(owner_holds(&f, OWNER), Ok(false));
    assert_eq!(owner_holds(&f, OTHER), Ok(true));
    assert_eq!(next(&f, OWNER), None);
    assert_eq!(f.used(), 8_192, "only the other plugin's socket is charged");
    assert!(f.closes.borrow().contains(&a));
    assert_eq!(send_bytes(&f, OTHER, kept, b"x"), Ok(()));
    owner_stopped(&f, OTHER).unwrap();
    assert_eq!(f.used(), 0);
    assert_eq!(f.demand.borrow().last(), Some(&false));
}

#[test]
fn ended_sockets_that_are_never_told_cannot_pile_up() {
    let f = Fake::x4();
    for token in 0..8 {
        let id = connect(&f, OWNER, token).unwrap();
        close(&f, OWNER, id).unwrap();
    }
    assert_eq!(
        connect(&f, OWNER, 9),
        Err(ERR_BUSY),
        "eight entries per plugin"
    );
    drain(&f, OWNER);
    assert!(connect(&f, OWNER, 9).is_ok());
}

#[test]
fn events_come_in_the_order_they_happened_across_sockets() {
    let f = Fake::x4();
    let a = connect(&f, OWNER, 1).unwrap();
    let b = connect(&f, OWNER, 2).unwrap();
    connected(&f, b).unwrap();
    connected(&f, a).unwrap();
    close(&f, OWNER, b).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![
            Ev::new(b, 2, kind::CONNECTED, 0, 0),
            Ev::new(a, 1, kind::CONNECTED, 0, 0),
            Ev::new(b, 2, kind::CLOSED, 0, 0)
        ]
    );
    let mut small = [0_u8; 4];
    assert_eq!(
        next_event(&f, OWNER, &mut small),
        Ok(None),
        "nothing to tell: nothing to fit"
    );
    close(&f, OWNER, a).unwrap();
    assert_eq!(
        next_event(&f, OWNER, &mut small),
        Err(paperpaw_native::host_glue::ERR_NO_SPACE)
    );
    assert!(
        next(&f, OWNER).is_some(),
        "a refused write consumed nothing"
    );
}

#[test]
fn the_shell_hears_the_demand_of_net_and_sockets_combined() {
    use crate::native_host::net_pool::DEMAND_NET;
    let pool = Rc::new(NetPool::new(49_152));
    let f = Fake::with_pool(SockProfile::X4, pool.clone());
    // NET is busy first: the socket adds no edge, and its end none either.
    assert_eq!(pool.demand(DEMAND_NET, true), Some(true));
    let id = open(&f, OWNER, 1);
    assert!(f.demand.borrow().is_empty());
    close(&f, OWNER, id).unwrap();
    drain(&f, OWNER);
    assert!(f.demand.borrow().is_empty());
    // A live socket outlasts NET going idle.
    let id = open(&f, OWNER, 2);
    assert_eq!(pool.demand(DEMAND_NET, false), None, "NET idle: no edge");
    close(&f, OWNER, id).unwrap();
    assert_eq!(&*f.demand.borrow(), &[false], "both idle: exactly one");
    drain(&f, OWNER);
    // Alone it is one edge each way.
    f.demand.borrow_mut().clear();
    let id = open(&f, OWNER, 3);
    close(&f, OWNER, id).unwrap();
    assert_eq!(&*f.demand.borrow(), &[true, false]);
}

// ---- memory ----

/// One scripted life of two sockets, every step allowed to fail.
fn scenario(f: &Fake) {
    let Ok(a) = connect(f, OWNER, 1) else { return };
    let _ = connected(f, a);
    let _ = send_bytes(f, OWNER, a, b"hello");
    let _ = read_bytes(f, OWNER, a, 16, false);
    let _ = received(f, a, b"world");
    let mut out = std::vec![0; 256];
    let _ = next_event(f, OWNER, &mut out);
    let _ = event_delivered(f, OWNER);
    if let Ok(b) = connect(f, OWNER, 2) {
        let _ = close(f, OWNER, b);
    }
    let _ = peer_closed(f, a);
    while let Ok(Some(_)) = next_event(f, OWNER, &mut out) {
        let _ = event_delivered(f, OWNER);
    }
}

#[test]
fn an_allocation_that_fails_anywhere_leaves_nothing_charged_and_nothing_half_made() {
    for nth in 1..64 {
        let f = Fake::x4();
        fail_nth(nth);
        scenario(&f);
        fail_nth(0);
        // Whatever happened, a clean stop gives every byte back.
        owner_stopped(&f, OWNER).unwrap();
        assert_eq!(f.used(), 0, "gate {nth}");
        assert!(f.table.held().0 == 0, "gate {nth}");
        // And the failure left no half-made socket behind: a fresh one works.
        assert!(connect(&f, OWNER, 9).is_ok(), "gate {nth}");
        drop(f);
    }
    // The first connect fails for memory at each of its gates, as ERR_NO_MEMORY.
    for nth in 1..=4 {
        let f = Fake::x4();
        fail_nth(nth);
        assert_eq!(connect(&f, OWNER, 1), Err(ERR_NO_MEMORY), "gate {nth}");
        fail_nth(0);
        assert_eq!(f.used(), 0);
        assert!(f.connects.borrow().is_empty());
        assert!(f.table.held().0 == 0);
    }
}

#[test]
fn the_pool_is_exactly_back_after_every_way_a_socket_can_end() {
    let pool = Rc::new(NetPool::new(49_152));
    assert!(pool.reserve(10_000), "someone else's bytes stay theirs");
    let f = Fake::with_pool(SockProfile::X4, pool.clone());
    for how in 0..5 {
        let id = open(&f, OWNER, 1);
        match how {
            0 => close(&f, OWNER, id).unwrap(),
            1 => failed(&f, id, reason::LOST).unwrap(),
            2 => network_lost(&f).unwrap(),
            3 => {
                read_bytes(&f, OWNER, id, 4, false).unwrap();
                peer_closed(&f, id).unwrap();
            }
            _ => owner_stopped(&f, OWNER).unwrap(),
        }
        drain(&f, OWNER);
        owner_stopped(&f, OWNER).unwrap();
        assert_eq!(pool.used(), 10_000, "way {how}");
    }
}

#[test]
fn turning_a_plugins_network_off_ends_each_socket_it_has_open_at_the_next_pass() {
    let f = Fake::x4();
    let open_one = open(&f, OWNER, 1);
    let connecting = connect(&f, OWNER, 2).unwrap();
    drain(&f, OWNER);
    // Offline and saving let what runs go on.
    f.power.set(Some(PowerEnd {
        reason: reason::OFFLINE,
        later: true,
    }));
    tick(&f).unwrap();
    assert_eq!(next(&f, OWNER), None);
    f.power.set(Some(PowerEnd {
        reason: reason::NETWORK_OFF,
        later: false,
    }));
    tick(&f).unwrap();
    let mut told = drain(&f, OWNER);
    told.sort_by_key(|e| e.token);
    assert_eq!(
        told,
        std::vec![
            Ev::new(open_one, 1, kind::FAILED, reason::NETWORK_OFF, 0),
            Ev::new(connecting, 2, kind::FAILED, reason::NETWORK_OFF, 0),
        ]
    );
    let mut closed = f.closes.borrow().clone();
    closed.sort_unstable();
    assert_eq!(
        closed,
        [open_one, connecting],
        "the transport let go of both"
    );
    // Nothing more is told.
    tick(&f).unwrap();
    assert_eq!(next(&f, OWNER), None);
}

/// Every socket of work run 7 that was stored told the run it opened, and
/// told it exactly once that it ended: after the plugin was told of the end
/// (the events are delivered here), or at once when no event is left to tell.
fn balanced(f: &Fake, opened: usize) {
    drain(f, OWNER);
    assert_eq!(*f.run_opened.borrow(), std::vec![7; opened], "opened");
    assert_eq!(
        *f.run_closed.borrow(),
        std::vec![7; opened],
        "each closed exactly once"
    );
}

#[test]
fn a_socket_of_a_work_run_keeps_the_run_until_it_ends_whichever_way_it_ends() {
    // Closed by the plugin; again changes nothing.
    let f = Fake::x4();
    f.run.set(Some(7));
    let id = open(&f, OWNER, 1);
    assert!(f.run_closed.borrow().is_empty(), "alive: the run goes on");
    close(&f, OWNER, id).unwrap();
    close(&f, OWNER, id).unwrap();
    // The run counts the socket until the plugin has been told of its end.
    assert!(f.run_closed.borrow().is_empty(), "ended, not yet told");
    assert!(next(&f, OWNER).is_some());
    assert!(f.run_closed.borrow().is_empty(), "told, callback running");
    event_delivered(&f, OWNER).unwrap();
    assert_eq!(*f.run_closed.borrow(), [7], "the callback returned");
    balanced(&f, 1);
    // The transport lost it.
    let f = Fake::x4();
    f.run.set(Some(7));
    let id = open(&f, OWNER, 1);
    failed(&f, id, reason::LOST).unwrap();
    failed(&f, id, reason::LOST).unwrap();
    balanced(&f, 1);
    // The user turned the plugin's network off while it ran.
    let f = Fake::x4();
    f.run.set(Some(7));
    open(&f, OWNER, 1);
    f.power.set(Some(PowerEnd {
        reason: reason::NETWORK_OFF,
        later: false,
    }));
    tick(&f).unwrap();
    tick(&f).unwrap();
    balanced(&f, 1);
    // The run reached its longest time.
    let f = Fake::x4();
    f.run.set(Some(7));
    open(&f, OWNER, 1);
    work_ended(&f, OWNER, 7).unwrap();
    work_ended(&f, OWNER, 7).unwrap();
    balanced(&f, 1);
    // The plugin stopped, with one open and one connecting.
    let f = Fake::x4();
    f.run.set(Some(7));
    open(&f, OWNER, 1);
    connect(&f, OWNER, 2).unwrap();
    owner_stopped(&f, OWNER).unwrap();
    owner_stopped(&f, OWNER).unwrap();
    balanced(&f, 2);
    // Ended by the power rules before it started.
    let f = Fake::x4();
    f.run.set(Some(7));
    f.power.set(Some(PowerEnd {
        reason: reason::SAVING,
        later: true,
    }));
    connect(&f, OWNER, 1).unwrap();
    balanced(&f, 1);
    // A transport that cannot start it, after it was stored.
    let f = Fake::x4();
    f.run.set(Some(7));
    f.connect_fails
        .set(Some(paperpaw_native::host_glue::ERR_IO));
    connect(&f, OWNER, 1).unwrap();
    balanced(&f, 1);
    // No run: nothing is told.
    let f = Fake::x4();
    let id = open(&f, OWNER, 1);
    close(&f, OWNER, id).unwrap();
    assert!(f.run_opened.borrow().is_empty() && f.run_closed.borrow().is_empty());
}

#[test]
fn a_run_that_cannot_be_kept_refuses_the_socket_and_nothing_is_held() {
    for error in [ERR_BUSY, paperpaw_native::host_glue::ERR_NOT_FOUND] {
        let f = Fake::x4();
        f.run.set(Some(7));
        f.run_open_fails.set(Some(error));
        assert_eq!(connect(&f, OWNER, 1), Err(error));
        assert!(f.connects.borrow().is_empty());
        assert_eq!(f.table.pool().used(), 0, "the charge went back");
        assert!(f.run_closed.borrow().is_empty(), "nothing was opened");
    }
}
