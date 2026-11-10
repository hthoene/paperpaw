//! `core.sockets` of interface 7 (`docs/abi/sockets.md`) over the X4's
//! network: what core asks of the transport ([`SockTransport`], [`SockTls`])
//! is queued, a step of the main loop does it with the same TCP, TLS and
//! UDP code the interface-6 sockets use (`sockets`), and what happened goes
//! back through [`SockReports`], each report followed by the kernel's tick
//! (the pump of the services' events runs at the end of every kernel call;
//! nothing polls, an idle socket costs no wake-up).
//!
//! The interface-7 sockets are a [`Sockets`] of their own, beside the
//! interface-6 one: the ids are core's, the TLS sessions are the device's
//! few (`tls_slots`, shared with the interface-6 sockets and requests: a
//! handshake waits there for the one session, and ends `timeout` when it
//! does not come).
//!
//! Nothing here calls into core from inside a [`SockTransport`] or
//! [`SockTls`] method: they only queue. The step runs the queue with the
//! table of the sockets borrowed, lets go of it, and only then reports: a
//! report ends in a tick, which can call the transport again.
//!
//! The clock is the main loop's own ([`crate::now`]): [`ShellNetwork::now_ms`]
//! and the `Instant` given to [`Kernel::tick`] are the same one, so that the
//! deadline the kernel wakes at is the services'.

mod http;
mod serving;

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use log::{info, warn};
use paperpaw_core::native_host::shell_wiring::{
    Connect, NetReports, PeerInfo, ShellNetwork, SockFailure, SockReports, SockTls, SockTransport,
    TlsFailure, TlsOpen, UntrustedWhy,
};
use paperpaw_core::sdk::{CertificateHash, Distrust, KeyHash, SocketError, Trust};
use paperpaw_core::{Effects, Kernel, SocketCall, SocketEvent, TlsSetup};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_platform::{Instant, Storage};
use paperpaw_x4::net_lost::{NetLost, Service};

use crate::http::Calls;
use crate::net::Net;
use crate::serve7::{self, Key, Reply, Servers7, Verdict};
use crate::sockets::Sockets;

/// The plugin network memory of the device (`SockLimits::pool_bytes`): the
/// X4's 256 KiB leave 48 KiB (ADR 0034).
pub const POOL_BYTES: u32 = 48 * 1024;
/// Bytes a socket takes to send at once: what the connection's send buffer
/// holds (`sockets::BUFFERS`). More waits for `writable`.
const SEND_ROOM: usize = 2048;
/// How long a handshake may take, waiting for the TLS session counted.
const HANDSHAKE_MS: u32 = 30_000;
/// How long a broadcast collects answers: the longest `wait_ms` (10,000)
/// and a little more; core ends the broadcast at its own `wait_ms`.
const BROADCAST_MS: u32 = 11_000;

/// What one step of the sockets came to, before it is reported.
enum Out {
    /// A socket's event.
    Socket(SocketEvent),
    /// A socket that was short of room can take more.
    Writable(u32),
    /// What the HTTP requests came to.
    Net(http::NetOut),
}

/// The reports core gave the shell to make them with.
struct Wired {
    net: Option<NetReports>,
    sockets: Option<SockReports>,
}

struct Inner {
    sockets: Sockets,
    /// What core asked since the last step, in order.
    calls: Vec<SocketCall>,
    /// How long each new connection may take (socket, milliseconds).
    connects: Vec<(u32, u32)>,
    /// Sockets told they could send less than they offered.
    blocked: Vec<u32>,
    /// Sockets core began TLS on that have not been told how it went.
    handshaking: Vec<u32>,
    /// Sockets' send room as of the last step (socket, bytes).
    room: Vec<(u32, usize)>,
    /// Reports that were refused `ERR_BUSY`: the next step makes them again.
    retry: Vec<Out>,
    reports: Option<Wired>,
    /// A network was up at the last step.
    was_up: bool,
    /// Sockets core closed (or ended the TLS of): room for one each is kept
    /// from `connect` on, so that a close is never dropped for want of
    /// memory and a connection never keeps the device's one TLS session.
    closes: Vec<u32>,
    /// Sockets a read or close could not be queued for: they end `failed`
    /// `no_memory` at the next step (room kept from `connect` on).
    broken: Vec<u32>,
    /// Something could not be remembered for want of memory: every socket
    /// ends at the next step (the network is told lost), nothing crashes.
    oom: bool,
    /// The HTTP requests of core (`core.net`).
    http: Calls,
    /// The card, for the answers written to a staged file.
    storage: Option<Rc<dyn Storage>>,
    /// Requests core ended (or never resumed): their connections go.
    aborts: Vec<u32>,
    /// Requests' most body bytes (request, bytes).
    caps: Vec<(u32, usize)>,
    /// Requests were going on at the last step with a network.
    http_was: bool,
    /// A request stalled without an answer: the radio starts anew.
    restart: bool,
    /// The servers of core (`serve`).
    servers: Servers7,
    /// Servers to open, to stop, answers to send and requests to end at the
    /// next step.
    listens: Vec<(u32, u16)>,
    unlistens: Vec<u32>,
    replies: Vec<Reply>,
    closes7: Vec<(u32, u32)>,
    /// What core decided about heads and bodies since the last step.
    verdicts: Vec<(Key, Verdict)>,
    /// The address of the device at the last step, for what `listen` answers.
    address: Option<serve7::Ipv4>,
    /// Reports dropped for a service that was not wired (since last logged).
    unwired: u32,
    /// Answers whose body went with a report core refused busy, since last
    /// logged.
    lost_answers: u32,
    /// The services still to be told the network is lost: flags, never a
    /// list item, so that saying so needs no memory (and is made again where
    /// core is busy).
    lost: NetLost,
}

/// The shell side of `core.sockets`, shared by the loader (which wires core
/// with it) and the network (which runs it).
pub struct Bridge {
    inner: RefCell<Inner>,
    /// A network is up (set by every step).
    up: Cell<bool>,
}

impl Bridge {
    /// A bridge with nothing wired and nothing asked.
    #[must_use]
    pub fn new() -> Rc<Self> {
        let mut sockets = Sockets::default();
        sockets.answers_with_ports();
        Rc::new(Self {
            inner: RefCell::new(Inner {
                sockets,
                calls: Vec::new(),
                connects: Vec::new(),
                blocked: Vec::new(),
                handshaking: Vec::new(),
                room: Vec::new(),
                retry: Vec::new(),
                reports: None,
                was_up: false,
                closes: Vec::new(),
                broken: Vec::new(),
                oom: false,
                http: Calls::default(),
                storage: None,
                aborts: Vec::new(),
                caps: Vec::new(),
                http_was: false,
                restart: false,
                servers: Servers7::default(),
                listens: Vec::new(),
                unlistens: Vec::new(),
                replies: Vec::new(),
                closes7: Vec::new(),
                verdicts: Vec::new(),
                address: None,
                unwired: 0,
                lost_answers: 0,
                lost: NetLost::new(),
            }),
            up: Cell::new(false),
        })
    }

    /// Keeps the reports core gave for what was wired.
    pub fn wired(
        &self,
        storage: Rc<dyn Storage>,
        net: Option<&NetReports>,
        sockets: Option<&SockReports>,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner.storage = Some(storage);
        inner.reports = Some(Wired {
            net: net.cloned(),
            sockets: sockets.cloned(),
        });
    }

    /// Whether a socket is open or asked for: the radio stays awake and the
    /// main loop comes by again soon.
    pub fn moving(&self) -> bool {
        self.inner.try_borrow().map_or(true, |inner| {
            !inner.calls.is_empty()
                || !inner.retry.is_empty()
                || inner.sockets.moving()
                || inner.http.busy()
                || inner.servers.moving()
        })
    }

    /// Whether a socket is open (the radio does not doze).
    pub fn open(&self) -> bool {
        self.inner.try_borrow().map_or(true, |inner| {
            inner.sockets.any()
                || !inner.calls.is_empty()
                || inner.http.busy()
                || inner.servers.any()
        })
    }

    /// One step with the radio on a network: what core asked is done, what
    /// the connections did is collected.
    fn step_online(&self, net: &mut Net, now: Instant, clock_ready: bool) -> Vec<Out> {
        self.up.set(true);
        let mut out = Vec::new();
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return out;
        };
        let inner = &mut *inner;
        inner.was_up = true;
        for item in core::mem::take(&mut inner.retry) {
            push_out(&mut out, item, &mut inner.oom);
        }
        // Another network than the sockets were opened on: they are lost.
        if inner.sockets.stale(net) {
            drop(inner.sockets.lost_events(None));
            inner.forget_all();
            inner.lost.raise_sockets();
        }
        // In the order core asked; the queue keeps its room.
        while !inner.calls.is_empty() {
            let call = inner.calls.remove(0);
            let timed = match &call {
                SocketCall::Connect { socket, .. } => Some(*socket),
                _ => None,
            };
            if let Some(event) = inner.sockets.apply(call, net, now) {
                push_out(&mut out, Out::Socket(event), &mut inner.oom);
            } else if let Some(socket) = timed
                && let Some(at) = inner.connects.iter().position(|(id, _)| *id == socket)
            {
                let (_, ms) = inner.connects.swap_remove(at);
                inner.sockets.connect_until(socket, now.0 + u64::from(ms));
            }
        }
        inner.connects.clear();
        // The closes core asked, after the rest: nothing it asked is undone
        // by an earlier close.
        while let Some(socket) = inner.closes.pop() {
            let _ = inner.sockets.apply(SocketCall::Close { socket }, net, now);
        }
        // Sockets a read or close could not be queued for end here.
        while let Some(socket) = inner.broken.pop() {
            let _ = inner.sockets.apply(SocketCall::Close { socket }, net, now);
            inner.room.retain(|(id, _)| *id != socket);
            inner.blocked.retain(|id| *id != socket);
            let event = SocketEvent::Failed {
                socket,
                error: SocketError::NoMemory,
                reason: String::new(),
            };
            push_out(&mut out, Out::Socket(event), &mut inner.oom);
        }
        for event in inner.sockets.poll_events(net, now, clock_ready) {
            push_out(&mut out, Out::Socket(event), &mut inner.oom);
        }
        inner.settle(&mut out);
        if inner.http_online(net, clock_ready, &mut out) {
            inner.restart = true;
        }
        inner.serve_online(net, now, &mut out);
        if inner.oom {
            // Nothing could be remembered: the sockets end (core is told
            // the network is lost, which needs no memory to say).
            inner.oom = false;
            drop(inner.sockets.lost_events(Some(net)));
            inner.forget_all();
            inner.http.clear(Some(net));
            inner.servers.lost(Some(&mut *net));
            // Core is told by flags, which need no memory.
            out.clear();
            inner.lost.raise_sockets();
            inner.lost.raise_requests();
        }
        out
    }

    /// One step with no network up: what was open is let go and core is told
    /// the network is gone, once.
    fn step_offline(&self, mut net: Option<&mut Net>) -> Vec<Out> {
        self.up.set(false);
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return Vec::new();
        };
        let inner = &mut *inner;
        // Released on the network they were opened on; core hears one
        // `network_lost`, not a failure per socket.
        let had = inner.sockets.any() || !inner.calls.is_empty() || inner.was_up;
        drop(inner.sockets.lost_events(net.as_deref_mut()));
        inner.forget_all();
        inner.calls.clear();
        inner.connects.clear();
        inner.was_up = false;
        if had {
            inner.lost.raise_sockets();
        }
        inner.serve_offline(net.as_deref_mut());
        inner.http_offline(net);
        inner.oom = false;
        // Core is told by the flag, which is made at the report.
        Vec::new()
    }

    /// Reports what the step came to, then the kernel ticks once: the pump
    /// of the services' events runs at the end of that call. The kernel's
    /// effects, for the main loop to apply; `None` when nothing was taken.
    fn report(&self, out: Vec<Out>, kernel: &mut Kernel) -> Option<Effects> {
        let (net, sockets) = self.inner.try_borrow().ok().and_then(|inner| {
            inner
                .reports
                .as_ref()
                .map(|wired| (wired.net.clone(), wired.sockets.clone()))
        })?;
        // One report at a time: what core refuses `ERR_BUSY` is kept for the
        // next step, if there is room to keep it (else the sockets end).
        // The network lost comes first, from its flags (nothing is allocated
        // to say it); where core is busy it stays raised.
        let mut pending = self
            .inner
            .try_borrow_mut()
            .map(|mut inner| inner.lost.take())
            .unwrap_or_default();
        let mut told = pending.report(|service| match service {
            Service::Sockets => sockets
                .as_ref()
                .is_none_or(|sockets| sockets.network_lost() != Err(ERR_BUSY)),
            Service::Requests => net
                .as_ref()
                .is_none_or(|net| net.network_lost() != Err(ERR_BUSY)),
            _ => true,
        });
        if pending.any()
            && let Ok(mut inner) = self.inner.try_borrow_mut()
        {
            inner.lost.merge(pending);
        }
        for item in out {
            match self.report_one(net.as_ref(), sockets.as_ref(), item) {
                Ok(()) => told = true,
                Err(item) => self.keep_for_later(item),
            }
        }
        // The time as it is at the end of the step: deadlines are not judged
        // early (the clock is the one the services' deadlines are in).
        told.then(|| kernel.tick(crate::now()))
    }

    /// A report was dropped for a service that is not wired, counted.
    fn count_unwired(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.unwired = inner.unwired.saturating_add(1);
        }
    }

    /// Keeps a report core refused for the next step.
    fn keep_for_later(&self, item: Out) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            let inner = &mut *inner;
            push_out(&mut inner.retry, item, &mut inner.oom);
        }
    }

    /// One report; the item again where core had the table borrowed. A
    /// report for a service that is not wired is dropped.
    fn report_one(
        &self,
        net: Option<&NetReports>,
        sockets: Option<&SockReports>,
        item: Out,
    ) -> Result<(), Out> {
        if let Out::Net(report) = item {
            let Some(net) = net else {
                self.count_unwired();
                return Ok(());
            };
            return self.report_net(net, report).map_err(Out::Net);
        }
        let Some(sockets) = sockets else {
            self.count_unwired();
            return Ok(());
        };
        let result = match &item {
            Out::Writable(id) => sockets.writable(*id),
            Out::Socket(event) => self.report_socket(sockets, event),
            Out::Net(_) => return Ok(()),
        };
        match result {
            Err(ERR_BUSY) => Err(item),
            // Anything else is core's answer: the report is told or ignored.
            _ => Ok(()),
        }
    }

    fn report_socket(&self, sockets: &SockReports, event: &SocketEvent) -> Result<(), i32> {
        match event {
            SocketEvent::Connected { socket } => sockets.connected(*socket),
            SocketEvent::Secured {
                socket,
                certificate,
                key,
                time_checked,
            } => {
                self.handshake_over(*socket);
                let peer = PeerInfo::new(*certificate.sha256(), *key.sha256(), *time_checked);
                sockets.secured(*socket, &peer)
            }
            SocketEvent::Untrusted {
                socket,
                certificate,
                key,
                why,
            } => {
                self.handshake_over(*socket);
                let peer = PeerInfo::new(*certificate.sha256(), *key.sha256(), false);
                let why = match why {
                    Distrust::NotPinned => UntrustedWhy::NotPinned,
                    _ => UntrustedWhy::UnknownIssuer,
                };
                sockets.untrusted(*socket, why, &peer)
            }
            SocketEvent::Data { socket, data } if data.is_empty() => sockets.peer_closed(*socket),
            SocketEvent::Data { socket, data } => match sockets.received(*socket, data) {
                Ok(taken) if taken < data.len() => {
                    // Only what a read asked for is read: more than that
                    // would be a fault of the transport.
                    warn!("sockets: core took {taken} of {} bytes", data.len());
                    Ok(())
                }
                Ok(_) => Ok(()),
                Err(status) => Err(status),
            },
            SocketEvent::Failed { socket, error, .. } => {
                if self.handshake_over(*socket) {
                    let how = match error {
                        SocketError::NameMismatch => TlsFailure::Name,
                        SocketError::Expired => TlsFailure::Expired,
                        SocketError::NoMemory => TlsFailure::NoMemory,
                        _ => TlsFailure::Tls,
                    };
                    sockets.tls_failed(*socket, how)
                } else {
                    sockets.failed(*socket, failure_of(*error))
                }
            }
            SocketEvent::Answer {
                broadcast,
                from,
                data,
            } => sockets.answered(*broadcast, from.as_bytes(), data),
            // Core ends a broadcast at its own `wait_ms`.
            _ => Ok(()),
        }
    }

    /// Notes the handshake of `socket` over (it was one); whether it was.
    fn handshake_over(&self, socket: u32) -> bool {
        self.inner
            .try_borrow_mut()
            .map(|mut inner| inner.handshaking.take(socket))
            .unwrap_or(false)
    }

    /// Logs how many broadcast answers were dropped and reports ignored
    /// since the last time, once the sockets are all closed (never what
    /// they held). Cheap: a counter each.
    fn log_counts(&self) {
        let (reports, unwired, lost) = match self.inner.try_borrow_mut() {
            Ok(mut inner) => {
                if inner.sockets.any() || !inner.calls.is_empty() || inner.http.busy() {
                    return;
                }
                (
                    inner.reports.as_ref().and_then(|w| w.sockets.clone()),
                    core::mem::take(&mut inner.unwired),
                    core::mem::take(&mut inner.lost_answers),
                )
            }
            Err(_) => return,
        };
        if unwired != 0 || lost != 0 {
            info!(
                "net: {unwired} reports dropped (service not wired), {lost} answers lost (core busy)"
            );
        }
        let Some(sockets) = reports else {
            return;
        };
        let (dropped, ignored) = (sockets.take_dropped(), sockets.take_ignored());
        if dropped != 0 || ignored != 0 {
            info!("sockets: {dropped} broadcast answers dropped, {ignored} reports ignored");
        }
    }
}

impl Inner {
    /// Forgets what was kept for sockets that are gone.
    fn forget_all(&mut self) {
        self.closes.clear();
        self.broken.clear();
        self.blocked.clear();
        self.handshaking.clear();
        self.room.clear();
    }

    /// Notes which sockets ended in `out`, and what room each socket has
    /// now; a socket that was short of room and has some is told.
    fn settle(&mut self, out: &mut Vec<Out>) {
        for item in out.iter() {
            if let Out::Socket(
                SocketEvent::Failed { socket, .. } | SocketEvent::Untrusted { socket, .. },
            ) = item
            {
                self.room.retain(|(id, _)| id != socket);
                self.blocked.retain(|id| id != socket);
            }
        }
        for (socket, room) in &mut self.room {
            *room = SEND_ROOM.saturating_sub(self.sockets.queued(*socket));
        }
        let mut at = 0;
        while at < self.blocked.len() {
            let socket = self.blocked[at];
            let has = self
                .room
                .iter()
                .find(|(id, _)| *id == socket)
                .is_some_and(|(_, room)| *room > 0);
            if has {
                self.blocked.remove(at);
                push_out(out, Out::Writable(socket), &mut self.oom);
            } else {
                at += 1;
            }
        }
    }

    /// Makes room for the next socket's close and failure: one entry each
    /// for every socket core holds, one more than now.
    fn keep_room_for_close(&mut self) -> Result<(), i32> {
        let want = self.room.len() + self.closes.len() + 1;
        let more = want.saturating_sub(self.closes.len());
        self.closes.try_reserve(more).map_err(|_| ERR_NO_MEMORY)?;
        let more = want.saturating_sub(self.broken.len());
        self.broken.try_reserve(more).map_err(|_| ERR_NO_MEMORY)?;
        // The queue keeps room for the three calls a socket usually has.
        self.calls.try_reserve(4).map_err(|_| ERR_NO_MEMORY)?;
        Ok(())
    }

    /// Queues a close of `id`, which needs no memory (it was kept at
    /// `connect`).
    fn queue_close(&mut self, id: u32) {
        if self.closes.contains(&id) {
            return;
        }
        if self.closes.len() == self.closes.capacity() && self.closes.try_reserve(1).is_err() {
            // Not meant to happen: the room was kept. Everything ends at the
            // next step rather than a connection staying open.
            self.oom = true;
            return;
        }
        self.closes.push(id);
    }

    /// Queues `call`; `ERR_NO_MEMORY` when there is no room for it.
    fn ask(&mut self, call: SocketCall) -> Result<(), i32> {
        self.calls.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        self.calls.push(call);
        Ok(())
    }
}

/// `item` into `out`, or `oom` set where there is no room for it.
fn push_out(out: &mut Vec<Out>, item: Out, oom: &mut bool) {
    if out.try_reserve(1).is_ok() {
        out.push(item);
    } else {
        *oom = true;
    }
}

/// A list of ids that can say "had it" while taking it out.
trait Take {
    fn take(&mut self, id: u32) -> bool;
}

impl Take for Vec<u32> {
    fn take(&mut self, id: u32) -> bool {
        match self.iter().position(|x| *x == id) {
            Some(at) => {
                self.swap_remove(at);
                true
            }
            None => false,
        }
    }
}

/// `text` as an owned string, or `ERR_NO_MEMORY`.
fn own(text: &str) -> Result<String, i32> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    Ok(owned)
}

/// What the socket's failure is as core tells it.
fn failure_of(error: SocketError) -> SockFailure {
    match error {
        SocketError::NoNetwork => SockFailure::NoNetwork,
        SocketError::Dns => SockFailure::Dns,
        SocketError::Refused | SocketError::NotAllowed | SocketError::TooMany => {
            SockFailure::Refused
        }
        SocketError::Timeout | SocketError::Busy => SockFailure::Timeout,
        SocketError::Lost => SockFailure::Lost,
        SocketError::NoMemory => SockFailure::NoMemory,
        _ => SockFailure::Io,
    }
}

impl SockTransport for Bridge {
    fn connect(&self, id: u32, to: &Connect<'_>) -> Result<(), i32> {
        let port = u16::try_from(to.port).map_err(|_| ERR_INVALID)?;
        let host = own(to.host)?;
        let mut inner = self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        // First the room for the close and the failure of this socket.
        inner.keep_room_for_close()?;
        inner.connects.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        inner.room.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        inner.ask(SocketCall::Connect {
            socket: id,
            host,
            port,
            tls: None,
        })?;
        inner.connects.push((id, to.timeout_ms));
        inner.room.push((id, SEND_ROOM));
        Ok(())
    }

    fn send(&self, id: u32, data: &[u8]) -> Result<usize, i32> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let room = inner
            .room
            .iter()
            .find(|(socket, _)| *socket == id)
            .map_or(0, |(_, room)| *room);
        let take = data.len().min(room);
        if take < data.len() && !inner.blocked.contains(&id) {
            inner.blocked.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            inner.blocked.push(id);
        }
        if take == 0 {
            return Ok(0);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(take).map_err(|_| ERR_NO_MEMORY)?;
        bytes.extend_from_slice(&data[..take]);
        inner.ask(SocketCall::Send {
            socket: id,
            data: bytes,
        })?;
        if let Some((_, room)) = inner.room.iter_mut().find(|(socket, _)| *socket == id) {
            *room -= take;
        }
        Ok(take)
    }

    fn read(&self, id: u32, max: u32) {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return;
        };
        if inner.ask(SocketCall::Read { socket: id, max }).is_ok() {
            return;
        }
        // Not dropped: the socket ends `no_memory` at the next step.
        if inner.broken.len() < inner.broken.capacity() {
            inner.broken.push(id);
        } else {
            inner.oom = true;
        }
    }

    fn close(&self, id: u32) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.blocked.retain(|socket| *socket != id);
            inner.handshaking.retain(|socket| *socket != id);
            inner.room.retain(|(socket, _)| *socket != id);
            inner.queue_close(id);
        }
    }

    fn broadcast(&self, id: u32, data: &[u8], ports: &[u16]) -> Result<(), i32> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(data.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        bytes.extend_from_slice(data);
        let mut to = Vec::new();
        to.try_reserve_exact(ports.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        to.extend_from_slice(ports);
        let mut inner = self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        inner.ask(SocketCall::Broadcast {
            broadcast: id,
            ports: to,
            data: bytes,
            wait_ms: BROADCAST_MS,
        })
    }
}

impl SockTls for Bridge {
    fn begin(&self, id: u32, to: &TlsOpen<'_>) -> Result<(), i32> {
        let trust = trust_of(to)?;
        let setup = TlsSetup {
            server_name: own(to.server_name)?,
            trust,
            timeout_ms: HANDSHAKE_MS,
            // Idle time is core's: it closes the socket itself.
            idle_ms: 0,
        };
        let mut inner = self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        inner
            .handshaking
            .try_reserve(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        inner.ask(SocketCall::StartTls {
            socket: id,
            tls: setup,
        })?;
        inner.handshaking.push(id);
        Ok(())
    }

    fn end(&self, id: u32) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.handshaking.retain(|socket| *socket != id);
            // The connection of a session that ends is closed (what was
            // secure is never left plain); a close of a socket that is gone
            // changes nothing.
            inner.queue_close(id);
        }
    }
}

/// How the pins of `to` set the trust: none, the authorities; leaf
/// certificates, or keys. A mix is refused.
fn trust_of(to: &TlsOpen<'_>) -> Result<Trust, i32> {
    if to.pins.is_empty() {
        return Ok(Trust::Authorities);
    }
    let certificates = to.pins.iter().filter(|pin| pin.kind == 0).count();
    if certificates == to.pins.len() {
        let mut pins = Vec::new();
        pins.try_reserve_exact(to.pins.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        pins.extend(to.pins.iter().map(|pin| CertificateHash::new(pin.hash)));
        Ok(Trust::Pinned(pins))
    } else if certificates == 0 && to.pins.iter().all(|pin| pin.kind == 1) {
        let mut keys = Vec::new();
        keys.try_reserve_exact(to.pins.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        keys.extend(to.pins.iter().map(|pin| KeyHash::new(pin.hash)));
        Ok(Trust::PinnedKeys(keys))
    } else {
        Err(ERR_INVALID)
    }
}

/// What the device tells `core.work`: the clock `Kernel::tick` is given
/// ([`crate::now`]) and whether a network is up. The radio is the user's and
/// is never turned on for work, so "the radio is on anyway" is the network
/// being up.
impl paperpaw_core::native_host::shell_wiring::WorkDevice for Bridge {
    fn now_ms(&self) -> u64 {
        crate::now().0
    }

    fn network_up(&self) -> bool {
        self.up.get()
    }

    fn radio_on(&self) -> bool {
        self.up.get()
    }
}

impl ShellNetwork for Bridge {
    fn now_ms(&self) -> u64 {
        crate::now().0
    }

    fn network_up(&self) -> bool {
        self.up.get()
    }

    fn network_demand(&self, wanted: bool) {
        // The radio is the user's: the device never turns it on by itself
        // (`network.md` 3.2), so the demand is told to the log only.
        log::debug!("sockets: network wanted {wanted}");
    }
}

impl Bridge {
    /// One step of the interface-7 sockets over `radio`: what core asked, what
    /// the connections did, the reports, and the kernel's tick after them.
    pub fn run(
        &self,
        radio: &mut crate::wifi::Radio,
        kernel: &mut Kernel,
        now: Instant,
        clock_ready: bool,
    ) -> Option<Effects> {
        let out = match radio.online() {
            Some(net) => self.step_online(net, now, clock_ready),
            None => self.step_offline(radio.net()),
        };
        if self
            .inner
            .try_borrow_mut()
            .is_ok_and(|mut inner| core::mem::take(&mut inner.restart))
        {
            // A request stalled without any answer: the radio starts anew.
            radio.restart();
        }
        let effects = self.report(out, kernel);
        self.log_counts();
        effects
    }
}
