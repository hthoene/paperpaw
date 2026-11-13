//! The servers of `core.net` (`serve`) on the computer: a plugin that serves a
//! page (the transfer page, the phone page of a flash card) listens on a real
//! TCP port, as on the X4, behind the same contract (`ShellHttp::listen` and
//! the rest, reports to core from the device's own loop, never from inside a
//! call).
//!
//! The sockets and their threads are `paperpaw_serve::Servers`', the engine
//! the interface-6 desktop shell runs; this turns its events into core's
//! reports. A request's head is told to core at once (`served_head`: core
//! decides, and gives the request its number); a body is appended to the file
//! core named as it comes (`served_body`, `served_whole`); the answer core
//! asks for goes back to the connection (`reply`). A request core finds busy
//! is answered 503 (the engine keeps no connection waiting). Like the other
//! servers of the emulator, these listen on the loopback address only.
//!
//! Nothing here borrows its tables while core is told something: core may call
//! back (`reply`, `close_served`, `unlisten`) from inside a report.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use paperpaw_core::native_host::shell_wiring::{
    NetReports, ServedDecision, ServedReply, ServerReason,
};
use paperpaw_core::{ServedFile, ServerCall, ServerEvent};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_platform::Storage;
use paperpaw_serve::{Head, Servers};

/// What core is told, by [`Serving::pump`]: `NetReports` where it runs, a
/// recorder in the tests.
pub(crate) trait ServeReports {
    fn started(&self, server: u32, address: &str) -> Result<(), i32>;
    fn head(
        &self,
        server: u32,
        method: &str,
        target: &str,
        length: Option<u64>,
        head_len: u32,
    ) -> Result<ServedDecision, i32>;
    fn body(&self, request: u32, received: u64, added: u64) -> Result<u32, i32>;
    fn whole(&self, request: u32, total: u64) -> Result<(), i32>;
    fn gone(&self, request: u32) -> Result<(), i32>;
}

impl ServeReports for NetReports {
    fn started(&self, server: u32, address: &str) -> Result<(), i32> {
        self.server_started(server, address)
    }

    fn head(
        &self,
        server: u32,
        method: &str,
        target: &str,
        length: Option<u64>,
        head_len: u32,
    ) -> Result<ServedDecision, i32> {
        self.served_head(server, method, target, length, false, head_len)
    }

    fn body(&self, request: u32, received: u64, added: u64) -> Result<u32, i32> {
        self.served_body(request, received, added)
    }

    fn whole(&self, request: u32, total: u64) -> Result<(), i32> {
        self.served_whole(request, total)
    }

    fn gone(&self, request: u32) -> Result<(), i32> {
        self.served_gone(request)
    }
}

/// A report that core did not take (`ERR_BUSY`), to be made again.
enum Work {
    Started {
        server: u32,
        address: String,
    },
    Head {
        engine: u32,
        server: u32,
        method: String,
        target: String,
        head: Option<Head>,
        has_body: bool,
    },
    Body {
        engine: u32,
        core: u32,
        received: u64,
        added: u64,
    },
    Whole {
        engine: u32,
        core: u32,
        total: u64,
    },
    Gone {
        core: u32,
    },
}

/// A request core accepted.
struct Open {
    core: u32,
    server: u32,
    path: String,
    received: u64,
}

#[derive(Default)]
struct Books {
    /// Reports to make, oldest first.
    work: VecDeque<Work>,
    /// Engine events read while waiting for a listener's, not turned into
    /// reports yet.
    events: VecDeque<ServerEvent>,
    /// By the engine's number of the request.
    open: HashMap<u32, Open>,
    /// Core's number of an open request to the engine's.
    engine_of: HashMap<u32, u32>,
}

/// The servers of `core.net` on the computer (see the module).
pub(crate) struct Serving {
    servers: RefCell<Servers>,
    storage: Rc<dyn Storage>,
    books: RefCell<Books>,
}

impl std::fmt::Debug for Serving {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Serving")
            .field("open", &self.books.borrow().open.len())
            .finish_non_exhaustive()
    }
}

impl Serving {
    /// Servers on the loopback address, over `storage` (the card: uploads are
    /// written to it, files sent are read from it).
    pub(crate) fn new(storage: Rc<dyn Storage>) -> Self {
        let servers = Servers::new(|| {})
            .listening_on(std::net::Ipv4Addr::LOCALHOST.into())
            .recording_heads();
        Self {
            servers: RefCell::new(servers),
            storage,
            books: RefCell::new(Books::default()),
        }
    }

    /// Whether a server runs or a file is being sent (the loop looks again
    /// soon).
    pub(crate) fn any(&self) -> bool {
        self.servers.borrow().any() || !self.books.borrow().work.is_empty()
    }

    /// `ShellHttp::listen`. The sockets are opened here (the engine binds at
    /// once); core is told `started` at the next pump.
    pub(crate) fn listen(
        &self,
        server: u32,
        port: u32,
        address: &mut [u8],
    ) -> Result<usize, ServerReason> {
        // The cable server is not offered; a port is 1 to 65535.
        let port = u16::try_from(port)
            .ok()
            .filter(|port| *port != 0)
            .ok_or(ServerReason::Refused)?;
        // The engine's queue has room for what it reports now.
        self.read_events();
        self.servers
            .borrow_mut()
            .apply(ServerCall::Listen { server, port });
        let opened = loop {
            let Some(event) = self.servers.borrow().next() else {
                return Err(ServerReason::Refused);
            };
            match event {
                ServerEvent::Started { server: s, address } if s == server => break address,
                ServerEvent::Failed { server: s, .. } if s == server => {
                    return Err(ServerReason::Refused);
                }
                other => self.books.borrow_mut().events.push_back(other),
            }
        };
        let Some(room) = address.get_mut(..opened.len()) else {
            self.unlisten(server);
            return Err(ServerReason::Refused);
        };
        room.copy_from_slice(opened.as_bytes());
        self.books.borrow_mut().work.push_back(Work::Started {
            server,
            address: opened.clone(),
        });
        Ok(opened.len())
    }

    /// `ShellHttp::unlisten`: the server and its connections end.
    pub(crate) fn unlisten(&self, server: u32) {
        self.servers.borrow_mut().apply(ServerCall::Stop { server });
        let mut books = self.books.borrow_mut();
        books.work.retain(|work| match work {
            Work::Started { server: s, .. } | Work::Head { server: s, .. } => *s != server,
            _ => true,
        });
        let gone: Vec<(u32, u32)> = books
            .open
            .iter()
            .filter(|(_, open)| open.server == server)
            .map(|(engine, open)| (*engine, open.core))
            .collect();
        for (engine, core) in gone {
            books.open.remove(&engine);
            books.engine_of.remove(&core);
        }
    }

    /// `ShellHttp::reply`: the answer of request `core` goes to its connection.
    pub(crate) fn reply(&self, core: u32, reply: &ServedReply<'_>) -> Result<(), i32> {
        let file = match reply.send_path {
            Some(path) => {
                let size = match reply.file_len {
                    Some(size) => size,
                    None => self.storage.size(path).map_err(|_| ERR_NOT_FOUND)?,
                };
                Some(ServedFile {
                    path: path.to_owned(),
                    size,
                })
            }
            None => None,
        };
        let headers = reply
            .headers
            .clone()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        self.answer(
            core,
            u16::try_from(reply.status).map_err(|_| ERR_UNSUPPORTED)?,
            reply.content_type,
            headers,
            reply.body.map(<[u8]>::to_vec).unwrap_or_default(),
            file,
        )
    }

    /// `ShellHttp::close_served`: the request ends with `status` of the host's
    /// own (the engine has no way to end one without an answer: 0 is 503).
    pub(crate) fn close_served(&self, core: u32, status: u32) {
        let status = u16::try_from(status)
            .ok()
            .filter(|s| *s != 0)
            .unwrap_or(503);
        let _ = self.answer(core, status, "text/plain", Vec::new(), Vec::new(), None);
    }

    fn answer(
        &self,
        core: u32,
        status: u16,
        content_type: &str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        file: Option<ServedFile>,
    ) -> Result<(), i32> {
        let engine = {
            let mut books = self.books.borrow_mut();
            let engine = books.engine_of.remove(&core).ok_or(ERR_NOT_FOUND)?;
            books.open.remove(&engine);
            engine
        };
        self.servers.borrow_mut().apply(ServerCall::Respond {
            request: engine,
            status,
            content_type: content_type.to_owned(),
            headers,
            body,
            file,
        });
        Ok(())
    }

    /// Answers the connection of `engine` with `status` (a refusal, no
    /// request of core's).
    fn refuse(&self, engine: u32, status: u16) {
        self.servers.borrow_mut().apply(ServerCall::Respond {
            request: engine,
            status,
            content_type: String::from("text/plain"),
            headers: Vec::new(),
            body: Vec::new(),
            file: None,
        });
    }

    fn read_events(&self) {
        while let Some(event) = self.servers.borrow().next() {
            self.books.borrow_mut().events.push_back(event);
        }
    }

    /// Tells core what happened and reads the files being sent.
    pub(crate) fn pump(&self, reports: &dyn ServeReports) {
        self.read_events();
        loop {
            let next = {
                let mut books = self.books.borrow_mut();
                books.work.pop_front()
            };
            let Some(work) = next.or_else(|| self.turn_next_event()) else {
                break;
            };
            if let Err(work) = self.report(work, reports) {
                // Core was busy: this one again at the next pump, in order.
                self.books.borrow_mut().work.push_front(work);
                break;
            }
        }
        let storage = &self.storage;
        self.servers
            .borrow_mut()
            .pump(|path, at, buffer| storage.read_at(path, at, buffer).ok());
    }

    /// The report the oldest engine event comes to, its effects on the card
    /// done.
    fn turn_next_event(&self) -> Option<Work> {
        loop {
            let event = self.books.borrow_mut().events.pop_front()?;
            match event {
                ServerEvent::Started { server, address } => {
                    return Some(Work::Started { server, address });
                }
                // Told by `listen`: the port could not be had.
                ServerEvent::Failed { .. } => {}
                ServerEvent::Request {
                    server,
                    request,
                    method,
                    target,
                    body,
                } => {
                    let head = self.servers.borrow().take_head(request);
                    return Some(Work::Head {
                        engine: request,
                        server,
                        method,
                        target,
                        head,
                        has_body: body,
                    });
                }
                ServerEvent::Body {
                    request,
                    data,
                    last,
                } => {
                    if let Some(work) = self.body_part(request, &data, last) {
                        return Some(work);
                    }
                }
                ServerEvent::Closed { request } => {
                    let mut books = self.books.borrow_mut();
                    if let Some(open) = books.open.remove(&request) {
                        books.engine_of.remove(&open.core);
                        return Some(Work::Gone { core: open.core });
                    }
                }
            }
        }
    }

    /// A part of an upload appended to the file core named.
    fn body_part(&self, engine: u32, data: &[u8], last: bool) -> Option<Work> {
        let (core, path, received) = {
            let mut books = self.books.borrow_mut();
            let open = books.open.get_mut(&engine)?;
            open.received += data.len() as u64;
            (open.core, open.path.clone(), open.received)
        };
        if self.storage.append(&path, data).is_err() {
            // No room, or no card: the upload ends 507, and core forgets it.
            self.refuse(engine, 507);
            let mut books = self.books.borrow_mut();
            books.open.remove(&engine);
            books.engine_of.remove(&core);
            return Some(Work::Gone { core });
        }
        let added = data.len() as u64;
        if last {
            // The report of the part comes first (the work is in order).
            self.books.borrow_mut().work.push_back(Work::Whole {
                engine,
                core,
                total: received,
            });
        }
        Some(Work::Body {
            engine,
            core,
            received,
            added,
        })
    }

    /// Tells core one report; `Err` with it where core was busy.
    fn report(&self, work: Work, reports: &dyn ServeReports) -> Result<(), Work> {
        match work {
            Work::Started { server, address } => match reports.started(server, &address) {
                Err(ERR_BUSY) => Err(Work::Started { server, address }),
                // The plugin stopped the server meanwhile: the listener ends.
                Err(_) => {
                    self.unlisten(server);
                    Ok(())
                }
                Ok(()) => Ok(()),
            },
            Work::Head {
                engine,
                server,
                method,
                target,
                head,
                has_body,
            } => {
                let length = head.map(|head| head.length).filter(|length| *length > 0);
                let head_len = head.map_or(0, |head| head.head_len);
                match reports.head(server, &method, &target, length, head_len) {
                    Err(ERR_BUSY) => Err(Work::Head {
                        engine,
                        server,
                        method,
                        target,
                        head,
                        has_body,
                    }),
                    Ok(ServedDecision::Accept { request, path }) => {
                        self.accepted(engine, server, request, path, has_body);
                        Ok(())
                    }
                    Ok(ServedDecision::Reject(status)) => {
                        self.refuse(engine, u16::try_from(status).unwrap_or(503));
                        Ok(())
                    }
                    // Busy (the engine keeps no connection waiting), or a
                    // server that stopped meanwhile.
                    Ok(_) | Err(_) => {
                        self.refuse(engine, 503);
                        Ok(())
                    }
                }
            }
            Work::Body {
                engine,
                core,
                received,
                added,
            } => match reports.body(core, received, added) {
                Err(ERR_BUSY) => Err(Work::Body {
                    engine,
                    core,
                    received,
                    added,
                }),
                Ok(0) => Ok(()),
                // Past the limit or no room: core removed the part and the
                // request.
                Ok(status) => {
                    self.ended(engine, core, u16::try_from(status).unwrap_or(503));
                    Ok(())
                }
                Err(_) => {
                    self.ended(engine, core, 503);
                    Ok(())
                }
            },
            Work::Whole {
                engine,
                core,
                total,
            } => match reports.whole(core, total) {
                Err(ERR_BUSY) => Err(Work::Whole {
                    engine,
                    core,
                    total,
                }),
                Ok(()) => Ok(()),
                Err(_) => {
                    self.ended(engine, core, 503);
                    Ok(())
                }
            },
            Work::Gone { core } => match reports.gone(core) {
                Err(ERR_BUSY) => Err(Work::Gone { core }),
                _ => Ok(()),
            },
        }
    }

    /// Core accepted the request: its upload goes to `path`.
    fn accepted(&self, engine: u32, server: u32, core: u32, path: String, has_body: bool) {
        {
            let mut books = self.books.borrow_mut();
            books.engine_of.insert(core, engine);
            books.open.insert(
                engine,
                Open {
                    core,
                    server,
                    path,
                    received: 0,
                },
            );
        }
        if !has_body {
            // Whole at once: the plugin is told of the request.
            self.books.borrow_mut().work.push_front(Work::Whole {
                engine,
                core,
                total: 0,
            });
        }
    }

    /// Core ended the request `core` with `status`: the connection is answered.
    fn ended(&self, engine: u32, core: u32, status: u16) {
        {
            let mut books = self.books.borrow_mut();
            books.open.remove(&engine);
            books.engine_of.remove(&core);
        }
        self.refuse(engine, status);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use std::cell::{Cell, RefCell};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    /// Core, as far as serving goes: what it was told, and what it decides.
    struct Core {
        told: RefCell<Vec<String>>,
        /// `Some(status)`: reject the heads; else accept them as request 7.
        reject: Cell<Option<u32>>,
        busy_once: Cell<bool>,
    }

    impl Core {
        fn new() -> Self {
            Self {
                told: RefCell::new(Vec::new()),
                reject: Cell::new(None),
                busy_once: Cell::new(false),
            }
        }

        fn told(&self) -> Vec<String> {
            self.told.borrow().clone()
        }
    }

    impl ServeReports for Core {
        fn started(&self, server: u32, address: &str) -> Result<(), i32> {
            let _ = address;
            self.told.borrow_mut().push(format!("started {server}"));
            Ok(())
        }

        fn head(
            &self,
            server: u32,
            method: &str,
            target: &str,
            length: Option<u64>,
            head_len: u32,
        ) -> Result<ServedDecision, i32> {
            if self.busy_once.replace(false) {
                return Err(ERR_BUSY);
            }
            self.told.borrow_mut().push(format!(
                "head {server} {method} {target} {length:?} {}",
                head_len > 0
            ));
            Ok(match self.reject.get() {
                Some(status) => ServedDecision::Reject(status),
                None => ServedDecision::Accept {
                    request: 7,
                    path: String::from("received/7"),
                },
            })
        }

        fn body(&self, request: u32, received: u64, added: u64) -> Result<u32, i32> {
            self.told
                .borrow_mut()
                .push(format!("body {request} {received} {added}"));
            Ok(0)
        }

        fn whole(&self, request: u32, total: u64) -> Result<(), i32> {
            self.told
                .borrow_mut()
                .push(format!("whole {request} {total}"));
            Ok(())
        }

        fn gone(&self, request: u32) -> Result<(), i32> {
            self.told.borrow_mut().push(format!("gone {request}"));
            Ok(())
        }
    }

    fn serving() -> (Serving, Rc<MemoryStorage>) {
        let storage = Rc::new(MemoryStorage::new());
        (Serving::new(storage.clone()), storage)
    }

    /// Opens a server on `port` and returns the port it got.
    fn open(serving: &Serving, core: &Core, port: u32) -> u16 {
        let mut address = [0u8; 64];
        let used = serving.listen(1, port, &mut address).unwrap();
        let text = std::str::from_utf8(&address[..used]).unwrap();
        let port = text.rsplit_once(':').unwrap().1.parse().unwrap();
        serving.pump(core);
        assert_eq!(core.told(), ["started 1"]);
        port
    }

    /// A client on a thread of its own: connects, sends `request`, reads the
    /// answer to its end.
    fn client(port: u16, request: Vec<u8>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            stream.write_all(&request).unwrap();
            let mut answer = Vec::new();
            let _ = stream.read_to_end(&mut answer);
            answer
        })
    }

    /// Pumps until `done` holds or five seconds pass.
    fn until(serving: &Serving, core: &Core, done: impl Fn(&Core) -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !done(core) {
            assert!(Instant::now() < end, "timed out: {:?}", core.told());
            serving.pump(core);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Pumps while the client reads (a file is read as the connection takes
    /// it), then returns what it read.
    fn finish(serving: &Serving, core: &Core, client: std::thread::JoinHandle<Vec<u8>>) -> Vec<u8> {
        let end = Instant::now() + Duration::from_secs(10);
        while !client.is_finished() {
            assert!(Instant::now() < end, "client did not finish");
            serving.pump(core);
            std::thread::sleep(Duration::from_millis(5));
        }
        client.join().unwrap()
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn a_page_is_served_to_a_client() {
        let (serving, _) = serving();
        let core = Core::new();
        let port = open(&serving, &core, 18_931);
        let get = client(port, b"GET /phone?x=1 HTTP/1.1\r\nHost: a\r\n\r\n".to_vec());
        until(&serving, &core, |core| core.told().len() == 3);
        assert_eq!(
            core.told()[1..],
            ["head 1 GET /phone?x=1 None true", "whole 7 0"]
        );
        serving
            .answer(
                7,
                200,
                "text/html",
                vec![("X-Test".into(), "yes".into())],
                b"<p>hello</p>".to_vec(),
                None,
            )
            .unwrap();
        let answer = text(&finish(&serving, &core, get));
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(answer.contains("text/html") && answer.contains("X-Test: yes"));
        assert!(answer.ends_with("<p>hello</p>"), "{answer}");
        // The request is answered: core's number is not known any more.
        assert_eq!(
            serving.answer(7, 200, "text/plain", vec![], vec![], None),
            Err(ERR_NOT_FOUND)
        );
    }

    #[test]
    fn an_upload_is_written_to_the_file_core_named_and_told() {
        let (serving, storage) = serving();
        let core = Core::new();
        let port = open(&serving, &core, 18_951);
        let put = client(
            port,
            b"PUT /up HTTP/1.1\r\nHost: a\r\nContent-Length: 5\r\n\r\nhello".to_vec(),
        );
        until(&serving, &core, |core| {
            core.told().iter().any(|line| line.starts_with("whole"))
        });
        assert_eq!(
            core.told()[1..],
            ["head 1 PUT /up Some(5) true", "body 7 5 5", "whole 7 5"]
        );
        let mut file = [0u8; 8];
        let n = storage.read_at("received/7", 0, &mut file).unwrap();
        assert_eq!(&file[..n], b"hello");
        serving
            .answer(7, 201, "text/plain", vec![], b"ok".to_vec(), None)
            .unwrap();
        assert!(text(&finish(&serving, &core, put)).starts_with("HTTP/1.1 201"));
    }

    #[test]
    fn a_head_core_rejects_is_answered_with_its_status() {
        let (serving, _) = serving();
        let core = Core::new();
        core.reject.set(Some(413));
        let port = open(&serving, &core, 18_971);
        let get = client(port, b"GET / HTTP/1.1\r\nHost: a\r\n\r\n".to_vec());
        assert!(text(&finish(&serving, &core, get)).starts_with("HTTP/1.1 413"));
    }

    #[test]
    fn a_report_core_was_too_busy_for_is_made_again() {
        let (serving, _) = serving();
        let core = Core::new();
        let port = open(&serving, &core, 18_991);
        core.busy_once.set(true);
        let get = client(port, b"GET /late HTTP/1.1\r\nHost: a\r\n\r\n".to_vec());
        until(&serving, &core, |core| core.told().len() == 3);
        assert_eq!(core.told()[1], "head 1 GET /late None true");
        serving
            .answer(7, 200, "text/plain", vec![], b"later".to_vec(), None)
            .unwrap();
        assert!(text(&finish(&serving, &core, get)).ends_with("later"));
    }

    #[test]
    fn a_file_is_sent_from_the_card_in_parts() {
        let (serving, storage) = serving();
        let core = Core::new();
        let port = open(&serving, &core, 19_011);
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        storage.write("books/big.bin", &data).unwrap();
        let get = client(port, b"GET /big HTTP/1.1\r\nHost: a\r\n\r\n".to_vec());
        until(&serving, &core, |core| core.told().len() == 3);
        let file = ServedFile {
            path: String::from("books/big.bin"),
            size: data.len() as u64,
        };
        serving
            .answer(
                7,
                200,
                "application/octet-stream",
                vec![],
                Vec::new(),
                Some(file),
            )
            .unwrap();
        let answer = finish(&serving, &core, get);
        let at = answer.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        assert!(answer[at..] == data[..], "{} bytes", answer.len() - at);
    }

    #[test]
    fn the_cable_port_and_a_stopped_server_are_not_served() {
        let (serving, _) = serving();
        let core = Core::new();
        let mut address = [0u8; 64];
        assert!(matches!(
            serving.listen(1, 0, &mut address),
            Err(ServerReason::Refused)
        ));
        // An address that does not fit ends the listener it opened.
        let mut small = [0u8; 4];
        assert!(matches!(
            serving.listen(2, 19_031, &mut small),
            Err(ServerReason::Refused)
        ));
        let port = open_second(&serving, &core, 19_051);
        serving.unlisten(1);
        assert!(!serving.any());
        let _ = port;
    }

    fn open_second(serving: &Serving, core: &Core, port: u32) -> u16 {
        let mut address = [0u8; 64];
        let used = serving.listen(1, port, &mut address).unwrap();
        let text = std::str::from_utf8(&address[..used]).unwrap();
        let port = text.rsplit_once(':').unwrap().1.parse().unwrap();
        serving.pump(core);
        port
    }
}
