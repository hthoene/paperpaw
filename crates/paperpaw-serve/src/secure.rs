//! A client TLS session over a TCP stream, for the shells whose own sockets
//! are not [`crate::Sockets`] (the desktop's `core.sockets`, interface 7):
//! the same checks as the sockets' own (the `tls` module), without the
//! interface-6 kernel types.
//!
//! - **Trust:** the authorities PaperPaw ships (Mozilla's list, as the X4
//!   has it: chain, name and dates), or the certificates or keys given as
//!   pins (the leaf's SHA-256 or its key's; its dates checked, chain and name
//!   not). There is no mode that trusts what it is shown.
//! - **Nothing before the handshake ends:** a [`Session`] exists only once it
//!   passed; until then no byte of the caller goes out.
//! - **No sessions are counted here:** the caller (core) owns the lease.

use std::io;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use paperpaw_core::TlsSetup;
use paperpaw_core::sdk::{CertificateHash, Distrust, KeyHash, SocketError, Trust};

use crate::tls::{self, Env, Gate, Refusal, Secure};

/// Which certificates are trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pins<'a> {
    /// The authorities PaperPaw ships, for the server's name.
    Authorities,
    /// The server's own certificate, by the SHA-256 of its DER: one of these.
    Certificates(&'a [[u8; 32]]),
    /// The server's key, by the SHA-256 of its SubjectPublicKeyInfo: one of these.
    Keys(&'a [[u8; 32]]),
}

/// What the handshake showed of the server's certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    /// SHA-256 of the leaf certificate's DER.
    pub certificate: [u8; 32],
    /// SHA-256 of its SubjectPublicKeyInfo DER.
    pub key: [u8; 32],
    /// The dates were checked against a clock that can be believed.
    pub time_checked: bool,
}

/// Why a handshake did not come about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// Only the trust failed (the hashes go with it): no authority vouches
    /// for the chain (`not_pinned` false), or no pin matched (`true`).
    Untrusted { peer: Peer, not_pinned: bool },
    /// The certificate is for another name.
    Name,
    /// The certificate has expired or is not valid yet.
    Expired,
    /// The handshake took too long.
    Timeout,
    /// The server ended the connection.
    Lost,
    /// Any other failure (the reason is not kept: it may name the server).
    Tls,
}

/// The secure session of a connection, shared by its reading and writing
/// (clone it for the thread that writes).
#[derive(Debug, Clone)]
pub struct Session(Arc<Secure>);

impl Session {
    /// Reads at most `buffer.len()` bytes of what the server sent, from `tcp`
    /// (the reading side of the connection) as needed; 0 where the server
    /// closed the session properly, an error where it did not.
    ///
    /// # Errors
    /// The stream's, or `InvalidData` for what the session refused.
    pub fn read(&self, tcp: &mut TcpStream, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(tcp, buffer)
    }

    /// Sends `data`, encrypted. A write that makes no progress for 30 s fails.
    ///
    /// # Errors
    /// The stream's.
    pub fn write_all(&self, data: &[u8]) -> io::Result<()> {
        self.0.write_all(data)
    }

    /// Tells the server this side is done (`close_notify`) and ends the
    /// connection's writing.
    pub fn close_write(&self) {
        self.0.close_write();
    }
}

/// The handshake on `tcp` for `server_name`, trusting `pins`, within
/// `timeout`.
///
/// # Errors
/// [`Failure`], nothing of the caller having been sent.
pub fn handshake(
    tcp: &mut TcpStream,
    server_name: &str,
    pins: Pins<'_>,
    timeout: Duration,
) -> Result<(Session, Peer), Failure> {
    let trust = match pins {
        Pins::Authorities => Trust::Authorities,
        Pins::Certificates(hashes) => {
            Trust::Pinned(hashes.iter().copied().map(CertificateHash::new).collect())
        }
        Pins::Keys(hashes) => Trust::PinnedKeys(hashes.iter().copied().map(KeyHash::new).collect()),
    };
    let setup = TlsSetup {
        server_name: server_name.to_owned(),
        trust,
        timeout_ms: u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX),
        idle_ms: 0,
    };
    let env = Env::system();
    let deadline = Instant::now() + timeout;
    // The caller counts the sessions; this gate only holds the one slot.
    let slot = Arc::new(Gate::new(1))
        .take(deadline)
        .ok_or(Failure::Timeout)?;
    let verifier = env.verifier(&setup);
    match tls::handshake(tcp, &setup, verifier, deadline, slot, env.stall()) {
        Ok((secure, seen)) => Ok((
            Session(secure),
            Peer {
                certificate: seen.certificate,
                key: seen.key,
                time_checked: seen.time_checked,
            },
        )),
        Err(Refusal::Untrusted {
            certificate,
            key,
            why,
        }) => Err(Failure::Untrusted {
            peer: Peer {
                certificate,
                key,
                // The dates of a certificate nobody trusted were not the point.
                time_checked: false,
            },
            not_pinned: why == Distrust::NotPinned,
        }),
        Err(Refusal::Failed(error, _)) => Err(match error {
            SocketError::NameMismatch => Failure::Name,
            SocketError::Expired => Failure::Expired,
            SocketError::Timeout => Failure::Timeout,
            SocketError::Lost => Failure::Lost,
            _ => Failure::Tls,
        }),
    }
}
