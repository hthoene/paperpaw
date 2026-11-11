//! TLS for sockets against servers of the tests' own, with a certificate
//! authority of their own: implicit TLS and STARTTLS, authorities and
//! pins (certificates and keys), what a certificate that is not trusted
//! or not right becomes, the dates with a clock known and not, idleness
//! and time limits, and what the walk through a certificate reads.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use paperpaw_core::sdk::{CertificateHash, Distrust, KeyHash, SocketError, Trust};
use paperpaw_core::{SocketCall, SocketEvent, TlsSetup};
use rcgen::{CertificateParams, CertifiedIssuer, IsCa, KeyPair, PublicKeyData, date_time_ymd};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};

use super::verify::Leaf;
use super::{Gate, Refusal, Verifier, der, refusal};
use crate::Sockets;

/// A certificate authority and the roots that trust it.
struct Pki {
    ca: CertifiedIssuer<'static, KeyPair>,
    roots: Arc<WebPkiServerVerifier>,
}

fn pki() -> Pki {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    let ca = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
    let mut store = RootCertStore::empty();
    store.add(ca.der().clone()).unwrap();
    let roots =
        WebPkiServerVerifier::builder_with_provider(Arc::new(store), super::verify::provider())
            .build()
            .unwrap();
    Pki { ca, roots }
}

/// A server's certificate and its key.
struct Identity {
    der: CertificateDer<'static>,
    key: KeyPair,
}

impl Identity {
    fn sha256(&self) -> [u8; 32] {
        paperpaw_digest::sha256::digest(&self.der)
    }

    fn key_sha256(&self) -> [u8; 32] {
        paperpaw_digest::sha256::digest(&self.key.subject_public_key_info())
    }

    fn config(&self) -> Arc<ServerConfig> {
        let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(self.key.serialize_der()));
        let config = ServerConfig::builder_with_provider(super::verify::provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![self.der.clone()], key)
            .unwrap();
        Arc::new(config)
    }
}

/// A certificate of the authority for `name`, valid from 2020 to 2090.
fn issued(pki: &Pki, name: &str, key: KeyPair) -> Identity {
    dated(pki, name, key, (2020, 1, 1), (2090, 1, 1))
}

fn dated(pki: &Pki, name: &str, key: KeyPair, from: (i32, u8, u8), to: (i32, u8, u8)) -> Identity {
    let mut params = CertificateParams::new(vec![name.to_string()]).unwrap();
    params.not_before = date_time_ymd(from.0, from.1, from.2);
    params.not_after = date_time_ymd(to.0, to.1, to.2);
    let cert = params.signed_by(&key, &pki.ca).unwrap();
    Identity {
        der: cert.der().clone(),
        key,
    }
}

/// A certificate nobody signed but its own key.
fn self_signed(name: &str, key: KeyPair) -> Identity {
    self_signed_as(name, key, false)
}

/// The same, a certificate authority's (as `openssl req -x509` makes one,
/// and servers that make their own do) or not.
fn self_signed_as(name: &str, key: KeyPair, authority: bool) -> Identity {
    let mut params = CertificateParams::new(vec![name.to_string()]).unwrap();
    if authority {
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    }
    let cert = params.self_signed(&key).unwrap();
    Identity {
        der: cert.der().clone(),
        key,
    }
}

fn key() -> KeyPair {
    KeyPair::generate().unwrap()
}

/// Accepts one connection and runs `script` on it, secure from the start.
fn secure_server(
    identity: &Identity,
    script: impl FnOnce(&mut StreamOwned<ServerConnection, TcpStream>) + Send + 'static,
) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = identity.config();
    let server = thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut stream = StreamOwned::new(ServerConnection::new(config).unwrap(), tcp);
        script(&mut stream);
    });
    (port, server)
}

/// Says "hello" and answers what comes with "ok", once.
fn greets(stream: &mut StreamOwned<ServerConnection, TcpStream>) {
    if stream
        .write_all(b"hello")
        .and_then(|()| stream.flush())
        .is_ok()
    {
        let mut got = [0u8; 16];
        if let Ok(n) = stream.read(&mut got) {
            let _ = stream.write_all(&got[..n]);
            let _ = stream.flush();
        }
        // Until the client is done.
        let _ = stream.read(&mut [0u8; 16]);
    }
}

fn wait(sockets: &mut Sockets) -> SocketEvent {
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(event) = sockets.take_event() {
            return event;
        }
        assert!(Instant::now() < until, "no event");
        thread::sleep(Duration::from_millis(2));
    }
}

fn setup(name: &str, trust: Trust) -> TlsSetup {
    TlsSetup {
        server_name: name.to_string(),
        trust,
        timeout_ms: 5_000,
        idle_ms: 0,
    }
}

fn sockets_trusting(pki: &Pki) -> Sockets {
    let mut sockets = Sockets::new(|| {});
    sockets.tls.roots = Some(pki.roots.clone());
    sockets
}

fn connect(sockets: &mut Sockets, port: u16, tls: TlsSetup) {
    sockets.apply(SocketCall::Connect {
        socket: 1,
        host: String::from("127.0.0.1"),
        port,
        tls: Some(tls),
    });
}

fn hash(bytes: [u8; 32]) -> CertificateHash {
    CertificateHash::new(bytes)
}

fn key_hash(bytes: [u8; 32]) -> KeyHash {
    KeyHash::new(bytes)
}

/// Reads one `Data` and gives its bytes.
fn read(sockets: &mut Sockets) -> Vec<u8> {
    sockets.apply(SocketCall::Read { socket: 1, max: 64 });
    let SocketEvent::Data { data, .. } = wait(sockets) else {
        panic!("no data");
    };
    data
}

#[test]
fn a_server_of_an_authority_is_secured_and_talks() {
    let pki = pki();
    let identity = issued(&pki, "mail.test", key());
    let (port, server) = secure_server(&identity, greets);
    let mut sockets = sockets_trusting(&pki);
    connect(&mut sockets, port, setup("mail.test", Trust::Authorities));
    assert_eq!(
        wait(&mut sockets),
        SocketEvent::Secured {
            socket: 1,
            certificate: hash(identity.sha256()),
            key: key_hash(identity.key_sha256()),
            time_checked: true,
        }
    );
    assert_eq!(read(&mut sockets), b"hello");
    sockets.apply(SocketCall::Send {
        socket: 1,
        data: b"ping".to_vec(),
    });
    assert_eq!(read(&mut sockets), b"ping");
    sockets.apply(SocketCall::Close { socket: 1 });
    server.join().unwrap();
}

#[test]
fn a_server_no_shipped_authority_vouches_for_is_untrusted_and_its_hashes_told() {
    let pki = pki();
    let identity = issued(&pki, "mail.test", key());
    let (port, server) = secure_server(&identity, greets);
    // The authorities PaperPaw ships: not the tests' own.
    let mut sockets = Sockets::new(|| {});
    connect(&mut sockets, port, setup("mail.test", Trust::Authorities));
    assert_eq!(
        wait(&mut sockets),
        SocketEvent::Untrusted {
            socket: 1,
            certificate: hash(identity.sha256()),
            key: key_hash(identity.key_sha256()),
            why: Distrust::UnknownIssuer,
        }
    );
    // Shut already: no Close comes, and nothing is open.
    assert!(!sockets.any());
    server.join().unwrap();
}

#[test]
fn a_self_made_certificate_of_a_server_is_untrusted_even_as_an_authority_s() {
    for authority in [false, true] {
        let own = self_signed_as("mail.test", key(), authority);
        let (port, server) = secure_server(&own, greets);
        let mut sockets = Sockets::new(|| {});
        connect(&mut sockets, port, setup("mail.test", Trust::Authorities));
        assert_eq!(
            wait(&mut sockets),
            SocketEvent::Untrusted {
                socket: 1,
                certificate: hash(own.sha256()),
                key: key_hash(own.key_sha256()),
                why: Distrust::UnknownIssuer,
            },
            "authority {authority}"
        );
        server.join().unwrap();
    }
}

#[test]
fn a_certificate_for_another_name_fails_the_name() {
    let pki = pki();
    let identity = issued(&pki, "mail.test", key());
    let (port, server) = secure_server(&identity, greets);
    let mut sockets = sockets_trusting(&pki);
    connect(&mut sockets, port, setup("other.test", Trust::Authorities));
    let SocketEvent::Failed { error, .. } = wait(&mut sockets) else {
        panic!("not failed");
    };
    assert_eq!(error, SocketError::NameMismatch);
    assert!(!sockets.any());
    server.join().unwrap();
}

#[test]
fn an_expired_certificate_fails_with_a_clock_and_passes_unchecked_without() {
    let pki = pki();
    let old = dated(&pki, "mail.test", key(), (2020, 1, 1), (2021, 1, 1));
    let (port, server) = secure_server(&old, greets);
    let mut sockets = sockets_trusting(&pki);
    connect(&mut sockets, port, setup("mail.test", Trust::Authorities));
    let SocketEvent::Failed { error, .. } = wait(&mut sockets) else {
        panic!("not failed");
    };
    assert_eq!(error, SocketError::Expired);
    server.join().unwrap();

    // A computer that does not know the time: all else is checked, the
    // dates are not, and it is said.
    let (port, server) = secure_server(&old, greets);
    let mut sockets = sockets_trusting(&pki);
    sockets.tls = sockets.tls.clone().with_clock(|| None);
    connect(&mut sockets, port, setup("mail.test", Trust::Authorities));
    assert_eq!(
        wait(&mut sockets),
        SocketEvent::Secured {
            socket: 1,
            certificate: hash(old.sha256()),
            key: key_hash(old.key_sha256()),
            time_checked: false,
        }
    );
    sockets.apply(SocketCall::Close { socket: 1 });
    server.join().unwrap();

    // Not that the name or the chain are forgotten without a clock.
    let other = issued(&pki, "mail.test", key());
    let (port, server) = secure_server(&other, greets);
    let mut sockets = Sockets::new(|| {});
    sockets.tls = sockets.tls.clone().with_clock(|| None);
    connect(&mut sockets, port, setup("mail.test", Trust::Authorities));
    assert!(matches!(
        wait(&mut sockets),
        SocketEvent::Untrusted {
            why: Distrust::UnknownIssuer,
            ..
        }
    ));
    server.join().unwrap();
}

#[test]
fn a_pinned_certificate_is_trusted_without_chain_or_name_and_others_are_not() {
    let own = self_signed("whatever.test", key());
    let (port, server) = secure_server(&own, greets);
    let mut sockets = Sockets::new(|| {});
    let pins = Trust::Pinned(vec![hash([9; 32]), hash(own.sha256())]);
    connect(&mut sockets, port, setup("another-name.test", pins));
    assert_eq!(
        wait(&mut sockets),
        SocketEvent::Secured {
            socket: 1,
            certificate: hash(own.sha256()),
            key: key_hash(own.key_sha256()),
            time_checked: true,
        }
    );
    assert_eq!(read(&mut sockets), b"hello");
    sockets.apply(SocketCall::Close { socket: 1 });
    server.join().unwrap();

    // Another certificate: untrusted, and the hash is what a plugin pins.
    let (port, server) = secure_server(&own, greets);
    let mut sockets = Sockets::new(|| {});
    connect(
        &mut sockets,
        port,
        setup("x.test", Trust::Pinned(vec![hash([9; 32])])),
    );
    assert_eq!(
        wait(&mut sockets),
        SocketEvent::Untrusted {
            socket: 1,
            certificate: hash(own.sha256()),
            key: key_hash(own.key_sha256()),
            why: Distrust::NotPinned,
        }
    );
    server.join().unwrap();
}

#[test]
fn a_pinned_key_outlasts_a_renewed_certificate_and_another_key_is_not_pinned() {
    let kept = key();
    let kept_hash = paperpaw_digest::sha256::digest(&kept.subject_public_key_info());
    let renewed = self_signed("a.test", kept);
    let (port, server) = secure_server(&renewed, greets);
    let mut sockets = Sockets::new(|| {});
    let trust = Trust::PinnedKeys(vec![key_hash(kept_hash)]);
    connect(&mut sockets, port, setup("a.test", trust.clone()));
    assert!(matches!(
        wait(&mut sockets),
        SocketEvent::Secured {
            time_checked: true,
            ..
        }
    ));
    sockets.apply(SocketCall::Close { socket: 1 });
    server.join().unwrap();

    let stranger = self_signed("a.test", key());
    let (port, server) = secure_server(&stranger, greets);
    let mut sockets = Sockets::new(|| {});
    connect(&mut sockets, port, setup("a.test", trust));
    assert_eq!(
        wait(&mut sockets),
        SocketEvent::Untrusted {
            socket: 1,
            certificate: hash(stranger.sha256()),
            key: key_hash(stranger.key_sha256()),
            why: Distrust::NotPinned,
        }
    );
    server.join().unwrap();
}

#[test]
fn a_pinned_certificate_is_checked_for_its_dates_when_the_time_is_known() {
    let pki = pki();
    let old = dated(&pki, "a.test", key(), (2020, 1, 1), (2021, 1, 1));
    let pins = Trust::Pinned(vec![hash(old.sha256())]);
    let (port, server) = secure_server(&old, greets);
    let mut sockets = Sockets::new(|| {});
    connect(&mut sockets, port, setup("a.test", pins.clone()));
    let SocketEvent::Failed { error, .. } = wait(&mut sockets) else {
        panic!("not failed");
    };
    assert_eq!(error, SocketError::Expired);
    server.join().unwrap();

    let (port, server) = secure_server(&old, greets);
    let mut sockets = Sockets::new(|| {});
    sockets.tls = sockets.tls.clone().with_clock(|| None);
    connect(&mut sockets, port, setup("a.test", pins));
    assert!(matches!(
        wait(&mut sockets),
        SocketEvent::Secured {
            time_checked: false,
            ..
        }
    ));
    sockets.apply(SocketCall::Close { socket: 1 });
    server.join().unwrap();
}

#[test]
fn a_server_that_speaks_no_tls_fails_the_handshake() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        let _ = peer.write_all(b"220 hello, I speak plain text\r\n");
        let _ = peer.read(&mut [0u8; 512]);
    });
    let mut sockets = Sockets::new(|| {});
    connect(&mut sockets, port, setup("x.test", Trust::Authorities));
    let SocketEvent::Failed { error, .. } = wait(&mut sockets) else {
        panic!("not failed");
    };
    assert_eq!(error, SocketError::Tls);
    assert!(!sockets.any());
    server.join().unwrap();
}

#[test]
fn a_handshake_that_never_ends_times_out() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        // Says nothing until the client leaves.
        let _ = peer.read(&mut [0u8; 4096]);
        let _ = peer.read(&mut [0u8; 4096]);
    });
    let mut sockets = Sockets::new(|| {});
    let mut tls = setup("x.test", Trust::Authorities);
    tls.timeout_ms = 300;
    connect(&mut sockets, port, tls);
    let SocketEvent::Failed { error, .. } = wait(&mut sockets) else {
        panic!("not failed");
    };
    assert_eq!(error, SocketError::Timeout);
    server.join().unwrap();
}

#[test]
fn a_secure_socket_idle_for_its_limit_ends_with_a_timeout() {
    let pki = pki();
    let identity = issued(&pki, "mail.test", key());
    let (port, server) = secure_server(&identity, |stream| {
        let _ = stream.read(&mut [0u8; 16]);
    });
    let mut sockets = sockets_trusting(&pki);
    let mut tls = setup("mail.test", Trust::Authorities);
    tls.idle_ms = 200;
    connect(&mut sockets, port, tls);
    assert!(matches!(wait(&mut sockets), SocketEvent::Secured { .. }));
    let SocketEvent::Failed { error, reason, .. } = wait(&mut sockets) else {
        panic!("not failed");
    };
    assert_eq!((error, reason.as_str()), (SocketError::Timeout, "idle"));
    assert!(!sockets.any());
    server.join().unwrap();
}

#[test]
fn starttls_switches_a_plain_socket_after_the_servers_go_ahead() {
    let pki = pki();
    let identity = issued(&pki, "mail.test", key());
    let config = identity.config();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.write_all(b"220 hello\r\n").unwrap();
        let mut asked = [0u8; 10];
        peer.read_exact(&mut asked).unwrap();
        assert_eq!(&asked, b"STARTTLS\r\n");
        peer.write_all(b"220 go ahead\r\n").unwrap();
        let mut stream = StreamOwned::new(ServerConnection::new(config).unwrap(), peer);
        greets(&mut stream);
    });
    let mut sockets = sockets_trusting(&pki);
    sockets.apply(SocketCall::Connect {
        socket: 1,
        host: String::from("127.0.0.1"),
        port,
        tls: None,
    });
    assert_eq!(wait(&mut sockets), SocketEvent::Connected { socket: 1 });
    let mut greeting = Vec::new();
    while greeting.len() < b"220 hello\r\n".len() {
        greeting.extend(read(&mut sockets));
    }
    sockets.apply(SocketCall::Send {
        socket: 1,
        data: b"STARTTLS\r\n".to_vec(),
    });
    let go = b"220 go ahead\r\n";
    let mut answer = Vec::new();
    while answer.len() < go.len() {
        sockets.apply(SocketCall::Read {
            socket: 1,
            max: u32::try_from(go.len() - answer.len()).unwrap(),
        });
        let SocketEvent::Data { data, .. } = wait(&mut sockets) else {
            panic!("no data");
        };
        answer.extend(data);
    }
    sockets.apply(SocketCall::StartTls {
        socket: 1,
        tls: setup("mail.test", Trust::Authorities),
    });
    assert_eq!(
        wait(&mut sockets),
        SocketEvent::Secured {
            socket: 1,
            certificate: hash(identity.sha256()),
            key: key_hash(identity.key_sha256()),
            time_checked: true,
        }
    );
    assert_eq!(read(&mut sockets), b"hello");
    sockets.apply(SocketCall::Close { socket: 1 });
    server.join().unwrap();
}

#[test]
fn starttls_against_an_untrusted_server_ends_the_socket_untrusted() {
    let own = self_signed("mail.test", key());
    let config = own.config();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (peer, _) = listener.accept().unwrap();
        let mut stream = StreamOwned::new(ServerConnection::new(config).unwrap(), peer);
        greets(&mut stream);
    });
    let mut sockets = Sockets::new(|| {});
    sockets.apply(SocketCall::Connect {
        socket: 1,
        host: String::from("127.0.0.1"),
        port,
        tls: None,
    });
    assert_eq!(wait(&mut sockets), SocketEvent::Connected { socket: 1 });
    sockets.apply(SocketCall::StartTls {
        socket: 1,
        tls: setup("mail.test", Trust::Authorities),
    });
    assert!(matches!(
        wait(&mut sockets),
        SocketEvent::Untrusted {
            why: Distrust::UnknownIssuer,
            ..
        }
    ));
    assert!(!sockets.any());
    server.join().unwrap();
}

#[test]
fn the_gate_holds_so_many_sessions_and_a_waiter_gets_one_given_back() {
    let gate = Arc::new(Gate::new(1));
    let soon = || Instant::now() + Duration::from_millis(100);
    let first = gate.take(soon()).unwrap();
    assert!(gate.take(soon()).is_none());
    let waiter = {
        let gate = gate.clone();
        thread::spawn(move || gate.take(Instant::now() + Duration::from_secs(5)).is_some())
    };
    thread::sleep(Duration::from_millis(50));
    drop(first);
    assert!(waiter.join().unwrap());
    // The waiter's session went with its thread: one is free again.
    assert!(gate.take(soon()).is_some());
}

#[test]
fn the_walk_through_a_certificate_finds_its_key_and_dates() {
    let pki = pki();
    let key = key();
    let spki = key.subject_public_key_info();
    let identity = dated(&pki, "a.test", key, (2023, 5, 17), (2031, 12, 31));
    let parts = der::parts(&identity.der).unwrap();
    assert_eq!(parts.spki, spki.as_slice());
    assert_eq!(parts.not_before, 1_684_281_600);
    assert_eq!(parts.not_after, 1_956_441_600);
    // The authority's own: a self-signed certificate reads alike.
    assert!(der::parts(pki.ca.der()).is_some());
}

/// A server that finishes the handshake, then reads nothing until told.
fn deaf_server(identity: &Identity, release: mpsc::Receiver<()>) -> (u16, JoinHandle<bool>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = identity.config();
    let server = thread::spawn(move || {
        let (mut tcp, _) = listener.accept().unwrap();
        let mut session = ServerConnection::new(config).unwrap();
        while session.is_handshaking() {
            session.complete_io(&mut tcp).unwrap();
        }
        let _ = release.recv_timeout(Duration::from_secs(20));
        // Now reads: until the client's end, if it ended the connection.
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            match tcp.read(&mut buffer) {
                Ok(0) => return true,
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => return true,
                Err(_) => return false,
            }
        }
    });
    (port, server)
}

fn send_much(sockets: &mut Sockets) {
    for _ in 0..10 {
        sockets.apply(SocketCall::Send {
            socket: 1,
            data: vec![7u8; 1024 * 1024],
        });
    }
}

#[test]
fn a_server_that_never_reads_cannot_hold_a_session_after_close() {
    let pki = pki();
    let identity = issued(&pki, "mail.test", key());
    let (release, released) = mpsc::channel();
    let (port, server) = deaf_server(&identity, released);
    let gate = Arc::new(Gate::new(2));
    let mut sockets = sockets_trusting(&pki);
    sockets.tls = sockets
        .tls
        .clone()
        .with_gate(gate.clone(), Duration::from_millis(300));
    connect(&mut sockets, port, setup("mail.test", Trust::Authorities));
    assert!(matches!(wait(&mut sockets), SocketEvent::Secured { .. }));
    assert_eq!(gate.held(), 1);
    // Ten megabytes to a server that reads none, and the plugin closes.
    send_much(&mut sockets);
    sockets.apply(SocketCall::Close { socket: 1 });
    let until = Instant::now() + Duration::from_secs(5);
    while gate.held() > 0 {
        assert!(Instant::now() < until, "the session is held still");
        thread::sleep(Duration::from_millis(20));
    }
    // The connection ended: the server finds its end, not an open stream.
    release.send(()).unwrap();
    assert!(server.join().unwrap(), "the connection was left open");
}

#[test]
fn a_server_that_never_reads_ends_the_socket_with_a_timeout() {
    let pki = pki();
    let identity = issued(&pki, "mail.test", key());
    let (release, released) = mpsc::channel();
    let (port, server) = deaf_server(&identity, released);
    let gate = Arc::new(Gate::new(2));
    let mut sockets = sockets_trusting(&pki);
    sockets.tls = sockets
        .tls
        .clone()
        .with_gate(gate.clone(), Duration::from_millis(300));
    connect(&mut sockets, port, setup("mail.test", Trust::Authorities));
    assert!(matches!(wait(&mut sockets), SocketEvent::Secured { .. }));
    send_much(&mut sockets);
    let SocketEvent::Failed { error, .. } = wait(&mut sockets) else {
        panic!("not failed");
    };
    assert_eq!(error, SocketError::Timeout);
    assert!(!sockets.any());
    let until = Instant::now() + Duration::from_secs(5);
    while gate.held() > 0 {
        assert!(Instant::now() < until, "the session is held still");
        thread::sleep(Duration::from_millis(20));
    }
    release.send(()).unwrap();
    assert!(server.join().unwrap(), "the connection was left open");
}

/// What a plugin is told, as a test can compare.
#[derive(Debug, PartialEq)]
enum Told {
    Untrusted(Distrust),
    Failed(SocketError),
}

#[test]
fn what_the_chain_says_of_a_certificate_becomes_the_verdict_the_design_lists() {
    use rustls::CertificateError as C;
    let other = || {
        C::Other(rustls::OtherError(Arc::new(io::Error::other(
            "a chain rule",
        ))))
    };
    // The table of the design (tls-sockets-sdk.md §7): the same rows.
    let table: Vec<(rustls::Error, Told)> = vec![
        (
            C::UnknownIssuer.into(),
            Told::Untrusted(Distrust::UnknownIssuer),
        ),
        (other().into(), Told::Untrusted(Distrust::UnknownIssuer)),
        (
            C::BadSignature.into(),
            Told::Untrusted(Distrust::UnknownIssuer),
        ),
        (
            C::InvalidPurpose.into(),
            Told::Untrusted(Distrust::UnknownIssuer),
        ),
        (
            C::UnhandledCriticalExtension.into(),
            Told::Untrusted(Distrust::UnknownIssuer),
        ),
        (C::Revoked.into(), Told::Untrusted(Distrust::UnknownIssuer)),
        (
            C::UnsupportedSignatureAlgorithmContext {
                signature_algorithm_id: Vec::new(),
                supported_algorithms: Vec::new(),
            }
            .into(),
            Told::Untrusted(Distrust::UnknownIssuer),
        ),
        (
            C::NotValidForName.into(),
            Told::Failed(SocketError::NameMismatch),
        ),
        (C::Expired.into(), Told::Failed(SocketError::Expired)),
        (C::NotValidYet.into(), Told::Failed(SocketError::Expired)),
        (C::BadEncoding.into(), Told::Failed(SocketError::Tls)),
        (
            rustls::Error::General(String::from("x")),
            Told::Failed(SocketError::Tls),
        ),
    ];
    let verifier = Verifier::new(&Trust::Authorities, super::system_clock).unwrap();
    verifier.remember(Leaf {
        certificate: [1; 32],
        key: [2; 32],
        time_checked: true,
    });
    for (error, told) in table {
        let io = io::Error::new(io::ErrorKind::InvalidData, error.clone());
        let got = match refusal(&io, &verifier) {
            Refusal::Untrusted {
                why,
                certificate,
                key,
            } => {
                assert_eq!((certificate, key), ([1; 32], [2; 32]), "{error:?}");
                Told::Untrusted(why)
            }
            Refusal::Failed(error, _) => Told::Failed(error),
        };
        assert_eq!(got, told, "{error:?}");
    }
    // Not a rustls error: by the kind of the failure.
    for (kind, told) in [
        (io::ErrorKind::TimedOut, SocketError::Timeout),
        (io::ErrorKind::UnexpectedEof, SocketError::Lost),
        (io::ErrorKind::PermissionDenied, SocketError::Tls),
    ] {
        let Refusal::Failed(error, _) = refusal(&io::Error::from(kind), &verifier) else {
            panic!("not failed");
        };
        assert_eq!(error, told, "{kind:?}");
    }
    // Where no certificate was shown, an untrusted one is a failure.
    let unshown = Verifier::new(&Trust::Authorities, super::system_clock).unwrap();
    let io = io::Error::new(
        io::ErrorKind::InvalidData,
        rustls::Error::from(C::UnknownIssuer),
    );
    assert!(matches!(
        refusal(&io, &unshown),
        Refusal::Failed(SocketError::Tls, _)
    ));
}

/// Presents `chain` whatever it is.
#[derive(Debug)]
struct Raw(Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for Raw {
    fn resolve(
        &self,
        _: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(self.0.clone())
    }
}

/// A server that presents `der` for `identity`'s key, whatever it is.
fn raw_server(identity: &Identity, der: Vec<u8>) -> (u16, JoinHandle<()>) {
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(identity.key.serialize_der()));
    let key = rustls::crypto::ring::sign::any_supported_type(&key).unwrap();
    let certified = rustls::sign::CertifiedKey::new(vec![CertificateDer::from(der)], key);
    let config = ServerConfig::builder_with_provider(super::verify::provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Raw(Arc::new(certified))));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = Arc::new(config);
    let server = thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut stream = StreamOwned::new(ServerConnection::new(config).unwrap(), tcp);
        greets(&mut stream);
    });
    (port, server)
}

#[test]
fn a_certificate_not_in_strict_der_is_refused_even_with_the_pinned_key() {
    let own = self_signed("a.test", key());
    let pinned = Trust::PinnedKeys(vec![key_hash(own.key_sha256())]);
    let strict = own.der.to_vec();
    // The same certificate, with bytes after it, and with its length
    // written in more bytes than it needs.
    let mut trailing = strict.clone();
    trailing.extend_from_slice(&[0, 0, 0]);
    assert_eq!(strict[1], 0x82, "a length of two bytes");
    let mut padded = vec![0x30, 0x83, 0x00, strict[2], strict[3]];
    padded.extend_from_slice(&strict[4..]);
    // The walk reads all three alike: the handshake must not.
    assert_eq!(der::parts(&strict), der::parts(&trailing));
    assert_eq!(der::parts(&strict), der::parts(&padded));
    for (name, der, secured) in [
        ("strict", strict, true),
        ("trailing", trailing, false),
        ("padded", padded, false),
    ] {
        let (port, server) = raw_server(&own, der);
        let mut sockets = Sockets::new(|| {});
        connect(&mut sockets, port, setup("a.test", pinned.clone()));
        let event = wait(&mut sockets);
        if secured {
            assert!(
                matches!(event, SocketEvent::Secured { .. }),
                "{name}: {event:?}"
            );
            sockets.apply(SocketCall::Close { socket: 1 });
        } else {
            assert!(
                matches!(
                    event,
                    SocketEvent::Failed {
                        error: SocketError::Tls,
                        ..
                    }
                ),
                "{name}: {event:?}"
            );
        }
        server.join().unwrap();
    }
}

// The public client session (`crate::secure`) over the same checks.

/// Takes the handshake through (the server's session starts on first use).
fn drives(stream: &mut StreamOwned<ServerConnection, TcpStream>) {
    let _ = stream.read(&mut [0u8; 1]);
}

fn client(port: u16) -> TcpStream {
    TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap()
}

#[test]
fn a_secure_session_reads_and_writes_after_a_pinned_handshake() {
    use crate::secure::{Pins, handshake};
    let own = self_signed("x.test", key());
    let (port, server) = secure_server(&own, greets);
    let mut tcp = client(port);
    let pins = [[9; 32], own.sha256()];
    let (session, peer) = handshake(
        &mut tcp,
        "x.test",
        Pins::Certificates(&pins),
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(
        (peer.certificate, peer.key),
        (own.sha256(), own.key_sha256())
    );
    let mut buf = [0u8; 16];
    let mut reader = tcp.try_clone().unwrap();
    let n = session.read(&mut reader, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello");
    session.write_all(b"ping").unwrap();
    let n = session.read(&mut reader, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"ping");
    session.close_write();
    server.join().unwrap();
}

#[test]
fn a_secure_session_trusts_a_pinned_key_and_nothing_else() {
    use crate::secure::{Failure, Pins, handshake};
    let own = self_signed("x.test", key());
    let (port, server) = secure_server(&own, drives);
    // Another key than the server's: untrusted, `not_pinned`, with its hashes.
    let mut tcp = client(port);
    let other = [[7; 32]];
    let failed = handshake(
        &mut tcp,
        "x.test",
        Pins::Keys(&other),
        Duration::from_secs(5),
    )
    .err()
    .unwrap();
    let Failure::Untrusted { peer, not_pinned } = failed else {
        panic!("{failed:?}");
    };
    assert!(not_pinned);
    assert_eq!(
        (peer.certificate, peer.key),
        (own.sha256(), own.key_sha256())
    );
    server.join().unwrap();
    // The right key is trusted.
    let (port, server) = secure_server(&own, drives);
    let mut tcp = client(port);
    let kept = [own.key_sha256()];
    let session = handshake(
        &mut tcp,
        "x.test",
        Pins::Keys(&kept),
        Duration::from_secs(5),
    );
    assert!(session.is_ok());
    // The server waits for the client to be done.
    drop(session);
    drop(tcp);
    server.join().unwrap();
}

#[test]
fn a_secure_session_does_not_trust_a_certificate_nobody_vouches_for() {
    use crate::secure::{Failure, Pins, handshake};
    // The shipped authorities do not know a certificate that signed itself:
    // there is no mode that accepts what it is shown.
    let own = self_signed("x.test", key());
    let (port, server) = secure_server(&own, drives);
    let mut tcp = client(port);
    let failed = handshake(
        &mut tcp,
        "x.test",
        Pins::Authorities,
        Duration::from_secs(5),
    )
    .err()
    .unwrap();
    assert!(
        matches!(
            failed,
            Failure::Untrusted {
                not_pinned: false,
                ..
            }
        ),
        "{failed:?}"
    );
    server.join().unwrap();
}

#[test]
fn a_secure_handshake_that_takes_too_long_fails_with_timeout() {
    use crate::secure::{Failure, Pins, handshake};
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let silent = thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        thread::sleep(Duration::from_millis(1500));
        drop(tcp);
    });
    let mut tcp = client(port);
    let started = Instant::now();
    let failed = handshake(
        &mut tcp,
        "x.test",
        Pins::Authorities,
        Duration::from_millis(300),
    )
    .err()
    .unwrap();
    assert_eq!(failed, Failure::Timeout);
    assert!(started.elapsed() < Duration::from_secs(1));
    silent.join().unwrap();
}
