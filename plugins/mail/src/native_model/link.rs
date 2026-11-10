//! The link between a session and its socket: what a session asks of
//! [`Io`] becomes calls on one socket ([`Net`]), and what the socket tells
//! becomes the session's [`Input`]. The calls are behind a small trait so that
//! the link is tested with a socket that is a double; the plugin lends the
//! real one (`native_socket::HostNet`, the SDK's `Sockets`).
//!
//! - What a session sends is queued here in a buffer that wipes itself (a
//!   login line passes through it) and handed to the socket piece by piece as
//!   its queue has room: a full queue (`Busy`) waits for the next `Sent`.
//! - Bytes of a `Received` event are read in pieces and given to the session
//!   one piece at a time (nothing of them is kept here).
//! - A refusal of `start_tls` for bytes the reader still holds (`State`) goes
//!   to the session as it is; it ends the session as unsafe.
use super::mem::{Error, SecretBytes};
use super::session::{Input, Io};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::sockets::{PIECE, Reason, SocketKind, Untrusted};

/// What the link asks of one socket.
pub trait Net {
    /// Opens the connection (TLS from the start where `tls`), the server's key
    /// pinned if given.
    ///
    /// # Errors
    /// What the host says.
    fn connect(
        &mut self,
        host: &str,
        port: u16,
        tls: bool,
        pin: Option<[u8; 32]>,
    ) -> Result<(), Error>;
    /// Switches the plain socket to TLS.
    ///
    /// # Errors
    /// What the host says; `State` while bytes of the plain stream are held.
    fn start_tls(&mut self, pin: Option<[u8; 32]>) -> Result<(), Error>;
    /// Queues one piece (1 to [`PIECE`] bytes).
    ///
    /// # Errors
    /// `Busy` where the queue is full; what the host says.
    fn send(&mut self, piece: &[u8]) -> Result<(), Error>;
    /// Asks for the next bytes, at most `max`.
    ///
    /// # Errors
    /// What the host says.
    fn read_bytes(&mut self, max: u32) -> Result<(), Error>;
    /// Asks for the next `len` bytes to be saved to the file `path`.
    ///
    /// # Errors
    /// What the host says.
    fn read_file(&mut self, path: &str, len: u64) -> Result<(), Error>;
    /// Up to [`PIECE`] bytes of what the last `Received` event brought, from
    /// `offset`, copied to `out`: how many (0 at the end).
    ///
    /// # Errors
    /// What the host says.
    fn data(&mut self, offset: u32, out: &mut [u8]) -> Result<usize, Error>;
    /// The hashes of the certificate and of the key of a `Secured` or
    /// `Untrusted` socket, whether the dates were checked, and why not trusted.
    ///
    /// # Errors
    /// What the host says.
    fn peer(&mut self) -> Result<Peer, Error>;
    /// Closes the socket.
    fn close(&mut self);
}

/// What the host knows of the server's certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    /// SHA-256 of the leaf certificate.
    pub certificate: [u8; 32],
    /// SHA-256 of its public key.
    pub key: [u8; 32],
    /// Whether the dates were checked (the clock is set).
    pub time_checked: bool,
    /// Why it is not trusted (an `Untrusted` socket).
    pub why: Untrusted,
}

/// What a socket event is to the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    /// What happened.
    pub kind: SocketKind,
    /// Why, for a failure.
    pub reason: Reason,
    /// How many bytes came, for `Received`.
    pub len: u32,
}

/// What a link keeps between two callbacks: what waits to be sent and the
/// file being read. The socket is lent for each callback ([`Link`]).
#[derive(Debug, Default)]
pub struct LinkState {
    /// What is to be sent (it may carry a password): wiped when it is gone.
    queue: SecretBytes,
    /// How much of it the socket took.
    sent: usize,
    /// The file a read saves to, until `Saved`.
    file: Option<String>,
}

impl LinkState {
    /// Whether bytes wait to be sent.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.sent < self.queue.as_slice().len()
    }
}

/// A link: the state, lent a socket for one callback.
#[derive(Debug)]
pub struct Link<'a, N: Net> {
    state: &'a mut LinkState,
    net: N,
}

impl<'a, N: Net> Link<'a, N> {
    /// The state, with `net` as its socket for now.
    pub fn new(state: &'a mut LinkState, net: N) -> Self {
        Self { state, net }
    }

    /// The socket.
    pub fn net(&mut self) -> &mut N {
        &mut self.net
    }

    /// Whether bytes wait to be sent.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.state.waiting()
    }

    /// Takes a socket notice in and gives the session what it means: each as
    /// an [`Input`] to `give`. A `Sent` queues more of what waits and tells the
    /// session nothing. `give` returns whether to go on (a finished session
    /// stops the reading of the rest).
    ///
    /// # Errors
    /// What the host says where a piece cannot be read or sent.
    pub fn notice(
        &mut self,
        notice: Notice,
        mut give: impl FnMut(Input<'_>, &mut Self) -> bool,
    ) -> Result<(), Error> {
        match notice.kind {
            SocketKind::Connected => {
                give(Input::Connected, self);
            }
            SocketKind::Secured => {
                let peer = self.net.peer()?;
                give(
                    Input::Secured {
                        certificate: peer.certificate,
                        key: peer.key,
                        time_checked: peer.time_checked,
                    },
                    self,
                );
            }
            SocketKind::Untrusted => {
                let peer = self.net.peer()?;
                give(
                    Input::Untrusted {
                        certificate: peer.certificate,
                        key: peer.key,
                        why: peer.why,
                    },
                    self,
                );
            }
            SocketKind::Received => self.received(notice.len, give)?,
            SocketKind::Saved => {
                let file = self.state.file.take().unwrap_or_default();
                give(Input::Saved(&file), self);
            }
            SocketKind::Sent => self.pump()?,
            SocketKind::Closed => {
                give(Input::Closed, self);
            }
            SocketKind::Failed => {
                give(Input::Failed(notice.reason), self);
            }
            // A kind a newer host added: nothing the session knows.
            _ => {}
        }
        Ok(())
    }

    /// Hands the `len` bytes that came to the session, a piece at a time; none
    /// at all (the other side closed its end) is told as an empty piece.
    fn received(
        &mut self,
        len: u32,
        mut give: impl FnMut(Input<'_>, &mut Self) -> bool,
    ) -> Result<(), Error> {
        if len == 0 {
            give(Input::Bytes(&[]), self);
            return Ok(());
        }
        // On the heap: a piece is 3,840 bytes, more than a frame may be.
        let mut piece = paperpaw_ui_kit::mem::buffer(PIECE)?;
        let result = self.hand_over(len, &mut piece, give);
        // What came may have held a password (a server that echoes): gone.
        super::mem::wipe(&mut piece);
        result
    }

    /// The loop of [`Self::received`]: each piece to `give`, wiped after it.
    fn hand_over(
        &mut self,
        len: u32,
        piece: &mut [u8],
        mut give: impl FnMut(Input<'_>, &mut Self) -> bool,
    ) -> Result<(), Error> {
        let mut offset = 0u32;
        while offset < len {
            let n = self.net.data(offset, piece)?;
            if n == 0 {
                break;
            }
            let got = piece.get(..n).unwrap_or_default();
            let more = give(Input::Bytes(got), self);
            super::mem::wipe(piece);
            offset = offset.saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
            if !more {
                break;
            }
        }
        Ok(())
    }

    /// Queues what waits, a piece at a time, until all is queued or the
    /// socket's queue is full (the next `Sent` goes on).
    fn pump(&mut self) -> Result<(), Error> {
        loop {
            let rest = self
                .state
                .queue
                .as_slice()
                .get(self.state.sent..)
                .unwrap_or_default();
            if rest.is_empty() {
                self.state.queue = SecretBytes::default();
                self.state.sent = 0;
                return Ok(());
            }
            let n = rest.len().min(PIECE);
            let piece = rest.get(..n).unwrap_or_default();
            match self.net.send(piece) {
                Ok(()) => self.state.sent = self.state.sent.saturating_add(n),
                Err(Error::Busy) => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }

    /// Adds `data` to what waits (in a new buffer, the old one wiped as it
    /// goes: a secret never grows in place) and sends what the socket takes.
    fn queue_and_pump(&mut self, data: &[u8]) -> Result<(), Error> {
        let rest = self
            .state
            .queue
            .as_slice()
            .get(self.state.sent..)
            .unwrap_or_default();
        let total = rest.len().checked_add(data.len()).ok_or(Error::NoMemory)?;
        let mut next = SecretBytes::with_capacity(total)?;
        next.push_bytes(rest)?;
        next.push_bytes(data)?;
        self.state.queue = next;
        self.state.sent = 0;
        self.pump()
    }
}

impl<N: Net> Io for Link<'_, N> {
    fn connect_tls(&mut self, host: &str, port: u16, pin: Option<[u8; 32]>) -> Result<(), Error> {
        self.net.connect(host, port, true, pin)
    }

    fn connect_plain(&mut self, host: &str, port: u16) -> Result<(), Error> {
        self.net.connect(host, port, false, None)
    }

    fn start_tls(&mut self, pin: Option<[u8; 32]>) -> Result<(), Error> {
        self.net.start_tls(pin)
    }

    fn send(&mut self, data: &[u8]) -> Result<(), Error> {
        self.queue_and_pump(data)
    }

    fn send_message(&mut self, mut data: Vec<u8>) -> Result<(), Error> {
        let result = self.queue_and_pump(&data);
        // The caller's copy of the message is gone with this call.
        super::mem::wipe(&mut data);
        result
    }

    fn read_bytes(&mut self, max: u32) -> Result<(), Error> {
        self.net.read_bytes(max)
    }

    fn read_file(&mut self, path: &str, len: u64) -> Result<(), Error> {
        let kept = super::mem::copy(path)?;
        self.net.read_file(path, len)?;
        self.state.file = Some(kept);
        Ok(())
    }

    fn close(&mut self) {
        self.net.close();
        self.state.queue = SecretBytes::default();
        self.state.sent = 0;
    }
}
