//! The link's socket as the host has it: [`Net`] over the SDK's `Sockets`.
//! One socket at a time (a mail job holds one TLS session); the host and the
//! service handle are lent for one callback and the socket's id is kept by
//! the caller between callbacks.
//!
//! The call buffers (4 KiB for the rows, 4 KiB for the reply) are made once for
//! the callback and wiped with it: the rows of a `connect` carry the pins, and a
//! login line never passes through them (it goes to `send` as bytes the host
//! copies and the link wipes).
use crate::native_model::link::{Net, Peer};
use crate::native_model::mem::Error;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Host,
    sockets::{Connect, INPUT, Pin, REPLY, SocketId, Sockets, Tls},
};
use paperpaw_ui_kit::mem::{buffer, wipe};

/// How long lookup, connect and handshake may take: as long as interface 6
/// waited for a server that does not answer.
const CONNECT_MS: u32 = 45_000;
/// How long a connection may be silent: a message streams into a file (up to
/// 256 KiB, the socket says nothing until it is saved) at 4 KiB a second.
const IDLE_MS: u32 = 70_000;

/// The socket of the callback.
pub(crate) struct HostNet<'h> {
    host: &'h Host<'h>,
    sockets: &'h Sockets,
    /// The socket's id, kept by the caller from one callback to the next.
    id: &'h mut Option<SocketId>,
    /// The token the socket's events carry.
    token: u32,
    /// The server's name, for TLS (`connect` keeps it for `start_tls`).
    name: &'h str,
    input: Vec<u8>,
    reply: Vec<u8>,
}

impl<'h> HostNet<'h> {
    /// The socket of this callback.
    ///
    /// # Errors
    /// [`Error::NoMemory`] for the call buffers.
    pub(crate) fn new(
        host: &'h Host<'h>,
        sockets: &'h Sockets,
        id: &'h mut Option<SocketId>,
        token: u32,
        name: &'h str,
    ) -> Result<Self, Error> {
        Ok(Self {
            host,
            sockets,
            id,
            token,
            name,
            input: buffer(INPUT)?,
            reply: buffer(REPLY)?,
        })
    }

    fn id(&self) -> Result<SocketId, Error> {
        (*self.id).ok_or(Error::NotFound)
    }
}

impl Drop for HostNet<'_> {
    fn drop(&mut self) {
        wipe(&mut self.input);
        wipe(&mut self.reply);
    }
}

impl Net for HostNet<'_> {
    fn connect(
        &mut self,
        host: &str,
        port: u16,
        tls: bool,
        pin: Option<[u8; 32]>,
    ) -> Result<(), Error> {
        let pins = pin.map(Pin::public_key);
        let mut connect = Connect::new(self.token, host, port)?
            .timeout_ms(CONNECT_MS)?
            .idle_ms(IDLE_MS)?;
        if tls {
            connect = connect.tls();
        }
        if let Some(pin) = &pins {
            connect = connect.pins(core::slice::from_ref(pin))?;
        }
        let id = self
            .sockets
            .connect(self.host, &connect, &mut self.input, &mut self.reply)?;
        *self.id = Some(id);
        Ok(())
    }

    fn start_tls(&mut self, pin: Option<[u8; 32]>) -> Result<(), Error> {
        let id = self.id()?;
        let pins = pin.map(Pin::public_key);
        let mut tls = Tls::new()
            .server_name(self.name)?
            .timeout_ms(CONNECT_MS)?
            .idle_ms(IDLE_MS)?;
        if let Some(pin) = &pins {
            tls = tls.pins(core::slice::from_ref(pin))?;
        }
        // The link reads through `data` and holds no bytes of its own: the
        // session refuses anything that came before the switch.
        let held = paperpaw_sdk::native::sockets::Reader::new(id, &mut []);
        self.sockets
            .start_tls(self.host, id, &tls, &held, &mut self.input, &mut self.reply)
    }

    fn send(&mut self, piece: &[u8]) -> Result<(), Error> {
        let id = self.id()?;
        let sent = self
            .sockets
            .send(self.host, id, piece, &mut self.input, &mut self.reply);
        // The input holds what was sent (a LOGIN line has the password): gone
        // now, not at the end of the callback.
        wipe(&mut self.input);
        sent
    }

    fn read_bytes(&mut self, max: u32) -> Result<(), Error> {
        let id = self.id()?;
        self.sockets
            .read_bytes(self.host, id, max, false, &mut self.input, &mut self.reply)
    }

    fn read_file(&mut self, path: &str, len: u64) -> Result<(), Error> {
        let id = self.id()?;
        let file = paperpaw_sdk::native::sockets::FileRead::new(
            paperpaw_sdk::native::sockets::Root::Data,
            path,
            len,
        )?;
        self.sockets
            .read_file(self.host, id, &file, &mut self.input, &mut self.reply)
    }

    fn data(&mut self, offset: u32, out: &mut [u8]) -> Result<usize, Error> {
        let id = self.id()?;
        let piece = self
            .sockets
            .data(self.host, id, offset, &mut self.input, &mut self.reply)?;
        let n = piece.len().min(out.len());
        out.get_mut(..n)
            .ok_or(Error::NoSpace)?
            .copy_from_slice(piece.get(..n).ok_or(Error::NoSpace)?);
        Ok(n)
    }

    fn peer(&mut self) -> Result<Peer, Error> {
        let id = self.id()?;
        let peer = self
            .sockets
            .peer(self.host, id, &mut self.input, &mut self.reply)?;
        Ok(Peer {
            certificate: peer.certificate()?,
            key: peer.key()?,
            time_checked: peer.time_checked(),
            why: peer.untrusted(),
        })
    }

    fn close(&mut self) {
        if let Some(id) = self.id.take() {
            // A socket that is gone already is fine; nothing else to do here.
            self.sockets
                .close(self.host, id, &mut self.input, &mut self.reply)
                .ok();
        }
    }
}
