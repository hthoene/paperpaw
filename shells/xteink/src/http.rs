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
use paperpaw_x4::download_boost;
use paperpaw_x4::file_sink::FileSink;

use crate::link::{Link, Received};
use crate::net::{self, Net, Resolve};
use crate::tls_slots::{Slot, Waiter};

/// How long a request waits for the network (e.g. while a network is
/// joined after a start), and at most without any progress.
const OFFLINE_MS: u64 = 20_000;
const STALL_MS: u64 = 20_000;
/// Redirects followed at most.
const MAX_REDIRECTS: u32 = 10;
/// Rounds of receiving over TLS in one step at most: the connection's
/// bytes go to mbedTLS 2 KB at a time (`tls::IN_LIMIT`, for the memory),
/// so one round would move no more than that per pass of the main loop.
const TLS_ROUNDS: usize = 8;
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
    /// Up to so many bytes, the rest cut; whether the room is taken once
    /// (core's requests, whose limit is reserved from the network pool) or
    /// grown from the size of the pieces (interface 6, whose default limit
    /// is 64 KiB and mostly not needed).
    Memory(Vec<u8>, usize, bool),
    /// The `.part` file written, in chunks (`paperpaw_x4::file_sink`).
    File(FileSink),
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
    /// Its connection and file chunks are the larger pair of a download
    /// (`download_boost::plan`), given back with the connection.
    boosted: bool,
    /// Its connection is one kept from an earlier request (`Idle`): if it
    /// fails before any byte of the answer, the server closed it meanwhile,
    /// and the request is made once more on a new one.
    reused: bool,
    /// Never on a kept connection (it failed as one).
    fresh: bool,
    /// When it was asked for (the download's time in the log, #216).
    began: u64,
    /// The user waits for it (`net_request.flags` bit 1, #369 H): the
    /// larger window whatever its file's cap says.
    foreground: bool,
    /// Never boosted (`net_request.flags` bit 2, #242).
    lean: bool,
}

/// A connection kept open for the next request to the same server.
struct Idle {
    host: String,
    port: u16,
    https: bool,
    link: Link,
    since: u64,
}

impl Idle {
    fn matches(&self, url: &Url<'_>) -> bool {
        self.port == url.port && self.https == url.https && self.host.eq_ignore_ascii_case(url.host)
    }
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
    /// The connection of the last answer, kept a few seconds
    /// (`download_boost::IDLE_MS`) for the next request to its server.
    idle: Option<Idle>,
}

impl Calls {
    /// Starts a request; one saving to the same file as an earlier one
    /// replaces it (the kernel passes over the earlier one's answer), so
    /// that two never write one file.
    pub fn add(&mut self, call: HttpCall) {
        self.add_as(call, false, false, false);
    }

    /// Starts a request of core (interface 7): its `save_to` is the staged
    /// file core made (empty), which this appends to and never renames.
    /// `foreground`: the user waits for it (core honoured the flag); `lean`:
    /// never boosted.
    #[cfg(feature = "ppn5")]
    pub fn add_staged(&mut self, call: HttpCall, foreground: bool, lean: bool) {
        self.add_as(call, true, foreground, lean);
    }

    #[allow(clippy::fn_params_excessive_bools, reason = "the request's flags")]
    fn add_as(&mut self, call: HttpCall, staged: bool, foreground: bool, lean: bool) {
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
            Some(path) if staged => Sink::File(FileSink::new(
                path.clone(),
                true,
                download_boost::BASE_CHUNK,
            )),
            Some(path) => Sink::File(FileSink::new(
                alloc::format!("{path}.part"),
                false,
                download_boost::BASE_CHUNK,
            )),
            None => Sink::Memory(Vec::new(), call.max_response, staged),
        };
        let deadline = now_ms() + u64::from(call.timeout_ms);
        if Url::parse(&call.url)
            .is_some_and(|url| url.https && !self.idle.as_ref().is_some_and(|i| i.matches(&url)))
        {
            crate::tls_slots::ask_room();
        }
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
            boosted: false,
            reused: false,
            fresh: false,
            began: now_ms(),
            foreground,
            lean,
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
        self.release_idle(net.as_deref_mut());
        for mut call in core::mem::take(&mut self.calls) {
            if let Some(net) = net.as_deref_mut() {
                call.stop(net);
            }
        }
        self.parked.clear();
    }

    /// Lets the kept connection go (its socket with it, where the network
    /// is still there).
    fn release_idle(&mut self, net: Option<&mut Net>) {
        if let Some(idle) = self.idle.take()
            && let Some(net) = net
        {
            net.release(idle.link.socket);
        }
    }

    /// Lets the kept connection go once it waited long enough, the other
    /// side closed it, another secure session is wanted (a plugin's socket,
    /// or a request to another server, whose session needs the heap), or
    /// the network is gone. Called every step, also while no request runs.
    pub fn settle_idle(&mut self, mut net: Option<&mut Net>, now: u64) {
        let Some(idle) = &self.idle else {
            return;
        };
        let ended = match net.as_deref_mut() {
            Some(net) => idle.link.closed(net),
            None => true,
        };
        let elsewhere = self.calls.iter().any(|c| {
            matches!(c.phase, Phase::Waiting)
                && Url::parse(&c.url).is_some_and(|url| url.https && !idle.matches(&url))
        });
        let pressed = idle.https && (crate::tls_slots::pressed() || elsewhere);
        if download_boost::drop_idle(idle.since, now, pressed, ended) {
            self.release_idle(net);
        }
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
        self.settle_idle(net.as_deref_mut(), now_ms());
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
            let boost_free = !self.calls.iter().any(|c| c.boosted);
            let call = &mut self.calls[at];
            // A TLS connection takes some 40 KB: one request at a time, and
            // within the device's TLS sessions, which plugins' sockets share
            // (first asked, first served). A kept connection is a session
            // that is held already.
            let keeps = call.can_reuse(self.idle.as_ref());
            let may_open = open < MAX_CALLS
                && (keeps || !call.secure() || (secure == 0 && clock_ready && call.slot_ready()));
            let outcome = match net.as_deref_mut() {
                None => Some(Err("no network")),
                Some(net) => call.step(net, storage, may_open, &mut self.idle, boost_free),
            };
            if let Some(Err(_)) = outcome
                && call.reused
                && !call.has_head()
            {
                // The server closed the kept connection meanwhile: once
                // more on a new one, no radio restart (the network is fine).
                log::info!("http: the kept connection was closed; on a new one");
                if let Some(net) = net.as_deref_mut() {
                    call.stop(net);
                }
                call.fresh = true;
                call.until = now_ms() + OFFLINE_MS;
                at += 1;
                continue;
            }
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
                    if result.is_ok() && call.got >= 4096 {
                        // What a device test reads (`final-b4` step 21): the
                        // whole request, the lookup and the handshake counted.
                        let ms = now_ms().saturating_sub(call.began).max(1);
                        let url = call.call.url.as_str();
                        let host = url.split("://").nth(1).unwrap_or(url);
                        let host = host.split(['/', '?']).next().unwrap_or(host);
                        log::info!(
                            "http: {} {host} {} B in {ms} ms ({} KB/s){}{}",
                            call.call.method,
                            call.got,
                            call.got * 1000 / 1024 / ms,
                            if call.boosted { " boosted" } else { "" },
                            if call.reused { " kept-connection" } else { "" },
                        );
                    }
                    if result.is_ok()
                        && call.keepable()
                        && let Phase::Exchanging { link, .. } =
                            core::mem::replace(&mut call.phase, Phase::Waiting)
                    {
                        // Kept for the next request to this server. A
                        // connection kept before it is let go first.
                        self.release_idle(net.as_deref_mut());
                        if let Some(url) = Url::parse(&call.url) {
                            self.idle = Some(Idle {
                                host: String::from(url.host),
                                port: url.port,
                                https: url.https,
                                link,
                                since: now_ms(),
                            });
                        } else if let Some(net) = net.as_deref_mut() {
                            net.release(link.socket);
                        }
                    }
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

    /// Whether the connection kept from an earlier request is this one's:
    /// the same server, and a `GET` (a request that changes something is
    /// never made again on a connection that may have been closed).
    fn can_reuse(&self, idle: Option<&Idle>) -> bool {
        !self.fresh
            && matches!(self.phase, Phase::Waiting)
            && self.call.method.eq_ignore_ascii_case("GET")
            && idle.is_some_and(|idle| Url::parse(&self.url).is_some_and(|url| idle.matches(&url)))
    }

    /// Whether any byte of the answer's head came.
    fn has_head(&self) -> bool {
        matches!(&self.phase, Phase::Exchanging { parser, .. } if parser.head().is_some())
    }

    /// Whether its connection may be kept for the next request: it ended
    /// cleanly, by its length, and the server keeps it (and it holds no
    /// download's larger buffers, which are given back with it).
    fn keepable(&self) -> bool {
        let Phase::Exchanging { parser, .. } = &self.phase else {
            return false;
        };
        let Some(head) = parser.head() else {
            return false;
        };
        !self.boosted
            && download_boost::may_keep(
                &self.call.method,
                head.keep_alive,
                parser.done(),
                parser.delimited(),
                head.status,
            )
    }

    /// One step on; the outcome once there is one.
    fn step(
        &mut self,
        net: &mut Net,
        storage: &dyn Storage,
        may_open: bool,
        idle: &mut Option<Idle>,
        boost_free: bool,
    ) -> Option<Outcome> {
        let now = now_ms();
        if now > self.deadline && !matches!(self.phase, Phase::Parked) {
            return Some(Err("timed out"));
        }
        if net.address().is_none() && !matches!(self.phase, Phase::Waiting | Phase::Parked) {
            // The address went: what was open on it is gone.
            return Some(Err("no network"));
        }
        match self.phase {
            Phase::Waiting => self.start(net, storage, may_open, now, idle, boost_free),
            Phase::Parked => None,
            Phase::Resolving(query, port) => {
                let resolved = net.resolved(query);
                if resolved.is_some() {
                    // The lookup is over (smoltcp freed it): nothing to stop.
                    self.phase = Phase::Waiting;
                }
                match resolved {
                    None => (now > self.until).then_some(Err("no such host")),
                    Some(Ok(address)) => self.connect(net, storage, address, port, boost_free),
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
        idle: &mut Option<Idle>,
        boost_free: bool,
    ) -> Option<Outcome> {
        if net.address().is_none() || !may_open {
            return (now > self.until).then_some(Err("no network"));
        }
        let Some(url) = Url::parse(&self.url) else {
            return Some(Err("not an address"));
        };
        let port = url.port;
        if self.can_reuse(idle.as_ref())
            && let Some(kept) = idle.take()
        {
            // No lookup, no handshake: the request goes over the open
            // connection.
            let (upload, request) = match self.prepare(storage, &url) {
                Ok(prepared) => prepared,
                Err(reason) => {
                    net.release(kept.link.socket);
                    return Some(Err(reason));
                }
            };
            self.reused = true;
            return self.send_over(kept.link, upload, request);
        }
        match net.resolve(url.host) {
            Ok(Resolve::Done(address)) => self.connect(net, storage, address, port, boost_free),
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
        let mut rounds = 0;
        let ended = loop {
            let (moved, ended) = match receive(
                link,
                net,
                parser,
                &mut self.sink,
                &mut self.got,
                max,
                storage,
            ) {
                Ok(received) => received,
                Err(problem) => return Some(Err(problem)),
            };
            progress |= moved;
            // Over TLS the connection's bytes pass 2 KB at a time: where
            // more waits and the answer goes on, they pass again in this
            // step, not one pass of the main loop for each 2 KB.
            rounds += 1;
            if !moved
                || ended
                || rounds >= TLS_ROUNDS
                || !link.secure()
                || parser.done()
                || parser.head().is_some_and(|head| head.is_redirect())
            {
                break ended;
            }
            match link.pump(net) {
                Ok(moved) => progress |= moved,
                Err(problem) => return Some(Err(problem.reason())),
            }
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

    /// The request's head and the file it sends, if any.
    fn prepare(
        &self,
        storage: &dyn Storage,
        url: &Url<'_>,
    ) -> Result<(Option<Upload>, Vec<u8>), &'static str> {
        let upload = match &self.call.body_from {
            Some(path) => match storage.size(path) {
                Ok(size) => Some(Upload {
                    path: path.clone(),
                    size,
                    at: 0,
                }),
                Err(_) => return Err("could not read the file"),
            },
            None => None,
        };
        let body = self.call.body.as_deref();
        let body_len = match &upload {
            Some(upload) => Some(upload.size as usize),
            None => body.map(<[u8]>::len),
        };
        // A `GET` asks the server to keep the connection for the next
        // request (`Idle`); what changes something is made on its own.
        let keep = self.call.method.eq_ignore_ascii_case("GET");
        let request = paperpaw_http::request_keeping(
            &self.call.method,
            url,
            &self.call.headers,
            body_len,
            keep,
        );
        Ok((upload, request))
    }

    /// The request goes over `link`.
    fn send_over(
        &mut self,
        link: Link,
        upload: Option<Upload>,
        request: Vec<u8>,
    ) -> Option<Outcome> {
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

    /// Opens the connection and sends the request.
    fn connect(
        &mut self,
        net: &mut Net,
        storage: &dyn Storage,
        address: smoltcp::wire::IpAddress,
        port: u16,
        boost_free: bool,
    ) -> Option<Outcome> {
        let Some(url) = Url::parse(&self.url) else {
            return Some(Err("not an address"));
        };
        // Before anything is opened: what cannot be made fails first.
        let (upload, request) = match self.prepare(storage, &url) {
            Ok(prepared) => prepared,
            Err(reason) => return Some(Err(reason)),
        };
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
        // A large download's larger window and chunks, where the heap keeps
        // its largest block (and for one download at a time).
        let plan = download_boost::plan(
            BUFFERS.receive,
            self.call.save_to.is_some(),
            // A download the user waits for is worth the buffers, whatever
            // its cap; others by the cap (`BOOST_MIN_FILE`).
            if self.foreground {
                usize::MAX
            } else {
                self.call.max_response
            },
            // A lean request never takes the larger pair.
            boost_free && !self.lean,
            cfg!(feature = "esp32c3"),
            crate::largest_block(),
        );
        let opened = if plan.boosted {
            match net.open(
                (address, port),
                net::Buffers {
                    receive: plan.receive,
                    send: BUFFERS.send,
                },
            ) {
                Err(net::Error::Memory) => net.open((address, port), BUFFERS),
                other => {
                    self.boosted = other.is_ok();
                    other
                }
            }
        } else {
            net.open((address, port), BUFFERS)
        };
        let socket = match opened {
            Ok(socket) => socket,
            Err(net::Error::Memory) => return Some(Err("not enough memory for a connection")),
            Err(_) => return Some(Err("could not connect")),
        };
        if self.boosted
            && let Sink::File(file) = &mut self.sink
        {
            file.set_chunk(plan.chunk);
        }
        log::debug!(
            "http: window {} B, chunk {} B, heap {}",
            if self.boosted {
                plan.receive
            } else {
                BUFFERS.receive
            },
            if self.boosted {
                plan.chunk
            } else {
                download_boost::BASE_CHUNK
            },
            heap()
        );
        self.send_over(Link::new(socket, tls), upload, request)
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
        self.boosted = false;
        self.reused = false;
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
            Sink::Memory(body, ..) => {
                // Kept to the most the request takes.
                answer.truncated = self.got > body.len() as u64;
                answer.body = body;
            }
            Sink::File(file) => {
                let target = self.call.save_to.as_deref().unwrap_or_default();
                if file.finish(storage, target).is_err() {
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
        // Core's staged file is core's to remove (`FileSink::discard`).
        if let Sink::File(file) = &self.sink {
            file.discard(storage);
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
        _ if reason.starts_with("not enough memory") => HttpError::Refused,
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
    let mut expected = parser.head().and_then(|head| head.length);
    let received = link.receive(net, &mut || !stop.get(), &mut |data| {
        let had_head = parser.head().is_some();
        let fed = parser.feed(data, &mut |piece| {
            if had_head {
                if let Err(e) = keep(sink, got, max, piece, expected, storage) {
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
            && matches!(sink, Sink::File(_))
        {
            *sink = Sink::Memory(Vec::new(), ERROR_BODY, false);
        }
        let redirect = parser.head().is_some_and(|head| head.is_redirect());
        if !had_head {
            expected = parser.head().and_then(|head| head.length);
        }
        if !early.is_empty() && !redirect {
            if let Err(e) = keep(sink, got, max, &early, expected, storage) {
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
    expected: Option<u64>,
    storage: &dyn Storage,
) -> Result<(), &'static str> {
    *got += piece.len() as u64;
    match sink {
        Sink::Memory(body, limit, once) => {
            let piece = &piece[..paperpaw_x4::body_room::fit(body.len(), piece.len(), *limit)];
            // Core's requests take the room once (what the request reserved),
            // or in exact steps; interface 6 grows from the pieces (a small
            // answer stays small). Never past the limit, and without failing
            // hard where memory ends.
            let capacity = if *once {
                paperpaw_x4::body_room::capacity_for(
                    body.capacity(),
                    body.len(),
                    piece.len(),
                    *limit,
                    expected.map(|length| usize::try_from(length).unwrap_or(usize::MAX)),
                )
            } else {
                paperpaw_x4::body_room::capacity_grown(
                    body.capacity(),
                    body.len(),
                    piece.len(),
                    *limit,
                )
            };
            if let Some(capacity) = capacity
                && body.try_reserve_exact(capacity - body.len()).is_err()
            {
                // Where doubling fails (interface 6), the exact size is tried
                // before giving up, as in Beta 3.
                let need = body.len() + piece.len();
                if *once || need == capacity || body.try_reserve_exact(piece.len()).is_err() {
                    return Err("the answer is too large for the memory of this device");
                }
            }
            body.extend_from_slice(piece);
        }
        Sink::File(file) => {
            if *got > max {
                return Err("too large");
            }
            file.push(storage, piece).map_err(|e| e.reason())?;
        }
    }
    Ok(())
}
