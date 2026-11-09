//! The server table of `core.net` (`network.md` 5): `serve` and
//! `stop_serving`, served requests with their uploads under
//! `.received/<server>/<request>`, `respond`, and events 101 and 102.
use super::super::host_services::Caller;
use super::{
    ANSWER_MS, IO_BUFFER, NetHost, PLUGIN_SERVERS, Reply, SERVER_OPEN, args, gate, own, reason,
    table::{Inner, Pick},
    wire::{self, Record},
};
use crate::native_host::codec;
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED,
};

/// Events a plugin may leave untold before `serve` is `ERR_BUSY`.
const EVENTS_WAITING: usize = 4;
/// An upload that moves no byte for this long is dropped (408, 5.2).
const STALL_MS: u64 = 30_000;
/// A head over this is answered 431 (5.2).
const HEAD_MAX: u32 = 8 * 1024;
/// A target over this is answered 414.
const TARGET_MAX: usize = 1024;
/// `event.server.state`.
const STARTED: u32 = 0;
const FAILED: u32 = 1;
const STOPPED: u32 = 2;

struct Server {
    key: u32,
    owner: u64,
    token: u32,
    port: u32,
    /// The screen instance the server lives on.
    screen: u64,
    max_body: u64,
    /// The shell reported that the listener is open (`server_started`); until
    /// then the plugin has not been told `started`.
    opened: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// The body (or head) is still coming.
    Receiving,
    /// Whole; `event.served_request` not yet told.
    Told,
    /// Told: the plugin has until `since + 120 s`.
    Open { since: u64 },
    /// Answered; the host holds what it sends until the transport is done.
    Answering,
}

struct Served {
    id: u32,
    server: u32,
    owner: u64,
    token: u32,
    stage: Stage,
    method: String,
    target: String,
    body_len: u64,
    stamp: u64,
    /// Pool bytes held (the upload buffer, then the answer's body).
    reserved: u32,
    /// When a byte last came (the head, then the body).
    last_move: u64,
}

struct Event {
    stamp: u64,
    at: u64,
    owner: u64,
    token: u32,
    state: u32,
    why: u32,
    address: String,
}

#[derive(Default)]
pub(super) struct Tables {
    servers: Vec<Server>,
    served: Vec<Served>,
    events: Vec<Event>,
    next_server: u32,
    next_request: u32,
    /// Terminal events that could not be kept: a broken invariant.
    pub(super) lost: u32,
}

impl Tables {
    /// Whether `owner` has a server, a served request or an event waiting.
    pub(super) fn owner_holds(&self, owner: u64) -> bool {
        self.servers.iter().any(|s| s.owner == owner)
            || self.served.iter().any(|s| s.owner == owner)
            || self.events.iter().any(|e| e.owner == owner)
    }
    pub(super) fn any(&self) -> bool {
        !self.servers.is_empty()
    }

    /// Whether an event waits for the pump.
    pub(super) fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// When the first served request runs out of time (`expire` ends it).
    pub(super) fn next_deadline(&self) -> Option<u64> {
        self.served
            .iter()
            .filter_map(|s| match s.stage {
                Stage::Receiving => Some(s.last_move.saturating_add(STALL_MS)),
                Stage::Open { since } => Some(since.saturating_add(ANSWER_MS)),
                _ => None,
            })
            .min()
    }
}

/// What the transport does with a request head it parsed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Receive it: the body goes to `.received/<token>/<request>`.
    Accept { request: u32, token: u32 },
    /// Answer with this status and close; the plugin is told nothing.
    Reject(u32),
    /// The server has its open requests: the connection waits in the shell.
    Busy,
}

fn tell(
    inner: &mut Inner,
    host: &dyn NetHost,
    owner: u64,
    token: u32,
    state: u32,
    why: u32,
    address: &str,
) -> Result<(), i32> {
    let stamp = inner.tick_stamp();
    let event = Event {
        stamp,
        at: host.now_ms(),
        owner,
        token,
        state,
        why,
        address: own(address)?,
    };
    let tables = &mut inner.serving;
    // Started is coalesced: the newest replaces an older pending one.
    tables
        .events
        .retain(|e| !(e.owner == owner && e.token == token && e.state == STARTED));
    gate()?;
    tables.events.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
    tables.events.push(event);
    Ok(())
}

/// The terminal event of a server (`stopped`): never lost. Its room was
/// reserved when the server started (the events vector holds at least the
/// pending events plus one per live server) and it needs no string, so it
/// allocates nothing and cannot fail.
fn tell_stopped(inner: &mut Inner, host: &dyn NetHost, owner: u64, token: u32, why: u32) {
    let stamp = inner.tick_stamp();
    let tables = &mut inner.serving;
    tables
        .events
        .retain(|e| !(e.owner == owner && e.token == token && e.state == STARTED));
    if tables.events.len() == tables.events.capacity() {
        // Unreachable by the reservation; if it ever broke, a coalescible
        // `started` of any server makes room rather than dropping this one.
        match tables.events.iter().position(|e| e.state == STARTED) {
            Some(at) => {
                tables.events.remove(at);
            }
            None => {
                tables.lost = tables.lost.saturating_add(1);
                return;
            }
        }
    }
    tables.events.push(Event {
        stamp,
        at: host.now_ms(),
        owner,
        token,
        state: STOPPED,
        why,
        address: String::new(),
    });
}

/// Everything `serve` needs before the old server of the token is touched:
/// the limits, then room for the server, this call's event and the terminal
/// event of every server alive. Returns the old server's index.
fn make_room(
    inner: &mut Inner,
    owner: u64,
    token: u32,
    port: u32,
    device_servers: u32,
) -> Result<Option<usize>, i32> {
    let old = inner
        .serving
        .servers
        .iter()
        .position(|s| s.owner == owner && s.token == token);
    let others = inner
        .serving
        .servers
        .iter()
        .filter(|s| s.owner == owner && s.token != token)
        .count();
    // A plugin that has not heard its events yet cannot pile up more by
    // looping in one callback.
    let waiting = inner
        .serving
        .events
        .iter()
        .filter(|e| e.owner == owner)
        .count();
    // A server of the same token is replaced and keeps its place; a new one
    // needs a place in the plugin's two and in the device's `SERVERS`.
    // The cable's server (port 0) uses no socket and no pool buffer: it
    // neither counts against `SERVERS` nor is refused by it.
    let network = inner.serving.servers.iter().filter(|s| s.port != 0).count();
    let device_full = port != 0 && network >= device_servers as usize;
    if (old.is_none() && (others >= PLUGIN_SERVERS || device_full)) || waiting >= EVENTS_WAITING {
        return Err(ERR_BUSY);
    }
    // Room for everything before the old server is touched: the server, this
    // call's event, and the terminal event of every server alive (including
    // this one) so that a stop can always tell it.
    gate()?;
    inner
        .serving
        .servers
        .try_reserve(1)
        .map_err(|_| ERR_NO_MEMORY)?;
    gate()?;
    let tables = &mut inner.serving;
    let need = tables.events.len() + tables.servers.len() + 2;
    if tables.events.capacity() < need {
        let extra = need - tables.events.len();
        tables
            .events
            .try_reserve_exact(extra)
            .map_err(|_| ERR_NO_MEMORY)?;
    }
    Ok(old)
}

/// `serve` (5.1).
pub(super) fn serve(
    host: &dyn NetHost,
    who: &Caller<'_>,
    serve: &wire::ServiceNetServe,
) -> Result<(), i32> {
    let profile = host.profile();
    if serve.flags != 0 || (serve.port == 0 && !profile.cable) {
        return Err(ERR_UNSUPPORTED);
    }
    if serve.port > 65_535 {
        return Err(ERR_INVALID);
    }
    let max_body = match serve.max_body {
        0 => args::SERVE_BODY_DEFAULT,
        wanted if wanted <= args::SERVE_BODY_MAX => wanted,
        _ => return Err(ERR_NO_SPACE),
    };
    // From the callback of a showing screen (a dialog included), that screen;
    // from any other callback but a work run's, the plugin's topmost own
    // screen if it is showing (`network.md` 5.1); else `ERR_STATE`. That
    // includes the callback of a covered own screen: its server goes to the
    // screen on top.
    let screen = match host.showing_screen(who.owner) {
        Some(screen) => screen,
        // (A scheduler that cannot be asked now fails closed.)
        None if host.run_known() && host.current_run(who.owner).is_none() => {
            host.topmost_screen(who.owner).ok_or(ERR_STATE)?
        }
        None => return Err(ERR_STATE),
    };
    if !host.subscribed(who.owner, wire::EVENT_SERVER_TAG) {
        return Err(ERR_STATE);
    }
    let mut inner = host.table().lock()?;
    let old = make_room(
        &mut inner,
        who.owner,
        serve.token,
        serve.port,
        profile.servers,
    )?;
    let key = inner.serving.next_server.checked_add(1).ok_or(ERR_FULL)?;
    if let Some(index) = old {
        stop(&mut inner, host, index, reason::OK, true);
    }
    inner.serving.next_server = key;
    host.clear_received(who.owner, Some(serve.token));
    let refusal = host
        .power_end(who.owner, false)
        .map(|e| e.reason)
        .or_else(|| (serve.port != 0 && !host.network_up()).then_some(reason::NO_NETWORK));
    // The shell's `listen` returns what it knows now; the listener opens at
    // its next step, and the plugin is told `started` (with the address) only
    // when the shell reports it (`server_started`), or `failed` if it cannot
    // (`server_failed`).
    let mut known = String::new();
    let outcome = match refusal {
        Some(why) => Err(why),
        None => host.transport().listen(key, serve.port, &mut known),
    };
    match outcome {
        Ok(()) => {
            inner.serving.servers.push(Server {
                key,
                owner: who.owner,
                token: serve.token,
                port: serve.port,
                screen,
                max_body,
                opened: false,
            });
        }
        Err(why) => tell(&mut inner, host, who.owner, serve.token, FAILED, why, "")?,
    }
    inner.sync_demand(host);
    Ok(())
}

/// The shell reports that the listener of server `key` is open at `address`:
/// the plugin is told `started`.
///
/// # Errors
/// `ERR_NOT_FOUND` for a server that does not exist (it stopped, or failed,
/// meanwhile); `ERR_STATE` when it was reported already; `ERR_BUSY` when the
/// table is borrowed.
pub(crate) fn server_started(host: &dyn NetHost, key: u32, address: &str) -> Result<(), i32> {
    let mut inner = host.table().lock()?;
    let at = inner
        .serving
        .servers
        .iter()
        .position(|s| s.key == key)
        .ok_or(ERR_NOT_FOUND)?;
    if inner.serving.servers[at].opened {
        return Err(ERR_STATE);
    }
    let (owner, token) = (
        inner.serving.servers[at].owner,
        inner.serving.servers[at].token,
    );
    // An address that does not fit is refused, and the shell stops listening.
    let told = if address.len() > 64 {
        Err(ERR_INVALID)
    } else {
        tell(&mut inner, host, owner, token, STARTED, 0, address)
    };
    match told {
        Ok(()) => inner.serving.servers[at].opened = true,
        Err(error) => {
            // The server is not kept; the plugin is told it ended (the
            // terminal event is never lost).
            stop(&mut inner, host, at, reason::REFUSED, false);
            if error == ERR_INVALID {
                if tell(&mut inner, host, owner, token, FAILED, reason::REFUSED, "").is_err() {
                    tell_stopped(&mut inner, host, owner, token, reason::REFUSED);
                }
            } else {
                tell_stopped(&mut inner, host, owner, token, reason::REFUSED);
            }
            inner.sync_demand(host);
            return if error == ERR_INVALID {
                Ok(())
            } else {
                Err(error)
            };
        }
    }
    inner.sync_demand(host);
    Ok(())
}

/// The shell reports that the listener of server `key` could not be opened
/// (or went away): the plugin is told `failed` with reason `refused`, and the
/// server's slot is free.
///
/// # Errors
/// `ERR_NOT_FOUND` for a server that does not exist; `ERR_BUSY` when the
/// table is borrowed.
pub(crate) fn server_failed(host: &dyn NetHost, key: u32) -> Result<(), i32> {
    let mut inner = host.table().lock()?;
    let at = inner
        .serving
        .servers
        .iter()
        .position(|s| s.key == key)
        .ok_or(ERR_NOT_FOUND)?;
    let (owner, token) = (
        inner.serving.servers[at].owner,
        inner.serving.servers[at].token,
    );
    // Its requests end and the shell is told to let go (a no-op for a
    // listener that never opened).
    stop(&mut inner, host, at, reason::REFUSED, false);
    if tell(&mut inner, host, owner, token, FAILED, reason::REFUSED, "").is_err() {
        // No room for a `failed`: the terminal `stopped` is never lost.
        tell_stopped(&mut inner, host, owner, token, reason::REFUSED);
    }
    inner.sync_demand(host);
    Ok(())
}

/// `stop_serving`: the plugin's server with the token stops (`stopped`). No
/// such server is `OK` (nothing to do).
pub(super) fn stop_serving(host: &dyn NetHost, owner: u64, token: u32) -> Result<(), i32> {
    let mut inner = host.table().lock()?;
    if let Some(index) = inner
        .serving
        .servers
        .iter()
        .position(|s| s.owner == owner && s.token == token)
    {
        stop(&mut inner, host, index, reason::OK, true);
    }
    inner.sync_demand(host);
    Ok(())
}

/// The screen instance closed: the servers tied to it stop.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn screen_closed(host: &dyn NetHost, owner: u64, screen: u64) -> Result<(), i32> {
    let mut inner = host.table().lock()?;
    while let Some(index) = inner
        .serving
        .servers
        .iter()
        .position(|s| s.owner == owner && s.screen == screen)
    {
        stop(&mut inner, host, index, reason::OK, true);
    }
    inner.sync_demand(host);
    Ok(())
}

/// Stop one server: its listening ends, its served requests end (told
/// nothing, their bodies removed) and, if `emit`, `stopped` is told.
fn stop(inner: &mut Inner, host: &dyn NetHost, index: usize, why: u32, emit: bool) {
    let server = inner.serving.servers.remove(index);
    host.transport().unlisten(server.key);
    let mut freed = 0_u32;
    let mut at = 0;
    while at < inner.serving.served.len() {
        let s = &inner.serving.served[at];
        if s.server != server.key {
            at += 1;
            continue;
        }
        if s.stage != Stage::Answering {
            host.transport().close_served(s.id, 503);
        }
        host.remove_received(s.owner, s.token, s.id);
        freed = freed.saturating_add(s.reserved);
        inner.serving.served.remove(at);
    }
    inner.release(host, freed);
    // Pending events of the server's requests are gone with them.
    if emit {
        tell_stopped(inner, host, server.owner, server.token, why);
    }
}

/// The plugin stopped: its servers stop and its requests end, told nothing.
pub(super) fn owner_stopped(inner: &mut Inner, host: &dyn NetHost, owner: u64) {
    while let Some(index) = inner.serving.servers.iter().position(|s| s.owner == owner) {
        stop(inner, host, index, reason::OK, false);
    }
    inner.serving.events.retain(|e| e.owner != owner);
}

/// The network went away: the servers on a network stop (`no_network`); the
/// cable server stays.
pub(super) fn network_lost(inner: &mut Inner, host: &dyn NetHost) {
    while let Some(index) = inner.serving.servers.iter().position(|s| s.port != 0) {
        stop(inner, host, index, reason::NO_NETWORK, true);
    }
}

/// A request the plugin did not answer in 120 s is answered 503 by the host.
pub(super) fn expire(inner: &mut Inner, host: &dyn NetHost) {
    // A server of a plugin whose network the user turned off stops.
    let mut at = 0;
    while at < inner.serving.servers.len() {
        let owner = inner.serving.servers[at].owner;
        let off = host
            .power_end(owner, false)
            .is_some_and(|end| end.reason == reason::NETWORK_OFF);
        if off {
            stop(inner, host, at, reason::NETWORK_OFF, true);
        } else {
            at += 1;
        }
    }
    let now = host.now_ms();
    let mut freed = 0_u32;
    let mut at = 0;
    while at < inner.serving.served.len() {
        let s = &inner.serving.served[at];
        let stalled = s.stage == Stage::Receiving && now.saturating_sub(s.last_move) >= STALL_MS;
        if stalled {
            host.transport().close_served(s.id, 408);
            host.remove_received(s.owner, s.token, s.id);
            freed = freed.saturating_add(s.reserved);
            inner.serving.served.remove(at);
        } else if matches!(s.stage, Stage::Open { since } if now.saturating_sub(since) >= ANSWER_MS)
        {
            host.transport().close_served(s.id, 503);
            host.remove_received(s.owner, s.token, s.id);
            freed = freed.saturating_add(s.reserved);
            inner.serving.served.remove(at);
        } else {
            at += 1;
        }
    }
    inner.release(host, freed);
}

/// The plugin that owns server `key`.
pub(crate) fn server_owner(host: &dyn NetHost, key: u32) -> Option<u64> {
    let inner = host.table().lock().ok()?;
    inner
        .serving
        .servers
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.owner)
}

/// A request head arrived on a server (`key` as `listen` got it).
///
/// # Errors
/// `ERR_NOT_FOUND` for a server that does not exist (it stopped meanwhile);
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn served_head(
    host: &dyn NetHost,
    key: u32,
    method: &str,
    target: &str,
    content_length: Option<u64>,
    chunked: bool,
    head_len: u32,
) -> Result<Decision, i32> {
    let mut inner = host.table().lock()?;
    let (token, owner, max_body) = {
        let server = inner
            .serving
            .servers
            .iter()
            .find(|s| s.key == key)
            .ok_or(ERR_NOT_FOUND)?;
        // A server the shell has not reported open has told its plugin
        // nothing: no request is taken for it.
        if !server.opened {
            return Ok(Decision::Reject(503));
        }
        (server.token, server.owner, server.max_body)
    };
    if head_len > HEAD_MAX {
        return Ok(Decision::Reject(431));
    }
    if target.len() > TARGET_MAX {
        return Ok(Decision::Reject(414));
    }
    if chunked {
        return Ok(Decision::Reject(411));
    }
    if !args::is_token(method, 16) {
        return Ok(Decision::Reject(400));
    }
    let open = inner
        .serving
        .served
        .iter()
        .filter(|s| s.server == key)
        .count();
    if open >= SERVER_OPEN {
        return Ok(Decision::Busy);
    }
    let length = content_length.unwrap_or(0);
    if length > max_body {
        return Ok(Decision::Reject(413));
    }
    if length > 0 && !host.room_for(length) {
        return Ok(Decision::Reject(507));
    }
    let Some(id) = inner.serving.next_request.checked_add(1) else {
        return Ok(Decision::Reject(503));
    };
    let reserved = if length > 0 { IO_BUFFER } else { 0 };
    if reserved > 0 && !inner.reserve(host, reserved) {
        return Ok(Decision::Reject(503));
    }
    let made = (|| {
        gate()?;
        inner
            .serving
            .served
            .try_reserve(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        Ok::<_, i32>((own(method)?, own(target)?))
    })();
    let Ok((method, target)) = made else {
        inner.release(host, reserved);
        return Ok(Decision::Reject(503));
    };
    inner.serving.next_request = id;
    inner.serving.served.push(Served {
        id,
        server: key,
        owner,
        token,
        stage: Stage::Receiving,
        method,
        target,
        body_len: length,
        stamp: 0,
        reserved,
        last_move: host.now_ms(),
    });
    Ok(Decision::Accept { request: id, token })
}

/// The upload of `request` has `received` bytes so far: past the server's
/// `max_body` it is answered 413, at the storage's free-space reserve 507; in
/// both the part is removed and the request ends.
///
/// # Errors
/// `ERR_NOT_FOUND` for a request that ended; `ERR_BUSY` when borrowed.
pub(crate) fn served_body(
    host: &dyn NetHost,
    request: u32,
    received: u64,
    added: u64,
) -> Result<Decision, i32> {
    let mut inner = host.table().lock()?;
    let at = inner
        .serving
        .served
        .iter()
        .position(|s| s.id == request && s.stage == Stage::Receiving)
        .ok_or(ERR_NOT_FOUND)?;
    let s = &inner.serving.served[at];
    let max = inner
        .serving
        .servers
        .iter()
        .find(|v| v.key == s.server)
        .map_or(0, |v| v.max_body);
    let status = if received > max {
        413
    } else if !host.room_for(added) {
        507
    } else {
        if added > 0 {
            inner.serving.served[at].last_move = host.now_ms();
        }
        return Ok(Decision::Busy);
    };
    let s = &inner.serving.served[at];
    host.remove_received(s.owner, s.token, s.id);
    let freed = s.reserved;
    inner.serving.served.remove(at);
    inner.release(host, freed);
    Ok(Decision::Reject(status))
}

/// The request is whole (`total` bytes of body, 0 for none): the plugin is
/// told with `event.served_request`.
///
/// # Errors
/// `ERR_NOT_FOUND` for a request that ended; `ERR_BUSY` when borrowed.
pub(crate) fn served_whole(host: &dyn NetHost, request: u32, total: u64) -> Result<(), i32> {
    let mut inner = host.table().lock()?;
    let stamp = inner.tick_stamp();
    let at = inner
        .serving
        .served
        .iter()
        .position(|s| s.id == request && s.stage == Stage::Receiving)
        .ok_or(ERR_NOT_FOUND)?;
    let s = &mut inner.serving.served[at];
    s.stage = Stage::Told;
    s.body_len = total;
    s.stamp = stamp;
    let freed = core::mem::take(&mut s.reserved);
    inner.release(host, freed);
    Ok(())
}

/// The request ended on the transport (answer sent, client gone, stalled):
/// its part is removed and its pool bytes are back. Idempotent.
///
/// # Errors
/// `ERR_BUSY` when the table is borrowed.
pub(crate) fn served_gone(host: &dyn NetHost, request: u32) -> Result<(), i32> {
    let mut inner = host.table().lock()?;
    if let Some(at) = inner.serving.served.iter().position(|s| s.id == request) {
        let s = inner.serving.served.remove(at);
        host.remove_received(s.owner, s.token, s.id);
        inner.release(host, s.reserved);
    }
    Ok(())
}

/// `respond` (5.3).
pub(super) fn respond(host: &dyn NetHost, who: &Caller<'_>, input: &[u8]) -> Result<(), i32> {
    let response = args::parse_response(input)?;
    let mut inner = host.table().lock()?;
    let at = inner
        .serving
        .served
        .iter()
        .position(|s| {
            s.id == response.head.request
                && s.owner == who.owner
                && matches!(s.stage, Stage::Open { .. })
        })
        .ok_or(ERR_NOT_FOUND)?;
    let file_len = match &response.send {
        Some(file) => {
            host.root_access(who, file.root, file.path, false)?;
            match host.file_len(who, file.root, file.path) {
                Ok(len) => Some(len),
                Err(e) if e == ERR_NOT_FOUND => None,
                Err(e) => return Err(e),
            }
        }
        None => None,
    };
    // A held answer body is reserved from the pool before it is used.
    let held = response.body.map_or(0, |b| b.len() as u32);
    if held > 0 && !inner.reserve(host, held) {
        return Err(ERR_NO_MEMORY);
    }
    let reply = Reply {
        owner: inner.serving.served[at].owner,
        response: &response,
        file_len,
    };
    let id = inner.serving.served[at].id;
    if let Err(error) = host.transport().reply(id, &reply) {
        inner.release(host, held);
        let s = inner.serving.served.remove(at);
        host.remove_received(s.owner, s.token, s.id);
        return Err(error);
    }
    let s = &mut inner.serving.served[at];
    s.stage = Stage::Answering;
    s.reserved = held;
    host.remove_received(s.owner, s.token, s.id);
    Ok(())
}

pub(super) fn scan(inner: &Inner, owner: u64, best: &mut Option<(u64, Pick)>) {
    for (index, e) in inner.serving.events.iter().enumerate() {
        if e.owner == owner && best.as_ref().is_none_or(|(s, _)| e.stamp < *s) {
            *best = Some((e.stamp, Pick::Server(index)));
        }
    }
    for s in &inner.serving.served {
        if s.owner == owner
            && s.stage == Stage::Told
            && best.as_ref().is_none_or(|(b, _)| s.stamp < *b)
        {
            *best = Some((s.stamp, Pick::Served(s.id)));
        }
    }
}

pub(super) fn encode(
    inner: &mut Inner,
    now: u64,
    pick: Pick,
    out: &mut [u8],
) -> Result<usize, i32> {
    match pick {
        Pick::Server(index) => {
            let e = inner.serving.events.get(index).ok_or(ERR_NOT_FOUND)?;
            let record = Record::EventServer(wire::EventServer {
                now_ms: e.at,
                server: e.token,
                state: e.state,
                reason: e.why,
                address: &e.address,
            });
            let used = codec::encode_event(&record, out)?;
            inner.serving.events.remove(index);
            Ok(used)
        }
        Pick::Served(id) => {
            let s = inner
                .serving
                .served
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or(ERR_NOT_FOUND)?;
            let record = Record::EventServedRequest(wire::EventServedRequest {
                now_ms: now,
                server: s.token,
                request: s.id,
                method: &s.method,
                target: &s.target,
                flags: u32::from(s.body_len > 0),
                body_len: s.body_len,
            });
            let used = codec::encode_event(&record, out)?;
            s.stage = Stage::Open { since: now };
            Ok(used)
        }
        _ => Err(ERR_INVALID),
    }
}
