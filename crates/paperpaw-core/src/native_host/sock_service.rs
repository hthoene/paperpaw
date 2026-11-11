//! Host side of the service `core.sockets` (`docs/abi/sockets.md`), first
//! slice: plain TCP sockets and the event 106.
//!
//! The device-independent part is here: the admission of every op (the
//! records and their fields, the limits per plugin and per device), the
//! socket table (ids, owners, the pool charge, the send queue, one read at a
//! time, the idle time) and the routing of `event.socket` from that table.
//! The network itself is behind [`SockTransport`] and the device's other
//! services behind [`SockHost`]; a shell wires both once, the tests wire
//! fakes.
//!
//! # TLS
//! `connect` with the `tls` flag, `start_tls` and `peer` (`sockets.md` 2.4)
//! need the shell's TLS session and trust store, behind [`SockTls`]. A shell
//! without it leaves all three `ERR_UNSUPPORTED`. The service owns the rest:
//! the one session is a lease in the shared `NetPool` (so `core.net` and
//! sockets are served first come, first served, one holder at a time), the
//! `busy` and `session_needed` rules, the state of the socket and the
//! cleanup on every end. The shell does the handshake, the trust and name
//! checks and the dates, and reports the outcome from its loop through
//! [`secured`], [`untrusted`] and [`tls_failed`].
//!
//! # Reads to a file
//! `read` mode 1 writes exactly `len` bytes to the staged file of `path` (`<path>.pp-staged`) and renames it
//! over `path` when whole, then tells `saved` (`sockets.md` 2.3). The card is
//! behind [`SockHost`] (the roots and path rules of `files.md`, the paths in
//! use, the free-space reserve, the staged file); the service owns the
//! sequence: the read part of the socket is the write buffer, each report is
//! written at once, and the socket ending first (closed early, a full or
//! failing card, the idle time, `close`, the screen, the run, the plugin
//! stopping) removes the staged file, leaves the old file as it was and
//! gives the claim back. A power cut leaves a staged file only; the target
//! is never half written.
//!
//! # Rules the table keeps
//! * A socket ends exactly once, with one `closed` or one `failed`; after it
//!   every call that names the id is `ERR_NOT_FOUND`. Events are never
//!   dropped: they live in the socket's own slots (a connect, a read, a
//!   drained queue, an end), `sent` is coalesced.
//! * Nothing arrives unasked: the transport may report bytes only for the
//!   read the plugin asked for, and at most that read's bytes are held.
//! * The pool charge (the send queue and the read part) is made at `connect`
//!   and given back exactly when the socket's buffers go. It never grows, so
//!   `send` and `read` never fail for memory.
//! * Nothing logs, and nothing keeps socket data beyond the queue and the
//!   read part.
//!
//! # Memory
//! Per socket: a small record, its host name, the send queue and the read
//! part (the profile's `queue` and `read`, 4 KiB each on the X4), all charged
//! to the device's network pool ([`NetPool`], shared with `core.net`). Every
//! allocation is fallible.
use super::{
    host_services::Caller,
    net_service::PowerEnd,
    registrations::{self as wire, Record},
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

pub(crate) mod device;
mod table;
#[cfg(test)]
mod tests;

pub(crate) use table::SockTable;

/// `event.socket.kind` (`sockets.md` 4).
pub(crate) mod kind {
    pub(crate) const CONNECTED: u32 = 0;
    pub(crate) const SECURED: u32 = 1;
    pub(crate) const UNTRUSTED: u32 = 2;
    pub(crate) const RECEIVED: u32 = 3;
    pub(crate) const SAVED: u32 = 4;
    pub(crate) const SENT: u32 = 5;
    pub(crate) const CLOSED: u32 = 6;
    pub(crate) const FAILED: u32 = 7;
}

/// `event.socket.reason` (`sockets.md` 4).
pub(crate) mod reason {
    pub(crate) const NONE: u32 = 0;
    pub(crate) const NO_NETWORK: u32 = 1;
    pub(crate) const DNS: u32 = 2;
    pub(crate) const TLS: u32 = 3;
    pub(crate) const TIMEOUT: u32 = 4;
    pub(crate) const REFUSED: u32 = 5;
    pub(crate) const IO: u32 = 9;
    pub(crate) const FULL: u32 = 10;
    pub(crate) const OFFLINE: u32 = 11;
    pub(crate) const SAVING: u32 = 12;
    pub(crate) const NETWORK_OFF: u32 = 13;
    pub(crate) const BUSY: u32 = 20;
    pub(crate) const SESSION_NEEDED: u32 = 21;
    pub(crate) const LOST: u32 = 22;
    pub(crate) const NAME: u32 = 23;
    pub(crate) const EXPIRED: u32 = 24;
    pub(crate) const CLOSED_EARLY: u32 = 25;
    pub(crate) const SCREEN_CLOSED: u32 = 26;
    pub(crate) const WORK_ENDED: u32 = 27;
    pub(crate) const NO_MEMORY: u32 = 28;
}

/// Open sockets per plugin (`sockets.md` 1).
pub(crate) const PLUGIN_SOCKETS: usize = 2;
/// Sockets of one plugin, ended ones whose last event is not yet told
/// included: a plugin that loops in one callback cannot pile them up.
pub(crate) const PLUGIN_ENTRIES: usize = 8;
/// The longest `send` piece and `data` piece (`sockets.md` 2.2, 2.3).
pub(crate) const PIECE: usize = 3_840;
/// Defaults and limits of `connect` (2.1), milliseconds.
pub(crate) const TIMEOUT_DEFAULT_MS: u32 = 20_000;
pub(crate) const TIMEOUT_MAX_MS: u32 = 60_000;
pub(crate) const IDLE_DEFAULT_MS: u32 = 60_000;
pub(crate) const IDLE_MAX_MS: u32 = 1_800_000;
/// Waiting for a network before `connect` fails (`network.md` 3.2).
pub(crate) const NO_NETWORK_MS: u64 = 30_000;
/// The largest single `read` (2.3).
pub(crate) const READ_MAX: u32 = 16_384;

/// What the device lets sockets use (`sockets.md` section 6): a host-profile
/// value, never a constant of the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) struct SockProfile {
    /// Open sockets at once on the device.
    pub(crate) sockets: u32,
    /// `SOCK_QUEUE`: the send queue of a socket, bytes.
    pub(crate) queue: u32,
    /// `SOCK_READ`: the read part of a socket, bytes.
    pub(crate) read: u32,
}

impl SockProfile {
    /// A profile of `sockets` open sockets with a send queue of `queue` and a
    /// read part of `read` bytes each.
    pub(crate) const fn new(sockets: u32, queue: u32, read: u32) -> Self {
        Self {
            sockets,
            queue,
            read,
        }
    }

    /// The X4: two sockets, 4 KiB each for the queue and the read part.
    pub(crate) const X4: Self = Self {
        sockets: 2,
        queue: 4_096,
        read: 4_096,
    };
    /// Other devices.
    pub(crate) const OTHER: Self = Self {
        sockets: 8,
        queue: 16_384,
        read: 16_384,
    };

    /// The bytes one socket charges to the pool at `connect`.
    pub(crate) const fn charge(&self) -> u32 {
        self.queue.saturating_add(self.read)
    }
}

/// What a socket gives the transport when it starts to connect.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Connect<'a> {
    /// A name of letters, digits, `-` and `.`, or an IPv4 or bracketed IPv6
    /// address (the service has checked it).
    pub host: &'a str,
    /// 1-65535.
    pub port: u32,
    /// Lookup and connect together; the service ends the socket after it.
    pub timeout_ms: u32,
}

/// What the sockets of a shell do. The service owns every limit; the
/// transport moves bytes.
///
/// A transport never calls back into the service from inside these methods:
/// it reports from the shell's loop, through `connected`, `received`,
/// `peer_closed`, `writable` and `failed`.
pub trait SockTransport {
    /// Begin to connect socket `id` (lookup, connect). `Err` means it could
    /// not begin.
    fn connect(&self, id: u32, to: &Connect<'_>) -> Result<(), i32>;
    /// Hand up to `data.len()` bytes to the connection; how many it took
    /// (0: it is full, `writable` says when to try again). `Err` ends the
    /// socket (`io`).
    fn send(&self, id: u32, data: &[u8]) -> Result<usize, i32>;
    /// Allow the transport to report up to `max` more bytes through
    /// `received` (and nothing beyond: nothing arrives unasked).
    fn read(&self, id: u32, max: u32);
    /// Close the connection of `id` (a socket or a broadcast) without any
    /// report.
    fn close(&self, id: u32);
    /// Send `data` from a port of its own to the network's broadcast address
    /// on each of `ports` and report the answers through `answered`. `Err`
    /// means it could not begin.
    fn broadcast(&self, id: u32, data: &[u8], ports: &[u16]) -> Result<(), i32>;
}

/// A pinned hash (`sockets.md` 2.4): `kind` 0 the SHA-256 of the leaf
/// certificate, 1 that of its SubjectPublicKeyInfo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TlsPin {
    /// 0: the SHA-256 of the leaf certificate's DER; 1: that of its
    /// SubjectPublicKeyInfo.
    pub kind: u32,
    /// The hash.
    pub hash: [u8; 32],
}

impl TlsPin {
    /// A pin of `kind` (see the field).
    #[must_use]
    pub const fn new(kind: u32, hash: [u8; 32]) -> Self {
        Self { kind, hash }
    }
}

/// What a socket gives the shell's TLS when its handshake begins.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct TlsOpen<'a> {
    /// The name the certificate must match.
    pub server_name: &'a str,
    /// At most 4 pins; with pins only the leaf is checked, against them.
    pub pins: &'a [TlsPin],
}

/// What the handshake learned about the peer (`sock_peer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PeerInfo {
    /// SHA-256 of the leaf certificate's DER.
    pub certificate: [u8; 32],
    /// SHA-256 of its SubjectPublicKeyInfo DER.
    pub key: [u8; 32],
    /// The dates were checked against network time.
    pub time_checked: bool,
}

impl PeerInfo {
    /// What the handshake learned about the peer.
    #[must_use]
    pub const fn new(certificate: [u8; 32], key: [u8; 32], time_checked: bool) -> Self {
        Self {
            certificate,
            key,
            time_checked,
        }
    }
}

/// What the codes the shell reports mean (anything else is `failed` `tls`).
pub(crate) mod tls_code {
    /// `untrusted`: no root of the store vouches for the chain.
    pub(crate) const UNKNOWN_ISSUER: u32 = 0;
    /// `untrusted`: no pin matched the leaf.
    pub(crate) const NOT_PINNED: u32 = 1;
    /// `tls_failed`: the name does not match.
    pub(crate) const NAME: u32 = 2;
    /// `tls_failed`: the certificate has expired (or is not yet valid).
    pub(crate) const EXPIRED: u32 = 3;
    /// `tls_failed`: any other TLS failure.
    pub(crate) const TLS: u32 = 4;
    /// `tls_failed`: the shell has no memory for the session.
    pub(crate) const NO_MEMORY: u32 = 5;
}

/// The TLS of a shell: the handshake over a socket's connection. Once a
/// socket is secured the transport's `send`, `read` and `close` carry the
/// encrypted bytes; the service never sees them in clear on the wire.
///
/// The shell never reports from inside these methods, only from its loop.
pub trait SockTls {
    /// Begin the handshake on the open connection of socket `id`, which
    /// holds the device's one session now. `Err(ERR_NO_MEMORY)` ends the
    /// socket `failed` `no_memory`; `Err(ERR_STATE)` says bytes from the
    /// peer arrived in clear before the switch (a STARTTLS injection) and
    /// ends it `failed` `tls`; any other `Err` ends it `failed` `tls`.
    fn begin(&self, id: u32, to: &TlsOpen<'_>) -> Result<(), i32>;
    /// Drop the session of `id`, whatever its state, without any report.
    /// Idempotent.
    fn end(&self, id: u32);
}

/// What the device provides around the service.
pub(crate) trait SockHost {
    fn table(&self) -> &SockTable;
    fn transport(&self) -> &dyn SockTransport;
    /// The shell's TLS; `None` on a shell without it (the TLS ops are then
    /// `ERR_UNSUPPORTED`).
    fn tls(&self) -> Option<&dyn SockTls> {
        None
    }
    fn profile(&self) -> SockProfile;
    fn now_ms(&self) -> u64;
    /// Whether the plugin subscribed to the event `tag`.
    fn subscribed(&self, owner: u64, tag: u32) -> bool;
    /// A socket or broadcast of work run `run` was stored: the run does not
    /// end while it lives (`work.md` 2.3). Called after it was admitted, once.
    ///
    /// # Errors
    /// `ERR_BUSY` (the scheduler is in use), `ERR_FULL`, `ERR_NOT_FOUND`: the
    /// socket is refused with it and nothing is stored.
    fn resource_opened(&self, run: u64) -> Result<(), i32>;
    /// The socket or broadcast of work run `run` ended (any way): exactly once
    /// for each `resource_opened`, and never allocating.
    fn resource_closed(&self, run: u64);
    /// Power saving (`network.md` 3.2): `Some` ends the socket now. `background`
    /// is set for a socket of a work run.
    fn power_end(&self, owner: u64, background: bool) -> Option<PowerEnd>;
    /// Whether a network is up.
    fn network_up(&self) -> bool;
    /// `true`: ask the network manager for a network; `false`: no request,
    /// server or socket is left. Core combines `core.net`'s and the sockets'
    /// demand (`NetPool::demand`): this is called only when the OR changes.
    fn network_demand(&self, wanted: bool);
    /// The work run the current callback of the plugin belongs to.
    fn current_run(&self, owner: u64) -> Option<u64>;
    /// The root's access for this caller and the path rules of `files.md`
    /// section 3 for a write (root 1 needs `uses.storage`, its exclusions,
    /// `.received/` is the host's): `Ok`, or `ERR_DENIED` / `ERR_INVALID`.
    fn root_access(&self, who: &Caller<'_>, root: u32, path: &str) -> Result<(), i32>;
    /// Whether `extra` more bytes can be written and the storage's free-space
    /// reserve (D2.6) is kept.
    fn room_for(&self, extra: u64) -> bool;
    /// Put the target and its `.pp-staged` file in the paths-in-use table:
    /// `ERR_BUSY` when anyone holds either, the same plugin's own NET request
    /// included. Unlike NET's counted claims, this one is exclusive.
    fn claim_save(&self, owner: u64, root: u32, path: &str) -> Result<(), i32>;
    /// Remove the staged file (best effort) and free the claim.
    fn release_save(&self, owner: u64, root: u32, path: &str);
    /// The plugin started: a staged file kept after a failed rename is tried
    /// again.
    fn recover_saves(&self, _owner: u64) {}
    /// Append `data` to the staged file of `path` (`<path>.pp-staged`); `first` creates it empty first (an
    /// older staged file goes). Whole or an error.
    fn staged_write(
        &self,
        owner: u64,
        root: u32,
        path: &str,
        data: &[u8],
        first: bool,
    ) -> Result<(), i32>;
    /// Rename the staged file of `path` (`<path>.pp-staged`) over `path`.
    fn staged_commit(&self, owner: u64, root: u32, path: &str) -> Result<(), i32>;
    /// The screen instance whose callback is running if it is showing.
    fn showing_screen(&self, owner: u64) -> Option<u64>;
}

/// What the shell hands the frame under the key `core.sockets`.
pub(crate) struct SockServices(pub(crate) Rc<dyn SockHost>);

/// `core.sockets` ops (`sockets.md` 2).
mod op {
    pub(super) const CONNECT: u32 = 1;
    pub(super) const SEND: u32 = 2;
    pub(super) const READ: u32 = 3;
    pub(super) const DATA: u32 = 4;
    pub(super) const START_TLS: u32 = 5;
    pub(super) const PEER: u32 = 6;
    pub(super) const CLOSE: u32 = 7;
    pub(super) const BROADCAST: u32 = 8;
}

/// The `core.sockets` backend.
pub(crate) fn backend(
    _frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let host = &*who.hosts.sockets().ok_or(ERR_UNSUPPORTED)?.0;
    match op.id {
        op::CONNECT => {
            let socket = table::connect(host, who, input)?;
            codec::encode_one(
                &Record::ServiceSockStarted(wire::ServiceSockStarted { socket }),
                out,
            )
        }
        op::SEND => {
            let s: wire::ServiceSockSend<'_> = codec::single(input)?;
            table::send(host, who.owner, &s).map(|()| 0)
        }
        op::READ => {
            let r: wire::ServiceSockRead<'_> = codec::single(input)?;
            table::read(host, who, &r).map(|()| 0)
        }
        op::DATA => {
            let a: wire::ServiceSockAt = codec::single(input)?;
            table::data(host, who.owner, &a, out)
        }
        op::CLOSE => {
            let r: wire::ServiceSockRef = codec::single(input)?;
            table::close(host, who.owner, r.socket).map(|()| 0)
        }
        op::BROADCAST => {
            let id = table::broadcast(host, who, input)?;
            codec::encode_one(
                &Record::ServiceSockStarted(wire::ServiceSockStarted { socket: id }),
                out,
            )
        }
        op::START_TLS => table::start_tls(host, who, input).map(|()| 0),
        op::PEER => {
            let r: wire::ServiceSockRef = codec::single(input)?;
            table::peer(host, who.owner, &r, out)
        }
        _ => Err(ERR_UNSUPPORTED),
    }
}

#[allow(
    unused_imports,
    reason = "the shell's hooks into the table; tests use them"
)]
pub(crate) use table::{
    answered, connected, event_delivered, failed, network_lost, next_deadline, next_event,
    next_event_run, peer_closed, received, screen_closed, secured, tick, tls_failed, untrusted,
    work_ended, writable,
};

/// The plugin started: a staged file kept after a failed rename comes back.
pub(crate) fn owner_started(host: &dyn SockHost, owner: u64) {
    host.recover_saves(owner);
}

/// `connect` as a plugin calls it (a test's way in from the wiring).
///
/// # Errors
/// What `connect` answers (`sockets.md` 2.1).
#[cfg(test)]
pub(crate) fn admit(host: &dyn SockHost, who: &Caller<'_>, input: &[u8]) -> Result<u32, i32> {
    table::connect(host, who, input)
}

/// A connect as the dispatcher admits it, for the tests of the work pump.
#[cfg(test)]
pub(crate) fn connect_for_tests(
    host: &dyn SockHost,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<u32, i32> {
    table::connect(host, who, input)
}

/// The plugin stopped (its callbacks are revoked): its sockets close without
/// events and the pool gets everything back. Idempotent.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed: call again.
pub(crate) fn owner_stopped(host: &dyn SockHost, owner: u64) -> Result<(), i32> {
    table::owner_stopped(host, owner)
}
