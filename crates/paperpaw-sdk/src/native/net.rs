//! Typed `core.net` requests (`docs/abi/network.md` §2): a request built from
//! borrowed parts, checked as the host checks it, sent with caller-owned
//! buffers; the answer read while the `http_done` event is delivered.
//!
//! A plugin subscribes to [`EventKind::HttpDone`](super::declare::EventKind)
//! first (a request without that subscription is refused as `State`), starts a
//! request with [`Net::request`] and gets one [`HttpDone`] event for it, in
//! which [`Net::answer`] and [`Net::body`] read what came. The host frees the
//! answer after that callback.
//!
//! This is the client side only: servers and sockets (SOCK) are not part of it,
//! and a request is never written to a log, `Debug` included.
use super::wire::services_wire::core_net_v1 as schema;
use super::{
    Error, Host,
    batch::{self, AsWire},
    declare::records_wire::{self as wire, RawRecord, WireEncode},
    services::Service,
};

/// Headers a request carries at most.
pub const MAX_HEADERS: usize = 16;
/// The longest address.
pub const MAX_URL: usize = 1024;
/// The longest body sent inline; larger ones are written to a file first.
pub const MAX_INLINE_BODY: usize = 2048;
/// Bytes one [`Net::body`] call returns at most.
pub const BODY_PART: usize = 3840;
/// The input a request needs at most, and the reply buffer `body` needs.
pub const REQUEST_INPUT: usize = 4096;
/// The buffer the answer and body replies need.
pub const REPLY: usize = 4096;
/// Where a file the request sends or saves lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileRoot {
    /// This plugin's data folder.
    Data,
    /// The user's files on the card (needs `uses.storage`).
    Files,
    /// The installed plugins' folder: a file may be sent from it, never saved
    /// to it (`save_to` refuses it with `Denied`).
    Install,
}

impl FileRoot {
    const fn wire(self) -> u32 {
        match self {
            Self::Data => 0,
            Self::Files => 1,
            Self::Install => 2,
        }
    }
}

/// An HTTP token: letters, digits and the marks RFC 9110 allows.
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// One header of a request. The value may carry a secret: it is never logged
/// by the host, and `Debug` hides it here too.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Header<'a> {
    name: &'a str,
    value: &'a str,
}

impl core::fmt::Debug for Header<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Header")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl<'a> Header<'a> {
    /// A header, checked as the host checks it.
    ///
    /// # Errors
    /// `Invalid` for a name that is not a token or a value with control bytes
    /// (a tab too),
    /// `NoSpace` for a name over 64 or a value over 1,024 bytes. The host's
    /// own headers (`Host`, `Connection`, `Content-Length`) are refused here too.
    pub fn new(name: &'a str, value: &'a str) -> Result<Self, Error> {
        if name.len() > 64 || value.len() > 1024 {
            return Err(Error::NoSpace);
        }
        if !is_token(name)
            || value.bytes().any(|b| b < 0x20 || b == 0x7f)
            || ["host", "connection", "content-length"]
                .iter()
                .any(|own| name.eq_ignore_ascii_case(own))
        {
            return Err(Error::Invalid);
        }
        Ok(Self { name, value })
    }
}

/// One request, built from borrowed parts.
#[derive(Clone, Copy)]
pub struct Request<'a> {
    token: u32,
    method: &'a str,
    url: &'a str,
    headers: &'a [Header<'a>],
    body: &'a [u8],
    send: Option<(FileRoot, &'a str)>,
    save: Option<(FileRoot, &'a str)>,
    flags: u32,
    timeout_ms: u32,
    max_body: u32,
    max_file: u64,
}

/// `Debug` shows the method and the sizes only: the address may carry a
/// credential or an API key, the headers and the body secrets (`network.md` 2.1).
impl core::fmt::Debug for Request<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Request")
            .field("token", &self.token)
            .field("method", &self.method)
            .field("url_len", &self.url.len())
            .field("headers", &self.headers.len())
            .field("body_len", &self.body.len())
            .field("send", &self.send.is_some())
            .field("save", &self.save.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> Request<'a> {
    /// A request of `method` to `url`, answered with events carrying `token`
    /// (the plugin's own number; several requests may share one).
    ///
    /// # Errors
    /// `Invalid` for a method that is not a token, an empty address, or one
    /// that is not `http://` or `https://`; `NoSpace` for a method over 16
    /// bytes or an address over 1,024 bytes. The host checks the rest (host names, ports).
    pub fn new(token: u32, method: &'a str, url: &'a str) -> Result<Self, Error> {
        if method.len() > 16 || url.len() > MAX_URL {
            return Err(Error::NoSpace);
        }
        let scheme = |prefix: &str| {
            url.get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        };
        if !is_token(method)
            || !(scheme("http://") || scheme("https://"))
            || url.bytes().any(|b| b < 0x20 || b == 0x7f)
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            token,
            method,
            url,
            headers: &[],
            body: &[],
            send: None,
            save: None,
            flags: 0,
            timeout_ms: 0,
            max_body: 0,
            max_file: 0,
        })
    }

    /// A `GET`.
    ///
    /// # Errors
    /// As [`Request::new`].
    pub fn get(token: u32, url: &'a str) -> Result<Self, Error> {
        Self::new(token, "GET", url)
    }

    /// The headers, at most [`MAX_HEADERS`].
    ///
    /// # Errors
    /// `NoSpace` for more.
    pub fn headers(mut self, headers: &'a [Header<'a>]) -> Result<Self, Error> {
        if headers.len() > MAX_HEADERS {
            return Err(Error::NoSpace);
        }
        self.headers = headers;
        Ok(self)
    }

    /// The body, sent inline: at most [`MAX_INLINE_BODY`] bytes, and not together
    /// with a file to send.
    ///
    /// # Errors
    /// `NoSpace` for a longer body, `Invalid` with a file to send.
    pub fn body(mut self, body: &'a [u8]) -> Result<Self, Error> {
        if body.len() > MAX_INLINE_BODY {
            return Err(Error::NoSpace);
        }
        if self.send.is_some() && !body.is_empty() {
            return Err(Error::Invalid);
        }
        self.body = body;
        Ok(self)
    }

    /// A file sent whole as the body (it must exist when the request starts).
    ///
    /// # Errors
    /// `Invalid` for an empty path or with an inline body, `NoSpace` for a path
    /// over 512 bytes.
    pub fn send_file(mut self, root: FileRoot, path: &'a str) -> Result<Self, Error> {
        check_path(path)?;
        if !self.body.is_empty() {
            return Err(Error::Invalid);
        }
        self.send = Some((root, path));
        Ok(self)
    }

    /// A file a 2xx answer's body is written to, without limit of memory.
    ///
    /// # Errors
    /// `Invalid` for an empty path, `NoSpace` for one over 512 bytes, `Denied`
    /// for [`FileRoot::Install`].
    pub fn save_to(mut self, root: FileRoot, path: &'a str) -> Result<Self, Error> {
        check_path(path)?;
        if root == FileRoot::Install {
            return Err(Error::Denied);
        }
        self.save = Some((root, path));
        Ok(self)
    }

    /// Asks for `http_progress` events while a body comes.
    #[must_use]
    pub const fn progress(mut self) -> Self {
        self.flags |= 1;
        self
    }

    /// How long the request may take: 1,000 to 600,000 ms; 0 for the default.
    ///
    /// # Errors
    /// `Invalid` for another value.
    pub fn timeout_ms(mut self, ms: u32) -> Result<Self, Error> {
        if ms != 0 && !(1_000..=600_000).contains(&ms) {
            return Err(Error::Invalid);
        }
        self.timeout_ms = ms;
        Ok(self)
    }

    /// The longest body kept in memory; 0 for the host's default and most
    /// (45,056 on the X4). The host refuses more.
    #[must_use]
    pub const fn max_body(mut self, bytes: u32) -> Self {
        self.max_body = bytes;
        self
    }

    /// The longest file saved; 0 for 64 MiB, the most.
    ///
    /// # Errors
    /// `NoSpace` for more than 64 MiB.
    pub const fn max_file(mut self, bytes: u64) -> Result<Self, Error> {
        if bytes > 64 << 20 {
            return Err(Error::NoSpace);
        }
        self.max_file = bytes;
        Ok(self)
    }
}

fn check_path(path: &str) -> Result<(), Error> {
    if path.is_empty() {
        return Err(Error::Invalid);
    }
    if path.len() > 512 {
        return Err(Error::NoSpace);
    }
    Ok(())
}

/// The host's number of a request, in its `HttpDone` event and its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(u32);

impl RequestId {
    /// The number, never 0.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The id an event carries.
    ///
    /// # Errors
    /// `Invalid` for 0.
    pub const fn new(id: u32) -> Result<Self, Error> {
        if id == 0 {
            Err(Error::Invalid)
        } else {
            Ok(Self(id))
        }
    }
}

/// Why a request ended (`network.md` §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// An answer came, of any status.
    Ok,
    /// No network came within 30 s, or it went away.
    NoNetwork,
    /// The name was not found.
    Dns,
    /// The certificate or the TLS handshake failed.
    Tls,
    /// The time passed.
    Timeout,
    /// Could not connect, too many or a refused redirect, a head over 16 KiB.
    Refused,
    /// Over the file limit, or no memory for the body.
    TooLarge,
    /// A newer request saves to the same file.
    Replaced,
    /// [`Net::cancel`].
    Cancelled,
    /// The card went away, or the file to send could not be read.
    Io,
    /// The card was full.
    Full,
    /// The offline power mode.
    Offline,
    /// The plugin's network use is on hold.
    Saving,
    /// The user turned this plugin's network off.
    NetworkOff,
    /// The work run it belongs to reached its longest time.
    WorkEnded,
    /// A reason a newer host added: a failure.
    Other(u32),
}

impl Reason {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Ok,
            1 => Self::NoNetwork,
            2 => Self::Dns,
            3 => Self::Tls,
            4 => Self::Timeout,
            5 => Self::Refused,
            6 => Self::TooLarge,
            7 => Self::Replaced,
            8 => Self::Cancelled,
            9 => Self::Io,
            10 => Self::Full,
            11 => Self::Offline,
            12 => Self::Saving,
            13 => Self::NetworkOff,
            14 => Self::WorkEnded,
            other => Self::Other(other),
        }
    }
}

/// A request ended: `event.http_done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpDone {
    now_ms: u64,
    id: RequestId,
    token: u32,
    status: u32,
    reason: Reason,
    flags: u32,
    body_len: u32,
}

impl HttpDone {
    pub(crate) fn decode(r: wire::EventHttpDone) -> Result<Self, Error> {
        Ok(Self {
            now_ms: r.now_ms,
            id: RequestId::new(r.id)?,
            token: r.token,
            status: r.status,
            reason: Reason::from_wire(r.reason),
            flags: r.flags,
            body_len: r.body_len,
        })
    }

    /// The host's clock when it ended.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The request.
    #[must_use]
    pub const fn id(&self) -> RequestId {
        self.id
    }

    /// The plugin's own number of the request.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }

    /// The HTTP status; 0 when none came.
    #[must_use]
    pub const fn status(&self) -> u32 {
        self.status
    }

    /// Why it ended.
    #[must_use]
    pub const fn reason(&self) -> Reason {
        self.reason
    }

    /// Whether it ended with an answer (any status).
    #[must_use]
    pub const fn answered(&self) -> bool {
        matches!(self.reason, Reason::Ok)
    }

    /// Whether a 2xx answer came.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        self.answered() && self.status >= 200 && self.status < 300
    }

    /// The request waited for the network or the power before it ran.
    #[must_use]
    pub const fn later(&self) -> bool {
        self.flags & 1 != 0
    }

    /// The body was cut: at `max_body`, and an error body (a non-2xx answer, or
    /// a failure with a status) at the lesser of 4,096 and `max_body` when it is
    /// kept in memory, at 4,096 when the request saves (then it is never written
    /// to the file).
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.flags & 2 != 0
    }

    /// The body was written to the file asked for.
    #[must_use]
    pub const fn saved(&self) -> bool {
        self.flags & 4 != 0
    }

    /// The memory body's length.
    #[must_use]
    pub const fn body_len(&self) -> u32 {
        self.body_len
    }
}

/// Bytes of a body so far: `event.http_progress`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpProgress {
    now_ms: u64,
    id: RequestId,
    token: u32,
    received: u64,
    total: Option<u64>,
}

impl HttpProgress {
    pub(crate) fn decode(r: wire::EventHttpProgress) -> Result<Self, Error> {
        Ok(Self {
            now_ms: r.now_ms,
            id: RequestId::new(r.id)?,
            token: r.token,
            received: r.received,
            total: (r.flags & 1 != 0).then_some(r.total),
        })
    }

    /// The host's clock.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The request.
    #[must_use]
    pub const fn id(&self) -> RequestId {
        self.id
    }

    /// The plugin's own number of the request.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }

    /// Bytes of the body so far.
    #[must_use]
    pub const fn received(&self) -> u64 {
        self.received
    }

    /// The `Content-Length`, when the answer said.
    #[must_use]
    pub const fn total(&self) -> Option<u64> {
        self.total
    }
}

/// What happened to a server of this plugin: `event.server` (`network.md` 5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerEvent<'a> {
    now_ms: u64,
    server: u32,
    state: ServerState<'a>,
}

/// The state a [`ServerEvent`] tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerState<'a> {
    /// The server runs. `address` is where it can be reached, such as
    /// `192.168.1.20:8080`, or empty for the cable (served over USB).
    Started {
        /// The address, empty for the cable.
        address: &'a str,
    },
    /// The server could not start; `reason` says why (`NoNetwork`, `Offline`,
    /// `Saving`, `NetworkOff`, or `Refused` for a port that could not be had).
    Failed {
        /// Why it failed.
        reason: Reason,
    },
    /// The server stopped: by `stop_serving`, by a newer `serve`, because its
    /// screen closed, or because the network went away (`reason` is then
    /// `NoNetwork`).
    Stopped {
        /// Why it stopped; `Ok` when the plugin asked.
        reason: Reason,
    },
    /// A state a newer host added.
    Other(u32),
}

impl<'a> ServerEvent<'a> {
    pub(crate) fn decode(r: wire::EventServer<'a>) -> Result<Self, Error> {
        let reason = Reason::from_wire(r.reason);
        Ok(Self {
            now_ms: r.now_ms,
            server: r.server,
            state: match r.state {
                0 => ServerState::Started { address: r.address },
                1 => ServerState::Failed { reason },
                2 => ServerState::Stopped { reason },
                other => ServerState::Other(other),
            },
        })
    }

    /// The host's clock.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The plugin's own number of the server (the token it passed to `serve`).
    #[must_use]
    pub const fn server(&self) -> u32 {
        self.server
    }

    /// What happened.
    #[must_use]
    pub const fn state(&self) -> ServerState<'a> {
        self.state
    }
}

/// A request a server of this plugin got: `event.served_request`
/// (`network.md` 5.2), borrowed from the event's record. Headers are not given.
/// The body, when there is one, is a file in the plugin's data folder that is
/// readable only in the callback that gets this event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServedRequest<'a> {
    now_ms: u64,
    server: u32,
    request: u32,
    method: &'a str,
    target: &'a str,
    body_len: Option<u64>,
}

impl<'a> ServedRequest<'a> {
    pub(crate) fn decode(r: wire::EventServedRequest<'a>) -> Result<Self, Error> {
        Ok(Self {
            now_ms: r.now_ms,
            server: r.server,
            request: r.request,
            method: r.method,
            target: r.target,
            body_len: (r.flags & 1 != 0).then_some(r.body_len),
        })
    }

    /// The host's clock.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The plugin's own number of the server that got it.
    #[must_use]
    pub const fn server(&self) -> u32 {
        self.server
    }

    /// The host's number of the request, unique while the device runs.
    #[must_use]
    pub const fn request(&self) -> u32 {
        self.request
    }

    /// The HTTP method, at most 16 bytes.
    #[must_use]
    pub const fn method(&self) -> &'a str {
        self.method
    }

    /// The path and query, at most 1,024 bytes.
    #[must_use]
    pub const fn target(&self) -> &'a str {
        self.target
    }

    /// The length of the body, when the request has one: it is whole in the
    /// data folder at `.received/<server>/<request>`.
    #[must_use]
    pub const fn body_len(&self) -> Option<u64> {
        self.body_len
    }
}

/// A finished request's status and head, borrowed from the reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer<'a>(wire::ServiceNetAnswer<'a>);

impl<'a> Answer<'a> {
    /// The HTTP status; 0 when none came.
    #[must_use]
    pub const fn status(&self) -> u32 {
        self.0.status
    }

    /// The body kept in memory was cut: at `max_body`, or at 4,096 for an error
    /// body (as [`HttpDone::truncated`]).
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.0.flags & 2 != 0
    }

    /// The body was written to the file asked for.
    #[must_use]
    pub const fn saved(&self) -> bool {
        self.0.flags & 4 != 0
    }

    /// The memory body's length.
    #[must_use]
    pub const fn body_len(&self) -> u32 {
        self.0.body_len
    }

    /// The `ETag`, `""` when the answer had none.
    #[must_use]
    pub const fn etag(&self) -> &'a str {
        self.0.etag
    }

    /// The `Content-Type`, `""` when the answer had none.
    #[must_use]
    pub const fn content_type(&self) -> &'a str {
        self.0.content_type
    }

    /// The address after redirects, `""` when the answer had none.
    #[must_use]
    pub const fn final_url(&self) -> &'a str {
        self.0.final_url
    }
}

/// A `core.net` handle.
#[derive(Debug)]
#[must_use = "release the net handle when finished"]
pub struct Net {
    service: Service,
}

impl Net {
    /// Opens the service (needs `uses.network`).
    ///
    /// # Errors
    /// `Denied` without the feature, `Unsupported` without a network at all.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, 0).map(|service| Self { service })
    }

    /// Releases the handle.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }

    fn call<'r, I>(
        &self,
        host: &Host<'_>,
        op: u32,
        records: I,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error>
    where
        I: Iterator<Item = Row<'r>> + Clone,
    {
        let operation = self.service.operation(op).ok_or(Error::Unsupported)?;
        let len = batch::encode_counted(
            records,
            usize::from(operation.input_rows),
            operation.max_input,
            input,
        )
        .map_err(Error::from_status)?;
        self.service.call(host, op, &input[..len], out)
    }

    /// Starts `request`. Its end comes as one [`HttpDone`] event.
    ///
    /// `input` holds the encoded request (at most [`REQUEST_INPUT`] bytes), `out`
    /// the reply (at least 1,536).
    ///
    /// # Errors
    /// `Busy` at 4 requests of the plugin or 16 of the device (wait for an
    /// `HttpDone`), `NoMemory` when the pool has no room (retry then), `State`
    /// without a subscription to `HttpDone`, `NotFound` for a file to send that
    /// is not there, `NoSpace` for input over the cap or a `max_body` over the
    /// host's, `Denied` for a root the plugin may not use; otherwise the host's.
    pub fn request(
        &self,
        host: &Host<'_>,
        request: &Request<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<RequestId, Error> {
        // The rows are walked in place, never copied into an array (a record
        // is over 100 bytes on the device's stack).
        let head = Row::Request(wire::ServiceNetRequest {
            token: request.token,
            method: request.method,
            url: request.url,
            flags: request.flags,
            timeout_ms: request.timeout_ms,
            max_body: request.max_body,
            max_file: request.max_file,
        });
        fn file(role: u32, file: Option<(FileRoot, &str)>) -> Option<Row<'_>> {
            file.map(|(root, path)| {
                Row::File(wire::ServiceNetFile {
                    role,
                    root: root.wire(),
                    path,
                })
            })
        }
        let rows = core::iter::once(head)
            .chain(request.headers.iter().map(|header| {
                Row::Header(wire::ServiceNetHeader {
                    name: header.name,
                    value: header.value,
                })
            }))
            .chain(file(0, request.send))
            .chain(file(1, request.save))
            .chain(
                (!request.body.is_empty())
                    .then_some(Row::Body(wire::ServiceNetBody { data: request.body })),
            );
        let n = self.call(host, schema::OP_REQUEST, rows, input, out)?;
        let started = one(&out[..n])?
            .decode::<wire::ServiceNetStarted>()
            .map_err(|_| Error::Invalid)?;
        RequestId::new(started.id)
    }

    /// Ends every request of this plugin with `token`, each with one
    /// [`HttpDone`] of reason [`Reason::Cancelled`]. None is fine.
    ///
    /// # Errors
    /// The host's refusal, or `Invalid` for a reply that is not empty.
    pub fn cancel(
        &self,
        host: &Host<'_>,
        token: u32,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let row = Row::Token(wire::ServiceNetToken { token });
        if self.call(host, schema::OP_CANCEL, core::iter::once(row), input, out)? != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// The status and head of the request that just ended. Readable only while
    /// its [`HttpDone`] event is delivered.
    ///
    /// # Errors
    /// `NotFound` elsewhere and for another plugin's request.
    pub fn answer<'a>(
        &self,
        host: &Host<'_>,
        id: RequestId,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Answer<'a>, Error> {
        let row = Row::Id(wire::ServiceNetId { id: id.0 });
        let n = self.call(host, schema::OP_ANSWER, core::iter::once(row), input, out)?;
        let answer = one(&out[..n])?
            .decode::<wire::ServiceNetAnswer<'a>>()
            .map_err(|_| Error::Invalid)?;
        Ok(Answer(answer))
    }

    /// Up to [`BODY_PART`] bytes of the memory body from `offset`; empty at
    /// or past the end. Readable only while the [`HttpDone`] event is delivered.
    ///
    /// # Errors
    /// `NotFound` elsewhere and for another plugin's request.
    pub fn body<'a>(
        &self,
        host: &Host<'_>,
        id: RequestId,
        offset: u32,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<&'a [u8], Error> {
        let row = Row::Read(wire::ServiceNetRead { id: id.0, offset });
        let n = self.call(host, schema::OP_BODY, core::iter::once(row), input, out)?;
        let bytes = one(&out[..n])?
            .decode::<wire::ServiceNetBytes<'a>>()
            .map_err(|_| Error::Invalid)?;
        if bytes.data.len() > BODY_PART {
            return Err(Error::Invalid);
        }
        Ok(bytes.data)
    }
}

fn one(bytes: &[u8]) -> Result<RawRecord<'_>, Error> {
    RawRecord::one(bytes).map_err(|_| Error::Invalid)
}

/// The rows a `core.net` request is made of; each is encoded as its own record type.
#[derive(Clone, Copy)]
enum Row<'a> {
    Request(wire::ServiceNetRequest<'a>),
    Header(wire::ServiceNetHeader<'a>),
    File(wire::ServiceNetFile<'a>),
    Body(wire::ServiceNetBody<'a>),
    Token(wire::ServiceNetToken),
    Id(wire::ServiceNetId),
    Read(wire::ServiceNetRead),
}
impl AsWire for Row<'_> {
    fn wire(&self) -> &dyn WireEncode {
        match self {
            Self::Request(row) => row,
            Self::Header(row) => row,
            Self::File(row) => row,
            Self::Body(row) => row,
            Self::Token(row) => row,
            Self::Id(row) => row,
            Self::Read(row) => row,
        }
    }
}

#[cfg(test)]
mod tests;
