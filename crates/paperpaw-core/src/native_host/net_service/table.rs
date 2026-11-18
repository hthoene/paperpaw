//! The request table of `core.net`: ids, tokens, the queue, the pool, the
//! held answers and the routing of events 99 and 100.
use super::super::host_services::Caller;
use super::super::net_pool::{DEMAND_NET, NetPool, TlsHolder, TlsLease};
use super::{
    BODY_PIECE, DEVICE_REQUESTS, DONE_LATER, DONE_SAVED, DONE_TRUNCATED, ERROR_BODY,
    FILE_TIMEOUT_MS, IO_BUFFER, MEMORY_TIMEOUT_MS, NO_NETWORK_MS, NetHost, PLUGIN_REQUESTS, Start,
    args, gate, reason, serving,
    wire::{self, Record},
};
use crate::native_host::codec;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::{RefCell, RefMut};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Waiting for room, a connection or a network.
    Queued,
    /// The transport has it.
    Running,
    /// Ended; the event is not yet told.
    Done,
    /// Ended and told: the plugin's callback runs, the answer is readable.
    Held,
}

/// A progress event waiting to be told (the newest replaces the older).
#[derive(Clone, Copy)]
struct Pending {
    stamp: u64,
    received: u64,
    total: u64,
    flags: u32,
    at: u64,
}

struct Done {
    stamp: u64,
    at: u64,
    status: u32,
    reason: u32,
    flags: u32,
    /// `etag`, `content_type`, `final_url` back to back.
    head: Vec<u8>,
    etag: u16,
    content_type: u16,
    body: Vec<u8>,
}

struct Req {
    id: u32,
    owner: u64,
    token: u32,
    run: Option<u64>,
    /// `resource_opened` was told for `run` and `resource_closed` is not yet.
    run_open: bool,
    state: State,
    /// The plugin's own input, parsed again when the request starts.
    batch: Box<[u8]>,
    saving: bool,
    /// An `https` request: it holds the device's one TLS session (shared
    /// with `core.sockets`, `NetPool`) from its start to its end.
    tls: bool,
    progress: bool,
    max_body: u32,
    max_file: u64,
    timeout_ms: u32,
    queued_at: u64,
    last_move: u64,
    net_wait: Option<u64>,
    /// Bytes held from the pool.
    reserved: u32,
    /// Pool bytes held for the plugin's input batch while the request lives.
    input: u32,
    sent: Option<(u64, u64)>,
    pending: Option<Pending>,
    done: Option<Done>,
}

/// Which event is next (the oldest of a plugin's by stamp).
#[derive(Clone, Copy)]
pub(super) enum Pick {
    Done(u32),
    Progress(u32),
    Server(usize),
    Served(u32),
}

#[derive(Default)]
pub(super) struct Inner {
    pub(super) stamp: u64,
    /// Pool bytes this table holds: what it reserved and has not released.
    /// The pool is shared (with `core.sockets`), so a table gives back only
    /// its own ledger, never the pool's whole count.
    ledger: u32,
    next_id: u32,
    reqs: Vec<Req>,
    demand: bool,
    pub(super) serving: serving::Tables,
}

/// The request and server tables of one device, and the device's network
/// pool they charge (shared with `core.sockets`, see [`NetPool`]).
pub(crate) struct NetTable {
    inner: RefCell<Inner>,
    pool: Rc<NetPool>,
}

impl NetTable {
    /// Tables charging `pool`; the shell gives the SOCK host a clone of the
    /// same `Rc` ([`Self::pool`]).
    pub(crate) fn new(pool: Rc<NetPool>) -> Self {
        Self {
            inner: RefCell::new(Inner::default()),
            pool,
        }
    }

    /// The device's pool: the one counter NET and SOCK both charge.
    pub(crate) fn pool(&self) -> &Rc<NetPool> {
        &self.pool
    }

    pub(super) fn lock(&self) -> Result<RefMut<'_, Inner>, i32> {
        self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)
    }
}

impl Drop for NetTable {
    /// The tables go: their payloads are freed with them, and so are their
    /// charges, exactly (the pool outlives them and may hold `core.sockets`
    /// reservations that are not the tables').
    fn drop(&mut self) {
        let held = core::mem::take(&mut self.inner.get_mut().ledger);
        self.pool.release(held);
        // A request that goes with the table gives the TLS session back.
        for r in self.inner.get_mut().reqs.iter().filter(|r| r.tls) {
            self.pool.tls_drop(TlsHolder::Net(r.id));
        }
    }
}

/// Whether `url` is an `https` one (the scheme in any case).
fn is_https(url: &str) -> bool {
    url.as_bytes()
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"https://"))
}

impl Inner {
    pub(super) fn tick_stamp(&mut self) -> u64 {
        self.stamp += 1;
        self.stamp
    }

    /// Reserve `bytes` from the pool; false when it has not that room.
    pub(super) fn reserve(&mut self, host: &dyn NetHost, bytes: u32) -> bool {
        let pool = host.table().pool();
        if !pool.reserve(bytes) {
            return false;
        }
        if pool.capacity() != 0 {
            self.ledger = self.ledger.saturating_add(bytes);
        }
        true
    }

    pub(super) fn release(&mut self, host: &dyn NetHost, bytes: u32) {
        // Never more than this table holds: the rest of the pool is not its.
        let give = bytes.min(self.ledger);
        self.ledger -= give;
        host.table().pool().release(give);
    }

    /// The request is done with: its work run is told, once. A request that
    /// was told of its end goes at `event_delivered`, one that was never told
    /// at `owner_stopped`; the run counts it until then.
    fn close_run(host: &dyn NetHost, r: &mut Req) {
        if let Some(run) = r.run
            && core::mem::take(&mut r.run_open)
        {
            host.resource_closed(run);
        }
    }

    fn live(r: &Req) -> bool {
        matches!(r.state, State::Queued | State::Running)
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.reqs.iter().position(|r| r.id == id)
    }

    /// Memory bodies that exist: arriving, and finished but not delivered with
    /// bytes held. A request that failed holds none.
    fn bodies(&self) -> u32 {
        self.reqs
            .iter()
            .filter(|r| {
                !r.saving
                    && match r.state {
                        State::Queued => false,
                        State::Running => true,
                        State::Done | State::Held => {
                            r.done.as_ref().is_some_and(|d| !d.body.is_empty())
                        }
                    }
            })
            .count() as u32
    }

    /// The limits of `request`: per plugin the requests not ended and those
    /// ended but not yet told (a plugin that loops in one callback cannot
    /// pile up ended requests), per device those not ended.
    fn admit_request(&self, host: &dyn NetHost, owner: u64) -> Result<(), i32> {
        let _ = host;
        let mine = self
            .reqs
            .iter()
            .filter(|r| {
                r.owner == owner && matches!(r.state, State::Queued | State::Running | State::Done)
            })
            .count();
        let device = self.reqs.iter().filter(|r| Self::live(r)).count();
        if mine >= PLUGIN_REQUESTS || device >= DEVICE_REQUESTS {
            return Err(ERR_BUSY);
        }
        Ok(())
    }

    /// Whether the pool holds `bytes` more once the `replaced` requests have
    /// given back their input (`ERR_NO_MEMORY`: nothing is held then).
    fn input_fits(&self, host: &dyn NetHost, replaced: &[u32], bytes: usize) -> Result<(), i32> {
        let back: u32 = replaced
            .iter()
            .filter_map(|id| self.index(*id))
            .map(|i| self.reqs[i].input)
            .sum();
        let bytes = u32::try_from(bytes).map_err(|_| ERR_NO_MEMORY)?;
        if host.table().pool().fits(bytes, back) {
            Ok(())
        } else {
            Err(ERR_NO_MEMORY)
        }
    }

    /// `network_demand` follows whether anything uses or waits for the
    /// network.
    pub(super) fn sync_demand(&mut self, host: &dyn NetHost) {
        let active = self.reqs.iter().any(Self::live) || self.serving.any();
        if active != self.demand {
            self.demand = active;
            // The shell hears the OR of NET and SOCK, once per change.
            if let Some(wanted) = host.table().pool().demand(DEMAND_NET, active) {
                host.network_demand(wanted);
            }
        }
    }

    /// End request `index` once. A later call finds it ended and does
    /// nothing. `abort`: the transport still has it.
    fn settle(&mut self, host: &dyn NetHost, index: usize, mut done: Done, abort: bool) {
        let stamp = self.tick_stamp();
        let r = &mut self.reqs[index];
        if !Self::live(r) {
            return;
        }
        if abort && r.state == State::Running {
            host.transport().abort(r.id);
        }
        // The work run is told when the plugin has been told (`event_delivered`).
        // The session goes back after the transport has let go of it, before
        // the plugin is told: on every way a request ends.
        if r.tls {
            host.table().pool().tls_drop(TlsHolder::Net(r.id));
        }
        if r.saving
            && let Ok(parsed) = args::parse_request(&r.batch)
            && let Some(file) = parsed.save
        {
            host.release_save(r.owner, file.root, file.path);
        }
        done.stamp = stamp;
        // The input is not needed any more: its bytes go back to the pool.
        let input = core::mem::take(&mut r.input);
        r.batch = Box::default();
        // The pool keeps only what the held answer needs.
        let keep = u32::try_from(done.body.capacity())
            .unwrap_or(u32::MAX)
            .min(r.reserved);
        let freed = r.reserved - keep;
        r.reserved = keep;
        r.pending = None;
        r.state = State::Done;
        r.done = Some(done);
        self.release(host, freed.saturating_add(input));
    }

    fn bare(&self, at: u64, status: u32, why: u32, flags: u32) -> Done {
        Done {
            stamp: 0,
            at,
            status,
            reason: why,
            flags,
            head: Vec::new(),
            etag: 0,
            content_type: 0,
            body: Vec::new(),
        }
    }

    fn end_with(&mut self, host: &dyn NetHost, index: usize, why: u32, flags: u32) {
        let done = self.bare(host.now_ms(), 0, why, flags);
        self.settle(host, index, done, true);
    }

    /// A request that runs for a plugin whose network the user turned off
    /// ends now (`network_off`), whatever its state: the setting is a privacy
    /// choice and is not waited out. (Offline and saving end what has not
    /// started only; what is on its way is let finish.)
    fn end_network_off(&mut self, host: &dyn NetHost) {
        for index in 0..self.reqs.len() {
            let r = &self.reqs[index];
            if r.state != State::Running {
                continue;
            }
            if let Some(end) = host.power_end(r.owner, r.run.is_some())
                && end.reason == reason::NETWORK_OFF
            {
                self.end_with(host, index, end.reason, 0);
            }
        }
    }

    /// Start queued requests in order; stop at the first that must wait.
    pub(super) fn pump(&mut self, host: &dyn NetHost) {
        let profile = host.profile();
        let now = host.now_ms();
        if host.network_up() {
            // A network came: the wait for one starts again if it goes.
            for r in &mut self.reqs {
                r.net_wait = None;
            }
        }
        self.end_network_off(host);
        let mut index = 0;
        while index < self.reqs.len() {
            if self.reqs[index].state != State::Queued {
                index += 1;
                continue;
            }
            let (owner, background) = (self.reqs[index].owner, self.reqs[index].run.is_some());
            if let Some(end) = host.power_end(owner, background) {
                let flags = if end.later { DONE_LATER } else { 0 };
                self.end_with(host, index, end.reason, flags);
                index += 1;
                continue;
            }
            if !host.network_up() {
                for r in &mut self.reqs {
                    if r.state == State::Queued && r.net_wait.is_none() {
                        r.net_wait = Some(now);
                    }
                }
                self.leave_tls_queue(host, index);
                break;
            }
            let running = self
                .reqs
                .iter()
                .filter(|r| r.state == State::Running)
                .count() as u32;
            let r = &self.reqs[index];
            let need = if r.saving { IO_BUFFER } else { r.max_body };
            if running >= profile.connections
                || (!r.saving && self.bodies() >= profile.memory_bodies)
            {
                self.leave_tls_queue(host, index);
                break;
            }
            // An https request starts only once it holds the TLS session, in
            // the order the requests and sockets asked for it. It asks only
            // when it could otherwise start, and again at every turn, so a
            // request at the head of the queue never blocks the ones behind
            // it for longer than its own turn.
            let (id, tls) = (r.id, r.tls);
            if tls && host.table().pool().tls_request(TlsHolder::Net(id)) != TlsLease::Held {
                break;
            }
            if !self.reserve(host, need) {
                if tls {
                    host.table().pool().tls_drop(TlsHolder::Net(id));
                }
                break;
            }
            self.reqs[index].reserved = if host.table().pool().capacity() == 0 {
                0
            } else {
                need
            };
            self.reqs[index].net_wait = None;
            let started = self.begin(host, index);
            if let Err(error) = started {
                let why = if error == ERR_NO_MEMORY {
                    reason::TOO_LARGE
                } else {
                    reason::REFUSED
                };
                self.end_with(host, index, why, 0);
            } else {
                self.reqs[index].state = State::Running;
            }
            index += 1;
        }
    }

    /// The requests from `from` on cannot start now (no network, no room
    /// for a connection or a body): the https ones among them give up their
    /// place for the TLS session. One that cannot start must not stand at the
    /// head of the queue while a session is free; it asks again when it can.
    fn leave_tls_queue(&self, host: &dyn NetHost, from: usize) {
        for r in self.reqs.iter().skip(from) {
            if r.tls && r.state == State::Queued {
                host.table().pool().tls_drop(TlsHolder::Net(r.id));
            }
        }
    }

    fn begin(&self, host: &dyn NetHost, index: usize) -> Result<(), i32> {
        let r = &self.reqs[index];
        let parsed = args::parse_request(&r.batch)?;
        let start = Start {
            owner: r.owner,
            request: &parsed,
            max_body: r.max_body,
            max_file: r.max_file,
            timeout_ms: r.timeout_ms,
        };
        host.transport().start(r.id, &start)
    }

    /// Deadlines: a request waiting or running longer than its time, a
    /// queued one waiting 30 s for a network.
    fn expire(&mut self, host: &dyn NetHost) {
        let now = host.now_ms();
        if host.network_up() {
            // A network that came back ends every wait for one, before the
            // clock is read against it.
            for r in &mut self.reqs {
                r.net_wait = None;
            }
        }
        for index in 0..self.reqs.len() {
            let r = &self.reqs[index];
            if !Self::live(r) {
                continue;
            }
            let since = if r.saving { r.last_move } else { r.queued_at };
            let over = now.saturating_sub(since) >= u64::from(r.timeout_ms);
            let no_net = r.state == State::Queued
                && r.net_wait
                    .is_some_and(|w| now.saturating_sub(w) >= NO_NETWORK_MS);
            if no_net {
                self.end_with(host, index, reason::NO_NETWORK, 0);
            } else if over {
                self.end_with(host, index, reason::TIMEOUT, 0);
            }
        }
    }
}

/// `request` (`network.md` 2.1).
pub(super) fn request(host: &dyn NetHost, who: &Caller<'_>, input: &[u8]) -> Result<u32, i32> {
    let parsed = args::parse_request(input)?;
    let profile = host.profile();
    let saving = parsed.save.is_some();
    let max_body = match (saving, parsed.head.max_body) {
        (true, _) => 0,
        (false, 0) => profile.max_body,
        (false, wanted) if wanted <= profile.max_body => wanted,
        _ => return Err(ERR_NO_SPACE),
    };
    for file in [&parsed.send, &parsed.save].into_iter().flatten() {
        args::root_ok(file)?;
        host.root_access(who, file.root, file.path, file.role == args::ROLE_SAVE)?;
    }
    if let Some(file) = &parsed.send {
        host.file_len(who, file.root, file.path)?;
    }
    if !host.subscribed(who.owner, wire::EVENT_HTTP_DONE_TAG) {
        return Err(ERR_STATE);
    }
    gate()?;
    let mut batch = Vec::new();
    batch
        .try_reserve_exact(input.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    batch.extend_from_slice(input);
    let mut inner = host_table(host)?;
    inner.admit_request(host, who.owner)?;
    let replaced = same_save_file(host, &inner, who.owner, &parsed)?;
    gate()?;
    inner.reqs.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    let id = inner.next_id.checked_add(1).ok_or(ERR_FULL)?;
    let now = host.now_ms();
    // Everything that can fail comes before the first change: the input's
    // room (counting what the replaced requests give back), then the claim.
    inner.input_fits(host, &replaced, input.len())?;
    // The work run (if any) is kept going while the request lives. Opened
    // last of what can fail but the claim, and given back if that fails.
    let run = host.current_run(who.owner);
    if let Some(run) = run {
        host.resource_opened(run)?;
    }
    if let Some(file) = &parsed.save
        && let Err(error) = host.claim_save(who.owner, file.root, file.path)
    {
        if let Some(run) = run {
            host.resource_closed(run);
        }
        return Err(error);
    }
    for old in replaced {
        if let Some(index) = inner.index(old) {
            inner.end_with(host, index, reason::REPLACED, 0);
        }
    }
    let held = inner.reserve(host, input.len() as u32);
    debug_assert!(held, "the room was checked");
    inner.next_id = id;
    let timeout_ms = match (parsed.head.timeout_ms, saving) {
        (0, true) => FILE_TIMEOUT_MS,
        (0, false) => MEMORY_TIMEOUT_MS,
        (set, _) => set,
    };
    inner.reqs.push(Req {
        id,
        owner: who.owner,
        token: parsed.head.token,
        run,
        run_open: run.is_some(),
        state: State::Queued,
        batch: batch.into_boxed_slice(),
        saving,
        tls: is_https(parsed.head.url),
        progress: parsed.head.flags & args::FLAG_PROGRESS != 0,
        max_body,
        max_file: if parsed.head.max_file == 0 {
            args::MAX_FILE
        } else {
            parsed.head.max_file
        },
        timeout_ms,
        queued_at: now,
        last_move: now,
        net_wait: None,
        reserved: 0,
        input: if host.table().pool().capacity() == 0 {
            0
        } else {
            input.len() as u32
        },
        sent: None,
        pending: None,
        done: None,
    });
    // After the pump: a request or socket that policy refuses at once never
    // wanted the network, and the shell is not told that it did.
    inner.pump(host);
    inner.sync_demand(host);
    Ok(id)
}

fn host_table(host: &dyn NetHost) -> Result<RefMut<'_, Inner>, i32> {
    host.table().lock()
}

/// The ids this save file replaces (the same plugin's older requests that
/// save to the same file); another plugin's request that saves to it is
/// `ERR_BUSY`.
fn same_save_file(
    host: &dyn NetHost,
    inner: &Inner,
    owner: u64,
    parsed: &args::Request<'_>,
) -> Result<Vec<u32>, i32> {
    let mut ids = Vec::new();
    let Some(new) = &parsed.save else {
        return Ok(ids);
    };
    for r in inner.reqs.iter().filter(|r| Inner::live(r) && r.saving) {
        let Ok(other) = args::parse_request(&r.batch) else {
            continue;
        };
        let Some(old) = other.save else {
            continue;
        };
        if old.root != new.root || !host.same_file(new.root, old.path, new.path) {
            continue;
        }
        if r.owner != owner {
            // Root 0 is each plugin's own folder: the same path is another
            // file there.
            if new.root == 0 {
                continue;
            }
            return Err(ERR_BUSY);
        }
        gate()?;
        ids.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        ids.push(r.id);
    }
    Ok(ids)
}

/// `cancel` (2.2): every unfinished request of the plugin with the token.
pub(super) fn cancel(host: &dyn NetHost, owner: u64, token: u32) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    for index in 0..inner.reqs.len() {
        let r = &inner.reqs[index];
        if r.owner == owner && r.token == token && Inner::live(r) {
            inner.end_with(host, index, reason::CANCELLED, 0);
        }
    }
    inner.pump(host);
    inner.sync_demand(host);
    Ok(())
}

/// What a transport reports for a request that came to an end.
pub(crate) struct Finished<'a> {
    pub(crate) status: u32,
    pub(crate) reason: u32,
    /// The 2xx body was written to the file.
    pub(crate) saved: bool,
    /// The memory body was cut at `max_body`.
    pub(crate) truncated: bool,
    pub(crate) etag: &'a str,
    pub(crate) content_type: &'a str,
    pub(crate) final_url: &'a str,
    /// The memory body (or the error body of a failure with a status). Its
    /// allocation (capacity, not length) must not exceed what the request
    /// reserved (`max_body` for a memory request, 4,096 for a saving request); a larger buffer
    /// ends the request with `TOO_LARGE`.
    pub(crate) body: Vec<u8>,
}

/// The transport's body buffer against the room the request reserved
/// (`ceiling`; `truncate` and `clear` keep capacity, so capacity is what is
/// held). Returns the body to keep, whether the buffer broke the contract
/// (the request then ends `TOO_LARGE`), and whether a body was lost.
///
/// An error answer (a status that is not a success) keeps its real status
/// even when the buffer is roomier than the reservation: the bytes it keeps
/// are copied into an exact-size buffer, or dropped (and said so) when even
/// that cannot be allocated.
fn fit_body(body: Vec<u8>, ceiling: usize, error_answer: bool) -> (Vec<u8>, bool, bool) {
    if body.capacity() <= ceiling {
        return (body, false, false);
    }
    if !error_answer {
        return (Vec::new(), true, false);
    }
    let mut exact = Vec::new();
    if body.len() > ceiling || gate().is_err() || exact.try_reserve_exact(body.len()).is_err() {
        return (Vec::new(), false, true);
    }
    exact.extend_from_slice(&body);
    (exact, false, false)
}

/// The transport ended request `id`. An id that is not running (cancelled,
/// timed out, replaced) is ignored: a request ends exactly once.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed: report again.
pub(crate) fn finished(host: &dyn NetHost, id: u32, mut outcome: Finished<'_>) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    let Some(index) = inner.index(id) else {
        return Ok(());
    };
    if inner.reqs[index].state != State::Running {
        return Ok(());
    }
    let r = &inner.reqs[index];
    let ok = outcome.reason == reason::OK;
    let saved = ok && outcome.saved && (200..300).contains(&outcome.status);
    // A failure without a status has no body; with one, at most the error
    // body; a saved answer's body is in the file.
    if (!ok && outcome.status == 0) || saved {
        // Dropped, not cleared: a cleared buffer would keep its capacity.
        outcome.body = Vec::new();
    }
    // An error body is at most 4,096 bytes, and a memory request keeps no
    // more than its `max_body`. A device without a pool has no ceiling.
    let ceiling = if host.table().pool().capacity() == 0 {
        usize::MAX
    } else {
        r.reserved as usize
    };
    let cap = if ok && !r.saving && (200..300).contains(&outcome.status) {
        r.max_body as usize
    } else if r.saving {
        ERROR_BODY
    } else {
        ERROR_BODY.min(r.max_body as usize)
    };
    let mut flags = if saved { DONE_SAVED } else { 0 };
    if outcome.body.len() > cap {
        outcome.body.truncate(cap);
        flags |= DONE_TRUNCATED;
    } else if outcome.truncated && !saved {
        flags |= DONE_TRUNCATED;
    }
    // The transport's buffer must fit what the request holds in the pool:
    // `truncate` and `clear` keep capacity, so the capacity is what is held.
    let error_answer =
        outcome.status != 0 && !(ok && !r.saving && (200..300).contains(&outcome.status));
    let (body, overshoot, lost) =
        fit_body(core::mem::take(&mut outcome.body), ceiling, error_answer);
    outcome.body = body;
    if lost {
        flags |= DONE_TRUNCATED;
    }
    let at = host.now_ms();
    let mut done = inner.bare(at, outcome.status, outcome.reason, flags);
    let mut parts = [outcome.etag, outcome.content_type, outcome.final_url];
    if parts[0].len() > 256 || parts[1].len() > 128 || parts[2].len() > 1024 {
        if !saved {
            // A hostile server's overlong head: the request ends now,
            // refused, and does not wait for its timeout.
            inner.end_with(host, index, reason::REFUSED, 0);
            inner.pump(host);
            inner.sync_demand(host);
            return Ok(());
        }
        // The file is committed: the success stays, the oversized metadata
        // is dropped.
        parts = ["", "", ""];
    }
    let total: usize = parts.iter().map(|p| p.len()).sum();
    if overshoot {
        done.reason = reason::TOO_LARGE;
        done.status = 0;
    } else if gate().is_err() || done.head.try_reserve_exact(total).is_err() {
        if saved || error_answer {
            // The outcome is real (the file is committed; the server's
            // status and reason stand): only the optional head (ETag, type,
            // final URL) is lost.
            done.body = outcome.body;
        } else {
            // No room for the answer's head: the body goes too.
            done.reason = reason::TOO_LARGE;
            done.status = 0;
        }
    } else {
        for part in parts {
            done.head.extend_from_slice(part.as_bytes());
        }
        done.etag = parts[0].len() as u16;
        done.content_type = parts[1].len() as u16;
        done.body = outcome.body;
    }
    inner.settle(host, index, done, false);
    inner.pump(host);
    inner.sync_demand(host);
    Ok(())
}

/// Bytes of the body arrived (`network.md` 4.2): the progress event, first
/// when the body starts, then every 5 % or 2,000 ms with more bytes; only for
/// 2xx answers of a request that asked for it.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn progress(
    host: &dyn NetHost,
    id: u32,
    status: u32,
    received: u64,
    total: Option<u64>,
) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    let Some(index) = inner.index(id) else {
        return Ok(());
    };
    let now = host.now_ms();
    let stamp = inner.tick_stamp();
    let r = &mut inner.reqs[index];
    if r.state != State::Running {
        return Ok(());
    }
    r.last_move = now;
    if !r.progress || !(200..300).contains(&status) {
        return Ok(());
    }
    let total_v = total.unwrap_or(0);
    let tell = match r.sent {
        None => true,
        Some((at, last)) => {
            received > last
                && (now.saturating_sub(at) >= 2_000
                    || (total_v > 0 && (received - last).saturating_mul(20) >= total_v))
        }
    };
    if tell {
        r.sent = Some((now, received));
        r.pending = Some(Pending {
            stamp,
            received,
            total: total_v,
            flags: u32::from(total.is_some()),
            at: now,
        });
    }
    Ok(())
}

/// The shell followed a redirect from `http` to `https` for request `id` and
/// closed the plain connection: the request goes back in the queue, where it
/// was started, as an https request (`network.md` 3.1). It starts again,
/// the shell resuming the request it parked, only when it holds the TLS
/// session; it keeps its start time, so `timeout_ms` counts from the first
/// start. The pool bytes it held are given back and asked for again.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed; `ERR_NOT_FOUND` for an id that is
/// unknown or over (a late report after a cancel is normal: the shell does
/// not treat it as a failure of the request); `ERR_STATE` for a request that
/// is not running or is https already.
pub(crate) fn redirected_to_https(host: &dyn NetHost, id: u32) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    let index = inner
        .index(id)
        .filter(|&i| Inner::live(&inner.reqs[i]))
        .ok_or(ERR_NOT_FOUND)?;
    let r = &mut inner.reqs[index];
    if r.state != State::Running || r.tls {
        return Err(ERR_STATE);
    }
    r.tls = true;
    r.state = State::Queued;
    r.pending = None;
    let held = core::mem::take(&mut r.reserved);
    inner.release(host, held);
    inner.pump(host);
    inner.sync_demand(host);
    Ok(())
}

/// The clock: ends what ran out of time and starts what can start.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn tick(host: &dyn NetHost) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    inner.expire(host);
    serving::expire(&mut inner, host);
    inner.pump(host);
    inner.sync_demand(host);
    Ok(())
}

/// When the first request or served request runs out of time, in the
/// host's clock: what [`tick`] would end then. `None` when nothing is
/// waiting on the clock. A borrowed table answers `Some(0)` (look again).
pub(crate) fn next_deadline(host: &dyn NetHost) -> Option<u64> {
    let Ok(inner) = host_table(host) else {
        return Some(0);
    };
    let requests = inner.reqs.iter().filter(|r| Inner::live(r)).flat_map(|r| {
        let since = if r.saving { r.last_move } else { r.queued_at };
        let over = since.saturating_add(u64::from(r.timeout_ms));
        let no_net = r
            .net_wait
            .filter(|_| r.state == State::Queued)
            .map(|w| w.saturating_add(NO_NETWORK_MS));
        [Some(over), no_net]
    });
    requests
        .flatten()
        .chain(inner.serving.next_deadline())
        .min()
}

/// The network went away: running requests end with `no_network`.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn network_lost(host: &dyn NetHost) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    for index in 0..inner.reqs.len() {
        if inner.reqs[index].state == State::Running {
            inner.end_with(host, index, reason::NO_NETWORK, 0);
        }
    }
    serving::network_lost(&mut inner, host);
    inner.sync_demand(host);
    Ok(())
}

/// A work run reached its longest time: its requests end with `work_ended`.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn work_ended(host: &dyn NetHost, owner: u64, run: u64) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    for index in 0..inner.reqs.len() {
        let r = &inner.reqs[index];
        if r.owner == owner && r.run == Some(run) && Inner::live(r) {
            inner.end_with(host, index, reason::WORK_ENDED, 0);
        }
    }
    inner.pump(host);
    inner.sync_demand(host);
    Ok(())
}

/// The oldest event of `owner` (99, 100, 101 or 102), encoded into `out`.
/// `Ok(None)` when there is none. An `event.http_done` makes its answer
/// readable until [`event_delivered`].
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed; `ERR_NO_SPACE` when `out` is too
/// small (nothing was consumed).
pub(crate) fn next_event(
    host: &dyn NetHost,
    owner: u64,
    out: &mut [u8],
) -> Result<Option<usize>, i32> {
    Ok(next_event_run(host, owner, out)?.map(|(used, _)| used))
}

/// [`next_event`] with the work run the event belongs to (`None`: no run).
///
/// # Errors
/// As [`next_event`].
pub(crate) fn next_event_run(
    host: &dyn NetHost,
    owner: u64,
    out: &mut [u8],
) -> Result<Option<(usize, Option<u64>)>, i32> {
    let mut inner = host_table(host)?;
    let mut best: Option<(u64, Pick)> = None;
    for r in inner.reqs.iter().filter(|r| r.owner == owner) {
        if let Some(done) = r.done.as_ref().filter(|_| r.state == State::Done) {
            take_min(&mut best, done.stamp, Pick::Done(r.id));
        }
        if let Some(p) = r.pending.filter(|_| r.state == State::Running) {
            take_min(&mut best, p.stamp, Pick::Progress(r.id));
        }
    }
    serving::scan(&inner, owner, &mut best);
    let Some((_, pick)) = best else {
        return Ok(None);
    };
    let run = match pick {
        Pick::Done(id) | Pick::Progress(id) => inner.index(id).and_then(|i| inner.reqs[i].run),
        _ => None,
    };
    let used = match pick {
        Pick::Done(id) => encode_done(&mut inner, id, out)?,
        Pick::Progress(id) => encode_progress(&mut inner, id, out)?,
        other => serving::encode(&mut inner, host.now_ms(), other, out)?,
    };
    Ok(Some((used, run)))
}

fn take_min(best: &mut Option<(u64, Pick)>, stamp: u64, pick: Pick) {
    if best.as_ref().is_none_or(|(s, _)| stamp < *s) {
        *best = Some((stamp, pick));
    }
}

fn encode_done(inner: &mut Inner, id: u32, out: &mut [u8]) -> Result<usize, i32> {
    let index = inner.index(id).ok_or(ERR_NOT_FOUND)?;
    let r = &mut inner.reqs[index];
    let done = r.done.as_ref().ok_or(ERR_NOT_FOUND)?;
    let record = Record::EventHttpDone(wire::EventHttpDone {
        now_ms: done.at,
        id: r.id,
        token: r.token,
        status: done.status,
        reason: done.reason,
        flags: done.flags,
        body_len: done.body.len() as u32,
    });
    let used = codec::encode_event(&record, out)?;
    r.state = State::Held;
    Ok(used)
}

fn encode_progress(inner: &mut Inner, id: u32, out: &mut [u8]) -> Result<usize, i32> {
    let index = inner.index(id).ok_or(ERR_NOT_FOUND)?;
    let r = &mut inner.reqs[index];
    let p = r.pending.ok_or(ERR_NOT_FOUND)?;
    let record = Record::EventHttpProgress(wire::EventHttpProgress {
        now_ms: p.at,
        id: r.id,
        token: r.token,
        received: p.received,
        total: p.total,
        flags: p.flags,
    });
    let used = codec::encode_event(&record, out)?;
    r.pending = None;
    Ok(used)
}

/// The callback of `owner` returned: its held answers are freed.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn event_delivered(host: &dyn NetHost, owner: u64) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    let mut freed = 0_u32;
    inner.reqs.retain_mut(|r| {
        let held = r.owner == owner && r.state == State::Held;
        if held {
            freed = freed.saturating_add(r.reserved);
            // The plugin has been told of the end: the run lets go of it.
            Inner::close_run(host, r);
        }
        !held
    });
    inner.release(host, freed);
    inner.pump(host);
    inner.sync_demand(host);
    Ok(())
}

fn held(inner: &Inner, owner: u64, id: u32) -> Result<&Req, i32> {
    inner
        .reqs
        .iter()
        .find(|r| r.id == id && r.owner == owner && r.state == State::Held)
        .ok_or(ERR_NOT_FOUND)
}

/// `answer` (2.3).
pub(super) fn answer(
    host: &dyn NetHost,
    owner: u64,
    id: u32,
    out: &mut [u8],
) -> Result<usize, i32> {
    let inner = host_table(host)?;
    let r = held(&inner, owner, id)?;
    let done = r.done.as_ref().ok_or(ERR_NOT_FOUND)?;
    let (etag, rest) = done.head.split_at(usize::from(done.etag));
    let (content_type, final_url) = rest.split_at(usize::from(done.content_type));
    fn text(bytes: &[u8]) -> Result<&str, i32> {
        core::str::from_utf8(bytes).map_err(|_| ERR_INVALID)
    }
    let record = Record::ServiceNetAnswer(wire::ServiceNetAnswer {
        status: done.status,
        flags: done.flags & !DONE_LATER,
        body_len: done.body.len() as u32,
        etag: text(etag)?,
        content_type: text(content_type)?,
        final_url: text(final_url)?,
    });
    super::reply(&record, out)
}

/// `body` (2.3): the bytes from `offset`, at most 3,840; none at or past the
/// end.
pub(super) fn body(
    host: &dyn NetHost,
    owner: u64,
    id: u32,
    offset: u32,
    out: &mut [u8],
) -> Result<usize, i32> {
    let inner = host_table(host)?;
    let r = held(&inner, owner, id)?;
    let body = &r.done.as_ref().ok_or(ERR_NOT_FOUND)?.body;
    let from = (offset as usize).min(body.len());
    let to = body.len().min(from + BODY_PIECE);
    super::reply(
        &Record::ServiceNetBytes(wire::ServiceNetBytes {
            data: &body[from..to],
        }),
        out,
    )
}

/// See [`super::owner_holds`].
pub(super) fn owner_holds(host: &dyn NetHost, owner: u64) -> Result<bool, i32> {
    let inner = host_table(host)?;
    Ok(inner.reqs.iter().any(|r| r.owner == owner) || inner.serving.owner_holds(owner))
}

/// The plugin stopped: its requests go without events, its servers stop
/// without events; the pool gets everything back.
pub(super) fn owner_stopped(host: &dyn NetHost, owner: u64) -> Result<(), i32> {
    let mut inner = host_table(host)?;
    let mut freed = 0_u32;
    let mut index = 0;
    while index < inner.reqs.len() {
        if inner.reqs[index].owner != owner {
            index += 1;
            continue;
        }
        let r = &mut inner.reqs[index];
        if r.state == State::Running {
            host.transport().abort(r.id);
        }
        if r.tls {
            host.table().pool().tls_drop(TlsHolder::Net(r.id));
        }
        Inner::close_run(host, r);
        if Inner::live(r)
            && r.saving
            && let Ok(parsed) = args::parse_request(&r.batch)
            && let Some(file) = parsed.save
        {
            host.release_save(owner, file.root, file.path);
        }
        freed = freed.saturating_add(r.reserved).saturating_add(r.input);
        inner.reqs.remove(index);
    }
    inner.release(host, freed);
    serving::owner_stopped(&mut inner, host, owner);
    // Whatever the plugin's servers left in its `.received/` goes with it.
    host.clear_received(owner, None);
    inner.pump(host);
    inner.sync_demand(host);
    Ok(())
}
