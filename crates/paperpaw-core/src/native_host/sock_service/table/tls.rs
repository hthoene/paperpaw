//! TLS for sockets (`sockets.md` 2.4): the row each socket keeps, `start_tls`,
//! `peer`, the session lease, the shell's reports and the cleanup.
//!
//! The order on every end is: the shell's session first (`end`), then the
//! lease, then the event, so a waiting socket may start at once and the
//! plugin never hears of an end whose session is still held.
use super::{
    Caller, ERR_INVALID, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED, IDLE_DEFAULT_MS, IDLE_MAX_MS,
    Inner, Slot, SockHost, State, TIMEOUT_DEFAULT_MS, TIMEOUT_MAX_MS, gate, host_ok, kind, owned,
    reason, table,
    wire::{self, Record},
};
use crate::native_host::{
    codec,
    net_pool::{TlsHolder, TlsLease},
    sock_service::{PeerInfo, TlsOpen, TlsPin, tls_code},
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::ERR_NO_MEMORY;

/// The most pins (2.4).
pub(super) const PINS_MAX: usize = 4;
/// A secure socket idle this long while another waits for the session ends
/// `session_needed` (2.1).
pub(in crate::native_host::sock_service) const SESSION_IDLE_MS: u64 = 5_000;

/// The pins that came after a `connect` or `start_tls`.
#[derive(Clone, Copy, Default)]
pub(super) struct Pins {
    list: [TlsPin; PINS_MAX],
    count: usize,
}

impl Pins {
    /// Adds one pin; `false` when it is malformed or one too many.
    pub(super) fn push(&mut self, kind: u32, hash: &[u8]) -> bool {
        let (Some(slot), Ok(hash)) = (self.list.get_mut(self.count), <[u8; 32]>::try_from(hash))
        else {
            return false;
        };
        if kind > 1 {
            return false;
        }
        *slot = TlsPin { kind, hash };
        self.count += 1;
        true
    }

    pub(super) fn len(&self) -> usize {
        self.count
    }

    fn as_slice(&self) -> &[TlsPin] {
        self.list.get(..self.count).unwrap_or(&[])
    }
}

/// What only a TLS socket needs, allocated for it alone (about 270 bytes): a
/// plain socket pays the few flags of [`Row`] and one empty `Vec`.
struct Heavy {
    /// The name the certificate must match.
    name: String,
    pins: Pins,
    /// The peer's facts and the reason of an `untrusted`.
    peer: Option<(PeerInfo, u32)>,
}

/// What a socket keeps for TLS.
#[derive(Default)]
pub(super) struct Row {
    /// A TLS socket from `connect` or `start_tls`.
    pub(super) wanted: bool,
    /// When the socket first asked for the session, which is after any wait
    /// for the network: the `busy` clock runs from there.
    pub(super) asked: Option<u64>,
    /// Zero or one: the name, the pins and the peer.
    heavy: Vec<Heavy>,
    /// The session is leased to this socket.
    pub(super) lease: bool,
    /// The shell's session exists (`begin` succeeded and `end` has not run).
    pub(super) begun: bool,
    /// The handshake passed.
    pub(super) secure: bool,
    /// The callback of `secured` or `untrusted` runs: `peer` answers.
    pub(super) peer_open: bool,
    pub(super) secured: Option<Slot>,
}

impl Row {
    /// The row of a TLS socket; `ERR_NO_MEMORY` when its fields do not fit.
    pub(super) fn new(name: String, pins: Pins) -> Result<Self, i32> {
        gate()?;
        let mut heavy = Vec::new();
        heavy.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)?;
        heavy.push(Heavy {
            name,
            pins,
            peer: None,
        });
        Ok(Self {
            wanted: true,
            heavy,
            ..Self::default()
        })
    }

    fn peer_mut(&mut self) -> Option<&mut Option<(PeerInfo, u32)>> {
        self.heavy.first_mut().map(|h| &mut h.peer)
    }
}

/// The reason an error of the shell's `begin` ends the socket with.
fn begin_reason(error: i32) -> u32 {
    if error == ERR_NO_MEMORY {
        reason::NO_MEMORY
    } else {
        reason::TLS
    }
}

/// The name to check: `server_name`, or the host when it is empty.
pub(super) fn name_of(server_name: &str, host: &str) -> Result<String, i32> {
    if server_name.is_empty() {
        return owned(host);
    }
    if !host_ok(server_name) {
        return Err(ERR_INVALID);
    }
    owned(server_name)
}

impl Inner {
    /// Asks for the session for socket `index`: `true` once it is held. Past
    /// `timeout_ms` of waiting the socket ends `busy`; a full queue ends it
    /// `no_memory`. The handshake's time counts from the grant.
    pub(super) fn take_session(&mut self, host: &dyn SockHost, index: usize, now: u64) -> bool {
        let s = &mut self.socks[index];
        if s.tls.lease {
            return true;
        }
        // The wait for the session starts now, not at `connect`: the network
        // may have been away for a while.
        let asked = *s.tls.asked.get_or_insert(now);
        match host.table().pool.tls_request(TlsHolder::Sock(s.id)) {
            TlsLease::Held => {
                s.tls.lease = true;
                s.deadline = now.saturating_add(u64::from(s.timeout_ms));
                true
            }
            TlsLease::Wait => {
                if now.saturating_sub(asked) >= u64::from(s.timeout_ms) {
                    let open = s.state == State::Securing;
                    self.end(host, index, kind::FAILED, reason::BUSY, open);
                }
                false
            }
            TlsLease::Full => {
                let open = s.state == State::Securing;
                self.end(host, index, kind::FAILED, reason::NO_MEMORY, open);
                false
            }
        }
    }

    /// Begins the handshake of socket `index`, which holds the session.
    pub(super) fn begin_tls(&mut self, host: &dyn SockHost, index: usize) {
        let now = host.now_ms();
        let s = &self.socks[index];
        let (Some(shell), Some(h)) = (host.tls(), s.tls.heavy.first()) else {
            self.end(host, index, kind::FAILED, reason::TLS, true);
            return;
        };
        let open = TlsOpen {
            server_name: &h.name,
            pins: h.pins.as_slice(),
        };
        match shell.begin(s.id, &open) {
            Ok(()) => {
                let s = &mut self.socks[index];
                s.tls.begun = true;
                s.state = State::Securing;
                s.last_move = now;
            }
            Err(error) => self.end(host, index, kind::FAILED, begin_reason(error), true),
        }
    }

    /// Starts the handshake of the sockets that switch to TLS and wait for the
    /// session (`start_tls`).
    pub(super) fn pump_session(&mut self, host: &dyn SockHost, index: usize, now: u64) {
        if self.take_session(host, index, now) {
            self.begin_tls(host, index);
        }
    }

    /// The shell's session and the lease go (`end` first, then the lease).
    pub(super) fn release_tls(&mut self, host: &dyn SockHost, index: usize) {
        let s = &mut self.socks[index];
        if !s.tls.wanted {
            return;
        }
        if core::mem::take(&mut s.tls.begun)
            && let Some(shell) = host.tls()
        {
            shell.end(s.id);
        }
        s.tls.lease = false;
        host.table().pool.tls_drop(TlsHolder::Sock(s.id));
    }

    /// A secure socket idle while another waits for the session gives it up.
    pub(super) fn session_needed(&self, host: &dyn SockHost, index: usize, now: u64) -> bool {
        let s = &self.socks[index];
        s.tls.secure
            && s.state == State::Open
            && now.saturating_sub(s.last_move) >= SESSION_IDLE_MS
            && host.table().pool.tls_waiting()
    }

    /// The socket `id` that is in its handshake, or `None` (counted): a
    /// report for an unknown or ended socket changes nothing.
    fn handshaking(&self, host: &dyn SockHost, id: u32) -> Option<usize> {
        let found = self
            .index(id)
            .filter(|&i| self.socks[i].state == State::Securing && self.socks[i].tls.begun);
        if found.is_none() {
            let ignored = &host.table().ignored;
            ignored.set(ignored.get().saturating_add(1));
        }
        found
    }
}

/// `start_tls` (2.4): an open plain socket switches to TLS.
pub(in crate::native_host::sock_service) fn start_tls(
    host: &dyn SockHost,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<(), i32> {
    let mut head = None;
    let mut pins = Pins::default();
    let mut bad = false;
    // One record more than allowed, so a fifth pin is `ERR_INVALID`.
    wire::host_records_visit(input, 2 + PINS_MAX, &mut |record| {
        match record {
            Record::ServiceSockTls(t) if head.is_none() => head = Some(t),
            Record::ServiceSockPin(p) if head.is_some() => bad |= !pins.push(p.kind, p.hash),
            _ => return Err(ERR_INVALID),
        }
        Ok(())
    })?;
    let t = head.ok_or(ERR_INVALID)?;
    if bad
        || t.timeout_ms > TIMEOUT_MAX_MS
        || t.idle_ms > IDLE_MAX_MS
        || (!t.server_name.is_empty() && !host_ok(t.server_name))
    {
        return Err(ERR_INVALID);
    }
    if host.tls().is_none() {
        return Err(ERR_UNSUPPORTED);
    }
    let mut inner = table(host)?;
    let index = inner.mine(who.owner, t.socket)?;
    let s = &inner.socks[index];
    // Nothing the peer sent may be taken as coming over TLS: no read, no
    // unread byte, no queue, no callback holding bytes, and not twice.
    if s.state != State::Open
        || s.tls.wanted
        || s.want.is_some()
        || s.received.is_some()
        || s.held
        || s.eof
        || s.file.is_some()
        || !s.inbox.is_empty()
        || !s.queue.is_empty()
    {
        return Err(ERR_STATE);
    }
    let row = Row::new(name_of(t.server_name, &s.target)?, pins)?;
    let now = host.now_ms();
    let s = &mut inner.socks[index];
    s.tls = row;
    s.state = State::Securing;
    s.sent_due = false;
    s.waiting_since = now;
    s.deadline = 0;
    s.last_move = now;
    s.timeout_ms = if t.timeout_ms == 0 {
        TIMEOUT_DEFAULT_MS
    } else {
        t.timeout_ms
    };
    s.idle_ms = if t.idle_ms == 0 {
        IDLE_DEFAULT_MS
    } else {
        t.idle_ms
    };
    inner.pump_session(host, index, now);
    inner.sync_demand(host);
    Ok(())
}

/// `peer` (2.4): the facts of the certificate during the `secured` or
/// `untrusted` callback.
pub(in crate::native_host::sock_service) fn peer(
    host: &dyn SockHost,
    owner: u64,
    r: &wire::ServiceSockRef,
    out: &mut [u8],
) -> Result<usize, i32> {
    if host.tls().is_none() {
        return Err(ERR_UNSUPPORTED);
    }
    let inner = table(host)?;
    let s = inner
        .index(r.socket)
        .map(|i| &inner.socks[i])
        .filter(|s| s.owner == owner)
        .ok_or(ERR_NOT_FOUND)?;
    let peer = s.tls.heavy.first().and_then(|h| h.peer);
    let (Some((info, why)), true) = (peer, s.tls.peer_open) else {
        return Err(ERR_STATE);
    };
    codec::encode_one(
        &Record::ServiceSockPeer(wire::ServiceSockPeer {
            flags: u32::from(info.time_checked),
            reason: why,
            certificate: &info.certificate,
            key: &info.key,
        }),
        out,
    )
}

/// The handshake of socket `id` passed: it is `secured`.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed: report again on the next turn.
pub(crate) fn secured(host: &dyn SockHost, id: u32, peer: &PeerInfo) -> Result<(), i32> {
    let mut inner = table(host)?;
    if let Some(index) = inner.handshaking(host, id) {
        let slot = inner.slot(host);
        let s = &mut inner.socks[index];
        s.state = State::Open;
        s.tls.secure = true;
        if let Some(slot) = s.tls.peer_mut() {
            *slot = Some((*peer, 0));
        }
        s.tls.secured = Some(slot);
        s.last_move = slot.at;
    }
    Ok(())
}

/// Only the trust failed (`why` is a [`tls_code`] `UNKNOWN_ISSUER` or
/// `NOT_PINNED`; anything else is `failed` `tls`): the socket ends
/// `untrusted` and `peer` answers during that callback.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed: report again on the next turn.
pub(crate) fn untrusted(
    host: &dyn SockHost,
    id: u32,
    why: u32,
    peer: &PeerInfo,
) -> Result<(), i32> {
    let mut inner = table(host)?;
    if let Some(index) = inner.handshaking(host, id) {
        if matches!(why, tls_code::UNKNOWN_ISSUER | tls_code::NOT_PINNED) {
            if let Some(slot) = inner.socks[index].tls.peer_mut() {
                *slot = Some((*peer, why));
            }
            inner.end(host, index, kind::UNTRUSTED, why, true);
        } else {
            inner.end(host, index, kind::FAILED, reason::TLS, true);
        }
        inner.sync_demand(host);
    }
    Ok(())
}

/// The handshake of socket `id` failed: `code` is a [`tls_code`] (an unknown
/// one is `tls`). The socket ends `failed`.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed: report again on the next turn.
pub(crate) fn tls_failed(host: &dyn SockHost, id: u32, code: u32) -> Result<(), i32> {
    let mut inner = table(host)?;
    if let Some(index) = inner.handshaking(host, id) {
        let why = match code {
            tls_code::NAME => reason::NAME,
            tls_code::EXPIRED => reason::EXPIRED,
            tls_code::NO_MEMORY => reason::NO_MEMORY,
            _ => reason::TLS,
        };
        inner.end(host, index, kind::FAILED, why, true);
        inner.sync_demand(host);
    }
    Ok(())
}
