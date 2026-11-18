//! What plugins' sockets are set up with and fail by: TLS (implicit, as
//! IMAPS on 993, or switched to on an open socket, as STARTTLS), the
//! certificates trusted, and why a socket failed
//! ([ADR 0014](https://github.com/hthoene/paperpaw/blob/main/docs/adr/0014-sockets-for-plugins.md)).
//!
//! A plugin gets TLS as it is, bytes in and out after the handshake: what
//! it speaks over it is its own. The server's certificate is checked
//! against the authorities PaperPaw ships unless the plugin pins the
//! certificates or keys it trusts instead ([`Trust`]).
//!
//! *Valid now* is checked where the device knows the time. Where it does
//! not yet (no clock that kept it, no network time since a cold start),
//! everything else is checked and the socket says so
//! ([`SocketNotice::Secured`](crate::SocketNotice::Secured)'s
//! `time_checked`): a plugin that must not trust an expired certificate
//! closes and tries again once the clock is set. Failing instead would
//! keep an X4 from every server just after it starts.

use alloc::string::String;
use alloc::vec::Vec;

/// The most certificates a socket pins ([`Trust::Pinned`]): the one in use
/// and the ones it is renewed to.
pub const MAX_PINS: usize = 4;

/// The most a connection and its handshake may take together
/// ([`Tls::timeout_ms`]).
pub const MAX_TLS_TIMEOUT_MS: u32 = 60_000;

/// The longest a TLS socket may wait for something to send or read
/// ([`Tls::idle_ms`]): 30 minutes, as long as a mail server waits for a
/// client waiting for new mail (IMAP IDLE, which asks again within 29).
pub const MAX_TLS_IDLE_MS: u32 = 1_800_000;

/// How a socket's TLS is set up (`Ask::connect_tls`,
/// `Ask::start_tls`). Made with [`Tls::new`] (the authorities
/// PaperPaw ships) or [`Tls::pinned`] and the methods that set the rest.
///
/// A TLS connection holds much memory while it is open (about 40 KiB on
/// the X4) and keeps the radio awake: connect, do the work, close.
///
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tls {
    /// Which certificates are trusted.
    pub trust: Trust,
    /// The name the server is asked for (SNI) and its certificate is
    /// checked for, where it is not the host connected to (e.g. connected
    /// by its address).
    pub server_name: Option<String>,
    /// How long the connection and the handshake may take together, in
    /// milliseconds, at most [`MAX_TLS_TIMEOUT_MS`]; also the wait for a
    /// device's secure connections to be free
    /// ([`SocketError::Busy`]). 20 s unless set.
    pub timeout_ms: u32,
    /// Nothing sent or read for this long, in milliseconds, ends the
    /// socket ([`SocketError::Timeout`], `reason` `idle`), at most
    /// [`MAX_TLS_IDLE_MS`]; 0: never, not for a socket of background work
    /// (it is the run's, which no screen closes: refused,
    /// [`SocketError::Options`]). 60 s unless set: an open connection
    /// holds memory and keeps the radio awake. Where the device holds few
    /// connections ([`Capabilities::tls_sessions`](crate::Capabilities)) and
    /// another request or socket waits for one, a socket idle for a few
    /// seconds is ended at once, whatever this says (`Timeout`, `reason`
    /// `slot needed`).
    pub idle_ms: u32,
    /// Spare, zero now.
    spare: u32,
}

impl Tls {
    /// Trusting the authorities PaperPaw ships: the certificate's chain
    /// ends in one of them, it is for the server's name and valid now.
    pub fn new() -> Self {
        Self {
            trust: Trust::Authorities,
            server_name: None,
            timeout_ms: 20_000,
            idle_ms: 60_000,
            spare: 0,
        }
    }

    /// Trusting `certificates` alone (a server of one's own, with a
    /// certificate no authority signed): the server's must be one of them
    /// and valid now; its name and chain are not checked. A renewed
    /// certificate is another: [`Tls::pinned_keys`] outlasts renewals that
    /// keep the key.
    pub fn pinned(certificates: Vec<CertificateHash>) -> Self {
        Self {
            trust: Trust::Pinned(certificates),
            ..Self::new()
        }
    }

    /// Trusting certificates of `keys` alone, whoever signed them: the
    /// server's must be for one of them and valid now; its name and chain
    /// are not checked.
    pub fn pinned_keys(keys: Vec<KeyHash>) -> Self {
        Self {
            trust: Trust::PinnedKeys(keys),
            ..Self::new()
        }
    }

    /// With `name` asked for and checked instead of the host.
    #[must_use]
    pub fn server_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self
    }

    /// With the connection and the handshake taking at most `ms`.
    #[must_use]
    pub fn timeout_ms(mut self, ms: u32) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Closing after `ms` with nothing sent or read (0: never, not for
    /// background work's sockets).
    #[must_use]
    pub fn idle_ms(mut self, ms: u32) -> Self {
        self.idle_ms = ms;
        self
    }

    /// What is wrong with it, if anything: too many or no pins, a server
    /// name that is no host name, a time beyond its most.
    pub fn check(&self) -> Result<(), SocketError> {
        let pins_fit = match &self.trust {
            Trust::Authorities => true,
            Trust::Pinned(pins) => (1..=MAX_PINS).contains(&pins.len()),
            Trust::PinnedKeys(keys) => (1..=MAX_PINS).contains(&keys.len()),
        };
        let name_fits = self.server_name.as_deref().is_none_or(is_host_name);
        let fits = pins_fit
            && name_fits
            && self.timeout_ms <= MAX_TLS_TIMEOUT_MS
            && self.idle_ms <= MAX_TLS_IDLE_MS;
        if fits {
            Ok(())
        } else {
            Err(SocketError::Options)
        }
    }
}

impl Default for Tls {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `name` may be a server's name: letters, digits, `-` and `.`,
/// 1 to 253 of them.
fn is_host_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// Which server certificates a TLS socket trusts ([`Tls::trust`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Trust {
    /// Those whose chain ends in an authority PaperPaw ships (Mozilla's
    /// list, updated with the firmware), for the server's name, valid now
    /// (where the time is known: see the module).
    Authorities,
    /// These alone (1 to [`MAX_PINS`]), valid now; name and chain not
    /// checked. The reader compares a certificate's hash
    /// ([`SocketNotice::Untrusted`](crate::SocketNotice::Untrusted)) before
    /// a plugin pins it.
    Pinned(Vec<CertificateHash>),
    /// Those for one of these keys (1 to [`MAX_PINS`]), valid now; name
    /// and chain not checked: renewed certificates of the same key stay
    /// trusted.
    PinnedKeys(Vec<KeyHash>),
}

/// A certificate, by the SHA-256 of its DER: what a plugin pins
/// ([`Trust::Pinned`]) and the kernel tells of the server's
/// ([`SocketNotice::Secured`], [`SocketNotice::Untrusted`]). Written as 64
/// hex digits.
///
/// [`SocketNotice::Secured`]: crate::SocketNotice::Secured
/// [`SocketNotice::Untrusted`]: crate::SocketNotice::Untrusted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CertificateHash([u8; 32]);

impl CertificateHash {
    /// The certificate whose DER's SHA-256 is `sha256`.
    pub const fn new(sha256: [u8; 32]) -> Self {
        Self(sha256)
    }

    /// Its SHA-256.
    pub const fn sha256(&self) -> &[u8; 32] {
        &self.0
    }

    /// The hash written in `text`: 64 hex digits, either case, with `:`
    /// or spaces between them or not (as browsers and `openssl` show it).
    pub fn parse(text: &str) -> Option<Self> {
        sha256_of(text).map(Self)
    }
}

impl core::fmt::Display for CertificateHash {
    /// 64 lower-case hex digits.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_hex(&self.0, f)
    }
}

/// A server's key, by the SHA-256 of its certificate's
/// SubjectPublicKeyInfo (DER), as browsers pinned keys: what a plugin pins
/// ([`Trust::PinnedKeys`]) and the kernel tells of the server's. Written
/// as 64 hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyHash([u8; 32]);

impl KeyHash {
    /// The key whose SubjectPublicKeyInfo's SHA-256 is `sha256`.
    pub const fn new(sha256: [u8; 32]) -> Self {
        Self(sha256)
    }

    /// Its SHA-256.
    pub const fn sha256(&self) -> &[u8; 32] {
        &self.0
    }

    /// The hash written in `text`, as [`CertificateHash::parse`] reads it.
    pub fn parse(text: &str) -> Option<Self> {
        sha256_of(text).map(Self)
    }
}

impl core::fmt::Display for KeyHash {
    /// 64 lower-case hex digits.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_hex(&self.0, f)
    }
}

/// A SHA-256 written as 64 hex digits, either case, `:` or spaces between
/// them or not.
fn sha256_of(text: &str) -> Option<[u8; 32]> {
    let mut hash = [0u8; 32];
    let mut digits = 0usize;
    for b in text.bytes() {
        if b == b':' || b == b' ' {
            continue;
        }
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        let byte = hash.get_mut(digits / 2)?;
        *byte = *byte << 4 | digit;
        digits += 1;
    }
    (digits == 64).then_some(hash)
}

fn write_hex(bytes: &[u8], f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    bytes.iter().try_for_each(|b| write!(f, "{b:02x}"))
}

/// Why a server's certificate was not trusted
/// ([`SocketNotice::Untrusted`](crate::SocketNotice::Untrusted)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Distrust {
    /// Its chain ends in no authority PaperPaw ships (e.g. one it signed
    /// itself, or a company's own).
    UnknownIssuer,
    /// It is none of those pinned, nor for a key pinned ([`Trust::Pinned`],
    /// [`Trust::PinnedKeys`]): renewed, or someone else's.
    NotPinned,
}

impl Distrust {
    /// Its name, e.g. `unknown-issuer`, as an event's parameters write it.
    pub fn as_str(self) -> &'static str {
        match self {
            Distrust::UnknownIssuer => "unknown-issuer",
            Distrust::NotPinned => "not-pinned",
        }
    }

    /// The reason named `name` (see [`as_str`](Self::as_str)).
    pub fn parse(name: &str) -> Option<Self> {
        [Distrust::UnknownIssuer, Distrust::NotPinned]
            .into_iter()
            .find(|d| d.as_str() == name)
    }
}

/// Why a socket failed
/// ([`SocketNotice::Failed`](crate::SocketNotice::Failed), whose `reason`
/// says more, in English, for a log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SocketError {
    /// No network to connect on.
    NoNetwork,
    /// Not allowed now: the plugin does not declare the network, or the
    /// power level keeps it offline (`reason` says which).
    NotAllowed,
    /// The host's name was not found.
    Dns,
    /// The connection could not be made (refused, no route).
    Refused,
    /// The connection, or its handshake, took longer than allowed.
    Timeout,
    /// The connection was lost.
    Lost,
    /// Too many sockets are open.
    TooMany,
    /// The device's secure connections were all in use for as long as
    /// the socket waited ([`Tls::timeout_ms`];
    /// [`Capabilities::tls_sessions`](crate::Capabilities::tls_sessions)).
    Busy,
    /// Not enough memory, e.g. for a secure connection.
    NoMemory,
    /// The device does not do this: TLS where
    /// [`Capabilities::tls_sessions`](crate::Capabilities::tls_sessions)
    /// is 0, or a way of reading this build does not know.
    Unsupported,
    /// The [`Tls`] set up is wrong ([`Tls::check`]).
    Options,
    /// Not now: a second read while one is under way; TLS switched to on
    /// a socket not connected, already secure, reading, or with bytes
    /// come that were not read (they would pass as secure).
    NotReady,
    /// The secure connection failed (the handshake, or the server ended it).
    Tls,
    /// The server's certificate is for another name.
    NameMismatch,
    /// The server's certificate has expired or is not valid yet (or the
    /// device's clock is wrong).
    Expired,
    /// Writing what was read failed (e.g. the card is full).
    Io,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused.
    Other(u16),
}

impl SocketError {
    const ALL: [SocketError; 16] = [
        SocketError::NoNetwork,
        SocketError::NotAllowed,
        SocketError::Dns,
        SocketError::Refused,
        SocketError::Timeout,
        SocketError::Lost,
        SocketError::TooMany,
        SocketError::Busy,
        SocketError::NoMemory,
        SocketError::Unsupported,
        SocketError::Options,
        SocketError::NotReady,
        SocketError::Tls,
        SocketError::NameMismatch,
        SocketError::Expired,
        SocketError::Io,
    ];

    /// Its name, e.g. `no-network`, as an event's parameters write it.
    pub fn as_str(self) -> &'static str {
        match self {
            SocketError::NoNetwork => "no-network",
            SocketError::NotAllowed => "not-allowed",
            SocketError::Dns => "dns",
            SocketError::Refused => "refused",
            SocketError::Timeout => "timeout",
            SocketError::Lost => "lost",
            SocketError::TooMany => "too-many",
            SocketError::Busy => "busy",
            SocketError::NoMemory => "no-memory",
            SocketError::Unsupported => "unsupported",
            SocketError::Options => "options",
            SocketError::NotReady => "not-ready",
            SocketError::Tls => "tls",
            SocketError::NameMismatch => "name-mismatch",
            SocketError::Expired => "expired",
            SocketError::Io => "io",
            SocketError::Other(_) => "other",
        }
    }

    /// The error named `name` (see [`as_str`](Self::as_str); an
    /// [`Other`](Self::Other) as `other-` and its code, as its `Display`
    /// writes it).
    pub fn parse(name: &str) -> Option<Self> {
        if let Some(code) = name.strip_prefix("other-") {
            return crate::replies::other_code(code).map(SocketError::Other);
        }
        Self::ALL.into_iter().find(|e| e.as_str() == name)
    }
}

impl core::fmt::Display for SocketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SocketError::Other(code) => write!(f, "other-{code}"),
            other => f.write_str(other.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    use super::*;
    use alloc::format;

    #[test]
    fn hashes_read_as_browsers_and_openssl_write_them() {
        let plain = "00112233445566778899aabbccddeeff00112233445566778899AABBCCDDEEFF";
        let hash = CertificateHash::parse(plain).unwrap();
        assert_eq!(hash.sha256()[1], 0x11);
        assert_eq!(hash.sha256()[31], 0xff);
        let colons = "00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:\
                      00:11:22:33:44:55:66:77:88:99:aa:bb:cc:dd:ee:ff";
        assert_eq!(CertificateHash::parse(colons), Some(hash));
        assert_eq!(format!("{hash}"), plain.to_ascii_lowercase());
        assert_eq!(CertificateHash::parse(&format!("{hash}")), Some(hash));
    }

    #[test]
    fn a_hash_of_the_wrong_length_or_digits_is_none() {
        let short = "0011";
        let long = "0".repeat(66);
        let odd = "0".repeat(63);
        let letter = format!("{}g", "0".repeat(63));
        for text in [short, &long, &odd, &letter, "", "::"] {
            assert_eq!(CertificateHash::parse(text), None, "{text}");
        }
    }

    #[test]
    fn options_are_checked() {
        let pin = CertificateHash::new([7; 32]);
        assert_eq!(Tls::new().check(), Ok(()));
        assert_eq!(Tls::pinned(alloc::vec![pin]).check(), Ok(()));
        let key = KeyHash::new([8; 32]);
        assert_eq!(Tls::pinned_keys(alloc::vec![key]).check(), Ok(()));
        assert_eq!(KeyHash::parse(&format!("{key}")), Some(key));
        let wrong = [
            Tls::pinned(Vec::new()),
            Tls::pinned(alloc::vec![pin; MAX_PINS + 1]),
            Tls::pinned_keys(Vec::new()),
            Tls::new().server_name(""),
            Tls::new().server_name("mail.example.org/inbox"),
            Tls::new().timeout_ms(MAX_TLS_TIMEOUT_MS + 1),
            Tls::new().idle_ms(MAX_TLS_IDLE_MS + 1),
        ];
        for tls in wrong {
            assert_eq!(tls.check(), Err(SocketError::Options), "{tls:?}");
        }
        assert_eq!(
            Tls::new().server_name("mail.lan").idle_ms(0).check(),
            Ok(())
        );
    }

    #[test]
    fn errors_and_reasons_read_back() {
        for error in SocketError::ALL {
            assert_eq!(SocketError::parse(error.as_str()), Some(error));
        }
        let other = SocketError::Other(7);
        assert_eq!(SocketError::parse(&format!("{other}")), Some(other));
        assert_eq!(SocketError::parse("not-a-cause"), None);
        for why in [Distrust::UnknownIssuer, Distrust::NotPinned] {
            assert_eq!(Distrust::parse(why.as_str()), Some(why));
        }
    }
}
