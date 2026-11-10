//! `core.sockets` for the linked native plugins on the computer: plain TCP
//! and UDP broadcast over the computer's own network, reported back to core
//! from the device's loop ([`NativeSock::pump`], called by
//! `Device::poll_http`), never from inside a call.
//!
//! What core does is core's (limits, the pool, the one TLS lease, reads to a
//! file, events); this moves bytes. Each connection has threads of its own
//! (connecting and reading, writing), so nothing here polls: they post what
//! happened to a channel that `pump` drains. A read is allowed only as far as
//! core asked for it (`read`), so nothing arrives unasked.
//!
//! **TLS** is `paperpaw_serve::secure` (rustls with ring; the authorities
//! PaperPaw ships, Mozilla's list as on the X4, or the pins a plugin gave; no
//! mode trusts what it is shown). Core starts a handshake ([`SockTls::begin`])
//! on a connected socket that holds the device's session; it runs on a thread
//! of its own and passes or fails through `secured`, `untrusted` or
//! `tls_failed`. Nothing of the plugin goes out before it passed, and
//! `begin` refuses a socket with bytes of the peer waiting, a read or unsent
//! bytes (a STARTTLS injection would pass as secure): `ERR_STATE`, and core
//! ends the socket. Pins of both kinds at once are refused (`ERR_INVALID`).
//!
//! [`SockTls::begin`]: paperpaw_core::native_host::shell_wiring::SockTls::begin

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{ErrorKind, Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs as _, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use paperpaw_core::native_host::shell_wiring::{
    Connect, PeerInfo, SockFailure, SockReports, SockTls, SockTransport, TlsFailure, TlsOpen,
    UntrustedWhy,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_serve::secure::{Failure, Pins, Session};

/// The bytes a connection's writer holds at most (the queue core keeps is
/// smaller: this only bounds what the transport takes at once).
const WRITE_HELD: usize = 16 << 10;

/// How long a handshake may take at most (core ends the socket sooner when
/// the plugin set a shorter time).
const HANDSHAKE_MAX: Duration = Duration::from_secs(60);

/// The most one read from the connection takes at a time.
const READ_PIECE: usize = 16 << 10;

/// How long a thread waits before it looks whether it was closed.
const LOOK: Duration = Duration::from_millis(100);

/// What core is told (`SockReports`, or a stand-in in tests).
trait Sink {
    fn connected(&self, id: u32) -> Result<(), i32>;
    fn failed(&self, id: u32, why: SockFailure) -> Result<(), i32>;
    fn received(&self, id: u32, bytes: &[u8]) -> Result<usize, i32>;
    fn peer_closed(&self, id: u32) -> Result<(), i32>;
    fn writable(&self, id: u32) -> Result<(), i32>;
    fn answered(&self, id: u32, from: &[u8], data: &[u8]) -> Result<(), i32>;
    fn secured(&self, id: u32, peer: &PeerInfo) -> Result<(), i32>;
    fn untrusted(&self, id: u32, why: UntrustedWhy, peer: &PeerInfo) -> Result<(), i32>;
    fn tls_failed(&self, id: u32, how: TlsFailure) -> Result<(), i32>;
    fn network_lost(&self) -> Result<(), i32>;
    fn tick(&self) -> Result<(), i32>;
}

impl Sink for SockReports {
    fn connected(&self, id: u32) -> Result<(), i32> {
        SockReports::connected(self, id)
    }
    fn failed(&self, id: u32, why: SockFailure) -> Result<(), i32> {
        SockReports::failed(self, id, why)
    }
    fn received(&self, id: u32, bytes: &[u8]) -> Result<usize, i32> {
        SockReports::received(self, id, bytes)
    }
    fn peer_closed(&self, id: u32) -> Result<(), i32> {
        SockReports::peer_closed(self, id)
    }
    fn writable(&self, id: u32) -> Result<(), i32> {
        SockReports::writable(self, id)
    }
    fn answered(&self, id: u32, from: &[u8], data: &[u8]) -> Result<(), i32> {
        SockReports::answered(self, id, from, data)
    }
    fn secured(&self, id: u32, peer: &PeerInfo) -> Result<(), i32> {
        SockReports::secured(self, id, peer)
    }
    fn untrusted(&self, id: u32, why: UntrustedWhy, peer: &PeerInfo) -> Result<(), i32> {
        SockReports::untrusted(self, id, why, peer)
    }
    fn tls_failed(&self, id: u32, how: TlsFailure) -> Result<(), i32> {
        SockReports::tls_failed(self, id, how)
    }
    fn network_lost(&self) -> Result<(), i32> {
        SockReports::network_lost(self)
    }
    fn tick(&self) -> Result<(), i32> {
        SockReports::tick(self)
    }
}

/// What a thread of a connection or broadcast reports.
#[derive(Debug)]
enum Event {
    Connected(u32),
    Failed(u32, SockFailure),
    Data(u32, Vec<u8>),
    PeerClosed(u32),
    Writable(u32),
    Answer(u32, String, Vec<u8>),
    Secured(u32, PeerInfo),
    Untrusted(u32, UntrustedWhy, PeerInfo),
    TlsFailed(u32, TlsFailure),
}

impl Event {
    fn id(&self) -> u32 {
        match self {
            Event::Connected(id)
            | Event::Failed(id, _)
            | Event::Data(id, _)
            | Event::PeerClosed(id)
            | Event::Writable(id)
            | Event::Answer(id, ..)
            | Event::Secured(id, _)
            | Event::Untrusted(id, ..)
            | Event::TlsFailed(id, _) => *id,
        }
    }
}

/// What a connection's threads and the transport share.
struct Link {
    /// How many more bytes the reader may take (what core asked for).
    allow: Mutex<usize>,
    wake: Condvar,
    closed: AtomicBool,
    /// Bytes handed to the writer and not written yet.
    held: AtomicUsize,
    /// The plugin was told the writer was full: tell it when there is room.
    wants_room: AtomicBool,
    /// The connection, once made (to shut it down on close).
    stream: Mutex<Option<TcpStream>>,
    /// The secure session, once the handshake passed: reading and writing go
    /// through it from then on.
    secure: Mutex<Option<Session>>,
    /// A handshake was begun on this connection: no second one, and with no
    /// session (running, or ended) nothing is read or written in clear.
    secured: AtomicBool,
    /// The reader is inside a read of the connection.
    reading: AtomicBool,
    /// Bytes the reader took off the connection and core has not taken yet
    /// (in the event channel, or refused and kept in `unread`): clear bytes
    /// that must not pass a switch to TLS.
    unoffered: AtomicUsize,
}

impl Link {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            allow: Mutex::new(0),
            wake: Condvar::new(),
            closed: AtomicBool::new(false),
            held: AtomicUsize::new(0),
            wants_room: AtomicBool::new(false),
            stream: Mutex::new(None),
            secure: Mutex::new(None),
            secured: AtomicBool::new(false),
            reading: AtomicBool::new(false),
            unoffered: AtomicUsize::new(0),
        })
    }

    /// The secure session, if the handshake passed.
    fn session(&self) -> Option<Session> {
        self.secure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A handshake was begun and no session came out of it (yet, or any
    /// more): nothing of the connection is clear to use.
    fn in_handshake(&self) -> bool {
        self.secured.load(Ordering::SeqCst) && self.session().is_none()
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Ends the connection's threads and the connection itself.
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.wake.notify_all();
        // The session says it is done (`close_notify`) before the connection goes.
        if let Some(session) = self.session() {
            session.close_write();
        }
        let stream = self.stream.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(stream) = stream.as_ref() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

/// A socket or a broadcast, as the transport keeps it.
struct Entry {
    link: Arc<Link>,
    /// Bytes for the writer (a connection).
    writer: Option<Sender<Vec<u8>>>,
}

/// The shell side of `core.sockets` (see the module).
pub struct NativeSock {
    entries: RefCell<HashMap<u32, Entry>>,
    events: Sender<Event>,
    arrived: Receiver<Event>,
    /// Reports core answered `ERR_BUSY`: again at the next turn.
    again: RefCell<Vec<Event>>,
    /// Bytes core did not take yet, by socket.
    unread: RefCell<HashMap<u32, Vec<u8>>>,
    /// A network was up at the last pump.
    was_up: Cell<bool>,
}

impl std::fmt::Debug for NativeSock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeSock")
            .field("open", &self.entries.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Default for NativeSock {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeSock {
    #[must_use]
    pub fn new() -> Self {
        let (events, arrived) = channel();
        Self {
            entries: RefCell::new(HashMap::new()),
            events,
            arrived,
            again: RefCell::new(Vec::new()),
            unread: RefCell::new(HashMap::new()),
            was_up: Cell::new(false),
        }
    }

    /// Sockets and broadcasts open (not closed or failed).
    #[must_use]
    pub fn open(&self) -> usize {
        self.entries.borrow().len()
    }

    /// Hands core what the threads reported, then lets it take its turn
    /// (`tick`); a network gone since the last pump (`up`: whether one is up
    /// now) ends every socket (`network_lost`).
    pub fn pump(&self, reports: &SockReports, up: bool) {
        self.pump_with(reports, up);
    }

    fn pump_with(&self, reports: &dyn Sink, up: bool) {
        if self.was_up.replace(up) && !up {
            for (_, entry) in self.entries.borrow_mut().drain() {
                entry.link.close();
            }
            self.unread.borrow_mut().clear();
            self.again.borrow_mut().clear();
            while self.arrived.try_recv().is_ok() {}
            let _ = reports.network_lost();
            let _ = reports.tick();
            return;
        }
        let mut todo = std::mem::take(&mut *self.again.borrow_mut());
        todo.extend(self.arrived.try_iter());
        for event in todo {
            self.report(event, reports);
        }
        // Bytes core did not take yet: offered again (a read may be asked now).
        let ids: Vec<u32> = self.unread.borrow().keys().copied().collect();
        for id in ids {
            self.offer(id, &[], reports);
        }
        let _ = reports.tick();
    }

    /// Tells core of one thing that happened.
    fn report(&self, event: Event, reports: &dyn Sink) {
        // What came from a socket that was closed since has no one to tell.
        if !self.entries.borrow().contains_key(&event.id()) {
            return;
        }
        let done = match &event {
            Event::Data(..) => {
                if let Event::Data(id, bytes) = event {
                    self.offer(id, &bytes, reports);
                }
                return;
            }
            Event::Connected(id) => reports.connected(*id),
            // The socket is forgotten once core has taken the report (or
            // will never take it): a busy core hears it again.
            Event::Failed(id, why) => {
                let done = reports.failed(*id, *why);
                if done != Err(ERR_BUSY) {
                    self.forget(*id);
                }
                done
            }
            Event::PeerClosed(id) => reports.peer_closed(*id),
            Event::Writable(id) => reports.writable(*id),
            Event::Answer(id, from, data) => reports.answered(*id, from.as_bytes(), data),
            // Core ends the socket after these two, and closes it.
            Event::Secured(id, peer) => reports.secured(*id, peer),
            Event::Untrusted(id, why, peer) => reports.untrusted(*id, *why, peer),
            Event::TlsFailed(id, how) => reports.tls_failed(*id, *how),
        };
        if done == Err(ERR_BUSY) {
            self.again.borrow_mut().push(event);
        }
    }

    /// Offers core `bytes` (after what it did not take before): what it takes
    /// is gone, the rest waits for the next read.
    fn offer(&self, id: u32, bytes: &[u8], reports: &dyn Sink) {
        let mut held = self.unread.borrow_mut().remove(&id).unwrap_or_default();
        held.extend_from_slice(bytes);
        if held.is_empty() {
            return;
        }
        // Core may call back (`close`) from `received`: no borrow is held.
        // `ERR_BUSY` and `ERR_STATE` (no read asked for yet): it stays.
        if let Ok(taken) = reports.received(id, &held) {
            let taken = taken.min(held.len());
            held.drain(..taken);
            if let Some(entry) = self.entries.borrow().get(&id) {
                let counter = &entry.link.unoffered;
                let mut left = counter.load(Ordering::SeqCst);
                while let Err(now) = counter.compare_exchange(
                    left,
                    left.saturating_sub(taken),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    left = now;
                }
            }
        }
        if !held.is_empty() && self.entries.borrow().contains_key(&id) {
            self.unread.borrow_mut().insert(id, held);
        }
    }

    /// Starts connecting socket `id` on a thread of its own.
    fn connect_to(&self, id: u32, host: &str, port: u16, timeout: Duration) {
        let link = Link::new();
        let (writer, queue) = channel();
        let host = host.to_owned();
        let (thread_link, events) = (link.clone(), self.events.clone());
        self.entries.borrow_mut().insert(
            id,
            Entry {
                link,
                writer: Some(writer),
            },
        );
        thread::spawn(move || {
            run_connection(id, (host, port, timeout), &thread_link, &events, queue);
        });
    }

    /// Starts the handshake of socket `id` for `server_name`, trusting `pins`
    /// (`(kind, hash)`: 0 the leaf certificate's SHA-256, 1 its key's; none:
    /// the authorities PaperPaw ships). The result comes from the pump.
    fn start_handshake(
        &self,
        id: u32,
        server_name: &str,
        pins: &[(u32, [u8; 32])],
    ) -> Result<(), i32> {
        let entries = self.entries.borrow();
        let Some(Entry {
            link,
            writer: Some(_),
        }) = entries.get(&id)
        else {
            return Err(ERR_NOT_FOUND);
        };
        let certificates: Vec<[u8; 32]> = pins.iter().filter(|p| p.0 == 0).map(|p| p.1).collect();
        let keys: Vec<[u8; 32]> = pins.iter().filter(|p| p.0 == 1).map(|p| p.1).collect();
        // Both kinds at once are not backed.
        if !certificates.is_empty() && !keys.is_empty() {
            return Err(ERR_INVALID);
        }
        // What is seen at once: bytes not sent yet, or bytes read and not taken
        // by core (in the event channel, or refused and kept): they would pass
        // as secure. Nothing is changed by this refusal.
        if link.held.load(Ordering::SeqCst) > 0 || link.unoffered.load(Ordering::SeqCst) > 0 {
            return Err(ERR_STATE);
        }
        // The switch is begun: from here nothing is read or written in clear
        // (the reader parks at its next look).
        if link
            .secured
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(ERR_STATE);
        }
        link.wake.notify_all();
        let (link, events) = (link.clone(), self.events.clone());
        let name = server_name.to_owned();
        drop(entries);
        // The rest runs on the handshake's thread, not core's: it waits for
        // the reader to park, looks once more for clear bytes (the reader may
        // have taken some just before it parked), and fails closed (`tls`,
        // nothing sent) where it finds any or the reader does not park in time.
        thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(1);
            while link.reading.load(Ordering::SeqCst) && Instant::now() < end {
                thread::sleep(Duration::from_millis(5));
            }
            let tcp = link
                .stream
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .and_then(|stream| stream.try_clone().ok());
            let clean = tcp.filter(|tcp| {
                if link.reading.load(Ordering::SeqCst) {
                    return false;
                }
                let _ = tcp.set_nonblocking(true);
                let waiting = tcp.peek(&mut [0u8; 1]);
                let _ = tcp.set_nonblocking(false);
                matches!(&waiting, Err(error) if error.kind() == ErrorKind::WouldBlock)
                    && link.unoffered.load(Ordering::SeqCst) == 0
                    && link.held.load(Ordering::SeqCst) == 0
                    && !link.is_closed()
            });
            let Some(mut tcp) = clean else {
                if !link.is_closed() {
                    let _ = events.send(Event::TlsFailed(id, TlsFailure::Tls));
                }
                return;
            };
            // The read core granted is over: the allowance ends here.
            *link.allow.lock().unwrap_or_else(PoisonError::into_inner) = 0;
            let pins = if !certificates.is_empty() {
                Pins::Certificates(&certificates)
            } else if !keys.is_empty() {
                Pins::Keys(&keys)
            } else {
                Pins::Authorities
            };
            let outcome = paperpaw_serve::secure::handshake(&mut tcp, &name, pins, HANDSHAKE_MAX);
            if link.is_closed() {
                return;
            }
            let event = match outcome {
                Ok((session, peer)) => {
                    *link.secure.lock().unwrap_or_else(PoisonError::into_inner) = Some(session);
                    link.wake.notify_all();
                    Event::Secured(
                        id,
                        PeerInfo::new(peer.certificate, peer.key, peer.time_checked),
                    )
                }
                Err(Failure::Untrusted { peer, not_pinned }) => Event::Untrusted(
                    id,
                    if not_pinned {
                        UntrustedWhy::NotPinned
                    } else {
                        UntrustedWhy::UnknownIssuer
                    },
                    PeerInfo::new(peer.certificate, peer.key, peer.time_checked),
                ),
                Err(Failure::Name) => Event::TlsFailed(id, TlsFailure::Name),
                Err(Failure::Expired) => Event::TlsFailed(id, TlsFailure::Expired),
                Err(_) => Event::TlsFailed(id, TlsFailure::Tls),
            };
            let _ = events.send(event);
        });
        Ok(())
    }

    /// A socket that ended: its entry and what it still held go.
    fn forget(&self, id: u32) {
        if let Some(entry) = self.entries.borrow_mut().remove(&id) {
            entry.link.close();
        }
        self.unread.borrow_mut().remove(&id);
    }
}

/// How an I/O error is told to core.
fn failure(error: &std::io::Error) -> SockFailure {
    match error.kind() {
        ErrorKind::ConnectionRefused => SockFailure::Refused,
        ErrorKind::TimedOut | ErrorKind::WouldBlock => SockFailure::Timeout,
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe => {
            SockFailure::Lost
        }
        _ => SockFailure::Io,
    }
}

/// Looks the name up and connects, all within `timeout`; the first address
/// that answers.
fn connect_to(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, SockFailure> {
    let end = Instant::now() + timeout;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addresses: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| SockFailure::Dns)?
        .collect();
    let mut last = SockFailure::Dns;
    for address in addresses {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(SockFailure::Timeout);
        }
        match TcpStream::connect_timeout(&address, left) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = failure(&error),
        }
    }
    Err(last)
}

/// A connection's thread: connects, then reads as far as core allowed.
fn run_connection(
    id: u32,
    to: (String, u16, Duration),
    link: &Arc<Link>,
    events: &Sender<Event>,
    queue: Receiver<Vec<u8>>,
) {
    let stream = match connect_to(&to.0, to.1, to.2) {
        Ok(stream) => stream,
        Err(why) => {
            let _ = events.send(Event::Failed(id, why));
            return;
        }
    };
    let _ = stream.set_nodelay(true);
    let (Ok(mut reader), Ok(writer)) = (stream.try_clone(), stream.try_clone()) else {
        let _ = events.send(Event::Failed(id, SockFailure::Io));
        return;
    };
    *link.stream.lock().unwrap_or_else(PoisonError::into_inner) = Some(stream);
    if link.is_closed() {
        link.close();
        return;
    }
    let _ = events.send(Event::Connected(id));
    let (link_w, events_w) = (link.clone(), events.clone());
    thread::spawn(move || write_connection(id, writer, &link_w, &events_w, &queue));
    let mut piece = vec![0; READ_PIECE];
    loop {
        let allowed = {
            let mut allow = link.allow.lock().unwrap_or_else(PoisonError::into_inner);
            // Nothing is read when no read was asked for, nor while a
            // handshake runs (its bytes are the handshake's).
            while (*allow == 0 || link.in_handshake()) && !link.is_closed() {
                allow = link
                    .wake
                    .wait(allow)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            *allow
        };
        if link.is_closed() {
            return;
        }
        // Said before the check: a handshake begun now sees this read, or
        // this read sees the handshake.
        link.reading.store(true, Ordering::SeqCst);
        if link.in_handshake() {
            link.reading.store(false, Ordering::SeqCst);
            continue;
        }
        let n = allowed.min(piece.len());
        let session = link.session();
        let read = if let Some(session) = &session {
            session.read(&mut reader, &mut piece[..n])
        } else {
            // A look now and then: a handshake that wants the connection
            // waits no longer than that for this read to end.
            let _ = reader.set_read_timeout(Some(LOOK));
            reader.read(&mut piece[..n])
        };
        if let Ok(k @ 1..) = read {
            let mut allow = link.allow.lock().unwrap_or_else(PoisonError::into_inner);
            *allow = allow.saturating_sub(k);
            drop(allow);
            link.unoffered.fetch_add(k, Ordering::SeqCst);
            let _ = events.send(Event::Data(id, piece[..k].to_vec()));
        }
        link.reading.store(false, Ordering::SeqCst);
        match read {
            Ok(0) => {
                if !link.is_closed() {
                    let _ = events.send(Event::PeerClosed(id));
                }
                return;
            }
            Ok(_) => {}
            Err(error)
                if error.kind() == ErrorKind::Interrupted
                    || (session.is_none()
                        && matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)) => {
            }
            Err(error) => {
                if !link.is_closed() {
                    let _ = events.send(Event::Failed(id, failure(&error)));
                }
                return;
            }
        }
    }
}

/// A connection's writer thread: writes what it is handed, in order, and says
/// when it has room again.
fn write_connection(
    id: u32,
    mut stream: TcpStream,
    link: &Link,
    events: &Sender<Event>,
    queue: &Receiver<Vec<u8>>,
) {
    for chunk in queue {
        if link.is_closed() {
            return;
        }
        let written = match link.session() {
            Some(session) => session.write_all(&chunk),
            // A connection that began TLS and has no session (the handshake
            // runs, or the session ended and the close is coming): nothing
            // goes out in clear.
            None if link.secured.load(Ordering::SeqCst) => return,
            None => stream.write_all(&chunk),
        };
        if let Err(error) = written {
            if !link.is_closed() {
                let _ = events.send(Event::Failed(id, failure(&error)));
            }
            return;
        }
        link.held.fetch_sub(chunk.len(), Ordering::AcqRel);
        if link.wants_room.swap(false, Ordering::AcqRel) {
            let _ = events.send(Event::Writable(id));
        }
    }
}

/// The most answers, and bytes of them, one broadcast passes on: a host that
/// floods the reply port gets no more memory than this between two pumps
/// (core keeps 8 answers of its own).
const ANSWERS_MAX: usize = 64;
const ANSWER_BYTES_MAX: usize = 64 << 10;

/// A broadcast's thread: the answers that come until it is closed. An answer
/// is read into 2,048 bytes: a longer one is cut here and is over core's
/// 1,024-byte limit anyway (core drops it).
fn run_broadcast(id: u32, socket: &UdpSocket, link: &Link, events: &Sender<Event>) {
    let mut buf = [0u8; 2048];
    let (mut answers, mut bytes) = (0usize, 0usize);
    while !link.is_closed() {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if answers < ANSWERS_MAX && bytes + n <= ANSWER_BYTES_MAX {
                    answers += 1;
                    bytes += n;
                    let _ = events.send(Event::Answer(id, from.to_string(), buf[..n].to_vec()));
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(_) => return,
        }
    }
}

impl SockTls for NativeSock {
    fn begin(&self, id: u32, to: &TlsOpen<'_>) -> Result<(), i32> {
        let pins: Vec<(u32, [u8; 32])> = to.pins.iter().map(|pin| (pin.kind, pin.hash)).collect();
        self.start_handshake(id, to.server_name, &pins)
    }

    fn end(&self, id: u32) {
        let link = self
            .entries
            .borrow()
            .get(&id)
            .map(|entry| entry.link.clone());
        if let Some(link) = link {
            // The session says it is done (`close_notify`), and the connection
            // ends with it: a writer that comes after finds no session and
            // writes nothing in clear.
            let session = link
                .secure
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(session) = session {
                session.close_write();
            }
            link.close();
        }
    }
}

impl SockTransport for NativeSock {
    fn connect(&self, id: u32, to: &Connect<'_>) -> Result<(), i32> {
        let port = u16::try_from(to.port).map_err(|_| ERR_INVALID)?;
        self.connect_to(
            id,
            to.host,
            port,
            Duration::from_millis(u64::from(to.timeout_ms)),
        );
        Ok(())
    }

    fn send(&self, id: u32, data: &[u8]) -> Result<usize, i32> {
        let entries = self.entries.borrow();
        let Some(Entry {
            link,
            writer: Some(writer),
        }) = entries.get(&id)
        else {
            return Err(ERR_NOT_FOUND);
        };
        let room = WRITE_HELD.saturating_sub(link.held.load(Ordering::Acquire));
        let take = data.len().min(room);
        if take == 0 {
            if data.is_empty() {
                return Ok(0);
            }
            link.wants_room.store(true, Ordering::Release);
            // Room was made meanwhile: it is told now, not never.
            if link.held.load(Ordering::Acquire) < WRITE_HELD
                && link.wants_room.swap(false, Ordering::AcqRel)
            {
                let _ = self.events.send(Event::Writable(id));
            }
            return Ok(0);
        }
        link.held.fetch_add(take, Ordering::AcqRel);
        if writer.send(data[..take].to_vec()).is_err() {
            return Err(ERR_IO);
        }
        Ok(take)
    }

    fn read(&self, id: u32, max: u32) {
        if let Some(entry) = self.entries.borrow().get(&id) {
            let mut allow = entry
                .link
                .allow
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *allow = allow.saturating_add(max as usize);
            drop(allow);
            entry.link.wake.notify_all();
        }
    }

    fn close(&self, id: u32) {
        self.forget(id);
    }

    fn broadcast(&self, id: u32, data: &[u8], ports: &[u16]) -> Result<(), i32> {
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|_| ERR_IO)?;
        socket.set_broadcast(true).map_err(|_| ERR_IO)?;
        socket.set_read_timeout(Some(LOOK)).map_err(|_| ERR_IO)?;
        let sent = ports
            .iter()
            .filter(|port| socket.send_to(data, ("255.255.255.255", **port)).is_ok())
            .count();
        if sent == 0 {
            return Err(ERR_IO);
        }
        let link = Link::new();
        let (thread_link, events) = (link.clone(), self.events.clone());
        self.entries
            .borrow_mut()
            .insert(id, Entry { link, writer: None });
        thread::spawn(move || run_broadcast(id, &socket, &thread_link, &events));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
    use super::*;
    use std::net::TcpListener;

    /// The next thing a thread reported, within a few seconds.
    fn next(sock: &NativeSock) -> Event {
        sock.arrived.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    fn connect(sock: &NativeSock, id: u32, port: u16) {
        sock.connect_to(id, "127.0.0.1", port, Duration::from_secs(5));
    }

    #[test]
    fn a_connection_sends_and_reads_only_what_was_asked() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"hello, world").unwrap();
            let mut got = [0u8; 4];
            stream.read_exact(&mut got).unwrap();
            got
        });
        let sock = NativeSock::new();
        connect(&sock, 7, port);
        assert!(matches!(next(&sock), Event::Connected(7)));
        assert_eq!(sock.send(7, b"ping").unwrap(), 4);
        assert_eq!(&server.join().unwrap(), b"ping");
        // Nothing arrives unasked: the greeting waits for a read of 5.
        assert!(
            sock.arrived
                .recv_timeout(Duration::from_millis(300))
                .is_err()
        );
        sock.read(7, 5);
        let Event::Data(7, first) = next(&sock) else {
            panic!("no data");
        };
        assert_eq!(first, b"hello");
        assert!(
            sock.arrived
                .recv_timeout(Duration::from_millis(300))
                .is_err()
        );
        sock.read(7, 100);
        let Event::Data(7, rest) = next(&sock) else {
            panic!("no data");
        };
        assert_eq!(rest, b", world");
        // The peer is gone: told, once a read is asked for.
        assert!(matches!(next(&sock), Event::PeerClosed(7)));
        sock.close(7);
        assert_eq!(sock.open(), 0);
    }

    #[test]
    fn a_refused_connection_and_an_unknown_name_say_why() {
        let sock = NativeSock::new();
        // A port nothing listens on.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        connect(&sock, 1, port);
        assert!(matches!(
            next(&sock),
            Event::Failed(1, SockFailure::Refused)
        ));
        sock.connect_to(2, "no-such-name.invalid", 80, Duration::from_secs(5));
        assert!(matches!(
            next(&sock),
            Event::Failed(2, SockFailure::Dns | SockFailure::Io)
        ));
    }

    #[test]
    fn a_closed_socket_is_not_reported_and_an_unknown_one_is_not_found() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let sock = NativeSock::new();
        connect(&sock, 3, port);
        let (_peer, _) = listener.accept().unwrap();
        sock.close(3);
        assert_eq!(sock.send(3, b"x"), Err(ERR_NOT_FOUND));
        assert_eq!(sock.open(), 0);
        // What the closed socket's threads posted is passed over by `report`
        // (it has no entry); `send` and `close` of an unknown id do nothing.
        sock.close(99);
        assert_eq!(sock.send(99, b"x"), Err(ERR_NOT_FOUND));
    }

    #[test]
    fn a_writer_that_is_full_takes_nothing_more_and_says_when_there_is_room() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let sock = NativeSock::new();
        connect(&sock, 4, port);
        let (mut peer, _) = listener.accept().unwrap();
        assert!(matches!(next(&sock), Event::Connected(4)));
        // The peer reads nothing: the kernel's buffers and the writer's fill.
        let block = vec![1u8; 4096];
        let mut sent = 0usize;
        loop {
            let n = sock.send(4, &block).unwrap();
            if n == 0 {
                break;
            }
            sent += n;
            assert!(sent < 64 << 20, "it never filled");
        }
        // The peer drains it: the plugin is told there is room.
        let drain = thread::spawn(move || {
            let mut left = sent;
            let mut buf = vec![0u8; 64 << 10];
            while left > 0 {
                left -= peer.read(&mut buf).unwrap().min(left);
            }
        });
        loop {
            if matches!(next(&sock), Event::Writable(4)) {
                break;
            }
        }
        drain.join().unwrap();
    }
    /// A core that is busy `busy` times, then takes what it is told.
    struct Core {
        busy: Cell<u32>,
        /// How many `received` it takes before it refuses them (`ERR_STATE`).
        receives: Cell<u32>,
        told: RefCell<Vec<String>>,
    }

    impl Core {
        fn new(busy: u32) -> Self {
            Self {
                busy: Cell::new(busy),
                receives: Cell::new(u32::MAX),
                told: RefCell::new(Vec::new()),
            }
        }

        fn take(&self, what: String) -> Result<(), i32> {
            if self.busy.get() > 0 {
                self.busy.set(self.busy.get() - 1);
                return Err(ERR_BUSY);
            }
            self.told.borrow_mut().push(what);
            Ok(())
        }
    }

    impl Sink for Core {
        fn connected(&self, id: u32) -> Result<(), i32> {
            self.take(format!("connected {id}"))
        }
        fn failed(&self, id: u32, why: SockFailure) -> Result<(), i32> {
            self.take(format!("failed {id} {why:?}"))
        }
        fn received(&self, id: u32, bytes: &[u8]) -> Result<usize, i32> {
            if self.receives.get() == 0 {
                return Err(ERR_STATE);
            }
            self.receives.set(self.receives.get() - 1);
            self.take(format!("received {id} {}", bytes.len()))?;
            Ok(bytes.len())
        }
        fn peer_closed(&self, id: u32) -> Result<(), i32> {
            self.take(format!("peer_closed {id}"))
        }
        fn writable(&self, id: u32) -> Result<(), i32> {
            self.take(format!("writable {id}"))
        }
        fn answered(&self, id: u32, _: &[u8], _: &[u8]) -> Result<(), i32> {
            self.take(format!("answered {id}"))
        }
        fn secured(&self, id: u32, _: &PeerInfo) -> Result<(), i32> {
            self.take(format!("secured {id}"))
        }
        fn untrusted(&self, id: u32, why: UntrustedWhy, _: &PeerInfo) -> Result<(), i32> {
            self.take(format!("untrusted {id} {why:?}"))
        }
        fn tls_failed(&self, id: u32, how: TlsFailure) -> Result<(), i32> {
            self.take(format!("tls_failed {id} {how:?}"))
        }
        fn network_lost(&self) -> Result<(), i32> {
            self.take(String::from("network_lost"))
        }
        fn tick(&self) -> Result<(), i32> {
            Ok(())
        }
    }

    /// A failure core answers `ERR_BUSY` is heard again at the next pump: the
    /// socket is forgotten only once core has taken it.
    #[test]
    fn a_failure_core_was_too_busy_to_take_is_told_again() {
        let sock = NativeSock::new();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        connect(&sock, 5, port);
        // The thread's report is moved to the pump by hand: core is busy.
        let core = Core::new(2);
        let failed = next(&sock);
        assert!(matches!(failed, Event::Failed(5, SockFailure::Refused)));
        sock.report(failed, &core);
        assert_eq!(sock.open(), 1, "busy: the socket is still there");
        sock.pump_with(&core, true);
        assert_eq!(sock.open(), 1, "busy again");
        sock.pump_with(&core, true);
        assert_eq!(*core.told.borrow(), ["failed 5 Refused"]);
        assert_eq!(sock.open(), 0, "taken: forgotten");
        sock.pump_with(&core, true);
        assert_eq!(core.told.borrow().len(), 1, "told once");
    }

    #[test]
    fn a_flood_of_broadcast_answers_is_capped() {
        let sock = NativeSock::new();
        // The broadcast thread passes on at most ANSWERS_MAX answers.
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(LOOK)).unwrap();
        let target = udp.local_addr().unwrap();
        let link = Link::new();
        let (events, arrived) = channel();
        let closer = link.clone();
        let reader = thread::spawn(move || run_broadcast(9, &udp, &closer, &events));
        let flooder = UdpSocket::bind("127.0.0.1:0").unwrap();
        for _ in 0..(ANSWERS_MAX * 3) {
            flooder.send_to(b"x", target).unwrap();
        }
        // The cap's worth of answers comes (as fast as the machine is), and
        // no more.
        for _ in 0..ANSWERS_MAX {
            arrived.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        thread::sleep(Duration::from_millis(300));
        link.close();
        reader.join().unwrap();
        assert_eq!(arrived.try_iter().count(), 0);
        assert_eq!(sock.open(), 0);
    }
    /// A server with a certificate of its own (nobody vouches for it): its
    /// SHA-256 hashes, and the port of a server that runs `script` once on
    /// the secure stream.
    fn tls_server(
        greeting: &'static [u8],
        script: impl FnOnce(&mut rustls::StreamOwned<rustls::ServerConnection, TcpStream>)
        + Send
        + 'static,
    ) -> (u16, [u8; 32], [u8; 32], thread::JoinHandle<()>) {
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec![String::from("x.test")])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )
            .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            use std::io::Write as _;
            let (mut tcp, _) = listener.accept().unwrap();
            // Said in clear, before the switch.
            tcp.write_all(greeting).unwrap();
            let mut stream =
                rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), tcp);
            script(&mut stream);
        });
        let hash = |bytes: &[u8]| paperpaw_digest::sha256::digest(bytes);
        let (der, spki) = (
            cert.der().to_vec(),
            rcgen::PublicKeyData::subject_public_key_info(&key),
        );
        (port, hash(&der), hash(&spki), server)
    }

    #[test]
    fn a_pinned_server_is_secured_and_the_plugins_bytes_go_through_the_session() {
        let (port, certificate, _, server) = tls_server(b"", |stream| {
            use std::io::{Read as _, Write as _};
            stream.write_all(b"hello").unwrap();
            stream.flush().unwrap();
            let mut got = [0u8; 8];
            let n = stream.read(&mut got).unwrap();
            stream.write_all(&got[..n]).unwrap();
            stream.flush().unwrap();
            let _ = stream.read(&mut got);
        });
        let sock = NativeSock::new();
        connect(&sock, 11, port);
        assert!(matches!(next(&sock), Event::Connected(11)));
        sock.start_handshake(11, "x.test", &[(0, [9; 32]), (0, certificate)])
            .unwrap();
        let Event::Secured(11, peer) = next(&sock) else {
            panic!("not secured");
        };
        assert_eq!(peer.certificate, certificate);
        sock.read(11, 5);
        let Event::Data(11, hello) = next(&sock) else {
            panic!("no data");
        };
        assert_eq!(hello, b"hello");
        assert_eq!(sock.send(11, b"ping").unwrap(), 4);
        sock.read(11, 8);
        let Event::Data(11, echo) = next(&sock) else {
            panic!("no echo");
        };
        assert_eq!(echo, b"ping");
        sock.close(11);
        server.join().unwrap();
    }

    #[test]
    fn a_server_nobody_vouches_for_is_untrusted_and_not_trusted_for_it() {
        let (port, _, key, server) = tls_server(b"", |stream| {
            use std::io::Read as _;
            let _ = stream.read(&mut [0u8; 1]);
        });
        let sock = NativeSock::new();
        connect(&sock, 12, port);
        assert!(matches!(next(&sock), Event::Connected(12)));
        // The shipped authorities do not know it: untrusted, with its hashes.
        sock.start_handshake(12, "x.test", &[]).unwrap();
        let Event::Untrusted(12, UntrustedWhy::UnknownIssuer, peer) = next(&sock) else {
            panic!("not untrusted");
        };
        assert_eq!(peer.key, key);
        sock.close(12);
        server.join().unwrap();
    }

    #[test]
    fn a_pin_of_another_certificate_is_not_pinned() {
        let (port, _, _, server) = tls_server(b"", |stream| {
            use std::io::Read as _;
            let _ = stream.read(&mut [0u8; 1]);
        });
        let sock = NativeSock::new();
        connect(&sock, 13, port);
        assert!(matches!(next(&sock), Event::Connected(13)));
        sock.start_handshake(13, "x.test", &[(1, [7; 32])]).unwrap();
        assert!(matches!(
            next(&sock),
            Event::Untrusted(13, UntrustedWhy::NotPinned, _)
        ));
        sock.close(13);
        server.join().unwrap();
    }

    /// A server that greets in clear and then waits; what it receives is
    /// kept for the test.
    fn clear_greeter(greeting: &'static [u8]) -> (u16, thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            let (mut tcp, _) = listener.accept().unwrap();
            tcp.write_all(greeting).unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 64];
            while let Ok(n @ 1..) = tcp.read(&mut buf) {
                got.extend_from_slice(&buf[..n]);
            }
            got
        });
        (port, server)
    }

    /// Pins of both kinds and an unknown socket are refused at once.
    #[test]
    fn a_handshake_is_refused_for_mixed_pins_and_an_unknown_socket() {
        let (port, server) = clear_greeter(b"");
        let sock = NativeSock::new();
        connect(&sock, 14, port);
        assert!(matches!(next(&sock), Event::Connected(14)));
        assert_eq!(
            sock.start_handshake(14, "x.test", &[(0, [1; 32]), (1, [2; 32])]),
            Err(ERR_INVALID)
        );
        assert_eq!(sock.start_handshake(99, "x.test", &[]), Err(ERR_NOT_FOUND));
        sock.close(14);
        server.join().unwrap();
    }

    /// The peer's greeting is waiting in clear when the switch begins: the
    /// handshake fails closed (`tls`) and sends nothing.
    #[test]
    fn a_greeting_waiting_in_clear_fails_the_handshake_and_sends_nothing() {
        let (port, server) = clear_greeter(b"220 injected");
        let sock = NativeSock::new();
        connect(&sock, 18, port);
        assert!(matches!(next(&sock), Event::Connected(18)));
        // Give the greeting time to arrive.
        thread::sleep(Duration::from_millis(300));
        sock.start_handshake(18, "x.test", &[]).unwrap();
        assert!(matches!(next(&sock), Event::TlsFailed(18, TlsFailure::Tls)));
        // And nothing of the plugin's goes out in clear after it.
        let _ = sock.send(18, b"CLEAR BYTES");
        thread::sleep(Duration::from_millis(300));
        sock.close(18);
        assert_eq!(server.join().unwrap(), b"");
    }

    /// Bytes the reader has already taken (not core's yet) refuse the switch
    /// at once, before anything is changed.
    #[test]
    fn bytes_read_and_not_taken_refuse_the_switch_at_once() {
        let (port, server) = clear_greeter(b"220 injected");
        let sock = NativeSock::new();
        connect(&sock, 19, port);
        assert!(matches!(next(&sock), Event::Connected(19)));
        sock.read(19, 4);
        thread::sleep(Duration::from_millis(400));
        assert_eq!(sock.start_handshake(19, "x.test", &[]), Err(ERR_STATE));
        assert!(
            !sock.entries.borrow()[&19]
                .link
                .secured
                .load(Ordering::SeqCst)
        );
        sock.close(19);
        server.join().unwrap();
    }

    /// A man in the middle sends the answer in two pieces: core takes the
    /// first and refuses the second, which the shell keeps. The switch is
    /// refused while it holds them, so the plugin's first secure read cannot
    /// be those clear bytes.
    #[test]
    fn clear_bytes_core_refused_do_not_pass_the_switch() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            use std::io::Write as _;
            let (mut tcp, _) = listener.accept().unwrap();
            tcp.write_all(b"abcd").unwrap();
            thread::sleep(Duration::from_millis(200));
            tcp.write_all(b"efghij").unwrap();
            thread::sleep(Duration::from_millis(1500));
        });
        let sock = NativeSock::new();
        connect(&sock, 15, port);
        assert!(matches!(next(&sock), Event::Connected(15)));
        sock.read(15, 10);
        thread::sleep(Duration::from_millis(600));
        let core = Core::new(0);
        // Core takes the first piece and refuses the second.
        core.receives.set(1);
        sock.pump_with(&core, true);
        assert_eq!(sock.unread.borrow().get(&15).map(Vec::len), Some(6));
        assert_eq!(sock.start_handshake(15, "x.test", &[]), Err(ERR_STATE));
        // Nor was a handshake begun: the connection is still plain.
        assert!(
            !sock.entries.borrow()[&15]
                .link
                .secured
                .load(Ordering::SeqCst)
        );
        sock.close(15);
        server.join().unwrap();
    }

    /// An ordinary STARTTLS: the plugin read a short answer (core granted the
    /// full `max`, so the allowance is not spent), then switches. The reader
    /// parks at its next look and the handshake takes the connection.
    #[test]
    fn a_switch_after_a_short_read_goes_through() {
        let (port, certificate, _, server) = tls_server(b"ok", |stream| {
            use std::io::{Read as _, Write as _};
            stream.write_all(b"secret").unwrap();
            stream.flush().unwrap();
            let _ = stream.read(&mut [0u8; 1]);
        });
        let sock = NativeSock::new();
        connect(&sock, 16, port);
        assert!(matches!(next(&sock), Event::Connected(16)));
        sock.read(16, 10);
        let core = Core::new(0);
        let until = Instant::now() + Duration::from_secs(10);
        while !core
            .told
            .borrow()
            .iter()
            .any(|line| line == "received 16 2")
        {
            sock.pump_with(&core, true);
            assert!(Instant::now() < until, "no greeting");
            thread::sleep(Duration::from_millis(10));
        }
        // 8 bytes of the read are still granted; the reader is blocked in a read.
        sock.start_handshake(16, "x.test", &[(0, certificate)])
            .unwrap();
        let Event::Secured(16, _) = next(&sock) else {
            panic!("not secured");
        };
        sock.read(16, 6);
        let Event::Data(16, data) = next(&sock) else {
            panic!("no data");
        };
        assert_eq!(data, b"secret");
        sock.close(16);
        server.join().unwrap();
    }

    /// Core drops the session just before it closes the socket: what the
    /// plugin queued in between is not written in clear, and the server is
    /// told the session ended (`close_notify`).
    #[test]
    fn after_the_session_ends_nothing_goes_out_in_clear() {
        let (port, certificate, _, server) = tls_server(b"", |stream| {
            use std::io::Read as _;
            // The first read takes the handshake through; what comes after is
            // the end of the session, not a clear byte (that would be an error).
            let mut buf = [0u8; 16];
            let first = stream.read(&mut buf);
            let after = stream.read(&mut buf);
            assert!(matches!(first, Ok(0)), "{first:?}");
            assert!(after.is_ok(), "{after:?}");
        });
        let sock = NativeSock::new();
        connect(&sock, 17, port);
        assert!(matches!(next(&sock), Event::Connected(17)));
        sock.start_handshake(17, "x.test", &[(0, certificate)])
            .unwrap();
        assert!(matches!(next(&sock), Event::Secured(17, _)));
        SockTls::end(&sock, 17);
        // Queued after the session went: the writer finds none.
        let _ = sock.send(17, b"CLEAR BYTES");
        server.join().unwrap();
        sock.close(17);
    }
}
