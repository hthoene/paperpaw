//! A connection to a mail server, as a state machine, for the native plugin:
//! the interface-6 `session` made fallible and bounded. Bytes and socket
//! notices go in (`step`), what to send and what to read next goes out
//! through [`Io`], and what happened comes back as [`Event`]s. The plugin's
//! screen holds the socket and lends it; the tests lend a fake server.
//! Nothing here waits.
//!
//! IMAP and SMTP are done one connection at a time (the X4 holds one TLS
//! session): connect, secure, log in, do the tasks, log out, close.
//!
//! What differs from interface 6, on purpose (`tests/native_session.rs` and
//! `tests/native_model_alloc_refusal.rs`):
//!
//! - a refused allocation ends the session (the socket is closed) and is
//!   told as [`Failure::NoMemory`]; nothing here aborts;
//! - the password is a [`Secret`] and every line that carries it (the final
//!   `LOGIN`, `AUTHENTICATE`, `AUTH PLAIN`, the password of `AUTH LOGIN`) is
//!   built in a buffer that wipes itself and is wiped as soon as [`Io::send`]
//!   returns, which copies what it keeps (and wipes that copy when it is
//!   sent); no `format!` ever holds it;
//! - a `{n}` literal of a message's body over `store::MAX_FETCH` (with a
//!   little over) ends the fetch as [`Failure::Protocol`] (interface 6 did the
//!   same), a literal of headers over 16 KiB is skipped, never kept; and a line
//!   or response past the bounds of `imap` or `smtp` (`Fault::TooLong`) closes
//!   the connection as [`Failure::Protocol`], never going on with a line half
//!   read;
//! - the message of an SMTP send is handed to [`Io::send_message`] whole and
//!   unchanged, as interface 6 did (the compose screen bounds it);
//! - the key of a certificate to trust is a plain `[u8; 32]`, and why it is not
//!   trusted and why a socket failed are the SDK's native [`Untrusted`] and
//!   [`Reason`].
mod imap_session;
mod smtp_session;

pub use imap_session::{Imap, Task};
pub use smtp_session::{Outgoing, Smtp};

use super::fail::Fail;
use super::imap::LineBuffer;
use super::mem::{Error, Secret, copy, push};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::sockets::{Reason, Untrusted};

/// How a socket is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    Implicit,
    /// Plain, then the server agrees to TLS (IMAP 143, SMTP 587).
    StartTls,
}

/// Where and as whom to connect.
#[non_exhaustive]
pub struct Endpoint {
    /// The server's name.
    pub host: String,
    /// Its port.
    pub port: u16,
    /// How the connection is secured.
    pub security: Security,
    /// The user name.
    pub user: String,
    /// The password; wipes itself.
    pub password: Secret,
    /// The SHA-256 of the key the reader trusted for this server (see
    /// [`Event::Untrusted`]).
    pub pin: Option<[u8; 32]>,
}

impl Endpoint {
    /// An endpoint with no pin.
    #[must_use]
    pub fn new(
        host: String,
        port: u16,
        security: Security,
        user: String,
        password: Secret,
    ) -> Self {
        Self {
            host,
            port,
            security,
            user,
            password,
            pin: None,
        }
    }
}

// The password never goes into a log or a test's message.
impl core::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Endpoint")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("user", &self.user)
            .field("password", &"…")
            .field("pin", &self.pin)
            .finish()
    }
}

/// What the session asks of the socket (the plugin turns each into a
/// command). `Error::NoMemory` ends the session as [`Failure::NoMemory`],
/// `Unsupported` as [`Failure::NoTls`], `Busy` as [`Failure::Busy`]; any other
/// refusal is a network failure.
pub trait Io {
    /// Connects with TLS from the start, the server's key pinned if given.
    ///
    /// # Errors
    /// What the host says.
    fn connect_tls(&mut self, host: &str, port: u16, pin: Option<[u8; 32]>) -> Result<(), Error>;
    /// Connects plain.
    ///
    /// # Errors
    /// What the host says.
    fn connect_plain(&mut self, host: &str, port: u16) -> Result<(), Error>;
    /// Switches the plain socket to TLS.
    ///
    /// # Errors
    /// What the host says.
    fn start_tls(&mut self, pin: Option<[u8; 32]>) -> Result<(), Error>;
    /// Queues `data` to send. It may hold a password: the implementation keeps
    /// what it must in a buffer that wipes itself, and the caller wipes its own.
    ///
    /// # Errors
    /// What the host says; `NoMemory` where the copy cannot be made.
    fn send(&mut self, data: &[u8]) -> Result<(), Error>;
    /// Queues a whole message (the bytes DATA takes).
    ///
    /// # Errors
    /// As [`Io::send`].
    fn send_message(&mut self, data: Vec<u8>) -> Result<(), Error>;
    /// Asks for the next bytes, at most `max`.
    ///
    /// # Errors
    /// What the host says.
    fn read_bytes(&mut self, max: u32) -> Result<(), Error>;
    /// Asks for the next `len` bytes to be saved into the file `path` of the
    /// data folder.
    ///
    /// # Errors
    /// What the host says.
    fn read_file(&mut self, path: &str, len: u64) -> Result<(), Error>;
    /// Closes the socket.
    fn close(&mut self);
}

/// What the socket tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Input<'a> {
    /// A plain connection is made.
    Connected,
    /// The TLS handshake is done.
    Secured {
        /// SHA-256 of the leaf certificate.
        certificate: [u8; 32],
        /// SHA-256 of its public key.
        key: [u8; 32],
        /// Whether the dates were checked (the clock is set).
        time_checked: bool,
    },
    /// The certificate is not trusted.
    Untrusted {
        /// SHA-256 of the leaf certificate.
        certificate: [u8; 32],
        /// SHA-256 of its public key.
        key: [u8; 32],
        /// Why.
        why: Untrusted,
    },
    /// Bytes came (none: the other side closed its end).
    Bytes(&'a [u8]),
    /// A file read is complete.
    Saved(&'a str),
    /// The other side closed (or the socket ended).
    Closed,
    /// The socket failed.
    Failed(Reason),
}

/// Where a session is, for a line of words on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stage {
    /// Connecting and securing.
    Connecting,
    /// Logging in.
    LoggingIn,
    /// Reading the folders.
    Folders,
    /// Reading the headers of a folder.
    Headers,
    /// Reading a message.
    Message,
    /// Setting a flag.
    Updating,
    /// Sending a message.
    Sending,
}

/// Why a session ended without finishing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// The network: no Wi-Fi, no such server, refused, timed out, lost.
    Network(Reason),
    /// The server's certificate or the secure connection (not trusted ones
    /// are [`Event::Untrusted`]).
    Secure(Reason),
    /// The device has no TLS (a shell without it).
    NoTls,
    /// The device cannot say whether the certificate is valid now: its
    /// clock is not set. The password was not sent.
    ClockNotSet,
    /// Another secure connection holds the one there is.
    Busy,
    /// Not enough memory, for the secure connection or for the session.
    NoMemory,
    /// The server refused the user name or password.
    Login(String),
    /// The server refused something else; its words.
    Refused(String),
    /// A recipient the server does not take (SMTP), and its words.
    Recipient(String, String),
    /// The server wants it later (SMTP 4xx).
    Temporary(String),
    /// The server's answer made no sense, or was too large.
    Protocol,
    /// The server does not offer a way to log in that is safe here, or no
    /// STARTTLS.
    Unsafe,
    /// The card could not be written.
    Storage,
}

/// What a session tells.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// The session moved on.
    Stage(Stage),
    /// Everything was done.
    Done,
    /// It ended without finishing.
    Failed(Failure),
    /// The server's certificate is not trusted: its hashes, for the reader to
    /// compare and, if they agree, to keep as the pin.
    Untrusted {
        /// SHA-256 of the leaf certificate.
        certificate: [u8; 32],
        /// SHA-256 of its public key.
        key: [u8; 32],
        /// Why.
        why: Untrusted,
    },
}

/// How a failed socket reads to the reader.
fn failure_of(reason: Reason) -> Failure {
    match reason {
        Reason::Busy => Failure::Busy,
        Reason::NoMemory => Failure::NoMemory,
        Reason::Tls | Reason::Name | Reason::Expired => Failure::Secure(reason),
        other => Failure::Network(other),
    }
}

/// The one event `event` as a list.
fn one(event: Event) -> Result<Vec<Event>, Fail> {
    let mut events = Vec::new();
    push(&mut events, event)?;
    Ok(events)
}

/// Bytes in, whole lines out, and whether a read is under way.
#[derive(Debug, Default)]
struct Wire {
    buf: LineBuffer,
    reading: bool,
    /// The socket is being secured (after STARTTLS): nothing is sent or read
    /// until the kernel says `Secured`; a read now makes it close the socket.
    switching: bool,
}

impl Wire {
    /// Asks for more bytes, unless some are asked for already, or the socket
    /// is being secured.
    fn more(&mut self, io: &mut dyn Io) -> Result<(), Error> {
        if !self.reading && !self.switching {
            self.reading = true;
            io.read_bytes(1024)?;
        }
        Ok(())
    }
}

/// What an [`Io`] call that failed means to the session: `None` where the
/// session cannot go on for want of memory.
fn io_failure(error: Error) -> Option<Failure> {
    match error {
        Error::NoMemory => None,
        Error::Unsupported => Some(Failure::NoTls),
        Error::Busy => Some(Failure::Busy),
        _ => Some(Failure::Network(Reason::Io)),
    }
}

/// `text` as an owned string, for a failure.
fn words(text: &str) -> Result<String, Fail> {
    Ok(copy(text)?)
}
