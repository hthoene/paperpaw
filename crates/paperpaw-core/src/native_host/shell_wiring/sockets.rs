//! What a shell gives `core.sockets` (`docs/abi/sockets.md`): its TCP and UDP
//! transport ([`SockTransport`]), its TLS if it has one ([`SockTls`]) and the
//! state of its network, clock and power ([`ShellNetwork`], the same one
//! `core.net` uses).
//!
//! Core owns every rule of the service (limits, the pool, the TLS session
//! lease, reads to a file, events); the shell moves bytes and does the
//! handshake. Every outcome comes back through [`SockReports`] from the
//! shell's own loop, never from inside a [`SockTransport`] or [`SockTls`]
//! method. `ERR_BUSY` from a report means the table is borrowed: call again on
//! the next turn of the loop, never spin in the same one.
//!
//! # Memory
//! The TLS session of the device (about 40 KiB on the X4) is the shell's: it
//! allocates it once, at start, before the heap fragments. A shell that
//! cannot gives no [`SockTls`], and then `connect` with `tls`, `start_tls`
//! and `peer` answer `ERR_UNSUPPORTED` (it fails closed: nothing is ever sent
//! in the clear for a plugin that asked for TLS).
use super::*;
use crate::native_host::{
    net_policy::NetPolicy,
    net_pool::NetPool,
    owner_directory::OwnerDirectory,
    paths_in_use::PathsInUse,
    shared::Shared,
    sock_service::{self, SockHost, SockProfile, device::SockDevice, reason, tls_code},
};
use paperpaw_native::host_glue::ERR_NOT_FOUND;

/// What the device lets sockets use (`sockets.md` section 6), picked by the
/// shell from its profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SockLimits {
    /// The device-wide bytes of plugin network memory, as for `core.net`
    /// ([`NetLimits::pool_bytes`]); 0: no pool.
    pub pool_bytes: u32,
    /// Open sockets at once on the device.
    pub sockets: u32,
    /// The send queue of a socket, bytes (`SOCK_QUEUE`).
    pub queue: u32,
    /// The read part of a socket, bytes (`SOCK_READ`).
    pub read: u32,
    /// The TLS sessions the device has at once, as for `core.net`
    /// ([`NetLimits::tls_sessions`]).
    pub tls_sessions: u8,
}

impl SockLimits {
    /// The X4: two sockets of 4 KiB each for the queue and the read part.
    #[must_use]
    pub const fn small(pool_bytes: u32) -> Self {
        Self {
            pool_bytes,
            sockets: 2,
            queue: 4_096,
            read: 4_096,
            tls_sessions: 1,
        }
    }

    /// Other devices: eight sockets of 16 KiB each.
    #[must_use]
    pub const fn large(pool_bytes: u32) -> Self {
        Self {
            pool_bytes,
            sockets: 8,
            queue: 16_384,
            read: 16_384,
            tls_sessions: 4,
        }
    }
}

/// What `core.sockets` is wired with: the card's storage and the shell's
/// network, transport and (optionally) TLS.
#[non_exhaustive]
pub struct SockInputs {
    storage: Rc<dyn Storage>,
    transport: Rc<dyn SockTransport>,
    tls: Option<Rc<dyn SockTls>>,
    network: Rc<dyn ShellNetwork>,
    limits: SockLimits,
}

impl core::fmt::Debug for SockInputs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SockInputs")
            .field("limits", &self.limits)
            .field("tls", &self.tls.is_some())
            .finish_non_exhaustive()
    }
}

impl SockInputs {
    /// `storage` is the card (files read to, the staged file). Without
    /// [`Self::tls`] the TLS ops are `ERR_UNSUPPORTED`.
    #[must_use]
    pub fn new(
        storage: Rc<dyn Storage>,
        transport: Rc<dyn SockTransport>,
        network: Rc<dyn ShellNetwork>,
        limits: SockLimits,
    ) -> Self {
        Self {
            storage,
            transport,
            tls: None,
            network,
            limits,
        }
    }

    /// The shell has TLS: `connect` with `tls`, `start_tls` and `peer` work.
    #[must_use]
    pub fn tls(mut self, tls: Rc<dyn SockTls>) -> Self {
        self.tls = Some(tls);
        self
    }

    /// The pool size and the TLS sessions this wiring names.
    pub(crate) fn pool_shape(&self) -> (u32, u8) {
        (self.limits.pool_bytes, self.limits.tls_sessions)
    }

    pub(crate) fn device(
        self,
        pool: Rc<NetPool>,
        in_use: Rc<PathsInUse>,
        owners: Shared<OwnerDirectory>,
        policy: Rc<NetPolicy>,
    ) -> SockDevice {
        let profile = SockProfile::new(self.limits.sockets, self.limits.queue, self.limits.read);
        SockDevice::new(
            self.storage,
            self.transport,
            self.tls,
            self.network,
            profile,
            pool,
            in_use,
            owners,
            policy,
        )
    }
}

/// Why the transport lost a socket, as the shell saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SockFailure {
    /// There is no network.
    NoNetwork,
    /// The name did not resolve.
    Dns,
    /// The connection was refused or could not be made.
    Refused,
    /// The connect or lookup took too long.
    Timeout,
    /// An I/O error on the connection.
    Io,
    /// The peer or the network dropped the connection.
    Lost,
    /// The shell had no memory for the connection.
    NoMemory,
}

/// Why a certificate was not trusted (and nothing else was wrong).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UntrustedWhy {
    /// No root of the shell's store vouches for the chain.
    UnknownIssuer,
    /// Pins were given and none matched the leaf.
    NotPinned,
}

/// How a TLS handshake failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TlsFailure {
    /// The certificate's name does not match the name asked for.
    Name,
    /// The certificate has expired or is not valid yet.
    Expired,
    /// Any other TLS failure.
    Tls,
    /// The shell had no memory for the session.
    NoMemory,
}

/// The transport's and TLS's reports, for the shell's loop. `ERR_BUSY` means
/// the table is borrowed: call again on the next turn. A report for a socket
/// that is unknown, ended or in another state changes nothing (the count of
/// such reports is [`SockReports::take_ignored`]).
#[derive(Clone)]
pub struct SockReports(pub(super) Rc<SockLater>);

/// `core.sockets` until its first call: the shell's inputs, then the device;
/// and the TLS reports for no socket counted before it was made (as the
/// device counts them, [`SockReports::take_ignored`]).
pub(crate) struct SockLater {
    pub(crate) device: super::later::Later<SockInputs, SockDevice>,
    ignored: core::cell::Cell<u32>,
}

impl SockLater {
    pub(crate) fn new(inputs: SockInputs) -> Self {
        Self {
            device: super::later::Later::new(inputs),
            ignored: core::cell::Cell::new(0),
        }
    }

    fn made(&self) -> Option<&SockDevice> {
        self.device.made()
    }

    /// A TLS report before the device was made: no socket is in its
    /// handshake, so it changes nothing and is counted.
    fn ignored(&self) -> Result<(), i32> {
        self.ignored.set(self.ignored.get().saturating_add(1));
        Ok(())
    }
}

impl core::fmt::Debug for SockReports {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SockReports")
    }
}

impl SockReports {
    /// The connection of socket `id` is up.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn connected(&self, id: u32) -> Result<(), i32> {
        self.0
            .made()
            .map_or(Ok(()), |d| sock_service::connected(d, id))
    }

    /// `bytes` arrived for the read of socket `id`: how many were taken (the
    /// rest stays with the shell; nothing beyond the read is ever held).
    ///
    /// # Errors
    /// `ERR_BUSY`; `ERR_STATE` when no read was asked for.
    pub fn received(&self, id: u32, bytes: &[u8]) -> Result<usize, i32> {
        self.0
            .made()
            .map_or(Err(ERR_NOT_FOUND), |d| sock_service::received(d, id, bytes))
    }

    /// The peer of socket `id` closed its side.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn peer_closed(&self, id: u32) -> Result<(), i32> {
        self.0
            .made()
            .map_or(Ok(()), |d| sock_service::peer_closed(d, id))
    }

    /// The connection of socket `id` can take more bytes.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn writable(&self, id: u32) -> Result<(), i32> {
        self.0
            .made()
            .map_or(Ok(()), |d| sock_service::writable(d, id))
    }

    /// The transport lost socket `id`, or could not connect it.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn failed(&self, id: u32, why: SockFailure) -> Result<(), i32> {
        let why = match why {
            SockFailure::NoNetwork => reason::NO_NETWORK,
            SockFailure::Dns => reason::DNS,
            SockFailure::Refused => reason::REFUSED,
            SockFailure::Timeout => reason::TIMEOUT,
            SockFailure::Lost => reason::LOST,
            SockFailure::NoMemory => reason::NO_MEMORY,
            _ => reason::IO,
        };
        self.0
            .made()
            .map_or(Ok(()), |d| sock_service::failed(d, id, why))
    }

    /// An answer to broadcast `id` arrived from `from` (its address and port
    /// as text) with `data`. Answers past the limits, malformed ones and ones
    /// after the end are dropped.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn answered(&self, id: u32, from: &[u8], data: &[u8]) -> Result<(), i32> {
        self.0
            .made()
            .map_or(Ok(()), |d| sock_service::answered(d, id, from, data))
    }

    /// The handshake of socket `id` passed.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn secured(&self, id: u32, peer: &PeerInfo) -> Result<(), i32> {
        self.0
            .made()
            .map_or_else(|| self.0.ignored(), |d| sock_service::secured(d, id, peer))
    }

    /// Only the trust of the certificate failed: the socket ends `untrusted`
    /// and the plugin may read the hashes with `peer` meanwhile.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn untrusted(&self, id: u32, why: UntrustedWhy, peer: &PeerInfo) -> Result<(), i32> {
        let code = match why {
            UntrustedWhy::NotPinned => tls_code::NOT_PINNED,
            _ => tls_code::UNKNOWN_ISSUER,
        };
        self.0.made().map_or_else(
            || self.0.ignored(),
            |d| sock_service::untrusted(d, id, code, peer),
        )
    }

    /// The handshake of socket `id` failed.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn tls_failed(&self, id: u32, how: TlsFailure) -> Result<(), i32> {
        let code = match how {
            TlsFailure::Name => tls_code::NAME,
            TlsFailure::Expired => tls_code::EXPIRED,
            TlsFailure::NoMemory => tls_code::NO_MEMORY,
            _ => tls_code::TLS,
        };
        self.0.made().map_or_else(
            || self.0.ignored(),
            |d| sock_service::tls_failed(d, id, code),
        )
    }

    /// Time passed: connect, idle and handshake times run out, waiting
    /// sockets start.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn tick(&self) -> Result<(), i32> {
        self.0.made().map_or(Ok(()), |d| sock_service::tick(d))
    }

    /// The network went away or the device sleeps: every socket ends.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn network_lost(&self) -> Result<(), i32> {
        self.0
            .made()
            .map_or(Ok(()), |d| sock_service::network_lost(d))
    }

    /// Broadcast answers dropped as malformed since the last call (the shell
    /// logs the count once per run, never the contents).
    #[must_use]
    pub fn take_dropped(&self) -> u32 {
        self.0.made().map_or(0, |d| d.table().take_dropped())
    }

    /// Reports for an unknown, ended or not-handshaking socket since the last
    /// call (logged the same way).
    #[must_use]
    pub fn take_ignored(&self) -> u32 {
        let early = self.0.ignored.replace(0);
        early.saturating_add(self.0.made().map_or(0, |d| d.table().take_ignored()))
    }
}
