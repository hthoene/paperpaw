//! TLS for HTTPS: mbedTLS (Espressif's, through `mbedtls-rs`, prebuilt
//! for the ESP32-C3), driven without blocking. A [`Session`] works on
//! buffers: what came over the connection goes in ([`Session::feed`]),
//! what is to go out comes out ([`Session::output`]), and the main loop
//! moves both between them and the TCP socket, as for plain HTTP. So the
//! X4 stays usable during a handshake (a second or two of arithmetic) and
//! a download.
//!
//! Certificates: the chain must lead to one of Mozilla's roots
//! ([`roots`](crate::roots), checked in mbedTLS's verify callback) and be
//! for the host asked; mbedTLS checks the rest of the chain. Their dates
//! are checked in the same callback against the time set over the
//! network ([`crate::clock`]). A plugin's socket may instead pin the
//! certificates it trusts (`paperpaw_x4::tls_trust`): then the server's
//! own is looked at, its dates and whether it is one of them, not its
//! chain or name. Secure requests wait until that time is
//! known or a try to get it failed; then they go ahead with the dates
//! unchecked rather than not at all (ADR 0017: the X4 has no clock that
//! runs while it is off).
//!
//! A session takes about 40 KB of heap (two record buffers of 16 KB,
//! which TLS needs: a server may send records that long), only while it
//! is open, and holds one of the device's few TLS slots
//! ([`tls_slots`](crate::tls_slots)) for as long.

use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::ffi::{c_int, c_uchar, c_void};

use paperpaw_sdk::{CertificateHash, KeyHash, Trust};
use paperpaw_x4::tls_trust::{Checks, Fault, Leaf};

use crate::tls_slots::Slot;

use mbedtls_rs::sys::{
    MBEDTLS_ERR_SSL_ALLOC_FAILED, MBEDTLS_ERR_SSL_CONN_EOF, MBEDTLS_ERR_SSL_PEER_CLOSE_NOTIFY,
    MBEDTLS_ERR_SSL_RECEIVED_NEW_SESSION_TICKET, MBEDTLS_ERR_SSL_WANT_READ,
    MBEDTLS_ERR_SSL_WANT_WRITE, MBEDTLS_SSL_IS_CLIENT, MBEDTLS_SSL_PRESET_DEFAULT,
    MBEDTLS_SSL_TRANSPORT_STREAM, MBEDTLS_SSL_VERIFY_REQUIRED, MBEDTLS_X509_BADCERT_CN_MISMATCH,
    MBEDTLS_X509_BADCERT_EXPIRED, MBEDTLS_X509_BADCERT_FUTURE, MBEDTLS_X509_BADCERT_NOT_TRUSTED,
    mbedtls_ssl_close_notify, mbedtls_ssl_conf_authmode, mbedtls_ssl_conf_ca_chain,
    mbedtls_ssl_conf_rng, mbedtls_ssl_conf_verify, mbedtls_ssl_config, mbedtls_ssl_config_defaults,
    mbedtls_ssl_config_free, mbedtls_ssl_config_init, mbedtls_ssl_context, mbedtls_ssl_free,
    mbedtls_ssl_get_verify_result, mbedtls_ssl_handshake, mbedtls_ssl_init, mbedtls_ssl_read,
    mbedtls_ssl_set_bio, mbedtls_ssl_set_hostname, mbedtls_ssl_setup, mbedtls_ssl_write,
    mbedtls_x509_crt, mbedtls_x509_crt_free, mbedtls_x509_crt_init, mbedtls_x509_time,
};

// The flags `paperpaw_x4::tls_trust` works with are mbedTLS's.
const _: () = {
    assert!(MBEDTLS_X509_BADCERT_EXPIRED == paperpaw_x4::tls_trust::EXPIRED);
    assert!(MBEDTLS_X509_BADCERT_CN_MISMATCH == paperpaw_x4::tls_trust::CN_MISMATCH);
    assert!(MBEDTLS_X509_BADCERT_NOT_TRUSTED == paperpaw_x4::tls_trust::NOT_TRUSTED);
    assert!(MBEDTLS_X509_BADCERT_FUTURE == paperpaw_x4::tls_trust::FUTURE);
};

/// Bytes received and not yet taken by mbedTLS, at most: the socket is
/// read no further meanwhile.
const IN_LIMIT: usize = 4096;
/// Bytes mbedTLS gave to send and not yet sent, at most.
const OUT_LIMIT: usize = 4096;

/// Why a session failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The certificate does not lead to a root trusted.
    Untrusted,
    /// The certificate is for another host.
    OtherHost,
    /// The certificate has expired, or is not valid yet.
    Dates,
    /// Not enough memory for a session.
    Memory,
    /// Anything else mbedTLS said (its code).
    Tls(i32),
}

impl Error {
    /// What it is to the socket layer.
    pub fn fault(self) -> Fault {
        match self {
            Error::Untrusted => Fault::Untrusted,
            Error::OtherHost => Fault::OtherHost,
            Error::Dates => Fault::Dates,
            Error::Memory => Fault::Memory,
            Error::Tls(code) => Fault::Other(code),
        }
    }

    /// The reason, for the plugin that asked.
    pub fn reason(self) -> &'static str {
        match self {
            Error::Untrusted => "the server's certificate is not trusted",
            Error::OtherHost => "the server's certificate is for another host",
            Error::Dates => "the server's certificate has expired or is not valid yet",
            Error::Memory => "not enough memory for a secure connection",
            Error::Tls(_) => "secure connection failed",
        }
    }
}

/// Random numbers for mbedTLS, from the chip's generator (random from
/// its radio noise, which is on whenever TLS is used).
struct ChipRng(esp_hal::rng::Rng);

impl rand_core::TryRng for ChipRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        Ok(self.0.random())
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        Ok(u64::from(self.0.random()) << 32 | u64::from(self.0.random()))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        self.0.read(dst);
        Ok(())
    }
}

impl rand_core::TryCryptoRng for ChipRng {}

/// mbedTLS's global set-up (its random numbers), once.
fn init() {
    static mut DONE: bool = false;
    // SAFETY: one core, and sessions are made from the main loop only.
    unsafe {
        if core::ptr::addr_of!(DONE).read() {
            return;
        }
        let rng: &'static mut ChipRng = Box::leak(Box::new(ChipRng(esp_hal::rng::Rng::new())));
        // It lives as long as the firmware; made once (checked above).
        if let Ok(tls) = mbedtls_rs::Tls::new(rng) {
            core::mem::forget(tls);
        }
        core::ptr::addr_of_mut!(DONE).write(true);
    }
}

/// mbedTLS's random numbers for a session.
unsafe extern "C" fn random(_: *mut c_void, out: *mut c_uchar, len: usize) -> c_int {
    // SAFETY: mbedTLS passes a buffer of `len` bytes.
    let out = unsafe { core::slice::from_raw_parts_mut(out, len) };
    esp_hal::rng::Rng::new().read(out);
    0
}

/// mbedTLS's state of a session; set up in place and never moved, as it
/// points into itself.
struct Contexts {
    ssl: mbedtls_ssl_context,
    conf: mbedtls_ssl_config,
    /// An empty list of trusted certificates: mbedTLS wants one to verify
    /// at all; the roots are checked in the verify callback.
    no_roots: mbedtls_x509_crt,
}

/// mbedTLS's contexts, zeroed in place (they are large, and mbedTLS sets
/// them up there), or `None` without the memory.
fn zeroed_contexts() -> Option<Box<Contexts>> {
    let layout = core::alloc::Layout::new::<Contexts>();
    // SAFETY: a zeroed allocation of the layout of `Contexts`, plain C
    // structs for which zero is their state before their `init`; owned
    // by the box from here on.
    unsafe {
        let ptr = alloc::alloc::alloc_zeroed(layout).cast::<Contexts>();
        (!ptr.is_null()).then(|| Box::from_raw(ptr))
    }
}

/// The bytes between mbedTLS and the connection.
#[derive(Default)]
struct Pipes {
    /// Received, from `taken` on not yet given to mbedTLS.
    incoming: Vec<u8>,
    taken: usize,
    /// The other side closed the connection: nothing more comes.
    ended: bool,
    outgoing: Vec<u8>,
}

/// What reading a session brought.
#[derive(Debug, PartialEq, Eq)]
pub enum Read {
    /// So many bytes.
    Data(usize),
    /// Nothing yet.
    Wait,
    /// The other side closed the session.
    Closed,
}

pub struct Session {
    contexts: Box<Contexts>,
    pipes: Box<Pipes>,
    /// What the certificates are checked by; mbedTLS's verify callback
    /// works on it.
    checks: Box<Checks>,
    /// The host's name, for mbedTLS (which keeps a pointer to it).
    host: CString,
    handshaken: bool,
    /// The device's TLS slot it holds: dropped last, after mbedTLS gave
    /// its memory back.
    _slot: Slot,
}

/// mbedTLS sends: into the outgoing pipe, as much as there is room for.
unsafe extern "C" fn send(ctx: *mut c_void, buf: *const c_uchar, len: usize) -> c_int {
    // SAFETY: `ctx` is the session's pipes, which outlive its contexts.
    let pipes = unsafe { &mut *ctx.cast::<Pipes>() };
    let room = OUT_LIMIT.saturating_sub(pipes.outgoing.len()).min(len);
    if room == 0 {
        return MBEDTLS_ERR_SSL_WANT_WRITE;
    }
    // SAFETY: mbedTLS passes `len` readable bytes.
    pipes
        .outgoing
        .extend_from_slice(unsafe { core::slice::from_raw_parts(buf, room) });
    room as c_int
}

/// mbedTLS receives: from the incoming pipe.
unsafe extern "C" fn receive(ctx: *mut c_void, buf: *mut c_uchar, len: usize) -> c_int {
    // SAFETY: as in `send`.
    let pipes = unsafe { &mut *ctx.cast::<Pipes>() };
    let available = &pipes.incoming[pipes.taken..];
    if available.is_empty() {
        // An end of the connection is the end of the stream (0).
        return if pipes.ended {
            0
        } else {
            MBEDTLS_ERR_SSL_WANT_READ
        };
    }
    let n = available.len().min(len);
    // SAFETY: mbedTLS passes room for `len` bytes.
    unsafe { core::slice::from_raw_parts_mut(buf, n) }.copy_from_slice(&available[..n]);
    pipes.taken += n;
    if pipes.taken == pipes.incoming.len() {
        pipes.incoming.clear();
        pipes.taken = 0;
    }
    n as c_int
}

/// mbedTLS's verify callback, for each certificate of the chain, from its
/// top down, `ctx` the session's [`Checks`] ([`Checks::flags`] says what
/// becomes of the findings). mbedTLS knows no trusted certificates (see
/// [`Contexts::no_roots`]), so it finds the top untrusted: it is trusted if
/// it is one of the roots itself (sent along, maybe signed by another, as
/// a cross-signed root is), or if one of the roots signed it. Every other
/// finding stays.
unsafe extern "C" fn verify(
    ctx: *mut c_void,
    crt: *mut mbedtls_x509_crt,
    depth: c_int,
    flags: *mut u32,
) -> c_int {
    if ctx.is_null() || crt.is_null() || flags.is_null() {
        return 0;
    }
    // SAFETY: mbedTLS passes a parsed certificate and its flags; `ctx` is the
    // session's checks, which outlive its contexts.
    unsafe {
        let checks = &mut *ctx.cast::<Checks>();
        let crt = &*crt;
        *flags = checks.flags(
            u32::try_from(depth).unwrap_or(u32::MAX),
            || {
                let der = core::slice::from_raw_parts(crt.raw.p, crt.raw.len);
                // Its SubjectPublicKeyInfo, whole: what browsers pin a key by.
                let key = core::slice::from_raw_parts(crt.pk_raw.p, crt.pk_raw.len);
                (
                    CertificateHash::new(paperpaw_digest::sha256::digest(der)),
                    KeyHash::new(paperpaw_digest::sha256::digest(key)),
                )
            },
            *flags,
            dates(crt.valid_from, crt.valid_to),
            || crate::roots::is_root(crt) || crate::roots::signed(crt),
        );
    }
    0
}

/// The flags of a certificate valid from `from` to `to`, now; `None`
/// while the time is not known, so its dates go unchecked (see the
/// module).
fn dates(from: mbedtls_x509_time, to: mbedtls_x509_time) -> Option<u32> {
    let now = crate::clock::unix_ms(crate::now()).map(|ms| ms / 1000)?;
    let seconds = |t: mbedtls_x509_time| {
        paperpaw_x4::sntp::unix_seconds(
            t.year,
            t.mon as u32,
            t.day as u32,
            t.hour as u32,
            t.min as u32,
            t.sec as u32,
        )
    };
    Some(if now < seconds(from) {
        MBEDTLS_X509_BADCERT_FUTURE
    } else if now > seconds(to) {
        MBEDTLS_X509_BADCERT_EXPIRED
    } else {
        0
    })
}

/// `Ok` for mbedTLS's results that are not errors; `Err` with the code.
fn check(code: c_int) -> Result<c_int, Error> {
    if code < 0 {
        Err(Error::Tls(code))
    } else {
        Ok(code)
    }
}

impl Session {
    /// A client session in `slot` for the server `name` (for its
    /// certificate, which `trust` says how to check, and SNI).
    pub fn new(name: &str, trust: &Trust, slot: Slot) -> Result<Self, Error> {
        init();
        let host = CString::new(name).map_err(|_| Error::Tls(0))?;
        let mut session = Self {
            contexts: zeroed_contexts().ok_or(Error::Memory)?,
            pipes: Box::default(),
            checks: Box::new(Checks::new(trust)),
            host,
            handshaken: false,
            _slot: slot,
        };
        let checks = core::ptr::from_mut::<Checks>(&mut *session.checks).cast::<c_void>();
        let c = &mut *session.contexts;
        let bio = core::ptr::from_mut::<Pipes>(&mut *session.pipes).cast::<c_void>();
        // SAFETY: mbedTLS's set-up of its contexts, which stay where they are
        // (in their box): first their `init`, so that `Drop` may free them
        // after any failure below; the pipes and the host's name outlive them.
        unsafe {
            mbedtls_ssl_init(&mut c.ssl);
            mbedtls_ssl_config_init(&mut c.conf);
            mbedtls_x509_crt_init(&mut c.no_roots);
            check(mbedtls_ssl_config_defaults(
                &mut c.conf,
                MBEDTLS_SSL_IS_CLIENT as c_int,
                MBEDTLS_SSL_TRANSPORT_STREAM as c_int,
                MBEDTLS_SSL_PRESET_DEFAULT as c_int,
            ))?;
            mbedtls_ssl_conf_authmode(&mut c.conf, MBEDTLS_SSL_VERIFY_REQUIRED as c_int);
            mbedtls_ssl_conf_ca_chain(&mut c.conf, &mut c.no_roots, core::ptr::null_mut());
            mbedtls_ssl_conf_verify(&mut c.conf, Some(verify), checks);
            mbedtls_ssl_conf_rng(&mut c.conf, Some(random), core::ptr::null_mut());
            // Allocates the record buffers (about 33 KB).
            check(mbedtls_ssl_setup(&mut c.ssl, &c.conf)).map_err(|e| match e {
                Error::Tls(MBEDTLS_ERR_SSL_ALLOC_FAILED) => Error::Memory,
                e => e,
            })?;
            check(mbedtls_ssl_set_hostname(&mut c.ssl, session.host.as_ptr()))?;
            mbedtls_ssl_set_bio(&mut c.ssl, bio, Some(send), Some(receive), None);
        }
        Ok(session)
    }

    /// The server's own certificate, as far as the handshake showed it.
    pub fn leaf(&self) -> Option<Leaf> {
        self.checks.leaf()
    }

    /// How many more received bytes it takes now: the rest stays in the
    /// connection's buffer, not twice in memory.
    pub fn room(&self) -> usize {
        IN_LIMIT.saturating_sub(self.pipes.incoming.len() - self.pipes.taken)
    }

    /// Bytes received over the connection, at most [`Session::room`].
    pub fn feed(&mut self, data: &[u8]) {
        let pipes = &mut *self.pipes;
        // What mbedTLS took goes first, so that the pipe holds no more
        // than `IN_LIMIT`.
        if pipes.taken > 0 {
            pipes.incoming.drain(..pipes.taken);
            pipes.taken = 0;
        }
        pipes.incoming.extend_from_slice(data);
    }

    /// The other side closed the connection.
    pub fn ended(&mut self) {
        self.pipes.ended = true;
    }

    /// Bytes to send over the connection; say how many went with
    /// [`Session::sent`].
    pub fn output(&self) -> &[u8] {
        &self.pipes.outgoing
    }

    pub fn sent(&mut self, n: usize) {
        self.pipes
            .outgoing
            .drain(..n.min(self.pipes.outgoing.len()));
    }

    /// Where it is, for the log.
    pub fn describe(&self) -> alloc::string::String {
        alloc::format!(
            "handshaken {} in {} out {} ended {}",
            self.handshaken,
            self.pipes.incoming.len() - self.pipes.taken,
            self.pipes.outgoing.len(),
            self.pipes.ended
        )
    }

    /// Moves the handshake on: `true` once it is done.
    pub fn handshake(&mut self) -> Result<bool, Error> {
        if self.handshaken {
            return Ok(true);
        }
        // SAFETY: the session's context, set up in `new`.
        let code = unsafe { mbedtls_ssl_handshake(&mut self.contexts.ssl) };
        match code {
            0 => {
                self.handshaken = true;
                Ok(true)
            }
            MBEDTLS_ERR_SSL_WANT_READ | MBEDTLS_ERR_SSL_WANT_WRITE => Ok(false),
            code => {
                // SAFETY: as above.
                let flags = unsafe { mbedtls_ssl_get_verify_result(&self.contexts.ssl) };
                Err(if flags & MBEDTLS_X509_BADCERT_NOT_TRUSTED != 0 {
                    Error::Untrusted
                } else if flags & MBEDTLS_X509_BADCERT_CN_MISMATCH != 0 {
                    Error::OtherHost
                } else if flags & (MBEDTLS_X509_BADCERT_EXPIRED | MBEDTLS_X509_BADCERT_FUTURE) != 0
                {
                    Error::Dates
                } else {
                    Error::Tls(code)
                })
            }
        }
    }

    /// Writes plain bytes (after the handshake); how many were taken, 0
    /// when there is no room now.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        if data.is_empty() {
            return Ok(0);
        }
        // SAFETY: as in `handshake`; `data` is readable.
        let code = unsafe { mbedtls_ssl_write(&mut self.contexts.ssl, data.as_ptr(), data.len()) };
        match code {
            MBEDTLS_ERR_SSL_WANT_READ | MBEDTLS_ERR_SSL_WANT_WRITE => Ok(0),
            code => check(code).map(|n| n as usize),
        }
    }

    /// Reads plain bytes (after the handshake).
    pub fn read(&mut self, out: &mut [u8]) -> Result<Read, Error> {
        // SAFETY: as in `handshake`; `out` is writable.
        let code = unsafe { mbedtls_ssl_read(&mut self.contexts.ssl, out.as_mut_ptr(), out.len()) };
        match code {
            0 | MBEDTLS_ERR_SSL_PEER_CLOSE_NOTIFY | MBEDTLS_ERR_SSL_CONN_EOF => Ok(Read::Closed),
            MBEDTLS_ERR_SSL_WANT_READ
            | MBEDTLS_ERR_SSL_WANT_WRITE
            | MBEDTLS_ERR_SSL_RECEIVED_NEW_SESSION_TICKET => Ok(Read::Wait),
            code => check(code).map(|n| Read::Data(n as usize)),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the contexts set up in `new`, freed once; the pipes they
        // point to are still there.
        unsafe {
            let c = &mut *self.contexts;
            if self.handshaken {
                // Polite, not waited for.
                let _ = mbedtls_ssl_close_notify(&mut c.ssl);
            }
            mbedtls_ssl_free(&mut c.ssl);
            mbedtls_ssl_config_free(&mut c.conf);
            mbedtls_x509_crt_free(&mut c.no_roots);
        }
    }
}
