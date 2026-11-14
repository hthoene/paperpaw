//! What a shell gives `core.net` (`docs/abi/network.md`), on plain types: its
//! HTTP client and server ([`ShellHttp`]) and the state of its network, its
//! clock and its power ([`ShellNetwork`], which `core.sockets` will use as
//! well).
//!
//! Core owns every rule of the service (limits, the queue, the pool, roots and
//! paths, events, the staged file of a saved answer); the shell only moves
//! bytes. A request arrives as an [`HttpJob`] with storage paths already
//! joined, and ends with one report through [`NetReports`] from the shell's
//! own loop, never from inside a [`ShellHttp`] method. `ERR_BUSY` from a
//! report means the table is borrowed: call again on the next turn of the
//! loop, never spin in the same one.
use super::*;
use crate::{
    native_host::registrations::ServiceNetHeader,
    native_host::{
        net_policy::NetPolicy,
        net_pool::NetPool,
        net_service::{self, NetDevice, NetProfile},
        owner_directory::OwnerDirectory,
        paths_in_use::PathsInUse,
        shared::Shared,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::ERR_NOT_FOUND;

/// The headers of a request or an answer, in order, as `(name, value)`.
#[derive(Clone)]
pub struct HttpHeaders<'a> {
    items: &'a [Option<ServiceNetHeader<'a>>],
    at: usize,
}

/// Header values carry credentials: only the number of headers is shown.
impl core::fmt::Debug for HttpHeaders<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpHeaders")
            .field("count", &self.clone().count())
            .finish_non_exhaustive()
    }
}

impl<'a> HttpHeaders<'a> {
    pub(crate) fn new(items: &'a [Option<ServiceNetHeader<'a>>]) -> Self {
        Self { items, at: 0 }
    }
}

impl<'a> Iterator for HttpHeaders<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.items.get(self.at)?.as_ref()?;
        self.at += 1;
        Some((item.name, item.value))
    }
}

/// One request for the shell's HTTP client. Its memory body is at most
/// `max_body` bytes. A saving request writes `save_path`, the **staged** path
/// core chose (the target's name and `.pp-staged`), in 4 KiB appends; core
/// renames it over the target when the answer is whole and removes it when the
/// request ends otherwise or is aborted, so the shell never writes a final
/// name. Core has **created the staged file empty** before `start` (whatever a
/// power cut left at that name is gone): the shell only appends, never
/// truncates and never creates.
#[derive(Clone)]
#[non_exhaustive]
pub struct HttpJob<'a> {
    /// The id the shell reports under ([`NetReports::finished`]).
    pub id: u32,
    pub method: &'a str,
    pub url: &'a str,
    pub headers: HttpHeaders<'a>,
    /// A body sent from memory.
    pub body: Option<&'a [u8]>,
    /// A storage path of a file sent as the body.
    pub send_path: Option<&'a str>,
    /// The staged storage path the answer is written to.
    pub save_path: Option<&'a str>,
    pub max_body: u32,
    pub max_file: u64,
    pub timeout_ms: u32,
}

/// The URL, headers and body are the plugin's data (they can hold
/// credentials): they are not printed.
impl core::fmt::Debug for HttpJob<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpJob")
            .field("id", &self.id)
            .field("method", &self.method)
            .field("max_body", &self.max_body)
            .field("max_file", &self.max_file)
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

impl<'a> HttpJob<'a> {
    #[allow(clippy::too_many_arguments, reason = "one job, built in one place")]
    pub(crate) fn new(
        id: u32,
        method: &'a str,
        url: &'a str,
        headers: HttpHeaders<'a>,
        body: Option<&'a [u8]>,
        send_path: Option<&'a str>,
        save_path: Option<&'a str>,
        max_body: u32,
        max_file: u64,
        timeout_ms: u32,
    ) -> Self {
        Self {
            id,
            method,
            url,
            headers,
            body,
            send_path,
            save_path,
            max_body,
            max_file,
            timeout_ms,
        }
    }
}

/// The answer to a request the shell's server received.
#[derive(Clone)]
#[non_exhaustive]
pub struct ServedReply<'a> {
    pub status: u32,
    pub content_type: &'a str,
    pub headers: HttpHeaders<'a>,
    pub body: Option<&'a [u8]>,
    /// A storage path of the file sent as the body.
    pub send_path: Option<&'a str>,
    /// Its size, where core has checked it.
    pub file_len: Option<u64>,
}

/// Headers and body are the plugin's data: they are not printed.
impl core::fmt::Debug for ServedReply<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServedReply")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl<'a> ServedReply<'a> {
    pub(crate) fn new(
        status: u32,
        content_type: &'a str,
        headers: HttpHeaders<'a>,
        body: Option<&'a [u8]>,
        send_path: Option<&'a str>,
        file_len: Option<u64>,
    ) -> Self {
        Self {
            status,
            content_type,
            headers,
            body,
            send_path,
            file_len,
        }
    }
}

/// Why a server could not start (`event.server.reason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerReason {
    /// No network is up.
    NoNetwork,
    /// The device is offline by the user's choice.
    Offline,
    /// Power saving holds the radio off.
    Saving,
    /// The user turned this plugin's network off.
    NetworkOff,
    /// The port (and the 20 after it) could not be had, or the address did not
    /// fit.
    Refused,
    /// The listener could not be made for lack of memory (reported by
    /// [`NetReports::server_failed`]; the plugin is told `refused`).
    NoMemory,
    /// The listener could not be opened for another reason (reported by
    /// [`NetReports::server_failed`]; the plugin is told `refused`).
    Failed,
}

/// What the device lets the network use (`network.md` 3.4): numbers, picked by
/// the shell from its profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NetLimits {
    /// The device-wide bytes of plugin network memory; 0: no pool.
    pub pool_bytes: u32,
    /// Connections running at once; the rest wait in the queue.
    pub connections: u32,
    /// Memory bodies existing at once.
    pub memory_bodies: u32,
    /// The default and the largest `max_body` of a request.
    pub max_body: u32,
    /// Whether the cable server (`serve` port 0) exists.
    pub cable: bool,
    /// Servers open at once over all plugins (`SERVERS`, `network.md` 5.1):
    /// 1 on the X4, 2 on the X4 Pro and Classic, 8 elsewhere. `serve` past it
    /// is `ERR_BUSY`. A plugin has at most 2 in any case.
    pub servers: u32,
    /// The TLS sessions the device has at once, shared with `core.sockets`
    /// (the X4: 1; at most 4). https requests and secure sockets take them
    /// in the order they ask.
    pub tls_sessions: u8,
}

impl NetLimits {
    /// A pool of `pool_bytes` and `connections`; the rest follows the pool
    /// (2 memory bodies and a `max_body` that leaves 4 KiB of the pool for a
    /// request's input, or 8 and 1 MiB without a pool) and is set with the
    /// methods below.
    #[must_use]
    pub fn new(pool_bytes: u32, connections: u32) -> Self {
        let pooled = pool_bytes > 0;
        Self {
            pool_bytes,
            connections,
            memory_bodies: if pooled { 2 } else { 8 },
            max_body: if pooled {
                pool_bytes.saturating_sub(4096)
            } else {
                1_048_576
            },
            cable: false,
            servers: if pooled { 1 } else { 8 },
            tls_sessions: 1,
        }
    }

    /// Servers open at once over all plugins (`SERVERS`).
    #[must_use]
    pub fn servers(mut self, servers: u32) -> Self {
        self.servers = servers;
        self
    }

    /// The TLS sessions the device has at once (1 to 4).
    #[must_use]
    pub fn tls_sessions(mut self, sessions: u8) -> Self {
        self.tls_sessions = sessions;
        self
    }

    /// Memory bodies existing at once.
    #[must_use]
    pub fn memory_bodies(mut self, bodies: u32) -> Self {
        self.memory_bodies = bodies;
        self
    }

    /// The default and the largest `max_body`.
    #[must_use]
    pub fn max_body(mut self, bytes: u32) -> Self {
        self.max_body = bytes;
        self
    }

    /// Whether the cable server exists: the shell passes the cable's lines
    /// to [`Kernel::cable_line`](crate::Kernel::cable_line) and sends what
    /// [`Kernel::take_cable_lines`](crate::Kernel::take_cable_lines) gives;
    /// core serves `net_serve` port 0 itself (its `listen` never reaches
    /// [`ShellHttp`]).
    #[must_use]
    pub fn cable(mut self, cable: bool) -> Self {
        self.cable = cable;
        self
    }
}

/// The shell's HTTP client and server. None of these may call back into core
/// (no [`NetReports`] method) before it returns.
pub trait ShellHttp {
    /// Begins one request (connect, send, read).
    ///
    /// # Errors
    /// An `ERR_*` status when it could not begin.
    fn start(&self, job: &HttpJob<'_>) -> Result<(), i32>;
    /// Drops the transfer of `id` with no report. Core removes the staged file
    /// right after this returns, so the shell must have **closed** its handle
    /// on it (and written nothing more) before it returns.
    fn abort(&self, id: u32);
    /// Starts listening on `port` (0: the cable), trying the next 20 ports
    /// when it is taken; writes the address the shell knows now (`""` for the
    /// cable) into `address` and returns its length. The listener may open at
    /// the shell's next step: the plugin is told `started` only when the shell
    /// reports it ([`NetReports::server_started`], with the address), or
    /// `failed` when it cannot ([`NetReports::server_failed`]).
    ///
    /// # Errors
    /// Why it could not: an address that does not fit is
    /// [`ServerReason::Refused`].
    fn listen(&self, server: u32, port: u32, address: &mut [u8]) -> Result<usize, ServerReason>;
    /// Stops listening; the server's connections end. Core calls it also when
    /// it refuses an address `listen` returned (one that is not text, or that
    /// it cannot keep), when the plugin stops a server (also one whose open
    /// has not been reported yet), and for a server the shell reported failed.
    /// It **cancels an open that is still pending** as well as closing an open
    /// listener, and is a no-op only for one the shell has already given up. A
    /// shell whose late [`NetReports::server_started`] gets `ERR_NOT_FOUND`
    /// must close that listener.
    fn unlisten(&self, server: u32);
    /// Sends the answer of a served request and closes it.
    ///
    /// # Errors
    /// An `ERR_*` status.
    fn reply(&self, request: u32, reply: &ServedReply<'_>) -> Result<(), i32>;
    /// Closes a served request with a status of the host's own (503 when the
    /// plugin was too slow), or without an answer.
    fn close_served(&self, request: u32, status: u32);
}

/// The state of the device's network and clock. Core decides what the user's
/// power and network settings allow (`net_policy`); the shell is not asked.
pub trait ShellNetwork {
    /// The monotonic clock, in milliseconds.
    fn now_ms(&self) -> u64;
    /// Whether a network is up (the cable counts for the cable server).
    fn network_up(&self) -> bool;
    /// `true`: a network is wanted; `false`: the last request and server ended.
    fn network_demand(&self, wanted: bool);
}

/// What `core.net` is wired with: the card's storage and the shell's network.
#[non_exhaustive]
pub struct NetInputs {
    pub(super) storage: Rc<dyn Storage>,
    pub(super) http: Rc<dyn ShellHttp>,
    pub(super) network: Rc<dyn ShellNetwork>,
    pub(super) limits: NetLimits,
}

impl core::fmt::Debug for NetInputs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NetInputs")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl NetInputs {
    /// `storage` is the card (files saved and sent, `.received/`).
    #[must_use]
    pub fn new(
        storage: Rc<dyn Storage>,
        http: Rc<dyn ShellHttp>,
        network: Rc<dyn ShellNetwork>,
        limits: NetLimits,
    ) -> Self {
        Self {
            storage,
            http,
            network,
            limits,
        }
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
    ) -> NetDevice {
        let profile = NetProfile {
            pool: self.limits.pool_bytes,
            memory_bodies: self.limits.memory_bodies,
            connections: self.limits.connections,
            max_body: self.limits.max_body,
            cable: self.limits.cable,
            servers: self.limits.servers,
        };
        let device = NetDevice::new(
            self.storage,
            self.http,
            self.network,
            profile,
            pool,
            in_use,
            owners,
            policy,
        );
        // The cable's server is the kernel's (`cable_link`): the shell passes
        // its lines to `Kernel::cable_line`. An empty link holds nothing from
        // the heap, so this cannot be refused (`Later::make`).
        if profile.cable {
            device.with_cable(crate::native_host::net_service::cable_link::CableLink::new())
        } else {
            device
        }
    }
}

/// Why a request ended, as the shell saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HttpReason {
    Ok,
    NoNetwork,
    Dns,
    /// The certificate or the TLS handshake failed (`network.md` 4.1, reason 3).
    Tls,
    Timeout,
    Refused,
    TooLarge,
    Io,
    Full,
}

/// What the shell reports for a request that came to an end.
#[non_exhaustive]
pub struct HttpDone<'a> {
    pub status: u32,
    pub reason: HttpReason,
    /// The 2xx body was written to `save_path`.
    pub saved: bool,
    /// The memory body was cut at `max_body`.
    pub truncated: bool,
    pub etag: &'a str,
    pub content_type: &'a str,
    pub final_url: &'a str,
    /// The memory body (or the error body of a failure with a status). The
    /// shell allocates it fallibly (`try_reserve`) with a capacity of at most
    /// `max_body` (4,096 bytes for a saving request): that room is what the
    /// request reserved from the device's network pool. A buffer with more
    /// capacity than the reservation is refused: the request ends with
    /// "too large" and nothing is kept.
    pub body: Vec<u8>,
}

/// The answer's body, URL and tags are the server's data: only sizes and
/// states are printed.
impl core::fmt::Debug for HttpDone<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpDone")
            .field("status", &self.status)
            .field("reason", &self.reason)
            .field("saved", &self.saved)
            .field("truncated", &self.truncated)
            .field("body_len", &self.body.len())
            .finish_non_exhaustive()
    }
}

impl<'a> HttpDone<'a> {
    pub(crate) fn into_finished(self) -> net_service::Finished<'a> {
        use net_service::reason as r;
        let reason = match self.reason {
            HttpReason::Ok => r::OK,
            HttpReason::NoNetwork => r::NO_NETWORK,
            HttpReason::Dns => r::DNS,
            HttpReason::Tls => r::TLS,
            HttpReason::Timeout => r::TIMEOUT,
            HttpReason::Refused => r::REFUSED,
            HttpReason::TooLarge => r::TOO_LARGE,
            HttpReason::Full => r::FULL,
            _ => r::IO,
        };
        net_service::Finished {
            status: self.status,
            reason,
            saved: self.saved,
            truncated: self.truncated,
            etag: self.etag,
            content_type: self.content_type,
            final_url: self.final_url,
            body: self.body,
        }
    }

    #[must_use]
    pub fn new(status: u32, reason: HttpReason) -> Self {
        Self {
            status,
            reason,
            saved: false,
            truncated: false,
            etag: "",
            content_type: "",
            final_url: "",
            body: Vec::new(),
        }
    }
}

/// What the core says about a request head the shell's server parsed.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServedDecision {
    /// Receive the body at the storage path `path`.
    Accept { request: u32, path: String },
    /// Answer with this status and close.
    Reject(u32),
    /// The server has its open requests: the connection waits.
    Busy,
}

/// The transport's reports, for the shell's loop. `ERR_BUSY` means the table is
/// borrowed: call again.
#[derive(Clone)]
pub struct NetReports(pub(super) Rc<NetLater>);

/// `core.net` until its first call: the shell's inputs, then the device.
pub(crate) type NetLater = super::later::Later<NetInputs, NetDevice>;

impl core::fmt::Debug for NetReports {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("NetReports")
    }
}

impl NetReports {
    /// Request `id` ended. An id that is not running is ignored.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn finished(&self, id: u32, done: HttpDone<'_>) -> Result<(), i32> {
        self.0
            .made()
            .map_or(Ok(()), |d| d.report_finished(id, done))
    }

    /// Bytes of request `id`'s answer arrived (only 2xx answers of a request
    /// that asked are told to the plugin).
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn progress(
        &self,
        id: u32,
        status: u32,
        received: u64,
        total: Option<u64>,
    ) -> Result<(), i32> {
        self.0.made().map_or(Ok(()), |d| {
            net_service::progress(d, id, status, received, total)
        })
    }

    /// Request `id` was redirected from `http` to `https` and the shell closed
    /// its plain connection first (never while the transport holds one: the
    /// request waits for the one TLS session, which a socket may hold). The
    /// request goes back in the queue at its start order, keeps its start
    /// time (`timeout_ms` counts from the first start), and `start` is called
    /// again when the session is its: the shell then resumes the request it
    /// parked (URL, redirects so far, method, body and headers as redirected).
    /// A session that stays with a socket ends the request `timeout`.
    ///
    /// # Errors
    /// `ERR_BUSY` (call again); `ERR_NOT_FOUND` for an id that is unknown or
    /// over, which is normal after a cancel and no failure of the request;
    /// `ERR_STATE` for a request that is not running or is https already.
    pub fn redirected_to_https(&self, id: u32) -> Result<(), i32> {
        self.0.made().map_or(Err(ERR_NOT_FOUND), |d| {
            net_service::redirected_to_https(d, id)
        })
    }

    /// Time passed: timeouts and the answer time of served requests run out.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn tick(&self) -> Result<(), i32> {
        self.0.made().map_or(Ok(()), |d| net_service::tick(d))
    }

    /// The network went away.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn network_lost(&self) -> Result<(), i32> {
        self.0
            .made()
            .map_or(Ok(()), |d| net_service::network_lost(d))
    }

    /// The listener of `server` is open at `address` (the host and port a
    /// client reaches it at, `""` for the cable). The plugin is told
    /// `event.server` `started` now, not when [`ShellHttp::listen`]
    /// returned: `listen` only says what the shell knows, the listener opens
    /// at the shell's next step.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` for a server that stopped or failed meanwhile: the
    /// plugin has been told, so **the shell must close that listener** (core
    /// called [`ShellHttp::unlisten`] for it, which a shell with an open
    /// still pending must cancel); `ERR_STATE` when it was reported already;
    /// `ERR_BUSY`.
    pub fn server_started(&self, server: u32, address: &str) -> Result<(), i32> {
        self.0.made().map_or(Err(ERR_NOT_FOUND), |d| {
            net_service::server_started(d, server, address)
        })
    }

    /// The listener of `server` could not be opened (or went away). The
    /// plugin is told `event.server` `failed` with reason `refused`, whatever
    /// the `reason` (it is for the shell's log), and the server's slot is
    /// free for a new `serve`. Core calls [`ShellHttp::unlisten`] for it too
    /// (nothing to close for a listener that never opened).
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` for a server that stopped meanwhile; `ERR_BUSY`.
    pub fn server_failed(&self, server: u32, reason: ServerReason) -> Result<(), i32> {
        let _ = reason;
        self.0.made().map_or(Err(ERR_NOT_FOUND), |d| {
            net_service::server_failed(d, server)
        })
    }

    /// A request head arrived on `server`.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` for a server that stopped meanwhile; `ERR_BUSY`.
    pub fn served_head(
        &self,
        server: u32,
        method: &str,
        target: &str,
        content_length: Option<u64>,
        chunked: bool,
        head_len: u32,
    ) -> Result<ServedDecision, i32> {
        self.0.made().map_or(Err(ERR_NOT_FOUND), |d| {
            d.served_head(server, method, target, content_length, chunked, head_len)
        })
    }

    /// `added` more bytes of the upload of `request` arrived (`received` in all).
    ///
    /// # Errors
    /// `ERR_NOT_FOUND`; `ERR_BUSY`.
    pub fn served_body(&self, request: u32, received: u64, added: u64) -> Result<u32, i32> {
        let device = self.0.made().ok_or(ERR_NOT_FOUND)?;
        match net_service::served_body(device, request, received, added)? {
            net_service::Decision::Reject(status) => Ok(status),
            _ => Ok(0),
        }
    }

    /// The upload of `request` is whole (`total` bytes).
    ///
    /// # Errors
    /// `ERR_NOT_FOUND`; `ERR_BUSY`.
    pub fn served_whole(&self, request: u32, total: u64) -> Result<(), i32> {
        self.0.made().map_or(Err(ERR_NOT_FOUND), |d| {
            net_service::served_whole(d, request, total)
        })
    }

    /// The connection of `request` went away.
    ///
    /// # Errors
    /// `ERR_BUSY`.
    pub fn served_gone(&self, request: u32) -> Result<(), i32> {
        self.0
            .made()
            .map_or(Ok(()), |d| net_service::served_gone(d, request))
    }
}
