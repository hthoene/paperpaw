#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The wiring of `core.sockets`: once per device, the one pool shared with
//! `core.net`, and the shell's plain reports becoming the service's.
extern crate std;
use super::*;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    event_tags::EventTags,
    host_services::{Caller, HostServices},
    registrations::{self as wire, Record},
    resources::HostResources,
    sock_service::{self, kind, reason},
};
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::MemoryStorage;

#[derive(Default)]
struct Shell {
    closes: RefCell<Vec<u32>>,
    ends: RefCell<Vec<u32>>,
}

impl SockTransport for Shell {
    fn connect(&self, _: u32, _: &Connect<'_>) -> Result<(), i32> {
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
    fn begin(&self, _: u32, _: &TlsOpen<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn end(&self, id: u32) {
        self.ends.borrow_mut().push(id);
    }
}
impl ShellNetwork for Shell {
    fn now_ms(&self) -> u64 {
        0
    }
    fn network_up(&self) -> bool {
        true
    }
    fn network_demand(&self, _: bool) {}
}

fn inputs(limits: SockLimits, tls: bool) -> (SockInputs, Rc<Shell>) {
    let shell = Rc::new(Shell::default());
    let mut inputs = SockInputs::new(
        Rc::new(MemoryStorage::new()),
        shell.clone(),
        shell.clone(),
        limits,
    );
    if tls {
        inputs = inputs.tls(shell.clone());
    }
    (inputs, shell)
}

#[test]
fn the_limits_are_the_shells_numbers() {
    assert_eq!(
        SockLimits::small(49_152),
        SockLimits {
            pool_bytes: 49_152,
            sockets: 2,
            queue: 4_096,
            read: 4_096,
            tls_sessions: 1
        }
    );
    let large = SockLimits::large(0);
    assert_eq!(
        (large.sockets, large.queue, large.read),
        (8, 16_384, 16_384)
    );
}

#[test]
fn a_shell_wires_the_sockets_once_and_nothing_is_half_wired() {
    let resources = HostResources::try_new().unwrap();
    let (first, _) = inputs(SockLimits::small(49_152), false);
    let wired = resources.wire(HostWiring::new().sockets(first)).unwrap();
    assert!(wired.sockets().is_some() && resources.hosts().sockets_wired());
    let (again, _) = inputs(SockLimits::small(49_152), false);
    let twice = resources.wire(
        HostWiring::new()
            .sockets(again)
            .hyphenation(Rc::new(MemoryStorage::new())),
    );
    assert_eq!(twice.err(), Some(ERR_STATE));
    assert!(resources.hosts().hyphen().is_none());
}

#[test]
fn net_and_sockets_charge_one_pool_whichever_is_wired_first() {
    for sockets_first in [false, true] {
        let resources = HostResources::try_new().unwrap();
        let shell = Rc::new(Shell::default());
        let net = || {
            struct Http;
            impl ShellHttp for Http {
                fn start(&self, _: &HttpJob<'_>) -> Result<(), i32> {
                    Ok(())
                }
                fn abort(&self, _: u32) {}
                fn listen(&self, _: u32, _: u32, _: &mut [u8]) -> Result<usize, ServerReason> {
                    Err(ServerReason::Refused)
                }
                fn unlisten(&self, _: u32) {}
                fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
                    Ok(())
                }
                fn close_served(&self, _: u32, _: u32) {}
            }
            NetInputs::new(
                Rc::new(MemoryStorage::new()),
                Rc::new(Http),
                shell.clone(),
                NetLimits::new(49_152, 2),
            )
        };
        let (socks, _) = inputs(SockLimits::small(49_152), false);
        let wiring = if sockets_first {
            HostWiring::new().sockets(socks).net(net())
        } else {
            HostWiring::new().net(net()).sockets(socks)
        };
        resources.wire(wiring).unwrap();
        let hosts = resources.hosts();
        let net_pool = hosts.net_for_call().unwrap().0.table().pool();
        let sock_pool = hosts.sockets_for_call().unwrap().0.table().pool();
        assert!(Rc::ptr_eq(net_pool, sock_pool), "one pool");
        assert_eq!(net_pool.capacity(), 49_152);
    }
}

struct NoHttp;
impl ShellHttp for NoHttp {
    fn start(&self, _: &HttpJob<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn abort(&self, _: u32) {}
    fn listen(&self, _: u32, _: u32, _: &mut [u8]) -> Result<usize, ServerReason> {
        Err(ServerReason::Refused)
    }
    fn unlisten(&self, _: u32) {}
    fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn close_served(&self, _: u32, _: u32) {}
}

fn net_inputs(shell: &Rc<Shell>, limits: NetLimits) -> NetInputs {
    NetInputs::new(
        Rc::new(MemoryStorage::new()),
        Rc::new(NoHttp),
        shell.clone(),
        limits,
    )
}

#[test]
fn net_and_sockets_naming_two_pools_are_refused_and_nothing_is_wired() {
    let resources = HostResources::try_new().unwrap();
    let shell = Rc::new(Shell::default());
    let (socks, _) = inputs(SockLimits::small(49_152), false);
    let two = HostWiring::new()
        .net(net_inputs(&shell, NetLimits::new(65_536, 2)))
        .sockets(socks);
    assert_eq!(resources.wire(two).err(), Some(ERR_INVALID));
    assert!(!resources.hosts().net_wired() && !resources.hosts().sockets_wired());
    // A pool made by an earlier wiring is not replaced by another shape.
    resources
        .wire(HostWiring::new().net(net_inputs(&shell, NetLimits::new(49_152, 2))))
        .unwrap();
    let (other, _) = inputs(SockLimits::small(65_536), false);
    assert_eq!(
        resources.wire(HostWiring::new().sockets(other)).err(),
        Some(ERR_INVALID)
    );
    assert!(!resources.hosts().sockets_wired());
    // The same shape joins the pool.
    let (same, _) = inputs(SockLimits::small(49_152), false);
    resources.wire(HostWiring::new().sockets(same)).unwrap();
}

/// A connected, open socket of plugin 7 `mail`.
fn open(resources: &HostResources, reports: &SockReports, tls: bool) -> u32 {
    let hosts = resources.hosts();
    // The kernel has filled the policy: nothing is off.
    hosts
        .policy()
        .update(0, false, false, [].into_iter(), [].into_iter());
    let owners = hosts.owners().unwrap();
    owners
        .admit(7, &paperpaw_sdk::Id::new_static("mail"))
        .unwrap();
    owners.subscribe(7, EventTags::collect(|_| true));
    let who = Caller {
        owner: 7,
        plugin: "mail",
        features: AdmittedFeatures::NONE,
        hosts,
    };
    let c = wire::ServiceSockConnect {
        token: 1,
        port: 993,
        flags: u32::from(tls),
        timeout_ms: 0,
        idle_ms: 0,
        host: "mail.example.org",
        server_name: "",
    };
    let mut out = std::vec![0; 4096];
    let used =
        wire::records_encode_counted(&[Record::ServiceSockConnect(c)], 20, 4096, &mut out).unwrap();
    let id =
        sock_service::admit(&*hosts.sockets_for_call().unwrap().0, &who, &out[..used]).unwrap();
    reports.connected(id).unwrap();
    id
}

fn events(resources: &HostResources) -> Vec<(u32, u32)> {
    let host = &*resources.hosts().sockets_for_call().unwrap().0;
    let mut seen = Vec::new();
    let mut out = std::vec![0; 256];
    while let Ok(Some(used)) = sock_service::next_event(host, 7, &mut out) {
        if let Record::EventSocket(e) =
            crate::native_host::codec::decode_any_single(&out[..used]).unwrap()
        {
            seen.push((e.kind, e.reason));
        }
        sock_service::event_delivered(host, 7).unwrap();
    }
    seen
}

#[test]
fn the_transports_failures_become_the_reasons_of_the_events() {
    for (why, want) in [
        (SockFailure::NoNetwork, reason::NO_NETWORK),
        (SockFailure::Dns, reason::DNS),
        (SockFailure::Refused, reason::REFUSED),
        (SockFailure::Timeout, reason::TIMEOUT),
        (SockFailure::Io, reason::IO),
        (SockFailure::Lost, reason::LOST),
        (SockFailure::NoMemory, reason::NO_MEMORY),
    ] {
        let resources = HostResources::try_new().unwrap();
        let (socks, _) = inputs(SockLimits::small(49_152), false);
        let wired = resources.wire(HostWiring::new().sockets(socks)).unwrap();
        let reports = wired.sockets().unwrap();
        let id = open(&resources, reports, false);
        events(&resources);
        reports.failed(id, why).unwrap();
        assert_eq!(events(&resources), [(kind::FAILED, want)], "{why:?}");
    }
}

#[test]
fn the_handshakes_outcomes_become_the_events_and_reasons() {
    let peer = PeerInfo::new([1; 32], [2; 32], true);
    for (how, want) in [
        (TlsFailure::Name, (kind::FAILED, reason::NAME)),
        (TlsFailure::Expired, (kind::FAILED, reason::EXPIRED)),
        (TlsFailure::NoMemory, (kind::FAILED, reason::NO_MEMORY)),
        (TlsFailure::Tls, (kind::FAILED, reason::TLS)),
    ] {
        let resources = HostResources::try_new().unwrap();
        let (socks, shell) = inputs(SockLimits::small(49_152), true);
        let wired = resources.wire(HostWiring::new().sockets(socks)).unwrap();
        let reports = wired.sockets().unwrap();
        let id = open(&resources, reports, true);
        reports.tls_failed(id, how).unwrap();
        assert_eq!(events(&resources), [want], "{how:?}");
        assert_eq!(*shell.ends.borrow(), [id], "the session ended first");
    }
    for (why, code) in [
        (UntrustedWhy::UnknownIssuer, 0),
        (UntrustedWhy::NotPinned, 1),
    ] {
        let resources = HostResources::try_new().unwrap();
        let (socks, _) = inputs(SockLimits::small(49_152), true);
        let wired = resources.wire(HostWiring::new().sockets(socks)).unwrap();
        let reports = wired.sockets().unwrap();
        let id = open(&resources, reports, true);
        reports.untrusted(id, why, &peer).unwrap();
        assert_eq!(events(&resources), [(kind::UNTRUSTED, code)], "{why:?}");
    }
    let resources = HostResources::try_new().unwrap();
    let (socks, _) = inputs(SockLimits::small(49_152), true);
    let wired = resources.wire(HostWiring::new().sockets(socks)).unwrap();
    let reports = wired.sockets().unwrap();
    let id = open(&resources, reports, true);
    reports.secured(id, &peer).unwrap();
    assert_eq!(events(&resources), [(kind::SECURED, 0)]);
}

#[test]
fn reports_for_unknown_sockets_change_nothing_and_are_counted() {
    let resources = HostResources::try_new().unwrap();
    let (socks, _) = inputs(SockLimits::small(49_152), true);
    let wired = resources.wire(HostWiring::new().sockets(socks)).unwrap();
    let reports = wired.sockets().unwrap();
    let peer = PeerInfo::new([0; 32], [0; 32], false);
    assert_eq!(reports.connected(99), Ok(()));
    assert_eq!(reports.received(99, b"x"), Err(ERR_NOT_FOUND));
    assert_eq!(reports.peer_closed(99), Ok(()));
    assert_eq!(reports.writable(99), Ok(()));
    assert_eq!(reports.failed(99, SockFailure::Io), Ok(()));
    assert_eq!(reports.answered(99, b"1.2.3.4:5", b"x"), Ok(()));
    assert_eq!(reports.secured(99, &peer), Ok(()));
    assert_eq!(
        reports.untrusted(99, UntrustedWhy::NotPinned, &peer),
        Ok(())
    );
    assert_eq!(reports.tls_failed(99, TlsFailure::Tls), Ok(()));
    assert_eq!(reports.tick(), Ok(()));
    assert_eq!(reports.network_lost(), Ok(()));
    assert_eq!(reports.take_ignored(), 3, "the three TLS reports");
    assert_eq!(reports.take_ignored(), 0);
    assert_eq!(reports.take_dropped(), 0);
}

#[test]
fn the_shell_without_tls_keeps_the_tls_flag_unsupported_through_the_wiring() {
    let resources = HostResources::try_new().unwrap();
    let (socks, _) = inputs(SockLimits::small(49_152), false);
    resources.wire(HostWiring::new().sockets(socks)).unwrap();
    let hosts = resources.hosts();
    assert!(hosts.sockets_for_call().unwrap().0.tls().is_none());
    let _: Option<&HostServices> = None;
}

/// Both network services wired on one pool, made by nobody yet.
fn wired_unmade(resources: &HostResources) -> WiredHosts {
    let shell = Rc::new(Shell::default());
    let (socks, _) = inputs(SockLimits::small(49_152), false);
    let wiring = HostWiring::new()
        .net(net_inputs(&shell, NetLimits::new(49_152, 1)))
        .sockets(socks);
    resources.wire(wiring).unwrap()
}

#[test]
fn the_network_services_are_made_at_their_first_call_and_one_without_room_tries_again() {
    use crate::native_host::shell::tests::FAIL_NTH;
    use paperpaw_native::host_glue::ERR_NO_MEMORY;
    let resources = HostResources::try_new().unwrap();
    let _wired = wired_unmade(&resources);
    let hosts = resources.hosts();
    // Wired, not made: a device no plugin asks for holds nothing (#248).
    assert!(hosts.net_wired() && hosts.net().is_none());
    assert!(hosts.sockets_wired() && hosts.sockets().is_none());
    // A first call without room for the device is refused; nothing is made.
    FAIL_NTH.with(|n| n.set(1));
    let net = hosts.net_for_call().err();
    FAIL_NTH.with(|n| n.set(1));
    let sockets = hosts.sockets_for_call().err();
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!((net, sockets), (Some(ERR_NO_MEMORY), Some(ERR_NO_MEMORY)));
    assert!(hosts.net().is_none() && hosts.sockets().is_none());
    // The next call makes each, once, on the one pool.
    assert!(hosts.net_for_call().is_ok() && hosts.sockets_for_call().is_ok());
    let net_pool = hosts.net().unwrap().0.table().pool();
    assert!(Rc::ptr_eq(
        net_pool,
        hosts.sockets().unwrap().0.table().pool()
    ));
}

#[test]
fn reports_before_the_first_call_answer_as_the_empty_device_does() {
    // One device made by a call, one not: the shell's reports for ids that
    // cannot exist answer the same on both.
    let (made, unmade) = (
        HostResources::try_new().unwrap(),
        HostResources::try_new().unwrap(),
    );
    let (a, b) = (wired_unmade(&made), wired_unmade(&unmade));
    made.hosts().net_for_call().unwrap();
    made.hosts().sockets_for_call().unwrap();
    assert!(unmade.hosts().net().is_none() && unmade.hosts().sockets().is_none());
    let net = |wired: &WiredHosts| {
        let r = wired.net().unwrap();
        (
            r.finished(9, HttpDone::new(200, HttpReason::Ok)),
            r.progress(9, 200, 1, Some(2)),
            r.redirected_to_https(9),
            r.tick(),
            r.network_lost(),
            r.server_started(9, "x"),
            r.server_failed(9, ServerReason::Refused),
            r.served_head(9, "GET", "/", None, false, 10),
            r.served_body(9, 1, 1),
            r.served_whole(9, 1),
            r.served_gone(9),
        )
    };
    assert_eq!(net(&a), net(&b));
    let peer = PeerInfo {
        certificate: [0; 32],
        key: [0; 32],
        time_checked: false,
    };
    let sockets = |wired: &WiredHosts| {
        let r = wired.sockets().unwrap();
        (
            (r.connected(9), r.received(9, b"x"), r.peer_closed(9)),
            (r.writable(9), r.failed(9, SockFailure::Io)),
            (r.answered(9, b"a", b"x"), r.secured(9, &peer)),
            r.untrusted(9, UntrustedWhy::NotPinned, &peer),
            r.tls_failed(9, TlsFailure::Tls),
            (
                r.tick(),
                r.network_lost(),
                r.take_dropped(),
                r.take_ignored(),
            ),
        )
    };
    assert_eq!(sockets(&a), sockets(&b));
}
