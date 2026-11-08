//! The socket table of `core.sockets`: ids, owners, the pool charge, the send
//! queue, one read at a time, the idle time and the routing of the event 106.
use super::super::host_services::Caller;
use super::super::net_pool::{DEMAND_SOCK, NetPool};
use super::super::net_service::gate;
use super::{
    Connect, IDLE_DEFAULT_MS, IDLE_MAX_MS, NO_NETWORK_MS, PIECE, PLUGIN_ENTRIES, PLUGIN_SOCKETS,
    READ_MAX, SockHost, TIMEOUT_DEFAULT_MS, TIMEOUT_MAX_MS, kind, reason,
    wire::{self, Record},
};
mod file;
use crate::native_host::codec;
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::{RefCell, RefMut};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};

/// `sock_connect.flags` bit 0: TLS from the start.
const FLAG_TLS: u32 = 1;
/// `sock_read.flags` bit 0: wait for exactly `max` bytes.
const FLAG_EXACT: u32 = 1;
/// `sock_read.mode`.
const MODE_BYTES: u32 = 0;
const MODE_FILE: u32 = 1;
/// The most pins after a `sock_connect` (2.1).
const PINS_MAX: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Waiting for a network (and the power rules).
    Waiting,
    /// The transport is connecting.
    Connecting,
    /// Connected: sends and reads work.
    Open,
    /// Over; its remaining events are not yet told.
    Ended,
}

/// An event that waits to be told: its order and its time.
#[derive(Clone, Copy)]
struct Slot {
    stamp: u64,
    at: u64,
}

/// The read the plugin asked for.
#[derive(Clone, Copy)]
struct Want {
    max: u32,
    exact: bool,
}

struct Sock {
    id: u32,
    owner: u64,
    token: u32,
    screen: Option<u64>,
    run: Option<u64>,
    state: State,
    target: String,
    port: u32,
    timeout_ms: u32,
    idle_ms: u32,
    waiting_since: u64,
    deadline: u64,
    last_move: u64,
    /// Pool bytes held for the send queue and for the read part.
    charge_queue: u32,
    charge_read: u32,
    queue: Vec<u8>,
    inbox: Vec<u8>,
    want: Option<Want>,
    /// The transport may report the wanted bytes (the read part is free).
    granted: bool,
    /// The peer closed its side.
    eof: bool,
    /// The `received` callback runs: the bytes are readable.
    held: bool,
    sent_due: bool,
    connected: Option<Slot>,
    received: Option<(Slot, u32)>,
    sent: Option<Slot>,
    saved: Option<Slot>,
    end: Option<(Slot, u32, u32)>,
    /// The read to a file under way.
    file: Option<file::File>,
}

impl Sock {
    fn live(&self) -> bool {
        self.state != State::Ended
    }

    fn pending(&self) -> bool {
        self.connected.is_some()
            || self.received.is_some()
            || self.sent.is_some()
            || self.saved.is_some()
            || self.end.is_some()
    }
}

#[derive(Default)]
pub(super) struct Inner {
    stamp: u64,
    /// Pool bytes this table holds: what it reserved and has not released.
    ledger: u32,
    next_id: u32,
    socks: Vec<Sock>,
    demand: bool,
}

/// The socket table of one device, and the device's network pool it charges
/// (shared with `core.net`, see [`NetPool`]).
pub(crate) struct SockTable {
    inner: RefCell<Inner>,
    pool: Rc<NetPool>,
}

impl SockTable {
    /// A table charging `pool`: the same `Rc` as `core.net`'s table.
    pub(crate) fn new(pool: Rc<NetPool>) -> Self {
        Self {
            inner: RefCell::new(Inner::default()),
            pool,
        }
    }

    /// The pool this table charges.
    #[cfg(test)]
    pub(crate) fn pool(&self) -> &Rc<NetPool> {
        &self.pool
    }

    /// The sockets held (ended ones included) and the id of the newest.
    #[cfg(test)]
    pub(super) fn held(&self) -> (usize, Option<u32>) {
        let inner = self.inner.borrow();
        (inner.socks.len(), inner.socks.last().map(|s| s.id))
    }

    fn lock(&self) -> Result<RefMut<'_, Inner>, i32> {
        self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)
    }
}

impl Drop for SockTable {
    /// The table goes with its payloads, and so do its charges, exactly.
    fn drop(&mut self) {
        let held = core::mem::take(&mut self.inner.get_mut().ledger);
        self.pool.release(held);
    }
}

fn table(host: &dyn SockHost) -> Result<RefMut<'_, Inner>, i32> {
    host.table().lock()
}

impl Inner {
    fn tick_stamp(&mut self) -> u64 {
        self.stamp += 1;
        self.stamp
    }

    fn slot(&mut self, host: &dyn SockHost) -> Slot {
        Slot {
            stamp: self.tick_stamp(),
            at: host.now_ms(),
        }
    }

    fn reserve(&mut self, host: &dyn SockHost, bytes: u32) -> bool {
        let pool = &host.table().pool;
        if !pool.reserve(bytes) {
            return false;
        }
        if pool.capacity() != 0 {
            self.ledger = self.ledger.saturating_add(bytes);
        }
        true
    }

    fn release(&mut self, host: &dyn SockHost, bytes: u32) {
        // Never more than this table holds: the rest of the pool is not its.
        let give = bytes.min(self.ledger);
        self.ledger -= give;
        host.table().pool.release(give);
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.socks.iter().position(|s| s.id == id)
    }

    /// The live socket `id` of `owner`: another plugin's, an ended or an
    /// unknown one is not found.
    fn mine(&self, owner: u64, id: u32) -> Result<usize, i32> {
        self.index(id)
            .filter(|&i| self.socks[i].owner == owner && self.socks[i].live())
            .ok_or(ERR_NOT_FOUND)
    }

    /// `network_demand` follows whether any socket is live.
    fn sync_demand(&mut self, host: &dyn SockHost) {
        let active = self.socks.iter().any(Sock::live);
        if active != self.demand {
            self.demand = active;
            // The shell hears the OR of NET and SOCK, once per change.
            if let Some(wanted) = host.table().pool.demand(DEMAND_SOCK, active) {
                host.network_demand(wanted);
            }
        }
    }

    fn admit(&self, host: &dyn SockHost, owner: u64) -> Result<(), i32> {
        let live = self.socks.iter().filter(|s| s.live()).count();
        let mine_live = self
            .socks
            .iter()
            .filter(|s| s.owner == owner && s.live())
            .count();
        let mine = self.socks.iter().filter(|s| s.owner == owner).count();
        if mine_live >= PLUGIN_SOCKETS
            || mine >= PLUGIN_ENTRIES
            || live >= host.profile().sockets as usize
        {
            return Err(ERR_BUSY);
        }
        Ok(())
    }

    /// Gives the read part back (the bytes are not needed any more).
    fn free_inbox(&mut self, host: &dyn SockHost, index: usize) {
        let s = &mut self.socks[index];
        s.inbox = Vec::new();
        s.held = false;
        let back = core::mem::take(&mut s.charge_read);
        self.release(host, back);
    }

    /// End socket `index` once with `kind` and `reason`; a second call finds
    /// it ended and does nothing. `abort`: the transport still has it.
    fn end(&mut self, host: &dyn SockHost, index: usize, kind: u32, reason: u32, abort: bool) {
        if !self.socks[index].live() {
            return;
        }
        if abort && matches!(self.socks[index].state, State::Connecting | State::Open) {
            host.transport().close(self.socks[index].id);
        }
        self.drop_file(host, index);
        let slot = self.slot(host);
        let s = &mut self.socks[index];
        s.state = State::Ended;
        s.end = Some((slot, kind, reason));
        s.sent = None;
        s.sent_due = false;
        s.want = None;
        s.granted = false;
        s.queue = Vec::new();
        let queue = core::mem::take(&mut s.charge_queue);
        // Bytes the plugin is yet to be told of stay until it has been.
        let keep = s.received.is_some() || s.held;
        self.release(host, queue);
        if !keep {
            self.free_inbox(host, index);
        }
    }

    /// Start the sockets that wait, in order.
    fn pump(&mut self, host: &dyn SockHost) {
        let now = host.now_ms();
        let up = host.network_up();
        for index in 0..self.socks.len() {
            if self.socks[index].state != State::Waiting {
                continue;
            }
            let (owner, background) = (self.socks[index].owner, self.socks[index].run.is_some());
            if let Some(end) = host.power_end(owner, background) {
                self.end(host, index, kind::FAILED, end.reason, false);
                continue;
            }
            if !up {
                if now.saturating_sub(self.socks[index].waiting_since) >= NO_NETWORK_MS {
                    self.end(host, index, kind::FAILED, reason::NO_NETWORK, false);
                }
                continue;
            }
            let s = &self.socks[index];
            let to = Connect {
                host: &s.target,
                port: s.port,
                timeout_ms: s.timeout_ms,
            };
            match host.transport().connect(s.id, &to) {
                Ok(()) => {
                    let s = &mut self.socks[index];
                    s.state = State::Connecting;
                    s.deadline = now.saturating_add(u64::from(s.timeout_ms));
                    s.last_move = now;
                }
                Err(error) => {
                    let why = if error == ERR_NO_MEMORY {
                        reason::NO_MEMORY
                    } else {
                        reason::IO
                    };
                    self.end(host, index, kind::FAILED, why, false);
                }
            }
        }
    }

    /// Hand the queue to the transport as far as it takes it; `sent` once it
    /// is empty after a `send`.
    fn flush(&mut self, host: &dyn SockHost, index: usize) {
        loop {
            let s = &self.socks[index];
            if s.state != State::Open || s.queue.is_empty() {
                break;
            }
            match host.transport().send(s.id, &s.queue) {
                Ok(0) => break,
                Ok(taken) => {
                    let now = host.now_ms();
                    let s = &mut self.socks[index];
                    let taken = taken.min(s.queue.len());
                    s.queue.drain(..taken);
                    s.last_move = now;
                }
                Err(_) => {
                    self.end(host, index, kind::FAILED, reason::IO, true);
                    return;
                }
            }
        }
        let s = &self.socks[index];
        if s.state == State::Open && s.queue.is_empty() && s.sent_due {
            let slot = self.slot(host);
            let s = &mut self.socks[index];
            s.sent = Some(slot);
            s.sent_due = false;
        }
    }

    /// Let the transport report the wanted bytes once the read part is free.
    fn grant(&mut self, host: &dyn SockHost, index: usize) {
        let s = &mut self.socks[index];
        if let Some(want) = s.want
            && !s.granted
            && s.inbox.is_empty()
            && !s.held
            && s.received.is_none()
        {
            s.granted = true;
            let left = want.max;
            host.transport().read(s.id, left);
        }
    }

    /// The peer has closed and the plugin waits for a read: it gets what came
    /// (a partial exact read) or an empty read, then the socket closes.
    fn settle_eof(&mut self, host: &dyn SockHost, index: usize) {
        let s = &self.socks[index];
        if s.eof && s.state == State::Open && (s.file.is_some() || s.want.is_some_and(|w| w.exact))
        {
            // A file read or an exact read the peer cut short: all or nothing.
            self.end(host, index, kind::FAILED, reason::CLOSED_EARLY, true);
            return;
        }
        if !s.eof || s.want.is_none() || s.received.is_some() || s.held || s.state != State::Open {
            return;
        }
        let len = s.inbox.len() as u32;
        let slot = self.slot(host);
        let s = &mut self.socks[index];
        s.want = None;
        s.granted = false;
        s.received = Some((slot, len));
        if len == 0 {
            self.end(host, index, kind::CLOSED, reason::NONE, true);
        }
    }

    fn expire(&mut self, host: &dyn SockHost) {
        let now = host.now_ms();
        for index in 0..self.socks.len() {
            let s = &self.socks[index];
            let over = match s.state {
                State::Connecting => now >= s.deadline,
                State::Open => now.saturating_sub(s.last_move) >= u64::from(s.idle_ms),
                _ => false,
            };
            if over {
                self.end(host, index, kind::FAILED, reason::TIMEOUT, true);
            }
        }
    }
}

/// A name of letters, digits, `-` and `.` in non-empty labels, an IPv4
/// address, or a bracketed IPv6 address (2.1).
fn host_ok(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    if let Some(inner) = host.strip_prefix('[') {
        return inner.strip_suffix(']').is_some_and(|v6| {
            !v6.is_empty()
                && v6
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        });
    }
    host.split('.').all(|label| {
        !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

fn owned(text: &str) -> Result<String, i32> {
    gate()?;
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(text);
    Ok(copy)
}

fn buffer(capacity: u32) -> Result<Vec<u8>, i32> {
    gate()?;
    let mut v = Vec::new();
    v.try_reserve_exact(capacity as usize)
        .map_err(|_| ERR_NO_MEMORY)?;
    Ok(v)
}

/// `connect` (2.1): a plain socket. Everything that can fail comes before the
/// first change.
pub(super) fn connect(host: &dyn SockHost, who: &Caller<'_>, input: &[u8]) -> Result<u32, i32> {
    let mut head = None;
    let mut pins = 0_usize;
    wire::host_records_visit(input, 1 + PINS_MAX, &mut |record| {
        match record {
            Record::ServiceSockConnect(c) if head.is_none() => head = Some(c),
            Record::ServiceSockPin(p)
                if head.is_some() && pins < PINS_MAX && p.kind <= 1 && p.hash.len() == 32 =>
            {
                pins += 1;
            }
            _ => return Err(ERR_INVALID),
        }
        Ok(())
    })?;
    let c = head.ok_or(ERR_INVALID)?;
    if c.flags & !FLAG_TLS != 0
        || c.port == 0
        || c.port > 65_535
        || !host_ok(c.host)
        || c.timeout_ms > TIMEOUT_MAX_MS
        || c.idle_ms > IDLE_MAX_MS
        || (c.flags & FLAG_TLS == 0 && pins != 0)
    {
        return Err(ERR_INVALID);
    }
    if c.flags & FLAG_TLS != 0 {
        // TLS needs the shell's session and trust store: a later slice.
        return Err(ERR_UNSUPPORTED);
    }
    if !host.subscribed(who.owner, wire::EVENT_SOCKET_TAG) {
        return Err(ERR_STATE);
    }
    let profile = host.profile();
    let target = owned(c.host)?;
    let queue = buffer(profile.queue)?;
    let inbox = buffer(profile.read)?;
    let mut inner = table(host)?;
    inner.admit(host, who.owner)?;
    gate()?;
    inner.socks.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    let id = inner.next_id.checked_add(1).ok_or(ERR_FULL)?;
    if !inner.reserve(host, profile.charge()) {
        return Err(ERR_NO_MEMORY);
    }
    let pooled = host.table().pool.capacity() != 0;
    let now = host.now_ms();
    inner.next_id = id;
    inner.socks.push(Sock {
        id,
        owner: who.owner,
        token: c.token,
        screen: host.showing_screen(who.owner),
        run: host.current_run(who.owner),
        state: State::Waiting,
        target,
        port: c.port,
        timeout_ms: if c.timeout_ms == 0 {
            TIMEOUT_DEFAULT_MS
        } else {
            c.timeout_ms
        },
        idle_ms: if c.idle_ms == 0 {
            IDLE_DEFAULT_MS
        } else {
            c.idle_ms
        },
        waiting_since: now,
        deadline: 0,
        last_move: now,
        charge_queue: if pooled { profile.queue } else { 0 },
        charge_read: if pooled { profile.read } else { 0 },
        queue,
        inbox,
        want: None,
        granted: false,
        eof: false,
        held: false,
        sent_due: false,
        connected: None,
        received: None,
        sent: None,
        saved: None,
        end: None,
        file: None,
    });
    inner.sync_demand(host);
    inner.pump(host);
    Ok(id)
}

/// `send` (2.2).
pub(super) fn send(
    host: &dyn SockHost,
    owner: u64,
    s: &wire::ServiceSockSend<'_>,
) -> Result<(), i32> {
    if s.data.is_empty() || s.data.len() > PIECE {
        return Err(ERR_INVALID);
    }
    let mut inner = table(host)?;
    let index = inner.mine(owner, s.socket)?;
    let sock = &mut inner.socks[index];
    if sock.state != State::Open {
        return Err(ERR_STATE);
    }
    if sock.queue.len() + s.data.len() > host.profile().queue as usize {
        return Err(ERR_BUSY);
    }
    // The queue's room was allocated at `connect`: this never grows it.
    sock.queue.extend_from_slice(s.data);
    sock.sent_due = true;
    inner.flush(host, index);
    inner.sync_demand(host);
    Ok(())
}

/// `read` (2.3): bytes, or a file.
pub(super) fn read(
    host: &dyn SockHost,
    who: &Caller<'_>,
    r: &wire::ServiceSockRead<'_>,
) -> Result<(), i32> {
    let owner = who.owner;
    match r.mode {
        MODE_BYTES => {}
        MODE_FILE => return file::read(host, who, r),
        _ => return Err(ERR_INVALID),
    }
    let cap = host.profile().read;
    let exact = r.flags & FLAG_EXACT != 0;
    if r.flags & !FLAG_EXACT != 0
        || r.max == 0
        || r.max > READ_MAX
        || r.len != 0
        || r.root != 0
        || !r.path.is_empty()
        || (exact && r.max > cap)
    {
        return Err(ERR_INVALID);
    }
    let mut inner = table(host)?;
    let index = inner.mine(owner, r.socket)?;
    let sock = &mut inner.socks[index];
    if sock.state != State::Open {
        return Err(ERR_STATE);
    }
    if sock.want.is_some() || sock.received.is_some() {
        return Err(ERR_BUSY);
    }
    sock.want = Some(Want {
        max: r.max.min(cap),
        exact,
    });
    inner.settle_eof(host, index);
    inner.grant(host, index);
    inner.sync_demand(host);
    Ok(())
}

/// `data` (2.3): the bytes of the `received` event whose callback runs.
pub(super) fn data(
    host: &dyn SockHost,
    owner: u64,
    at: &wire::ServiceSockAt,
    out: &mut [u8],
) -> Result<usize, i32> {
    let inner = table(host)?;
    let index = inner
        .index(at.socket)
        .filter(|&i| inner.socks[i].owner == owner)
        .ok_or(ERR_NOT_FOUND)?;
    let sock = &inner.socks[index];
    if !sock.held {
        return Err(ERR_STATE);
    }
    let from = (at.offset as usize).min(sock.inbox.len());
    // A bare reply: the record's frame must fit the caller's room.
    let mut take = (sock.inbox.len() - from).min(PIECE);
    loop {
        let bytes = sock.inbox.get(from..from + take).ok_or(ERR_INVALID)?;
        match codec::encode_one(
            &Record::ServiceSockBytes(wire::ServiceSockBytes { data: bytes }),
            out,
        ) {
            Err(paperpaw_native::host_glue::ERR_NO_SPACE) if take > 0 => take /= 2,
            other => return other,
        }
    }
}

/// `close` (2.1): a closed or unknown socket is `OK`.
pub(super) fn close(host: &dyn SockHost, owner: u64, socket: u32) -> Result<(), i32> {
    let mut inner = table(host)?;
    if let Ok(index) = inner.mine(owner, socket) {
        inner.end(host, index, kind::CLOSED, reason::NONE, true);
        inner.sync_demand(host);
    }
    Ok(())
}

/// The transport has connected socket `id`.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed: call again on the next turn.
pub(crate) fn connected(host: &dyn SockHost, id: u32) -> Result<(), i32> {
    let mut inner = table(host)?;
    let Some(index) = inner.index(id) else {
        return Ok(());
    };
    if inner.socks[index].state == State::Connecting {
        let slot = inner.slot(host);
        let s = &mut inner.socks[index];
        s.state = State::Open;
        s.last_move = slot.at;
        s.connected = Some(slot);
    }
    Ok(())
}

/// Bytes arrived for the read of socket `id`; how many were taken (the rest
/// stays with the transport: nothing beyond the read's `max` is held).
///
/// # Errors
/// `ERR_BUSY` (the table is borrowed), `ERR_STATE` (no read was asked for).
pub(crate) fn received(host: &dyn SockHost, id: u32, bytes: &[u8]) -> Result<usize, i32> {
    let mut inner = table(host)?;
    let index = inner.index(id).ok_or(ERR_NOT_FOUND)?;
    let now = host.now_ms();
    let s = &mut inner.socks[index];
    let want = s.want.filter(|_| s.granted && s.state == State::Open);
    let Some(want) = want else {
        return Err(ERR_STATE);
    };
    if s.file.is_some() {
        return inner.file_received(host, index, bytes);
    }
    let room = (want.max as usize).saturating_sub(s.inbox.len());
    let taken = bytes.len().min(room);
    s.inbox.extend_from_slice(bytes.get(..taken).unwrap_or(&[]));
    if taken > 0 {
        s.last_move = now;
    }
    let done = if want.exact {
        s.inbox.len() >= want.max as usize
    } else {
        !s.inbox.is_empty()
    };
    if done {
        let len = s.inbox.len() as u32;
        s.want = None;
        s.granted = false;
        let slot = inner.slot(host);
        inner.socks[index].received = Some((slot, len));
    }
    Ok(taken)
}

/// The peer of socket `id` closed its side.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn peer_closed(host: &dyn SockHost, id: u32) -> Result<(), i32> {
    let mut inner = table(host)?;
    if let Some(index) = inner.index(id) {
        inner.socks[index].eof = true;
        inner.settle_eof(host, index);
        inner.sync_demand(host);
    }
    Ok(())
}

/// The connection of socket `id` can take more bytes.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn writable(host: &dyn SockHost, id: u32) -> Result<(), i32> {
    let mut inner = table(host)?;
    if let Some(index) = inner.index(id) {
        inner.flush(host, index);
    }
    Ok(())
}

/// The transport lost socket `id` (or could not connect): `reason` is one of
/// [`reason`]. The socket ends `failed`; the transport has dropped it.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn failed(host: &dyn SockHost, id: u32, why: u32) -> Result<(), i32> {
    let mut inner = table(host)?;
    if let Some(index) = inner.index(id) {
        inner.end(host, index, kind::FAILED, why, false);
        inner.sync_demand(host);
    }
    Ok(())
}

/// The shell's loop: connect and idle times, sockets that wait.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn tick(host: &dyn SockHost) -> Result<(), i32> {
    let mut inner = table(host)?;
    inner.expire(host);
    inner.pump(host);
    inner.sync_demand(host);
    Ok(())
}

/// The network went away or the device sleeps: every socket ends
/// `failed` `no_network`.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn network_lost(host: &dyn SockHost) -> Result<(), i32> {
    let mut inner = table(host)?;
    for index in 0..inner.socks.len() {
        inner.end(host, index, kind::FAILED, reason::NO_NETWORK, true);
    }
    inner.sync_demand(host);
    Ok(())
}

/// A screen instance closed: its sockets end `closed` `screen_closed`.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn screen_closed(host: &dyn SockHost, owner: u64, instance: u64) -> Result<(), i32> {
    let mut inner = table(host)?;
    for index in 0..inner.socks.len() {
        let s = &inner.socks[index];
        if s.owner == owner && s.screen == Some(instance) {
            inner.end(host, index, kind::CLOSED, reason::SCREEN_CLOSED, true);
        }
    }
    inner.sync_demand(host);
    Ok(())
}

/// A work run reached its longest time: its sockets end `closed` `work_ended`.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn work_ended(host: &dyn SockHost, owner: u64, run: u64) -> Result<(), i32> {
    let mut inner = table(host)?;
    for index in 0..inner.socks.len() {
        let s = &inner.socks[index];
        if s.owner == owner && s.run == Some(run) {
            inner.end(host, index, kind::CLOSED, reason::WORK_ENDED, true);
        }
    }
    inner.sync_demand(host);
    Ok(())
}

#[derive(Clone, Copy)]
enum Pick {
    Connected,
    Received,
    Sent,
    Saved,
    End,
}

/// The oldest event 106 of `owner`, encoded into `out`. `Ok(None)` when there
/// is none. A `received` event makes its bytes readable (`data`) until
/// [`event_delivered`].
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed; `ERR_NO_SPACE` when `out` is too
/// small (nothing was consumed).
pub(crate) fn next_event(
    host: &dyn SockHost,
    owner: u64,
    out: &mut [u8],
) -> Result<Option<usize>, i32> {
    let mut inner = table(host)?;
    let mut best: Option<(u64, usize, Pick)> = None;
    for (index, s) in inner
        .socks
        .iter()
        .enumerate()
        .filter(|(_, s)| s.owner == owner)
    {
        let slots = [
            (s.connected.map(|e| e.stamp), Pick::Connected),
            (s.received.map(|(e, _)| e.stamp), Pick::Received),
            (s.sent.map(|e| e.stamp), Pick::Sent),
            (s.saved.map(|e| e.stamp), Pick::Saved),
            (s.end.map(|(e, _, _)| e.stamp), Pick::End),
        ];
        for (stamp, pick) in slots {
            if let Some(stamp) = stamp
                && best.is_none_or(|(b, _, _)| stamp < b)
            {
                best = Some((stamp, index, pick));
            }
        }
    }
    let Some((_, index, pick)) = best else {
        return Ok(None);
    };
    let s = &inner.socks[index];
    let (at, what, why, len) = match pick {
        Pick::Connected => (s.connected.map_or(0, |e| e.at), kind::CONNECTED, 0, 0),
        Pick::Received => s.received.map_or((0, kind::RECEIVED, 0, 0), |(e, len)| {
            (e.at, kind::RECEIVED, 0, len)
        }),
        Pick::Sent => (s.sent.map_or(0, |e| e.at), kind::SENT, 0, 0),
        // The plugin knows the length it asked for (the field is 32 bits).
        Pick::Saved => (s.saved.map_or(0, |e| e.at), kind::SAVED, 0, 0),
        Pick::End => s
            .end
            .map_or((0, kind::FAILED, 0, 0), |(e, k, r)| (e.at, k, r, 0)),
    };
    let used = codec::encode_one(
        &Record::EventSocket(wire::EventSocket {
            now_ms: at,
            socket: s.id,
            token: s.token,
            kind: what,
            reason: why,
            len,
        }),
        out,
    )?;
    let s = &mut inner.socks[index];
    match pick {
        Pick::Connected => s.connected = None,
        Pick::Received => {
            s.received = None;
            s.held = true;
        }
        Pick::Sent => s.sent = None,
        Pick::Saved => s.saved = None,
        Pick::End => s.end = None,
    }
    Ok(Some(used))
}

/// The callback of `owner` returned: the bytes it could read are freed, a
/// read it asked for meanwhile starts, and the sockets whose last event it
/// was go.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn event_delivered(host: &dyn SockHost, owner: u64) -> Result<(), i32> {
    let mut inner = table(host)?;
    for index in 0..inner.socks.len() {
        if inner.socks[index].owner != owner || !inner.socks[index].held {
            continue;
        }
        if inner.socks[index].live() {
            let s = &mut inner.socks[index];
            s.inbox.clear();
            s.held = false;
            inner.settle_eof(host, index);
            inner.grant(host, index);
        } else {
            inner.free_inbox(host, index);
        }
    }
    // An ended socket with nothing left to tell is gone.
    let mut index = 0;
    while index < inner.socks.len() {
        let s = &inner.socks[index];
        if s.owner == owner && !s.live() && !s.pending() && !s.held {
            let back = s.charge_queue.saturating_add(s.charge_read);
            inner.release(host, back);
            inner.socks.remove(index);
        } else {
            index += 1;
        }
    }
    inner.sync_demand(host);
    Ok(())
}

/// The plugin stopped: its sockets close without events; the pool gets
/// everything back.
pub(super) fn owner_stopped(host: &dyn SockHost, owner: u64) -> Result<(), i32> {
    let mut inner = table(host)?;
    let mut freed = 0_u32;
    let mut index = 0;
    while index < inner.socks.len() {
        if inner.socks[index].owner != owner {
            index += 1;
            continue;
        }
        inner.drop_file(host, index);
        let s = &inner.socks[index];
        if matches!(s.state, State::Connecting | State::Open) {
            host.transport().close(s.id);
        }
        freed = freed
            .saturating_add(s.charge_queue)
            .saturating_add(s.charge_read);
        inner.socks.remove(index);
    }
    inner.release(host, freed);
    inner.sync_demand(host);
    Ok(())
}
