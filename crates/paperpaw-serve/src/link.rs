//! One TCP connection of a plugin's socket, on threads: the one that
//! connects reads what the kernel asks for, one writes, and with an idle
//! limit one watches. With TLS ([`crate::tls`]) the handshake runs on
//! them before reads and writes begin: at once on a connection, or where
//! the kernel asks for STARTTLS, once what was sent before is out.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use paperpaw_core::sdk::{CertificateHash, KeyHash, SocketError};
use paperpaw_core::{SocketEvent, TlsSetup};

use crate::tls::{self, Refusal, Secure, Secured, Slot, TlsContext};

/// How long connecting may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What a connection reports (nothing, once the kernel closed it).
pub(crate) type Report = Arc<dyn Fn(SocketEvent) + Send + Sync>;

/// What the writing thread is asked to do, in order.
#[derive(Debug)]
pub(crate) enum Job {
    Send(Vec<u8>),
    /// Switch to TLS once what was sent before is out; the call's time
    /// limit ends at `deadline`.
    StartTls {
        tls: TlsSetup,
        deadline: Instant,
    },
}

/// A connection, seen from the shell's side.
#[derive(Debug)]
pub(crate) struct Link {
    pub reads: Sender<u32>,
    pub writes: Sender<Job>,
    pub stream: Arc<Mutex<Option<TcpStream>>>,
}

/// Why connecting failed with `e`.
fn connect_error(e: &std::io::Error) -> SocketError {
    match e.kind() {
        std::io::ErrorKind::TimedOut => SocketError::Timeout,
        std::io::ErrorKind::OutOfMemory => SocketError::NoMemory,
        _ => SocketError::Refused,
    }
}

/// Why reading or writing failed with `e`.
fn io_error(e: &std::io::Error) -> SocketError {
    match e.kind() {
        // A write that moved nothing for the stall (`WouldBlock` is what a
        // socket's timeout is on Unix).
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => SocketError::Timeout,
        std::io::ErrorKind::InvalidData => SocketError::Tls,
        _ => SocketError::Lost,
    }
}

/// What a handshake that did not come about is reported as.
fn refused(socket: u32, refusal: Refusal) -> SocketEvent {
    match refusal {
        Refusal::Untrusted {
            certificate,
            key,
            why,
        } => SocketEvent::Untrusted {
            socket,
            certificate: CertificateHash::new(certificate),
            key: KeyHash::new(key),
            why,
        },
        Refusal::Failed(error, reason) => SocketEvent::Failed {
            socket,
            error,
            reason,
        },
    }
}

fn secured(socket: u32, shown: Secured) -> SocketEvent {
    SocketEvent::Secured {
        socket,
        certificate: CertificateHash::new(shown.certificate),
        key: KeyHash::new(shown.key),
        time_checked: shown.time_checked,
    }
}

/// Whether bytes of the server wait, unread, on `tcp`: sent past its
/// go-ahead. Where that cannot be told, they do.
///
/// Non-blocking is a flag of the socket, so it holds for the reading
/// handle too while it is set: that is safe only because the kernel asks
/// for STARTTLS only with no read under way (`start_tls` in
/// `paperpaw-core`'s `io/sockets.rs`), and the reading thread does
/// nothing between reads.
fn bytes_wait(tcp: &TcpStream) -> bool {
    if tcp.set_nonblocking(true).is_err() {
        return true;
    }
    let waiting = matches!(tcp.peek(&mut [0u8; 1]), Ok(n) if n > 0);
    waiting || tcp.set_nonblocking(false).is_err()
}

/// What the threads of one connection share.
struct Shared {
    socket: u32,
    report: Report,
    /// Set by whoever reported the end of the connection first.
    failed: AtomicBool,
    secure: OnceLock<Arc<Secure>>,
    env: TlsContext,
    /// Told each time something was sent or read on a secure connection.
    tick: Sender<()>,
    /// What watches for idleness, until a secure connection takes it.
    ticks: Mutex<Option<Receiver<()>>>,
    stream: Arc<Mutex<Option<TcpStream>>>,
}

impl Shared {
    /// Reports the end of the connection, the first time only.
    fn end(&self, event: SocketEvent) {
        if !self.failed.swap(true, Ordering::Relaxed) {
            (self.report)(event);
        }
    }

    /// Ends what is read and written, so that no thread waits on it.
    fn shut(&self) {
        if let Ok(stream) = self.stream.lock()
            && let Some(stream) = stream.as_ref()
        {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    fn tick(&self) {
        if self.secure.get().is_some() {
            let _ = self.tick.send(());
        }
    }

    /// The handshake on `tcp` for `tls`, within `deadline`, holding
    /// `slot`: the connection is secure after it (reported), or ended
    /// (reported, and shut). Whether it is secure.
    fn secure_up(
        self: &Arc<Self>,
        tcp: &mut TcpStream,
        tls: &TlsSetup,
        deadline: Instant,
        slot: Slot,
    ) -> bool {
        match tls::handshake(
            tcp,
            tls,
            self.env.verifier(tls),
            deadline,
            slot,
            self.env.stall(),
        ) {
            Ok((secure, shown)) => {
                let _ = self.secure.set(secure);
                (self.report)(secured(self.socket, shown));
                self.watch(tls.idle_ms);
                true
            }
            Err(refusal) => {
                self.shut();
                self.end(refused(self.socket, refusal));
                false
            }
        }
    }

    /// Ends the connection where nothing is sent or read for `idle_ms`
    /// (0: never), reporting `Failed`, `Timeout`.
    fn watch(self: &Arc<Self>, idle_ms: u32) {
        let ticks = self.ticks.lock().ok().and_then(|mut ticks| ticks.take());
        let (Some(ticks), true) = (ticks, idle_ms > 0) else {
            return;
        };
        let idle = Duration::from_millis(u64::from(idle_ms));
        let shared: Weak<Self> = Arc::downgrade(self);
        thread::spawn(move || {
            loop {
                match ticks.recv_timeout(idle) {
                    Ok(()) => {}
                    // The connection's threads are gone.
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {
                        let Some(shared) = shared.upgrade() else {
                            return;
                        };
                        shared.shut();
                        shared.end(SocketEvent::Failed {
                            socket: shared.socket,
                            error: SocketError::Timeout,
                            reason: String::from("idle"),
                        });
                        return;
                    }
                }
            }
        });
    }
}

/// Connects to `host`:`port` on threads of its own; with `tls`, securely.
pub(crate) fn start(
    socket: u32,
    host: String,
    port: u16,
    tls: Option<TlsSetup>,
    env: TlsContext,
    report: Report,
) -> Link {
    let (reads, read_asks) = channel::<u32>();
    let (writes, write_asks) = channel::<Job>();
    let (tick, ticks) = channel::<()>();
    let stream = Arc::new(Mutex::new(None));
    let shared = Arc::new(Shared {
        socket,
        report,
        failed: AtomicBool::new(false),
        secure: OnceLock::new(),
        env,
        tick,
        ticks: Mutex::new(Some(ticks)),
        stream: stream.clone(),
    });
    thread::spawn(move || connection(&shared, &host, port, tls, &read_asks, write_asks));
    Link {
        reads,
        writes,
        stream,
    }
}

/// Resolves `host` and connects, within `limit`.
fn dial(host: &str, port: u16, limit: Duration) -> Result<TcpStream, (SocketError, String)> {
    let mut addresses = (host, port)
        .to_socket_addrs()
        .map_err(|e| (SocketError::Dns, e.to_string()))?;
    let address: SocketAddr = addresses
        .next()
        .ok_or((SocketError::Dns, String::from("no such host")))?;
    TcpStream::connect_timeout(&address, limit).map_err(|e| (connect_error(&e), e.to_string()))
}

fn connection(
    shared: &Arc<Shared>,
    host: &str,
    port: u16,
    tls: Option<TlsSetup>,
    read_asks: &Receiver<u32>,
    write_asks: Receiver<Job>,
) {
    let socket = shared.socket;
    let failure = |error, reason: &str| SocketEvent::Failed {
        socket,
        error,
        reason: String::from(reason),
    };
    let deadline = tls
        .as_ref()
        .map(|t| Instant::now() + Duration::from_millis(u64::from(t.timeout_ms)));
    // A secure session first, before the host is looked up.
    let slot = match deadline {
        Some(deadline) => match shared.env.gate().take(deadline) {
            Some(slot) => Some(slot),
            None => {
                let wait = "no secure session was free in time";
                return shared.end(failure(SocketError::Busy, wait));
            }
        },
        None => None,
    };
    let limit = deadline.map_or(CONNECT_TIMEOUT, |d| CONNECT_TIMEOUT.min(tls::left(d)));
    let mut reading = match dial(host, port, limit) {
        Ok(stream) => stream,
        Err((error, reason)) => return shared.end(failure(error, &reason)),
    };
    let _ = reading.set_nodelay(true);
    let writing = match reading.try_clone() {
        Ok(w) => w,
        Err(e) => return shared.end(failure(SocketError::Other(0), &e.to_string())),
    };
    if let Ok(mut held) = shared.stream.lock() {
        *held = reading.try_clone().ok();
    }
    // Writes that make no progress for long end the connection: a server
    // that never reads must not hold the writing thread (and a secure
    // session) for ever. The option is the socket's: the clones share it.
    let _ = writing.set_write_timeout(Some(shared.env.stall()));
    match (tls, deadline, slot) {
        (Some(setup), Some(deadline), Some(slot)) => {
            if !shared.secure_up(&mut reading, &setup, deadline, slot) {
                return;
            }
        }
        _ => (shared.report)(SocketEvent::Connected { socket }),
    }
    // Writing on a thread of its own, so a read waiting does not hold
    // sends back.
    {
        let shared = shared.clone();
        thread::spawn(move || write_all(&shared, writing, &write_asks));
    }
    for max in read_asks {
        let mut buffer = vec![0; max.max(1) as usize];
        let read = match shared.secure.get() {
            Some(secure) => secure.read(&mut reading, &mut buffer),
            None => reading.read(&mut buffer),
        };
        match read {
            Ok(n) => {
                buffer.truncate(n);
                shared.tick();
                (shared.report)(SocketEvent::Data {
                    socket,
                    data: buffer,
                });
            }
            Err(e) => {
                shared.shut();
                return shared.end(failure(io_error(&e), &e.to_string()));
            }
        }
    }
}

/// What is asked of the writing thread, in order.
fn write_all(shared: &Arc<Shared>, mut writing: TcpStream, jobs: &Receiver<Job>) {
    let socket = shared.socket;
    let failure = |error, reason: &str| SocketEvent::Failed {
        socket,
        error,
        reason: String::from(reason),
    };
    for job in jobs {
        let sent = match job {
            Job::Send(data) => match shared.secure.get() {
                Some(secure) => secure.write_all(&data),
                None => writing.write_all(&data).and_then(|()| writing.flush()),
            },
            Job::StartTls { tls, deadline } => {
                let refusal = if shared.secure.get().is_some() {
                    Some((SocketError::NotReady, "the socket is secure already"))
                } else if bytes_wait(&writing) {
                    // They might be injected, and would pass as secure.
                    Some((
                        SocketError::NotReady,
                        "the server sent bytes before the switch to TLS",
                    ))
                } else {
                    None
                };
                if let Some((error, reason)) = refusal {
                    shared.shut();
                    return shared.end(failure(error, reason));
                }
                let Some(slot) = shared.env.gate().take(deadline) else {
                    shared.shut();
                    let wait = "no secure session was free in time";
                    return shared.end(failure(SocketError::Busy, wait));
                };
                if !shared.secure_up(&mut writing, &tls, deadline, slot) {
                    return;
                }
                Ok(())
            }
        };
        match sent {
            Ok(()) => shared.tick(),
            Err(e) => {
                // Shut, so that nothing waits on it: a read, the session.
                shared.shut();
                return shared.end(failure(io_error(&e), &e.to_string()));
            }
        }
    }
    // Closed (its sender gone) once everything asked before was sent: the
    // other side reads the end.
    match shared.secure.get() {
        Some(secure) => secure.close_write(),
        None => {
            let _ = writing.shutdown(Shutdown::Write);
        }
    }
}
