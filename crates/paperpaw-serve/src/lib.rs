//! Serving HTTP for plugins, for shells with the standard library (the
//! emulator, Android): a small HTTP/1.1 server per plugin server, each
//! connection on a thread of its own, one request per connection. Also
//! plugins' sockets ([`Sockets`]), secure ones too (rustls with ring:
//! implicit TLS, STARTTLS, Mozilla's authorities or pinned certificates and
//! keys; see `tls`).
//!
//! The shell starts and stops servers as the kernel asks
//! (`ServerCall`), passes requests on (`ServerEvent`) and the answers
//! back. Bodies go in parts both ways, so they never need to fit in the
//! kernel's memory: request bodies as `ServerEvent::Body`, files sent as
//! answers through [`Servers::pump`]. The channels in between are
//! bounded, so a fast network waits for a slow storage, not the other
//! way round.
//!
//! Nothing a client in the network sends holds on to more than a little:
//! a request's head is read by `paperpaw_http::server`, up to its limit
//! and within half a minute in all; a server serves a few connections at
//! a time (more are answered 503); an answer that comes before the body
//! is whole (no room for it) ends the upload; and stopping a server ends
//! its connections.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError, channel,
    sync_channel,
};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use paperpaw_core::{ServedFile, ServerCall, ServerEvent};
use paperpaw_http::server::{MAX_REQUEST_HEAD, RequestHead, parse_request};

mod link;
#[cfg(not(target_arch = "wasm32"))]
pub mod secure;
mod sockets;
#[cfg(not(target_arch = "wasm32"))]
mod tls;
#[cfg(target_arch = "wasm32")]
#[path = "tls_stub.rs"]
mod tls;

pub use sockets::Sockets;

/// The secure sessions [`Sockets`] holds at once, for
/// `KernelBuilder::tls_sessions` (0 in the browser's module, which has no
/// sockets).
pub const TLS_SESSIONS: u8 = tls::SESSIONS;

/// Size of the parts bodies go in.
const PART: usize = 64 * 1024;
/// Events waiting at most.
const QUEUE: usize = 8;
/// Parts of a file read ahead at most, per connection. The shell is
/// woken each time half of them were sent, so it reads many at once.
const READ_AHEAD: usize = 32;
/// Connections a server serves at a time; more are answered 503.
const MAX_CONNECTIONS: usize = 8;
/// How long a request's head may take to come, in all.
const HEAD_TIME: Duration = if cfg!(test) {
    Duration::from_secs(1)
} else {
    Duration::from_secs(30)
};
/// How long a read of a request's body may wait.
const READ_TIME: Duration = Duration::from_secs(30);
/// How long an answer may take the plugin.
const ANSWER_TIME: Duration = Duration::from_secs(120);
/// What of a body is still taken after an answer that came before the
/// body was whole (so that the client reads the answer rather than a
/// reset connection), at most.
const DRAIN: u64 = 256 * 1024;
const DRAIN_TIME: Duration = Duration::from_secs(2);

/// An answer for a waiting connection.
struct Answer {
    status: u16,
    content_type: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// Instead of `body`: its size, and its parts as they are read.
    file: Option<(u64, Receiver<Vec<u8>>)>,
}

/// A file being sent.
struct Stream {
    /// The server it is sent for.
    server: u32,
    path: String,
    size: u64,
    at: u64,
    parts: SyncSender<Vec<u8>>,
    /// A part read but not taken yet.
    held: Option<Vec<u8>>,
}

/// The requests waiting for an answer: their server, and where the
/// answer goes.
type Waiting = Arc<Mutex<HashMap<u32, (u32, Sender<Answer>)>>>;
type Wake = Arc<dyn Fn() + Send + Sync>;

/// A server listening.
struct Running {
    /// Ends the server's thread once set (and it is woken).
    stop: Arc<AtomicBool>,
    port: u16,
    /// Its connections, numbered, to end them with it.
    open: Open,
}

type Open = Arc<Mutex<HashMap<u64, TcpStream>>>;

/// The servers of all plugins.
pub struct Servers {
    events: SyncSender<ServerEvent>,
    receiver: Receiver<ServerEvent>,
    running: HashMap<u32, Running>,
    waiting: Waiting,
    streams: Vec<Stream>,
    next_request: Arc<AtomicU32>,
    /// Called when there is something to do, e.g. to wake an event loop.
    wake: Wake,
    /// Where servers listen: every interface (the local network) unless
    /// told otherwise.
    address: IpAddr,
}

impl std::fmt::Debug for Servers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Servers")
            .field("running", &self.running.len())
            .field("streams", &self.streams.len())
            .finish_non_exhaustive()
    }
}

impl Servers {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (events, receiver) = sync_channel(QUEUE);
        Self {
            events,
            receiver,
            running: HashMap::new(),
            waiting: Arc::default(),
            streams: Vec::new(),
            next_request: Arc::new(AtomicU32::new(0)),
            wake: Arc::new(wake),
            address: Ipv4Addr::UNSPECIFIED.into(),
        }
    }

    /// Servers that listen on `address` only, e.g. the loopback address
    /// for tests, which then reach nothing outside the computer (and ask
    /// no firewall for the network).
    pub fn listening_on(mut self, address: IpAddr) -> Self {
        self.address = address;
        self
    }

    /// Does what the kernel asks.
    pub fn apply(&mut self, call: ServerCall) {
        match call {
            ServerCall::Listen { server, port } => self.listen(server, port),
            ServerCall::Stop { server } => self.stop(server),
            ServerCall::Respond {
                request,
                status,
                content_type,
                headers,
                body,
                file,
            } => {
                let Some((server, answer)) = self
                    .waiting
                    .lock()
                    .ok()
                    .and_then(|mut w| w.remove(&request))
                else {
                    return;
                };
                let file = file.map(|ServedFile { path, size }| {
                    let (parts, receiver) = sync_channel(READ_AHEAD);
                    self.streams.push(Stream {
                        server,
                        path,
                        size,
                        at: 0,
                        parts,
                        held: None,
                    });
                    (size, receiver)
                });
                let _ = answer.send(Answer {
                    status,
                    content_type,
                    headers,
                    body,
                    file,
                });
            }
        }
    }

    /// Stops a server: its thread, its connections, the answers its
    /// requests wait for and the files it sends.
    fn stop(&mut self, server: u32) {
        let Some(running) = self.running.remove(&server) else {
            return;
        };
        running.stop.store(true, Ordering::Relaxed);
        // Wakes the thread waiting for the next connection.
        let wake_up = (Ipv4Addr::LOCALHOST, running.port).into();
        let _ = TcpStream::connect_timeout(&wake_up, Duration::from_secs(1));
        if let Ok(mut open) = running.open.lock() {
            for (_, stream) in open.drain() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.retain(|_, (of, _)| *of != server);
        }
        self.streams.retain(|stream| stream.server != server);
    }

    /// An event to pass to the kernel, if any.
    pub fn next(&self) -> Option<ServerEvent> {
        self.receiver.try_recv().ok()
    }

    /// Reads the next parts of the files being sent, as far as the
    /// connections take them; `read` reads part of a file from `at`
    /// (`Kernel::read_served`). Call after the events were passed on.
    pub fn pump(&mut self, mut read: impl FnMut(&str, u64, &mut [u8]) -> Option<usize>) {
        let mut buffer = Vec::new();
        self.streams.retain_mut(|stream| {
            loop {
                let part = match stream.held.take() {
                    Some(part) => part,
                    None if stream.at >= stream.size => return false,
                    None => {
                        buffer.resize(PART, 0);
                        let want = (stream.size - stream.at).min(PART as u64) as usize;
                        match read(&stream.path, stream.at, &mut buffer[..want]) {
                            Some(n) if n > 0 => buffer[..n].to_vec(),
                            // Gone or shorter than it was: the connection
                            // ends early.
                            _ => return false,
                        }
                    }
                };
                let len = part.len() as u64;
                match stream.parts.try_send(part) {
                    Ok(()) => stream.at += len,
                    Err(TrySendError::Full(part)) => {
                        stream.held = Some(part);
                        return true;
                    }
                    Err(TrySendError::Disconnected(_)) => return false,
                }
            }
        });
    }

    /// Whether any server runs or a file is being sent, e.g. to keep
    /// polling.
    pub fn any(&self) -> bool {
        !self.running.is_empty() || !self.streams.is_empty()
    }

    fn listen(&mut self, server: u32, port: u16) {
        let address = self.address;
        let events = self.events.clone();
        let wake = self.wake.clone();
        // The shell is woken once the event is there to take, and before
        // waiting for room.
        let send = move |event: ServerEvent| {
            match events.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(event)) => {
                    wake();
                    if events.send(event).is_err() {
                        return;
                    }
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
            wake();
        };
        // The port asked for, else the next free ones.
        let listener =
            (port..=port.saturating_add(19)).find_map(|p| TcpListener::bind((address, p)).ok());
        let Some(listener) = listener else {
            send(ServerEvent::Failed {
                server,
                reason: format!("port {port} is taken"),
            });
            return;
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
        let stop = Arc::new(AtomicBool::new(false));
        let open = Open::default();
        self.running.insert(
            server,
            Running {
                stop: stop.clone(),
                port,
                open: open.clone(),
            },
        );
        send(ServerEvent::Started {
            server,
            address: format!("{}:{port}", local_address()),
        });
        let (waiting, next, wake) = (
            self.waiting.clone(),
            self.next_request.clone(),
            self.wake.clone(),
        );
        thread::spawn(move || {
            let count = Arc::new(AtomicUsize::new(0));
            let mut numbers = 0u64;
            for stream in listener.incoming() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(mut stream) = stream else {
                    // E.g. out of file handles: not at full speed.
                    thread::sleep(Duration::from_millis(50));
                    continue;
                };
                if count.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                    count.fetch_sub(1, Ordering::Relaxed);
                    let _ = write_head(&mut stream, 503, "text/plain", &[], 0);
                    continue;
                }
                numbers += 1;
                let number = numbers;
                if let (Ok(mut open), Ok(clone)) = (open.lock(), stream.try_clone()) {
                    open.insert(number, clone);
                }
                let connection = Connection {
                    server,
                    send: send.clone(),
                    waiting: waiting.clone(),
                    next: next.clone(),
                    wake: wake.clone(),
                };
                let (count, open) = (count.clone(), open.clone());
                thread::spawn(move || {
                    connection.handle(stream);
                    if let Ok(mut open) = open.lock() {
                        open.remove(&number);
                    }
                    count.fetch_sub(1, Ordering::Relaxed);
                });
            }
        });
    }
}

/// This device's address in the local network (where the default route
/// goes; no packet is sent), else the loopback address.
pub fn local_address() -> IpAddr {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|socket| {
            socket.connect(("8.8.8.8", 80))?;
            socket.local_addr()
        })
        .map(|address| address.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

/// One connection: one request and its answer.
struct Connection<F> {
    server: u32,
    send: F,
    waiting: Waiting,
    next: Arc<AtomicU32>,
    wake: Wake,
}

/// Why no request came: the status to answer, if any.
enum NoRequest {
    /// The connection ended or took too long.
    Gone,
    Answer(u16),
}

impl<F: Fn(ServerEvent)> Connection<F> {
    fn handle(self, mut stream: TcpStream) {
        let _ = stream.set_nonblocking(false);
        let Ok(mut writer) = stream.try_clone() else {
            return;
        };
        let (head, mut early) = match read_head(&mut stream) {
            Ok(request) => request,
            Err(NoRequest::Gone) => return,
            Err(NoRequest::Answer(status)) => {
                let _ = write_head(&mut writer, status, "text/plain", &[], 0);
                return;
            }
        };
        if head.chunked {
            let _ = write_head(&mut writer, 411, "text/plain", &[], 0);
            return;
        }
        let _ = stream.set_read_timeout(Some(READ_TIME));
        let request = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (answer, receiver) = channel();
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.insert(request, (self.server, answer));
        }
        let RequestHead {
            method,
            target,
            length,
            ..
        } = head;
        (self.send)(ServerEvent::Request {
            server: self.server,
            request,
            method,
            target,
            body: length > 0,
        });
        let mut left = length;
        early.truncate(usize::try_from(left).unwrap_or(usize::MAX));
        while left > 0 {
            // An answer before the body is whole (e.g. no room for it):
            // the rest is not wanted.
            match receiver.try_recv() {
                Ok(answer) => {
                    self.answer(&mut writer, answer);
                    return drain(&mut stream, left);
                }
                Err(TryRecvError::Disconnected) => return self.close(request),
                Err(TryRecvError::Empty) => {}
            }
            let part = if early.is_empty() {
                let mut part = vec![0u8; left.min(PART as u64) as usize];
                if stream.read_exact(&mut part).is_err() {
                    return self.close(request);
                }
                part
            } else {
                std::mem::take(&mut early)
            };
            left -= part.len() as u64;
            (self.send)(ServerEvent::Body {
                request,
                data: part,
                last: left == 0,
            });
        }
        match receiver.recv_timeout(ANSWER_TIME) {
            Ok(answer) => self.answer(&mut writer, answer),
            Err(RecvTimeoutError::Timeout) => {
                let _ = write_head(&mut writer, 503, "text/plain", &[], 0);
                self.close(request);
            }
            // The server stopped.
            Err(RecvTimeoutError::Disconnected) => self.close(request),
        }
    }

    /// Writes `answer`, its file as the shell reads it.
    fn answer(&self, writer: &mut TcpStream, answer: Answer) {
        let size = answer
            .file
            .as_ref()
            .map_or(answer.body.len() as u64, |f| f.0);
        let written = write_head(
            writer,
            answer.status,
            &answer.content_type,
            &answer.headers,
            size,
        )
        .and_then(|()| writer.write_all(&answer.body));
        if written.is_err() {
            return;
        }
        if let Some((_, parts)) = answer.file {
            let mut sent = 0;
            loop {
                let part = match parts.try_recv() {
                    Ok(part) => part,
                    Err(TryRecvError::Disconnected) => break,
                    // Nothing read ahead: the shell reads on.
                    Err(TryRecvError::Empty) => {
                        (self.wake)();
                        match parts.recv_timeout(Duration::from_secs(60)) {
                            Ok(part) => part,
                            Err(_) => break,
                        }
                    }
                };
                sent += 1;
                if sent % (READ_AHEAD / 2) == 0 {
                    (self.wake)();
                }
                if writer.write_all(&part).is_err() {
                    return;
                }
            }
        }
        let _ = writer.flush();
    }

    /// The connection was lost: the kernel forgets the request.
    fn close(&self, request: u32) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(&request);
        }
        (self.send)(ServerEvent::Closed { request });
    }
}

/// Reads a request's head, at most [`MAX_REQUEST_HEAD`] bytes within
/// [`HEAD_TIME`] in all; the head and the bytes of the body that came
/// with it.
fn read_head(stream: &mut TcpStream) -> Result<(RequestHead, Vec<u8>), NoRequest> {
    let until = Instant::now() + HEAD_TIME;
    let mut data = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return Err(NoRequest::Gone);
        }
        let n = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return Err(NoRequest::Gone),
            Ok(n) => n,
        };
        data.extend_from_slice(&buffer[..n]);
        match parse_request(&data) {
            Ok(Some((head, used))) => return Ok((head, data.split_off(used))),
            Ok(None) => {}
            Err(_) if data.len() >= MAX_REQUEST_HEAD => return Err(NoRequest::Answer(431)),
            Err(_) => return Err(NoRequest::Answer(400)),
        }
    }
}

/// Takes up to [`DRAIN`] of `left` bytes still coming, for a short
/// while, then ends the connection.
fn drain(stream: &mut TcpStream, left: u64) {
    let _ = stream.shutdown(Shutdown::Write);
    let _ = stream.set_read_timeout(Some(DRAIN_TIME));
    let until = Instant::now() + DRAIN_TIME;
    let mut taken = 0;
    let mut buffer = [0u8; 4096];
    while taken < left.min(DRAIN) && Instant::now() < until {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => taken += n as u64,
        }
    }
}

fn write_head(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    headers: &[(String, String)],
    length: u64,
) -> std::io::Result<()> {
    stream.write_all(&paperpaw_http::server::response_head(
        status,
        content_type,
        headers,
        length,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next(servers: &Servers) -> ServerEvent {
        servers
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
    }

    #[test]
    fn serves_a_request_in_parts() {
        let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
        servers.apply(ServerCall::Listen {
            server: 1,
            port: 18_765,
        });
        let ServerEvent::Started { address, .. } = next(&servers) else {
            panic!("not started");
        };
        let port: u16 = address.rsplit(':').next().unwrap().parse().unwrap();
        // A body of several parts goes up; a file of several parts comes
        // down.
        let up: Vec<u8> = (0..PART * 2 + 10).map(|i| i as u8).collect();
        let down: Vec<u8> = (0..PART * 3 + 7).map(|i| (i * 7) as u8).collect();
        let sent = up.clone();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let head = format!(
                "PUT /up?x=1 HTTP/1.1\r\nHost: a\r\nContent-Length: {}\r\n\r\n",
                sent.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&sent).unwrap();
            let mut answer = Vec::new();
            stream.read_to_end(&mut answer).unwrap();
            answer
        });
        let ServerEvent::Request {
            request,
            method,
            target,
            body,
            ..
        } = next(&servers)
        else {
            panic!("no request");
        };
        assert_eq!(
            (method.as_str(), target.as_str(), body),
            ("PUT", "/up?x=1", true)
        );
        let mut received = Vec::new();
        loop {
            let ServerEvent::Body { data, last, .. } = next(&servers) else {
                panic!("no body");
            };
            assert!(data.len() <= PART);
            received.extend(data);
            if last {
                break;
            }
        }
        assert_eq!(received, up);
        servers.apply(ServerCall::Respond {
            request,
            status: 200,
            content_type: String::from("application/octet-stream"),
            headers: Vec::new(),
            body: Vec::new(),
            file: Some(ServedFile {
                path: String::from("f"),
                size: down.len() as u64,
            }),
        });
        while servers.any() && !client.is_finished() {
            servers.pump(|path, at, buffer| {
                assert_eq!(path, "f");
                let at = at as usize;
                let n = buffer.len().min(down.len() - at);
                buffer[..n].copy_from_slice(&down[at..at + n]);
                Some(n)
            });
            if servers.streams.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let answer = client.join().unwrap();
        let split = answer.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let head = String::from_utf8_lossy(&answer[..split]);
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
        assert!(head.contains(&format!("Content-Length: {}", down.len())));
        assert!(answer[split..] == down[..], "the file arrives whole");
        servers.apply(ServerCall::Stop { server: 1 });
        assert!(!servers.any());
    }

    /// A server listening (as server 1) near `port`, and its port.
    fn serving(servers: &mut Servers, port: u16) -> u16 {
        servers.apply(ServerCall::Listen { server: 1, port });
        let ServerEvent::Started { address, .. } = next(servers) else {
            panic!("not started");
        };
        address.rsplit(':').next().unwrap().parse().unwrap()
    }

    fn connect(port: u16) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
    }

    /// The head of the answer `stream` gets (empty if none).
    fn answer_head(stream: &mut TcpStream) -> String {
        let mut answer = Vec::new();
        let mut byte = [0u8];
        while !answer.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => answer.push(byte[0]),
                _ => break,
            }
        }
        String::from_utf8_lossy(&answer).into_owned()
    }

    #[test]
    fn long_or_bad_heads_are_refused_at_once() {
        let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
        let port = serving(&mut servers, 18_865);
        // A line without end, just longer than any head: all of it is read
        // before the answer, so closing never resets what was answered.
        let mut client = connect(port);
        let _ = client.write_all(&vec![b'a'; MAX_REQUEST_HEAD + 1]);
        assert!(answer_head(&mut client).starts_with("HTTP/1.1 431 "));
        for bad in [
            &b"PUT / HTTP/1.1\r\nContent-Length: many\r\n\r\n"[..],
            b"GET / SPDY/3\r\n\r\n",
            b"GET /\r\n\r\n",
        ] {
            let mut client = connect(port);
            client.write_all(bad).unwrap();
            assert!(answer_head(&mut client).starts_with("HTTP/1.1 400 "));
        }
        assert!(
            servers
                .receiver
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "no request reaches the kernel"
        );
        servers.apply(ServerCall::Stop { server: 1 });
    }

    #[test]
    fn a_slow_head_ends_in_time() {
        let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
        let port = serving(&mut servers, 18_885);
        let mut client = connect(port);
        let started = Instant::now();
        // A byte a while, each well within a read's patience.
        let mut ended = false;
        for byte in b"GET / HTTP/1.1\r\nX: ".iter().cycle().take(40) {
            if client.write_all(&[*byte]).is_err() {
                ended = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
            let mut probe = [0u8];
            client.set_nonblocking(true).unwrap();
            let read = client.read(&mut probe);
            client.set_nonblocking(false).unwrap();
            if matches!(read, Ok(0)) {
                ended = true;
                break;
            }
        }
        assert!(ended, "the connection ends");
        assert!(started.elapsed() < HEAD_TIME * 3);
        servers.apply(ServerCall::Stop { server: 1 });
    }

    #[test]
    fn connections_are_limited() {
        let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
        let port = serving(&mut servers, 18_905);
        let idle: Vec<TcpStream> = (0..MAX_CONNECTIONS).map(|_| connect(port)).collect();
        let mut one_more = connect(port);
        assert!(answer_head(&mut one_more).starts_with("HTTP/1.1 503 "));
        drop(idle);
        servers.apply(ServerCall::Stop { server: 1 });
    }

    #[test]
    fn stopping_ends_connections_and_frees_the_port() {
        let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
        let port = serving(&mut servers, 18_925);
        let mut client = connect(port);
        client.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        assert!(matches!(next(&servers), ServerEvent::Request { .. }));
        // The request waits for its answer; the server stops.
        let started = Instant::now();
        servers.apply(ServerCall::Stop { server: 1 });
        assert_eq!(answer_head(&mut client), "");
        assert!(started.elapsed() < Duration::from_secs(4));
        // The listener goes once its thread woke.
        let freed = (0..40).any(|_| {
            thread::sleep(Duration::from_millis(50));
            TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
        });
        assert!(freed, "the port is free again");
    }

    #[test]
    fn an_early_answer_ends_the_upload() {
        let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
        let port = serving(&mut servers, 18_945);
        let mut client = connect(port);
        let head = format!("PUT /up HTTP/1.1\r\nContent-Length: {}\r\n\r\n", 1u64 << 34);
        client.write_all(head.as_bytes()).unwrap();
        client.write_all(&[7; PART]).unwrap();
        let ServerEvent::Request { request, body, .. } = next(&servers) else {
            panic!("no request");
        };
        assert!(body);
        assert!(matches!(
            next(&servers),
            ServerEvent::Body { last: false, .. }
        ));
        // No room for it: the kernel answers before the body is whole.
        servers.apply(ServerCall::Respond {
            request,
            status: 507,
            content_type: String::from("text/plain"),
            headers: Vec::new(),
            body: b"the storage is full".to_vec(),
            file: None,
        });
        let _ = client.write_all(&[7; PART]);
        assert!(answer_head(&mut client).starts_with("HTTP/1.1 507 "));
        servers.apply(ServerCall::Stop { server: 1 });
    }
}
