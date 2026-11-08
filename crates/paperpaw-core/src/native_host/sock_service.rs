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
//! # Not in this slice
//! Each of these answers `ERR_UNSUPPORTED` (`sockets.md` 5: a host without it)
//! until it lands, and none of them changes a wire record:
//! * TLS: `connect` with the `tls` flag or with pins, `start_tls`, `peer`
//!   (they need the shell's TLS session and trust store, section 2.4);
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

mod table;
#[cfg(test)]
mod tests;

pub(crate) use table::SockTable;

/// `event.socket.kind` (`sockets.md` 4).
pub(crate) mod kind {
    pub(crate) const CONNECTED: u32 = 0;
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
    pub(crate) const TIMEOUT: u32 = 4;
    pub(crate) const REFUSED: u32 = 5;
    pub(crate) const IO: u32 = 9;
    pub(crate) const FULL: u32 = 10;
    pub(crate) const OFFLINE: u32 = 11;
    pub(crate) const SAVING: u32 = 12;
    pub(crate) const NETWORK_OFF: u32 = 13;
    pub(crate) const LOST: u32 = 22;
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
pub(crate) struct Connect<'a> {
    pub(crate) host: &'a str,
    pub(crate) port: u32,
    pub(crate) timeout_ms: u32,
}

/// What the sockets of a shell do. The service owns every limit; the
/// transport moves bytes.
///
/// A transport never calls back into the service from inside these methods:
/// it reports from the shell's loop, through [`connected`], [`received`],
/// [`peer_closed`], [`writable`] and [`failed`].
pub(crate) trait SockTransport {
    /// Begin to connect socket `id` (lookup, connect). `Err` means it could
    /// not begin.
    fn connect(&self, id: u32, to: &Connect<'_>) -> Result<(), i32>;
    /// Hand up to `data.len()` bytes to the connection; how many it took
    /// (0: it is full, [`writable`] says when to try again). `Err` ends the
    /// socket (`io`).
    fn send(&self, id: u32, data: &[u8]) -> Result<usize, i32>;
    /// Allow the transport to report up to `max` more bytes through
    /// [`received`] (and nothing beyond: nothing arrives unasked).
    fn read(&self, id: u32, max: u32);
    /// Close the connection of `id` (a socket or a broadcast) without any
    /// report.
    fn close(&self, id: u32);
    /// Send `data` from a port of its own to the network's broadcast address
    /// on each of `ports` and report the answers through [`answered`]. `Err`
    /// means it could not begin.
    fn broadcast(&self, id: u32, data: &[u8], ports: &[u16]) -> Result<(), i32>;
}

/// What the device provides around the service.
pub(crate) trait SockHost {
    fn table(&self) -> &SockTable;
    fn transport(&self) -> &dyn SockTransport;
    fn profile(&self) -> SockProfile;
    fn now_ms(&self) -> u64;
    /// Whether the plugin subscribed to the event `tag`.
    fn subscribed(&self, owner: u64, tag: u32) -> bool;
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
        // A later slice: TLS (`start_tls`, `peer`).
        op::START_TLS | op::PEER => Err(ERR_UNSUPPORTED),
        _ => Err(ERR_UNSUPPORTED),
    }
}

#[allow(
    unused_imports,
    reason = "the shell's hooks into the table; tests use them"
)]
pub(crate) use table::{
    answered, connected, event_delivered, failed, network_lost, next_event, peer_closed, received,
    screen_closed, tick, work_ended, writable,
};

/// The plugin stopped (its callbacks are revoked): its sockets close without
/// events and the pool gets everything back. Idempotent.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed: call again.
pub(crate) fn owner_stopped(host: &dyn SockHost, owner: u64) -> Result<(), i32> {
    table::owner_stopped(host, owner)
}
