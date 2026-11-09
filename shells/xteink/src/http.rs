//! Plugins' HTTP requests (the kernel's `HttpCall`s), made over the
//! network of `net` without stopping the X4: each one moves on a step at a
//! time from the main loop (finding the host, connecting, sending,
//! receiving), the protocol by `paperpaw_http`. A body meant for a file
//! goes there as it comes (by way of a `.part` file, renamed once whole),
//! so a large download never needs to fit in memory; an error's body is
//! kept apart, never written over the file. Redirects are followed as
//! `paperpaw_http::follow` says: never down to plain `http`, never with
//! credentials to another server.
//!
//! `https://` goes over TLS (`tls`), with the certificate checked; one
//! such connection at a time, for its memory.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_core::sdk::HttpError;
use paperpaw_core::{HttpCall, HttpResponse};
use paperpaw_http::{Parser, Url};
use paperpaw_platform::Storage;
use paperpaw_sdk::Trust;

use crate::link::{Link, Received};
use crate::net::{self, Net, Resolve};
use crate::tls_slots::{Slot, Waiter};

/// How long a request waits for the network (e.g. while a network is
/// joined after a start), and at most without any progress.
const OFFLINE_MS: u64 = 20_000;
const STALL_MS: u64 = 20_000;
/// Redirects followed at most.
const MAX_REDIRECTS: u32 = 10;
/// Bytes gathered before they are appended to a file: few, larger writes
/// to the card.
const FILE_BUF: usize = 4096;
/// Bytes of an error's body kept (its reason) where a file was asked for.
const ERROR_BODY: usize = 4096;
/// A connection that stalled without a byte of the answer
/// (`paperpaw_x4::stall`).
const SILENT: &str = "no answer";

fn now_ms() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_millis()
}

/// Where a body goes.
enum Sink {
    /// Up to so many bytes, the rest cut.
    Memory(Vec<u8>, usize),
    File {
        /// The `.part` file written.
        part: String,
        buf: Vec<u8>,
        started: bool,
    },
}

/// A file sent as the body, read from storage in parts as it goes.
struct Upload {
    path: String,
    size: u64,
    /// Bytes sent so far.
    at: u64,
}

/// Requests at a time.
pub const MAX_CALLS: usize = 2;
/// A connection's buffers: more to receive (downloads), little to send.
/// On the X4 8 KB of window keep a download at what the radio and the
/// card take (about 190 KB/s), with memory left while a book is open; the
/// X4 Pro and Classic, with memory to spare, take 16.
const BUFFERS: net::Buffers = net::Buffers {
    receive: if cfg!(feature = "esp32c3") { 8 } else { 16 } * 1024,
    send: 1024,
};

/// Bytes of a file read for sending at a time.
const UPLOAD_PART: usize = 1024;

enum Phase {
    /// Waiting for the network, or for a free connection.
    Waiting,
    /// A request core made (interface 7) that was redirected from `http` to
    /// `https`: the plain connection is closed, the request waits as it is
    /// (URL, redirects so far, method, body, headers) until core starts it
    /// again, which it does once the one TLS session is its.
    Parked,
    Resolving(net::Query, u16),
    Exchanging {
        link: Link,
        /// The head to send; the body (`HttpCall::body`) follows it. How
        /// much of both went.
        request: Vec<u8>,
        sent: usize,
        /// A file sent after them.
        upload: Option<Upload>,
        parser: Parser,
    },
}

/// What a request comes to: the answer, or why there is none.
type Outcome = Result<HttpResponse, &'static str>;

struct Call {
    call: HttpCall,
    /// The address asked now (after redirects).
    url: String,
    redirects: u32,
    phase: Phase,
    sink: Sink,
    /// Bytes of the body so far.
    got: u64,
    /// Bytes of the body last told the kernel ([`Calls::progress`]).
    told: u64,
    /// Until when it may wait for the network, or for progress.
    until: u64,
    /// Until when it may take in all ([`HttpCall::timeout_ms`]).
    deadline: u64,
    /// A later call saves to the same file: this one ends unanswered.
    replaced: bool,
    /// Made once more after its connection stalled without an answer.
    again: bool,
    /// In line for one of the device's TLS sessions (a request over
    /// TLS, once it may go on), and the one it was given until the
    /// connection takes it.
    waiter: Option<Waiter>,
    slot: Option<Slot>,
    /// Core's (interface 7): `save_to` is a staged file that exists, written
    /// by appends and never renamed or removed here; a redirect from `http`
    /// to `https` parks the request.
    staged: bool,
    /// Parked just now: core is not yet told.
    just_parked: bool,
}

/// The requests going on.
#[derive(Default)]
pub struct Calls {
    calls: Vec<Call>,
    /// A connection stalled without an answer: the radio starts anew
    /// before its request is made once more ([`Calls::take_restart`]).
    restart: bool,
    /// Requests parked since the last [`Calls::take_parked`].
    parked: Vec<u32>,
}

impl Calls {
    /// Starts a request; one saving to the same file as an earlier one
    /// replaces it (the kernel passes over the earlier one's answer), so
    /// that two never write one file.
    pub fn add(&mut self, call: HttpCall) {
        self.add_as(call, false);
    }

    /// Starts a request of core (interface 7): its `save_to` is the staged
    /// file core made (empty), which this appends to and never renames.
    #[cfg(feature = "ppn5")]
    pub fn add_staged(&mut self, call: HttpCall) {
        self.add_as(call, true);
    }

    fn add_as(&mut self, call: HttpCall, staged: bool) {
        if !staged && let Some(path) = &call.save_to {
            for earlier in &mut self.calls {
                if earlier
                    .call
                    .save_to
                    .as_deref()
                    .is_some_and(|other| other.eq_ignore_ascii_case(path))
                {
                    earlier.replaced = true;
                }
            }
        }
        let sink = match &call.save_to {
            Some(path) if staged => Sink::File {
                part: path.clone(),
                buf: Vec::new(),
                started: true,
            },
            Some(path) => Sink::File {
                part: alloc::format!("{path}.part"),
                buf: Vec::new(),
                started: false,
            },
            None => Sink::Memory(Vec::new(), call.max_response),
        };
        let deadline = now_ms() + u64::from(call.timeout_ms);
        self.calls.push(Call {
            url: call.url.clone(),
            call,
            redirects: 0,
            phase: Phase::Waiting,
            sink,
            got: 0,
            told: 0,
            until: now_ms() + OFFLINE_MS,
            deadline,
            replaced: false,
            again: false,
            waiter: None,
            slot: None,
            staged,
            just_parked: false,
        });
    }

    /// The ids of requests parked since the last call.
    #[cfg(feature = "ppn5")]
    pub fn take_parked(&mut self) -> Vec<u32> {
        core::mem::take(&mut self.parked)
    }

    /// Core starts a parked request again: it goes on as it was (the
    /// redirects so far and the address it was sent to stay). `false` when
    /// no request `id` is parked.
    #[cfg(feature = "ppn5")]
    pub fn resume(&mut self, id: u32) -> bool {
        let Some(call) = self
            .calls
            .iter_mut()
            .find(|c| c.call.id == id && matches!(c.phase, Phase::Parked))
        else {
            return false;
        };
        call.phase = Phase::Waiting;
        call.until = now_ms() + OFFLINE_MS;
        true
    }

    /// Drops request `id` with no answer: its connection is let go, and no
    /// storage is touched (core removes the staged file after this).
    #[cfg(feature = "ppn5")]
    pub fn abort(&mut self, id: u32, net: Option<&mut Net>) {
        if let Some(at) = self.calls.iter().position(|c| c.call.id == id) {
            let mut call = self.calls.remove(at);
            if let Some(net) = net {
                call.stop(net);
            }
        }
    }

    /// Drops every request with no answer (the network went; core was told).
    #[cfg(feature = "ppn5")]
    pub fn clear(&mut self, net: Option<&mut Net>) {
        let mut net = net;
        for mut call in core::mem::take(&mut self.calls) {
            if let Some(net) = net.as_deref_mut() {
                call.stop(net);
            }
        }
        self.parked.clear();
    }

    /// Whether the radio is to start anew (once taken, until a connection
    /// stalls again without an answer).
    pub fn take_restart(&mut self) -> bool {
        core::mem::take(&mut self.restart)
    }

    /// How far the successful answers of requests that ask for it
    /// ([`HttpCall::progress`]) came since the last call: the request's
    /// id, its body's bytes so far, and in all where the answer said
    /// (the kernel thins them out).
    pub fn progress(&mut self) -> Vec<(u32, u64, Option<u64>)> {
        self.calls
            .iter_mut()
            .filter(|c| c.call.progress && c.got > c.told)
            .filter_map(|c| {
                let Phase::Exchanging { parser, .. } = &c.phase else {
                    return None;
                };
                let head = parser.head().filter(|h| (200..300).contains(&h.status))?;
                c.told = c.got;
                Some((c.call.id, c.got, head.length))
            })
            .collect()
    }

    /// Requests are going on.
    pub fn busy(&self) -> bool {
        self.calls.iter().any(|c| !matches!(c.phase, Phase::Parked))
    }

    /// Moves every request on; the answers of those done. `net` is `None`
    /// while the radio is off.
    // A frame of its own: not under the kernel's work in `Network::step`.
    #[inline(never)]
    pub fn poll(
        &mut self,
        mut net: Option<&mut Net>,
        storage: &dyn Storage,
        clock_ready: bool,
    ) -> Vec<HttpResponse> {
        let mut answers = Vec::new();
        let mut at = 0;
        while at < self.calls.len() {
            let open = self
                .calls
                .iter()
                .filter(|c| !matches!(c.phase, Phase::Waiting | Phase::Parked))
                .count();
            let secure = self
                .calls
                .iter()
                .filter(|c| !matches!(c.phase, Phase::Waiting | Phase::Parked) && c.secure())
                .count();
            if self.calls[at].replaced {
                let mut call = self.calls.remove(at);
                if let Some(net) = net.as_deref_mut() {
                    call.stop(net);
                }
                call.discard(storage);
                continue;
            }
            let call = &mut self.calls[at];
            // A TLS connection takes some 40 KB: one request at a time, and
            // within the device's TLS sessions, which plugins' sockets share
            // (first asked, first served).
            let may_open = open < MAX_CALLS
                && (!call.secure() || (secure == 0 && clock_ready && call.slot_ready()));
            let outcome = match net.as_deref_mut() {
                None => Some(Err("no network")),
                Some(net) => call.step(net, storage, may_open),
            };
            if core::mem::take(&mut call.just_parked) {
                self.parked.push(call.call.id);
            }
            if let Some(Err(SILENT)) = outcome
                && paperpaw_x4::stall::stalled(false, call.again)
                    == paperpaw_x4::stall::Stalled::Again
            {
                // Once more, on the radio started anew (nothing of the
                // answer came, so nothing is to be let go of the body).
                log::warn!("http: no answer at all; the radio starts anew, then once more");
                if let Some(net) = net.as_deref_mut() {
                    call.stop(net);
                }
                call.again = true;
                let now = now_ms();
                call.until = now + OFFLINE_MS;
                call.deadline = now + u64::from(call.call.timeout_ms);
                self.restart = true;
                at += 1;
                continue;
            }
            match outcome {
                None => at += 1,
                Some(result) => {
                    let mut call = self.calls.remove(at);
                    if let Some(net) = net.as_deref_mut() {
                        call.stop(net);
                    }
                    answers.push(match result {
                        Ok(answer) => call.finish(answer, storage),
                        Err(reason) => {
                            // The host only: addresses may carry keys.
                            let url = call.call.url.as_str();
                            let host = url.split("://").nth(1).unwrap_or(url);
                            let host = host.split(['/', '?']).next().unwrap_or(host);
                            log::warn!("http {} {host}: {reason}", call.call.method);
                            call.discard(storage);
                            HttpResponse::failed(call.call.id, error_of(reason), reason)
                        }
                    });
                }
            }
        }
        answers
    }
}

impl Call {
    /// Whether it holds one of the device's TLS sessions, in line for it
    /// until then.
    fn slot_ready(&mut self) -> bool {
        if self.slot.is_none() {
            self.slot = self.waiter.get_or_insert_with(Waiter::ask).take();
            if self.slot.is_some() {
                self.waiter = None;
            }
        }
        self.slot.is_some()
    }

    /// It goes over TLS (`https://`).
    fn secure(&self) -> bool {
        Url::parse(&self.url).is_some_and(|url| url.https)
    }

    /// One step on; the outcome once there is one.
    fn step(&mut self, net: &mut Net, storage: &dyn Storage, may_open: bool) -> Option<Outcome> {
        let now = now_ms();
        if now > self.deadline && !matches!(self.phase, Phase::Parked) {
            return Some(Err("timed out"));
        }
        if net.address().is_none() && !matches!(self.phase, Phase::Waiting | Phase::Parked) {
            // The address went: what was open on it is gone.
            return Some(Err("no network"));
        }
        match self.phase {
            Phase::Waiting => self.start(net, storage, may_open, now),
            Phase::Parked => None,
            Phase::Resolving(query, port) => {
                let resolved = net.resolved(query);
                if resolved.is_some() {
                    // The lookup is over (smoltcp freed it): nothing to stop.
                    self.phase = Phase::Waiting;
                }
                match resolved {
                    None => (now > self.until).then_some(Err("no such host")),
                    Some(Ok(address)) => self.connect(net, storage, address, port),
                    Some(Err(_)) => Some(Err("no such host")),
                }
            }
            Phase::Exchanging { .. } => self.exchange(net, storage, now),
        }
    }

    /// Looks the host up once there is a network and a connection may be
    /// opened; connects at once where its address is known.
    fn start(
        &mut self,
        net: &mut Net,
        storage: &dyn Storage,
        may_open: bool,
        now: u64,
    ) -> Option<Outcome> {
        if net.address().is_none() || !may_open {
            return (now > self.until).then_some(Err("no network"));
        }
        let Some(url) = Url::parse(&self.url) else {
            return Some(Err("not an address"));
        };
        let port = url.port;
        match net.resolve(url.host) {
            Ok(Resolve::Done(address)) => self.connect(net, storage, address, port),
            Ok(Resolve::Asking(query)) => {
                self.phase = Phase::Resolving(query, port);
                self.until = now + STALL_MS;
                None
            }
            Err(_) => Some(Err("no such host")),
        }
    }

    /// Sends the request and reads the answer as far as the connection
    /// goes; the answer once it is whole.
    fn exchange(&mut self, net: &mut Net, storage: &dyn Storage, now: u64) -> Option<Outcome> {
        let Phase::Exchanging {
            link,
            request,
            sent,
            upload,
            parser,
        } = &mut self.phase
        else {
            return None;
        };
        let mut progress = match link.pump(net) {
            Ok(moved) => moved,
            Err(problem) => return Some(Err(problem.reason())),
        };
        let body = self.call.body.as_deref().unwrap_or_default();
        if link.ready(net) {
            match send_request(link, net, storage, request, body, sent, upload.as_mut()) {
                Ok(moved) => progress |= moved,
                Err(problem) => return Some(Err(problem)),
            }
        }
        let max = self.call.max_response as u64;
        let ended = match receive(
            link,
            net,
            parser,
            &mut self.sink,
            &mut self.got,
            max,
            storage,
        ) {
            Ok((moved, ended)) => {
                progress |= moved;
                ended
            }
            Err(problem) => return Some(Err(problem)),
        };
        if parser.head().is_some_and(|head| head.is_redirect()) {
            return self.redirect(net);
        }
        let all_sent =
            *sent == request.len() + body.len() && upload.as_ref().is_none_or(|u| u.at == u.size);
        if parser.done() || (ended && all_sent && parser.close().is_ok()) {
            let head = parser.head().cloned().unwrap_or_default();
            return Some(Ok(HttpResponse {
                id: self.call.id,
                status: head.status,
                etag: head.etag,
                content_type: head.content_type,
                url: (self.url != self.call.url).then(|| self.url.clone()),
                headers: head.headers,
                ..HttpResponse::default()
            }));
        }
        if ended {
            return Some(Err("connection lost"));
        }
        if progress {
            self.until = now + STALL_MS;
            // A file's download or upload is limited by its bytes coming,
            // not in all ([`HttpCall::timeout_ms`]).
            if self.call.save_to.is_some() || self.call.body_from.is_some() {
                self.deadline = now + u64::from(self.call.timeout_ms);
            }
            None
        } else if now > self.until {
            log::warn!(
                "http: stalled, {} sent {}/{}, heap {}",
                link.describe(net),
                sent,
                request.len() + body.len(),
                heap()
            );
            Some(Err(if link.received() { "timed out" } else { SILENT }))
        } else {
            None
        }
    }

    /// Opens the connection and sends the request.
    fn connect(
        &mut self,
        net: &mut Net,
        storage: &dyn Storage,
        address: smoltcp::wire::IpAddress,
        port: u16,
    ) -> Option<Outcome> {
        let Some(url) = Url::parse(&self.url) else {
            return Some(Err("not an address"));
        };
        let upload = match &self.call.body_from {
            Some(path) => match storage.size(path) {
                Ok(size) => Some(Upload {
                    path: path.clone(),
                    size,
                    at: 0,
                }),
                Err(_) => return Some(Err("could not read the file")),
            },
            None => None,
        };
        let body = self.call.body.as_deref();
        let body_len = match &upload {
            Some(upload) => Some(upload.size as usize),
            None => body.map(<[u8]>::len),
        };
        let request = paperpaw_http::request(&self.call.method, &url, &self.call.headers, body_len);
        // The secure session first: it takes the most memory.
        let tls = match url.https.then(|| self.slot.take()) {
            Some(Some(slot)) => match crate::tls::Session::new(url.host, &Trust::Authorities, slot)
            {
                Ok(session) => Some(alloc::boxed::Box::new(session)),
                Err(e) => return Some(Err(e.reason())),
            },
            Some(None) => return Some(Err("no secure connection was free")),
            None => None,
        };
        let socket = match net.open((address, port), BUFFERS) {
            Ok(socket) => socket,
            Err(net::Error::Memory) => return Some(Err("not enough memory for a connection")),
            Err(_) => return Some(Err("could not connect")),
        };
        let link = Link::new(socket, tls);
        self.phase = Phase::Exchanging {
            link,
            request,
            sent: 0,
            upload,
            parser: Parser::new(&self.call.method).wanting(&self.call.want_headers),
        };
        self.until = now_ms() + STALL_MS;
        None
    }

    /// Follows a redirect as `paperpaw_http::follow` says: the same
    /// request (a `GET` without body after a 303, or a 301 or 302 of a
    /// `POST`) to where it leads, without credentials to another server.
    fn redirect(&mut self, net: &mut Net) -> Option<Outcome> {
        let Phase::Exchanging { parser, .. } = &self.phase else {
            return None;
        };
        let head = parser.head().cloned().unwrap_or_default();
        self.redirects += 1;
        if self.redirects > MAX_REDIRECTS {
            return Some(Err("too many redirects"));
        }
        let body = self.call.body.is_some() || self.call.body_from.is_some();
        let redirect = match paperpaw_http::follow(
            &self.url,
            head.status,
            head.location.as_deref().unwrap_or_default(),
            &self.call.method,
            body,
        ) {
            Ok(redirect) => redirect,
            Err(refusal) => return Some(Err(refusal.reason())),
        };
        if redirect.to_get {
            self.call.method = String::from("GET");
            self.call.body = None;
            self.call.body_from = None;
        }
        redirect.keep(&mut self.call.headers);
        let upgrade = self.staged
            && !Url::parse(&self.url).is_some_and(|url| url.https)
            && Url::parse(&redirect.url).is_some_and(|url| url.https);
        self.stop(net);
        self.url = redirect.url;
        if upgrade {
            // The plain connection is closed; core starts the request again
            // when the TLS session is its.
            self.phase = Phase::Parked;
            self.just_parked = true;
            return None;
        }
        self.phase = Phase::Waiting;
        self.until = now_ms() + OFFLINE_MS;
        None
    }

    /// Lets the connection or lookup go.
    fn stop(&mut self, net: &mut Net) {
        match core::mem::replace(&mut self.phase, Phase::Waiting) {
            Phase::Waiting | Phase::Parked => {}
            Phase::Resolving(query, _) => net.forget(query),
            Phase::Exchanging { link, .. } => net.release(link.socket),
        }
    }

    /// The answer of a request done: the body in memory, or the file
    /// made whole (the old one replaced only then).
    fn finish(self, mut answer: HttpResponse, storage: &dyn Storage) -> HttpResponse {
        match self.sink {
            Sink::Memory(body, _) => {
                // Kept to the most the request takes.
                answer.truncated = self.got > body.len() as u64;
                answer.body = body;
            }
            Sink::File { part, buf, .. } if self.staged => {
                // Core's staged file: what is gathered is appended, and
                // core renames it over the target.
                if !buf.is_empty() && storage.append(&part, &buf).is_err() {
                    let reason = "could not write the file";
                    return HttpResponse::failed(self.call.id, HttpError::Io, reason);
                }
                answer.saved = true;
            }
            Sink::File { part, buf, started } => {
                let path = self.call.save_to.as_deref().unwrap_or_default();
                let written = if started {
                    storage.append(&part, &buf)
                } else {
                    storage.write(&part, &buf)
                };
                let replaced = written.and_then(|()| {
                    let _ = storage.remove(path);
                    storage.rename(&part, path)
                });
                if replaced.is_err() {
                    let _ = storage.remove(&part);
                    let reason = "could not write the file";
                    return HttpResponse::failed(self.call.id, HttpError::Io, reason);
                }
                answer.saved = true;
            }
        }
        answer
    }

    /// Removes what a failed request wrote.
    fn discard(&self, storage: &dyn Storage) {
        // Core's staged file is core's to remove.
        if self.staged {
            return;
        }
        if let Sink::File {
            part,
            started: true,
            ..
        } = &self.sink
        {
            let _ = storage.remove(part);
        }
    }
}

/// The heap's free bytes by region (internal RAM, PSRAM), for the log.
pub(crate) fn heap() -> String {
    let stats = esp_alloc::HEAP.stats();
    let mut text = String::new();
    for region in stats.region_stats.iter().flatten() {
        let external = region
            .capabilities
            .contains(esp_alloc::MemoryCapability::External);
        text.push_str(&alloc::format!(
            "{}{} free of {}",
            if text.is_empty() { "" } else { ", " },
            region.free,
            region.size
        ));
        if external {
            text.push_str(" (PSRAM)");
        }
    }
    text
}

/// What kind of failure `reason` is: of this module's texts, the
/// connection's ([`Link`]) and the secure session's.
fn error_of(reason: &str) -> HttpError {
    match reason {
        "no network" => HttpError::NoNetwork,
        "no such host" | "not an address" => HttpError::Dns,
        "timed out" | SILENT => HttpError::Timeout,
        "could not connect" | "too many redirects" | "too large" => HttpError::Refused,
        _ if reason.contains("certificate") || reason.contains("secure") => HttpError::Tls,
        _ => HttpError::Io,
    }
}

/// The request, then the file, as the connection takes them; whether some
/// went.
fn send_request(
    link: &mut Link,
    net: &mut Net,
    storage: &dyn Storage,
    head: &[u8],
    body: &[u8],
    sent: &mut usize,
    mut upload: Option<&mut Upload>,
) -> Result<bool, &'static str> {
    let mut progress = false;
    let total = head.len() + body.len();
    while *sent < total {
        let rest = match head.get(*sent..) {
            Some(rest) if !rest.is_empty() => rest,
            _ => &body[*sent - head.len()..],
        };
        match link.send(net, rest)? {
            0 => break,
            n => {
                *sent += n;
                progress = true;
            }
        }
    }
    while *sent == total
        && let Some(upload) = upload.as_deref_mut()
        && upload.at < upload.size
    {
        let mut part = [0u8; UPLOAD_PART];
        let want = (upload.size - upload.at).min(UPLOAD_PART as u64) as usize;
        let read = storage.read_at(&upload.path, upload.at, &mut part[..want]);
        let Ok(read @ 1..) = read else {
            return Err("could not read the file");
        };
        match link.send(net, &part[..read])? {
            0 => break,
            n => {
                upload.at += n as u64;
                progress = true;
            }
        }
    }
    Ok(progress)
}

/// The answer as far as it came, its body kept in `sink` (`got` bytes of
/// at most `max`). Body bytes that come with the head wait until the head
/// says whether they are wanted (not a redirect's) and where they go (not
/// to the file, for an error). Whether some came, and whether the
/// connection ended.
fn receive(
    link: &mut Link,
    net: &mut Net,
    parser: &mut Parser,
    sink: &mut Sink,
    got: &mut u64,
    max: u64,
    storage: &dyn Storage,
) -> Result<(bool, bool), &'static str> {
    let mut problem = None;
    let mut early = Vec::new();
    let stop = core::cell::Cell::new(parser.done());
    let received = link.receive(net, &mut || !stop.get(), &mut |data| {
        let had_head = parser.head().is_some();
        let fed = parser.feed(data, &mut |piece| {
            if had_head {
                if let Err(e) = keep(sink, got, max, piece, storage) {
                    problem.get_or_insert(e);
                }
            } else {
                early.extend_from_slice(piece);
            }
        });
        if fed.is_err() {
            problem.get_or_insert("not an HTTP answer");
        }
        // An error's body is its reason, not the file asked for.
        if !had_head
            && let Some(head) = parser.head()
            && !(200..300).contains(&head.status)
            && !head.is_redirect()
            && matches!(sink, Sink::File { .. })
        {
            *sink = Sink::Memory(Vec::new(), ERROR_BODY);
        }
        let redirect = parser.head().is_some_and(|head| head.is_redirect());
        if !early.is_empty() && !redirect {
            if let Err(e) = keep(sink, got, max, &early, storage) {
                problem.get_or_insert(e);
            }
            early.clear();
        }
        stop.set(problem.is_some() || redirect || parser.done());
    });
    let outcome = match received? {
        Received::Some(moved) => (moved, false),
        Received::Closed => (false, true),
    };
    match problem {
        Some(problem) => Err(problem),
        None => Ok(outcome),
    }
}

/// Takes a piece of the body: into memory (cut at its limit) or towards
/// the file (failing past `max`).
fn keep(
    sink: &mut Sink,
    got: &mut u64,
    max: u64,
    piece: &[u8],
    storage: &dyn Storage,
) -> Result<(), &'static str> {
    *got += piece.len() as u64;
    match sink {
        Sink::Memory(body, limit) => {
            let room = limit.saturating_sub(body.len());
            let piece = &piece[..piece.len().min(room)];
            // Growing as it comes, without failing hard where memory ends.
            if body.try_reserve(piece.len()).is_err()
                && body.try_reserve_exact(piece.len()).is_err()
            {
                return Err("the answer is too large for the memory of this device");
            }
            body.extend_from_slice(piece);
        }
        Sink::File { part, buf, started } => {
            if *got > max {
                return Err("too large");
            }
            let mut write = |bytes: &[u8]| {
                let written = if *started {
                    storage.append(part, bytes)
                } else {
                    storage.write(part, bytes)
                };
                *started = true;
                written.map_err(|_| "could not write the file")
            };
            // Small pieces gathered to write in fewer calls; the buffer
            // never grows past `FILE_BUF`, as pieces come up to a
            // connection's whole buffer (16 KB) when memory is short.
            if buf.len() + piece.len() > FILE_BUF && !buf.is_empty() {
                write(buf)?;
                buf.clear();
            }
            if piece.len() >= FILE_BUF {
                write(piece)?;
            } else {
                if buf.capacity() < FILE_BUF {
                    buf.try_reserve_exact(FILE_BUF - buf.len())
                        .map_err(|_| "not enough memory to write the file")?;
                }
                buf.extend_from_slice(piece);
            }
        }
    }
    Ok(())
}
