//! TLS: `connect` with `tls`, `start_tls`, `peer`, the session lease, the
//! shell's reports, every refusal and every way a secure socket can end, over
//! a fake TLS backend.
use super::*;

fn peer_info(n: u8) -> PeerInfo {
    PeerInfo {
        certificate: [n; 32],
        key: [n + 1; 32],
        time_checked: true,
    }
}

fn pin(kind: u32, n: u8) -> (u32, [u8; 32]) {
    (kind, [n; 32])
}

fn tls_connect_with(
    f: &Fake,
    owner: u64,
    c: wire::ServiceSockConnect<'_>,
    pins: &[(u32, [u8; 32])],
) -> Result<u32, i32> {
    let hosts = HostServices::new();
    let mut records = std::vec![Record::ServiceSockConnect(c)];
    for (kind, hash) in pins {
        records.push(Record::ServiceSockPin(wire::ServiceSockPin {
            kind: *kind,
            hash,
        }));
    }
    table::connect(f, &caller(owner, &hosts), &batch(&records))
}

fn tls_connect(f: &Fake, owner: u64, token: u32) -> Result<u32, i32> {
    let c = wire::ServiceSockConnect {
        flags: 1,
        port: 993,
        ..head(token)
    };
    tls_connect_with(f, owner, c, &[])
}

fn start_with(
    f: &Fake,
    owner: u64,
    t: wire::ServiceSockTls<'_>,
    pins: &[(u32, [u8; 32])],
) -> Result<(), i32> {
    let hosts = HostServices::new();
    let mut records = std::vec![Record::ServiceSockTls(t)];
    for (kind, hash) in pins {
        records.push(Record::ServiceSockPin(wire::ServiceSockPin {
            kind: *kind,
            hash,
        }));
    }
    table::start_tls(f, &caller(owner, &hosts), &batch(&records))
}

fn start(f: &Fake, owner: u64, socket: u32) -> Result<(), i32> {
    let t = wire::ServiceSockTls {
        socket,
        timeout_ms: 0,
        idle_ms: 0,
        server_name: "",
    };
    start_with(f, owner, t, &[])
}

#[derive(Debug, PartialEq, Eq)]
struct Peer {
    flags: u32,
    reason: u32,
    certificate: Vec<u8>,
    key: Vec<u8>,
}

fn peer_of(f: &Fake, owner: u64, socket: u32) -> Result<Peer, i32> {
    let mut out = std::vec![0; 256];
    let used = table::peer(f, owner, &wire::ServiceSockRef { socket }, &mut out)?;
    match wire::record_decode_one(&out[..used]).unwrap() {
        Record::ServiceSockPeer(p) => Ok(Peer {
            flags: p.flags,
            reason: p.reason,
            certificate: p.certificate.to_vec(),
            key: p.key.to_vec(),
        }),
        other => panic!("not sock_peer: {other:?}"),
    }
}

fn fake() -> Fake {
    let f = Fake::x4();
    f.tls_on.set(true);
    f
}

/// A TLS socket through its handshake, `secured` told and delivered.
fn secure(f: &Fake, owner: u64, token: u32) -> u32 {
    let id = tls_connect(f, owner, token).unwrap();
    connected(f, id).unwrap();
    secured(f, id, &peer_info(1)).unwrap();
    assert_eq!(
        next(f, owner),
        Some(Ev::new(id, token, kind::SECURED, 0, 0))
    );
    event_delivered(f, owner).unwrap();
    id
}

fn holder(f: &Fake) -> Option<TlsHolder> {
    f.table.pool().tls_holder()
}

#[test]
fn a_tls_connect_takes_the_session_handshakes_and_tells_secured_not_connected() {
    let f = fake();
    let pins = [pin(0, 7), pin(1, 8)];
    let c = wire::ServiceSockConnect {
        flags: 1,
        port: 993,
        server_name: "imap.example.org",
        ..head(5)
    };
    let id = tls_connect_with(&f, OWNER, c, &pins).unwrap();
    assert_eq!(holder(&f), Some(TlsHolder::Sock(id)), "the session first");
    assert_eq!(f.connects.borrow().len(), 1);
    assert!(f.begins.borrow().is_empty(), "no handshake before the TCP");
    connected(&f, id).unwrap();
    {
        let begins = f.begins.borrow();
        assert_eq!(begins.len(), 1);
        assert_eq!(begins[0].0, id);
        assert_eq!(begins[0].1, "imap.example.org");
        assert_eq!(
            begins[0].2,
            std::vec![
                TlsPin {
                    kind: 0,
                    hash: [7; 32]
                },
                TlsPin {
                    kind: 1,
                    hash: [8; 32]
                }
            ]
        );
    }
    assert_eq!(next(&f, OWNER), None, "`connected` is for plain sockets");
    // Switching: nothing flows yet.
    assert_eq!(send_bytes(&f, OWNER, id, b"x"), Err(ERR_STATE));
    assert_eq!(read_bytes(&f, OWNER, id, 4, false), Err(ERR_STATE));
    assert_eq!(peer_of(&f, OWNER, id), Err(ERR_STATE));
    secured(&f, id, &peer_info(3)).unwrap();
    assert_eq!(next(&f, OWNER), Some(Ev::new(id, 5, kind::SECURED, 0, 0)));
    // `peer` answers during the callback only, and only to its owner.
    assert_eq!(
        peer_of(&f, OWNER, id).unwrap(),
        Peer {
            flags: 1,
            reason: 0,
            certificate: std::vec![3; 32],
            key: std::vec![4; 32]
        }
    );
    assert_eq!(peer_of(&f, OTHER, id), Err(ERR_NOT_FOUND));
    event_delivered(&f, OWNER).unwrap();
    assert_eq!(peer_of(&f, OWNER, id), Err(ERR_STATE), "callback over");
    // Now bytes flow over the secure connection.
    send_bytes(&f, OWNER, id, b"a1 LOGIN").unwrap();
    assert_eq!(&*f.wire.borrow(), b"a1 LOGIN");
    assert_eq!(f.used(), 8_192, "the session is outside the pool");
}

#[test]
fn an_empty_server_name_checks_the_host_and_time_checked_clear_is_told() {
    let f = fake();
    let id = tls_connect(&f, OWNER, 1).unwrap();
    connected(&f, id).unwrap();
    assert_eq!(f.begins.borrow()[0].1, "mail.example.org");
    let late = PeerInfo {
        time_checked: false,
        ..peer_info(1)
    };
    secured(&f, id, &late).unwrap();
    next(&f, OWNER).unwrap();
    assert_eq!(peer_of(&f, OWNER, id).unwrap().flags, 0);
}

#[test]
fn start_tls_switches_an_open_plain_socket_and_waits_for_the_session() {
    let f = fake();
    let net = TlsHolder::Net(1);
    let id = open(&f, OWNER, 2);
    assert_eq!(
        f.table.pool().tls_request(net),
        crate::native_host::net_pool::TlsLease::Held
    );
    let t = wire::ServiceSockTls {
        socket: id,
        timeout_ms: 5_000,
        idle_ms: 30_000,
        server_name: "mail.other.org",
    };
    start_with(&f, OWNER, t, &[pin(0, 9)]).unwrap();
    assert!(f.begins.borrow().is_empty(), "NET holds the session");
    assert_eq!(send_bytes(&f, OWNER, id, b"x"), Err(ERR_STATE));
    assert_eq!(read_bytes(&f, OWNER, id, 4, false), Err(ERR_STATE));
    assert_eq!(start(&f, OWNER, id), Err(ERR_STATE), "not twice");
    // NET's request ends: the next tick hands the session on.
    f.advance(100);
    tick(&f).unwrap();
    assert!(f.begins.borrow().is_empty());
    f.table.pool().tls_drop(net);
    tick(&f).unwrap();
    assert_eq!(f.begins.borrow().len(), 1);
    assert_eq!(f.begins.borrow()[0].1, "mail.other.org");
    assert_eq!(holder(&f), Some(TlsHolder::Sock(id)));
    secured(&f, id, &peer_info(2)).unwrap();
    assert_eq!(next(&f, OWNER), Some(Ev::new(id, 2, kind::SECURED, 0, 0)));
    event_delivered(&f, OWNER).unwrap();
    send_bytes(&f, OWNER, id, b"a2 LOGIN").unwrap();
    assert_eq!(next(&f, OWNER), Some(Ev::new(id, 2, kind::SENT, 0, 0)));
    event_delivered(&f, OWNER).unwrap();
    // The new idle time (30 s) applies.
    f.advance(29_999);
    tick(&f).unwrap();
    assert_eq!(next(&f, OWNER), None);
    f.advance(1);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 2, kind::FAILED, reason::TIMEOUT, 0)]
    );
    assert_eq!(holder(&f), None);
}

#[test]
fn start_tls_refuses_what_could_hand_over_bytes_that_were_not_secure() {
    let f = Fake::new(SockProfile::OTHER, 0);
    f.tls_on.set(true);
    // Still connecting.
    let c = connect(&f, OWNER, 1).unwrap();
    assert_eq!(start(&f, OWNER, c), Err(ERR_STATE));
    connected(&f, c).unwrap();
    next(&f, OWNER).unwrap();
    event_delivered(&f, OWNER).unwrap();
    // A read under way.
    read_bytes(&f, OWNER, c, 4, false).unwrap();
    assert_eq!(start(&f, OWNER, c), Err(ERR_STATE));
    received(&f, c, b"* OK").unwrap();
    // Bytes the callback holds.
    assert_eq!(next(&f, OWNER).unwrap().kind, kind::RECEIVED);
    assert_eq!(start(&f, OWNER, c), Err(ERR_STATE));
    event_delivered(&f, OWNER).unwrap();
    // An unsent queue.
    f.room.set(0);
    send_bytes(&f, OWNER, c, b"STARTTLS").unwrap();
    assert_eq!(start(&f, OWNER, c), Err(ERR_STATE));
    f.room.set(usize::MAX);
    writable(&f, c).unwrap();
    drain(&f, OWNER);
    // A peer that has closed.
    let d = open(&f, OWNER, 2);
    peer_closed(&f, d).unwrap();
    assert_eq!(start(&f, OWNER, d), Err(ERR_STATE));
    // Not found: another plugin's, unknown.
    assert_eq!(start(&f, OTHER, c), Err(ERR_NOT_FOUND));
    assert_eq!(start(&f, OWNER, 999), Err(ERR_NOT_FOUND));
    assert!(f.begins.borrow().is_empty(), "nothing was switched");
    assert_eq!(holder(&f), None);
    // The socket is unharmed and can still switch.
    drain(&f, OWNER);
    assert_eq!(start(&f, OWNER, c), Ok(()));
}

#[test]
fn every_tls_refusal_changes_nothing() {
    let f = fake();
    let c = |flags, name| wire::ServiceSockConnect {
        flags,
        server_name: name,
        port: 993,
        ..head(1)
    };
    let h = |n| (0, [n; 32]);
    // Bad fields.
    assert_eq!(
        tls_connect_with(&f, OWNER, c(1, "bad name"), &[]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        tls_connect_with(&f, OWNER, c(1, "a..b"), &[]),
        Err(ERR_INVALID)
    );
    let five: Vec<_> = (0..5).map(h).collect();
    assert_eq!(
        tls_connect_with(&f, OWNER, c(1, ""), &five),
        Err(ERR_INVALID)
    );
    assert_eq!(
        tls_connect_with(&f, OWNER, c(1, ""), &[(2, [0; 32])]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        tls_connect_with(&f, OWNER, c(0, ""), &[h(1)]),
        Err(ERR_INVALID),
        "pins without tls"
    );
    assert_eq!(tls_connect_with(&f, OWNER, c(2, ""), &[]), Err(ERR_INVALID));
    let long = wire::ServiceSockConnect {
        timeout_ms: 60_001,
        ..c(1, "")
    };
    assert_eq!(tls_connect_with(&f, OWNER, long, &[]), Err(ERR_INVALID));
    assert_eq!(f.used(), 0);
    assert!(f.connects.borrow().is_empty());
    assert_eq!(holder(&f), None);
    // start_tls: bad fields, then the same on a good socket.
    let id = open(&f, OWNER, 3);
    let t = |timeout_ms, idle_ms, server_name| wire::ServiceSockTls {
        socket: id,
        timeout_ms,
        idle_ms,
        server_name,
    };
    assert_eq!(
        start_with(&f, OWNER, t(60_001, 0, ""), &[]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        start_with(&f, OWNER, t(0, 1_800_001, ""), &[]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        start_with(&f, OWNER, t(0, 0, "no good"), &[]),
        Err(ERR_INVALID)
    );
    assert_eq!(start_with(&f, OWNER, t(0, 0, ""), &five), Err(ERR_INVALID));
    assert_eq!(
        start_with(&f, OWNER, t(0, 0, ""), &[(9, [0; 32])]),
        Err(ERR_INVALID)
    );
    assert!(f.begins.borrow().is_empty());
    assert_eq!(
        peer_of(&f, OWNER, id),
        Err(ERR_STATE),
        "a plain socket has no peer"
    );
    assert_eq!(peer_of(&f, OWNER, 999), Err(ERR_NOT_FOUND));
    // The socket still works as it did.
    send_bytes(&f, OWNER, id, b"x").unwrap();
}

#[test]
fn a_shell_without_tls_leaves_tls_unsupported() {
    let f = Fake::x4();
    assert_eq!(tls_connect(&f, OWNER, 1), Err(ERR_UNSUPPORTED));
    assert_eq!(f.used(), 0);
    let id = open(&f, OWNER, 2);
    assert_eq!(start(&f, OWNER, id), Err(ERR_UNSUPPORTED));
    assert_eq!(peer_of(&f, OWNER, id), Err(ERR_UNSUPPORTED));
    send_bytes(&f, OWNER, id, b"x").unwrap();
}

#[test]
fn untrusted_ends_the_socket_and_peer_answers_in_that_callback() {
    for (code, want) in [(tls_code::UNKNOWN_ISSUER, 0), (tls_code::NOT_PINNED, 1)] {
        let f = fake();
        let id = tls_connect(&f, OWNER, 4).unwrap();
        connected(&f, id).unwrap();
        untrusted(&f, id, code, &peer_info(5)).unwrap();
        assert_eq!(
            next(&f, OWNER),
            Some(Ev::new(id, 4, kind::UNTRUSTED, want, 0))
        );
        let p = peer_of(&f, OWNER, id).unwrap();
        assert_eq!(
            (p.reason, p.certificate, p.key),
            (want, std::vec![5; 32], std::vec![6; 32])
        );
        event_delivered(&f, OWNER).unwrap();
        assert_eq!(peer_of(&f, OWNER, id), Err(ERR_NOT_FOUND), "gone");
        assert_eq!(f.tls_ends.borrow().as_slice(), &[id]);
        assert_eq!(f.closes.borrow().as_slice(), &[id], "the connection closed");
        assert_eq!(holder(&f), None);
        assert_eq!(f.used(), 0);
        assert_eq!(f.table.held().0, 0);
    }
    // A code the shell invented is never trusted: `failed` `tls`.
    let f = fake();
    let id = tls_connect(&f, OWNER, 4).unwrap();
    connected(&f, id).unwrap();
    untrusted(&f, id, 77, &peer_info(5)).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 4, kind::FAILED, reason::TLS, 0)]
    );
}

#[test]
fn a_failed_handshake_maps_to_its_reason_and_unknown_codes_are_tls() {
    for (code, want) in [
        (tls_code::NAME, reason::NAME),
        (tls_code::EXPIRED, reason::EXPIRED),
        (tls_code::TLS, reason::TLS),
        (tls_code::NO_MEMORY, reason::NO_MEMORY),
        (0, reason::TLS),
        (99, reason::TLS),
        (u32::MAX, reason::TLS),
    ] {
        let f = fake();
        let id = tls_connect(&f, OWNER, 6).unwrap();
        connected(&f, id).unwrap();
        tls_failed(&f, id, code).unwrap();
        assert_eq!(
            drain(&f, OWNER),
            std::vec![Ev::new(id, 6, kind::FAILED, want, 0)],
            "code {code}"
        );
        assert_eq!(f.tls_ends.borrow().as_slice(), &[id]);
        assert_eq!(holder(&f), None);
        assert_eq!(f.used(), 0);
    }
}

#[test]
fn a_begin_that_fails_ends_the_socket_and_gives_the_session_back() {
    for (error, want) in [
        (ERR_NO_MEMORY, reason::NO_MEMORY),
        (ERR_STATE, reason::TLS),
        (paperpaw_native::host_glue::ERR_IO, reason::TLS),
    ] {
        // On connect.
        let f = fake();
        f.begin_fails.set(Some(error));
        let id = tls_connect(&f, OWNER, 1).unwrap();
        connected(&f, id).unwrap();
        assert_eq!(
            drain(&f, OWNER),
            std::vec![Ev::new(id, 1, kind::FAILED, want, 0)]
        );
        assert_eq!(holder(&f), None, "{error}");
        assert_eq!(f.used(), 0);
        // On start_tls: the injected bytes the shell saw.
        let f = fake();
        let id = open(&f, OWNER, 2);
        f.begin_fails.set(Some(error));
        start(&f, OWNER, id).unwrap();
        assert_eq!(
            drain(&f, OWNER),
            std::vec![Ev::new(id, 2, kind::FAILED, want, 0)]
        );
        assert_eq!(holder(&f), None);
        assert!(f.tls_ends.borrow().is_empty(), "no session was begun");
        assert_eq!(f.closes.borrow().as_slice(), &[id]);
    }
}

#[test]
fn a_socket_waits_for_the_session_in_order_and_answers_busy_after_its_timeout() {
    let f = Fake::new(SockProfile::OTHER, 0);
    f.tls_on.set(true);
    let net = TlsHolder::Net(1);
    f.table.pool().tls_request(net);
    let c = |token, timeout_ms| wire::ServiceSockConnect {
        flags: 1,
        timeout_ms,
        ..head(token)
    };
    let a = tls_connect_with(&f, OWNER, c(1, 10_000), &[]).unwrap();
    let b = tls_connect_with(&f, OTHER, c(2, 30_000), &[]).unwrap();
    assert!(
        f.connects.borrow().is_empty(),
        "nothing is resolved without the session"
    );
    f.advance(9_999);
    tick(&f).unwrap();
    assert!(drain(&f, OWNER).is_empty());
    f.advance(1);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(a, 1, kind::FAILED, reason::BUSY, 0)]
    );
    // b was behind a; with a gone and NET done it is next, and its handshake
    // time counts from the grant.
    f.advance(5_000);
    f.table.pool().tls_drop(net);
    tick(&f).unwrap();
    assert_eq!(f.connects.borrow().len(), 1);
    assert_eq!(holder(&f), Some(TlsHolder::Sock(b)));
    f.advance(29_999);
    tick(&f).unwrap();
    assert!(drain(&f, OTHER).is_empty());
    f.advance(1);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OTHER),
        std::vec![Ev::new(b, 2, kind::FAILED, reason::TIMEOUT, 0)]
    );
    assert_eq!(holder(&f), None);
}

#[test]
fn net_and_sockets_are_served_first_come_first_served_one_holder_at_a_time() {
    let f = Fake::new(SockProfile::OTHER, 0);
    f.tls_on.set(true);
    let pool = f.table.pool().clone();
    use crate::native_host::net_pool::TlsLease;
    let (n1, n2) = (TlsHolder::Net(1), TlsHolder::Net(2));
    assert_eq!(pool.tls_request(n1), TlsLease::Held);
    let a = tls_connect(&f, OWNER, 1).unwrap();
    assert_eq!(pool.tls_request(n2), TlsLease::Wait);
    let b = tls_connect(&f, OTHER, 2).unwrap();
    // Order of asking: n1 (holds), a, n2, b.
    let order = |f: &Fake| (holder(f), f.connects.borrow().len());
    assert_eq!(order(&f), (Some(n1), 0));
    pool.tls_drop(n1);
    tick(&f).unwrap();
    assert_eq!(order(&f), (Some(TlsHolder::Sock(a)), 1), "a before n2");
    tick(&f).unwrap();
    assert_eq!(order(&f).1, 1, "b waits behind n2");
    close(&f, OWNER, a).unwrap();
    assert_eq!(pool.tls_request(n2), TlsLease::Held, "n2 before b");
    tick(&f).unwrap();
    assert_eq!(order(&f), (Some(n2), 1));
    pool.tls_drop(n2);
    tick(&f).unwrap();
    assert_eq!(order(&f), (Some(TlsHolder::Sock(b)), 2));
}

#[test]
fn a_secure_socket_that_idles_gives_the_session_up_only_when_someone_waits() {
    let f = Fake::new(SockProfile::OTHER, 0);
    f.tls_on.set(true);
    let a = secure(&f, OWNER, 1);
    f.advance(30_000);
    tick(&f).unwrap();
    assert!(drain(&f, OWNER).is_empty(), "nobody waits: it keeps it");
    let b = tls_connect(&f, OTHER, 2).unwrap();
    assert!(f.connects.borrow().len() == 1, "b waits");
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(a, 1, kind::FAILED, reason::SESSION_NEEDED, 0)]
    );
    assert_eq!(f.tls_ends.borrow().as_slice(), &[a]);
    tick(&f).unwrap();
    assert_eq!(holder(&f), Some(TlsHolder::Sock(b)));
    // A socket that moved within 5 s is not evicted.
    let c = secure_after(&f, b);
    let _ = c;
}

/// `b` gets through its handshake; another waits; `b` moves, so it stays.
fn secure_after(f: &Fake, b: u32) -> u32 {
    connected(f, b).unwrap();
    secured(f, b, &peer_info(2)).unwrap();
    drain(f, OTHER);
    let w = tls_connect(f, OWNER, 3).unwrap();
    f.advance(4_999);
    send_bytes(f, OTHER, b, b"x").unwrap();
    f.advance(4_999);
    tick(f).unwrap();
    let told = drain(f, OTHER);
    assert!(
        told.iter().all(|e| e.kind == kind::SENT),
        "it moved 4,999 ms ago: {told:?}"
    );
    assert_eq!(holder(f), Some(TlsHolder::Sock(b)));
    w
}

#[test]
fn whatever_ends_a_tls_socket_ends_the_session_before_the_lease_and_the_event() {
    for how in 0..9 {
        let f = fake();
        let pool = f.table.pool().clone();
        let id = if how % 2 == 0 {
            secure(&f, OWNER, 1)
        } else {
            let id = tls_connect(&f, OWNER, 1).unwrap();
            connected(&f, id).unwrap();
            id
        };
        match how {
            0 | 1 => close(&f, OWNER, id).unwrap(),
            2 | 3 => network_lost(&f).unwrap(),
            4 | 5 => screen_closed(&f, OWNER, 5).unwrap(),
            6 => failed(&f, id, reason::LOST).unwrap(),
            7 => {
                f.run.set(Some(9));
                work_ended(&f, OWNER, 9).unwrap();
                owner_stopped(&f, OWNER).unwrap();
            }
            _ => owner_stopped(&f, OWNER).unwrap(),
        }
        // Before the plugin hears anything, the session and the lease are gone.
        assert_eq!(f.tls_ends.borrow().as_slice(), &[id], "way {how}");
        assert_eq!(
            f.order.borrow().as_slice(),
            &["end, lease held"],
            "way {how}: the session ends first"
        );
        assert_eq!(pool.tls_holder(), None, "way {how}");
        drain(&f, OWNER);
        owner_stopped(&f, OWNER).unwrap();
        assert_eq!(pool.used(), 0, "way {how}");
        assert_eq!(f.table.held().0, 0, "way {how}");
        assert_eq!(f.tls_ends.borrow().len(), 1, "once, way {how}");
    }
}

#[test]
fn a_table_that_goes_with_a_waiting_socket_leaves_the_lease_free() {
    let f = fake();
    let pool = f.table.pool().clone();
    pool.tls_request(TlsHolder::Net(1));
    tls_connect(&f, OWNER, 1).unwrap();
    drop(f);
    pool.tls_drop(TlsHolder::Net(1));
    assert_eq!(pool.tls_holder(), None);
    assert!(!pool.tls_waiting(), "the dead socket left the queue");
}

#[test]
fn reports_for_unknown_ended_or_not_yet_handshaking_sockets_change_nothing() {
    let f = fake();
    let id = tls_connect(&f, OWNER, 1).unwrap();
    // Before the TCP is up there is no handshake.
    secured(&f, id, &peer_info(1)).unwrap();
    tls_failed(&f, id, tls_code::NAME).unwrap();
    untrusted(&f, id, 0, &peer_info(1)).unwrap();
    // Unknown.
    secured(&f, 999, &peer_info(1)).unwrap();
    assert_eq!(f.table.take_ignored(), 4);
    assert_eq!(f.table.take_ignored(), 0);
    assert_eq!(next(&f, OWNER), None);
    connected(&f, id).unwrap();
    close(&f, OWNER, id).unwrap();
    drain(&f, OWNER);
    // After it ended, and twice for one handshake.
    secured(&f, id, &peer_info(1)).unwrap();
    let id = tls_connect(&f, OWNER, 2).unwrap();
    connected(&f, id).unwrap();
    secured(&f, id, &peer_info(1)).unwrap();
    tls_failed(&f, id, tls_code::TLS).unwrap();
    secured(&f, id, &peer_info(9)).unwrap();
    assert_eq!(f.table.take_ignored(), 3);
    let events = drain(&f, OWNER);
    assert_eq!(
        events.len(),
        1,
        "only the first secured counted: {events:?}"
    );
    assert_eq!(events[0].kind, kind::SECURED);
}

#[test]
fn a_plain_socket_is_unaffected_by_the_tls_backend() {
    let f = fake();
    let id = open(&f, OWNER, 1);
    assert_eq!(holder(&f), None);
    close(&f, OWNER, id).unwrap();
    drain(&f, OWNER);
    assert!(f.tls_ends.borrow().is_empty());
    assert!(f.begins.borrow().is_empty());
}

fn tls_scenario(f: &Fake) {
    let Ok(a) = tls_connect(f, OWNER, 1) else {
        return;
    };
    let _ = connected(f, a);
    let _ = secured(f, a, &peer_info(1));
    let mut out = std::vec![0; 256];
    let _ = next_event(f, OWNER, &mut out);
    let _ = table::peer(f, OWNER, &wire::ServiceSockRef { socket: a }, &mut out);
    let _ = event_delivered(f, OWNER);
    let _ = send_bytes(f, OWNER, a, b"hello");
    if let Ok(b) = connect(f, OWNER, 2) {
        let _ = connected(f, b);
        let _ = start(f, OWNER, b);
        let _ = untrusted(f, b, 0, &peer_info(2));
    }
    while let Ok(Some(_)) = next_event(f, OWNER, &mut out) {
        let _ = event_delivered(f, OWNER);
    }
}

#[test]
fn an_allocation_that_fails_anywhere_in_tls_leaves_nothing_held() {
    for nth in 1..96 {
        let f = fake();
        fail_nth(nth);
        tls_scenario(&f);
        fail_nth(0);
        owner_stopped(&f, OWNER).unwrap();
        assert_eq!(f.used(), 0, "gate {nth}");
        assert_eq!(f.table.held().0, 0, "gate {nth}");
        assert_eq!(holder(&f), None, "gate {nth}");
        assert!(!f.table.pool().tls_waiting(), "gate {nth}");
        assert!(tls_connect(&f, OWNER, 9).is_ok(), "gate {nth}");
    }
    // The first TLS connect fails for memory at each of its gates.
    for nth in 1..=6 {
        let f = fake();
        fail_nth(nth);
        assert_eq!(tls_connect(&f, OWNER, 1), Err(ERR_NO_MEMORY), "gate {nth}");
        fail_nth(0);
        assert_eq!(f.used(), 0);
        assert_eq!(holder(&f), None);
        assert!(f.connects.borrow().is_empty());
    }
}

#[test]
fn a_start_tls_that_gets_no_session_in_time_ends_busy_and_closes_the_connection() {
    let f = fake();
    let id = open(&f, OWNER, 2);
    f.table.pool().tls_request(TlsHolder::Net(1));
    let t = wire::ServiceSockTls {
        socket: id,
        timeout_ms: 2_000,
        idle_ms: 0,
        server_name: "",
    };
    start_with(&f, OWNER, t, &[]).unwrap();
    f.advance(1_999);
    tick(&f).unwrap();
    assert!(drain(&f, OWNER).is_empty());
    f.advance(1);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 2, kind::FAILED, reason::BUSY, 0)]
    );
    assert_eq!(f.closes.borrow().as_slice(), &[id]);
    assert!(f.begins.borrow().is_empty());
    assert!(!f.table.pool().tls_waiting(), "it left the queue");
    assert_eq!(holder(&f), Some(TlsHolder::Net(1)), "NET's is untouched");
}

#[test]
fn the_wait_for_the_session_counts_from_the_end_of_the_wait_for_the_network() {
    let f = fake();
    f.table.pool().tls_request(TlsHolder::Net(1));
    f.network.set(false);
    let c = wire::ServiceSockConnect {
        flags: 1,
        timeout_ms: 10_000,
        ..head(1)
    };
    let id = tls_connect_with(&f, OWNER, c, &[]).unwrap();
    f.advance(20_000);
    tick(&f).unwrap();
    assert!(drain(&f, OWNER).is_empty(), "still waiting for the network");
    // The network is back; NET still holds the session. The clock starts now.
    f.network.set(true);
    tick(&f).unwrap();
    assert!(drain(&f, OWNER).is_empty(), "not busy at once");
    f.advance(9_999);
    tick(&f).unwrap();
    assert!(drain(&f, OWNER).is_empty());
    f.advance(1);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 1, kind::FAILED, reason::BUSY, 0)]
    );
}

#[test]
fn a_socket_that_cannot_connect_leaves_the_queue_for_the_session_and_asks_again() {
    let f = Fake::new(SockProfile::OTHER, 0);
    f.tls_on.set(true);
    let net = TlsHolder::Net(1);
    f.table.pool().tls_request(net);
    let a = tls_connect_with(
        &f,
        OWNER,
        wire::ServiceSockConnect {
            flags: 1,
            timeout_ms: 60_000,
            ..head(1)
        },
        &[],
    )
    .unwrap();
    assert!(f.table.pool().tls_waiting(), "it waits for the session");
    // The network goes: the socket cannot connect, so it does not stand at the
    // head of the queue while the session is free.
    f.network.set(false);
    tick(&f).unwrap();
    assert!(!f.table.pool().tls_waiting());
    f.table.pool().tls_drop(net);
    assert_eq!(
        f.table.pool().tls_request(TlsHolder::Net(2)),
        crate::native_host::net_pool::TlsLease::Held,
        "a free session is not held back by a socket that cannot use it"
    );
    f.table.pool().tls_drop(TlsHolder::Net(2));
    // The network is back: it asks again and gets the session.
    f.network.set(true);
    tick(&f).unwrap();
    assert_eq!(holder(&f), Some(TlsHolder::Sock(a)));
}
