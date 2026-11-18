//! The connection an HTTP request goes over: a TCP socket of `net`, plain
//! or with TLS over it (`tls`). Either way it is used alike: moved on
//! with [`Link::pump`], ready for the request once its handshake is done,
//! then plain bytes sent and received.

use alloc::boxed::Box;

use smoltcp::socket::tcp;

use crate::net::{Handle, Net};
use crate::tls;

/// Plain bytes read from TLS at a time.
const READ_PART: usize = 1024;

pub struct Link {
    pub socket: Handle,
    tls: Option<Box<tls::Session>>,
    /// Some bytes came over the connection (see `paperpaw_x4::stall`).
    received: bool,
}

/// Why a connection failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The connection is gone.
    Lost,
    /// Its TLS failed.
    Tls(tls::Error),
}

impl Failure {
    /// The reason, for the plugin that asked.
    pub fn reason(self) -> &'static str {
        match self {
            Failure::Lost => "connection lost",
            Failure::Tls(e) => e.reason(),
        }
    }
}

impl From<tls::Error> for Failure {
    fn from(e: tls::Error) -> Self {
        Failure::Tls(e)
    }
}

impl From<Failure> for &'static str {
    fn from(failure: Failure) -> Self {
        failure.reason()
    }
}

/// What receiving brought.
#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    /// Bytes were passed on (or not, for now).
    Some(bool),
    /// The other side is done: nothing more comes.
    Closed,
}

fn tcp_ended(socket: &tcp::Socket<'_>) -> bool {
    matches!(
        socket.state(),
        tcp::State::CloseWait | tcp::State::Closed | tcp::State::TimeWait
    ) && !socket.can_recv()
}

impl Link {
    /// Over the open socket `socket`, with TLS if given a session (made
    /// first: it takes the most memory).
    pub fn new(socket: Handle, tls: Option<Box<tls::Session>>) -> Self {
        Self {
            socket,
            tls,
            received: false,
        }
    }

    /// The TLS session over the connection, if it has one.
    pub fn session(&self) -> Option<&tls::Session> {
        self.tls.as_deref()
    }

    /// It runs TLS over its socket.
    pub fn secure(&self) -> bool {
        self.tls.is_some()
    }

    /// The other side closed the connection, or it is gone: a connection
    /// kept for a later request is of no use.
    pub fn closed(&self, net: &mut Net) -> bool {
        let socket = net.socket(self.socket);
        !socket.is_active() || tcp_ended(socket)
    }

    /// Whether any byte came over the connection.
    pub fn received(&self) -> bool {
        self.received
    }

    /// Moves the encrypted bytes between the socket and TLS, and TLS's
    /// handshake on; whether anything moved.
    pub fn pump(&mut self, net: &mut Net) -> Result<bool, Failure> {
        let Some(session) = self.tls.as_mut() else {
            return Ok(false);
        };
        let socket = net.socket(self.socket);
        let mut moved = false;
        while session.room() > 0 && socket.can_recv() {
            let got = socket
                .recv(|data| {
                    let n = data.len().min(session.room());
                    session.feed(&data[..n]);
                    (n, n)
                })
                .map_err(|_| Failure::Lost)?;
            moved |= got > 0;
            if got == 0 {
                break;
            }
            self.received = true;
        }
        if tcp_ended(socket) {
            session.ended();
        }
        session.handshake()?;
        moved |= Self::flush(session, socket)?;
        Ok(moved)
    }

    /// Sends what TLS gave to send, as far as the socket takes it.
    fn flush(session: &mut tls::Session, socket: &mut tcp::Socket<'_>) -> Result<bool, Failure> {
        let output = session.output();
        if output.is_empty() || !socket.can_send() {
            return Ok(false);
        }
        let n = socket.send_slice(output).map_err(|_| Failure::Lost)?;
        session.sent(n);
        Ok(n > 0)
    }

    /// Where it is, for the log when it stalls.
    pub fn describe(&self, net: &mut Net) -> alloc::string::String {
        let socket = net.socket(self.socket);
        let state = socket.state();
        let (rx, tx) = (socket.recv_queue(), socket.send_queue());
        let to = socket
            .remote_endpoint()
            .map(|to| alloc::format!("{to}"))
            .unwrap_or_default();
        let received = self.received;
        match &self.tls {
            None => alloc::format!("tcp {state} to {to} rx {rx} tx {tx} received {received}"),
            Some(session) => alloc::format!(
                "tcp {state} to {to} rx {rx} tx {tx} received {received}, tls {}",
                session.describe()
            ),
        }
    }

    /// Ready for the request: connected, and TLS's handshake done.
    pub fn ready(&mut self, net: &mut Net) -> bool {
        let socket = net.socket(self.socket);
        if !socket.may_send() {
            return false;
        }
        match self.tls.as_mut() {
            None => true,
            Some(session) => session.handshake() == Ok(true),
        }
    }

    /// Sends plain bytes; how many were taken (0 when there is no room).
    pub fn send(&mut self, net: &mut Net, data: &[u8]) -> Result<usize, Failure> {
        let socket = net.socket(self.socket);
        match self.tls.as_mut() {
            None if socket.can_send() => socket.send_slice(data).map_err(|_| Failure::Lost),
            None => Ok(0),
            Some(session) => {
                let n = session.write(data)?;
                Self::flush(session, socket)?;
                Ok(n)
            }
        }
    }

    /// Reads plain bytes into `out` from the session (after the
    /// handshake); there is none without one.
    pub fn read(&mut self, out: &mut [u8]) -> Result<tls::Read, Failure> {
        match self.tls.as_mut() {
            Some(session) => Ok(session.read(out)?),
            None => Ok(tls::Read::Wait),
        }
    }

    /// Passes the plain bytes that came on to `take`, a piece at a time,
    /// while `more` says so.
    pub fn receive(
        &mut self,
        net: &mut Net,
        more: &mut dyn FnMut() -> bool,
        take: &mut dyn FnMut(&[u8]),
    ) -> Result<Received, Failure> {
        let socket = net.socket(self.socket);
        let mut moved = false;
        match self.tls.as_mut() {
            None => {
                while more() && socket.can_recv() {
                    socket
                        .recv(|data| {
                            take(data);
                            (data.len(), ())
                        })
                        .map_err(|_| Failure::Lost)?;
                    moved = true;
                    self.received = true;
                }
                if more() && tcp_ended(socket) {
                    return Ok(Received::Closed);
                }
            }
            Some(session) => {
                let mut part = [0u8; READ_PART];
                while more() {
                    match session.read(&mut part)? {
                        tls::Read::Data(n) => {
                            take(&part[..n]);
                            moved = true;
                        }
                        tls::Read::Wait => break,
                        tls::Read::Closed => return Ok(Received::Closed),
                    }
                }
            }
        }
        Ok(Received::Some(moved))
    }
}
