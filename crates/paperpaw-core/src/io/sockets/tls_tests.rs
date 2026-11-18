//! TLS on plugins' sockets: secure from the start, switched to (STARTTLS),
//! refused where it would not be secure, and what the shell reports.

#![allow(clippy::unwrap_used, reason = "tests")]

use super::*;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::socket_params as p;

const EVENTS: Id = Id::new_static("test.mail-events");
const SOCKET: Id = Id::new_static("test.imap");
const HOST: &str = "mail.example.org";

/// The value of `key` in the last event told.
fn last(out: &[Ask], key: &str) -> Option<String> {
    match out.last()? {
        Ask::Emit(_, params) => params.get(key).map(String::from),
        _ => None,
    }
}

fn kinds(out: &[Ask]) -> Vec<String> {
    out.iter()
        .filter_map(|c| match c {
            Ask::Emit(_, params) => params.get(p::KIND).map(String::from),
            _ => None,
        })
        .collect()
}

fn key() -> KeyHash {
    KeyHash::new([0x6b; 32])
}

/// What a shell reports once socket 1 is secure, its dates checked or not.
fn secured(time_checked: bool) -> SocketEvent {
    SocketEvent::Secured {
        socket: 1,
        certificate: certificate(),
        key: key(),
        time_checked,
    }
}

fn certificate() -> CertificateHash {
    CertificateHash::new([0x5a; 32])
}

/// A plain socket to `HOST`, connected, as socket 1.
fn plain(storage: &MemoryStorage) -> (Sockets, Vec<Ask>) {
    let mut sockets = Sockets::new(1);
    let mut out = Vec::new();
    let connect = Ask::connect(SOCKET, HOST, 587, EVENTS);
    sockets.command(connect, true, Owner::Screens(1), storage, &mut out);
    sockets.calls.clear();
    sockets.event(SocketEvent::Connected { socket: 1 }, storage, &mut out);
    (sockets, out)
}

#[test]
fn a_socket_secure_from_the_start_says_so_once_it_is() {
    let storage = MemoryStorage::new();
    let mut sockets = Sockets::new(1);
    let mut out = Vec::new();
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, Tls::new());
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    let tls = TlsSetup {
        server_name: String::from(HOST),
        trust: Trust::Authorities,
        timeout_ms: 20_000,
        idle_ms: 60_000,
    };
    assert_eq!(
        sockets.take(),
        [SocketCall::Connect {
            socket: 1,
            host: String::from(HOST),
            port: 993,
            tls: Some(tls),
        }]
    );
    // A shell that says it connected first is not heard: nothing is to be
    // done before the handshake.
    sockets.event(SocketEvent::Connected { socket: 1 }, &storage, &mut out);
    assert!(out.is_empty());
    sockets.event(secured(true), &storage, &mut out);
    assert_eq!(kinds(&out), [p::SECURED]);
    assert_eq!(last(&out, p::CERTIFICATE), Some("5a".repeat(32)));
    // Told once.
    sockets.event(secured(true), &storage, &mut out);
    assert_eq!(out.len(), 1);
    // Secure, it sends and reads.
    let login = Ask::socket_send(SOCKET, b"a1 LOGIN".to_vec());
    sockets.command(login, true, Owner::Screens(1), &storage, &mut out);
    let read = Ask::socket_read(SOCKET, SocketRead::Text { max: 64 });
    sockets.command(read, true, Owner::Screens(1), &storage, &mut out);
    assert!(matches!(
        sockets.take()[..],
        [
            SocketCall::Send { socket: 1, .. },
            SocketCall::Read { socket: 1, .. }
        ]
    ));
}

#[test]
fn nothing_goes_out_or_is_read_before_a_tls_connection_is_secure() {
    let storage = MemoryStorage::new();
    let login = || Ask::socket_send(SOCKET, b"a1 LOGIN user secret".to_vec());
    let read = || Ask::socket_read(SOCKET, SocketRead::Bytes { max: 64 });
    for early in [login(), read()] {
        let mut sockets = Sockets::new(1);
        let mut out = Vec::new();
        let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, Tls::new());
        sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
        sockets.calls.clear();
        sockets.command(early, true, Owner::Screens(1), &storage, &mut out);
        assert_eq!(last(&out, p::ERROR).as_deref(), Some("not-ready"));
        assert_eq!(sockets.take(), [SocketCall::Close { socket: 1 }]);
        assert!(!sockets.any());
    }
    // Nor after a switch was asked for, until it is secure.
    for early in [login(), read()] {
        let (mut sockets, mut out) = plain(&storage);
        let switch = Ask::start_tls(SOCKET, Tls::new());
        sockets.command(switch, true, Owner::Screens(1), &storage, &mut out);
        assert!(matches!(
            sockets.take()[..],
            [SocketCall::StartTls { socket: 1, .. }]
        ));
        sockets.command(early, true, Owner::Screens(1), &storage, &mut out);
        assert_eq!(last(&out, p::ERROR).as_deref(), Some("not-ready"));
        assert_eq!(sockets.take(), [SocketCall::Close { socket: 1 }]);
        assert!(!sockets.any());
    }
}

#[test]
fn a_certificate_whose_dates_could_not_be_checked_says_so() {
    let storage = MemoryStorage::new();
    let mut sockets = Sockets::new(1);
    let mut out = Vec::new();
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, Tls::new());
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    // A shell that does not know the time yet.
    sockets.event(secured(false), &storage, &mut out);
    assert_eq!(last(&out, p::TIME_CHECKED).as_deref(), Some("false"));
    let Some(Ask::Emit(_, params)) = out.last() else {
        panic!("no event");
    };
    assert_eq!(
        SocketNotice::from_params(params),
        Some(SocketNotice::Secured {
            socket: SOCKET,
            certificate: certificate(),
            key: key(),
            time_checked: false,
        })
    );
}

#[test]
fn bytes_are_read_as_they_came() {
    let storage = MemoryStorage::new();
    let (mut sockets, mut out) = plain(&storage);
    let read = Ask::socket_read(SOCKET, SocketRead::Bytes { max: 1 << 20 });
    sockets.command(read, true, Owner::Screens(1), &storage, &mut out);
    // As large as a text read may be.
    assert_eq!(
        sockets.take(),
        [SocketCall::Read {
            socket: 1,
            max: SOCKET_PART
        }]
    );
    // A header in Latin-1, no UTF-8.
    let header = b"Subject: Gr\xfc\xdfe\r\n".to_vec();
    let data = SocketEvent::Data {
        socket: 1,
        data: header.clone(),
    };
    sockets.event(data, &storage, &mut out);
    let Some(Ask::Emit(_, params)) = out.last() else {
        panic!("no event");
    };
    assert_eq!(
        SocketNotice::from_params(params),
        Some(SocketNotice::Bytes {
            socket: SOCKET,
            data: header,
        })
    );
}

#[test]
fn the_name_checked_and_pins_go_to_the_shell() {
    let storage = MemoryStorage::new();
    let mut sockets = Sockets::new(1);
    let mut out = Vec::new();
    let tls = Tls::pinned(alloc::vec![certificate()])
        .server_name("mail.lan")
        .timeout_ms(5_000)
        .idle_ms(0);
    let connect = Ask::connect_tls(SOCKET, "192.168.1.9", 993, EVENTS, tls);
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    let Some(SocketCall::Connect { tls: Some(tls), .. }) = sockets.take().pop() else {
        panic!("no connect");
    };
    assert_eq!(tls.server_name, "mail.lan");
    assert_eq!(tls.trust, Trust::Pinned(alloc::vec![certificate()]));
    assert_eq!((tls.timeout_ms, tls.idle_ms), (5_000, 0));
}

#[test]
fn tls_fails_at_once_where_the_shell_does_none_or_it_is_set_up_wrong() {
    let storage = MemoryStorage::new();
    let mut out = Vec::new();
    let mut none = Sockets::new(0);
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, Tls::new());
    none.command(connect, true, Owner::Screens(1), &storage, &mut out);
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("unsupported"));
    assert!(none.calls.is_empty() && !none.any());
    let mut sockets = Sockets::new(1);
    let wrong = Tls::pinned(Vec::new());
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, wrong);
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("options"));
    assert!(sockets.calls.is_empty() && !sockets.any());
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, Tls::new());
    sockets.command(connect, false, Owner::Screens(1), &storage, &mut out);
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("no-network"));
    assert_eq!(kinds(&out), [p::FAILED, p::FAILED, p::FAILED]);
}

#[test]
fn a_plain_socket_switches_to_tls_once_everything_come_is_read() {
    let storage = MemoryStorage::new();
    let (mut sockets, mut out) = plain(&storage);
    let read = Ask::socket_read(SOCKET, SocketRead::Text { max: 512 });
    sockets.command(read, true, Owner::Screens(1), &storage, &mut out);
    sockets.calls.clear();
    let go_ahead = b"220 2.0.0 Ready to start TLS\r\n".to_vec();
    let data = SocketEvent::Data {
        socket: 1,
        data: go_ahead,
    };
    sockets.event(data, &storage, &mut out);
    sockets.command(
        Ask::start_tls(SOCKET, Tls::new()),
        true,
        Owner::Screens(1),
        &storage,
        &mut out,
    );
    let Some(SocketCall::StartTls { socket: 1, tls }) = sockets.take().pop() else {
        panic!("no switch");
    };
    // The name checked is the host connected to.
    assert_eq!(tls.server_name, HOST);
    sockets.event(secured(true), &storage, &mut out);
    assert_eq!(kinds(&out), [p::CONNECTED, p::RECEIVED, p::SECURED]);
    // Secure once: a second switch fails and closes.
    sockets.command(
        Ask::start_tls(SOCKET, Tls::new()),
        true,
        Owner::Screens(1),
        &storage,
        &mut out,
    );
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("not-ready"));
    assert_eq!(sockets.take(), [SocketCall::Close { socket: 1 }]);
    assert!(!sockets.any());
}

#[test]
fn a_switch_while_reading_or_with_bytes_unread_fails_and_closes() {
    let storage = MemoryStorage::new();
    // While a read is under way: what it brings would pass as secure.
    let (mut sockets, mut out) = plain(&storage);
    let read = Ask::socket_read(SOCKET, SocketRead::Text { max: 512 });
    sockets.command(read, true, Owner::Screens(1), &storage, &mut out);
    sockets.calls.clear();
    sockets.command(
        Ask::start_tls(SOCKET, Tls::new()),
        true,
        Owner::Screens(1),
        &storage,
        &mut out,
    );
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("not-ready"));
    assert_eq!(sockets.take(), [SocketCall::Close { socket: 1 }]);
    assert!(!sockets.any());
    // With the start of a character come and not told.
    let (mut sockets, mut out) = plain(&storage);
    let read = Ask::socket_read(SOCKET, SocketRead::Text { max: 2 });
    sockets.command(read, true, Owner::Screens(1), &storage, &mut out);
    let data = SocketEvent::Data {
        socket: 1,
        data: b"o\xc3".to_vec(),
    };
    sockets.event(data, &storage, &mut out);
    sockets.calls.clear();
    sockets.command(
        Ask::start_tls(SOCKET, Tls::new()),
        true,
        Owner::Screens(1),
        &storage,
        &mut out,
    );
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("not-ready"));
    assert!(!sockets.any());
    // Before it connected.
    let mut sockets = Sockets::new(1);
    let connect = Ask::connect(SOCKET, HOST, 587, EVENTS);
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    sockets.command(
        Ask::start_tls(SOCKET, Tls::new()),
        true,
        Owner::Screens(1),
        &storage,
        &mut out,
    );
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("not-ready"));
    assert!(!sockets.any());
}

#[test]
fn a_switch_where_the_shell_does_no_tls_fails_and_closes() {
    let storage = MemoryStorage::new();
    let mut sockets = Sockets::new(0);
    let mut out = Vec::new();
    let connect = Ask::connect(SOCKET, HOST, 587, EVENTS);
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    sockets.calls.clear();
    sockets.event(SocketEvent::Connected { socket: 1 }, &storage, &mut out);
    sockets.command(
        Ask::start_tls(SOCKET, Tls::new()),
        true,
        Owner::Screens(1),
        &storage,
        &mut out,
    );
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("unsupported"));
    assert_eq!(sockets.take(), [SocketCall::Close { socket: 1 }]);
    assert!(!sockets.any());
}

#[test]
fn an_untrusted_certificate_is_told_and_the_socket_is_gone() {
    let storage = MemoryStorage::new();
    let mut sockets = Sockets::new(1);
    let mut out = Vec::new();
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, Tls::new());
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    let untrusted = SocketEvent::Untrusted {
        socket: 1,
        certificate: certificate(),
        key: key(),
        why: Distrust::UnknownIssuer,
    };
    sockets.event(untrusted, &storage, &mut out);
    assert_eq!(kinds(&out), [p::UNTRUSTED]);
    assert_eq!(last(&out, p::WHY).as_deref(), Some("unknown-issuer"));
    let Some(Ask::Emit(_, params)) = out.last() else {
        panic!("no event");
    };
    let notice = SocketNotice::from_params(params).unwrap();
    assert_eq!(
        notice,
        SocketNotice::Untrusted {
            socket: SOCKET,
            certificate: certificate(),
            key: key(),
            why: Distrust::UnknownIssuer,
        }
    );
    assert!(!sockets.any());
}

#[test]
fn why_the_shell_failed_reaches_the_plugin() {
    let storage = MemoryStorage::new();
    let mut sockets = Sockets::new(1);
    let mut out = Vec::new();
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, Tls::new());
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    let busy = SocketEvent::Failed {
        socket: 1,
        error: SocketError::Busy,
        reason: String::from("a secure connection is already open"),
    };
    sockets.event(busy, &storage, &mut out);
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("busy"));
    assert!(!sockets.any());
    // Calls that waited too long for a network.
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, Tls::new());
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    sockets.fail_waiting("no network", &storage, &mut out);
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("no-network"));
    assert!(!sockets.any());
}

#[test]
fn a_secure_socket_of_background_work_ends_when_idle() {
    let storage = MemoryStorage::new();
    let mut out = Vec::new();
    let mut sockets = Sockets::new(1);
    let never = || Tls::new().idle_ms(0);
    // The reader's may stay open for good.
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, never());
    sockets.command(connect, true, Owner::Screens(1), &storage, &mut out);
    assert!(sockets.any() && out.is_empty());
    // A run's, which no screen closes, is refused...
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, never());
    sockets.command(connect, true, Owner::Run(1), &storage, &mut out);
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("options"));
    assert!(!sockets.any());
    // ... one waiting half an hour for new mail (IMAP IDLE) is not.
    let idle = Tls::new().idle_ms(paperpaw_sdk::MAX_TLS_IDLE_MS);
    let connect = Ask::connect_tls(SOCKET, HOST, 993, EVENTS, idle);
    sockets.command(connect, true, Owner::Run(1), &storage, &mut out);
    assert!(sockets.any());
    assert_eq!(paperpaw_sdk::MAX_TLS_IDLE_MS, 30 * 60 * 1000);
    // Switched to TLS that never ends (STARTTLS): refused, and closed.
    let mut sockets = Sockets::new(1);
    let connect = Ask::connect(SOCKET, HOST, 587, EVENTS);
    sockets.command(connect, true, Owner::Run(1), &storage, &mut out);
    sockets.event(SocketEvent::Connected { socket: 1 }, &storage, &mut out);
    sockets.calls.clear();
    let switch = Ask::start_tls(SOCKET, never());
    sockets.command(switch, true, Owner::Run(1), &storage, &mut out);
    assert_eq!(last(&out, p::ERROR).as_deref(), Some("options"));
    assert_eq!(sockets.take(), [SocketCall::Close { socket: 1 }]);
}
