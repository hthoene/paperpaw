//! Host side of the service `core.net` (`docs/abi/network.md`): HTTP
//! requests, serving HTTP and the events 99-102.
//!
//! The device-independent part is here: the admission of every op (the
//! records and their fields, the limits, the roots, the power ends), the
//! request table (ids, tokens, a strictly first-in-first-out queue, the
//! network pool, the answer hold), the server table (screen ties, uploads
//! under `.received/<server>/`, the 120 s answer time) and the routing of the
//! events from those tables. The network itself is behind [`NetTransport`]
//! (HTTP client and server, TLS) and the device's other services behind
//! [`NetHost`]; a shell wires both once, the tests wire fakes.
//!
//! # Rules the table keeps
//! * Every request ends exactly once: the id is `Running` or `Queued` until
//!   one `end` turns it into a finished entry; a second `end` finds nothing
//!   to end. That entry is told to the plugin as one `event.http_done` and
//!   freed when the callback returns ([`event_delivered`]).
//! * The queue is strict first in, first out. The head waits for the pool,
//!   for a memory body slot and for a connection; nothing behind it starts.
//! * The pool is reserved before use and only ever shrinks while a request
//!   lives. A call that is not a request (a held `respond` body, an upload
//!   buffer) that finds no room is `ERR_NO_MEMORY` and holds nothing.
//! * Nothing here logs, and nothing keeps a URL, a header or a body beyond
//!   the request's own batch (at most 4,096 bytes) and its answer.
//!
//! # Memory
//! Per request: its batch (at most 4,096 bytes, the plugin's own input,
//! usually a few hundred) and a small record; 16 requests per device. The
//! answers and the buffers are in the pool; the transport's connections and
//! the TLS session (about 40 KiB, one fixed block) are outside it. Every
//! allocation is fallible.
use super::{
    host_services::Caller,
    registrations::{self as wire, Record},
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use alloc::{rc::Rc, string::String};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_UNSUPPORTED};

pub(crate) mod args;
pub(crate) mod cable_link;
mod device;
mod serving;
mod table;
#[cfg(test)]
mod tests;

pub(crate) use device::NetDevice;
pub(crate) use table::NetTable;

/// `event.http_done.reason` (`network.md` 4.1).
pub(crate) mod reason {
    pub(crate) const OK: u32 = 0;
    pub(crate) const NO_NETWORK: u32 = 1;
    pub(crate) const DNS: u32 = 2;
    pub(crate) const TLS: u32 = 3;
    pub(crate) const TIMEOUT: u32 = 4;
    pub(crate) const REFUSED: u32 = 5;
    pub(crate) const TOO_LARGE: u32 = 6;
    pub(crate) const REPLACED: u32 = 7;
    pub(crate) const CANCELLED: u32 = 8;
    pub(crate) const IO: u32 = 9;
    pub(crate) const FULL: u32 = 10;
    pub(crate) const OFFLINE: u32 = 11;
    pub(crate) const SAVING: u32 = 12;
    pub(crate) const NETWORK_OFF: u32 = 13;
    pub(crate) const WORK_ENDED: u32 = 14;
}

/// `event.http_done.flags`.
pub(crate) const DONE_LATER: u32 = 1;
pub(crate) const DONE_TRUNCATED: u32 = 2;
pub(crate) const DONE_SAVED: u32 = 4;

/// Requests not yet ended: per plugin and per device (`network.md` 2.1).
pub(crate) const PLUGIN_REQUESTS: usize = 4;
pub(crate) const DEVICE_REQUESTS: usize = 16;
/// Servers per plugin (5.1) and open served requests per server (5.2).
pub(crate) const PLUGIN_SERVERS: usize = 2;
pub(crate) const SERVER_OPEN: usize = 2;
/// The write buffer of a saving request and of an upload being received.
pub(crate) const IO_BUFFER: u32 = 4096;
/// The error body kept for a failed answer (2.3).
pub(crate) const ERROR_BODY: usize = 4096;
/// The longest `body` piece (2.3).
pub(crate) const BODY_PIECE: usize = 3840;
/// Milliseconds: waiting for a network (3.2), the default time of a request in
/// memory and of one with a file without a byte moving (2.1), the time a
/// plugin has to answer a served request (5.2).
pub(crate) const NO_NETWORK_MS: u64 = 30_000;
pub(crate) const MEMORY_TIMEOUT_MS: u32 = 30_000;
pub(crate) const FILE_TIMEOUT_MS: u32 = 60_000;
pub(crate) const ANSWER_MS: u64 = 120_000;

/// What the device lets the network use (`network.md` 3.4): a host-profile
/// value, never a constant of the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NetProfile {
    /// `NET_POOL`: the device-wide bytes of plugin network memory; 0 means
    /// no pool (the per-item caps and `memory_bodies` bound memory).
    pub(crate) pool: u32,
    /// `MEMORY_BODIES`: memory bodies existing at once, arriving or not yet
    /// delivered.
    pub(crate) memory_bodies: u32,
    /// Connections running at once; the rest wait in the queue.
    pub(crate) connections: u32,
    /// The default and the largest `max_body` of a request.
    pub(crate) max_body: u32,
    /// Whether the cable server (`serve` port 0) exists.
    pub(crate) cable: bool,
    /// `SERVERS`: servers open at once over all plugins, from `serve` until
    /// they stop, fail or end (1 on the X4, 2 on the X4 Pro and Classic, 8
    /// elsewhere). A `serve` past it answers `ERR_BUSY` and opens nothing.
    pub(crate) servers: u32,
}

impl NetProfile {
    /// The X4: a 48 KiB pool until Firmware has measured, two memory bodies,
    /// two connections; the default and largest `max_body` leave room in the
    /// pool for one request's input (4 KiB).
    pub(crate) const X4: Self = Self {
        pool: 49_152,
        memory_bodies: 2,
        connections: 2,
        max_body: 45_056,
        cable: true,
        servers: 1,
    };
    /// Devices without a pool (Android, the emulator).
    pub(crate) const OTHER: Self = Self {
        pool: 0,
        memory_bodies: 8,
        connections: 6,
        max_body: 1_048_576,
        cable: false,
        servers: 8,
    };
}

/// A power end of a request or a server, told before any byte goes out
/// (`network.md` 3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PowerEnd {
    /// 11 `offline`, 12 `saving` or 13 `network_off`.
    pub(crate) reason: u32,
    /// Set `later` on the event (try again once online).
    pub(crate) later: bool,
}

/// What a request gives the transport when it starts.
pub(crate) struct Start<'a> {
    /// The plugin that made the request (the transport runs outside its call).
    pub(crate) owner: u64,
    pub(crate) request: &'a args::Request<'a>,
    pub(crate) max_body: u32,
    pub(crate) max_file: u64,
    pub(crate) timeout_ms: u32,
}

/// An answer to a served request.
pub(crate) struct Reply<'a> {
    /// The plugin that serves.
    pub(crate) owner: u64,
    pub(crate) response: &'a args::Response<'a>,
    /// The file to send exists (a missing one is answered 404 by the
    /// transport; the service already checked).
    pub(crate) file_len: Option<u64>,
}

/// Why a server could not start (`event.server.reason`).
pub(crate) type Refused = u32;

/// What the HTTP client and server of a shell do. The service owns every
/// limit; the transport moves bytes.
///
/// A transport never calls back into the service from inside these methods:
/// it reports progress and ends later, from the shell's loop, through
/// [`finished`], [`progress`], [`served_head`], [`served_body`],
/// [`served_whole`] and [`served_gone`].
pub(crate) trait NetTransport {
    /// Begin one request (connect, send, read). Its memory body is bounded
    /// by `max_body` (reserved from the pool already); a saving request writes
    /// `<path>.pp-staged` in 4 KiB appends and renames it over the target only when
    /// the answer is whole. `Err` means it could not begin.
    fn start(&self, id: u32, start: &Start<'_>) -> Result<(), i32>;
    /// Drop the transfer of `id` without any report; a saving request's
    /// `.pp-staged` file is removed by the service ([`NetHost::release_save`]).
    fn abort(&self, id: u32);
    /// Listen on `port` (0: the cable), trying the next 20 ports when it is
    /// taken. The address that was got (`""` for the cable) or the reason.
    fn listen(&self, server: u32, port: u32, address: &mut String) -> Result<(), Refused>;
    /// Stop listening; the connections of the server end.
    fn unlisten(&self, server: u32);
    /// Send the answer of a served request and close it.
    fn reply(&self, request: u32, reply: &Reply<'_>) -> Result<(), i32>;
    /// Close a served request without an answer, or with a status of the
    /// host's own (503 when the plugin was too slow).
    fn close_served(&self, request: u32, status: u32);
}

/// What the device provides around the service.
pub(crate) trait NetHost {
    fn table(&self) -> &NetTable;
    fn transport(&self) -> &dyn NetTransport;
    fn profile(&self) -> NetProfile;
    fn now_ms(&self) -> u64;
    /// The root's access for this caller and the path rules of `files.md`
    /// section 3 (root 1 needs `uses.storage`, its exclusions, root 2
    /// read-only): `Ok`, or `ERR_DENIED` / `ERR_INVALID`.
    fn root_access(&self, who: &Caller<'_>, root: u32, path: &str, write: bool) -> Result<(), i32>;
    /// Size of a file to send: `ERR_NOT_FOUND` if it does not exist.
    fn file_len(&self, who: &Caller<'_>, root: u32, path: &str) -> Result<u64, i32>;
    /// Whether two paths of the same root name the same file (case, 8.3
    /// aliases): the storage's identity, not the spelling.
    fn same_file(&self, root: u32, a: &str, b: &str) -> bool;
    /// Put the target and its `.pp-staged` file in the paths-in-use table. Claims
    /// are counted: the same plugin may claim a file it already has claimed (a
    /// request that replaces its older one claims first, so a refusal changes
    /// nothing), and each `release_save` gives one back.
    fn claim_save(&self, owner: u64, root: u32, path: &str) -> Result<(), i32>;
    /// Remove the `.pp-staged` file (best effort) and free the claim.
    fn release_save(&self, owner: u64, root: u32, path: &str);
    /// Whether the plugin subscribed to the event `tag`.
    fn subscribed(&self, owner: u64, tag: u32) -> bool;
    /// Power saving (3.2): `Some` ends the request or the server now.
    /// `background` is set for a request of a work run (no user-input
    /// window).
    fn power_end(&self, owner: u64, background: bool) -> Option<PowerEnd>;
    /// Whether a network is up (the cable counts for the cable server).
    fn network_up(&self) -> bool;
    /// `true`: ask the network manager for a network
    /// (`event.network_wanted`); `false`: the last request and server ended
    /// (`event.network_idle`).
    fn network_demand(&self, wanted: bool);
    /// The work run the current callback of the plugin belongs to.
    fn current_run(&self, owner: u64) -> Option<u64>;
    /// A request of work run `run` was stored: the run does not end while it
    /// lives (`work.md` 2.3). Called after the request was admitted, once.
    ///
    /// # Errors
    /// `ERR_BUSY` (the scheduler is in use), `ERR_FULL`, `ERR_NOT_FOUND`: the
    /// request is refused with it and nothing is stored.
    fn resource_opened(&self, run: u64) -> Result<(), i32>;
    /// The request of work run `run` ended (any way): exactly once for each
    /// `resource_opened`, and never allocating.
    fn resource_closed(&self, run: u64);
    /// The screen instance whose callback is running if it is showing.
    fn showing_screen(&self, owner: u64) -> Option<u64>;
    /// The topmost screen instance of the owner when it is showing, for a
    /// `serve` from a callback that is no screen's (`network.md` 5.1). The
    /// default knows none.
    fn topmost_screen(&self, _owner: u64) -> Option<u64> {
        None
    }
    /// Whether [`Self::current_run`] can tell now: `false` while the
    /// scheduler is in use or none is attached, and then a call outside a
    /// screen does not count as outside a run.
    fn run_known(&self) -> bool {
        true
    }
    /// Whether `extra` more bytes can be written and the storage's free-space
    /// reserve (D2.6) is kept.
    fn room_for(&self, extra: u64) -> bool;
    /// Empty a plugin's whole `.received/` (`server` `None`) or one server's
    /// folder.
    fn clear_received(&self, owner: u64, server: Option<u32>);
    /// Remove one received body, best effort.
    fn remove_received(&self, owner: u64, server: u32, request: u32);
}

/// What the shell hands the frame under the key `core.net`.
pub(crate) struct NetServices(pub(crate) NetRef);

/// The host behind `core.net`: one handed over (tests), or the device's own,
/// made fallibly at the service's first call (#248).
pub(crate) enum NetRef {
    Handed(Rc<dyn NetHost>),
    Made(super::shared::Shared<NetDevice>),
}

impl core::ops::Deref for NetRef {
    type Target = dyn NetHost;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Handed(host) => &**host,
            Self::Made(device) => &**device,
        }
    }
}

/// `core.net` ops (`network.md` 2).
mod op {
    pub(super) const REQUEST: u32 = 1;
    pub(super) const CANCEL: u32 = 2;
    pub(super) const ANSWER: u32 = 3;
    pub(super) const BODY: u32 = 4;
    pub(super) const SERVE: u32 = 5;
    pub(super) const STOP_SERVING: u32 = 6;
    pub(super) const RESPOND: u32 = 7;
}

/// The `core.net` backend.
pub(crate) fn backend(
    _frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let host = &*who.hosts.net_for_call()?.0;
    match op.id {
        op::REQUEST => {
            let id = table::request(host, who, input)?;
            reply(
                &Record::ServiceNetStarted(wire::ServiceNetStarted { id }),
                out,
            )
        }
        op::CANCEL => {
            let t: wire::ServiceNetToken = codec::single(input)?;
            table::cancel(host, who.owner, t.token).map(|()| 0)
        }
        op::ANSWER => {
            let i: wire::ServiceNetId = codec::single(input)?;
            table::answer(host, who.owner, i.id, out)
        }
        op::BODY => {
            let r: wire::ServiceNetRead = codec::single(input)?;
            table::body(host, who.owner, r.id, r.offset, out)
        }
        op::SERVE => {
            let s: wire::ServiceNetServe = codec::single(input)?;
            serving::serve(host, who, &s).map(|()| 0)
        }
        op::STOP_SERVING => {
            let t: wire::ServiceNetToken = codec::single(input)?;
            serving::stop_serving(host, who.owner, t.token).map(|()| 0)
        }
        op::RESPOND => serving::respond(host, who, input).map(|()| 0),
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// A bare reply: the one record's framed bytes, no count and no end.
fn reply(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    codec::encode_one(record, out)
}

// The test hook for allocation failures: the `n`th gate of a test that armed
// `tests::FAIL_NTH` answers `ERR_NO_MEMORY`. Every fallible allocation of this
// service goes through `gate` first.
#[cfg(test)]
pub(crate) fn gate() -> Result<(), i32> {
    tests::FAIL_NTH.with(|nth| match nth.get() {
        0 => Ok(()),
        1 => {
            nth.set(0);
            Err(ERR_NO_MEMORY)
        }
        n => {
            nth.set(n - 1);
            Ok(())
        }
    })
}

/// Makes the `n`th next [`gate`] (counting from 1) fail; 0 clears it.
#[cfg(test)]
pub(crate) fn fail_nth(n: u32) {
    tests::FAIL_NTH.with(|nth| nth.set(n));
}

#[cfg(not(test))]
#[inline]
pub(crate) const fn gate() -> Result<(), i32> {
    Ok(())
}

/// A fallible copy of a string (`ERR_NO_MEMORY`).
fn own(text: &str) -> Result<String, i32> {
    gate()?;
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(text);
    Ok(copy)
}

// The transport's reports and the shell's hooks. All of them are fallible
// only by `ERR_BUSY` (a table borrowed): the shell calls again.

#[allow(
    unused_imports,
    reason = "the shell's hooks into the tables; tests use them"
)]
pub(crate) use serving::{
    Decision, screen_closed, served_body, served_gone, served_head, served_whole, server_failed,
    server_owner, server_started,
};
#[allow(
    unused_imports,
    reason = "the shell's hooks into the tables; tests use them"
)]
pub(crate) use table::{
    Finished, event_delivered, finished, network_lost, next_deadline, next_event, next_event_run,
    progress, redirected_to_https, tick, work_ended,
};

/// A request as the dispatcher admits it, for the tests of the shell's wiring.
#[cfg(test)]
pub(crate) fn request_for_tests(
    host: &dyn NetHost,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<u32, i32> {
    table::request(host, who, input)
}

/// `serve`, `respond` and `stop_serving` as the dispatcher admits them, for
/// the tests of the cable's kernel side.
#[cfg(test)]
pub(crate) fn serve_for_tests(
    host: &dyn NetHost,
    who: &Caller<'_>,
    serve: &wire::ServiceNetServe,
) -> Result<(), i32> {
    serving::serve(host, who, serve)
}

/// See [`serve_for_tests`].
#[cfg(test)]
pub(crate) fn respond_for_tests(
    host: &dyn NetHost,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<(), i32> {
    serving::respond(host, who, input)
}

/// See [`serve_for_tests`].
#[cfg(test)]
pub(crate) fn stop_for_tests(host: &dyn NetHost, owner: u64, token: u32) -> Result<(), i32> {
    serving::stop_serving(host, owner, token)
}

/// The plugin stopped (its callbacks are revoked): its requests are
/// cancelled without events, its servers stopped without events, held
/// bodies freed, `.pp-staged` files removed and its `.received/` emptied.
/// Idempotent.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed: call again.
pub(crate) fn owner_stopped(host: &dyn NetHost, owner: u64) -> Result<(), i32> {
    table::owner_stopped(host, owner)
}

/// Whether `owner` has a request, a server, a served request or an event
/// not told yet (#248, D3).
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn owner_holds(host: &dyn NetHost, owner: u64) -> Result<bool, i32> {
    table::owner_holds(host, owner)
}

/// The plugin started: whatever a power cut left in its `.received/` goes.
pub(crate) fn owner_started(host: &dyn NetHost, owner: u64) {
    host.clear_received(owner, None);
}
