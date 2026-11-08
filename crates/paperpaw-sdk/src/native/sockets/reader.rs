//! What a plugin keeps between events: bytes read beyond what it used
//! ([`Reader`]) and bytes it has yet to send ([`Outbox`]). Both borrow the
//! caller's memory and allocate nothing.
use super::{Error, Host, PIECE, SocketId, Sockets};
use core::fmt;

/// The bytes a socket brought that the plugin has not used yet.
///
/// A read brings what has come (mail reads 1,024 bytes at a time), often more
/// than one protocol line or less than one; the reader keeps the surplus in the
/// buffer it was given. [`Sockets::start_tls`] refuses while a reader holds
/// bytes, so nothing read before the switch is taken as coming over TLS.
///
/// Debug shows lengths only: the bytes may carry a password.
#[must_use = "a reader holds bytes of its socket"]
pub struct Reader<'b> {
    id: SocketId,
    buf: &'b mut [u8],
    start: usize,
    end: usize,
}

impl fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader")
            .field("id", &self.id)
            .field("held", &self.held())
            .field("capacity", &self.buf.len())
            .finish()
    }
}

impl<'b> Reader<'b> {
    /// A reader of socket `id` keeping its surplus in `buf`. An empty buffer
    /// keeps nothing (a plugin that uses [`Sockets::data`] directly passes it
    /// to [`Sockets::start_tls`]).
    pub const fn new(id: SocketId, buf: &'b mut [u8]) -> Self {
        Self {
            id,
            buf,
            start: 0,
            end: 0,
        }
    }

    /// The socket.
    #[must_use]
    pub const fn id(&self) -> SocketId {
        self.id
    }

    /// How many bytes are held, not used yet.
    #[must_use]
    pub const fn held(&self) -> usize {
        self.end - self.start
    }

    /// The bytes held.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.buf[self.start..self.end]
    }

    /// Uses up to `n` bytes (the first ones); how many were there.
    pub fn consume(&mut self, n: usize) -> usize {
        let n = n.min(self.held());
        self.start += n;
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        }
        n
    }

    /// Forgets every byte held, and wipes them.
    pub fn clear(&mut self) {
        self.buf.fill(0);
        self.start = 0;
        self.end = 0;
    }

    fn compact(&mut self) {
        if self.start != 0 {
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
    }

    /// Takes the `len` bytes a `Received` event brought (its
    /// [`SocketEvent::len`](super::SocketEvent::len)) into the buffer, after what
    /// is held; call it during that event's callback. 0 takes nothing (the peer
    /// closed its side). The bytes are gone when the callback returns, so room
    /// is checked first and nothing is taken when it is short.
    ///
    /// `input` and `out` are the call buffers ([`REPLY`](super::REPLY) bytes for `out`).
    ///
    /// # Errors
    /// `NoSpace` when the buffer cannot hold what is held and `len` bytes (use
    /// some with [`Reader::consume`] or [`Reader::next_line`] and call again in
    /// the same callback); the host's refusals, with the bytes taken so far kept;
    /// `Invalid` for a host that sends fewer bytes than the event said, with
    /// those kept.
    pub fn fill(
        &mut self,
        sockets: &Sockets,
        host: &Host<'_>,
        len: u32,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let len = usize::try_from(len).map_err(|_| Error::NoSpace)?;
        if len == 0 {
            return Ok(0);
        }
        if self.held() + len > self.buf.len() {
            return Err(Error::NoSpace);
        }
        self.compact();
        let mut got = 0;
        while got < len {
            let offset = u32::try_from(got).map_err(|_| Error::NoSpace)?;
            let part = sockets.data(host, self.id, offset, input, out)?;
            if part.is_empty() {
                return Err(Error::Invalid);
            }
            let take = part.len().min(len - got);
            self.buf[self.end..self.end + take].copy_from_slice(&part[..take]);
            self.end += take;
            got += take;
        }
        Ok(got)
    }

    /// Uses the next line held, without its `\r\n` or `\n`, as text.
    /// `None` while no whole line is held.
    ///
    /// # Errors
    /// `Utf8` for a line that is not UTF-8 (the line is used up all the same);
    /// `NoSpace` when no line end is held and the buffer is full: the line is
    /// longer than the buffer (close the socket, or [`Reader::clear`]).
    pub fn next_line(&mut self) -> Result<Option<&str>, Error> {
        let held = &self.buf[self.start..self.end];
        let Some(at) = held.iter().position(|&b| b == b'\n') else {
            if self.start == 0 && self.end == self.buf.len() {
                return Err(Error::NoSpace);
            }
            return Ok(None);
        };
        let from = self.start;
        let line_end = if at > 0 && held[at - 1] == b'\r' {
            from + at - 1
        } else {
            from + at
        };
        self.start = from + at + 1;
        if self.start == self.end {
            // Nothing else is held: the next fill starts at the front. The
            // line's bytes stay where they are until then.
            self.start = 0;
            self.end = 0;
        }
        core::str::from_utf8(&self.buf[from..line_end])
            .map(Some)
            .map_err(|_| Error::Utf8)
    }
}

/// Where an [`Outbox`] stands after [`Outbox::pump`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Progress {
    /// Everything is queued with the host (the `Sent` event says it left).
    Queued,
    /// The queue is full: call [`Outbox::pump`] again on the `Sent` event.
    Waiting,
}

/// Bytes to send, queued piece by piece as the host's send queue has room
/// (4 KiB on the X4), the unsent rest kept here: no allocation, the caller
/// owns the whole message.
///
/// Debug shows lengths only.
#[must_use = "pump the outbox until it reports Queued"]
pub struct Outbox<'a> {
    data: &'a [u8],
    queued: usize,
}

impl fmt::Debug for Outbox<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Outbox")
            .field("len", &self.data.len())
            .field("queued", &self.queued)
            .finish()
    }
}

impl<'a> Outbox<'a> {
    /// An outbox of `data`, none of it queued yet.
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, queued: 0 }
    }

    /// Bytes not queued yet.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.data.len() - self.queued
    }

    /// Whether every byte is queued.
    #[must_use]
    pub const fn is_queued(&self) -> bool {
        self.queued == self.data.len()
    }

    /// Queues pieces of up to 3,840 bytes until all are queued or the host's
    /// queue is full (`Busy`: nothing of that piece is queued).
    ///
    /// # Errors
    /// The host's refusals other than `Busy` (`State` while the socket
    /// connects, `NotFound` for an ended one); what was queued stays counted.
    pub fn pump(
        &mut self,
        sockets: &Sockets,
        host: &Host<'_>,
        id: SocketId,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Progress, Error> {
        while self.queued < self.data.len() {
            let end = (self.queued + PIECE).min(self.data.len());
            match sockets.send(host, id, &self.data[self.queued..end], input, out) {
                Ok(()) => self.queued = end,
                Err(Error::Busy) => return Ok(Progress::Waiting),
                Err(other) => return Err(other),
            }
        }
        Ok(Progress::Queued)
    }
}
