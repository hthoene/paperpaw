//! Plugins' sockets (the kernel's `SocketCall`s, see ADR 0014) over the
//! network of `net`, a step at a time from the main loop: TCP connections
//! that read only when asked (so what waits for the kernel is at most one
//! part per socket), and UDP broadcasts that collect answers for a while
//! (e.g. to find Calibre in the local network).
//!
//! TLS: a connection made secure from the start, or switched to it once
//! open (STARTTLS), is a `tls::Session` over a `link::Link`, as an HTTPS
//! request is, in one of the device's few TLS slots (`tls_slots`, shared
//! with HTTPS: first asked, first served; none free for as long as the
//! plugin allowed is `Busy`). The plugin hears `Secured` once the handshake
//! is done, with the server's certificate, or `Untrusted` where it is not
//! trusted, and what it sent and read before waited (notes
//! `designs/tls-sockets.md`, `designs/tls-sockets-sdk.md`).

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_core::sdk::{SocketError, Trust};
use paperpaw_core::{Effects, Kernel, SocketCall, SocketEvent, TlsSetup};
use paperpaw_platform::Instant;
use paperpaw_x4::tls_trust::{self, Leaf, Outcome};
use paperpaw_x4::tls_watch::{Stage, Watch};
use smoltcp::socket::tcp;
use smoltcp::wire::IpAddress;

use crate::link::{Failure, Link};
use crate::net::{self, Handle, Net, Query, Resolve};
use crate::tls::{self, Read};
use crate::tls_slots::{Slot, Waiter};

/// Connections at a time.
const MAX_SOCKETS: usize = 2;
/// A connection's buffers: much to receive (books), little to send.
const BUFFERS: net::Buffers = net::Buffers {
    receive: 8 * 1024,
    send: 2048,
};
/// Bytes of a large read passed on at a time.
const PART: usize = 16 * 1024;
/// How long finding the host and connecting may take.
const CONNECT_MS: u64 = 10_000;
/// A quiet connection asks the other side this often whether it is there.
const KEEP_ALIVE_MS: u64 = 15_000;
/// A connection whose other side answers nothing for this long ends.
const GIVE_UP_MS: u64 = 45_000;
/// A secure socket idle for this long gives its slot up where another
/// request or socket waits for one (see `tls_slots::pressed`).
const YIELD_AFTER_MS: u64 = 5_000;
/// Plain bytes read from a secure connection at a time.
const READ_PART: usize = 1024;
/// Answers to a broadcast taken at most, and their bytes in all.
const ANSWERS: usize = 8;
const ANSWER_BYTES: usize = 1024;

enum Phase {
    /// A secure connection in line for a TLS slot, before the host is
    /// looked up (no radio or memory used meanwhile).
    Queued {
        waiter: Waiter,
        host: String,
        port: u16,
    },
    Resolving {
        query: Query,
        port: u16,
    },
    Connecting(Handle),
    Open(Handle),
    /// An open connection switching to TLS, in line for a slot.
    Switching {
        handle: Handle,
        waiter: Waiter,
    },
    /// TLS's handshake going on over the connection.
    Handshake(Link),
    /// Secure.
    Secure(Link),
    /// Nothing left to let go (a lookup that failed is gone by itself).
    Ended,
}

/// What a socket that is secure, or becomes it, has besides.
struct Secure {
    setup: TlsSetup,
    watch: Watch,
    /// The slot, from when it is given until the session takes it.
    slot: Option<Slot>,
    /// The heap used when the session was made, for the log.
    used_before: usize,
}

impl Secure {
    fn new(setup: TlsSetup, now: Instant) -> Self {
        let watch = Watch::new(now.0, setup.timeout_ms, setup.idle_ms);
        Self {
            setup,
            watch,
            slot: None,
            used_before: 0,
        }
    }
}

struct Socket {
    number: u32,
    phase: Phase,
    /// When connecting gives up.
    until: u64,
    /// Reads asked for (their most bytes), answered in turn.
    reads: VecDeque<u32>,
    /// What is still to send, and how much of the first was sent.
    sends: VecDeque<Vec<u8>>,
    sent: usize,
    /// Received for the first read, not passed on yet.
    gathered: Vec<u8>,
    /// Asked to be secure.
    secure: Option<Secure>,
}

struct Broadcast {
    number: u32,
    udp: Handle,
    until: u64,
}

/// The sockets and broadcasts of all plugins.
#[derive(Default)]
pub struct Sockets {
    sockets: Vec<Socket>,
    broadcasts: Vec<Broadcast>,
    /// The network they are open on.
    generation: u32,
    /// Something came or went in the last step.
    moved: bool,
    /// An answer to a broadcast says its sender's port too (interface 7).
    with_ports: bool,
}

impl Sockets {
    #[cfg(feature = "ppn5")]
    /// Answers to a broadcast say their sender's address and port, as
    /// interface 7 has them (interface 6 has the address alone).
    pub fn answers_with_ports(&mut self) {
        self.with_ports = true;
    }

    /// Whether a socket or broadcast is open.
    pub fn any(&self) -> bool {
        !self.sockets.is_empty() || !self.broadcasts.is_empty()
    }

    /// Whether data is moving (or about to): the main loop comes by again
    /// soon.
    pub fn moving(&self) -> bool {
        self.moved
            || self.sockets.iter().any(|s| {
                !s.sends.is_empty()
                    || !s.gathered.is_empty()
                    || matches!(s.phase, Phase::Handshake(_) | Phase::Switching { .. })
            })
    }

    /// Does what the kernel asks; what it says at once.
    pub fn apply(&mut self, call: SocketCall, net: &mut Net, now: Instant) -> Option<SocketEvent> {
        if !self.any() {
            self.generation = net.generation();
        }
        match call {
            SocketCall::Connect {
                socket,
                host,
                port,
                tls: Some(setup),
            } => {
                if self.sockets.len() >= MAX_SOCKETS {
                    return Some(failed(socket, SocketError::TooMany, "too many connections"));
                }
                // In line for a TLS slot first: nothing is looked up or
                // opened until one is free.
                self.sockets.push(Socket {
                    number: socket,
                    phase: Phase::Queued {
                        waiter: Waiter::ask(),
                        host,
                        port,
                    },
                    until: now.0 + CONNECT_MS,
                    reads: VecDeque::new(),
                    sends: VecDeque::new(),
                    sent: 0,
                    gathered: Vec::new(),
                    secure: Some(Secure::new(setup, now)),
                });
                None
            }
            SocketCall::StartTls { socket, tls } => {
                let at = self.sockets.iter().position(|s| s.number == socket)?;
                let socket_ref = &mut self.sockets[at];
                // Only an open, plain connection with no read under way;
                // else it is closed, not left plain: what it was to send
                // secure would go in the clear.
                if let Phase::Open(handle) = socket_ref.phase
                    && socket_ref.secure.is_none()
                    && socket_ref.reads.is_empty()
                {
                    socket_ref.secure = Some(Secure::new(tls, now));
                    socket_ref.phase = Phase::Switching {
                        handle,
                        waiter: Waiter::ask(),
                    };
                    return None;
                }
                self.sockets.remove(at).release(net);
                Some(failed(
                    socket,
                    SocketError::NotReady,
                    "the connection is not open and plain, or is being read",
                ))
            }
            SocketCall::Connect {
                socket,
                host,
                port,
                tls: None,
            } => {
                if self.sockets.len() >= MAX_SOCKETS {
                    return Some(failed(socket, SocketError::TooMany, "too many connections"));
                }
                let phase = match net.resolve(&host) {
                    Ok(Resolve::Done(address)) => match connect(net, address, port) {
                        Ok(phase) => phase,
                        Err(reason) => return Some(failed(socket, SocketError::Refused, reason)),
                    },
                    Ok(Resolve::Asking(query)) => Phase::Resolving { query, port },
                    Err(_) => return Some(failed(socket, SocketError::Dns, "no such host")),
                };
                self.sockets.push(Socket {
                    number: socket,
                    phase,
                    until: now.0 + CONNECT_MS,
                    reads: VecDeque::new(),
                    sends: VecDeque::new(),
                    sent: 0,
                    gathered: Vec::new(),
                    secure: None,
                });
                None
            }
            SocketCall::Send { socket, data } => {
                if let Some(refusal) = self.securing(socket, net) {
                    return Some(refusal);
                }
                if let Some(s) = self.sockets.iter_mut().find(|s| s.number == socket) {
                    s.sends.push_back(data);
                }
                None
            }
            SocketCall::Read { socket, max } => {
                if let Some(refusal) = self.securing(socket, net) {
                    return Some(refusal);
                }
                if let Some(s) = self.sockets.iter_mut().find(|s| s.number == socket) {
                    s.reads.push_back(max.max(1));
                }
                None
            }
            SocketCall::Close { socket } => {
                if let Some(at) = self.sockets.iter().position(|s| s.number == socket) {
                    self.sockets.remove(at).release(net);
                }
                None
            }
            SocketCall::Broadcast {
                broadcast,
                ports,
                data,
                wait_ms,
            } => {
                self.broadcast(broadcast, &ports, &data, wait_ms, net, now);
                // Nothing could be sent: done at once.
                (!self.broadcasts.iter().any(|b| b.number == broadcast))
                    .then_some(SocketEvent::Done { broadcast })
            }
        }
    }

    /// Closes `socket` and tells why, if it is being made secure: nothing
    /// is sent or read on it until `Secured`, as the kernel keeps to
    /// (the contract: bytes put on it now would go out before the
    /// handshake, in the clear). `None` for any other.
    fn securing(&mut self, socket: u32, net: &mut Net) -> Option<SocketEvent> {
        let at = self.sockets.iter().position(|s| s.number == socket)?;
        matches!(
            self.sockets[at].stage(),
            Some(Stage::Queued | Stage::Securing)
        )
        .then(|| {
            self.sockets.remove(at).release(net);
            failed(
                socket,
                SocketError::NotReady,
                "nothing is sent or read before the connection is secure",
            )
        })
    }

    fn broadcast(
        &mut self,
        number: u32,
        ports: &[u16],
        data: &[u8],
        wait_ms: u32,
        net: &mut Net,
        now: Instant,
    ) {
        let Some(to) = net.broadcast_address() else {
            return;
        };
        // Room to send to every port at once, and for the answers.
        let packets = ports.len().max(ANSWERS);
        let bytes = (data.len() * ports.len()).max(ANSWER_BYTES);
        let Ok(udp) = net.open_udp(packets, bytes) else {
            return;
        };
        for &port in ports {
            let _ = net.udp(udp).send_slice(data, (to, port));
        }
        self.broadcasts.push(Broadcast {
            number,
            udp,
            until: now.0 + u64::from(wait_ms),
        });
    }

    /// The network went (or is another one now): every socket fails, every
    /// broadcast is done. What they hold is let go on `net` if they were
    /// opened on it (else it went with an earlier one).
    pub fn lost(
        &mut self,
        net: Option<&mut Net>,
        kernel: &mut Kernel,
        now: Instant,
    ) -> Vec<Effects> {
        self.lost_events(net)
            .into_iter()
            .map(|event| kernel.socket_event(now, event))
            .collect()
    }

    /// [`Sockets::lost`], as the events (for the interface-7 transport,
    /// which reports them itself).
    pub fn lost_events(&mut self, net: Option<&mut Net>) -> Vec<SocketEvent> {
        let mut net = net.filter(|net| net.generation() == self.generation);
        let mut events = Vec::new();
        for socket in core::mem::take(&mut self.sockets) {
            if let Some(net) = net.as_deref_mut() {
                socket.release(net);
            }
            events.push(failed(
                socket.number,
                SocketError::NoNetwork,
                "the network went away",
            ));
        }
        for broadcast in core::mem::take(&mut self.broadcasts) {
            if let Some(net) = net.as_deref_mut() {
                net.release_udp(broadcast.udp);
            }
            events.push(SocketEvent::Done {
                broadcast: broadcast.number,
            });
        }
        events
    }

    /// One step for every socket and broadcast.
    pub fn poll(
        &mut self,
        net: &mut Net,
        kernel: &mut Kernel,
        now: Instant,
        clock_ready: bool,
    ) -> Vec<Effects> {
        if !self.any() {
            return Vec::new();
        }
        if net.generation() != self.generation {
            return self.lost(None, kernel, now);
        }
        self.poll_events(net, now, clock_ready)
            .into_iter()
            .map(|event| kernel.socket_event(now, event))
            .collect()
    }

    /// [`Sockets::poll`], as the events (for the interface-7 transport,
    /// which reports them itself). A network that is another one now is
    /// [`Sockets::lost_events`]'s.
    pub fn poll_events(
        &mut self,
        net: &mut Net,
        now: Instant,
        clock_ready: bool,
    ) -> Vec<SocketEvent> {
        let mut events = Vec::new();
        self.sockets.retain_mut(|socket| {
            let keep = socket.step(net, now, clock_ready, &mut events);
            if !keep {
                socket.release(net);
            }
            keep
        });
        let with_ports = self.with_ports;
        self.broadcasts.retain(|broadcast| {
            let udp = net.udp(broadcast.udp);
            while let Ok((data, meta)) = udp.recv() {
                events.push(SocketEvent::Answer {
                    broadcast: broadcast.number,
                    from: if with_ports {
                        format!("{}", meta.endpoint)
                    } else {
                        format!("{}", meta.endpoint.addr)
                    },
                    data: data.to_vec(),
                });
            }
            if now.0 < broadcast.until {
                return true;
            }
            net.release_udp(broadcast.udp);
            events.push(SocketEvent::Done {
                broadcast: broadcast.number,
            });
            false
        });
        self.moved = !events.is_empty();
        events
    }

    #[cfg(feature = "ppn5")]
    /// Whether the network these sockets were opened on is another one now
    /// (they are all lost then).
    pub fn stale(&self, net: &Net) -> bool {
        self.any() && net.generation() != self.generation
    }

    #[cfg(feature = "ppn5")]
    /// Bytes socket `socket` has queued to send and not yet handed to the
    /// connection (0 for one that is unknown).
    pub fn queued(&self, socket: u32) -> usize {
        self.sockets
            .iter()
            .find(|s| s.number == socket)
            .map_or(0, |s| s.sends.iter().map(Vec::len).sum::<usize>() - s.sent)
    }

    #[cfg(feature = "ppn5")]
    /// Lets connecting `socket` take until `until` (milliseconds on the
    /// clock of `now`).
    pub fn connect_until(&mut self, socket: u32, until: u64) {
        if let Some(s) = self.sockets.iter_mut().find(|s| s.number == socket) {
            s.until = until;
        }
    }
}

fn failed(socket: u32, error: SocketError, reason: &str) -> SocketEvent {
    SocketEvent::Failed {
        socket,
        error,
        reason: String::from(reason),
    }
}

/// What a session's `fault` is to the plugin: `Untrusted` with the
/// certificate shown (`leaf`), or why the socket failed.
fn fault_event(
    socket: u32,
    fault: tls_trust::Fault,
    secure: &Secure,
    leaf: Option<Leaf>,
) -> SocketEvent {
    let pinned = matches!(secure.setup.trust, Trust::Pinned(_));
    match (tls_trust::outcome(fault, pinned, leaf.is_some()), leaf) {
        (Outcome::Untrusted(why), Some(leaf)) => SocketEvent::Untrusted {
            socket,
            certificate: leaf.certificate,
            key: leaf.key,
            why,
        },
        (Outcome::Untrusted(_), None) => {
            failed(socket, SocketError::Tls, "secure connection failed")
        }
        (Outcome::Failed(error, reason), _) => failed(socket, error, reason),
    }
}

/// What a failed connection is to the plugin, once it was secure.
fn link_event(socket: u32, failure: Failure, secure: &Secure) -> SocketEvent {
    match failure {
        Failure::Lost => failed(socket, SocketError::Lost, "connection lost"),
        Failure::Tls(e) => fault_event(socket, e.fault(), secure, None),
    }
}

/// The heap's peak since start, in test builds (`remote`), for the log.
fn peak_note() -> String {
    #[cfg(feature = "remote")]
    return format!(", peak {}", crate::peak::peak());
    #[cfg(not(feature = "remote"))]
    String::new()
}

/// Starts connecting to `address`.
fn connect(net: &mut Net, address: IpAddress, port: u16) -> Result<Phase, &'static str> {
    match net.open((address, port), BUFFERS) {
        Ok(handle) => {
            // A connection the other side no longer answers ends, instead
            // of waiting for ever (e.g. while a book comes, with its end
            // lost on the way): asked every while it is quiet, it is given
            // up once nothing comes back.
            let socket = net.socket(handle);
            socket.set_keep_alive(Some(smoltcp::time::Duration::from_millis(KEEP_ALIVE_MS)));
            socket.set_timeout(Some(smoltcp::time::Duration::from_millis(GIVE_UP_MS)));
            Ok(Phase::Connecting(handle))
        }
        Err(net::Error::Memory) => Err("not enough memory for a connection"),
        Err(_) => Err("could not connect"),
    }
}

impl Socket {
    /// Lets its lookup or connection go.
    fn release(&self, net: &mut Net) {
        match &self.phase {
            Phase::Resolving { query, .. } => net.forget(*query),
            Phase::Connecting(handle) | Phase::Open(handle) | Phase::Switching { handle, .. } => {
                net.release(*handle)
            }
            Phase::Handshake(link) | Phase::Secure(link) => net.release(link.socket),
            Phase::Queued { .. } | Phase::Ended => {}
        }
    }

    /// Where a secure socket is, for its time; `None` for one that is
    /// not (or not yet) asked to be.
    fn stage(&self) -> Option<Stage> {
        self.secure.as_ref()?;
        match self.phase {
            Phase::Queued { .. } | Phase::Switching { .. } => Some(Stage::Queued),
            Phase::Resolving { .. } | Phase::Connecting(_) | Phase::Handshake(_) => {
                Some(Stage::Securing)
            }
            Phase::Secure(_) => Some(Stage::Secure),
            Phase::Open(_) | Phase::Ended => None,
        }
    }

    /// One step; whether it stays open (else it failed, with the event).
    fn step(
        &mut self,
        net: &mut Net,
        now: Instant,
        clock_ready: bool,
        events: &mut Vec<SocketEvent>,
    ) -> bool {
        let number = self.number;
        if let (Some(secure), Some(stage)) = (&self.secure, self.stage())
            && let Some((error, reason)) = secure.watch.expired(now.0, stage)
        {
            // In line only for the clock (the certificates' dates need it).
            let (error, reason) = if stage == Stage::Queued && !clock_ready {
                (SocketError::Timeout, "the time was not set")
            } else {
                (error, reason)
            };
            events.push(failed(number, error, reason));
            return false;
        }
        // Every slot is held and someone waits: a socket that sits idle
        // (a mail plugin gone quiet) makes room, instead of HTTPS failing
        // `Busy` behind it.
        if let (Some(secure), Phase::Secure(_)) = (&self.secure, &self.phase)
            && crate::tls_slots::pressed()
            && let Some((error, reason)) = secure.watch.yields(now.0, YIELD_AFTER_MS)
        {
            log::info!("socket {number}: closed, {reason}");
            events.push(failed(number, error, reason));
            return false;
        }
        match self.phase {
            Phase::Queued { .. } => self.queued(net, now, clock_ready, events),
            Phase::Resolving { query, port } => match net.resolved(query) {
                Some(Ok(address)) => match connect(net, address, port) {
                    Ok(phase) => {
                        self.phase = phase;
                        true
                    }
                    Err(reason) => {
                        self.phase = Phase::Ended;
                        events.push(failed(number, SocketError::Refused, reason));
                        false
                    }
                },
                Some(Err(_)) => {
                    self.phase = Phase::Ended;
                    events.push(failed(number, SocketError::Dns, "no such host"));
                    false
                }
                None if now.0 > self.until => {
                    events.push(failed(number, SocketError::Timeout, "no answer"));
                    false
                }
                None => true,
            },
            Phase::Connecting(handle) => {
                let socket = net.socket(handle);
                match socket.state() {
                    tcp::State::Established => {
                        if self.secure.is_some() {
                            // Secure: no `Connected`, the handshake first.
                            let slot = self.secure.as_mut().and_then(|s| s.slot.take());
                            return self.start_session(handle, slot, events);
                        }
                        self.phase = Phase::Open(handle);
                        events.push(SocketEvent::Connected { socket: number });
                        self.exchange(net, events)
                    }
                    tcp::State::SynSent | tcp::State::SynReceived if now.0 <= self.until => true,
                    _ => {
                        events.push(failed(number, SocketError::Refused, "could not connect"));
                        false
                    }
                }
            }
            Phase::Open(_) => self.exchange(net, events),
            Phase::Switching { .. } => self.switching(net, clock_ready, events),
            Phase::Handshake(_) => self.handshake(net, now, events),
            Phase::Secure(_) => self.exchange_secure(net, now, events),
            Phase::Ended => false,
        }
    }

    /// In line for a TLS slot: once one is given (and the time is known,
    /// for the certificate's dates) the host is looked up as for any
    /// connection.
    fn queued(
        &mut self,
        net: &mut Net,
        now: Instant,
        clock_ready: bool,
        events: &mut Vec<SocketEvent>,
    ) -> bool {
        let number = self.number;
        let Phase::Queued { waiter, host, port } = &mut self.phase else {
            return true;
        };
        if !clock_ready {
            return true;
        }
        let Some(slot) = waiter.take() else {
            return true;
        };
        let (host, port) = (core::mem::take(host), *port);
        if let Some(secure) = &mut self.secure {
            secure.slot = Some(slot);
        }
        self.until = now.0 + CONNECT_MS;
        match net.resolve(&host) {
            Ok(Resolve::Done(address)) => match connect(net, address, port) {
                Ok(phase) => self.phase = phase,
                Err(reason) => {
                    self.phase = Phase::Ended;
                    events.push(failed(number, SocketError::Refused, reason));
                    return false;
                }
            },
            Ok(Resolve::Asking(query)) => self.phase = Phase::Resolving { query, port },
            Err(_) => {
                self.phase = Phase::Ended;
                events.push(failed(number, SocketError::Dns, "no such host"));
                return false;
            }
        }
        true
    }

    /// An open connection switching to TLS: what it sent goes out first;
    /// then, in its turn and once nothing the server sent waits to be read
    /// (it would pass for secure), the session starts.
    fn switching(
        &mut self,
        net: &mut Net,
        clock_ready: bool,
        events: &mut Vec<SocketEvent>,
    ) -> bool {
        let Phase::Switching { handle, .. } = self.phase else {
            return true;
        };
        if !self.push_sends(net, handle, events) {
            return false;
        }
        let socket = net.socket(handle);
        if !self.sends.is_empty() || socket.send_queue() > 0 || !clock_ready {
            return true;
        }
        // What came since the server's go-ahead was read (it should wait
        // for the handshake) might be injected plaintext, which would pass
        // for secure: the socket is closed, not started over it.
        if let Some((error, reason)) =
            tls_trust::starttls_refusal(socket.recv_queue() + self.gathered.len())
        {
            events.push(failed(self.number, error, reason));
            return false;
        }
        let Phase::Switching { waiter, .. } = &mut self.phase else {
            return true;
        };
        let Some(slot) = waiter.take() else {
            return true;
        };
        self.start_session(handle, Some(slot), events)
    }

    /// Makes the session over the connection `handle`, in `slot`.
    fn start_session(
        &mut self,
        handle: Handle,
        slot: Option<Slot>,
        events: &mut Vec<SocketEvent>,
    ) -> bool {
        let number = self.number;
        let Some(secure) = &mut self.secure else {
            return false;
        };
        secure.used_before = esp_alloc::HEAP.used();
        let session = match slot {
            Some(slot) => tls::Session::new(&secure.setup.server_name, &secure.setup.trust, slot),
            None => Err(tls::Error::Memory),
        };
        match session {
            Ok(session) => {
                let link = Link::new(handle, Some(alloc::boxed::Box::new(session)));
                // The connection is the link's from here: `release` finds it
                // there.
                self.phase = Phase::Handshake(link);
                true
            }
            Err(e) => {
                events.push(fault_event(number, e.fault(), secure, None));
                // The connection is let go by `release`, whichever phase
                // it is in.
                self.phase = Phase::Open(handle);
                false
            }
        }
    }

    /// The handshake, a step at a time; once done, `Secured`.
    fn handshake(&mut self, net: &mut Net, now: Instant, events: &mut Vec<SocketEvent>) -> bool {
        let number = self.number;
        let (Phase::Handshake(link), Some(secure)) = (&mut self.phase, &mut self.secure) else {
            return true;
        };
        if let Err(failure) = link.pump(net) {
            let leaf = link.session().and_then(tls::Session::leaf);
            events.push(match failure {
                Failure::Lost => failed(number, SocketError::Lost, "connection lost"),
                Failure::Tls(e) => fault_event(number, e.fault(), secure, leaf),
            });
            return false;
        }
        if !link.ready(net) {
            return true;
        }
        let Some(leaf) = link.session().and_then(tls::Session::leaf) else {
            events.push(failed(
                number,
                SocketError::Tls,
                "the server showed no certificate",
            ));
            return false;
        };
        // What a session costs, for the log: the heap used now over before
        // the session was made, and the most it held since start.
        let used = esp_alloc::HEAP.used();
        log::info!(
            "socket {number}: secure, session {} B (heap used {used}){}",
            used.saturating_sub(secure.used_before),
            peak_note()
        );
        events.push(SocketEvent::Secured {
            socket: number,
            certificate: leaf.certificate,
            key: leaf.key,
            time_checked: leaf.time_checked,
        });
        secure.watch.secured(now.0);
        let Phase::Handshake(link) = core::mem::replace(&mut self.phase, Phase::Ended) else {
            return false;
        };
        self.phase = Phase::Secure(link);
        self.exchange_secure(net, now, events)
    }

    /// Sends and answers the reads asked for over the secure connection, as
    /// far as it allows; whether it stays open.
    fn exchange_secure(
        &mut self,
        net: &mut Net,
        now: Instant,
        events: &mut Vec<SocketEvent>,
    ) -> bool {
        let number = self.number;
        let (Phase::Secure(link), Some(secure)) = (&mut self.phase, &mut self.secure) else {
            return true;
        };
        let mut moved = match link.pump(net) {
            Ok(moved) => moved,
            Err(failure) => {
                events.push(link_event(number, failure, secure));
                return false;
            }
        };
        while let Some(data) = self.sends.front() {
            match link.send(net, &data[self.sent..]) {
                Ok(0) => break,
                Ok(n) => {
                    self.sent += n;
                    moved = true;
                }
                Err(failure) => {
                    events.push(link_event(number, failure, secure));
                    return false;
                }
            }
            if self.sent < data.len() {
                break;
            }
            self.sends.pop_front();
            self.sent = 0;
        }
        while let Some(&max) = self.reads.front() {
            // As for a plain connection: gathered into parts of up to
            // [`PART`] bytes; what came is passed on once the part is full
            // or nothing more comes.
            let goal = (max as usize).min(PART);
            let mut closed = false;
            while self.gathered.len() < goal {
                let missing = goal - self.gathered.len();
                if self.gathered.capacity() < goal
                    && self.gathered.try_reserve_exact(missing).is_err()
                {
                    break;
                }
                let mut part = [0u8; READ_PART];
                match link.read(&mut part[..missing.min(READ_PART)]) {
                    Ok(Read::Data(n)) => {
                        self.gathered.extend_from_slice(&part[..n]);
                        moved = true;
                    }
                    Ok(Read::Wait) => break,
                    Ok(Read::Closed) => {
                        closed = true;
                        break;
                    }
                    Err(failure) => {
                        events.push(link_event(number, failure, secure));
                        return false;
                    }
                }
            }
            if self.gathered.len() >= goal || !self.gathered.is_empty() || closed {
                self.reads.pop_front();
                events.push(SocketEvent::Data {
                    socket: number,
                    data: core::mem::take(&mut self.gathered),
                });
            } else {
                break;
            }
        }
        if moved {
            secure.watch.touch(now.0);
        }
        true
    }

    /// Sends what waits over the plain connection `handle`, as far as it
    /// allows; whether it stays open.
    fn push_sends(&mut self, net: &mut Net, handle: Handle, events: &mut Vec<SocketEvent>) -> bool {
        let number = self.number;
        let socket = net.socket(handle);
        while let Some(data) = self.sends.front() {
            if !socket.may_send() {
                log::info!(
                    "socket {number}: lost (no more sending), {:?}",
                    socket.state()
                );
                events.push(failed(number, SocketError::Lost, "connection lost"));
                return false;
            }
            match socket.send_slice(&data[self.sent..]) {
                Ok(n) => self.sent += n,
                Err(_) => {
                    log::info!(
                        "socket {number}: lost (sending failed), {:?}",
                        socket.state()
                    );
                    events.push(failed(number, SocketError::Lost, "connection lost"));
                    return false;
                }
            }
            if self.sent < data.len() {
                break;
            }
            self.sends.pop_front();
            self.sent = 0;
        }
        true
    }

    /// Sends what waits and answers the reads asked for, as far as the
    /// connection allows; whether it stays open.
    fn exchange(&mut self, net: &mut Net, events: &mut Vec<SocketEvent>) -> bool {
        let Phase::Open(handle) = self.phase else {
            return true;
        };
        let number = self.number;
        if !self.push_sends(net, handle, events) {
            return false;
        }
        let socket = net.socket(handle);
        while let Some(&max) = self.reads.front() {
            // Large reads (a file) gathered into parts of up to [`PART`]
            // bytes, for fewer, larger writes to the card; what came is
            // passed on once the part is full or nothing more comes.
            let goal = (max as usize).min(PART);
            let mut came = false;
            while self.gathered.len() < goal && socket.can_recv() {
                if self.gathered.capacity() < goal
                    && self
                        .gathered
                        .try_reserve_exact(goal - self.gathered.len())
                        .is_err()
                {
                    break;
                }
                let gathered = &mut self.gathered;
                let got = socket.recv(|bytes| {
                    let n = bytes.len().min(goal - gathered.len());
                    gathered.extend_from_slice(&bytes[..n]);
                    (n, n)
                });
                match got {
                    Ok(n) if n > 0 => came = true,
                    _ => break,
                }
            }
            if self.gathered.len() >= goal || (!came && !self.gathered.is_empty()) {
                self.reads.pop_front();
                events.push(SocketEvent::Data {
                    socket: number,
                    data: core::mem::take(&mut self.gathered),
                });
            } else if !self.gathered.is_empty() || socket.can_recv() {
                break;
            } else if socket.state() == tcp::State::Closed {
                // Reset (or gone after both sides closed).
                log::info!("socket {number}: lost (closed), {:?}", socket.state());
                events.push(failed(number, SocketError::Lost, "connection lost"));
                return false;
            } else if !socket.may_recv() {
                // The other side closed: nothing more comes.
                self.reads.pop_front();
                events.push(SocketEvent::Data {
                    socket: number,
                    data: Vec::new(),
                });
            } else {
                break;
            }
        }
        true
    }
}
