//! The IMAP side of a mail session (see the parent module).
use super::super::codec;
use super::super::fail::Fail;
use super::super::header;
use super::super::imap::{self, Fault, LineBuffer, Response, Status, Token, Untagged};
use super::super::mem::{
    Error, Secret, lower_ascii, push, push_bytes, push_char, push_str, string_with, vec_with,
    wipe_text,
};
use super::super::store::{self, Counts, Folder, Row};
use super::{
    Endpoint, Event, Failure, Input, Io, Security, Stage, Wire, failure_of, io_failure, one, words,
};
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{Storage, StorageError};
use paperpaw_ui_kit::text;

/// A literal of headers over this is skipped, never kept.
const MAX_HEADERS_LITERAL: usize = 16 * 1024;
/// The pieces a message's second part is joined in.
const JOIN_PIECE: usize = 4096;

/// What to do on the server, in this order, in one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Task {
    /// The list of folders, written to `out`.
    Folders {
        /// The file.
        out: String,
    },
    /// The latest `count` messages of `folder` (as the server names it): their
    /// list to `index`, the counts to `status`; `now` is the time to keep with
    /// them (0 where the clock is not known).
    Sync {
        /// The folder.
        folder: String,
        /// How many messages at most.
        count: u32,
        /// The file of the list.
        index: String,
        /// The file of the counts.
        status: String,
        /// Seconds since 1970 of the sync.
        now: i64,
    },
    /// The message `uid` of `folder` as the server has it, to `out`.
    Body {
        /// The folder.
        folder: String,
        /// The message.
        uid: u32,
        /// The file.
        out: String,
    },
    /// Sets (`on`) or clears a flag (`\Seen`, `\Flagged`) of a message.
    Flag {
        /// The folder.
        folder: String,
        /// The message.
        uid: u32,
        /// The flag.
        flag: &'static str,
        /// Set or clear.
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
    /// Into memory (a block of headers); `keep` is false where it is too large
    /// and only skipped.
    Memory {
        left: usize,
        data: Vec<u8>,
        keep: bool,
    },
    /// The rest of a message is read by the socket into a file of its own, then
    /// joined to the first part.
    File { scratch: String },
    /// A literal of the server's that is of no use.
    Skip { left: usize },
}

#[derive(Debug, Default)]
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
    /// A session that does `tasks` at `endpoint`.
    #[must_use]
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
    ///
    /// # Errors
    /// [`Fail::NoMemory`] where not even the failure can be told.
    pub fn start(&mut self, io: &mut dyn Io) -> Result<Vec<Event>, Fail> {
        let mut events = Vec::new();
        let result = self.begin(io, &mut events);
        self.settle(io, events, result)
    }

    fn begin(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        let e = &self.endpoint;
        let connected = match e.security {
            Security::Implicit => io.connect_tls(&e.host, e.port, e.pin),
            Security::StartTls => io.connect_plain(&e.host, e.port),
        };
        self.command = Some(Command::Greeting);
        push(events, Event::Stage(Stage::Connecting))?;
        self.went(io, events, connected)?;
        Ok(())
    }

    /// An end of an operation: what was told, or, for want of memory, the
    /// session ends and says so.
    fn settle(
        &mut self,
        io: &mut dyn Io,
        events: Vec<Event>,
        result: Result<(), Fail>,
    ) -> Result<Vec<Event>, Fail> {
        match result {
            Ok(()) => Ok(events),
            Err(Fail::NoMemory) => {
                self.finished = true;
                io.close();
                drop(events);
                one(Event::Failed(Failure::NoMemory))
            }
            Err(other) => Err(other),
        }
    }

    fn has(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case(capability))
    }

    /// Ends the session, telling why.
    fn fail(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        failure: Failure,
    ) -> Result<(), Fail> {
        self.finished = true;
        io.close();
        Ok(push(events, Event::Failed(failure))?)
    }

    /// What an [`Io`] call said: `Ok(true)` where the session goes on.
    fn went(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        result: Result<(), Error>,
    ) -> Result<bool, Fail> {
        match result {
            Ok(()) => Ok(true),
            Err(error) => match io_failure(error) {
                None => Err(Fail::NoMemory),
                Some(failure) => {
                    self.fail(io, events, failure)?;
                    Ok(false)
                }
            },
        }
    }

    /// What a read of a line said: `None` where it ended the session.
    fn took<T>(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        result: Result<T, Fault>,
    ) -> Result<Option<T>, Fail> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(Fault::NoMemory) => Err(Fail::NoMemory),
            Err(_) => {
                self.fail(io, events, Failure::Protocol)?;
                Ok(None)
            }
        }
    }

    /// What a write to the card said: `Ok(true)` where the session goes on.
    fn stored(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        result: Result<(), Fail>,
    ) -> Result<bool, Fail> {
        match result {
            Ok(()) => Ok(true),
            Err(Fail::NoMemory) => Err(Fail::NoMemory),
            Err(_) => {
                self.fail(io, events, Failure::Storage)?;
                Ok(false)
            }
        }
    }

    /// Sends `parts` joined, then `\r\n`, from a buffer that wipes itself.
    fn line(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        parts: &[&str],
    ) -> Result<bool, Fail> {
        let mut size = 2;
        for part in parts {
            size += part.len();
        }
        let mut line = Secret::with_capacity(size)?;
        for part in parts {
            line.push_str(part)?;
        }
        line.push_str("\r\n")?;
        let sent = io.send(line.as_str().as_bytes());
        drop(line);
        self.went(io, events, sent)
    }

    /// Sends `parts` as the next command, tagged.
    fn command(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        command: Command,
        parts: &[&str],
    ) -> Result<bool, Fail> {
        self.tag += 1;
        self.command = Some(command);
        let mut digits = [0u8; 20];
        let tag = text::decimal(self.tag as usize, &mut digits);
        let mut size = 2 + 1 + tag.len() + 1;
        for part in parts {
            size += part.len();
        }
        let mut line = Secret::with_capacity(size)?;
        line.push_str("m")?;
        line.push_str(tag)?;
        line.push_str(" ")?;
        for part in parts {
            line.push_str(part)?;
        }
        line.push_str("\r\n")?;
        let sent = io.send(line.as_str().as_bytes());
        drop(line);
        self.went(io, events, sent)
    }

    /// What happened on the socket.
    ///
    /// # Errors
    /// [`Fail::NoMemory`] where not even the failure can be told.
    pub fn step(
        &mut self,
        input: Input<'_>,
        io: &mut dyn Io,
        storage: &dyn Storage,
    ) -> Result<Vec<Event>, Fail> {
        let mut events = Vec::new();
        let result = self.take(input, io, storage, &mut events);
        self.settle(io, events, result)
    }

    fn take(
        &mut self,
        input: Input<'_>,
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        if self.finished {
            return Ok(());
        }
        match input {
            Input::Connected => {
                self.more(io, events)?;
            }
            Input::Secured { time_checked, .. } => {
                if !time_checked {
                    return self.fail(io, events, Failure::ClockNotSet);
                }
                self.secure = true;
                self.wire.switching = false;
                if self.command == Some(Command::StartTls) {
                    // Over TLS now: what the server offers is asked again.
                    self.capabilities.clear();
                    if !self.command(io, events, Command::Capability, &["CAPABILITY"])? {
                        return Ok(());
                    }
                }
                self.more(io, events)?;
            }
            Input::Untrusted {
                certificate,
                key,
                why,
            } => {
                self.finished = true;
                io.close();
                push(
                    events,
                    Event::Untrusted {
                        certificate,
                        key,
                        why,
                    },
                )?;
            }
            Input::Bytes(bytes) => {
                self.wire.reading = false;
                if bytes.is_empty() {
                    return self.lost(io, events);
                }
                let pushed = self.wire.buf.push(bytes);
                if self.took(io, events, pushed)?.is_some() {
                    self.run(io, storage, events)?;
                }
            }
            Input::Saved(path) => {
                let waiting =
                    matches!(&self.literal, Some(Literal::File { scratch }) if scratch == path);
                if waiting {
                    self.literal = None;
                    let joined = self.join(storage, path);
                    if !self.stored(io, events, joined)? {
                        return Ok(());
                    }
                    self.run(io, storage, events)?;
                }
            }
            Input::Closed => return self.lost(io, events),
            Input::Failed(reason) => {
                self.finished = true;
                push(events, Event::Failed(failure_of(reason)))?;
            }
        }
        Ok(())
    }

    /// Asks for more bytes (unless asked for or switching).
    fn more(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<bool, Fail> {
        let asked = self.wire.more(io);
        self.went(io, events, asked)
    }

    /// Whether a message is being read into a file: the socket is busy and no
    /// notice comes until it is saved.
    #[must_use]
    pub fn receiving(&self) -> bool {
        matches!(self.literal, Some(Literal::File { .. }))
    }

    /// The socket ended: expected after LOGOUT, else a loss.
    fn lost(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        let after_logout = self.command == Some(Command::Logout);
        self.finished = true;
        io.close();
        push(
            events,
            if after_logout {
                Event::Done
            } else {
                Event::Failed(Failure::Network(
                    paperpaw_sdk::native::sockets::Reason::Lost,
                ))
            },
        )?;
        Ok(())
    }

    /// Works through what is in the buffer; asks for more when it is used up.
    fn run(
        &mut self,
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        while !self.finished {
            if self.literal.is_some() {
                if !self.take_literal(io, events)? {
                    return Ok(());
                }
                continue;
            }
            let next = self.wire.buf.line();
            match self.took(io, events, next)? {
                None => return Ok(()),
                Some(None) => {
                    self.more(io, events)?;
                    return Ok(());
                }
                Some(Some(line)) => {
                    if !self.respond(&line, io, storage, events)? {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    /// One line of the server. False where the read of a file took over (no
    /// more lines until it is in) or the session ended.
    fn respond(
        &mut self,
        line: &[u8],
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<bool, Fail> {
        if self.fetching.is_some() && self.literal.is_none() {
            // What follows a literal: maybe more items, then `)`.
            return self.rest_of_fetch(line, io, events);
        }
        let parsed = imap::response(line);
        let Some(parsed) = self.took(io, events, parsed)? else {
            return Ok(false);
        };
        let Some(response) = parsed else {
            return Ok(true);
        };
        if self.command == Some(Command::Greeting) {
            return self.greeting(response, io, events);
        }
        match response {
            Response::Continue(_) => {
                if self.command == Some(Command::Auth) {
                    return self.credentials(io, events);
                }
                Ok(true)
            }
            Response::Untagged(u) => self.untagged(u, line, io, storage, events),
            Response::Tagged { tag, status, text } => {
                let mine = tag
                    .strip_prefix('m')
                    .and_then(|n| n.parse::<u32>().ok())
                    .is_some_and(|n| n == self.tag);
                if !mine {
                    return Ok(true);
                }
                self.completed(status == Status::Ok, text, io, storage, events)?;
                Ok(!self.finished)
            }
        }
    }

    /// The answer to the server's `+`: the credentials, encoded.
    fn credentials(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<bool, Fail> {
        let data = codec::plain_credentials(&self.endpoint.user, self.endpoint.password.as_str())?;
        let encoded = codec::base64_line(data.as_slice())?;
        drop(data);
        let sent = self.line(io, events, &[&encoded]);
        wipe_text(encoded);
        sent
    }

    /// The server's first words: on to TLS where it is not yet, else to what
    /// the server offers.
    fn greeting(
        &mut self,
        response: Response,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
    ) -> Result<bool, Fail> {
        match response {
            Response::Untagged(Untagged::Bye(_)) | Response::Tagged { .. } => {
                self.fail(
                    io,
                    events,
                    Failure::Network(paperpaw_sdk::native::sockets::Reason::Lost),
                )?;
                Ok(false)
            }
            Response::Untagged(u) => {
                if let Untagged::Code { code, args } = u
                    && code == "CAPABILITY"
                {
                    self.capabilities = args;
                }
                if self.secure {
                    self.command(io, events, Command::Capability, &["CAPABILITY"])
                } else {
                    self.command(io, events, Command::StartTls, &["STARTTLS"])
                }
            }
            _ => Ok(true),
        }
    }

    fn untagged(
        &mut self,
        u: Untagged,
        line: &[u8],
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<bool, Fail> {
        match u {
            Untagged::Capability(list) => self.capabilities = list,
            Untagged::Code { code, args } => match code.as_str() {
                "CAPABILITY" => self.capabilities = args,
                "UIDVALIDITY" => {
                    self.uid_validity = args.first().and_then(|a| a.parse().ok()).unwrap_or(0);
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
                    let mut kept = Vec::new();
                    for flag in flags.iter().take(16) {
                        push(&mut kept, header::capped(&lower_ascii(flag)?, 32)?)?;
                    }
                    let folder = Folder {
                        name: imap::mailbox_display(&name)?,
                        raw: name,
                        flags: kept,
                    };
                    push(&mut self.folders, folder)?;
                }
            }
            Untagged::Status { items, .. } => {
                let get = |key: &str| items.iter().find(|(k, _)| k == key).map(|(_, v)| *v as u32);
                self.counts = Some((get("MESSAGES").unwrap_or(0), get("UNSEEN").unwrap_or(0)));
            }
            Untagged::Bye(_) => {
                if self.command != Some(Command::Logout) {
                    self.fail(
                        io,
                        events,
                        Failure::Network(paperpaw_sdk::native::sockets::Reason::Lost),
                    )?;
                    return Ok(false);
                }
            }
            Untagged::Fetch { items, .. } => {
                return self.fetch(&items, line, io, storage, events);
            }
            _ => {}
        }
        Ok(true)
    }

    /// A FETCH answer: its numbers, and the literal that may follow.
    fn fetch(
        &mut self,
        items: &[Token],
        line: &[u8],
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<bool, Fail> {
        let flags = imap::fetch_flags(items);
        let Some(flags) = self.took(io, events, flags)? else {
            return Ok(false);
        };
        self.fetching = Some(Fetching {
            uid: imap::fetch_number(items, "UID").unwrap_or(0) as u32,
            flags,
            size: imap::fetch_number(items, "RFC822.SIZE").unwrap_or(0),
            date: imap::fetch_text(items, "INTERNALDATE")
                .map(imap_date)
                .transpose()?
                .flatten()
                .unwrap_or(0),
            headers: Vec::new(),
        });
        let Some(len) = imap::literal_at_end(line) else {
            // No literal (flags changed, say): the answer is complete.
            self.finish_fetch()?;
            return Ok(true);
        };
        match self.command {
            Some(Command::FetchHeaders) => {
                self.literal = Some(Literal::Memory {
                    left: len,
                    data: Vec::new(),
                    keep: len <= MAX_HEADERS_LITERAL,
                });
            }
            Some(Command::FetchBody) => return self.body_literal(len, io, storage, events),
            _ => self.literal = Some(Literal::Skip { left: len }),
        }
        Ok(true)
    }

    /// A message's bytes (`len`): those that came with the line start its file,
    /// the socket reads the rest into a file of its own.
    fn body_literal(
        &mut self,
        len: usize,
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<bool, Fail> {
        let out = match &self.task {
            Some(Task::Body { out, .. }) if len <= store::MAX_FETCH + 1024 => words(out)?,
            _ => {
                self.fail(io, events, Failure::Protocol)?;
                return Ok(false);
            }
        };
        let carry = self.wire.buf.take(len)?;
        let written = storage.write(&out, &carry).map_err(Fail::from);
        if !self.stored(io, events, written)? {
            return Ok(false);
        }
        let left = len - carry.len();
        if left == 0 {
            return Ok(true);
        }
        self.scratch += 1;
        let scratch = text::format(format_args!("{out}.{}", self.scratch))?;
        let asked = io.read_file(&scratch, left as u64);
        self.literal = Some(Literal::File { scratch });
        self.went(io, events, asked)?;
        Ok(false)
    }

    /// Takes what a literal still needs from the buffer. False while it waits
    /// for more bytes or for a file.
    fn take_literal(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<bool, Fail> {
        match &mut self.literal {
            Some(Literal::Memory { left, data, keep }) => {
                let got = self.wire.buf.take(*left)?;
                *left -= got.len();
                if *keep {
                    push_bytes(data, &got)?;
                }
                if *left > 0 {
                    self.more(io, events)?;
                    return Ok(false);
                }
                let data = core::mem::take(data);
                let keep = *keep;
                if let (true, Some(f)) = (keep, &mut self.fetching) {
                    f.headers = data;
                }
            }
            Some(Literal::Skip { left }) => {
                let got = self.wire.buf.take(*left)?;
                *left -= got.len();
                if *left > 0 {
                    self.more(io, events)?;
                    return Ok(false);
                }
            }
            Some(Literal::File { .. }) => return Ok(false),
            None => return Ok(true),
        }
        self.literal = None;
        Ok(true)
    }

    /// The line after a literal: more items of the answer, then `)`.
    fn rest_of_fetch(
        &mut self,
        line: &[u8],
        io: &mut dyn Io,
        events: &mut Vec<Event>,
    ) -> Result<bool, Fail> {
        let items = imap::tokens(line);
        let Some(items) = self.took(io, events, items)? else {
            return Ok(false);
        };
        if let Some(f) = &mut self.fetching {
            if let Some(uid) = imap::fetch_number(&items, "UID") {
                f.uid = uid as u32;
            }
            if let Some(size) = imap::fetch_number(&items, "RFC822.SIZE") {
                f.size = size;
            }
            let flags = imap::fetch_flags(&items);
            let flags = match flags {
                Ok(flags) => flags,
                Err(Fault::NoMemory) => return Err(Fail::NoMemory),
                Err(_) => {
                    self.fail(io, events, Failure::Protocol)?;
                    return Ok(false);
                }
            };
            if !flags.is_empty() {
                f.flags = flags;
            }
        }
        self.finish_fetch()?;
        Ok(true)
    }

    fn finish_fetch(&mut self) -> Result<(), Fail> {
        let Some(f) = self.fetching.take() else {
            return Ok(());
        };
        if self.command == Some(Command::FetchHeaders)
            && f.uid != 0
            && self.rows.len() < store::MAX_MESSAGES as usize
        {
            let row = Row::from_headers(f.uid, &f.flags, f.date, f.size, &f.headers)?;
            push(&mut self.rows, row)?;
        }
        Ok(())
    }

    /// The scratch file of a literal's rest joined to the message's file.
    fn join(&mut self, storage: &dyn Storage, scratch: &str) -> Result<(), Fail> {
        let Some(Task::Body { out, .. }) = &self.task else {
            return Err(StorageError::InvalidPath.into());
        };
        let mut chunk = vec_with(JOIN_PIECE)?;
        chunk.resize(JOIN_PIECE, 0);
        let mut offset = 0u64;
        loop {
            let n = storage.read_at(scratch, offset, &mut chunk)?;
            if n == 0 {
                break;
            }
            storage.append(out, chunk.get(..n).unwrap_or(&[]))?;
            offset += n as u64;
        }
        storage.remove(scratch).ok();
        Ok(())
    }

    /// The command the session waited for is done.
    fn completed(
        &mut self,
        ok: bool,
        reply: String,
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        let Some(command) = self.command else {
            return Ok(());
        };
        match command {
            Command::Greeting => {}
            Command::StartTls => {
                if !ok {
                    return self.fail(io, events, Failure::Unsafe);
                }
                // The switch: nothing is sent or read until it is done.
                self.wire.buf = LineBuffer::default();
                self.wire.switching = true;
                let started = io.start_tls(self.endpoint.pin);
                self.went(io, events, started)?;
            }
            Command::Capability => {
                if !ok {
                    return self.fail(io, events, Failure::Protocol);
                }
                push(events, Event::Stage(Stage::LoggingIn))?;
                self.login(io, events)?;
            }
            Command::Auth | Command::Login => {
                if !ok {
                    return self.fail(io, events, Failure::Login(reply));
                }
                self.next_task(io, events)?;
            }
            Command::List => {
                if !ok {
                    return self.fail(io, events, Failure::Refused(reply));
                }
                let mut folders = core::mem::take(&mut self.folders);
                store::sort_folders(&mut folders);
                let written = match &self.task {
                    Some(Task::Folders { out }) => store::write_folders(storage, out, &folders),
                    _ => Ok(()),
                };
                if !self.stored(io, events, written)? {
                    return Ok(());
                }
                self.next_task(io, events)?;
            }
            Command::Select => {
                if !ok {
                    return self.fail(io, events, Failure::Refused(reply));
                }
                self.after_select(io, storage, events)?;
            }
            // The counts are not needed so much that a refusal ends it.
            Command::Status => self.fetch_headers(io, events)?,
            Command::FetchHeaders => {
                if !ok {
                    return self.fail(io, events, Failure::Refused(reply));
                }
                self.finish_sync(io, storage, events)?;
            }
            Command::FetchBody | Command::Store => {
                if !ok {
                    return self.fail(io, events, Failure::Refused(reply));
                }
                self.next_task(io, events)?;
            }
            Command::Logout => {
                self.finished = true;
                io.close();
                push(events, Event::Done)?;
            }
        }
        Ok(())
    }

    /// Logs in the safest way the server offers.
    fn login(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        if !self.secure {
            // Never a password in the clear.
            return self.fail(io, events, Failure::Unsafe);
        }
        if self.has("AUTH=PLAIN") {
            if self.has("SASL-IR") {
                let data =
                    codec::plain_credentials(&self.endpoint.user, self.endpoint.password.as_str())?;
                let encoded = codec::base64_line(data.as_slice())?;
                drop(data);
                let sent = self.command(
                    io,
                    events,
                    Command::Auth,
                    &["AUTHENTICATE PLAIN ", &encoded],
                );
                wipe_text(encoded);
                sent?;
            } else {
                self.command(io, events, Command::Auth, &["AUTHENTICATE PLAIN"])?;
            }
        } else if !self.has("LOGINDISABLED") {
            let user = imap::quoted(&self.endpoint.user)?;
            let password = imap::quoted_secret(self.endpoint.password.as_str())?;
            self.command(
                io,
                events,
                Command::Login,
                &["LOGIN ", &user, " ", password.as_str()],
            )?;
        } else {
            return self.fail(io, events, Failure::Unsafe);
        }
        Ok(())
    }

    /// Starts the next task, or logs out.
    fn next_task(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        self.task = self.tasks.pop_front();
        self.exists = 0;
        self.rows.clear();
        self.counts = None;
        self.uid_validity = 0;
        self.folders.clear();
        let Some(task) = &self.task else {
            self.command(io, events, Command::Logout, &["LOGOUT"])?;
            return Ok(());
        };
        let stage = match task {
            Task::Folders { .. } => Stage::Folders,
            Task::Sync { .. } => Stage::Headers,
            Task::Body { .. } => Stage::Message,
            Task::Flag { .. } => Stage::Updating,
        };
        let folder = match task.folder() {
            Some(folder) => Some(imap::quoted(folder)?),
            None => None,
        };
        push(events, Event::Stage(stage))?;
        match folder {
            None => self.command(io, events, Command::List, &["LIST \"\" \"*\""])?,
            Some(folder) => self.command(io, events, Command::Select, &["SELECT ", &folder])?,
        };
        Ok(())
    }

    fn after_select(
        &mut self,
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        match &self.task {
            Some(Task::Sync { folder, .. }) => {
                if self.exists == 0 {
                    // Nothing to fetch: the list is empty.
                    self.counts = Some((0, 0));
                    return self.finish_sync(io, storage, events);
                }
                let folder = imap::quoted(folder)?;
                self.command(
                    io,
                    events,
                    Command::Status,
                    &["STATUS ", &folder, " (MESSAGES UNSEEN)"],
                )?;
            }
            Some(Task::Body { uid, .. }) => {
                let line = text::format(format_args!(
                    "UID FETCH {uid} BODY.PEEK[]<0.{}>",
                    store::MAX_FETCH
                ))?;
                self.command(io, events, Command::FetchBody, &[&line])?;
            }
            Some(Task::Flag { uid, flag, on, .. }) => {
                let sign = if *on { '+' } else { '-' };
                let line =
                    text::format(format_args!("UID STORE {uid} {sign}FLAGS.SILENT ({flag})"))?;
                self.command(io, events, Command::Store, &[&line])?;
            }
            _ => return self.next_task(io, events),
        }
        Ok(())
    }

    fn fetch_headers(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        let Some(Task::Sync { count, .. }) = &self.task else {
            return Ok(());
        };
        let first = self
            .exists
            .saturating_sub((*count).clamp(1, store::MAX_MESSAGES) - 1)
            .max(1);
        let line = text::format(format_args!(
            "FETCH {first}:{} (UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID)])",
            self.exists
        ))?;
        self.command(io, events, Command::FetchHeaders, &[&line])?;
        Ok(())
    }

    /// The list and the counts of a folder are written, newest first.
    fn finish_sync(
        &mut self,
        io: &mut dyn Io,
        storage: &dyn Storage,
        events: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        let Some(Task::Sync {
            index, status, now, ..
        }) = &self.task
        else {
            return Ok(());
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
            checked: *now,
        };
        let written = store::write_index(storage, index, &rows)
            .and_then(|()| store::write_atomic(storage, status, counts.line()?.as_bytes()));
        if !self.stored(io, events, written)? {
            return Ok(());
        }
        self.next_task(io, events)
    }
}

/// `INTERNALDATE "03-Nov-2026 09:15:00 +0100"` as seconds.
fn imap_date(text: &str) -> Result<Option<i64>, Fail> {
    let text = text.trim();
    let Some((day, rest)) = text.split_once('-') else {
        return Ok(None);
    };
    let Some((month, rest)) = rest.split_once('-') else {
        return Ok(None);
    };
    let mut joined = string_with(text.len() + 2)?;
    push_str(&mut joined, day.trim())?;
    push_char(&mut joined, ' ')?;
    push_str(&mut joined, month)?;
    push_char(&mut joined, ' ')?;
    push_str(&mut joined, rest)?;
    Ok(header::date_seconds(&joined))
}
