//! Plugins' sockets (see [ADR 0014](../../../docs/adr/0014-sockets-for-plugins.md)):
//! TCP connections and UDP broadcasts the shell makes, numbered for it,
//! and what it reports turned into the plugins' events. A file read goes
//! to storage in parts as they come; text keeps its characters whole
//! where a read ends inside one. TLS is the shell's: the kernel checks
//! what a plugin set up, says when a socket may switch to it, and tells
//! the plugin what the shell reports.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::Ask;
use paperpaw_platform::Storage;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{
    CertificateHash, Distrust, Id, KeyHash, SocketError, SocketNotice, SocketRead, Tls, Trust,
};

/// The most a shell is asked to read at once; a file read goes in such
/// parts.
pub const SOCKET_PART: u32 = 64 * 1024;

/// How a shell sets up a socket's TLS (`SocketCall::Connect`'s `tls`,
/// `SocketCall::StartTls`), as the plugin asked, checked
/// ([`Tls::check`]) and with the defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsSetup {
    /// The name asked for (SNI) and the certificate is checked for: the
    /// host, unless the plugin named another.
    pub server_name: String,
    /// Which certificates are trusted: the authorities the shell ships
    /// (chain, name and dates checked), or those pinned (the leaf's
    /// SHA-256 one of them, or its key's, its dates checked).
    pub trust: Trust,
    /// How long the connection and handshake may take, waiting for a free
    /// TLS session counted (then `Failed`, `Timeout` or `Busy`).
    pub timeout_ms: u32,
    /// Nothing sent or read for this long: close the socket and report
    /// `Failed` with `Timeout` and the reason `idle`; 0: never.
    pub idle_ms: u32,
}

impl TlsSetup {
    fn new(tls: Tls, host: &str) -> Self {
        Self {
            server_name: tls.server_name.unwrap_or_else(|| String::from(host)),
            trust: tls.trust,
            timeout_ms: tls.timeout_ms,
            idle_ms: tls.idle_ms,
        }
    }
}

/// What a shell does for plugins' sockets ([`Io::Socket`](super::Io::Socket)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketCall {
    /// Open a TCP connection; report `Connected` or `Failed`. With `tls`,
    /// make it secure first and report `Secured`, `Untrusted` or `Failed`
    /// instead of `Connected`. A shell with no TLS ([`tls_sessions`] 0) is
    /// never asked.
    ///
    /// TLS, here and in `StartTls`: **nothing goes out or is read until
    /// `Secured`**. The kernel sends no `Send` or `Read` for the socket
    /// before it (a plugin's fail it, `NotReady`, and close it), and the
    /// shell passes no byte of the plugin's out, nor one it received to
    /// the kernel, before the handshake is done. The certificate is
    /// checked as `TlsSetup::trust` says, its dates where the shell knows
    /// the time: where it does not (no clock, no network time yet), it
    /// checks the rest and reports `Secured` with `time_checked` false;
    /// `Failed` with `Expired` only for a time known and outside the dates.
    ///
    /// [`tls_sessions`]: crate::KernelBuilder::tls_sessions
    Connect {
        socket: u32,
        host: String,
        port: u16,
        tls: Option<TlsSetup>,
    },
    Send {
        socket: u32,
        data: Vec<u8>,
    },
    /// Report what comes next as one `Data` of at most `max` bytes (at
    /// least one, or none when the other side closed).
    Read {
        socket: u32,
        max: u32,
    },
    Close {
        socket: u32,
    },
    /// Send `data` to the local network's broadcast address on each of
    /// `ports`; report each answer within `wait_ms` as `Answer`, then
    /// `Done`.
    Broadcast {
        broadcast: u32,
        ports: Vec<u16>,
        data: Vec<u8>,
        wait_ms: u32,
    },
    /// Switch the connected socket to TLS (STARTTLS); report `Secured`,
    /// `Untrusted` or `Failed`. Nothing is read or waits to be read: the
    /// kernel asks only so. Bytes that come before the handshake starts
    /// (a server sending past its go-ahead) fail it, `NotReady`: they
    /// would pass as secure.
    StartTls {
        socket: u32,
        tls: TlsSetup,
    },
}

/// What happened with a socket or a broadcast (see `Kernel::socket_event`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketEvent {
    Connected {
        socket: u32,
    },
    /// The handshake is done, `certificate` (the server's, by the SHA-256
    /// of its DER) and its `key` (of its SubjectPublicKeyInfo) trusted;
    /// `time_checked` false where its dates could not be checked (the
    /// time unknown).
    Secured {
        socket: u32,
        certificate: CertificateHash,
        key: KeyHash,
        time_checked: bool,
    },
    /// The server's certificate was not trusted, for `why`. The shell has
    /// closed the socket already: no `Close` comes for it.
    Untrusted {
        socket: u32,
        certificate: CertificateHash,
        key: KeyHash,
        why: Distrust,
    },
    /// What a `Read` brought; empty when the other side closed.
    Data {
        socket: u32,
        data: Vec<u8>,
    },
    /// The connection ended or could not be made: `error` says why,
    /// `reason` more, for a log. The shell has closed the socket already:
    /// no `Close` comes for it.
    Failed {
        socket: u32,
        error: SocketError,
        reason: String,
    },
    Answer {
        broadcast: u32,
        from: String,
        data: Vec<u8>,
    },
    Done {
        broadcast: u32,
    },
}

/// What a socket is reading.
#[derive(Debug)]
enum Reading {
    /// Text, up to `max` bytes a read.
    Text { max: u32 },
    /// Bytes as they come.
    Bytes,
    /// Into `path`, `left` bytes still to come.
    File { path: String, left: u64 },
}

/// How far a socket is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Connecting, without TLS.
    Connecting,
    /// Connected, without TLS.
    Open,
    /// Connecting with TLS, or switching to it: `Connected` is not told.
    Securing,
    /// Encrypted.
    Secure,
}

/// What a socket is of: the screens open when it was opened (it closes
/// once fewer are), or a run of background work (it closes when the run
/// ends, whatever screens open and close meanwhile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    Screens(usize),
    Run(u32),
}

/// A plugin's [`Ask::Connect`], taken apart.
struct Connect {
    socket: Id,
    host: String,
    port: u16,
    events: Id,
    tls: Option<alloc::boxed::Box<Tls>>,
}

#[derive(Debug)]
struct Socket {
    number: u32,
    id: Id,
    events: Id,
    /// What it connected to: the name a switch to TLS checks, unless the
    /// plugin names another.
    host: String,
    phase: Phase,
    owner: Owner,
    reading: Option<Reading>,
    /// The start of a character a text read ended in (at most 3 bytes),
    /// kept for the next one.
    partial: Vec<u8>,
}

/// The sockets and broadcasts of all plugins.
#[derive(Debug, Default)]
pub(crate) struct Sockets {
    sockets: Vec<Socket>,
    /// Broadcasts waiting for answers: number, id, events.
    broadcasts: Vec<(u32, Id, Id)>,
    next: u32,
    calls: Vec<SocketCall>,
    /// How many TLS sessions the shell holds (0: no TLS).
    tls_sessions: u8,
}

/// The command that tells a plugin `notice` on `events`.
fn emit(events: &Id, notice: SocketNotice) -> Ask {
    Ask::Emit(events.clone(), notice.to_params())
}

impl Sockets {
    /// The sockets of a shell that holds `tls_sessions` TLS sessions at
    /// once (0: it does no TLS).
    pub fn new(tls_sessions: u8) -> Self {
        Self {
            tls_sessions,
            ..Self::default()
        }
    }

    /// What the shell is to do.
    pub fn take(&mut self) -> Vec<SocketCall> {
        core::mem::take(&mut self.calls)
    }

    /// Whether calls wait for the shell (for a network).
    pub fn waiting(&self) -> bool {
        !self.calls.is_empty()
    }

    /// The calls waited too long for a network: each fails, for `reason`
    /// (a broadcast ends).
    pub fn fail_waiting(&mut self, reason: &str, storage: &dyn Storage, out: &mut Vec<Ask>) {
        for call in self.take() {
            let event = match call {
                SocketCall::Connect { socket, .. }
                | SocketCall::Send { socket, .. }
                | SocketCall::Read { socket, .. }
                | SocketCall::Close { socket }
                | SocketCall::StartTls { socket, .. } => SocketEvent::Failed {
                    socket,
                    error: SocketError::NoNetwork,
                    reason: String::from(reason),
                },
                SocketCall::Broadcast { broadcast, .. } => SocketEvent::Done { broadcast },
            };
            self.event(event, storage, out);
        }
    }

    fn number(&mut self) -> u32 {
        self.next = self.next.wrapping_add(1);
        self.next
    }

    fn at(&self, id: &Id) -> Option<usize> {
        self.sockets.iter().position(|s| s.id == *id)
    }

    /// Carries out a plugin's socket command, with `network` whether the
    /// device has one and `owner` what a socket it opens is of; events to
    /// emit go to `out`.
    pub(crate) fn command(
        &mut self,
        command: Ask,
        network: bool,
        owner: Owner,
        storage: &dyn Storage,
        out: &mut Vec<Ask>,
    ) {
        match command {
            Ask::Connect {
                socket,
                host,
                port,
                events,
                tls,
                ..
            } => {
                let connect = Connect {
                    socket,
                    host,
                    port,
                    events,
                    tls,
                };
                self.connect(connect, network, owner, out);
            }
            Ask::SocketSend { socket, data, .. } => {
                if let Some(at) = self.at(&socket) {
                    if self.securing(at, out) {
                        return;
                    }
                    let number = self.sockets[at].number;
                    self.calls.push(SocketCall::Send {
                        socket: number,
                        data,
                    });
                }
            }
            Ask::SocketRead { socket, read, .. } => {
                if let Some(at) = self.at(&socket) {
                    if self.securing(at, out) {
                        return;
                    }
                    self.read(at, read, storage, out);
                }
            }
            Ask::StartTls { socket, tls, .. } => {
                if let Some(at) = self.at(&socket) {
                    self.start_tls(at, *tls, out);
                }
            }
            Ask::Disconnect(socket) => {
                if let Some(at) = self.at(&socket) {
                    self.close(at, None, out);
                }
            }
            Ask::Broadcast(broadcast) => {
                let (id, events) = (broadcast.id().clone(), broadcast.events().clone());
                let (ports, wait_ms) = (broadcast.ports().to_vec(), broadcast.wait_ms());
                let data = broadcast.into_data();
                if !network {
                    let reason = String::from("no network");
                    out.push(emit(&events, SocketNotice::Done { socket: id, reason }));
                    return;
                }
                let number = self.number();
                self.broadcasts.retain(|(_, b, _)| *b != id);
                self.broadcasts.push((number, id, events));
                self.calls.push(SocketCall::Broadcast {
                    broadcast: number,
                    ports,
                    data,
                    wait_ms,
                });
            }
            _ => {}
        }
    }

    /// Opens a socket, in place of one with its id; secure from the start
    /// where `tls` says, if the shell does TLS and it is set up right.
    fn connect(&mut self, connect: Connect, network: bool, owner: Owner, out: &mut Vec<Ask>) {
        let Connect {
            socket,
            host,
            port,
            events,
            tls,
        } = connect;
        if let Some(at) = self.at(&socket) {
            let old = self.sockets.remove(at);
            self.calls.push(SocketCall::Close { socket: old.number });
        }
        let refused = if !network {
            Some((SocketError::NoNetwork, "no network"))
        } else {
            tls.as_deref().and_then(|tls| self.tls_refused(tls, owner))
        };
        if let Some((error, reason)) = refused {
            let reason = String::from(reason);
            let failed = SocketNotice::Failed {
                socket,
                reason,
                error,
            };
            out.push(emit(&events, failed));
            return;
        }
        let number = self.number();
        let tls = tls.map(|tls| TlsSetup::new(*tls, &host));
        self.sockets.push(Socket {
            number,
            id: socket,
            events,
            host: host.clone(),
            phase: if tls.is_some() {
                Phase::Securing
            } else {
                Phase::Connecting
            },
            owner,
            reading: None,
            partial: Vec::new(),
        });
        self.calls.push(SocketCall::Connect {
            socket: number,
            host,
            port,
            tls,
        });
    }

    /// Whether socket `at` waits for its handshake: then what a plugin
    /// sends or reads fails it, `NotReady`, and closes it. Nothing goes
    /// out or is read until `Secured`: sent, it could go in the clear, and
    /// read, bytes from before the handshake would pass as secure.
    fn securing(&mut self, at: usize, out: &mut Vec<Ask>) -> bool {
        if self.sockets[at].phase != Phase::Securing {
            return false;
        }
        let socket = self.sockets.remove(at);
        self.calls.push(SocketCall::Close {
            socket: socket.number,
        });
        let reason = String::from("nothing is sent or read before the connection is secure");
        out.push(socket.notice(|socket| SocketNotice::Failed {
            socket,
            reason,
            error: SocketError::NotReady,
        }));
        true
    }

    /// Why TLS as `tls` sets it up is refused here for a socket of
    /// `owner`, if it is. A run of background work's socket ends when
    /// idle: one never ending (`idle_ms` 0) would hold a connection (on
    /// the X4 its one TLS session) for good, the run being no screen that
    /// closes.
    fn tls_refused(&self, tls: &Tls, owner: Owner) -> Option<(SocketError, &'static str)> {
        if self.tls_sessions == 0 {
            return Some((
                SocketError::Unsupported,
                "secure connections are not supported here",
            ));
        }
        if matches!(owner, Owner::Run(_)) && tls.idle_ms == 0 {
            return Some((
                SocketError::Options,
                "background work's secure connections end when idle (idle_ms 0)",
            ));
        }
        tls.check()
            .err()
            .map(|error| (error, "the secure connection is set up wrong"))
    }

    /// Asks socket `at` for what comes next, as `read` says.
    fn read(&mut self, at: usize, read: SocketRead, storage: &dyn Storage, out: &mut Vec<Ask>) {
        let s = &mut self.sockets[at];
        // A read that cannot start fails, the socket left as it was.
        let fail = |s: &Socket, error, reason: String| {
            s.notice(|socket| SocketNotice::Failed {
                socket,
                reason,
                error,
            })
        };
        if s.reading.is_some() {
            let reason = String::from("already reading");
            out.push(fail(s, SocketError::NotReady, reason));
            return;
        }
        let max = match read {
            SocketRead::Text { max } => {
                let max = max.clamp(1, SOCKET_PART);
                s.reading = Some(Reading::Text { max });
                max
            }
            SocketRead::Bytes { max } => {
                let max = max.clamp(1, SOCKET_PART);
                s.reading = Some(Reading::Bytes);
                max
            }
            SocketRead::File { path, len } => {
                if let Err(e) = storage.write(&path, &[]) {
                    out.push(fail(s, SocketError::Io, format!("{e}")));
                    return;
                }
                if len == 0 {
                    out.push(s.notice(|socket| SocketNotice::Saved { socket, path }));
                    return;
                }
                let max = len.min(u64::from(SOCKET_PART)) as u32;
                s.reading = Some(Reading::File { path, left: len });
                max
            }
            _ => {
                let reason = String::from("a way of reading not known");
                out.push(fail(s, SocketError::Unsupported, reason));
                return;
            }
        };
        self.calls.push(SocketCall::Read {
            socket: s.number,
            max,
        });
    }

    /// Switches socket `at` to TLS, as `tls` sets up, where it may: TLS
    /// done here and set up right, the socket connected without it,
    /// nothing being read nor come unread (the start of a character a
    /// text read ended in). Else it fails and closes: going on unsecured
    /// would send in the clear what the plugin means to send secured.
    fn start_tls(&mut self, at: usize, tls: Tls, out: &mut Vec<Ask>) {
        let s = &self.sockets[at];
        let refused = self.tls_refused(&tls, s.owner).or_else(|| {
            let ready = s.phase == Phase::Open && s.reading.is_none() && s.partial.is_empty();
            (!ready).then_some((
                SocketError::NotReady,
                "not ready for a secure connection: not connected, already secure, or reading",
            ))
        });
        if let Some((error, reason)) = refused {
            let socket = self.sockets.remove(at);
            self.calls.push(SocketCall::Close {
                socket: socket.number,
            });
            let reason = String::from(reason);
            out.push(socket.notice(|socket| SocketNotice::Failed {
                socket,
                reason,
                error,
            }));
            return;
        }
        let s = &mut self.sockets[at];
        s.phase = Phase::Securing;
        let tls = TlsSetup::new(tls, &s.host);
        self.calls.push(SocketCall::StartTls {
            socket: s.number,
            tls,
        });
    }

    /// Closes socket `at` for the shell and tells its plugin, with why if
    /// it was not asked to.
    fn close(&mut self, at: usize, reason: Option<String>, out: &mut Vec<Ask>) {
        let socket = self.sockets.remove(at);
        self.calls.push(SocketCall::Close {
            socket: socket.number,
        });
        let reason = reason.unwrap_or_default();
        out.push(socket.notice(|socket| SocketNotice::Closed { socket, reason }));
    }

    /// Closes the sockets opened while more than `screens` screens were
    /// open (not those of background work).
    pub fn close_for_closed_screens(&mut self, screens: usize, out: &mut Vec<Ask>) {
        let closed = |s: &Socket| matches!(s.owner, Owner::Screens(n) if n > screens);
        while let Some(at) = self.sockets.iter().position(closed) {
            self.close(at, Some(String::from("screen closed")), out);
        }
    }

    /// Closes the sockets of the run of background work `run`: it ended.
    pub(crate) fn close_for_run(&mut self, run: u32, out: &mut Vec<Ask>) {
        while let Some(at) = self.sockets.iter().position(|s| s.owner == Owner::Run(run)) {
            self.close(at, Some(String::from("work ended")), out);
        }
    }

    /// Whether the run of background work `run` has sockets open.
    pub(crate) fn of_run(&self, run: u32) -> bool {
        self.sockets.iter().any(|s| s.owner == Owner::Run(run))
    }

    /// Whether a socket or broadcast is open, e.g. to keep polling.
    pub fn any(&self) -> bool {
        !self.sockets.is_empty() || !self.broadcasts.is_empty()
    }

    /// Turns what the shell reports into the plugins' events.
    pub fn event(&mut self, event: SocketEvent, storage: &dyn Storage, out: &mut Vec<Ask>) {
        match event {
            SocketEvent::Connected { socket } => {
                // A socket secure from the start says so once it is.
                if let Some(s) = self.socket_in(socket, Phase::Connecting) {
                    s.phase = Phase::Open;
                    out.push(s.notice(|socket| SocketNotice::Connected { socket }));
                }
            }
            SocketEvent::Secured {
                socket,
                certificate,
                key,
                time_checked,
            } => {
                if let Some(s) = self.socket_in(socket, Phase::Securing) {
                    s.phase = Phase::Secure;
                    out.push(s.notice(|socket| SocketNotice::Secured {
                        socket,
                        certificate,
                        key,
                        time_checked,
                    }));
                }
            }
            // The shell closed it (as for `Failed`).
            SocketEvent::Untrusted {
                socket,
                certificate,
                key,
                why,
            } => {
                if let Some(at) = self.sockets.iter().position(|s| s.number == socket) {
                    let s = self.sockets.remove(at);
                    out.push(s.notice(|socket| SocketNotice::Untrusted {
                        socket,
                        certificate,
                        key,
                        why,
                    }));
                }
            }
            SocketEvent::Failed {
                socket,
                error,
                reason,
            } => {
                if let Some(at) = self.sockets.iter().position(|s| s.number == socket) {
                    let s = self.sockets.remove(at);
                    out.push(s.notice(|socket| SocketNotice::Failed {
                        socket,
                        reason,
                        error,
                    }));
                }
            }
            SocketEvent::Data { socket, data } => {
                let Some(at) = self.sockets.iter().position(|s| s.number == socket) else {
                    return;
                };
                self.received(at, data, storage, out);
            }
            SocketEvent::Answer {
                broadcast,
                from,
                data,
            } => {
                if let Some((_, id, events)) =
                    self.broadcasts.iter().find(|(n, ..)| *n == broadcast)
                {
                    let text = String::from_utf8_lossy(&data).into_owned();
                    let socket = id.clone();
                    out.push(emit(events, SocketNotice::Answer { socket, text, from }));
                }
            }
            SocketEvent::Done { broadcast } => {
                if let Some(at) = self.broadcasts.iter().position(|(n, ..)| *n == broadcast) {
                    let (_, socket, events) = self.broadcasts.remove(at);
                    let reason = String::new();
                    out.push(emit(&events, SocketNotice::Done { socket, reason }));
                }
            }
        }
    }

    /// Socket number `socket`, if it is in `phase`.
    fn socket_in(&mut self, socket: u32, phase: Phase) -> Option<&mut Socket> {
        self.sockets
            .iter_mut()
            .find(|s| s.number == socket && s.phase == phase)
    }

    /// What a read of socket `at` brought.
    fn received(&mut self, at: usize, data: Vec<u8>, storage: &dyn Storage, out: &mut Vec<Ask>) {
        let s = &mut self.sockets[at];
        match s.reading.take() {
            // The other side closed; half a character is no text.
            Some(Reading::Text { .. }) if data.is_empty() => {
                s.partial.clear();
                let text = String::new();
                out.push(s.notice(|socket| SocketNotice::Received { socket, text }));
            }
            // As they came, with the start of a character a text read
            // before left.
            Some(Reading::Bytes) => {
                let mut bytes = core::mem::take(&mut s.partial);
                bytes.extend_from_slice(&data);
                let data = bytes;
                out.push(s.notice(|socket| SocketNotice::Bytes { socket, data }));
            }
            Some(Reading::Text { max }) => {
                let mut bytes = core::mem::take(&mut s.partial);
                bytes.extend_from_slice(&data);
                let whole = bytes.len() - incomplete_end(&bytes);
                s.partial = bytes.split_off(whole);
                if bytes.is_empty() {
                    // Only the start of a character came: read on, as
                    // empty text would say the other side closed.
                    s.reading = Some(Reading::Text { max });
                    let socket = s.number;
                    self.calls.push(SocketCall::Read { socket, max });
                    return;
                }
                let text = String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                out.push(s.notice(|socket| SocketNotice::Received { socket, text }));
            }
            Some(Reading::File { path, left }) => {
                if data.is_empty() {
                    let reason = String::from("closed before the file came");
                    return self.close(at, Some(reason), out);
                }
                if let Err(e) = storage.append(&path, &data) {
                    let reason = format!("{e}");
                    return self.close(at, Some(reason), out);
                }
                let left = left.saturating_sub(data.len() as u64);
                if left == 0 {
                    out.push(s.notice(|socket| SocketNotice::Saved { socket, path }));
                } else {
                    let max = left.min(u64::from(SOCKET_PART)) as u32;
                    s.reading = Some(Reading::File { path, left });
                    self.calls.push(SocketCall::Read {
                        socket: s.number,
                        max,
                    });
                }
            }
            // Data not asked for: shells send none.
            None => {}
        }
    }
}

/// How many bytes at the end of `bytes` begin a UTF-8 character that is
/// not complete yet (0 to 3).
fn incomplete_end(bytes: &[u8]) -> usize {
    let start = bytes.len().saturating_sub(3);
    for at in (start..bytes.len()).rev() {
        let byte = bytes[at];
        if byte & 0xC0 == 0x80 {
            // A continuation byte: its character starts before.
            continue;
        }
        let length = match byte {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        };
        let present = bytes.len() - at;
        return if present < length { present } else { 0 };
    }
    0
}

impl Socket {
    /// The command that tells its plugin the notice `make` makes of its id.
    fn notice(&self, make: impl FnOnce(Id) -> SocketNotice) -> Ask {
        emit(&self.events, make(self.id.clone()))
    }
}

#[cfg(test)]
mod tls_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::builtin::params;
    use paperpaw_sdk::socket_params as p;

    const EVENTS: Id = Id::new_static("test.socket-events");
    const SOCKET: Id = Id::new_static("test.socket");

    fn kinds(out: &[Ask]) -> Vec<String> {
        out.iter()
            .filter_map(|c| match c {
                Ask::Emit(_, params) => params.get(p::KIND).map(String::from),
                _ => None,
            })
            .collect()
    }

    fn connected(storage: &MemoryStorage) -> (Sockets, Vec<Ask>) {
        let mut sockets = Sockets::new(1);
        let mut out = Vec::new();
        let connect = Ask::connect(SOCKET, String::from("calibre.local"), 9090, EVENTS);
        sockets.command(connect, true, Owner::Screens(2), storage, &mut out);
        assert_eq!(
            sockets.calls.pop(),
            Some(SocketCall::Connect {
                socket: 1,
                host: String::from("calibre.local"),
                port: 9090,
                tls: None,
            })
        );
        sockets.event(SocketEvent::Connected { socket: 1 }, storage, &mut out);
        (sockets, out)
    }

    #[test]
    fn text_is_read_when_asked() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        let read = Ask::socket_read(SOCKET, SocketRead::Text { max: 100 });
        sockets.command(read.clone(), true, Owner::Screens(2), &storage, &mut out);
        // A second read before the first came fails.
        sockets.command(read, true, Owner::Screens(2), &storage, &mut out);
        assert_eq!(
            sockets.calls,
            [SocketCall::Read {
                socket: 1,
                max: 100
            }]
        );
        let data = SocketEvent::Data {
            socket: 1,
            data: b"12[0,{}]".to_vec(),
        };
        sockets.event(data, &storage, &mut out);
        assert_eq!(kinds(&out), [p::CONNECTED, p::FAILED, p::RECEIVED]);
        let Some(Ask::Emit(_, params)) = out.last() else {
            panic!("no event");
        };
        assert_eq!(params.get(p::TEXT), Some("12[0,{}]"));
        assert_eq!(params.get(params::ID), Some(SOCKET.as_str()));
    }

    #[test]
    fn characters_split_between_reads_stay_whole() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        let mut texts = Vec::new();
        // "Grüße 🐾" with each read ending inside a character, one read
        // bringing only the start of one.
        let reads: [&[u8]; 5] = [b"Gr\xc3", b"\xbc\xc3", b"\x9fe \xf0", b"\x9f\x90", b"\xbe"];
        for data in reads {
            out.clear();
            let read = Ask::socket_read(SOCKET, SocketRead::Text { max: 4 });
            if sockets.calls.is_empty() {
                sockets.command(read, true, Owner::Screens(2), &storage, &mut out);
            }
            assert_eq!(
                sockets.calls.pop(),
                Some(SocketCall::Read { socket: 1, max: 4 })
            );
            let data = SocketEvent::Data {
                socket: 1,
                data: data.to_vec(),
            };
            sockets.event(data, &storage, &mut out);
            if let Some(Ask::Emit(_, params)) = out.last() {
                texts.push(String::from(params.get(p::TEXT).unwrap()));
            }
        }
        assert_eq!(texts, ["Gr", "ü", "ße ", "🐾"]);
        // An invalid byte is replaced, as before.
        let read = Ask::socket_read(SOCKET, SocketRead::Text { max: 4 });
        sockets.command(read, true, Owner::Screens(2), &storage, &mut out);
        let data = SocketEvent::Data {
            socket: 1,
            data: b"a\xffb".to_vec(),
        };
        sockets.event(data, &storage, &mut out);
        let Some(Ask::Emit(_, params)) = out.last() else {
            panic!("no event");
        };
        assert_eq!(params.get(p::TEXT), Some("a\u{fffd}b"));
    }

    #[test]
    fn files_come_in_parts_into_storage() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        let len = u64::from(SOCKET_PART) + 10;
        let read = Ask::socket_read(
            SOCKET,
            SocketRead::File {
                path: String::from("books/new.epub"),
                len,
            },
        );
        sockets.command(read, true, Owner::Screens(2), &storage, &mut out);
        assert_eq!(
            sockets.calls.pop(),
            Some(SocketCall::Read {
                socket: 1,
                max: SOCKET_PART
            })
        );
        let part = alloc::vec![7u8; SOCKET_PART as usize];
        sockets.event(
            SocketEvent::Data {
                socket: 1,
                data: part,
            },
            &storage,
            &mut out,
        );
        assert_eq!(
            sockets.calls.pop(),
            Some(SocketCall::Read { socket: 1, max: 10 })
        );
        let rest = alloc::vec![8u8; 10];
        sockets.event(
            SocketEvent::Data {
                socket: 1,
                data: rest,
            },
            &storage,
            &mut out,
        );
        assert_eq!(kinds(&out), [p::CONNECTED, p::SAVED]);
        assert_eq!(storage.size("books/new.epub").unwrap(), len);
        assert!(sockets.calls.is_empty());
    }

    #[test]
    fn closing_too_early_closes_the_socket() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        let read = Ask::socket_read(
            SOCKET,
            SocketRead::File {
                path: String::from("b.epub"),
                len: 5,
            },
        );
        sockets.command(read, true, Owner::Screens(2), &storage, &mut out);
        sockets.calls.clear();
        sockets.event(
            SocketEvent::Data {
                socket: 1,
                data: Vec::new(),
            },
            &storage,
            &mut out,
        );
        assert_eq!(kinds(&out), [p::CONNECTED, p::CLOSED]);
        assert_eq!(sockets.calls, [SocketCall::Close { socket: 1 }]);
        assert!(!sockets.any());
    }

    #[test]
    fn sockets_end_with_their_screen() {
        let storage = MemoryStorage::new();
        let (mut sockets, mut out) = connected(&storage);
        sockets.close_for_closed_screens(2, &mut out);
        assert!(sockets.any());
        sockets.close_for_closed_screens(1, &mut out);
        assert!(!sockets.any());
        assert_eq!(kinds(&out), [p::CONNECTED, p::CLOSED]);
    }

    #[test]
    fn without_a_network_everything_fails_at_once() {
        let storage = MemoryStorage::new();
        let mut sockets = Sockets::default();
        let mut out = Vec::new();
        let connect = Ask::connect(SOCKET, String::from("h"), 1, EVENTS);
        sockets.command(connect, false, Owner::Screens(1), &storage, &mut out);
        let broadcast = Ask::broadcast(SOCKET, alloc::vec![54982], b"hi".to_vec(), 100, EVENTS);
        sockets.command(broadcast, false, Owner::Screens(1), &storage, &mut out);
        assert_eq!(kinds(&out), [p::FAILED, p::DONE]);
        assert!(sockets.calls.is_empty());
    }

    #[test]
    fn broadcast_answers_then_done() {
        let storage = MemoryStorage::new();
        let mut sockets = Sockets::default();
        let mut out = Vec::new();
        let broadcast = Ask::broadcast(
            SOCKET,
            alloc::vec![54982, 48123],
            b"hi".to_vec(),
            500,
            EVENTS,
        );
        sockets.command(broadcast, true, Owner::Screens(1), &storage, &mut out);
        assert!(matches!(
            sockets.calls[0],
            SocketCall::Broadcast { broadcast: 1, .. }
        ));
        let answer = SocketEvent::Answer {
            broadcast: 1,
            from: String::from("192.168.1.5"),
            data: b"calibre wireless device client (on pc);8080,9090".to_vec(),
        };
        sockets.event(answer, &storage, &mut out);
        sockets.event(SocketEvent::Done { broadcast: 1 }, &storage, &mut out);
        assert_eq!(kinds(&out), [p::ANSWER, p::DONE]);
        let Ask::Emit(_, params) = &out[0] else {
            panic!("no answer");
        };
        assert_eq!(params.get(p::FROM), Some("192.168.1.5"));
        assert!(!sockets.any());
    }
}
