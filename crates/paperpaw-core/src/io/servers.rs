//! Plugins' HTTP servers in the local network: what the shell is to do
//! for them ([`Io::Server`]), and what it reports (`Kernel::server_event`)
//! turned into the plugins' [`ServerNotice`]s. A request's body is saved
//! as it comes, so it never needs to fit in memory; bodies left behind
//! (by a crash) go when the next one comes.
//!
//! A server on [`CABLE_PORT`] is reached over the cable instead
//! ([`cable`](super::cable)): it needs no network, the shell does nothing
//! for it but pass lines, and it keeps no radio on.
//!
//! [`Io::Server`]: super::Io::Server

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::served::CABLE_PORT;
use paperpaw_sdk::{Command, Id, ServerNotice, ServerResponse};

use super::cable::{self, Cable};

/// What a shell does for plugins' servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerCall {
    /// Listen on `port` in the local network.
    Listen {
        server: u32,
        port: u16,
    },
    Stop {
        server: u32,
    },
    /// Answer request `request` with `body`, or with `file`.
    Respond {
        request: u32,
        status: u16,
        content_type: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        file: Option<ServedFile>,
    },
}

/// A file sent as an answer: the shell reads it in parts with
/// `Kernel::read_served`, so it never needs to fit in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedFile {
    pub path: String,
    pub size: u64,
}

/// What a shell tells about servers (see `Kernel::server_event`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    /// Listening; reached at `address` (e.g. `192.168.1.20:8080`).
    Started {
        server: u32,
        address: String,
    },
    Failed {
        server: u32,
        reason: String,
    },
    /// A request; answer it with the same `request`. With a `body`, its
    /// parts follow as [`ServerEvent::Body`]; the plugin hears of the
    /// request once the last part came.
    Request {
        server: u32,
        request: u32,
        method: String,
        target: String,
        body: bool,
    },
    /// Part of a request's body, saved as it comes (so it never needs to
    /// fit in memory); `last` with the final part.
    Body {
        request: u32,
        data: Vec<u8>,
        last: bool,
    },
    /// The connection of a request was lost.
    Closed {
        request: u32,
    },
}

/// A server running.
#[derive(Debug)]
struct Running {
    /// Its number for the shell.
    number: u32,
    id: Id,
    events: Id,
    /// Screens open when it started: it stops once fewer are.
    screens: usize,
}

/// A request whose body is still coming.
#[derive(Debug)]
struct Incoming {
    request: u32,
    server: u32,
    method: String,
    target: String,
    /// Where the body goes.
    path: String,
}

/// The servers of all plugins.
#[derive(Debug, Default)]
pub(crate) struct Servers {
    calls: Vec<ServerCall>,
    running: Vec<Running>,
    next: u32,
    /// Bodies of requests not answered yet: request, file.
    bodies: Vec<(u32, String)>,
    incoming: Vec<Incoming>,
    /// The number of the server on the cable, while one runs.
    cable_server: Option<u32>,
    /// The cable's requests and the lines to send.
    pub(crate) cable: Cable,
}

impl Servers {
    /// Starts serving for a plugin (instead of a server it had of that
    /// id), or tells it why not.
    pub fn serve(
        &mut self,
        server: Id,
        port: u16,
        events: Id,
        network: bool,
        screens: usize,
        out: &mut Vec<Command>,
    ) {
        if let Some(at) = self.running.iter().position(|r| r.id == server) {
            let old = self.running.remove(at);
            self.calls.push(ServerCall::Stop { server: old.number });
        }
        let on_cable = port == CABLE_PORT;
        if !network && !on_cable {
            let reason = String::from("no network");
            return tell(&events, ServerNotice::Failed { server, reason }, out);
        }
        self.next = self.next.wrapping_add(1);
        self.running.push(Running {
            number: self.next,
            id: server.clone(),
            events: events.clone(),
            screens,
        });
        if on_cable {
            // Its shell part is the lines passed on: it runs at once.
            self.cable_server = Some(self.next);
            let address = String::from("cable");
            tell(&events, ServerNotice::Started { server, address }, out);
        } else {
            self.calls.push(ServerCall::Listen {
                server: self.next,
                port,
            });
        }
    }

    /// The number of the server on the cable, while one runs.
    pub fn cable_server(&self) -> Option<u32> {
        self.cable_server
    }

    /// Stops a plugin's server, if it runs.
    pub fn stop(&mut self, server: &Id, out: &mut Vec<Command>) {
        if let Some(at) = self.running.iter().position(|r| r.id == *server) {
            self.stop_at(at, out);
        }
    }

    fn stop_at(&mut self, at: usize, out: &mut Vec<Command>) {
        let running = self.running.remove(at);
        if self.cable_server == Some(running.number) {
            // Ended on the device: not opened again by the next line.
            self.cable_server = None;
            self.cable.ended = true;
        } else {
            self.calls.push(ServerCall::Stop {
                server: running.number,
            });
        }
        let server = running.id;
        tell(&running.events, ServerNotice::Stopped { server }, out);
    }

    /// Stops the servers started while more than `screens` screens were
    /// open.
    pub fn stop_for_closed_screens(&mut self, screens: usize, out: &mut Vec<Command>) {
        while let Some(at) = self.running.iter().position(|r| r.screens > screens) {
            self.stop_at(at, out);
        }
    }

    /// Passes a plugin's answer to the shell; a file to send goes in
    /// parts (`Kernel::read_served`).
    pub fn respond(&mut self, response: ServerResponse, storage: &dyn Storage) {
        let response = response.into_parts();
        let mut status = response.status;
        let mut body = response.body;
        let mut file = None;
        if let Some(path) = response.body_from {
            match storage.size(&path) {
                Ok(size) => file = Some(ServedFile { path, size }),
                Err(_) => {
                    status = 404;
                    body = b"not found".to_vec();
                }
            }
        }
        // The request's body is done with, unless the plugin moved it.
        if let Some(at) = self.bodies.iter().position(|(r, _)| *r == response.request) {
            let (_, path) = self.bodies.remove(at);
            let _ = storage.remove(&path);
        }
        if response.request >= cable::FIRST_REQUEST {
            let ct = &response.content_type;
            // A full queue leaves it unanswered (`cable::OUT_MAX`).
            let _ = self
                .cable
                .answer(response.request, status, ct, body, file.is_some());
            return;
        }
        self.calls.push(ServerCall::Respond {
            request: response.request,
            status,
            content_type: response.content_type,
            headers: response.headers,
            body,
            file,
        });
    }

    /// Turns what the shell reports into the plugins' notices.
    pub fn event(&mut self, event: ServerEvent, storage: &dyn Storage, out: &mut Vec<Command>) {
        match event {
            ServerEvent::Started { server, address } => {
                if let Some(running) = self.running.iter().find(|r| r.number == server) {
                    let server = running.id.clone();
                    tell(
                        &running.events,
                        ServerNotice::Started { server, address },
                        out,
                    );
                }
            }
            ServerEvent::Failed { server, reason } => {
                if let Some(at) = self.running.iter().position(|r| r.number == server) {
                    let running = self.running.remove(at);
                    let server = running.id;
                    tell(
                        &running.events,
                        ServerNotice::Failed { server, reason },
                        out,
                    );
                }
            }
            ServerEvent::Request {
                server,
                request,
                method,
                target,
                body: false,
            } => {
                self.forget_incoming(request, storage);
                self.pass_request(server, request, method, target, None, storage, out);
            }
            ServerEvent::Request {
                server,
                request,
                method,
                target,
                body: true,
            } => {
                // The body is saved as it comes, then the plugin is told.
                self.forget_incoming(request, storage);
                let tmp = format!("{}/tmp", paperpaw_sdk::data::DIR);
                let path = format!("{tmp}/request-{request}");
                let _ = storage.create_dir(&tmp);
                if self.bodies.is_empty() {
                    // None is in use: those there are left from before.
                    remove_bodies(storage, &tmp);
                }
                // Made empty by an append, not a whole write: that takes a
                // FAT card several looks through the folder. One of that
                // name still there goes first.
                let _ = storage.remove(&path);
                if storage.append(&path, &[]).is_ok() {
                    self.bodies.push((request, path.clone()));
                    self.incoming.push(Incoming {
                        request,
                        server,
                        method,
                        target,
                        path,
                    });
                } else {
                    self.refuse(request, 507, "cannot save the body");
                }
            }
            ServerEvent::Body {
                request,
                data,
                last,
            } => {
                let Some(at) = self.incoming.iter().position(|i| i.request == request) else {
                    return;
                };
                if let Err(error) = storage.append(&self.incoming[at].path, &data) {
                    let incoming = self.incoming.remove(at);
                    self.forget_body(request, &incoming.path, storage);
                    // Answered at once, while the client still sends: the
                    // shell ends the upload with it.
                    match error {
                        paperpaw_platform::StorageError::Full => {
                            self.refuse(request, 507, "the storage is full");
                        }
                        _ => self.refuse(request, 500, "the card did not take the file"),
                    }
                } else if last {
                    let i = self.incoming.remove(at);
                    let body = Some(i.path);
                    self.pass_request(i.server, request, i.method, i.target, body, storage, out);
                }
            }
            ServerEvent::Closed { request } => {
                self.incoming.retain(|i| i.request != request);
                let body = self.bodies.iter().find(|(r, _)| *r == request);
                if let Some(path) = body.map(|(_, path)| path.clone()) {
                    self.forget_body(request, &path, storage);
                }
            }
        }
    }

    /// Tells the plugin of server `server` about a complete request.
    #[expect(clippy::too_many_arguments, reason = "a request's parts, passed on")]
    fn pass_request(
        &mut self,
        server: u32,
        request: u32,
        method: String,
        target: String,
        body: Option<String>,
        storage: &dyn Storage,
        out: &mut Vec<Command>,
    ) {
        let Some(running) = self.running.iter().find(|r| r.number == server) else {
            if let Some(path) = body {
                self.forget_body(request, &path, storage);
            }
            return;
        };
        let notice = ServerNotice::Request {
            server: running.id.clone(),
            request,
            method,
            target,
            body_file: body,
        };
        tell(&running.events, notice, out);
    }

    /// Forgets what came before of a request numbered `request`: the shell
    /// reported it again, or reuses its number for a new one, whose body
    /// must not go where the old one's did.
    fn forget_incoming(&mut self, request: u32, storage: &dyn Storage) {
        while let Some(at) = self.incoming.iter().position(|i| i.request == request) {
            let incoming = self.incoming.remove(at);
            self.forget_body(request, &incoming.path, storage);
        }
    }

    /// Removes the body of `request`, e.g. after the connection was lost.
    fn forget_body(&mut self, request: u32, path: &str, storage: &dyn Storage) {
        self.bodies.retain(|(r, _)| *r != request);
        let _ = storage.remove(path);
    }

    /// Answers a request without its plugin, e.g. when its body could not
    /// be saved.
    fn refuse(&mut self, request: u32, status: u16, reason: &str) {
        if request >= cable::FIRST_REQUEST {
            let ct = "text/plain";
            let _ = self
                .cable
                .answer(request, status, ct, reason.as_bytes().to_vec(), false);
            return;
        }
        self.calls.push(ServerCall::Respond {
            request,
            status,
            content_type: String::from("text/plain"),
            headers: Vec::new(),
            body: reason.as_bytes().to_vec(),
            file: None,
        });
    }

    /// What the shell is to do.
    pub fn take(&mut self) -> Vec<ServerCall> {
        core::mem::take(&mut self.calls)
    }

    /// Whether calls wait for the shell (for a network).
    pub fn waiting(&self) -> bool {
        !self.calls.is_empty()
    }

    /// Whether a server runs on the network.
    pub fn busy(&self) -> bool {
        self.running
            .iter()
            .any(|r| Some(r.number) != self.cable_server)
    }

    /// The calls waited too long for a network: every server fails, for
    /// `reason`.
    pub fn fail_waiting(&mut self, reason: &str, out: &mut Vec<Command>) {
        if self.take().is_empty() {
            return;
        }
        let cable = self.cable_server;
        let (kept, failed) = core::mem::take(&mut self.running)
            .into_iter()
            .partition(|r| Some(r.number) == cable);
        self.running = kept;
        for running in failed {
            let server = running.id;
            let reason = String::from(reason);
            tell(
                &running.events,
                ServerNotice::Failed { server, reason },
                out,
            );
        }
    }
}

/// Tells a server's plugin `notice` on its `events`.
fn tell(events: &Id, notice: ServerNotice, out: &mut Vec<Command>) {
    out.push(Command::Emit(events.clone(), notice.to_params()));
}

/// Removes the requests' bodies in `tmp`.
fn remove_bodies(storage: &dyn Storage, tmp: &str) {
    let mut names = Vec::new();
    let _ = storage.list(tmp, &mut |entry| {
        if !entry.is_dir && entry.name.starts_with("request-") {
            names.push(format!("{tmp}/{}", entry.name));
        }
    });
    for path in names {
        let _ = storage.remove(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    const SERVER: Id = Id::new_static("test.server");
    const EVENTS: Id = Id::new_static("test.server-events");

    fn body(request: u32) -> ServerEvent {
        ServerEvent::Request {
            server: 1,
            request,
            method: String::from("PUT"),
            target: String::from("/up"),
            body: true,
        }
    }

    #[test]
    fn bodies_left_behind_go() {
        let tmp = format!("{}/tmp", paperpaw_sdk::data::DIR);
        let left = format!("{tmp}/request-3");
        let storage = MemoryStorage::new()
            .with(&left, alloc::vec![0; 1000])
            .with(&format!("{tmp}/other"), "kept");
        let mut servers = Servers::default();
        let mut out = Vec::new();
        servers.serve(SERVER, 8080, EVENTS, true, 1, &mut out);
        servers.event(body(1), &storage, &mut out);
        assert!(!storage.exists(&left));
        assert!(storage.exists(&format!("{tmp}/request-1")));
        assert!(storage.exists(&format!("{tmp}/other")));
        // A body still in use stays while another comes.
        servers.event(body(2), &storage, &mut out);
        assert!(storage.exists(&format!("{tmp}/request-1")));
    }

    /// A request whose number comes again starts with an empty body, not
    /// after the one of before.
    #[test]
    fn a_body_starts_empty() {
        let storage = MemoryStorage::new();
        let mut servers = Servers::default();
        let mut out = Vec::new();
        servers.serve(SERVER, 8080, EVENTS, true, 1, &mut out);
        servers.event(body(2), &storage, &mut out);
        for _ in 0..2 {
            servers.event(body(1), &storage, &mut out);
            let data = alloc::vec![7; 10];
            let last = true;
            servers.event(
                ServerEvent::Body {
                    request: 1,
                    data,
                    last,
                },
                &storage,
                &mut out,
            );
        }
        let path = format!("{}/tmp/request-1", paperpaw_sdk::data::DIR);
        assert_eq!(storage.get(&path), Some(alloc::vec![7; 10]));
    }
}
