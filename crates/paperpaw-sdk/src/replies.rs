//! What comes back to a plugin, as typed events: the answer to an HTTP
//! request ([`HttpReply`]), what happens to its server ([`ServerNotice`])
//! or its sockets ([`SocketNotice`]), and what the reader typed, chose or
//! answered when asked ([`Typed`], [`FolderChosen`], [`Confirmed`], see
//! [`ask`](crate::ask)).
//!
//! A plugin names the event that brings it, e.g.
//! `const FETCHED: Event<HttpReply> = Event::new("news.fetched")`, passes
//! it with the request, and hears it with
//! [`Registrar::on`](crate::Registrar::on) or reads it on a screen with
//! [`Event::read`](crate::event::Event::read). The parameters it gave the
//! request come back as [`HttpReply::context`] (and so on), to tell its
//! requests apart.

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::params;
use crate::command::{http_params, server_params, socket_params, speech_params};
use crate::event::EventData;
use crate::id::Id;
use crate::params::Params;

/// The parameters of `params` but `known`, as a plugin gave them.
fn context(params: &Params, known: &[&str]) -> Params {
    let mut context = Params::new();
    for (key, value) in params.iter().filter(|(k, _)| !known.contains(k)) {
        context.set(String::from(key), value);
    }
    context
}

fn with_context(mut params: Params, context: &Params) -> Params {
    for (key, value) in context.iter() {
        params.set(String::from(key), value);
    }
    params
}

fn text(params: &Params, key: &str) -> Option<String> {
    params.get(key).map(String::from)
}

/// Why an HTTP request had no answer ([`HttpReply::error`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HttpError {
    /// No network to send it on.
    NoNetwork,
    /// The host's name was not found.
    Dns,
    /// The secure connection failed (e.g. a certificate refused).
    Tls,
    /// No answer in time.
    Timeout,
    /// Refused on the way: by the host (a connection refused), too many
    /// redirects, an answer larger than a file may be, or not sent at
    /// all ([`HttpReply::refused`] says why).
    Refused,
    /// A newer request with the same reply took its place.
    Replaced,
    /// Taken back before it was answered.
    Cancelled,
    /// Reading or writing failed (e.g. the file it was saved to).
    Io,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl HttpError {
    const ALL: [HttpError; 8] = [
        HttpError::NoNetwork,
        HttpError::Dns,
        HttpError::Tls,
        HttpError::Timeout,
        HttpError::Refused,
        HttpError::Replaced,
        HttpError::Cancelled,
        HttpError::Io,
    ];

    /// Its name, e.g. `no-network`, as an event's parameters write it.
    pub fn as_str(self) -> &'static str {
        match self {
            HttpError::NoNetwork => "no-network",
            HttpError::Dns => "dns",
            HttpError::Tls => "tls",
            HttpError::Timeout => "timeout",
            HttpError::Refused => "refused",
            HttpError::Replaced => "replaced",
            HttpError::Cancelled => "cancelled",
            HttpError::Io => "io",
            HttpError::Other(_) => "other",
        }
    }

    /// The error named `name` (see [`as_str`](Self::as_str); an
    /// [`Other`](Self::Other) as `other-` and its code, as its `Display`
    /// writes it).
    pub fn parse(name: &str) -> Option<Self> {
        if let Some(code) = name.strip_prefix("other-") {
            return code.parse().ok().map(HttpError::Other);
        }
        Self::ALL.into_iter().find(|e| e.as_str() == name)
    }
}

impl core::fmt::Display for HttpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HttpError::Other(code) => write!(f, "other-{code}"),
            other => f.write_str(other.as_str()),
        }
    }
}

/// The answer to an HTTP request ([`Command::Http`](crate::Command::Http)).
///
/// Heard with [`Registrar::on`](crate::Registrar::on), its body comes as
/// the shell gave it, without a copy. Observers of events see its other
/// values, not the body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpReply {
    /// The status, e.g. 200; 0 if there was no answer: then [`error`]
    /// says why, and the body may say more, as text.
    ///
    /// [`error`]: Self::error
    pub status: u16,
    /// The body as it came, unless it was saved to a file
    /// ([`HttpRequest::save_to`](crate::HttpRequest)); see
    /// [`text`](Self::text).
    pub body: Vec<u8>,
    /// The body was cut to the most the request takes
    /// ([`HttpOptions::max_response`](crate::HttpOptions)).
    pub truncated: bool,
    /// Why there was no answer (status 0), where it is known.
    pub error: Option<HttpError>,
    /// The `ETag` header, if there was one.
    pub etag: Option<String>,
    /// The `Content-Type` header, if there was one.
    pub content_type: Option<String>,
    /// Where the answer came from, if it was redirected.
    pub final_url: Option<String>,
    /// The request's own parameters
    /// ([`HttpRequest::params`](crate::HttpRequest)).
    pub context: Params,
    /// Why it was not sent, if it was not (status 0): the power mode, the
    /// reader switched the plugin's network off, too soon again, and when
    /// to try again (see [`work`](crate::work)).
    pub refused: Option<crate::work::Refusal>,
    /// The headers asked for that the answer had
    /// ([`HttpOptions::want_headers`](crate::HttpOptions)), their names in
    /// lower case.
    pub headers: alloc::vec::Vec<(String, String)>,
}

impl HttpReply {
    /// An answer with `status` and `body`, without headers or context.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
            truncated: false,
            error: None,
            etag: None,
            content_type: None,
            final_url: None,
            context: Params::new(),
            refused: None,
            headers: alloc::vec::Vec::new(),
        }
    }

    const KEYS: [&'static str; 9] = [
        http_params::STATUS,
        http_params::BODY,
        http_params::ERROR,
        http_params::TRUNCATED,
        http_params::ETAG,
        http_params::CONTENT_TYPE,
        http_params::FINAL_URL,
        crate::work::params::REFUSED,
        crate::work::params::RETRY,
    ];

    /// No answer (status 0), for `error`; `reason` says more, as the
    /// body.
    pub fn failed(error: HttpError, reason: impl Into<String>, context: Params) -> Self {
        Self {
            status: 0,
            body: reason.into().into_bytes(),
            truncated: false,
            error: Some(error),
            etag: None,
            content_type: None,
            final_url: None,
            context,
            refused: None,
            headers: alloc::vec::Vec::new(),
        }
    }

    /// Not sent, for `refusal` (status 0, its reason as the body).
    pub fn refused(refusal: crate::work::Refusal, context: Params) -> Self {
        let error = match refusal.reason {
            crate::work::Reason::Cancelled => HttpError::Cancelled,
            crate::work::Reason::NoNetwork => HttpError::NoNetwork,
            _ => HttpError::Refused,
        };
        Self {
            refused: Some(refusal),
            ..Self::failed(error, refusal.reason.as_str(), context)
        }
    }

    /// The body as text: as it is where it is UTF-8, with `�` for what
    /// is not.
    pub fn text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.body)
    }

    /// The header `name` of the answer, if it was asked for and came.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Whether the status says it succeeded (2xx).
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

impl HttpReply {
    /// Its values but the body, as an event's parameters write them.
    fn written(&self) -> Params {
        let mut params = with_context(Params::new(), &self.context)
            .with(http_params::STATUS, self.status.to_string());
        if self.truncated {
            params.set(http_params::TRUNCATED, "true");
        }
        if let Some(error) = self.error {
            params.set(http_params::ERROR, error.to_string());
        }
        for (key, value) in [
            (http_params::ETAG, &self.etag),
            (http_params::CONTENT_TYPE, &self.content_type),
            (http_params::FINAL_URL, &self.final_url),
        ] {
            if let Some(value) = value {
                params.set(key, value.clone());
            }
        }
        if let Some(refusal) = &self.refused {
            refusal.write(&mut params);
        }
        for (name, value) in &self.headers {
            params.set(
                alloc::format!("{}{}", http_params::HEADER, name.to_ascii_lowercase()),
                value.clone(),
            );
        }
        params
    }
}

impl EventData for HttpReply {
    /// Its body as text (where it is not UTF-8, as [`HttpReply::text`]).
    fn to_params(&self) -> Params {
        self.written()
            .with(http_params::BODY, self.text().into_owned())
    }

    /// Its body goes along as it is, not copied (nor written for
    /// observers).
    fn into_params(self) -> Params {
        self.written().carrying(self)
    }

    fn from_params(params: &Params) -> Option<Self> {
        Some(Self {
            status: params.get(http_params::STATUS)?.parse().ok()?,
            body: params
                .get(http_params::BODY)
                .map(|body| body.as_bytes().to_vec())
                .unwrap_or_default(),
            truncated: params.get(http_params::TRUNCATED) == Some("true"),
            error: params.get(http_params::ERROR).and_then(HttpError::parse),
            etag: text(params, http_params::ETAG),
            content_type: text(params, http_params::CONTENT_TYPE),
            final_url: text(params, http_params::FINAL_URL),
            context: context(params, &Self::KEYS)
                .iter()
                .filter(|(k, _)| !k.starts_with(http_params::HEADER))
                .fold(Params::new(), |p, (k, v)| p.with(String::from(k), v)),
            refused: crate::work::Refusal::read(params),
            headers: params
                .iter()
                .filter_map(|(k, v)| {
                    let name = k.strip_prefix(http_params::HEADER)?;
                    Some((String::from(name), String::from(v)))
                })
                .collect(),
        })
    }
}

/// How far the answer to a request has come while its body comes
/// ([`HttpRequest::progress`](crate::HttpRequest::progress)): for a bar
/// with its percent where the size is known, else the bytes received.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpProgress {
    /// The bytes of the body received so far.
    pub received: u64,
    /// The bytes of the body in all, where the server said
    /// (`Content-Length`).
    pub total: Option<u64>,
    /// The request's own parameters
    /// ([`HttpRequest::with`](crate::HttpRequest::with)).
    pub context: Params,
}

impl HttpProgress {
    pub fn new(received: u64, total: Option<u64>, context: Params) -> Self {
        Self {
            received,
            total,
            context,
        }
    }

    /// How much came, in thousandths of the whole (at most 1000), where
    /// its size is known.
    pub fn permille(&self) -> Option<u16> {
        let total = self.total.filter(|t| *t > 0)?;
        let share = u128::from(self.received) * 1000 / u128::from(total);
        Some(u16::try_from(share.min(1000)).unwrap_or(1000))
    }
}

impl EventData for HttpProgress {
    fn to_params(&self) -> Params {
        let params = with_context(Params::new(), &self.context)
            .with(http_params::RECEIVED, self.received.to_string());
        match self.total {
            Some(total) => params.with(http_params::TOTAL, total.to_string()),
            None => params,
        }
    }

    fn from_params(params: &Params) -> Option<Self> {
        Some(Self {
            received: params.get(http_params::RECEIVED)?.parse().ok()?,
            total: params.get(http_params::TOTAL).and_then(|t| t.parse().ok()),
            context: context(params, &[http_params::RECEIVED, http_params::TOTAL]),
        })
    }
}

/// The reply to [`Command::Speak`](crate::Command::Speak): whether the
/// text was spoken, and the speech's own parameters
/// ([`Speech::with`](crate::Speech::with)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Spoken {
    /// It was spoken; `false` if it could not be (e.g. without a voice
    /// for its language).
    pub spoken: bool,
    /// The parameters the speech was given.
    pub context: Params,
}

impl Spoken {
    pub fn new(spoken: bool, context: Params) -> Self {
        Self { spoken, context }
    }
}

impl EventData for Spoken {
    fn to_params(&self) -> Params {
        let params = with_context(Params::new(), &self.context);
        if self.spoken {
            params
        } else {
            params.with(speech_params::FAILED, "1")
        }
    }

    fn from_params(params: &Params) -> Option<Self> {
        Some(Self {
            spoken: params.get(speech_params::FAILED).is_none(),
            context: context(params, &[speech_params::FAILED]),
        })
    }
}

/// What happened to a plugin's server ([`Command::Serve`](crate::Command::Serve)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerNotice {
    /// It serves at `address`, e.g. `192.168.1.20:8080`.
    Started {
        server: Id,
        address: String,
    },
    /// A request came: answer it with
    /// [`Command::Respond`](crate::Command::Respond) and its `request`
    /// number.
    Request {
        server: Id,
        request: u32,
        /// `GET`, `PUT`, …
        method: String,
        /// Its path and query.
        target: String,
        /// Where its body was saved, if it had one; removed once it is
        /// answered, unless moved.
        body_file: Option<String>,
    },
    Failed {
        server: Id,
        reason: String,
    },
    Stopped {
        server: Id,
    },
}

impl ServerNotice {
    /// Which server.
    pub fn server(&self) -> &Id {
        match self {
            ServerNotice::Started { server, .. }
            | ServerNotice::Request { server, .. }
            | ServerNotice::Failed { server, .. }
            | ServerNotice::Stopped { server } => server,
        }
    }
}

impl EventData for ServerNotice {
    fn to_params(&self) -> Params {
        let p = Params::new().with(params::ID, self.server().as_str().to_string());
        match self {
            ServerNotice::Started { address, .. } => p
                .with(server_params::KIND, server_params::STARTED)
                .with(server_params::ADDRESS, address.clone()),
            ServerNotice::Request {
                request,
                method,
                target,
                body_file,
                ..
            } => {
                let mut p = p
                    .with(server_params::KIND, server_params::REQUEST)
                    .with(server_params::REQUEST, request.to_string())
                    .with(server_params::METHOD, method.clone())
                    .with(server_params::TARGET, target.clone());
                if let Some(file) = body_file {
                    p.set(server_params::BODY_FILE, file.clone());
                }
                p
            }
            ServerNotice::Failed { reason, .. } => p
                .with(server_params::KIND, server_params::FAILED)
                .with(server_params::REASON, reason.clone()),
            ServerNotice::Stopped { .. } => p.with(server_params::KIND, server_params::STOPPED),
        }
    }

    fn from_params(p: &Params) -> Option<Self> {
        let server = Id::parse(p.get(params::ID)?).ok()?;
        Some(match p.get(server_params::KIND)? {
            server_params::STARTED => ServerNotice::Started {
                server,
                address: text(p, server_params::ADDRESS)?,
            },
            server_params::REQUEST => ServerNotice::Request {
                server,
                request: p.get(server_params::REQUEST)?.parse().ok()?,
                method: text(p, server_params::METHOD)?,
                target: text(p, server_params::TARGET)?,
                body_file: text(p, server_params::BODY_FILE),
            },
            server_params::FAILED => ServerNotice::Failed {
                server,
                reason: text(p, server_params::REASON).unwrap_or_default(),
            },
            server_params::STOPPED => ServerNotice::Stopped { server },
            _ => return None,
        })
    }
}

/// What happened to a plugin's socket ([`Command::Connect`](crate::Command::Connect))
/// or broadcast ([`Command::Broadcast`](crate::Command::Broadcast)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SocketNotice {
    Connected {
        socket: Id,
    },
    /// Text came; empty when the other side closed.
    Received {
        socket: Id,
        text: String,
    },
    /// A file read is complete.
    Saved {
        socket: Id,
        path: String,
    },
    Closed {
        socket: Id,
        reason: String,
    },
    Failed {
        socket: Id,
        reason: String,
    },
    /// An answer to a broadcast, and the address it came from.
    Answer {
        socket: Id,
        text: String,
        from: String,
    },
    /// No more answers to a broadcast will come; `reason` says why it
    /// ended early (e.g. `no network`), empty if it ran its time.
    Done {
        socket: Id,
        reason: String,
    },
}

impl SocketNotice {
    /// Which socket or broadcast.
    pub fn socket(&self) -> &Id {
        match self {
            SocketNotice::Connected { socket }
            | SocketNotice::Received { socket, .. }
            | SocketNotice::Saved { socket, .. }
            | SocketNotice::Closed { socket, .. }
            | SocketNotice::Failed { socket, .. }
            | SocketNotice::Answer { socket, .. }
            | SocketNotice::Done { socket, .. } => socket,
        }
    }
}

impl EventData for SocketNotice {
    fn to_params(&self) -> Params {
        let p = Params::new().with(params::ID, self.socket().as_str().to_string());
        let kind = |p: Params, kind: &'static str| p.with(socket_params::KIND, kind);
        match self {
            SocketNotice::Connected { .. } => kind(p, socket_params::CONNECTED),
            SocketNotice::Received { text, .. } => {
                kind(p, socket_params::RECEIVED).with(socket_params::TEXT, text.clone())
            }
            SocketNotice::Saved { path, .. } => {
                kind(p, socket_params::SAVED).with(socket_params::PATH, path.clone())
            }
            SocketNotice::Closed { reason, .. } => {
                kind(p, socket_params::CLOSED).with(socket_params::REASON, reason.clone())
            }
            SocketNotice::Failed { reason, .. } => {
                kind(p, socket_params::FAILED).with(socket_params::REASON, reason.clone())
            }
            SocketNotice::Answer { text, from, .. } => kind(p, socket_params::ANSWER)
                .with(socket_params::TEXT, text.clone())
                .with(socket_params::FROM, from.clone()),
            SocketNotice::Done { reason, .. } => {
                let p = kind(p, socket_params::DONE);
                if reason.is_empty() {
                    p
                } else {
                    p.with(socket_params::REASON, reason.clone())
                }
            }
        }
    }

    fn from_params(p: &Params) -> Option<Self> {
        let socket = Id::parse(p.get(params::ID)?).ok()?;
        let get = |key| text(p, key).unwrap_or_default();
        Some(match p.get(socket_params::KIND)? {
            socket_params::CONNECTED => SocketNotice::Connected { socket },
            socket_params::RECEIVED => SocketNotice::Received {
                socket,
                text: get(socket_params::TEXT),
            },
            socket_params::SAVED => SocketNotice::Saved {
                socket,
                path: get(socket_params::PATH),
            },
            socket_params::CLOSED => SocketNotice::Closed {
                socket,
                reason: get(socket_params::REASON),
            },
            socket_params::FAILED => SocketNotice::Failed {
                socket,
                reason: get(socket_params::REASON),
            },
            socket_params::ANSWER => SocketNotice::Answer {
                socket,
                text: get(socket_params::TEXT),
                from: get(socket_params::FROM),
            },
            socket_params::DONE => SocketNotice::Done {
                socket,
                reason: get(socket_params::REASON),
            },
            _ => return None,
        })
    }
}

/// What the reader typed when asked ([`ask::text`](crate::ask::text)).
/// Made with [`Typed::new`] (a keyboard of one's own answers so).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Typed {
    pub text: String,
    /// The text so far, while the reader still types: an answer to a
    /// question asked [`live`](crate::ask::TextQuestion::live).
    pub typing: bool,
    /// The parameters the question was asked with.
    pub context: Params,
}

impl Typed {
    /// The text typed for the question asked with `context`, the reader
    /// done typing.
    pub fn new(text: impl Into<String>, context: Params) -> Self {
        Self {
            text: text.into(),
            typing: false,
            context,
        }
    }

    /// The text so far, while the reader still types (see
    /// [`Typed::typing`]).
    pub fn while_typing(mut self) -> Self {
        self.typing = true;
        self
    }
}

impl EventData for Typed {
    fn to_params(&self) -> Params {
        let p = with_context(Params::new(), &self.context).with(params::TEXT, self.text.clone());
        if self.typing {
            p.with(params::TYPING, "1")
        } else {
            p
        }
    }

    fn from_params(p: &Params) -> Option<Self> {
        Some(Self {
            text: text(p, params::TEXT)?,
            typing: p.get(params::TYPING).is_some(),
            context: context(p, &[params::TEXT, params::TYPING]),
        })
    }
}

/// The folder the reader chose when asked ([`ask::folder`](crate::ask::folder)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FolderChosen {
    pub path: String,
    /// The parameters the question was asked with.
    pub context: Params,
}

impl FolderChosen {
    /// The folder `path`, chosen for a question asked with `context`.
    pub fn new(path: impl Into<String>, context: Params) -> Self {
        Self {
            path: path.into(),
            context,
        }
    }
}

impl EventData for FolderChosen {
    fn to_params(&self) -> Params {
        with_context(Params::new(), &self.context).with(params::PATH, self.path.clone())
    }

    fn from_params(p: &Params) -> Option<Self> {
        Some(Self {
            path: text(p, params::PATH)?,
            context: context(p, &[params::PATH]),
        })
    }
}

/// The answer to a yes or no question ([`ask::confirm`](crate::ask::confirm)).
/// Made with [`Confirmed::new`] (a screen of one's own on slot
/// [`CONFIRM`](crate::builtin::slots::CONFIRM) answers so). No answer
/// comes when the reader goes back.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Confirmed {
    /// Yes, or no.
    pub yes: bool,
    /// The parameters the question was asked with.
    pub context: Params,
}

impl Confirmed {
    /// The answer `yes` to the question asked with `context`.
    pub fn new(yes: bool, context: Params) -> Self {
        Self { yes, context }
    }
}

impl EventData for Confirmed {
    fn to_params(&self) -> Params {
        let answer = if self.yes { "yes" } else { "no" };
        with_context(Params::new(), &self.context).with(crate::ui::confirm::ANSWER, answer)
    }

    fn from_params(p: &Params) -> Option<Self> {
        let yes = match p.get(crate::ui::confirm::ANSWER)? {
            "yes" => true,
            "no" => false,
            _ => return None,
        };
        Some(Self {
            yes,
            context: context(p, &[crate::ui::confirm::ANSWER]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A download's progress, written and read again: its thousandths
    /// where the size is known, the request's parameters along; a
    /// request keeps its progress event whatever parameters it gets.
    #[test]
    fn progress_is_written_and_read_with_its_share() {
        let context = Params::new().with("book", "b");
        let half = HttpProgress::new(512, Some(1024), context.clone());
        assert_eq!(half.permille(), Some(500));
        assert_eq!(HttpProgress::from_params(&half.to_params()), Some(half));
        let unsized_ = HttpProgress::new(10, None, context);
        assert_eq!(unsized_.permille(), None);
        assert_eq!(
            HttpProgress::new(9, Some(4), Params::new()).permille(),
            Some(1000)
        );
        assert_eq!(
            HttpProgress::new(9, Some(0), Params::new()).permille(),
            None
        );
        assert_eq!(
            HttpProgress::from_params(&unsized_.to_params()),
            Some(unsized_)
        );
        const PROGRESS: crate::event::Event<HttpProgress> = crate::event::Event::new("t.progress");
        let request = crate::HttpRequest::get("https://h.org/b", Id::new_static("t.reply"))
            .progress(&PROGRESS)
            .with(Params::new().with("book", "b"));
        assert_eq!(
            request.params().get(http_params::PROGRESS),
            Some("t.progress")
        );
        assert_eq!(request.params().get("book"), Some("b"));
    }

    #[test]
    fn replies_through_params_and_back() {
        let reply = HttpReply {
            status: 200,
            body: "{}".into(),
            truncated: true,
            error: None,
            etag: Some("x1".into()),
            content_type: None,
            final_url: None,
            context: Params::new().with("run", "4"),
            refused: None,
            headers: alloc::vec![(String::from("last-modified"), String::from("Tue"))],
        };
        let back = HttpReply::from_params(&reply.to_params()).unwrap();
        assert_eq!(back, reply);
        assert!(back.ok());
        assert_eq!(back.header("Last-Modified"), Some("Tue"));
        // As emitted: the body goes along as it is, not written; read
        // back whole, also where it is no text.
        let binary = HttpReply::new(200, alloc::vec![0xff, 0, 0xfe]);
        let params = binary.clone().into_params();
        assert_eq!(params.get(http_params::BODY), None);
        assert_eq!(params.carried::<HttpReply>(), Some(&binary));
        assert_eq!(binary.text(), "\u{fffd}\0\u{fffd}");
        let failed = HttpReply::failed(HttpError::Dns, "no such host", Params::new());
        let back = HttpReply::from_params(&failed.to_params()).unwrap();
        assert_eq!(back.error, Some(HttpError::Dns));
        assert_eq!(back.text(), "no such host");
        // A cause of a newer build keeps its code across.
        let newer = HttpReply::failed(HttpError::Other(7), "", Params::new());
        let back = HttpReply::from_params(&newer.to_params()).unwrap();
        assert_eq!(back.error, Some(HttpError::Other(7)));
        assert_eq!(HttpError::Other(7).as_str(), "other");
        let refusal = crate::work::Refusal::new(
            crate::work::Reason::Offline,
            crate::work::Retry::WhenAllowed,
        );
        let refused = HttpReply::refused(refusal, Params::new().with("run", "5"));
        let back = HttpReply::from_params(&refused.to_params()).unwrap();
        assert_eq!(back, refused);
        assert_eq!(back.status, 0);
        assert_eq!(back.context, Params::new().with("run", "5"));
        let request = ServerNotice::Request {
            server: Id::new_static("a.server"),
            request: 7,
            method: "PUT".into(),
            target: "/f?k=1".into(),
            body_file: None,
        };
        assert_eq!(
            ServerNotice::from_params(&request.to_params()),
            Some(request)
        );
        let answer = SocketNotice::Answer {
            socket: Id::new_static("a.find"),
            text: "hello".into(),
            from: "192.168.1.2".into(),
        };
        assert_eq!(SocketNotice::from_params(&answer.to_params()), Some(answer));
        let mut typed = Typed {
            text: "secret".into(),
            typing: false,
            context: Params::new().with("field", "user"),
        };
        assert_eq!(Typed::from_params(&typed.to_params()), Some(typed.clone()));
        typed.typing = true;
        assert_eq!(Typed::from_params(&typed.to_params()), Some(typed));
        for yes in [true, false] {
            let confirmed = Confirmed::new(yes, Params::new().with("book", "a.epub"));
            let back = Confirmed::from_params(&confirmed.to_params());
            assert_eq!(back, Some(confirmed));
        }
        assert_eq!(Confirmed::from_params(&Params::new()), None);
    }
}
