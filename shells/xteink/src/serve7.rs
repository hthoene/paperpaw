//! The servers of interface 7 (`core.net` `serve`, `docs/abi/network.md` 5):
//! the connections of `serve` (a few sockets waiting per server, one request
//! on each, its head read by `paperpaw_http::server`) with core deciding on
//! each request and owning every number: the shell reports a head and is told
//! to accept it (with the storage path its body goes to), refuse it with a
//! status, or wait; it appends the body there as it comes and reports how far
//! it is; the answer comes from core (`reply`), a file read from storage as
//! the connection takes it, so neither a large upload nor a large download
//! needs to fit in memory.
//!
//! Nothing here calls into core: [`Servers7::poll`] returns what happened as
//! [`Event`]s, the caller reports them after it let go of its tables, and
//! hands core's decisions back as [`Verdict`]s at the next step.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_http::server as http;
use paperpaw_platform::{Instant, Storage};
use smoltcp::socket::tcp;

use crate::net::{Handle, Net};
use crate::serve::{
    BUFFERS, CLOSE_MS, HEAD_MS, MORE_PORTS, SLOTS, STALL_MS, drop_received, lost, part_done,
};

/// Bytes of a request's body gathered before they are appended to its file
/// (the 4 KiB the request reserved from the pool).
const PART: usize = 4096;
/// How long a head waits before it is asked about again after core said wait.
const ASK_AGAIN_MS: u64 = 250;

/// What a request's head said.
#[derive(Clone)]
pub struct Head {
    pub method: String,
    pub target: String,
    /// The bytes of the body that follow (0 for none).
    pub length: u64,
    pub chunked: bool,
    /// The head's own bytes.
    pub head_len: u32,
}

/// What the servers have to tell core.
pub enum Event {
    /// A head came on connection `conn` of `server`.
    Head { server: u32, conn: u32, head: Head },
    /// `added` more bytes of the body of `request` were written (`received` in
    /// all).
    Body {
        request: u32,
        received: u64,
        added: u64,
    },
    /// The body of `request` is whole.
    Whole { request: u32, total: u64 },
    /// The connection of `request` ended.
    Gone { request: u32 },
}

/// What a verdict of core is about.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// The head on a connection.
    Conn(u32),
    /// A request's body or end.
    Request(u32),
}

/// Core's decision.
pub enum Verdict {
    /// Receive the body at `path`.
    Accept { request: u32, path: String },
    /// Answer with this status and close.
    Reject(u32),
    /// Ask again in a moment.
    Busy,
    /// The request or server is gone in core: the connection ends.
    Drop,
}

/// The answer to a request, owned.
pub struct Reply {
    pub request: u32,
    pub status: u16,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// A file to send: its path and its size, where core found it.
    pub file: Option<(String, Option<u64>)>,
}

struct File {
    path: String,
    size: u64,
}

enum Phase {
    /// Waiting for a connection.
    Listening,
    /// Reading the request's head: what came so far.
    Head(Vec<u8>),
    /// Core was told of the head and decides; `first` is what came after the
    /// head.
    Asking {
        head: Head,
        first: Vec<u8>,
        /// The head is with core (else it is told when `ask_at` comes).
        told: bool,
        ask_at: u64,
    },
    /// Writing the body to `path`.
    Receiving {
        request: u32,
        path: String,
        left: u64,
        received: u64,
        part: Vec<u8>,
    },
    /// Core has the request and is to answer.
    Answer { request: u32 },
    /// Sending the answer's head (and body), then the file, if any.
    Sending {
        out: Vec<u8>,
        sent: usize,
        file: Option<File>,
        at: u64,
        request: Option<u32>,
    },
    /// Sent; waiting for the connection to close.
    Closing,
}

struct Slot {
    id: u32,
    socket: Handle,
    phase: Phase,
    /// When the phase started, or last made progress.
    since: u64,
}

struct Server {
    number: u32,
    port: u16,
    slots: Vec<Slot>,
}

/// The servers of all plugins.
#[derive(Default)]
pub struct Servers7 {
    servers: Vec<Server>,
    next_conn: u32,
    /// The network the servers listen on.
    generation: u32,
}

/// Fallible copy of a string.
fn own(text: &str) -> Option<String> {
    let mut owned = String::new();
    owned.try_reserve_exact(text.len()).ok()?;
    owned.push_str(text);
    Some(owned)
}

/// `event` into `events`, or `false` where there is no room for it.
fn tell(events: &mut Vec<Event>, event: Event) -> bool {
    if events.try_reserve(1).is_ok() {
        events.push(event);
        true
    } else {
        false
    }
}

impl Servers7 {
    /// Whether a connection is moving data (or about to): the main loop comes
    /// by again soon.
    pub fn moving(&self) -> bool {
        self.servers.iter().flat_map(|s| &s.slots).any(|slot| {
            matches!(
                slot.phase,
                Phase::Head(_)
                    | Phase::Asking { .. }
                    | Phase::Receiving { .. }
                    | Phase::Sending { .. }
            )
        })
    }

    /// Whether a server is listening.
    pub fn any(&self) -> bool {
        !self.servers.is_empty()
    }

    /// The port a server of `want` would get among these servers and the
    /// `taken` ones: the first of it and the 20 after it that is free.
    pub fn free_port(&self, want: u16, taken: &[u16]) -> Option<u16> {
        let busy = |p: u16| taken.contains(&p) || self.servers.iter().any(|s| s.port == p);
        (want..=want.saturating_add(MORE_PORTS)).find(|&p| p != 0 && !busy(p))
    }

    /// Starts listening on `port` for `server`; `false` where no socket could
    /// be had (nothing is left half done).
    pub fn listen(&mut self, server: u32, port: u16, net: &mut Net) -> bool {
        if self.servers.is_empty() {
            self.generation = net.generation();
        }
        let mut slots = Vec::new();
        if slots.try_reserve_exact(SLOTS).is_err() || self.servers.try_reserve(1).is_err() {
            return false;
        }
        for _ in 0..SLOTS {
            match net.listen(port, BUFFERS) {
                Ok(socket) => {
                    self.next_conn = self.next_conn.wrapping_add(1);
                    slots.push(Slot {
                        id: self.next_conn,
                        socket,
                        phase: Phase::Listening,
                        since: 0,
                    });
                }
                Err(_) => break,
            }
        }
        if slots.is_empty() {
            return false;
        }
        self.servers.push(Server {
            number: server,
            port,
            slots,
        });
        true
    }

    /// Stops `server`: its sockets go, and the requests on them end.
    pub fn stop(&mut self, server: u32, net: Option<&mut Net>, events: &mut Vec<Event>) {
        let Some(at) = self.servers.iter().position(|s| s.number == server) else {
            return;
        };
        let server = self.servers.remove(at);
        let mut net = net;
        for slot in server.slots {
            if let Some(net) = net.as_deref_mut() {
                net.release(slot.socket);
            }
            if let Some(request) = slot.request() {
                let _ = tell(events, Event::Gone { request });
            }
        }
    }

    /// The network went (or is another one now): every server ends, and no
    /// request is told (core was told the network is lost). The sockets are
    /// let go on `net` if they were opened on it.
    pub fn lost(&mut self, net: Option<&mut Net>) {
        let mut net = net.filter(|net| net.generation() == self.generation);
        for server in core::mem::take(&mut self.servers) {
            for slot in &server.slots {
                if let Some(net) = net.as_deref_mut() {
                    net.release(slot.socket);
                }
            }
        }
    }

    /// Whether the network these servers listen on is another one now.
    pub fn stale(&self, net: &Net) -> bool {
        self.any() && net.generation() != self.generation
    }

    /// The answer of core to a request: the connection sends it.
    pub fn reply(&mut self, reply: Reply, now: Instant) {
        let Some(slot) = self.slots().find(|slot| {
            matches!(
                slot.phase,
                Phase::Answer { request } | Phase::Receiving { request, .. } if request == reply.request
            )
        }) else {
            return;
        };
        let (status, file) = match reply.file {
            Some((path, Some(size))) => (reply.status, Some(File { path, size })),
            // A file that is not there: not found, whatever was asked.
            Some((_, None)) => (404, None),
            None => (reply.status, None),
        };
        let length = file.as_ref().map_or(reply.body.len() as u64, |f| f.size);
        let mut out = http::response_head(status, &reply.content_type, &reply.headers, length);
        if file.is_none() && out.try_reserve(reply.body.len()).is_ok() {
            out.extend_from_slice(&reply.body);
        }
        slot.phase = Phase::Sending {
            out,
            sent: 0,
            file,
            at: 0,
            request: Some(reply.request),
        };
        slot.since = now.0;
    }

    /// Core ends `request`: `status` 0 drops the connection, another answers
    /// with it and closes.
    pub fn close_request(&mut self, request: u32, status: u32, net: &mut Net, now: Instant) {
        let Some(slot) = self.slots().find(|slot| slot.request() == Some(request)) else {
            return;
        };
        if status == 0 {
            slot.drop_connection(net);
        } else {
            slot.refuse(u16::try_from(status).unwrap_or(503), now);
        }
    }

    fn slots(&mut self) -> impl Iterator<Item = &mut Slot> {
        self.servers.iter_mut().flat_map(|s| s.slots.iter_mut())
    }

    /// One step for every connection: `verdicts` are core's decisions since
    /// the last step; what happened goes into `events`.
    pub fn poll(
        &mut self,
        net: &mut Net,
        storage: &dyn Storage,
        now: Instant,
        verdicts: &mut Vec<(Key, Verdict)>,
        events: &mut Vec<Event>,
    ) {
        for server in &mut self.servers {
            let number = server.number;
            let port = server.port;
            for slot in &mut server.slots {
                slot.step(number, port, net, storage, now, verdicts, events);
            }
        }
    }
}

/// Takes core's verdict for `key` out of `verdicts`.
fn verdict(verdicts: &mut Vec<(Key, Verdict)>, key: Key) -> Option<Verdict> {
    let at = verdicts.iter().position(|(k, _)| *k == key)?;
    Some(verdicts.swap_remove(at).1)
}

impl Slot {
    /// The request core knows of on this connection, if any.
    fn request(&self) -> Option<u32> {
        match &self.phase {
            Phase::Receiving { request, .. } | Phase::Answer { request } => Some(*request),
            Phase::Sending { request, .. } => *request,
            _ => None,
        }
    }

    /// Ends the connection at once; the socket waits for the next.
    fn again(&mut self, net: &mut Net, port: u16) {
        net.listen_again(self.socket, port);
        self.phase = Phase::Listening;
    }

    /// The connection ends without an answer (no request is told).
    fn drop_connection(&mut self, net: &mut Net) {
        // Back to listening on the same port is the server's: the slot is
        // given its port by the caller of `step`, so only mark it closing.
        net.socket(self.socket).abort();
        self.phase = Phase::Closing;
    }

    /// Answers at once, without the plugin.
    fn refuse(&mut self, status: u16, now: Instant) {
        self.phase = Phase::Sending {
            out: http::response_head(status, "text/plain", &[], 0),
            sent: 0,
            file: None,
            at: 0,
            request: None,
        };
        self.since = now.0;
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    fn step(
        &mut self,
        server: u32,
        port: u16,
        net: &mut Net,
        storage: &dyn Storage,
        now: Instant,
        verdicts: &mut Vec<(Key, Verdict)>,
        events: &mut Vec<Event>,
    ) {
        match &self.phase {
            Phase::Listening => {
                // Connected (not while the handshake goes on).
                if !lost(net.socket(self.socket)) {
                    self.phase = Phase::Head(Vec::new());
                    self.since = now.0;
                }
            }
            Phase::Head(_) => self.read_head(port, net, now),
            Phase::Asking { .. } => self.asking(server, port, net, now, verdicts, events),
            Phase::Receiving { .. } => self.receive(port, net, storage, now, verdicts, events),
            Phase::Answer { request } => {
                let request = *request;
                if matches!(
                    verdict(verdicts, Key::Request(request)),
                    Some(Verdict::Drop)
                ) {
                    self.again(net, port);
                } else if lost(net.socket(self.socket)) {
                    let _ = tell(events, Event::Gone { request });
                    self.again(net, port);
                }
            }
            Phase::Sending { .. } => self.send(port, net, storage, now.0, events),
            Phase::Closing => self.close(port, net, now.0),
        }
    }

    /// Reads the request's head; once it is whole, core is asked.
    fn read_head(&mut self, port: u16, net: &mut Net, now: Instant) {
        let socket = net.socket(self.socket);
        let Phase::Head(head) = &mut self.phase else {
            return;
        };
        while socket.can_recv() && head.len() < http::MAX_REQUEST_HEAD {
            let room = http::MAX_REQUEST_HEAD - head.len();
            if head.try_reserve(room.min(1024)).is_err() {
                break;
            }
            let got = socket.recv(|data| {
                let n = data.len().min(room).min(head.capacity() - head.len());
                head.extend_from_slice(&data[..n]);
                (n, n)
            });
            if !matches!(got, Ok(n) if n > 0) {
                break;
            }
        }
        match http::parse_request(head) {
            Ok(None) if lost(socket) || now.0 > self.since + HEAD_MS => self.again(net, port),
            Ok(None) => {}
            Err(_) => self.refuse(400, now),
            Ok(Some((request, used))) => {
                let first = head.split_off(used);
                let head_len = used as u32;
                self.phase = Phase::Asking {
                    head: Head {
                        method: request.method,
                        target: request.target,
                        length: request.length,
                        chunked: request.chunked,
                        head_len,
                    },
                    first,
                    told: false,
                    ask_at: 0,
                };
                self.since = now.0;
            }
        }
    }

    /// Tells core of the head (once, and again when it said wait), and acts
    /// on what it decided.
    fn asking(
        &mut self,
        server: u32,
        port: u16,
        net: &mut Net,
        now: Instant,
        verdicts: &mut Vec<(Key, Verdict)>,
        events: &mut Vec<Event>,
    ) {
        match verdict(verdicts, Key::Conn(self.id)) {
            Some(Verdict::Accept { request, path }) => {
                let Phase::Asking { head, first, .. } =
                    core::mem::replace(&mut self.phase, Phase::Closing)
                else {
                    return;
                };
                self.since = now.0;
                self.accepted(request, path, head.length, first, now, events);
            }
            Some(Verdict::Reject(status)) => self.refuse(u16::try_from(status).unwrap_or(503), now),
            Some(Verdict::Busy) => {
                if let Phase::Asking { told, ask_at, .. } = &mut self.phase {
                    *told = false;
                    *ask_at = now.0 + ASK_AGAIN_MS;
                }
            }
            Some(Verdict::Drop) => self.again(net, port),
            None => {}
        }
        let Phase::Asking {
            head, told, ask_at, ..
        } = &mut self.phase
        else {
            return;
        };
        if *told {
            if lost(net.socket(self.socket)) {
                // The client left while core decided.
                self.again(net, port);
            }
            return;
        }
        if now.0 < *ask_at {
            return;
        }
        let (Some(method), Some(target)) = (own(&head.method), own(&head.target)) else {
            self.refuse(503, now);
            return;
        };
        let copy = Head {
            method,
            target,
            length: head.length,
            chunked: head.chunked,
            head_len: head.head_len,
        };
        if tell(
            events,
            Event::Head {
                server,
                conn: self.id,
                head: copy,
            },
        ) {
            *told = true;
        }
    }

    /// Core accepted the request: its body goes to `path`.
    fn accepted(
        &mut self,
        request: u32,
        path: String,
        length: u64,
        mut first: Vec<u8>,
        now: Instant,
        events: &mut Vec<Event>,
    ) {
        if length == 0 {
            let _ = tell(events, Event::Whole { request, total: 0 });
            self.phase = Phase::Answer { request };
            return;
        }
        // What came with the head is the body's start.
        first.truncate(
            first
                .len()
                .min(usize::try_from(length).unwrap_or(usize::MAX)),
        );
        let received = first.len() as u64;
        self.phase = Phase::Receiving {
            request,
            path,
            left: length - received,
            received: 0,
            part: first,
        };
        self.since = now.0;
    }

    /// Takes the body, appending it to its file in parts of [`PART`] bytes:
    /// fewer, larger writes to the card.
    fn receive(
        &mut self,
        port: u16,
        net: &mut Net,
        storage: &dyn Storage,
        now: Instant,
        verdicts: &mut Vec<(Key, Verdict)>,
        events: &mut Vec<Event>,
    ) {
        let ms = now.0;
        let socket = net.socket(self.socket);
        let Phase::Receiving {
            request,
            path,
            left,
            received,
            part,
        } = &mut self.phase
        else {
            return;
        };
        let request = *request;
        // Core ended it (a limit, the card's reserve, a stop): drop or answer.
        match verdict(verdicts, Key::Request(request)) {
            Some(Verdict::Reject(status)) => {
                self.refuse(u16::try_from(status).unwrap_or(503), now);
                return;
            }
            Some(Verdict::Drop) => {
                self.again(net, port);
                return;
            }
            _ => {}
        }
        let mut failed = false;
        loop {
            // A whole part (or the last one) is appended and told.
            if part.len() >= PART || (*left == 0 && !part.is_empty()) {
                let added = part.len() as u64;
                if storage.append(path, part).is_err() {
                    failed = true;
                    break;
                }
                part.clear();
                *received += added;
                self.since = ms;
                let body = Event::Body {
                    request,
                    received: *received,
                    added,
                };
                if !tell(events, body) {
                    failed = true;
                    break;
                }
                continue;
            }
            if *left == 0 || !socket.can_recv() {
                break;
            }
            let want = (*left).min((PART - part.len()) as u64) as usize;
            if part.capacity() < PART && part.try_reserve_exact(PART - part.len()).is_err() {
                failed = true;
                break;
            }
            let got = socket.recv(|data| {
                let n = data.len().min(want);
                part.extend_from_slice(&data[..n]);
                (n, n)
            });
            match got {
                Ok(n) if n > 0 => *left -= n as u64,
                _ => break,
            }
        }
        if failed {
            // The card (or memory) failed: answered, and core is told.
            let _ = tell(events, Event::Gone { request });
            self.refuse(507, now);
        } else if *left == 0 {
            let total = *received;
            let _ = tell(events, Event::Whole { request, total });
            self.phase = Phase::Answer { request };
        } else if lost(socket) || ms > self.since + STALL_MS {
            let _ = tell(events, Event::Gone { request });
            self.again(net, port);
        }
    }

    /// Sends the answer's head (and body), then the file, if any; closes once
    /// all went, and tells core the request is over.
    fn send(
        &mut self,
        port: u16,
        net: &mut Net,
        storage: &dyn Storage,
        ms: u64,
        events: &mut Vec<Event>,
    ) {
        let socket = net.socket(self.socket);
        drop_received(socket);
        let Phase::Sending {
            out,
            sent,
            file,
            at,
            request,
        } = &mut self.phase
        else {
            return;
        };
        let mut ended = false;
        while *sent < out.len() && socket.can_send() {
            match socket.send_slice(&out[*sent..]) {
                Ok(n) if n > 0 => {
                    if part_done(*sent as u64, (*sent + n) as u64) {
                        self.since = ms;
                    }
                    *sent += n;
                }
                _ => break,
            }
        }
        if *sent == out.len() {
            if out.capacity() > 0 {
                // Sent: its memory is not needed while a file goes.
                *out = Vec::new();
                *sent = 0;
            }
            match file {
                Some(f) if *at < f.size => {
                    ended = !send_file(socket, storage, f, at, &mut self.since, ms);
                }
                _ => {
                    socket.close();
                    if let Some(request) = request.take() {
                        let _ = tell(events, Event::Gone { request });
                    }
                    self.phase = Phase::Closing;
                    self.since = ms;
                    return;
                }
            }
        }
        if ended || lost(socket) || ms > self.since + STALL_MS {
            if let Some(request) = request.take() {
                let _ = tell(events, Event::Gone { request });
            }
            self.again(net, port);
        }
    }

    /// Closed once all was sent; the other side closes too (or it is given up
    /// on, a while after it has everything).
    fn close(&mut self, port: u16, net: &mut Net, ms: u64) {
        let socket = net.socket(self.socket);
        drop_received(socket);
        if socket.send_queue() > 0 {
            // Still going out: progress keeps it waiting.
            if ms > self.since + STALL_MS {
                self.again(net, port);
            }
            return;
        }
        if matches!(socket.state(), tcp::State::Closed | tcp::State::TimeWait)
            || ms > self.since + CLOSE_MS
        {
            self.again(net, port);
        }
    }
}

/// As much of `file` from `at` on as the socket takes, `since` set when a part
/// of it went; `false` where the file is gone or shorter than it was (the
/// connection ends early).
fn send_file(
    socket: &mut tcp::Socket<'_>,
    storage: &dyn Storage,
    file: &File,
    at: &mut u64,
    since: &mut u64,
    ms: u64,
) -> bool {
    while *at < file.size && socket.can_send() {
        let want = (file.size - *at).min(usize::MAX as u64) as usize;
        let read = socket.send(|room| {
            let room_len = room.len().min(want);
            match storage.read_at(&file.path, *at, &mut room[..room_len]) {
                Ok(n) => (n, Some(n)),
                Err(_) => (0, None),
            }
        });
        match read {
            Ok(Some(n)) if n > 0 => {
                if part_done(*at, *at + n as u64) {
                    *since = ms;
                }
                *at += n as u64;
            }
            _ => return false,
        }
    }
    true
}

/// The address of the device.
pub type Ipv4 = smoltcp::wire::Ipv4Address;

/// `ip:port` as a plugin is shown it.
pub fn address_text(ip: Ipv4, port: u16) -> String {
    format!("{ip}:{port}")
}
