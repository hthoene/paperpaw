//! A connection to a mail server, as a state machine: bytes and socket
//! events go in (`step`), what to send and what to read next goes out
//! through [`Io`], and what happened comes back as [`Event`]s. The
//! plugin's screen holds the socket and lends it; the tests lend a fake
//! server. Nothing here waits.
//!
//! IMAP and SMTP are done one connection at a time (the X4 holds one TLS
//! session): connect, secure, log in, do the tasks, log out, close.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{CertificateHash, Distrust, KeyHash, SocketError, Storage, Tls};

use crate::codec;
use crate::header;
use crate::imap::{self, LineBuffer, Response, Status, Token, Untagged};
use crate::smtp::{self, Extensions, Reply, ReplyParser};
use crate::store::{self, Counts, Folder, Row};

/// How a socket is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    Implicit,
    /// Plain, then the server agrees to TLS (IMAP 143, SMTP 587).
    StartTls,
}

/// Where and as whom to connect.
#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub user: String,
    pub password: String,
    /// The key the reader trusted for this server (see [`Event::Untrusted`]).
    pub pin: Option<KeyHash>,
}

// The password never goes into a log or a test's message.
impl core::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Endpoint")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("user", &self.user)
            .field("password", &"…")
            .field("pin", &self.pin)
            .finish()
    }
}

impl Endpoint {
    fn tls(&self) -> Tls {
        match self.pin {
            Some(key) => Tls::pinned_keys(alloc::vec![key]),
            None => Tls::new(),
        }
    }
}

/// What the session asks of the socket (the plugin turns each into a
/// `Command`).
pub trait Io {
    fn connect_tls(&mut self, host: &str, port: u16, tls: Tls);
    fn connect_plain(&mut self, host: &str, port: u16);
    fn start_tls(&mut self, tls: Tls);
    fn send(&mut self, data: Vec<u8>);
    fn read_bytes(&mut self, max: u32);
    fn read_file(&mut self, path: &str, len: u64);
    fn close(&mut self);
}

/// What the socket tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A plain connection is made.
    Connected,
    Secured {
        certificate: CertificateHash,
        key: KeyHash,
        time_checked: bool,
    },
    Untrusted {
        certificate: CertificateHash,
        key: KeyHash,
        why: Distrust,
    },
    Bytes(Vec<u8>),
    /// A file read is complete.
    Saved(String),
    /// The other side closed (or the socket ended).
    Closed,
    Failed(SocketError, String),
}

/// Where a session is, for a line of words on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Connecting,
    LoggingIn,
    Folders,
    Headers,
    Message,
    Updating,
    Sending,
}

/// Why a session ended without finishing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The network: no Wi-Fi, no such server, refused, timed out, lost.
    Network(SocketError),
    /// The server's certificate or the secure connection (not trusted ones
    /// are [`Event::Untrusted`]).
    Secure(SocketError),
    /// The device has no TLS (a shell without it).
    NoTls,
    /// The device cannot say whether the certificate is valid now: its
    /// clock is not set. The password was not sent.
    ClockNotSet,
    /// Another secure connection holds the one there is.
    Busy,
    /// Not enough memory for the secure connection.
    NoMemory,
    /// The server refused the user name or password.
    Login(String),
    /// The server refused something else; its words.
    Refused(String),
    /// A recipient the server does not take (SMTP), and its words.
    Recipient(String, String),
    /// The server wants it later (SMTP 4xx).
    Temporary(String),
    /// The server's answer made no sense, or was too large.
    Protocol,
    /// The server does not offer a way to log in that is safe here, or no
    /// STARTTLS.
    Unsafe,
    /// The card could not be written.
    Storage,
}

/// What a session tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Stage(Stage),
    /// Everything was done.
    Done,
    Failed(Failure),
    /// The server's certificate is not trusted: its hashes, for the
    /// reader to compare and, if they agree, to keep as the pin.
    Untrusted {
        certificate: CertificateHash,
        key: KeyHash,
        why: Distrust,
    },
}

fn failure_of(error: SocketError) -> Failure {
    match error {
        SocketError::Unsupported => Failure::NoTls,
        SocketError::Busy => Failure::Busy,
        SocketError::NoMemory => Failure::NoMemory,
        SocketError::Tls | SocketError::NameMismatch | SocketError::Expired => {
            Failure::Secure(error)
        }
        other => Failure::Network(other),
    }
}

/// Bytes in, whole lines out, and whether a read is under way.
#[derive(Debug, Default)]
struct Wire {
    buf: LineBuffer,
    reading: bool,
    /// The socket is being secured (after STARTTLS): nothing is sent or
    /// read until the kernel says `Secured`; a read now makes it close the
    /// socket.
    switching: bool,
}

impl Wire {
    /// Asks for more bytes, unless some are asked for already, or the
    /// socket is being secured.
    fn more(&mut self, io: &mut dyn Io) {
        if !self.reading && !self.switching {
            self.reading = true;
            io.read_bytes(1024);
        }
    }
}

// ---------------------------------------------------------------- IMAP

/// What to do on the server, in this order, in one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    /// The list of folders, written to `out`.
    Folders { out: String },
    /// The latest `count` messages of `folder` (as the server names it):
    /// their list to `index`, the counts to `status`; `now` is the time to
    /// keep with them (0 where the clock is not known).
    Sync {
        folder: String,
        count: u32,
        index: String,
        status: String,
        now: i64,
    },
    /// The message `uid` of `folder` as the server has it, to `out`.
    Body {
        folder: String,
        uid: u32,
        out: String,
    },
    /// Sets (`on`) or clears a flag (`\Seen`, `\Flagged`) of a message.
    Flag {
        folder: String,
        uid: u32,
        flag: &'static str,
        on: bool,
    },
}

impl Task {
    fn folder(&self) -> Option<&str> {
        match self {
            Task::Folders { .. } => None,
            Task::Sync { folder, .. } | Task::Body { folder, .. } | Task::Flag { folder, .. } => {
                Some(folder)
            }
        }
    }
}

/// The command the session waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Greeting,
    StartTls,
    Capability,
    Auth,
    Login,
    List,
    Select,
    Status,
    FetchHeaders,
    FetchBody,
    Store,
    Logout,
}

#[derive(Debug)]
enum Literal {
    /// Into memory (a block of headers); `keep` is false where it is too
    /// large and only skipped.
    Memory {
        left: usize,
        data: Vec<u8>,
        keep: bool,
    },
    /// The rest of a message is read by the socket into a file of its own,
    /// then joined to the first part.
    File { scratch: String },
    /// A literal of the server's that is of no use.
    Skip { left: usize },
}

#[derive(Debug, Clone, Default)]
struct Fetching {
    uid: u32,
    flags: Vec<String>,
    size: u64,
    date: i64,
    headers: Vec<u8>,
}

/// The IMAP side of a mail session.
#[derive(Debug)]
pub struct Imap {
    endpoint: Endpoint,
    tasks: VecDeque<Task>,
    wire: Wire,
    tag: u32,
    command: Option<Command>,
    capabilities: Vec<String>,
    secure: bool,
    // What the commands bring.
    folders: Vec<Folder>,
    exists: u32,
    uid_validity: u64,
    counts: Option<(u32, u32)>,
    rows: Vec<Row>,
    /// A FETCH answer in the making.
    fetching: Option<Fetching>,
    literal: Option<Literal>,
    task: Option<Task>,
    finished: bool,
    scratch: u32,
}

impl Imap {
    pub fn new(endpoint: Endpoint, tasks: Vec<Task>) -> Self {
        Self {
            endpoint,
            tasks: tasks.into(),
            wire: Wire::default(),
            tag: 0,
            command: None,
            capabilities: Vec::new(),
            secure: false,
            folders: Vec::new(),
            exists: 0,
            uid_validity: 0,
            counts: None,
            rows: Vec::new(),
            fetching: None,
            literal: None,
            task: None,
            finished: false,
            scratch: 0,
        }
    }

    /// Opens the connection.
    pub fn start(&mut self, io: &mut dyn Io) -> Vec<Event> {
        let e = &self.endpoint;
        match e.security {
            Security::Implicit => io.connect_tls(&e.host, e.port, e.tls()),
            Security::StartTls => io.connect_plain(&e.host, e.port),
        }
        self.command = Some(Command::Greeting);
        alloc::vec![Event::Stage(Stage::Connecting)]
    }

    fn has(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case(capability))
    }

    /// Sends `text` as the next command, tagged.
    fn send(&mut self, io: &mut dyn Io, command: Command, text: &str) {
        self.tag += 1;
        self.command = Some(command);
        io.send(format!("m{} {text}\r\n", self.tag).into_bytes());
    }

    fn fail(&mut self, io: &mut dyn Io, failure: Failure) -> Vec<Event> {
        self.finished = true;
        io.close();
        alloc::vec![Event::Failed(failure)]
    }

    /// What happened on the socket.
    pub fn step(&mut self, input: Input, io: &mut dyn Io, storage: &dyn Storage) -> Vec<Event> {
        let mut events = Vec::new();
        if self.finished {
            return events;
        }
        match input {
            Input::Connected => self.wire.more(io),
            Input::Secured { time_checked, .. } => {
                if !time_checked {
                    return self.fail(io, Failure::ClockNotSet);
                }
                self.secure = true;
                self.wire.switching = false;
                if self.command == Some(Command::StartTls) {
                    // Over TLS now: what the server offers is asked again.
                    self.capabilities.clear();
                    self.send(io, Command::Capability, "CAPABILITY");
                }
                self.wire.more(io);
            }
            Input::Untrusted {
                certificate,
                key,
                why,
            } => {
                self.finished = true;
                io.close();
                return alloc::vec![Event::Untrusted {
                    certificate,
                    key,
                    why
                }];
            }
            Input::Bytes(bytes) => {
                self.wire.reading = false;
                if bytes.is_empty() {
                    return self.lost(io);
                }
                self.wire.buf.push(&bytes);
                self.run(io, storage, &mut events);
            }
            Input::Saved(path) => {
                let waiting =
                    matches!(&self.literal, Some(Literal::File { scratch }) if *scratch == path);
                if waiting {
                    self.literal = None;
                    if self.join(storage, &path).is_err() {
                        return self.fail(io, Failure::Storage);
                    }
                    self.run(io, storage, &mut events);
                }
            }
            Input::Closed => return self.lost(io),
            Input::Failed(error, _) => {
                self.finished = true;
                return alloc::vec![Event::Failed(failure_of(error))];
            }
        }
        events
    }

    /// Whether a message is being read into a file: the socket is busy and
    /// no notice comes until it is saved.
    pub fn receiving(&self) -> bool {
        matches!(self.literal, Some(Literal::File { .. }))
    }

    /// The socket ended: expected after LOGOUT, else a loss.
    fn lost(&mut self, io: &mut dyn Io) -> Vec<Event> {
        let after_logout = self.command == Some(Command::Logout);
        self.finished = true;
        io.close();
        alloc::vec![if after_logout {
            Event::Done
        } else {
            Event::Failed(Failure::Network(SocketError::Lost))
        }]
    }

    /// Works through what is in the buffer; asks for more when it is used
    /// up.
    fn run(&mut self, io: &mut dyn Io, storage: &dyn Storage, events: &mut Vec<Event>) {
        while !self.finished {
            if self.literal.is_some() {
                if !self.take_literal(io) {
                    return;
                }
                continue;
            }
            match self.wire.buf.line() {
                Err(_) => {
                    events.extend(self.fail(io, Failure::Protocol));
                    return;
                }
                Ok(None) => {
                    self.wire.more(io);
                    return;
                }
                Ok(Some(line)) => {
                    if !self.line(&line, io, storage, events) {
                        return;
                    }
                }
            }
        }
    }

    /// One line of the server. False where the read of a file took over
    /// (no more lines until it is in).
    fn line(
        &mut self,
        line: &[u8],
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> bool {
        if self.fetching.is_some() && self.literal.is_none() {
            // What follows a literal: maybe more items, then `)`.
            self.rest_of_fetch(line);
            return true;
        }
        let Some(response) = imap::response(line) else {
            return true;
        };
        if self.command == Some(Command::Greeting) {
            return self.greeting(response, io, events);
        }
        match response {
            Response::Continue(_) => {
                if self.command == Some(Command::Auth) {
                    let data = auth_plain_data(&self.endpoint);
                    io.send(format!("{}\r\n", codec::base64_line(&data)).into_bytes());
                }
                true
            }
            Response::Untagged(u) => self.untagged(u, line, io, storage, events),
            Response::Tagged { tag, status, text } => {
                if tag != format!("m{}", self.tag) {
                    return true;
                }
                self.completed(status == Status::Ok, text, io, storage, events);
                true
            }
        }
    }

    /// The server's first words: on to TLS where it is not yet, else to
    /// what the server offers.
    fn greeting(&mut self, response: Response, io: &mut dyn Io, events: &mut Vec<Event>) -> bool {
        match response {
            Response::Untagged(Untagged::Bye(_)) | Response::Tagged { .. } => {
                events.extend(self.fail(io, Failure::Network(SocketError::Lost)));
                false
            }
            Response::Untagged(u) => {
                if let Untagged::Code { code, args } = u
                    && code == "CAPABILITY"
                {
                    self.capabilities = args;
                }
                if self.secure {
                    self.send(io, Command::Capability, "CAPABILITY");
                } else {
                    self.send(io, Command::StartTls, "STARTTLS");
                }
                true
            }
            Response::Continue(_) => true,
        }
    }

    fn untagged(
        &mut self,
        u: Untagged,
        line: &[u8],
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> bool {
        match u {
            Untagged::Capability(list) => self.capabilities = list,
            Untagged::Code { code, args } => match code.as_str() {
                "CAPABILITY" => self.capabilities = args,
                "UIDVALIDITY" => {
                    self.uid_validity = args.first().and_then(|a| a.parse().ok()).unwrap_or(0)
                }
                _ => {}
            },
            Untagged::Exists(n) => self.exists = n,
            Untagged::List { flags, name, .. } => {
                // A name no server has is skipped.
                if self.folders.len() < store::MAX_FOLDERS
                    && !name.is_empty()
                    && name.len() <= store::MAX_FOLDER_NAME
                {
                    self.folders.push(Folder {
                        name: imap::mailbox_display(&name),
                        raw: name,
                        flags: flags
                            .iter()
                            .take(16)
                            .map(|f| header::capped(&f.to_ascii_lowercase(), 32))
                            .collect(),
                    });
                }
            }
            Untagged::Status { items, .. } => {
                let get = |key: &str| items.iter().find(|(k, _)| k == key).map(|(_, v)| *v as u32);
                self.counts = Some((get("MESSAGES").unwrap_or(0), get("UNSEEN").unwrap_or(0)));
            }
            Untagged::Bye(_) => {
                if self.command != Some(Command::Logout) {
                    events.extend(self.fail(io, Failure::Network(SocketError::Lost)));
                    return false;
                }
            }
            Untagged::Fetch { items, .. } => return self.fetch(&items, line, io, storage, events),
            Untagged::Search(_) | Untagged::Other => {}
        }
        true
    }

    /// A FETCH answer: its numbers, and the literal that may follow.
    fn fetch(
        &mut self,
        items: &[Token],
        line: &[u8],
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> bool {
        self.fetching = Some(Fetching {
            uid: imap::fetch_number(items, "UID").unwrap_or(0) as u32,
            flags: imap::fetch_flags(items),
            size: imap::fetch_number(items, "RFC822.SIZE").unwrap_or(0),
            date: imap::fetch_text(items, "INTERNALDATE")
                .and_then(imap_date)
                .unwrap_or(0),
            headers: Vec::new(),
        });
        let Some(len) = imap::literal_at_end(line) else {
            // No literal (flags changed, say): the answer is complete.
            self.finish_fetch();
            return true;
        };
        match self.command {
            Some(Command::FetchHeaders) => {
                self.literal = Some(Literal::Memory {
                    left: len,
                    data: Vec::new(),
                    keep: len <= 16 * 1024,
                });
            }
            Some(Command::FetchBody) => return self.body_literal(len, io, storage, events),
            _ => self.literal = Some(Literal::Skip { left: len }),
        }
        true
    }

    /// A message's bytes (`len`): those that came with the line start its
    /// file, the socket reads the rest into a file of its own.
    fn body_literal(
        &mut self,
        len: usize,
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> bool {
        let out = match &self.task {
            Some(Task::Body { out, .. }) if len <= store::MAX_FETCH + 1024 => out.clone(),
            _ => {
                events.extend(self.fail(io, Failure::Protocol));
                return false;
            }
        };
        let carry = self.wire.buf.take(len);
        if storage_prepare(storage, &out).is_err() || storage.write(&out, &carry).is_err() {
            events.extend(self.fail(io, Failure::Storage));
            return false;
        }
        let left = len - carry.len();
        if left == 0 {
            return true;
        }
        self.scratch += 1;
        let scratch = format!("{out}.{}", self.scratch);
        io.read_file(&scratch, left as u64);
        self.literal = Some(Literal::File { scratch });
        false
    }

    /// Takes what a literal still needs from the buffer. False while it
    /// waits for more bytes or for a file.
    fn take_literal(&mut self, io: &mut dyn Io) -> bool {
        match &mut self.literal {
            Some(Literal::Memory { left, data, keep }) => {
                let got = self.wire.buf.take(*left);
                *left -= got.len();
                if *keep {
                    data.extend_from_slice(&got);
                }
                if *left > 0 {
                    self.wire.more(io);
                    return false;
                }
                let data = core::mem::take(data);
                let keep = *keep;
                if let (true, Some(f)) = (keep, &mut self.fetching) {
                    f.headers = data;
                }
            }
            Some(Literal::Skip { left }) => {
                let got = self.wire.buf.take(*left);
                *left -= got.len();
                if *left > 0 {
                    self.wire.more(io);
                    return false;
                }
            }
            Some(Literal::File { .. }) => return false,
            None => return true,
        }
        self.literal = None;
        true
    }

    /// The line after a literal: more items of the answer, then `)`.
    fn rest_of_fetch(&mut self, line: &[u8]) {
        let items = imap::tokens(line);
        // The items sit in the parentheses of the answer: this line
        // closes them, or a `(` was not closed.
        if let Some(f) = &mut self.fetching {
            if let Some(uid) = imap::fetch_number(&items, "UID") {
                f.uid = uid as u32;
            }
            if let Some(size) = imap::fetch_number(&items, "RFC822.SIZE") {
                f.size = size;
            }
            let flags = imap::fetch_flags(&items);
            if !flags.is_empty() {
                f.flags = flags;
            }
        }
        self.finish_fetch();
    }

    fn finish_fetch(&mut self) {
        let Some(f) = self.fetching.take() else {
            return;
        };
        if self.command == Some(Command::FetchHeaders)
            && f.uid != 0
            && self.rows.len() < store::MAX_MESSAGES as usize
        {
            self.rows.push(Row::from_headers(
                f.uid, &f.flags, f.date, f.size, &f.headers,
            ));
        }
    }

    /// The scratch file of a literal's rest joined to the message's file.
    fn join(&mut self, storage: &dyn Storage, scratch: &str) -> Result<(), ()> {
        let Some(Task::Body { out, .. }) = &self.task else {
            return Err(());
        };
        let mut offset = 0u64;
        let mut chunk = [0u8; 1024];
        loop {
            let n = storage
                .read_at(scratch, offset, &mut chunk)
                .map_err(|_| ())?;
            if n == 0 {
                break;
            }
            storage.append(out, &chunk[..n]).map_err(|_| ())?;
            offset += n as u64;
        }
        let _ = storage.remove(scratch);
        Ok(())
    }

    /// The command the session waited for is done.
    fn completed(
        &mut self,
        ok: bool,
        text: String,
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) {
        let Some(command) = self.command else {
            return;
        };
        match command {
            Command::Greeting => {}
            Command::StartTls => {
                if !ok {
                    events.extend(self.fail(io, Failure::Unsafe));
                    return;
                }
                // The switch: nothing is sent or read until it is done.
                self.wire.buf = LineBuffer::default();
                self.wire.switching = true;
                io.start_tls(self.endpoint.tls());
            }
            Command::Capability => {
                if !ok {
                    events.extend(self.fail(io, Failure::Protocol));
                    return;
                }
                events.push(Event::Stage(Stage::LoggingIn));
                self.login(io, events);
            }
            Command::Auth | Command::Login => {
                if !ok {
                    events.extend(self.fail(io, Failure::Login(text)));
                    return;
                }
                self.next_task(io, events);
            }
            Command::List => {
                if !ok {
                    events.extend(self.fail(io, Failure::Refused(text)));
                    return;
                }
                if let Some(Task::Folders { out }) = self.task.clone() {
                    let mut folders = core::mem::take(&mut self.folders);
                    store::sort_folders(&mut folders);
                    if store::write_folders(storage, &out, &folders).is_err() {
                        events.extend(self.fail(io, Failure::Storage));
                        return;
                    }
                }
                self.next_task(io, events);
            }
            Command::Select => {
                if !ok {
                    events.extend(self.fail(io, Failure::Refused(text)));
                    return;
                }
                self.after_select(io, storage, events);
            }
            // The counts are not needed so much that a refusal ends it.
            Command::Status => self.fetch_headers(io),
            Command::FetchHeaders => {
                if !ok {
                    events.extend(self.fail(io, Failure::Refused(text)));
                    return;
                }
                self.finish_sync(io, storage, events);
            }
            Command::FetchBody | Command::Store => {
                if !ok {
                    events.extend(self.fail(io, Failure::Refused(text)));
                    return;
                }
                self.next_task(io, events);
            }
            Command::Logout => {
                self.finished = true;
                io.close();
                events.push(Event::Done);
            }
        }
    }

    /// Logs in the safest way the server offers.
    fn login(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) {
        if !self.secure {
            // Never a password in the clear.
            events.extend(self.fail(io, Failure::Unsafe));
        } else if self.has("AUTH=PLAIN") {
            if self.has("SASL-IR") {
                let text = format!(
                    "AUTHENTICATE PLAIN {}",
                    codec::base64_line(&auth_plain_data(&self.endpoint))
                );
                self.send(io, Command::Auth, &text);
            } else {
                self.send(io, Command::Auth, "AUTHENTICATE PLAIN");
            }
        } else if !self.has("LOGINDISABLED") {
            let text = format!(
                "LOGIN {} {}",
                imap::quoted(&self.endpoint.user),
                imap::quoted(&self.endpoint.password)
            );
            self.send(io, Command::Login, &text);
        } else {
            events.extend(self.fail(io, Failure::Unsafe));
        }
    }

    /// Starts the next task, or logs out.
    fn next_task(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) {
        self.task = self.tasks.pop_front();
        self.exists = 0;
        self.rows.clear();
        self.counts = None;
        self.uid_validity = 0;
        self.folders.clear();
        let Some(task) = self.task.clone() else {
            self.send(io, Command::Logout, "LOGOUT");
            return;
        };
        events.push(Event::Stage(match &task {
            Task::Folders { .. } => Stage::Folders,
            Task::Sync { .. } => Stage::Headers,
            Task::Body { .. } => Stage::Message,
            Task::Flag { .. } => Stage::Updating,
        }));
        match task.folder() {
            None => self.send(io, Command::List, "LIST \"\" \"*\""),
            Some(folder) => {
                let text = format!("SELECT {}", imap::quoted(folder));
                self.send(io, Command::Select, &text);
            }
        }
    }

    fn after_select(&mut self, io: &mut dyn Io, storage: &dyn Storage, events: &mut Vec<Event>) {
        match self.task.clone() {
            Some(Task::Sync { folder, .. }) => {
                if self.exists == 0 {
                    // Nothing to fetch: the list is empty.
                    self.counts = Some((0, 0));
                    self.finish_sync(io, storage, events);
                } else {
                    self.send(
                        io,
                        Command::Status,
                        &format!("STATUS {} (MESSAGES UNSEEN)", imap::quoted(&folder)),
                    );
                }
            }
            Some(Task::Body { uid, .. }) => {
                let text = format!("UID FETCH {uid} BODY.PEEK[]<0.{}>", store::MAX_FETCH);
                self.send(io, Command::FetchBody, &text);
            }
            Some(Task::Flag { uid, flag, on, .. }) => {
                let sign = if on { '+' } else { '-' };
                self.send(
                    io,
                    Command::Store,
                    &format!("UID STORE {uid} {sign}FLAGS.SILENT ({flag})"),
                );
            }
            _ => self.next_task(io, events),
        }
    }

    fn fetch_headers(&mut self, io: &mut dyn Io) {
        let Some(Task::Sync { count, .. }) = &self.task else {
            return;
        };
        let first = self
            .exists
            .saturating_sub((*count).clamp(1, store::MAX_MESSAGES) - 1)
            .max(1);
        let text = format!(
            "FETCH {first}:{} (UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID)])",
            self.exists
        );
        self.send(io, Command::FetchHeaders, &text);
    }

    /// The list and the counts of a folder are written, newest first.
    fn finish_sync(&mut self, io: &mut dyn Io, storage: &dyn Storage, events: &mut Vec<Event>) {
        let Some(Task::Sync {
            index, status, now, ..
        }) = self.task.clone()
        else {
            return;
        };
        let mut rows = core::mem::take(&mut self.rows);
        paperpaw_sdk::sort::sort_by_key(&mut rows, |r| core::cmp::Reverse(r.uid));
        let unseen = self.counts.map_or_else(
            || rows.iter().filter(|r| !r.seen()).count() as u32,
            |(_, unseen)| unseen,
        );
        let messages = self.counts.map_or(self.exists, |(messages, _)| messages);
        let counts = Counts {
            messages,
            unseen,
            uid_validity: self.uid_validity,
            checked: now,
        };
        let written = store::write_index(storage, &index, &rows)
            .and_then(|()| store::write_atomic(storage, &status, counts.line().as_bytes()));
        if written.is_err() {
            events.extend(self.fail(io, Failure::Storage));
            return;
        }
        self.next_task(io, events);
    }
}

/// `\0user\0password`.
fn auth_plain_data(endpoint: &Endpoint) -> Vec<u8> {
    let mut data = Vec::with_capacity(endpoint.user.len() + endpoint.password.len() + 2);
    data.push(0);
    data.extend_from_slice(endpoint.user.as_bytes());
    data.push(0);
    data.extend_from_slice(endpoint.password.as_bytes());
    data
}

/// `INTERNALDATE "03-Nov-2026 09:15:00 +0100"` as seconds.
fn imap_date(text: &str) -> Option<i64> {
    let text = text.trim();
    let (day, rest) = text.split_once('-')?;
    let (month, rest) = rest.split_once('-')?;
    crate::header::date_seconds(&format!("{} {month} {rest}", day.trim()))
}

/// The folders above `file` exist.
fn storage_prepare(storage: &dyn Storage, file: &str) -> Result<(), paperpaw_sdk::StorageError> {
    if let Some((parent, _)) = file.rsplit_once('/') {
        let mut path = String::new();
        for part in parent.split('/') {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            if !storage.exists(&path) {
                storage.create_dir(&path)?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- SMTP

/// A message to send: the envelope and the bytes DATA takes
/// ([`smtp::data`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub from: String,
    pub to: Vec<String>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Greeting,
    Ehlo,
    StartTls,
    SecureEhlo,
    AuthPlain,
    LoginUser,
    LoginName,
    LoginPassword,
    MailFrom,
    Rcpt(usize),
    Data,
    Body,
    Quit,
}

/// The SMTP side: one message sent in one connection.
#[derive(Debug)]
pub struct Smtp {
    endpoint: Endpoint,
    message: Outgoing,
    wire: Wire,
    replies: ReplyParser,
    step: Step,
    extensions: Extensions,
    secure: bool,
    finished: bool,
}

impl Smtp {
    pub fn new(endpoint: Endpoint, message: Outgoing) -> Self {
        Self {
            endpoint,
            message,
            wire: Wire::default(),
            replies: ReplyParser::default(),
            step: Step::Greeting,
            extensions: Extensions::default(),
            secure: false,
            finished: false,
        }
    }

    pub fn start(&mut self, io: &mut dyn Io) -> Vec<Event> {
        let e = &self.endpoint;
        match e.security {
            Security::Implicit => io.connect_tls(&e.host, e.port, e.tls()),
            Security::StartTls => io.connect_plain(&e.host, e.port),
        }
        alloc::vec![Event::Stage(Stage::Connecting)]
    }

    fn fail(&mut self, io: &mut dyn Io, failure: Failure) -> Vec<Event> {
        self.finished = true;
        io.close();
        alloc::vec![Event::Failed(failure)]
    }

    fn say(&mut self, io: &mut dyn Io, step: Step, text: &str) {
        self.step = step;
        io.send(format!("{text}\r\n").into_bytes());
    }

    pub fn step(&mut self, input: Input, io: &mut dyn Io) -> Vec<Event> {
        if self.finished {
            return Vec::new();
        }
        match input {
            Input::Connected => {
                self.wire.more(io);
                Vec::new()
            }
            Input::Secured { time_checked, .. } => {
                if !time_checked {
                    return self.fail(io, Failure::ClockNotSet);
                }
                self.secure = true;
                self.wire.switching = false;
                if self.step == Step::StartTls {
                    // The server forgets what it offered: asked again.
                    self.say(io, Step::SecureEhlo, "EHLO localhost");
                }
                self.wire.more(io);
                Vec::new()
            }
            Input::Untrusted {
                certificate,
                key,
                why,
            } => {
                self.finished = true;
                io.close();
                alloc::vec![Event::Untrusted {
                    certificate,
                    key,
                    why
                }]
            }
            Input::Bytes(bytes) => {
                self.wire.reading = false;
                if bytes.is_empty() {
                    return self.lost(io);
                }
                self.wire.buf.push(&bytes);
                self.run(io)
            }
            Input::Saved(_) => Vec::new(),
            Input::Closed => self.lost(io),
            Input::Failed(error, _) => {
                self.finished = true;
                alloc::vec![Event::Failed(failure_of(error))]
            }
        }
    }

    fn lost(&mut self, io: &mut dyn Io) -> Vec<Event> {
        self.finished = true;
        io.close();
        if self.step == Step::Quit {
            alloc::vec![Event::Done]
        } else {
            alloc::vec![Event::Failed(Failure::Network(SocketError::Lost))]
        }
    }

    fn run(&mut self, io: &mut dyn Io) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            if self.finished {
                return events;
            }
            match self.wire.buf.line() {
                Err(_) => {
                    events.extend(self.fail(io, Failure::Protocol));
                    return events;
                }
                Ok(None) => {
                    self.wire.more(io);
                    return events;
                }
                Ok(Some(line)) => {
                    if let Some(reply) = self.replies.line(&line) {
                        self.reply(reply, io, &mut events);
                    }
                }
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one match over the steps of the dialogue; split it when it is next changed"
    )]
    fn reply(&mut self, reply: Reply, io: &mut dyn Io, events: &mut Vec<Event>) {
        if reply.code == 0 {
            events.extend(self.fail(io, Failure::Protocol));
            return;
        }
        if reply.code == 421 {
            events.extend(self.fail(io, Failure::Temporary(String::from(reply.text()))));
            return;
        }
        match self.step {
            Step::Greeting => {
                if reply.code != 220 {
                    events.extend(self.fail(io, Failure::Refused(String::from(reply.text()))));
                    return;
                }
                self.say(io, Step::Ehlo, "EHLO localhost");
            }
            Step::Ehlo => {
                if !reply.is_ok() {
                    events.extend(self.fail(io, Failure::Refused(String::from(reply.text()))));
                    return;
                }
                self.extensions = Extensions::of(&reply);
                if self.secure {
                    self.authenticate(io, events);
                } else if self.extensions.starttls {
                    self.say(io, Step::StartTls, "STARTTLS");
                } else {
                    // Never a password in the clear.
                    events.extend(self.fail(io, Failure::Unsafe));
                }
            }
            Step::StartTls => {
                if reply.code != 220 {
                    events.extend(self.fail(io, Failure::Unsafe));
                    return;
                }
                // Nothing is sent or read until the switch is done.
                self.wire.buf = LineBuffer::default();
                self.wire.reading = false;
                self.wire.switching = true;
                io.start_tls(self.endpoint.tls());
            }
            Step::SecureEhlo => {
                if !reply.is_ok() {
                    events.extend(self.fail(io, Failure::Refused(String::from(reply.text()))));
                    return;
                }
                self.extensions = Extensions::of(&reply);
                self.authenticate(io, events);
            }
            Step::AuthPlain | Step::LoginPassword => {
                if reply.code == 235 {
                    self.envelope(io, events);
                } else {
                    events.extend(self.fail(io, Failure::Login(String::from(reply.text()))));
                }
            }
            Step::LoginUser => {
                if reply.code != 334 {
                    events.extend(self.fail(io, Failure::Login(String::from(reply.text()))));
                    return;
                }
                let user = codec::base64_line(self.endpoint.user.as_bytes());
                self.say(io, Step::LoginName, &user);
            }
            Step::LoginName => {
                if reply.code != 334 {
                    events.extend(self.fail(io, Failure::Login(String::from(reply.text()))));
                    return;
                }
                let password = codec::base64_line(self.endpoint.password.as_bytes());
                self.say(io, Step::LoginPassword, &password);
            }
            Step::MailFrom => {
                if !reply.is_ok() {
                    events.extend(self.fail(io, Failure::Refused(String::from(reply.text()))));
                    return;
                }
                self.rcpt(0, io, events);
            }
            Step::Rcpt(n) => {
                if !reply.is_ok() {
                    let who = self.message.to.get(n).cloned().unwrap_or_default();
                    let text = String::from(reply.text());
                    let failure = if reply.is_temporary() {
                        Failure::Temporary(text)
                    } else {
                        Failure::Recipient(who, text)
                    };
                    events.extend(self.fail(io, failure));
                    return;
                }
                self.rcpt(n + 1, io, events);
            }
            Step::Data => {
                if reply.code != 354 {
                    events.extend(self.fail(io, Failure::Refused(String::from(reply.text()))));
                    return;
                }
                events.push(Event::Stage(Stage::Sending));
                self.step = Step::Body;
                io.send(core::mem::take(&mut self.message.data));
            }
            Step::Body => {
                if !reply.is_ok() {
                    let text = String::from(reply.text());
                    let failure = if reply.is_temporary() {
                        Failure::Temporary(text)
                    } else {
                        Failure::Refused(text)
                    };
                    events.extend(self.fail(io, failure));
                    return;
                }
                self.say(io, Step::Quit, "QUIT");
            }
            Step::Quit => {
                self.finished = true;
                io.close();
                events.push(Event::Done);
            }
        }
    }

    fn authenticate(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) {
        events.push(Event::Stage(Stage::LoggingIn));
        if self.extensions.auth_plain {
            let line = smtp::auth_plain(&self.endpoint.user, &self.endpoint.password);
            self.step = Step::AuthPlain;
            io.send(line.into_bytes());
        } else if self.extensions.auth_login {
            self.say(io, Step::LoginUser, "AUTH LOGIN");
        } else {
            events.extend(self.fail(io, Failure::Unsafe));
        }
    }

    fn envelope(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) {
        if self.extensions.size > 0 && self.message.data.len() as u64 > self.extensions.size {
            events.extend(self.fail(
                io,
                Failure::Refused(String::from("the message is larger than the server takes")),
            ));
            return;
        }
        let line = smtp::mail_from(&self.message.from);
        self.step = Step::MailFrom;
        io.send(line.into_bytes());
    }

    fn rcpt(&mut self, n: usize, io: &mut dyn Io, events: &mut Vec<Event>) {
        let _ = events;
        match self.message.to.get(n) {
            Some(address) => {
                let line = smtp::rcpt_to(address);
                self.step = Step::Rcpt(n);
                io.send(line.into_bytes());
            }
            None => self.say(io, Step::Data, "DATA"),
        }
    }
}
