//! What the browser's module has of TLS for sockets: nothing. It has no
//! sockets (`KernelBuilder::tls_sessions` is 0 there, so the kernel asks
//! for no secure connection); these are the shapes `link` needs.

use std::io;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use paperpaw_core::TlsSetup;
use paperpaw_core::sdk::{Distrust, SocketError};

pub const SESSIONS: u8 = 0;

pub(crate) const WRITE_STALL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    #[expect(dead_code, reason = "nothing is secured here: only Failed is made")]
    Untrusted {
        certificate: [u8; 32],
        key: [u8; 32],
        why: Distrust,
    },
    Failed(SocketError, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Secured {
    pub certificate: [u8; 32],
    pub key: [u8; 32],
    pub time_checked: bool,
}

#[derive(Debug)]
pub(crate) struct Gate;

impl Gate {
    pub(crate) const fn new(_limit: u8) -> Self {
        Self
    }

    pub(crate) fn take(self: &Arc<Self>, _deadline: Instant) -> Option<Slot> {
        None
    }
}

#[derive(Debug)]
pub(crate) struct Slot;

#[derive(Debug)]
pub(crate) struct Secure;

impl Secure {
    pub(crate) fn read(&self, _tcp: &mut TcpStream, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub(crate) fn write_all(&self, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub(crate) fn close_write(&self) {}
}

#[derive(Debug, Clone)]
pub(crate) struct TlsContext(Arc<Gate>);

impl TlsContext {
    pub(crate) fn system() -> Self {
        Self(Arc::new(Gate::new(0)))
    }

    pub(crate) fn gate(&self) -> &Arc<Gate> {
        &self.0
    }

    pub(crate) fn stall(&self) -> Duration {
        WRITE_STALL
    }

    pub(crate) fn verifier(&self, _setup: &TlsSetup) -> Option<()> {
        None
    }
}

pub(crate) fn handshake(
    _tcp: &mut TcpStream,
    _setup: &TlsSetup,
    _verifier: Option<()>,
    _deadline: Instant,
    _slot: Slot,
    _stall: Duration,
) -> Result<(Arc<Secure>, Secured), Refusal> {
    Err(Refusal::Failed(
        SocketError::Unsupported,
        String::from("secure connections are not supported here"),
    ))
}

pub(crate) fn left(deadline: Instant) -> Duration {
    deadline
        .checked_duration_since(Instant::now())
        .unwrap_or_default()
        .max(Duration::from_millis(1))
}
