//! Plugins' servers (the kernel's `ServerCall`s) over the network of
//! `net`, a step at a time from the main loop: each server has a few
//! sockets waiting for connections, one request on each (its head read by
//! `paperpaw_http::server`), passed to the kernel with its body in parts as
//! it comes, and the plugin's answer sent back, a file read from storage
//! as the connection takes it. So neither a large upload nor a large
//! download needs to fit in memory.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_core::{Effects, Kernel, ServedFile, ServerCall, ServerEvent};
use paperpaw_http::server::{self as http, RequestHead};
use paperpaw_platform::Instant;
use smoltcp::socket::tcp;

use crate::net::{self, Handle, Net};

/// Connections per server at a time; each socket waits for the next once
/// its request is answered. Two, for a page and what it loads with it.
pub(crate) const SLOTS: usize = 2;
/// A connection's buffers: uploads come in, pages and files go out.
pub(crate) const BUFFERS: net::Buffers = net::Buffers {
    receive: 8 * 1024,
    send: 4 * 1024,
};
/// Bytes of a request's body passed on at a time.
const PART: usize = 16 * 1024;
/// Ports tried after the one asked for, if it is taken.
pub(crate) const MORE_PORTS: u16 = 20;
/// How long a request's head may take to come in all, and its plugin to
/// answer; how long a connection may make no progress, and take to close.
/// Progress is a whole [`PART`] of a body or an answer moved: a client
/// trickling bytes cannot hold a connection, while a large upload or
/// download that moves takes as long as it needs.
pub(crate) const HEAD_MS: u64 = 30_000;
const ANSWER_MS: u64 = 120_000;
pub(crate) const STALL_MS: u64 = 30_000;
pub(crate) const CLOSE_MS: u64 = 2_000;

/// What a connection is at.
enum Phase {
    /// Waiting for a connection.
    Listening,
    /// Reading the request's head: what came so far.
    Head(Vec<u8>),
    /// Passing the body on: bytes still to come, and those gathered.
    Body {
        request: u32,
        left: u64,
        part: Vec<u8>,
    },
    /// The plugin has the request and is to answer.
    Answer { request: u32 },
    /// Sending the answer: the head (and body), then the file, if any.
    Sending {
        out: Vec<u8>,
        sent: usize,
        file: Option<ServedFile>,
        at: u64,
    },
    /// Sent; waiting for the connection to close.
    Closing,
}

struct Slot {
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
pub struct Servers {
    servers: Vec<Server>,
    next_request: u32,
    /// The network the servers listen on.
    generation: u32,
}

impl Servers {
    /// Whether data is moving (or about to): the main loop comes by again
    /// soon.
    pub fn moving(&self) -> bool {
        self.servers.iter().flat_map(|s| &s.slots).any(|slot| {
            matches!(
                slot.phase,
                Phase::Head(_) | Phase::Body { .. } | Phase::Sending { .. }
            )
        })
    }

    /// Does what the kernel asks.
    pub fn apply(
        &mut self,
        call: ServerCall,
        net: &mut Net,
        kernel: &mut Kernel,
        now: Instant,
    ) -> Option<Effects> {
        match call {
            ServerCall::Listen { server, port } => {
                let event = self.listen(server, port, net);
                Some(kernel.server_event(now, event))
            }
            ServerCall::Stop { server } => {
                let at = self.servers.iter().position(|s| s.number == server)?;
                let server = self.servers.remove(at);
                let mut effects = None;
                for slot in server.slots {
                    net.release(slot.socket);
                    if let Some(request) = slot.request() {
                        effects = Some(kernel.server_event(now, ServerEvent::Closed { request }));
                    }
                }
                effects
            }
            ServerCall::Respond {
                request,
                status,
                content_type,
                headers,
                body,
                file,
            } => {
                // Also while the body still comes: an upload refused on the
                // way (the card full or failing) is answered at once, the
                // rest of it let go (see `drop_received`).
                let slot = self.servers.iter_mut().flat_map(|s| &mut s.slots).find(|slot| {
                    matches!(
                        slot.phase,
                        Phase::Answer { request: r } | Phase::Body { request: r, .. } if r == request
                    )
                })?;
                let length = file.as_ref().map_or(body.len() as u64, |f| f.size);
                let mut out = http::response_head(status, &content_type, &headers, length);
                out.extend_from_slice(&body);
                slot.phase = Phase::Sending {
                    out,
                    sent: 0,
                    file,
                    at: 0,
                };
                slot.since = now.0;
                None
            }
        }
    }

    /// Starts server `server` on `port`, or the next free port.
    fn listen(&mut self, server: u32, port: u16, net: &mut Net) -> ServerEvent {
        if self.servers.is_empty() {
            self.generation = net.generation();
        }
        let taken = |p: u16| self.servers.iter().any(|s| s.port == p);
        let Some(port) = (port..port.saturating_add(MORE_PORTS)).find(|&p| !taken(p)) else {
            return ServerEvent::Failed {
                server,
                reason: format!("port {port} is taken"),
            };
        };
        let mut slots = Vec::new();
        for _ in 0..SLOTS {
            match net.listen(port, BUFFERS) {
                Ok(socket) => slots.push(Slot {
                    socket,
                    phase: Phase::Listening,
                    since: 0,
                }),
                Err(_) => break,
            }
        }
        let Some(address) = net.address() else {
            for slot in slots {
                net.release(slot.socket);
            }
            return failed(server, "no network");
        };
        if slots.is_empty() {
            return failed(server, "not enough memory");
        }
        self.servers.push(Server {
            number: server,
            port,
            slots,
        });
        ServerEvent::Started {
            server,
            address: format!("{}:{port}", address.address()),
        }
    }

    /// The network went (or is another one now): every server fails, and
    /// requests going on are forgotten. Their sockets are let go on `net`
    /// if they were opened on it (else they went with an earlier one).
    pub fn lost(
        &mut self,
        net: Option<&mut Net>,
        kernel: &mut Kernel,
        now: Instant,
    ) -> Vec<Effects> {
        let mut net = net.filter(|net| net.generation() == self.generation);
        let mut effects = Vec::new();
        for server in core::mem::take(&mut self.servers) {
            for slot in &server.slots {
                if let Some(net) = net.as_deref_mut() {
                    net.release(slot.socket);
                }
                if let Some(request) = slot.request() {
                    effects.push(kernel.server_event(now, ServerEvent::Closed { request }));
                }
            }
            let event = failed(server.number, "the network went away");
            effects.push(kernel.server_event(now, event));
        }
        effects
    }

    /// One step for every connection.
    pub fn poll(&mut self, net: &mut Net, kernel: &mut Kernel, now: Instant) -> Vec<Effects> {
        let mut effects = Vec::new();
        if self.servers.is_empty() {
            return effects;
        }
        if net.generation() != self.generation {
            return self.lost(None, kernel, now);
        }
        for server in &mut self.servers {
            for slot in &mut server.slots {
                let mut events = Vec::new();
                slot.step(
                    server.number,
                    server.port,
                    net,
                    kernel,
                    now,
                    &mut self.next_request,
                    &mut events,
                );
                for event in events {
                    effects.push(kernel.server_event(now, event));
                }
            }
        }
        effects
    }
}

fn failed(server: u32, reason: &str) -> ServerEvent {
    ServerEvent::Failed {
        server,
        reason: String::from(reason),
    }
}

/// The connection is gone (reset, or closed before its answer).
pub(crate) fn lost(socket: &tcp::Socket<'_>) -> bool {
    !matches!(
        socket.state(),
        tcp::State::Established | tcp::State::CloseWait
    )
}

impl Slot {
    /// The request the kernel knows of on this connection, if any.
    fn request(&self) -> Option<u32> {
        match self.phase {
            Phase::Body { request, .. } | Phase::Answer { request } => Some(request),
            _ => None,
        }
    }

    /// Ends the connection; the socket waits for the next.
    fn again(&mut self, net: &mut Net, port: u16) {
        net.listen_again(self.socket, port);
        self.phase = Phase::Listening;
    }

    /// Answers at once, without the plugin (a request that cannot be
    /// taken).
    fn refuse(&mut self, status: u16, now: Instant) {
        self.phase = Phase::Sending {
            out: http::response_head(status, "text/plain", &[], 0),
            sent: 0,
            file: None,
            at: 0,
        };
        self.since = now.0;
    }

    /// One step of the connection, by its phase.
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    fn step(
        &mut self,
        server: u32,
        port: u16,
        net: &mut Net,
        kernel: &Kernel,
        now: Instant,
        next_request: &mut u32,
        events: &mut Vec<ServerEvent>,
    ) {
        match self.phase {
            Phase::Listening => {
                // Connected (not while the handshake goes on).
                if !lost(net.socket(self.socket)) {
                    self.phase = Phase::Head(Vec::new());
                    self.since = now.0;
                }
            }
            Phase::Head(_) => self.read_head(server, port, net, now, next_request, events),
            Phase::Body { .. } => self.read_body(port, net, now.0, events),
            Phase::Answer { request } => {
                if lost(net.socket(self.socket)) {
                    events.push(ServerEvent::Closed { request });
                    self.again(net, port);
                } else if now.0 > self.since + ANSWER_MS {
                    events.push(ServerEvent::Closed { request });
                    self.refuse(503, now);
                }
            }
            Phase::Sending { .. } => self.send(port, net, kernel, now.0),
            Phase::Closing => self.close(port, net, now.0),
        }
    }

    /// Reads the request's head; once it is whole, the plugin hears of the
    /// request.
    fn read_head(
        &mut self,
        server: u32,
        port: u16,
        net: &mut Net,
        now: Instant,
        next_request: &mut u32,
        events: &mut Vec<ServerEvent>,
    ) {
        let socket = net.socket(self.socket);
        let Phase::Head(head) = &mut self.phase else {
            return;
        };
        while socket.can_recv() && head.len() < http::MAX_REQUEST_HEAD {
            let room = http::MAX_REQUEST_HEAD - head.len();
            let got = socket.recv(|data| {
                let n = data.len().min(room);
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
            Ok(Some((RequestHead { chunked: true, .. }, _))) => self.refuse(411, now),
            Ok(Some((request_head, used))) => {
                *next_request = next_request.wrapping_add(1);
                let first = head.split_off(used);
                self.begin(server, *next_request, request_head, first, now.0, events);
            }
        }
    }

    /// A request whose head came, `first` what came after it: the plugin
    /// hears of it, and of its body as far as it came.
    fn begin(
        &mut self,
        server: u32,
        request: u32,
        head: RequestHead,
        mut first: Vec<u8>,
        ms: u64,
        events: &mut Vec<ServerEvent>,
    ) {
        let length = head.length;
        events.push(ServerEvent::Request {
            server,
            request,
            method: head.method,
            target: head.target,
            body: length > 0,
        });
        self.since = ms;
        if length == 0 {
            self.phase = Phase::Answer { request };
            return;
        }
        // What came with the head is the body's start.
        first.truncate(
            first
                .len()
                .min(usize::try_from(length).unwrap_or(usize::MAX)),
        );
        let left = length - first.len() as u64;
        self.phase = if left == 0 {
            events.push(ServerEvent::Body {
                request,
                data: first,
                last: true,
            });
            Phase::Answer { request }
        } else {
            Phase::Body {
                request,
                left,
                part: first,
            }
        };
    }

    /// Passes the body on, gathered into parts of [`PART`] bytes: fewer,
    /// larger writes to the card.
    fn read_body(&mut self, port: u16, net: &mut Net, ms: u64, events: &mut Vec<ServerEvent>) {
        let socket = net.socket(self.socket);
        let Phase::Body {
            request,
            left,
            part,
        } = &mut self.phase
        else {
            return;
        };
        let request = *request;
        while *left > 0 && socket.can_recv() {
            let want = (*left).min((PART - part.len()) as u64) as usize;
            if part.capacity() < PART && part.try_reserve_exact(PART - part.len()).is_err() {
                break;
            }
            let got = socket.recv(|data| {
                let n = data.len().min(want);
                part.extend_from_slice(&data[..n]);
                (n, n)
            });
            let Ok(n) = got else { break };
            *left -= n as u64;
            if part.len() >= PART || *left == 0 {
                self.since = ms;
                events.push(ServerEvent::Body {
                    request,
                    data: core::mem::take(part),
                    last: *left == 0,
                });
            }
            if n == 0 {
                break;
            }
        }
        if *left == 0 {
            self.phase = Phase::Answer { request };
        } else if lost(socket) || ms > self.since + STALL_MS {
            events.push(ServerEvent::Closed { request });
            self.again(net, port);
        }
    }

    /// Sends the answer's head (and body), then the file, if any; closes
    /// once all went.
    fn send(&mut self, port: u16, net: &mut Net, kernel: &Kernel, ms: u64) {
        let socket = net.socket(self.socket);
        drop_received(socket);
        let Phase::Sending {
            out,
            sent,
            file,
            at,
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
                    ended = !send_file(socket, kernel, f, at, &mut self.since, ms);
                }
                _ => {
                    socket.close();
                    self.phase = Phase::Closing;
                    self.since = ms;
                    return;
                }
            }
        }
        if ended || lost(socket) || ms > self.since + STALL_MS {
            self.again(net, port);
        }
    }

    /// Closed once all was sent; the other side closes too (or it is given
    /// up on, a while after it has everything).
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

/// Lets go of what the client still sends once it is answered (a body
/// refused on the way): its window stays open, so it is not held up
/// before it reads the answer.
pub(crate) fn drop_received(socket: &mut tcp::Socket<'_>) {
    while socket.can_recv() {
        if !matches!(socket.recv(|data| (data.len(), data.len())), Ok(n) if n > 0) {
            break;
        }
    }
}

/// As much of `file` from `at` on as the socket takes, `since` set when a
/// part of it went; false where the file is gone or shorter than it was
/// (the connection ends early).
fn send_file(
    socket: &mut tcp::Socket<'_>,
    kernel: &Kernel,
    file: &ServedFile,
    at: &mut u64,
    since: &mut u64,
    ms: u64,
) -> bool {
    while *at < file.size && socket.can_send() {
        let want = (file.size - *at).min(usize::MAX as u64) as usize;
        let read = socket.send(|room| {
            let room_len = room.len().min(want);
            match kernel.read_served(&file.path, *at, &mut room[..room_len]) {
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

/// Whether moving on from `from` to `to` bytes finished a [`PART`]: only
/// that counts as progress (see [`HEAD_MS`]).
pub(crate) fn part_done(from: u64, to: u64) -> bool {
    from / PART as u64 != to / PART as u64
}
