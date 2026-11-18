use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use crate::id::Id;
use crate::params::Params;

/// An HTTP request of a plugin. When it is answered the kernel emits
/// `reply` with `params` plus `status` (the HTTP status; `0` when the
/// request failed, e.g. without network) and `body` (the response as text).
///
/// Made with [`get`](Self::get) or [`new`](Self::new) and the methods
/// that set the rest; every request is answered once, also when it is
/// refused or taken back ([`HttpReply::refused`](crate::HttpReply)).
///
/// Printed for debugging without what may be secret: the address without
/// its query, the headers' names without their values, the body's length
/// without the body, the params' names without their values.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpRequest {
    /// `GET`, `PUT`, `POST`, …
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    /// A file in storage to send as the body instead, e.g. a database to
    /// upload.
    body_from: Option<String>,
    /// A file in storage to write the response's body to (also large
    /// ones, e.g. a download); the reply then has no `body`.
    save_to: Option<String>,
    reply: Id,
    params: Params,
    /// What few requests set, kept apart so that commands stay small
    /// ([`options`](Self::options)).
    options: Option<alloc::boxed::Box<HttpOptions>>,
}

impl core::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let address = self.url.split(['?', '#']).next().unwrap_or_default();
        let headers: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        let params: Vec<&str> = self.params.iter().map(|(name, _)| name).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &address)
            .field("headers", &headers)
            .field("body", &self.body.as_ref().map(String::len))
            .field("body_from", &self.body_from)
            .field("save_to", &self.save_to)
            .field("reply", &self.reply)
            .field("params", &params)
            .field("options", &self.options)
            .finish()
    }
}

/// An [`HttpRequest`]'s parts, for the kernel to take over.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[derive(Debug)]
#[non_exhaustive]
pub struct HttpParts {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub body_from: Option<String>,
    pub save_to: Option<String>,
    pub reply: Id,
    pub params: Params,
    pub options: HttpOptions,
}

/// What few requests set ([`HttpRequest::options`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpOptions {
    /// How long it may take in all, the connection and TLS counted; for
    /// a download to a file ([`HttpRequest::save_to`]) or an upload from
    /// one, how long it may go without a byte, however long it takes in
    /// all. `None`: the default, 30 s, or a minute without a byte.
    pub timeout: Option<core::time::Duration>,
    /// Headers of the answer to pass back in
    /// [`HttpReply::headers`](crate::HttpReply), by name (any case).
    /// `ETag` and `Content-Type` always come back.
    pub want_headers: Vec<String>,
    /// The longest body kept in memory, in bytes: a longer one is cut
    /// there. `None`: by the device's memory (64 KiB on small devices,
    /// 1 MiB on others); at most 1 MiB. Large answers go to a file
    /// ([`HttpRequest::save_to`]).
    pub max_body: Option<usize>,
    /// The largest file a download ([`HttpRequest::save_to`]) may write,
    /// in bytes: a longer answer fails ([`HttpError::Refused`](crate::HttpError))
    /// and the file stays as it was. `None`: 64 MiB, the most any may.
    pub max_file: Option<u64>,
}

/// The options of a request that sets none.
static NO_OPTIONS: HttpOptions = HttpOptions {
    timeout: None,
    want_headers: Vec::new(),
    max_body: None,
    max_file: None,
};

impl HttpRequest {
    /// A request without headers or body; its answer comes as `reply`, an
    /// [`Event<HttpReply>`](crate::replies::HttpReply) (or an id).
    pub fn new(method: &str, url: &str, reply: impl Into<Id>) -> Self {
        Self {
            method: String::from(method),
            url: String::from(url),
            headers: Vec::new(),
            body: None,
            body_from: None,
            save_to: None,
            reply: reply.into(),
            params: Params::new(),
            options: None,
        }
    }

    /// Its timeout, the headers it wants back and the longest body.
    pub fn options(&self) -> &HttpOptions {
        self.options.as_deref().unwrap_or(&NO_OPTIONS)
    }

    /// Its method, e.g. `GET`.
    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// The body it sends, as text ([`body`](Self::body)).
    pub fn body_text(&self) -> Option<&str> {
        self.body.as_deref()
    }

    /// The file it sends as its body ([`body_from`](Self::body_from)).
    pub fn body_file(&self) -> Option<&str> {
        self.body_from.as_deref()
    }

    /// Where its answer's body is saved ([`save_to`](Self::save_to)).
    pub fn save_path(&self) -> Option<&str> {
        self.save_to.as_deref()
    }

    /// The event its answer comes as.
    pub fn reply(&self) -> &Id {
        &self.reply
    }

    /// The parameters passed back with its answer.
    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Its parts, taken over by the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_parts(self) -> HttpParts {
        HttpParts {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            body_from: self.body_from,
            save_to: self.save_to,
            reply: self.reply,
            params: self.params,
            options: self.options.map(|o| *o).unwrap_or_default(),
        }
    }

    fn options_mut(&mut self) -> &mut HttpOptions {
        self.options.get_or_insert_with(Default::default)
    }

    /// A `GET` of `url`.
    pub fn get(url: &str, reply: impl Into<Id>) -> Self {
        Self::new("GET", url, reply)
    }

    /// With this header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// With this body, as text.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// With the file at this storage path as its body.
    pub fn body_from(mut self, path: impl Into<String>) -> Self {
        self.body_from = Some(path.into());
        self
    }

    /// Its answer's body written to the file at this storage path.
    pub fn save_to(mut self, path: impl Into<String>) -> Self {
        self.save_to = Some(path.into());
        self
    }

    /// With these parameters, passed back with the reply.
    pub fn with(mut self, params: Params) -> Self {
        let progress = self.params.get(http_params::PROGRESS).map(String::from);
        self.params = params;
        if let Some(progress) = progress {
            self.params.set(http_params::PROGRESS, progress);
        }
        self
    }

    /// With `event` while its answer's body comes (a download, see
    /// [`save_to`](Self::save_to)): how much came and, where the server
    /// said, how much there is in all ([`HttpProgress`](crate::HttpProgress),
    /// with this request's parameters). The kernel passes it on at most
    /// every 5 % or every 2 seconds, and never after the reply: a screen
    /// redraws its progress no more often than that.
    pub fn progress(mut self, event: &crate::event::Event<crate::HttpProgress>) -> Self {
        self.params
            .set(http_params::PROGRESS, String::from(event.id().as_str()));
        self
    }

    /// With this parameter too, passed back with the reply.
    pub fn param(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.params.set(key, value);
        self
    }

    /// Taking at most this long in all (a file's download or upload: at
    /// most this long without a byte).
    pub fn timeout(mut self, timeout: core::time::Duration) -> Self {
        self.options_mut().timeout = Some(timeout);
        self
    }

    /// Passing back these headers of the answer.
    pub fn want_headers(mut self, names: &[&str]) -> Self {
        self.options_mut().want_headers = names.iter().map(|n| String::from(*n)).collect();
        self
    }

    /// Keeping at most this many bytes of the answer's body in memory.
    pub fn max_body(mut self, bytes: usize) -> Self {
        self.options_mut().max_body = Some(bytes);
        self
    }

    /// Saving a file of at most this many bytes ([`save_to`](Self::save_to)):
    /// a longer answer fails and nothing is written, so a download cannot
    /// fill the card.
    pub fn max_file(mut self, bytes: u64) -> Self {
        self.options_mut().max_file = Some(bytes);
        self
    }
}

/// The answer to a request a plugin's server received (see
/// [`Command::Serve`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServerResponse {
    /// The request's number ([`ServerNotice::Request`](crate::ServerNotice::Request)).
    request: u32,
    status: u16,
    content_type: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// A file in storage to send as the body instead.
    body_from: Option<String>,
}

/// A [`ServerResponse`]'s parts, for the kernel to take over.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[derive(Debug)]
#[non_exhaustive]
pub struct ResponseParts {
    pub request: u32,
    pub status: u16,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub body_from: Option<String>,
}

impl ServerResponse {
    /// An answer with `body` of `content_type`.
    pub fn new(request: u32, status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            request,
            status,
            content_type: String::from(content_type),
            headers: Vec::new(),
            body,
            body_from: None,
        }
    }

    /// With this header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// With the file at this storage path as its body.
    pub fn body_from(mut self, path: impl Into<String>) -> Self {
        self.body_from = Some(path.into());
        self
    }

    /// The number of the request it answers.
    pub fn request(&self) -> u32 {
        self.request
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The file it sends as its body ([`body_from`](Self::body_from)).
    pub fn body_file(&self) -> Option<&str> {
        self.body_from.as_deref()
    }

    /// Its parts, taken over by the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_parts(self) -> ResponseParts {
        ResponseParts {
            request: self.request,
            status: self.status,
            content_type: self.content_type,
            headers: self.headers,
            body: self.body,
            body_from: self.body_from,
        }
    }
}

/// UDP datagrams to the local network (see [`Command::Broadcast`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Broadcast {
    id: Id,
    ports: Vec<u16>,
    data: Vec<u8>,
    wait_ms: u32,
    events: Id,
}

impl Broadcast {
    /// `data` to `ports`, answers within `wait_ms` heard as `events` with
    /// `id`.
    pub fn new(id: Id, ports: Vec<u16>, data: Vec<u8>, wait_ms: u32, events: Id) -> Self {
        Self {
            id,
            ports,
            data,
            wait_ms,
            events,
        }
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// How long answers are waited for, in milliseconds.
    pub fn wait_ms(&self) -> u32 {
        self.wait_ms
    }

    /// The event the answers come as.
    pub fn events(&self) -> &Id {
        &self.events
    }

    /// Its data, taken over by the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
}

/// Parameters of a server's events (see [`Command::Serve`]).
pub mod server_params {
    /// What happened: [`STARTED`], [`REQUEST`], [`FAILED`] or [`STOPPED`].
    pub const KIND: &str = "kind";
    pub const STARTED: &str = "started";
    pub const REQUEST: &str = "request";
    pub const FAILED: &str = "failed";
    pub const STOPPED: &str = "stopped";
    /// Where the server is reached, e.g. `192.168.1.20:8080`.
    pub const ADDRESS: &str = "address";
    /// Why it failed.
    pub const REASON: &str = "reason";
    /// A request's method (`GET`, `PUT`, …) and target (path and query).
    pub const METHOD: &str = "method";
    pub const TARGET: &str = "target";
    /// Where a request's body was saved, if it had one; it is removed
    /// once the request is answered, unless moved.
    pub const BODY_FILE: &str = "body-file";
}

/// What a socket is to read next (see [`Command::SocketRead`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SocketRead {
    /// What has come, up to `max` bytes, as text (UTF-8; invalid bytes
    /// replaced): the notice [`SocketNotice::Received`](crate::SocketNotice::Received) with it.
    Text { max: u32 },
    /// What has come, up to `max` bytes, as it came (e.g. a protocol's
    /// 8-bit lines that are no UTF-8): the notice
    /// [`SocketNotice::Bytes`](crate::SocketNotice::Bytes) with them.
    Bytes { max: u32 },
    /// Exactly `len` bytes, written to `path` in storage as they come (a
    /// file there is replaced): the notice [`SocketNotice::Saved`](crate::SocketNotice::Saved) once
    /// they all came, so files of any size never need to fit in memory.
    File { path: String, len: u64 },
}

/// Parameters of a socket's events (see [`Command::Connect`]) and of a
/// broadcast's (see [`Command::Broadcast`]).
pub mod socket_params {
    /// What happened: [`CONNECTED`], [`RECEIVED`], [`BYTES`], [`SAVED`], [`CLOSED`],
    /// [`FAILED`], [`SECURED`] or [`UNTRUSTED`]; for broadcasts [`ANSWER`]
    /// and [`DONE`].
    pub const KIND: &str = "kind";
    pub const CONNECTED: &str = "connected";
    /// Text came, in [`TEXT`]; empty when the other side closed.
    pub const RECEIVED: &str = "received";
    /// A file read is complete; its [`PATH`].
    pub const SAVED: &str = "saved";
    pub const CLOSED: &str = "closed";
    pub const FAILED: &str = "failed";
    /// An answer to a broadcast: its [`TEXT`] and where it came [`FROM`].
    pub const ANSWER: &str = "answer";
    /// No more answers to a broadcast will come.
    pub const DONE: &str = "done";
    /// The socket is encrypted from now on; the server's certificate in
    /// [`CERTIFICATE`].
    pub const SECURED: &str = "secured";
    /// The server's certificate is not trusted; its SHA-256 in
    /// [`CERTIFICATE`], why in [`WHY`].
    pub const UNTRUSTED: &str = "untrusted";
    /// The server's certificate's SHA-256, hex.
    pub const CERTIFICATE: &str = "certificate";
    /// The SHA-256 of the server's key (its SubjectPublicKeyInfo), hex.
    pub const KEY: &str = "key";
    /// `false` where the certificate's dates were not checked (the time
    /// was not known).
    pub const TIME_CHECKED: &str = "time-checked";
    /// Bytes came, in [`DATA`].
    pub const BYTES: &str = "bytes";
    /// Bytes, each written as the character of its value (U+0000 to
    /// U+00FF, as Latin-1 does).
    pub const DATA: &str = "data";
    /// Why a certificate was not trusted, as `Distrust::as_str` writes it.
    pub const WHY: &str = "why";
    /// Why it failed, as `SocketError`'s `Display` writes it.
    pub const ERROR: &str = "error";
    pub const TEXT: &str = "text";
    pub const PATH: &str = "path";
    /// The address an answer came from, e.g. `192.168.1.20`.
    pub const FROM: &str = "from";
    /// Why it failed or closed.
    pub const REASON: &str = "reason";
}

/// Parameters of an HTTP reply event (see [`HttpRequest`]).
pub mod http_params {
    pub const STATUS: &str = "status";
    pub const BODY: &str = "body";
    /// The response's `ETag` header, if it had one.
    pub const ETAG: &str = "etag";
    /// The response's `Content-Type` header, if it had one, e.g.
    /// `text/html; charset=utf-8`.
    pub const CONTENT_TYPE: &str = "content-type";
    /// Where the answer came from, if redirected elsewhere (named apart
    /// from a request's own parameters, which come back too).
    pub const FINAL_URL: &str = "final-url";
    /// Before a header's name in lower case, for the headers asked for
    /// ([`HttpOptions::want_headers`](crate::HttpOptions::want_headers)), e.g. `header:last-modified`.
    pub const HEADER: &str = "header:";
    /// Why there was no answer, as [`HttpError::as_str`](crate::HttpError::as_str)
    /// writes it.
    pub const ERROR: &str = "error";
    /// `true` where the body was cut to the most the request takes.
    pub const TRUNCATED: &str = "truncated";
    /// In a request's parameters, the event its progress comes as
    /// ([`HttpRequest::progress`](crate::HttpRequest::progress)); the
    /// kernel takes it out.
    pub const PROGRESS: &str = "http-progress";
    /// How many bytes of the body came so far
    /// ([`HttpProgress`](crate::HttpProgress)).
    pub const RECEIVED: &str = "received";
    /// How many bytes the body has in all, where the server said.
    pub const TOTAL: &str = "total";
}

/// Text to speak aloud (see [`Command::Speak`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Speech {
    text: String,
    /// The language of the text, e.g. `de` or `en-GB`, for the voice;
    /// empty for the system's.
    language: String,
    /// How fast, in percent of the voice's normal speed.
    rate: u16,
    /// Emitted once the text has been spoken, or could not be: a
    /// [`Spoken`](crate::replies::Spoken) with `params`.
    reply: Id,
    params: Params,
    /// Spoken after the texts given before instead of replacing them: the
    /// next sentence handed over while one is spoken follows without a
    /// pause.
    queue: bool,
}

impl Speech {
    /// `text` in the system's language at normal speed.
    pub fn new(text: impl Into<String>, reply: impl Into<Id>) -> Self {
        Self {
            text: text.into(),
            language: String::new(),
            rate: 100,
            reply: reply.into(),
            params: Params::new(),
            queue: false,
        }
    }

    /// In the language with this tag (empty: the system's).
    pub fn in_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// At `rate` percent of the normal speed.
    pub fn at_rate(mut self, rate: u16) -> Self {
        self.rate = rate;
        self
    }

    /// With `params` passed back in the reply.
    pub fn with(mut self, params: Params) -> Self {
        self.params = params;
        self
    }

    /// After what is being spoken, instead of in its place.
    pub fn queued(mut self, queue: bool) -> Self {
        self.queue = queue;
        self
    }

    /// The text to speak.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The language's tag, e.g. `de`; empty for the system's.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// How fast, in percent of the voice's normal speed.
    pub fn rate(&self) -> u16 {
        self.rate
    }

    /// The event its reply comes as.
    pub fn reply(&self) -> &Id {
        &self.reply
    }

    /// The parameters passed back with its reply.
    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Whether it is spoken after what is being spoken.
    pub fn is_queued(&self) -> bool {
        self.queue
    }

    /// Its text, taken over by the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_text(self) -> String {
        self.text
    }

    /// Its reply, the event and its params: it was `spoken`, or could
    /// not be.
    pub fn answer(&self, spoken: bool) -> (Id, Params) {
        use crate::event::EventData;
        let reply = crate::replies::Spoken::new(spoken, self.params.clone());
        (self.reply.clone(), reply.into_params())
    }
}

/// Parameters of the reply to [`Command::Speak`].
pub mod speech_params {
    /// Present (`1`) when the text could not be spoken, e.g. without a
    /// voice for it.
    pub const FAILED: &str = "failed";
}

/// The most text the clipboard holds, in bytes.
pub const MAX_CLIPBOARD: usize = 4096;
