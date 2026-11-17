//! Plugins' sockets for shells with the standard library (see
//! `paperpaw_core::SocketCall`): each TCP connection has a thread that
//! connects and reads what the kernel asks for, and one that writes;
//! broadcasts have a thread that sends and collects the answers. Reads
//! happen only when asked, one at a time per socket, so what waits for
//! the kernel is at most one part per socket.
//!
//! TLS is rustls with ring ([`crate::tls`], owner decision 52), for the
//! shells that say they hold sessions (`KernelBuilder::tls_sessions`,
//! [`crate::TLS_SESSIONS`]); the browser's module holds none.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Shutdown, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use paperpaw_core::{SocketCall, SocketEvent};

use crate::link::{self, Job, Link};
use crate::tls::TlsContext;

/// The largest answer to a broadcast read.
const DATAGRAM: usize = 1500;

type Wake = Arc<dyn Fn() + Send + Sync>;

/// An open connection, seen from the shell's side.
struct Connection {
    link: Link,
    /// Set once the kernel closed it: nothing is reported after.
    closed: Arc<AtomicBool>,
}

/// The sockets and broadcasts of all plugins.
pub struct Sockets {
    events: Sender<SocketEvent>,
    receiver: Receiver<SocketEvent>,
    open: HashMap<u32, Connection>,
    broadcasts: usize,
    /// Where broadcasts go; the local network's broadcast address.
    broadcast_to: Ipv4Addr,
    wake: Wake,
    /// What TLS depends on besides the server (tests set it).
    pub(crate) tls: TlsContext,
}

impl std::fmt::Debug for Sockets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sockets")
            .field("open", &self.open.len())
            .finish_non_exhaustive()
    }
}

impl Sockets {
    /// `wake` is called when there is something to pass to the kernel,
    /// e.g. to wake an event loop.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (events, receiver) = channel();
        Self {
            events,
            receiver,
            open: HashMap::new(),
            broadcasts: 0,
            broadcast_to: Ipv4Addr::BROADCAST,
            wake: Arc::new(wake),
            tls: TlsContext::system(),
        }
    }

    /// Does what the kernel asks.
    pub fn apply(&mut self, call: SocketCall) {
        match call {
            SocketCall::Connect {
                socket,
                host,
                port,
                tls,
            } => self.connect(socket, host, port, tls),
            SocketCall::StartTls { socket, tls } => {
                let deadline = Instant::now() + Duration::from_millis(u64::from(tls.timeout_ms));
                if let Some(c) = self.open.get(&socket) {
                    let _ = c.link.writes.send(Job::StartTls { tls, deadline });
                }
            }
            SocketCall::Send { socket, data } => {
                if let Some(c) = self.open.get(&socket) {
                    let _ = c.link.writes.send(Job::Send(data));
                }
            }
            SocketCall::Read { socket, max } => {
                if let Some(c) = self.open.get(&socket) {
                    let _ = c.link.reads.send(max);
                }
            }
            SocketCall::Close { socket } => {
                // Its senders go with it: the writing thread sends what
                // was asked before, then ends the stream (see `link`).
                if let Some(c) = self.open.remove(&socket) {
                    c.closed.store(true, Ordering::Relaxed);
                    if let Ok(stream) = c.link.stream.lock()
                        && let Some(stream) = stream.as_ref()
                    {
                        let _ = stream.shutdown(Shutdown::Read);
                    }
                }
            }
            SocketCall::Broadcast {
                broadcast,
                ports,
                data,
                wait_ms,
            } => self.broadcast(broadcast, ports, data, wait_ms),
        }
    }

    /// An event to pass to the kernel, if any.
    pub fn take_event(&mut self) -> Option<SocketEvent> {
        let event = self.receiver.try_recv().ok()?;
        match &event {
            // The connection is over, and shut.
            SocketEvent::Failed { socket, .. } | SocketEvent::Untrusted { socket, .. } => {
                self.open.remove(socket);
            }
            SocketEvent::Done { .. } => self.broadcasts = self.broadcasts.saturating_sub(1),
            _ => {}
        }
        Some(event)
    }

    /// Whether a socket or broadcast is open, e.g. to keep polling.
    pub fn any(&self) -> bool {
        !self.open.is_empty() || self.broadcasts > 0
    }

    fn connect(
        &mut self,
        socket: u32,
        host: String,
        port: u16,
        tls: Option<paperpaw_core::TlsSetup>,
    ) {
        let closed = Arc::new(AtomicBool::new(false));
        let events = self.events.clone();
        let wake = self.wake.clone();
        let report: link::Report = {
            let closed = closed.clone();
            Arc::new(move |event: SocketEvent| {
                if !closed.load(Ordering::Relaxed) {
                    let _ = events.send(event);
                    wake();
                }
            })
        };
        let link = link::start(socket, host, port, tls, self.tls.clone(), report);
        self.open.insert(socket, Connection { link, closed });
    }

    fn broadcast(&mut self, broadcast: u32, ports: Vec<u16>, data: Vec<u8>, wait_ms: u32) {
        self.broadcasts += 1;
        let events = self.events.clone();
        let wake = self.wake.clone();
        let to = self.broadcast_to;
        thread::spawn(move || {
            let report = |event| {
                let _ = events.send(event);
                wake();
            };
            // Sent from the loopback address where it goes there (tests),
            // so that no firewall is asked for the network.
            let from = if to.is_loopback() {
                Ipv4Addr::LOCALHOST
            } else {
                Ipv4Addr::UNSPECIFIED
            };
            if let Ok(udp) = UdpSocket::bind((from, 0))
                && udp.set_broadcast(true).is_ok()
            {
                for port in ports {
                    let _ = udp.send_to(&data, (to, port));
                }
                let until = Instant::now() + Duration::from_millis(u64::from(wait_ms));
                let mut buffer = [0u8; DATAGRAM];
                while let Some(left) = until.checked_duration_since(Instant::now()) {
                    if left.is_zero() || udp.set_read_timeout(Some(left)).is_err() {
                        break;
                    }
                    match udp.recv_from(&mut buffer) {
                        Ok((n, from)) => report(SocketEvent::Answer {
                            broadcast,
                            from: from.ip().to_string(),
                            data: buffer[..n].to_vec(),
                        }),
                        Err(_) => break,
                    }
                }
            }
            report(SocketEvent::Done { broadcast });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_core::sdk::SocketError;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// The next event, waiting up to two seconds.
    fn wait(sockets: &mut Sockets) -> SocketEvent {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(event) = sockets.take_event() {
                return event;
            }
            assert!(Instant::now() < until, "no event");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn connects_reads_when_asked_and_sends() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer.write_all(b"12[9,{}]").unwrap();
            let mut got = [0u8; 5];
            peer.read_exact(&mut got).unwrap();
            got
        });
        let mut sockets = Sockets::new(|| {});
        sockets.apply(SocketCall::Connect {
            socket: 1,
            host: String::from("127.0.0.1"),
            port,
            tls: None,
        });
        assert_eq!(wait(&mut sockets), SocketEvent::Connected { socket: 1 });
        assert!(sockets.any());
        sockets.apply(SocketCall::Read { socket: 1, max: 64 });
        let SocketEvent::Data { socket: 1, data } = wait(&mut sockets) else {
            panic!("no data");
        };
        assert!(b"12[9,{}]".starts_with(&data) && !data.is_empty());
        sockets.apply(SocketCall::Send {
            socket: 1,
            data: b"hello".to_vec(),
        });
        assert_eq!(&server.join().unwrap(), b"hello");
        sockets.apply(SocketCall::Close { socket: 1 });
        assert!(!sockets.any());
    }

    #[test]
    fn a_closed_peer_reads_as_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || drop(listener.accept().unwrap()));
        let mut sockets = Sockets::new(|| {});
        sockets.apply(SocketCall::Connect {
            socket: 2,
            host: String::from("127.0.0.1"),
            port,
            tls: None,
        });
        assert_eq!(wait(&mut sockets), SocketEvent::Connected { socket: 2 });
        server.join().unwrap();
        sockets.apply(SocketCall::Read { socket: 2, max: 8 });
        match wait(&mut sockets) {
            SocketEvent::Data { data, .. } => assert!(data.is_empty()),
            SocketEvent::Failed { .. } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unreachable_hosts_fail() {
        let mut sockets = Sockets::new(|| {});
        sockets.apply(SocketCall::Connect {
            socket: 3,
            host: String::from("no-such-host.invalid"),
            port: 9,
            tls: None,
        });
        assert!(matches!(
            wait(&mut sockets),
            SocketEvent::Failed {
                socket: 3,
                error: SocketError::Dns,
                ..
            }
        ));
        assert!(!sockets.any());
        // A port no one listens on refuses.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        sockets.apply(SocketCall::Connect {
            socket: 4,
            host: String::from("127.0.0.1"),
            port,
            tls: None,
        });
        assert!(matches!(
            wait(&mut sockets),
            SocketEvent::Failed {
                socket: 4,
                error: SocketError::Refused | SocketError::Timeout,
                ..
            }
        ));
    }

    #[test]
    fn broadcasts_collect_answers() {
        let service = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = service.local_addr().unwrap().port();
        let answering = thread::spawn(move || {
            let mut buffer = [0u8; 64];
            let (n, from) = service.recv_from(&mut buffer).unwrap();
            assert_eq!(&buffer[..n], b"hello");
            service
                .send_to(b"calibre wireless device client (on pc);8080,9090", from)
                .unwrap();
        });
        let mut sockets = Sockets::new(|| {});
        sockets.broadcast_to = Ipv4Addr::LOCALHOST;
        sockets.apply(SocketCall::Broadcast {
            broadcast: 1,
            ports: vec![port],
            data: b"hello".to_vec(),
            wait_ms: 1500,
        });
        let SocketEvent::Answer { from, data, .. } = wait(&mut sockets) else {
            panic!("no answer");
        };
        assert_eq!(from, "127.0.0.1");
        assert!(data.ends_with(b";8080,9090"));
        answering.join().unwrap();
        // Done once the time is up.
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(SocketEvent::Done { broadcast: 1 }) = sockets.take_event() {
                break;
            }
            assert!(Instant::now() < until, "not done");
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!sockets.any());
    }
}
