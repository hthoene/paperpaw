//! TCP and TLS sockets and UDP broadcast from `core.sockets` major 1
//! (`docs/abi/sockets.md`): typed calls, the events they come back as, and a
//! reader and an outbox that keep what the plugin has not used yet.
//!
//! A plugin subscribes to [`EventKind::Socket`](super::declare::EventKind) (and
//! `Broadcast` for broadcasts) first; a `connect` without that is refused as
//! `State`. [`Sockets::connect`] answers a [`SocketId`] and the outcome comes as
//! [`SocketEvent`]s carrying the plugin's token: `Connected` (or `Secured`,
//! `Untrusted`), then `Received` for each read asked, `Sent` when the send queue
//! drained, and exactly one `Closed` or `Failed` at the end.
//!
//! Nothing arrives unasked: [`Sockets::read_bytes`] asks for the next data, and
//! during the `Received` event's callback [`Sockets::data`] (or
//! [`Reader::fill`]) takes it; the host frees it when the callback returns.
//!
//! Every call checks what the contract checks before the host call, takes the
//! caller's buffers and allocates nothing. Socket data never reaches a `Debug`
//! output: a protocol exchange may carry a password.
//!
//! ```
//! use paperpaw_sdk::native::sockets::{Connect, Pin};
//! // Mail over TLS, the server's key pinned after the user accepted it.
//! let pins = [Pin::public_key([7; 32])];
//! let connect = Connect::new(1, "imap.example.org", 993)
//!     .unwrap()
//!     .tls()
//!     .pins(&pins)
//!     .unwrap();
//! assert!(connect.check().is_ok());
//! ```
use super::wire::services_wire::core_sockets_v1 as schema;
use super::{
    Error, Host,
    batch::{self, AsWire},
    declare::records_wire::{self as wire, RawRecord, WireEncode},
    services::Service,
};
use core::fmt;

mod reader;

pub use reader::{Outbox, Progress, Reader};

/// The input a call needs at most.
pub const INPUT: usize = 4096;
/// The reply buffer [`Sockets::data`] needs.
pub const REPLY: usize = 4096;
/// The reply buffer every other call needs.
pub const SMALL_REPLY: usize = 256;
/// Bytes one [`Sockets::send`] queues at most, and one [`Sockets::data`]
/// returns at most.
pub const PIECE: usize = 3840;
/// The most pins of a connection.
pub const MAX_PINS: usize = 4;
/// The most bytes of a `bytes` read (the host reads at most its own cap, 4,096
/// on the X4).
pub const MAX_READ: u32 = 16_384;
/// The longest file read, in bytes.
pub const MAX_FILE_LEN: u64 = 1 << 40;
/// The longest path of a file read.
pub const MAX_PATH: usize = 512;
/// The longest connect or TLS time, in milliseconds.
pub const TIMEOUT_MAX_MS: u32 = 60_000;
/// The longest idle time, in milliseconds.
pub const IDLE_MAX_MS: u32 = 1_800_000;
/// The longest broadcast payload.
pub const BROADCAST_MAX_BYTES: usize = 512;
/// The most ports of a broadcast.
pub const BROADCAST_MAX_PORTS: usize = 8;

const FLAG_TLS: u32 = 1;
const FLAG_EXACT: u32 = 1;

/// The host's number of a socket (or a broadcast), never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketId(u32);

impl SocketId {
    /// The number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The id an event carries.
    ///
    /// # Errors
    /// `Invalid` for 0.
    pub const fn new(id: u32) -> Result<Self, Error> {
        if id == 0 {
            Err(Error::Invalid)
        } else {
            Ok(Self(id))
        }
    }
}

/// Whether `name` is a host name of letters, digits, `-` and `.` (labels not
/// empty), as the host checks it.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name
            .split('.')
            .all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

/// A host to connect to: a name, an IPv4 address or a bracketed IPv6 address.
fn is_host(host: &str) -> bool {
    if host.len() > 253 {
        return false;
    }
    if let Some(inner) = host.strip_prefix('[') {
        return inner.strip_suffix(']').is_some_and(|v6| {
            !v6.is_empty()
                && v6
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
        });
    }
    is_name(host)
}

/// What a pin hashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PinKind {
    /// The SHA-256 of the leaf certificate's DER.
    Certificate,
    /// The SHA-256 of the leaf certificate's SubjectPublicKeyInfo DER.
    PublicKey,
}

/// One pinned certificate or key. With pins only the leaf must match one of
/// them; the chain and the name are not checked, the dates are
/// (`docs/abi/sockets.md` 2.4). Hashes are public: a `Debug` shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pin {
    kind: PinKind,
    hash: [u8; 32],
}

impl Pin {
    /// The SHA-256 of a leaf certificate's DER.
    #[must_use]
    pub const fn certificate(hash: [u8; 32]) -> Self {
        Self {
            kind: PinKind::Certificate,
            hash,
        }
    }

    /// The SHA-256 of a leaf certificate's SubjectPublicKeyInfo DER.
    #[must_use]
    pub const fn public_key(hash: [u8; 32]) -> Self {
        Self {
            kind: PinKind::PublicKey,
            hash,
        }
    }

    /// What it hashes.
    #[must_use]
    pub const fn kind(&self) -> PinKind {
        self.kind
    }

    /// The hash.
    #[must_use]
    pub const fn hash(&self) -> &[u8; 32] {
        &self.hash
    }

    const fn wire_kind(&self) -> u32 {
        match self.kind {
            PinKind::Certificate => 0,
            PinKind::PublicKey => 1,
        }
    }
}

fn check_ms(ms: u32, most: u32) -> Result<u32, Error> {
    if ms > most {
        Err(Error::Invalid)
    } else {
        Ok(ms)
    }
}

fn check_name(name: &str) -> Result<(), Error> {
    if name.is_empty() || is_name(name) {
        Ok(())
    } else {
        Err(Error::Invalid)
    }
}

/// A connection to make, built from borrowed parts.
#[derive(Clone, Copy)]
#[must_use = "connect with `Sockets::connect`"]
pub struct Connect<'a> {
    token: u32,
    host: &'a str,
    port: u16,
    tls: bool,
    server_name: &'a str,
    timeout_ms: u32,
    idle_ms: u32,
    pins: &'a [Pin],
}

impl fmt::Debug for Connect<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connect")
            .field("token", &self.token)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("pins", &self.pins.len())
            .finish_non_exhaustive()
    }
}

impl<'a> Connect<'a> {
    /// A plain connection to `host`:`port`, its events carrying `token` (the
    /// plugin's own number; several sockets may share one).
    ///
    /// # Errors
    /// `Invalid` for a host that is no name, IPv4 or bracketed IPv6 address of at
    /// most 253 bytes, or port 0.
    pub fn new(token: u32, host: &'a str, port: u16) -> Result<Self, Error> {
        if port == 0 || !is_host(host) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            token,
            host,
            port,
            tls: false,
            server_name: "",
            timeout_ms: 0,
            idle_ms: 0,
            pins: &[],
        })
    }

    /// TLS from the start (the connection ends in `Secured`, `Untrusted` or a
    /// failure, never `Connected`).
    pub const fn tls(mut self) -> Self {
        self.tls = true;
        self
    }

    /// The name the certificate is checked against, when it is not the host's
    /// (TLS only).
    ///
    /// # Errors
    /// `Invalid` for a name that is not a host name.
    pub fn server_name(mut self, name: &'a str) -> Result<Self, Error> {
        check_name(name)?;
        self.server_name = name;
        Ok(self)
    }

    /// Lookup, connect and handshake together may take this long: up to 60,000
    /// ms, 0 for 20,000.
    ///
    /// # Errors
    /// `Invalid` for more.
    pub fn timeout_ms(mut self, ms: u32) -> Result<Self, Error> {
        self.timeout_ms = check_ms(ms, TIMEOUT_MAX_MS)?;
        Ok(self)
    }

    /// No byte either way for this long ends the socket with `timeout`: up to
    /// 1,800,000 ms, 0 for 60,000.
    ///
    /// # Errors
    /// `Invalid` for more.
    pub fn idle_ms(mut self, ms: u32) -> Result<Self, Error> {
        self.idle_ms = check_ms(ms, IDLE_MAX_MS)?;
        Ok(self)
    }

    /// Pins the leaf certificate or key: at most [`MAX_PINS`], TLS only.
    ///
    /// # Errors
    /// `Invalid` for more.
    pub fn pins(mut self, pins: &'a [Pin]) -> Result<Self, Error> {
        if pins.len() > MAX_PINS {
            return Err(Error::Invalid);
        }
        self.pins = pins;
        Ok(self)
    }

    /// What the host checks before it opens anything: pins and a server name
    /// belong to TLS.
    ///
    /// # Errors
    /// `Invalid` for pins or a server name without [`Connect::tls`].
    pub fn check(&self) -> Result<(), Error> {
        if !self.tls && (!self.pins.is_empty() || !self.server_name.is_empty()) {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

/// A switch to TLS on an open plain socket (STARTTLS), built from borrowed parts.
#[derive(Clone, Copy)]
#[must_use = "switch with `Sockets::start_tls`"]
pub struct Tls<'a> {
    server_name: &'a str,
    timeout_ms: u32,
    idle_ms: u32,
    pins: &'a [Pin],
}

impl fmt::Debug for Tls<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tls")
            .field("server_name", &self.server_name)
            .field("pins", &self.pins.len())
            .finish_non_exhaustive()
    }
}

impl<'a> Tls<'a> {
    /// A switch checking the certificate against the name the socket
    /// connected to.
    pub const fn new() -> Self {
        Self {
            server_name: "",
            timeout_ms: 0,
            idle_ms: 0,
            pins: &[],
        }
    }

    /// The name the certificate is checked against, when it is not the host's.
    ///
    /// # Errors
    /// `Invalid` for a name that is not a host name.
    pub fn server_name(mut self, name: &'a str) -> Result<Self, Error> {
        check_name(name)?;
        self.server_name = name;
        Ok(self)
    }

    /// The handshake may take this long: up to 60,000 ms, 0 for 20,000.
    ///
    /// # Errors
    /// `Invalid` for more.
    pub fn timeout_ms(mut self, ms: u32) -> Result<Self, Error> {
        self.timeout_ms = check_ms(ms, TIMEOUT_MAX_MS)?;
        Ok(self)
    }

    /// The idle time after the switch: up to 1,800,000 ms, 0 for 60,000.
    ///
    /// # Errors
    /// `Invalid` for more.
    pub fn idle_ms(mut self, ms: u32) -> Result<Self, Error> {
        self.idle_ms = check_ms(ms, IDLE_MAX_MS)?;
        Ok(self)
    }

    /// Pins the leaf certificate or key: at most [`MAX_PINS`].
    ///
    /// # Errors
    /// `Invalid` for more.
    pub fn pins(mut self, pins: &'a [Pin]) -> Result<Self, Error> {
        if pins.len() > MAX_PINS {
            return Err(Error::Invalid);
        }
        self.pins = pins;
        Ok(self)
    }
}

impl Default for Tls<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// The card root a file read writes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Root {
    /// This plugin's data folder.
    Data,
    /// The user's files on the card (needs `uses.storage`).
    Files,
}

impl Root {
    const fn wire(self) -> u32 {
        match self {
            Self::Data => 0,
            Self::Files => 1,
        }
    }
}

/// A UDP broadcast to send: a payload to the network's broadcast address on
/// each port, and the answers collected for a time.
#[derive(Clone, Copy)]
#[must_use = "send with `Sockets::broadcast`"]
pub struct Broadcast<'a> {
    token: u32,
    data: &'a [u8],
    wait_ms: u32,
    ports: &'a [u16],
}

impl fmt::Debug for Broadcast<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Broadcast")
            .field("token", &self.token)
            .field("data_len", &self.data.len())
            .field("wait_ms", &self.wait_ms)
            .field("ports", &self.ports)
            .finish()
    }
}

impl<'a> Broadcast<'a> {
    /// A broadcast of `data` to `ports`, answers collected for `wait_ms`.
    ///
    /// # Errors
    /// `Invalid` for no data, no port or a port 0; `NoSpace` for data over
    /// [`BROADCAST_MAX_BYTES`] or more than [`BROADCAST_MAX_PORTS`] ports;
    /// `Invalid` for a wait outside 100 to 10,000 ms.
    pub fn new(token: u32, data: &'a [u8], wait_ms: u32, ports: &'a [u16]) -> Result<Self, Error> {
        if data.len() > BROADCAST_MAX_BYTES || ports.len() > BROADCAST_MAX_PORTS {
            return Err(Error::NoSpace);
        }
        if data.is_empty()
            || ports.is_empty()
            || ports.contains(&0)
            || !(100..=10_000).contains(&wait_ms)
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            token,
            data,
            wait_ms,
            ports,
        })
    }
}

/// Why a socket ended (`docs/abi/sockets.md` section 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// None given.
    None,
    /// No network came within 30 s, or the radio went off.
    NoNetwork,
    /// The name was not found.
    Dns,
    /// The TLS handshake failed.
    Tls,
    /// The time passed (connecting, or idle).
    Timeout,
    /// Could not connect.
    Refused,
    /// The card failed.
    Io,
    /// The card was full.
    Full,
    /// The offline power mode.
    Offline,
    /// The plugin's network use is on hold.
    Saving,
    /// The user turned this plugin's network off.
    NetworkOff,
    /// No TLS session came in time.
    Busy,
    /// A quiet secure socket gave its session to a waiting one.
    SessionNeeded,
    /// The connection or the network went away.
    Lost,
    /// The certificate does not match the name.
    Name,
    /// The certificate is expired.
    Expired,
    /// The peer closed before a file read's length.
    ClosedEarly,
    /// The screen the socket belonged to closed.
    ScreenClosed,
    /// The work run it belonged to reached its longest time.
    WorkEnded,
    /// The shell had no memory for the socket's own buffers.
    NoMemory,
    /// A reason a newer host added: a failure.
    Other(u32),
}

impl Reason {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::NoNetwork,
            2 => Self::Dns,
            3 => Self::Tls,
            4 => Self::Timeout,
            5 => Self::Refused,
            9 => Self::Io,
            10 => Self::Full,
            11 => Self::Offline,
            12 => Self::Saving,
            13 => Self::NetworkOff,
            20 => Self::Busy,
            21 => Self::SessionNeeded,
            22 => Self::Lost,
            23 => Self::Name,
            24 => Self::Expired,
            25 => Self::ClosedEarly,
            26 => Self::ScreenClosed,
            27 => Self::WorkEnded,
            28 => Self::NoMemory,
            other => Self::Other(other),
        }
    }
}

/// What happened to a socket: `event.socket.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SocketKind {
    /// A plain socket connected.
    Connected,
    /// A TLS handshake passed; [`Sockets::peer`] answers during this event.
    Secured,
    /// Only the trust failed (the socket is closed); [`Sockets::peer`] answers
    /// during this event, so the user can pin the key.
    Untrusted,
    /// Data came for the read asked: [`SocketEvent::len`] bytes, 0 when the
    /// peer closed its side.
    Received,
    /// A file read finished and the file is in place.
    Saved,
    /// The send queue drained: send the rest now.
    Sent,
    /// The socket ended without a failure.
    Closed,
    /// The socket ended with a failure; see [`SocketEvent::reason`].
    Failed,
    /// A kind a newer host added.
    Other(u32),
}

impl SocketKind {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Connected,
            1 => Self::Secured,
            2 => Self::Untrusted,
            3 => Self::Received,
            4 => Self::Saved,
            5 => Self::Sent,
            6 => Self::Closed,
            7 => Self::Failed,
            other => Self::Other(other),
        }
    }
}

/// Something happened to a socket: `event.socket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketEvent {
    now_ms: u64,
    id: SocketId,
    token: u32,
    kind: SocketKind,
    reason: Reason,
    len: u32,
}

impl SocketEvent {
    pub(crate) fn decode(r: wire::EventSocket) -> Result<Self, Error> {
        Ok(Self {
            now_ms: r.now_ms,
            id: SocketId::new(r.socket)?,
            token: r.token,
            kind: SocketKind::from_wire(r.kind),
            reason: Reason::from_wire(r.reason),
            len: r.len,
        })
    }

    /// The host's clock.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The socket.
    #[must_use]
    pub const fn id(&self) -> SocketId {
        self.id
    }

    /// The plugin's own number of the socket.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }

    /// What happened.
    #[must_use]
    pub const fn kind(&self) -> SocketKind {
        self.kind
    }

    /// Why it ended, for `Failed` and `Closed`; [`Reason::None`] otherwise.
    #[must_use]
    pub const fn reason(&self) -> Reason {
        self.reason
    }

    /// For `Received`: the bytes that came to read now; 0 when the peer closed
    /// its side. 0 for other kinds.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.len
    }

    /// Whether [`SocketEvent::len`] is 0.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// What happened to a broadcast: `event.broadcast.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BroadcastKind {
    /// One answer came: read it with [`Sockets::data`] during this event.
    Answer,
    /// The wait is over (or there was no network: [`Reason::NoNetwork`]).
    Done,
    /// The broadcast was refused by power or feature.
    Failed,
    /// A kind a newer host added.
    Other(u32),
}

/// Something happened to a broadcast: `event.broadcast`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BroadcastEvent<'a> {
    now_ms: u64,
    id: SocketId,
    token: u32,
    kind: BroadcastKind,
    reason: Reason,
    len: u32,
    from: &'a str,
}

impl<'a> BroadcastEvent<'a> {
    pub(crate) fn decode(r: wire::EventBroadcast<'a>) -> Result<Self, Error> {
        Ok(Self {
            now_ms: r.now_ms,
            id: SocketId::new(r.socket)?,
            token: r.token,
            kind: match r.kind {
                0 => BroadcastKind::Answer,
                1 => BroadcastKind::Done,
                2 => BroadcastKind::Failed,
                other => BroadcastKind::Other(other),
            },
            reason: Reason::from_wire(r.reason),
            len: r.len,
            from: r.from,
        })
    }

    /// The host's clock.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The broadcast's id (the one [`Sockets::broadcast`] answered).
    #[must_use]
    pub const fn id(&self) -> SocketId {
        self.id
    }

    /// The plugin's own number of the broadcast.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }

    /// What happened.
    #[must_use]
    pub const fn kind(&self) -> BroadcastKind {
        self.kind
    }

    /// Why it ended, for `Done` and `Failed`.
    #[must_use]
    pub const fn reason(&self) -> Reason {
        self.reason
    }

    /// For an answer: its length, at most 1,024 bytes.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.len
    }

    /// Whether [`BroadcastEvent::len`] is 0.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// For an answer: the sender's address and port.
    #[must_use]
    pub const fn from(&self) -> &'a str {
        self.from
    }
}

/// Why the trust of a TLS socket failed (for an `Untrusted` socket).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Untrusted {
    /// The chain does not lead to a root of the host's store.
    UnknownIssuer,
    /// Pins were given and none matched the leaf.
    NotPinned,
    /// A reason a newer host added.
    Other(u32),
}

/// The certificate and key of a TLS socket, borrowed from the reply buffer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Peer<'a>(wire::ServiceSockPeer<'a>);

impl fmt::Debug for Peer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Peer")
            .field("time_checked", &self.time_checked())
            .finish_non_exhaustive()
    }
}

impl Peer<'_> {
    /// Whether the dates were checked against network time. Clear when the
    /// time was not known within 5 s: mail refuses such a session.
    #[must_use]
    pub const fn time_checked(&self) -> bool {
        self.0.flags & 1 != 0
    }

    /// For an `Untrusted` socket, why.
    #[must_use]
    pub const fn untrusted(&self) -> Untrusted {
        match self.0.reason {
            0 => Untrusted::UnknownIssuer,
            1 => Untrusted::NotPinned,
            other => Untrusted::Other(other),
        }
    }

    /// The SHA-256 of the leaf certificate's DER.
    ///
    /// # Errors
    /// `Invalid` for a hash that is not 32 bytes.
    pub fn certificate(&self) -> Result<[u8; 32], Error> {
        self.0.certificate.try_into().map_err(|_| Error::Invalid)
    }

    /// The SHA-256 of the leaf's SubjectPublicKeyInfo DER.
    ///
    /// # Errors
    /// `Invalid` for a hash that is not 32 bytes.
    pub fn key(&self) -> Result<[u8; 32], Error> {
        self.0.key.try_into().map_err(|_| Error::Invalid)
    }

    /// A pin of the leaf's key: what a user accepts after `Untrusted`.
    ///
    /// # Errors
    /// `Invalid` for a hash that is not 32 bytes.
    pub fn pin_key(&self) -> Result<Pin, Error> {
        self.key().map(Pin::public_key)
    }

    /// A pin of the leaf certificate.
    ///
    /// # Errors
    /// `Invalid` for a hash that is not 32 bytes.
    pub fn pin_certificate(&self) -> Result<Pin, Error> {
        self.certificate().map(Pin::certificate)
    }
}

/// A `core.sockets` handle.
#[derive(Debug)]
#[must_use = "release the sockets handle when finished"]
pub struct Sockets {
    service: Service,
}

impl Sockets {
    /// Opens the service (needs `uses.network`).
    ///
    /// # Errors
    /// `Denied` without the feature, `Unsupported` on a host without sockets.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, 0).map(|service| Self { service })
    }

    /// Releases the handle. Open sockets stay until they end or the plugin
    /// stops.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }

    fn call<'r, I>(
        &self,
        host: &Host<'_>,
        op: u32,
        records: I,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error>
    where
        I: Iterator<Item = Row<'r>> + Clone,
    {
        let operation = self.service.operation(op).ok_or(Error::Unsupported)?;
        let len = batch::encode_counted(
            records,
            usize::from(operation.input_rows),
            operation.max_input,
            input,
        )
        .map_err(Error::from_status)?;
        self.service.call(host, op, &input[..len], out)
    }

    fn call_empty<'r, I>(
        &self,
        host: &Host<'_>,
        op: u32,
        records: I,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error>
    where
        I: Iterator<Item = Row<'r>> + Clone,
    {
        if self.call(host, op, records, input, out)? != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// Opens a connection. Its outcome comes as [`SocketEvent`]s with the
    /// connection's token; memory for its queue and read part is reserved now.
    ///
    /// `input` holds the encoded rows (at most [`INPUT`] bytes), `out` the reply
    /// ([`SMALL_REPLY`] bytes).
    ///
    /// # Errors
    /// `Invalid` for what [`Connect::check`] refuses (nothing is sent);
    /// `Denied` without `uses.network`; `Busy` at 2 sockets of the plugin (or
    /// the device's); `NoMemory` when the network pool has no room (retry
    /// then); `State` without a subscription to the socket event;
    /// `Unsupported` for TLS on a host without it; otherwise the host's.
    pub fn connect(
        &self,
        host: &Host<'_>,
        connect: &Connect<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<SocketId, Error> {
        connect.check()?;
        let head = Row::Connect(wire::ServiceSockConnect {
            token: connect.token,
            port: u32::from(connect.port),
            flags: if connect.tls { FLAG_TLS } else { 0 },
            timeout_ms: connect.timeout_ms,
            idle_ms: connect.idle_ms,
            host: connect.host,
            server_name: connect.server_name,
        });
        let rows = core::iter::once(head).chain(pin_rows(connect.pins));
        let n = self.call(host, schema::OP_CONNECT, rows, input, out)?;
        started(&out[..n])
    }

    /// Queues `data` to send: 1 to [`PIECE`] bytes. The host sends a `Sent`
    /// event once the queue has drained.
    ///
    /// # Errors
    /// `Invalid` for no data, `NoSpace` for more than [`PIECE`] bytes, `Busy`
    /// when the queue cannot take it (nothing is queued: send the rest on
    /// `Sent`), `State` while the socket connects or switches to TLS,
    /// `NotFound` for another plugin's or an ended socket.
    pub fn send(
        &self,
        host: &Host<'_>,
        id: SocketId,
        data: &[u8],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if data.is_empty() {
            return Err(Error::Invalid);
        }
        if data.len() > PIECE {
            return Err(Error::NoSpace);
        }
        let row = Row::Send(wire::ServiceSockSend { socket: id.0, data });
        self.call_empty(host, schema::OP_SEND, core::iter::once(row), input, out)
    }

    /// Asks for the next data: up to `max` bytes (1 to [`MAX_READ`]; the host
    /// reads at most its own cap), or with `exact` exactly `max`. One
    /// `Received` event answers; read the bytes during it.
    ///
    /// # Errors
    /// `Invalid` for `max` 0 or over [`MAX_READ`], or `exact` with more than the
    /// host's cap; `Busy` while a read is under way; `State` while the socket
    /// connects or switches; `NotFound` for another plugin's or an ended socket.
    pub fn read_bytes(
        &self,
        host: &Host<'_>,
        id: SocketId,
        max: u32,
        exact: bool,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if max == 0 || max > MAX_READ {
            return Err(Error::Invalid);
        }
        let row = Row::Read(wire::ServiceSockRead {
            socket: id.0,
            mode: 0,
            flags: if exact { FLAG_EXACT } else { 0 },
            max,
            len: 0,
            root: 0,
            path: "",
        });
        self.call_empty(host, schema::OP_READ, core::iter::once(row), input, out)
    }

    /// Asks the host to write the next `len` bytes to `path` (as
    /// `<path>.part`, renamed when whole): a `Saved` event when it is in
    /// place, a `Failed` one (and the old file kept) when not. No byte of it
    /// is held in memory.
    ///
    /// # Errors
    /// `Invalid` for `len` 0 or over [`MAX_FILE_LEN`] or an empty path,
    /// `NoSpace` for a path over [`MAX_PATH`] bytes; `Denied` for a root or path
    /// the plugin may not write; `Busy` for a read under way or a target
    /// in use; `Full` for a length above the free space; `State` while the
    /// socket connects; `NotFound` for another plugin's or an ended socket.
    pub fn read_file(
        &self,
        host: &Host<'_>,
        id: SocketId,
        root: Root,
        path: &str,
        len: u64,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if len == 0 || len > MAX_FILE_LEN || path.is_empty() {
            return Err(Error::Invalid);
        }
        if path.len() > MAX_PATH {
            return Err(Error::NoSpace);
        }
        let row = Row::Read(wire::ServiceSockRead {
            socket: id.0,
            mode: 1,
            flags: 0,
            max: 0,
            len,
            root: root.wire(),
            path,
        });
        self.call_empty(host, schema::OP_READ, core::iter::once(row), input, out)
    }

    /// Up to [`PIECE`] bytes of what the last `Received` event brought (or of a
    /// broadcast's answer), from `offset`; empty at the end. Readable only while
    /// that event is delivered. `out` must hold [`REPLY`] bytes.
    ///
    /// # Errors
    /// `State` elsewhere; `NotFound` for another plugin's or an ended socket.
    pub fn data<'a>(
        &self,
        host: &Host<'_>,
        id: SocketId,
        offset: u32,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<&'a [u8], Error> {
        let row = Row::At(wire::ServiceSockAt {
            socket: id.0,
            offset,
        });
        let n = self.call(host, schema::OP_DATA, core::iter::once(row), input, out)?;
        let bytes = one(&out[..n])?
            .decode::<wire::ServiceSockBytes<'a>>()
            .map_err(|_| Error::Invalid)?;
        if bytes.data.len() > PIECE {
            return Err(Error::Invalid);
        }
        Ok(bytes.data)
    }

    /// Switches an open plain socket to TLS (STARTTLS). The outcome is a
    /// `Secured`, `Untrusted` or `Failed` event.
    ///
    /// `held` is the [`Reader`] of this socket: nothing it holds may be taken as
    /// coming over TLS, so a reader with unused bytes makes this `State` (close
    /// the socket then). A plugin that reads without the SDK's reader passes
    /// `Reader::new(id, &mut [])` and checks the same itself.
    ///
    /// # Errors
    /// `Invalid` for a reader of another socket; `State` for unused bytes in the
    /// reader, a read under way, unread or unsent bytes, or a socket not open;
    /// `Unsupported` without TLS; `NotFound` for another plugin's or an ended
    /// socket.
    pub fn start_tls(
        &self,
        host: &Host<'_>,
        id: SocketId,
        tls: &Tls<'_>,
        held: &Reader<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if held.id() != id {
            return Err(Error::Invalid);
        }
        if held.held() != 0 {
            return Err(Error::State);
        }
        let head = Row::Tls(wire::ServiceSockTls {
            socket: id.0,
            timeout_ms: tls.timeout_ms,
            idle_ms: tls.idle_ms,
            server_name: tls.server_name,
        });
        let rows = core::iter::once(head).chain(pin_rows(tls.pins));
        self.call_empty(host, schema::OP_START_TLS, rows, input, out)
    }

    /// The certificate and key hashes of a `Secured` or `Untrusted` socket.
    /// Readable only while that event is delivered.
    ///
    /// # Errors
    /// `State` elsewhere; `NotFound` for another plugin's or an ended socket.
    pub fn peer<'a>(
        &self,
        host: &Host<'_>,
        id: SocketId,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Peer<'a>, Error> {
        let row = Row::Ref(wire::ServiceSockRef { socket: id.0 });
        let n = self.call(host, schema::OP_PEER, core::iter::once(row), input, out)?;
        let peer = one(&out[..n])?
            .decode::<wire::ServiceSockPeer<'a>>()
            .map_err(|_| Error::Invalid)?;
        Ok(Peer(peer))
    }

    /// Closes a socket (or a broadcast). A closed or unknown one is fine.
    ///
    /// # Errors
    /// The host's refusal, or `Invalid` for a reply that is not empty.
    pub fn close(
        &self,
        host: &Host<'_>,
        id: SocketId,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let row = Row::Ref(wire::ServiceSockRef { socket: id.0 });
        self.call_empty(host, schema::OP_CLOSE, core::iter::once(row), input, out)
    }

    /// Sends a UDP broadcast and collects the answers as
    /// [`BroadcastEvent`]s: at most 8, then one `Done`. It counts as one of the
    /// plugin's sockets while it runs.
    ///
    /// # Errors
    /// `Busy` at the socket limit, `NoMemory` when the pool has no room, `State`
    /// without a subscription to the broadcast event; otherwise the host's.
    pub fn broadcast(
        &self,
        host: &Host<'_>,
        broadcast: &Broadcast<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<SocketId, Error> {
        let head = Row::Broadcast(wire::ServiceSockBroadcast {
            token: broadcast.token,
            wait_ms: broadcast.wait_ms,
            data: broadcast.data,
        });
        let rows = core::iter::once(head).chain(broadcast.ports.iter().map(|&port| {
            Row::Port(wire::ServiceSockPort {
                port: u32::from(port),
            })
        }));
        let n = self.call(host, schema::OP_BROADCAST, rows, input, out)?;
        started(&out[..n])
    }
}

fn pin_rows<'a>(pins: &'a [Pin]) -> impl Iterator<Item = Row<'a>> + Clone {
    pins.iter().map(|pin| {
        Row::Pin(wire::ServiceSockPin {
            kind: pin.wire_kind(),
            hash: &pin.hash,
        })
    })
}

fn started(bytes: &[u8]) -> Result<SocketId, Error> {
    let started = one(bytes)?
        .decode::<wire::ServiceSockStarted>()
        .map_err(|_| Error::Invalid)?;
    SocketId::new(started.socket)
}

fn one(bytes: &[u8]) -> Result<RawRecord<'_>, Error> {
    RawRecord::one(bytes).map_err(|_| Error::Invalid)
}

/// The rows a `core.sockets` call is made of; each is encoded as its own record type.
#[derive(Clone, Copy)]
enum Row<'a> {
    Connect(wire::ServiceSockConnect<'a>),
    Pin(wire::ServiceSockPin<'a>),
    Send(wire::ServiceSockSend<'a>),
    Read(wire::ServiceSockRead<'a>),
    At(wire::ServiceSockAt),
    Tls(wire::ServiceSockTls<'a>),
    Ref(wire::ServiceSockRef),
    Broadcast(wire::ServiceSockBroadcast<'a>),
    Port(wire::ServiceSockPort),
}
impl AsWire for Row<'_> {
    fn wire(&self) -> &dyn WireEncode {
        match self {
            Self::Connect(row) => row,
            Self::Pin(row) => row,
            Self::Send(row) => row,
            Self::Read(row) => row,
            Self::At(row) => row,
            Self::Tls(row) => row,
            Self::Ref(row) => row,
            Self::Broadcast(row) => row,
            Self::Port(row) => row,
        }
    }
}

#[cfg(test)]
mod tests;
