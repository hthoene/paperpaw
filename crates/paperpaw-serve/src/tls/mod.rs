//! TLS for plugins' sockets in shells with the standard library (owner
//! decision 52): rustls with ring, over the sockets' own TCP streams, as
//! `paperpaw_core::SocketCall` asks and as the X4's firmware does it
//! (`docs`: tls-sockets-sdk.md §7).
//!
//! - **Trust** ([`verify`]): the authorities PaperPaw ships (Mozilla's
//!   list, `webpki-roots`, the same as the X4's), or the certificates or
//!   keys a plugin pinned. Nothing else; no mode trusts what it is shown.
//! - **Nothing before `Secured`:** the handshake runs before a stream is
//!   given to reading and writing; the kernel asks for no `Send` or
//!   `Read` meanwhile.
//! - **STARTTLS** refuses a socket with bytes of the server waiting
//!   (they might be injected, and would pass as secure).
//! - **Sessions:** at most [`SESSIONS`] at once; a socket waits for a
//!   free one until its timeout ([`Gate`]).

mod der;
#[cfg(test)]
mod tests;
mod verify;

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use paperpaw_core::TlsSetup;
use paperpaw_core::sdk::{Distrust, SocketError};
use rustls::pki_types::ServerName;
use rustls::{CertificateError, ClientConfig, ClientConnection, Error};

pub(crate) use verify::{Clock, system_clock};
use verify::{Leaf, NotPinned, Verifier};

/// Secure sessions at once (`KernelBuilder::tls_sessions`): a computer has
/// room for many, the number only keeps a plugin that opens sockets in a
/// loop in bounds.
pub const SESSIONS: u8 = 8;

/// How long a write may make no progress before the connection ends,
/// `Failed` with `Timeout`: a server that never reads must not hold a
/// session (and the threads) for ever. A write that moves bytes, however
/// slowly, is not cut off.
pub(crate) const WRITE_STALL: Duration = Duration::from_secs(30);

/// What a handshake that did not come about is: a certificate not
/// trusted (its hashes go with the news), or a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    Untrusted {
        certificate: [u8; 32],
        key: [u8; 32],
        why: Distrust,
    },
    Failed(SocketError, String),
}

impl Refusal {
    pub(crate) fn failed(error: SocketError, reason: &str) -> Self {
        Self::Failed(error, String::from(reason))
    }
}

/// What a handshake showed of the server's certificate, trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Secured {
    pub certificate: [u8; 32],
    pub key: [u8; 32],
    pub time_checked: bool,
}

/// How many secure sessions are open, and who waits for one.
#[derive(Debug)]
pub(crate) struct Gate {
    open: Mutex<u8>,
    freed: Condvar,
    limit: u8,
}

impl Gate {
    pub(crate) const fn new(limit: u8) -> Self {
        Self {
            open: Mutex::new(0),
            freed: Condvar::new(),
            limit,
        }
    }

    /// How many sessions are held (tests).
    #[cfg(test)]
    pub(crate) fn held(&self) -> u8 {
        self.open.lock().map_or(0, |open| *open)
    }

    /// A session, once one is free, if that is before `deadline`.
    pub(crate) fn take(self: &Arc<Self>, deadline: Instant) -> Option<Slot> {
        let mut open = self.open.lock().ok()?;
        while *open >= self.limit {
            let left = deadline.checked_duration_since(Instant::now())?;
            let (again, timeout) = self.freed.wait_timeout(open, left).ok()?;
            open = again;
            if timeout.timed_out() && *open >= self.limit {
                return None;
            }
        }
        *open += 1;
        Some(Slot(self.clone()))
    }
}

/// A session of the [`Gate`], held; given back when dropped.
#[derive(Debug)]
pub(crate) struct Slot(Arc<Gate>);

impl Drop for Slot {
    fn drop(&mut self) {
        if let Ok(mut open) = self.0.open.lock() {
            *open = open.saturating_sub(1);
        }
        self.0.freed.notify_one();
    }
}

/// The secure session of a socket, shared by its reading and writing.
/// Locks are held for the work on the session only, never while waiting
/// for the network to read; writes to the stream are in the order the
/// session made them.
#[derive(Debug)]
pub(crate) struct Secure {
    session: Mutex<ClientConnection>,
    out: Mutex<TcpStream>,
    _slot: Slot,
}

fn poisoned<T>(_: T) -> io::Error {
    io::Error::other("a lock was poisoned")
}

impl Secure {
    /// Sends what the session has to send, and gives up its lock only
    /// once the stream's is held (so what two threads make is sent in
    /// order).
    fn flush(&self, mut session: MutexGuard<'_, ClientConnection>) -> io::Result<()> {
        let mut records = Vec::new();
        while session.wants_write() {
            session.write_tls(&mut records)?;
        }
        if records.is_empty() {
            return Ok(());
        }
        let mut out = self.out.lock().map_err(poisoned)?;
        drop(session);
        out.write_all(&records)?;
        out.flush()
    }

    /// Reads at most `buffer.len()` bytes of what the server sent, from
    /// `tcp` (the reading side of the stream) as needed; 0 where it
    /// closed the session properly, an error where it did not.
    pub(crate) fn read(&self, tcp: &mut TcpStream, buffer: &mut [u8]) -> io::Result<usize> {
        let mut raw = [0u8; 16 * 1024 + 512];
        loop {
            match self.session.lock().map_err(poisoned)?.reader().read(buffer) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                done => return done,
            }
            let count = tcp.read(&mut raw)?;
            let mut session = self.session.lock().map_err(poisoned)?;
            let mut bytes = &raw[..count];
            // An empty read is the stream's end: the session then knows
            // it was cut off.
            loop {
                let used = session.read_tls(&mut bytes)?;
                if let Err(e) = session.process_new_packets() {
                    let _ = self.flush(session);
                    return Err(io::Error::new(io::ErrorKind::InvalidData, e));
                }
                if bytes.is_empty() {
                    break;
                }
                // Bytes left that the session will not take (its buffer is
                // empty here, so it should): they are not dropped, which
                // would fail the stream later with a MAC error.
                if used == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the secure session took no more of what was read",
                    ));
                }
            }
            self.flush(session)?;
        }
    }

    /// Sends `data`, encrypted.
    pub(crate) fn write_all(&self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let mut session = self.session.lock().map_err(poisoned)?;
            let taken = session.writer().write(data)?;
            data = &data[taken..];
            self.flush(session)?;
        }
        Ok(())
    }

    /// Tells the server this side is done (`close_notify`), then ends the
    /// stream's writing.
    pub(crate) fn close_write(&self) {
        if let Ok(mut session) = self.session.lock() {
            session.send_close_notify();
            let _ = self.flush(session);
        }
        if let Ok(out) = self.out.lock() {
            let _ = out.shutdown(Shutdown::Write);
        }
    }
}

/// What a failed handshake was, as a plugin is told.
fn refusal(error: &io::Error, verifier: &Verifier) -> Refusal {
    let shown = verifier.seen();
    let inner = error.get_ref().and_then(|e| e.downcast_ref::<Error>());
    let Some(Error::InvalidCertificate(why)) = inner else {
        return match error.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                Refusal::failed(SocketError::Timeout, "the secure connection took too long")
            }
            io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset => {
                Refusal::failed(SocketError::Lost, "the server ended the connection")
            }
            _ => Refusal::Failed(
                SocketError::Tls,
                format!("secure connection failed: {error}"),
            ),
        };
    };
    let untrusted = |why| {
        shown.map_or_else(
            || Refusal::failed(SocketError::Tls, "the server showed no certificate"),
            |leaf: Leaf| Refusal::Untrusted {
                certificate: leaf.certificate,
                key: leaf.key,
                why,
            },
        )
    };
    match why {
        CertificateError::Other(other) if other.0.downcast_ref::<NotPinned>().is_some() => {
            untrusted(Distrust::NotPinned)
        }
        // No authority vouches for it: the chain does not reach one it
        // ships, or what the chain says of its certificates does not allow
        // this (a certificate authority's own, as a server that made its
        // own presents, is no server's). The reader may pin it.
        CertificateError::UnknownIssuer
        | CertificateError::Other(_)
        | CertificateError::BadSignature
        | CertificateError::UnsupportedSignatureAlgorithmContext { .. }
        | CertificateError::UnsupportedSignatureAlgorithmForPublicKeyContext { .. }
        | CertificateError::InvalidPurpose
        | CertificateError::UnhandledCriticalExtension
        | CertificateError::Revoked => untrusted(Distrust::UnknownIssuer),
        CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
            Refusal::failed(
                SocketError::NameMismatch,
                "the server's certificate is for another host",
            )
        }
        CertificateError::Expired
        | CertificateError::ExpiredContext { .. }
        | CertificateError::NotValidYet
        | CertificateError::NotValidYetContext { .. } => Refusal::failed(
            SocketError::Expired,
            "the server's certificate has expired or is not valid yet",
        ),
        other => Refusal::Failed(
            SocketError::Tls,
            format!("the certificate is not usable: {other:?}"),
        ),
    }
}

/// The handshake on `tcp`, as `setup` asks, within `deadline`, holding
/// `slot`.
pub(crate) fn handshake(
    tcp: &mut TcpStream,
    setup: &TlsSetup,
    verifier: Option<Verifier>,
    deadline: Instant,
    slot: Slot,
    stall: Duration,
) -> Result<(Arc<Secure>, Secured), Refusal> {
    let Some(verifier) = verifier else {
        return Err(Refusal::failed(SocketError::Tls, "no authorities to trust"));
    };
    let verifier = Arc::new(verifier);
    let name = ServerName::try_from(setup.server_name.clone())
        .map_err(|_| Refusal::failed(SocketError::Options, "the server name is not valid"))?;
    let builder = ClientConfig::builder_with_provider(verify::provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| Refusal::Failed(SocketError::Tls, e.to_string()))?;
    let mut config = builder
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    // A resumed session shows no certificate to check.
    config.resumption = rustls::client::Resumption::disabled();
    let mut session = ClientConnection::new(Arc::new(config), name)
        .map_err(|e| Refusal::Failed(SocketError::Tls, e.to_string()))?;
    let failed = |e: io::Error| refusal(&e, &verifier);
    while session.is_handshaking() {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| Refusal::failed(SocketError::Timeout, "the handshake took too long"))?;
        tcp.set_read_timeout(Some(left)).map_err(failed)?;
        tcp.set_write_timeout(Some(left)).map_err(failed)?;
        if session.wants_write() {
            session.write_tls(tcp).map_err(failed)?;
        } else if session.wants_read() {
            if session.read_tls(tcp).map_err(failed)? == 0 {
                return Err(Refusal::failed(
                    SocketError::Lost,
                    "the server ended the connection",
                ));
            }
            if let Err(e) = session.process_new_packets() {
                let _ = session.write_tls(tcp);
                return Err(failed(io::Error::new(io::ErrorKind::InvalidData, e)));
            }
        }
    }
    while session.wants_write() {
        session.write_tls(tcp).map_err(failed)?;
    }
    tcp.set_read_timeout(None).map_err(failed)?;
    // Writes later on give up when they make no progress (`WRITE_STALL`).
    tcp.set_write_timeout(Some(stall)).map_err(failed)?;
    let leaf = verifier
        .seen()
        .ok_or_else(|| Refusal::failed(SocketError::Tls, "the server showed no certificate"))?;
    let out = tcp.try_clone().map_err(failed)?;
    let secure = Secure {
        session: Mutex::new(session),
        out: Mutex::new(out),
        _slot: slot,
    };
    Ok((
        Arc::new(secure),
        Secured {
            certificate: leaf.certificate,
            key: leaf.key,
            time_checked: leaf.time_checked,
        },
    ))
}

fn shared_gate() -> &'static Arc<Gate> {
    static GATE: OnceLock<Arc<Gate>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(Gate::new(SESSIONS)))
}

/// What a socket's TLS depends on besides the server: the clock, and in
/// tests the authorities.
#[derive(Debug, Clone)]
pub(crate) struct TlsContext {
    clock: Clock,
    gate: Arc<Gate>,
    stall: Duration,
    #[cfg(test)]
    pub(crate) roots: Option<Arc<rustls::client::WebPkiServerVerifier>>,
}

impl TlsContext {
    /// The computer's clock and the authorities PaperPaw ships.
    pub(crate) fn system() -> Self {
        Self {
            clock: system_clock,
            gate: shared_gate().clone(),
            stall: WRITE_STALL,
            #[cfg(test)]
            roots: None,
        }
    }

    /// The sessions of this socket's computer, shared by all sockets.
    pub(crate) fn gate(&self) -> &Arc<Gate> {
        &self.gate
    }

    /// How long a write may make no progress.
    pub(crate) fn stall(&self) -> Duration {
        self.stall
    }

    /// With sessions of their own and a write stall of `stall` (tests).
    #[cfg(test)]
    pub(crate) fn with_gate(mut self, gate: Arc<Gate>, stall: Duration) -> Self {
        self.gate = gate;
        self.stall = stall;
        self
    }

    /// With the clock `clock` (tests).
    #[cfg(test)]
    pub(crate) fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// The checks for `setup`'s trust.
    pub(crate) fn verifier(&self, setup: &TlsSetup) -> Option<Verifier> {
        #[cfg(test)]
        if let Some(roots) = &self.roots {
            return Verifier::with_authorities(&setup.trust, self.clock, || Some(roots.clone()));
        }
        Verifier::new(&setup.trust, self.clock)
    }
}

/// How long is left until `deadline`, at least a millisecond.
pub(crate) fn left(deadline: Instant) -> Duration {
    deadline
        .checked_duration_since(Instant::now())
        .unwrap_or_default()
        .max(Duration::from_millis(1))
}
