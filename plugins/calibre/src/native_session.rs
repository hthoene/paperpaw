//! The wireless session with Calibre, without host calls: what Calibre
//! says is answered, what it sends is received, what it asks to delete is
//! deleted, and the screen's state follows. The plugin (`native_plugin.rs`)
//! feeds it what comes (an answer to a broadcast, a connection made, bytes
//! read) and does the [`Step`]s it returns; what it needs of the device and of
//! the books Calibre sent is behind [`Backing`].
//!
//! It is the interface-6 screen's logic made fallible: a refused allocation is
//! `NoMemory`, nothing indexes text in a way that can panic, and a message is
//! held whole up to the framing's bound ([`crate::native_model::max_message`]).
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy, decimal},
};

use crate::native_model::{self as model, Framing, Message, Refused, op};

/// How long to wait for Calibre's answer to a broadcast.
pub const FIND_MS: u32 = 1500;
/// Broadcasts sent before Calibre counts as not found: a datagram may be
/// lost on the way, and nothing says so.
pub const FIND_TRIES: u8 = 4;
/// Space reported to Calibre where the storage cannot tell: Calibre refuses
/// to send to a device it thinks full.
const SPACE: u64 = 64 << 30;
/// Bytes of the book list sent to Calibre in one part, about.
pub const LIST_PART: usize = 16 * 1024;
/// The password is not asked for more than once in a row: a wrong one ends
/// the connection and the reader types it again.
const OK_EMPTY: &str = "{}";

/// Why a connection ended: shown, in the reader's language, as the status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// There was no network; `managed` where PaperPaw manages the Wi-Fi
    /// (the words then name the settings).
    NoNetwork { managed: bool },
    /// Calibre did not answer.
    NotFound,
    /// The connection ended (by Calibre, or lost).
    Closed,
    /// Calibre ejected the device and closed.
    Ejected,
    /// The other side does not speak Calibre's protocol.
    NotCalibre,
    /// The connection failed, for this reason.
    Failed(String),
    /// Calibre said there was an error, in these words.
    Error(String),
}

/// Where the connection is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Asking the network for Calibre.
    Finding,
    /// Calibre answered from a computer not trusted yet: connect to it?
    Found {
        host: String,
        port: u16,
    },
    Connecting,
    Connected,
    /// A book is coming: its title, which of how many.
    Receiving {
        title: String,
        number: u64,
        total: u64,
    },
    /// Calibre asked for a password, or the one kept was wrong.
    Password,
    Ended(Ending),
}

/// What the plugin does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Queue this frame to Calibre.
    Send(Vec<u8>),
    /// Read what comes next of a message ([`Session::wanted`] bytes).
    Read,
    /// Receive `len` bytes into the incoming file (or, `dropped`, into the
    /// file whose bytes are not kept).
    ReadFile { len: u64, dropped: bool },
    /// Close the connection.
    Disconnect,
    /// Connect to Calibre at `host`.
    Connect { host: String, port: u16 },
    /// Broadcast for Calibre.
    Find,
    /// Tell the reader: a book was not received because of a file there
    /// already.
    NotReplaced { title: String },
    /// Delete the book at `path` below the library folder.
    Delete { path: String },
    /// The bytes of the book are in the incoming file: put it in the library.
    Import(Coming),
    /// Remove the file of bytes that were taken and dropped.
    RemoveDropped,
    /// The screen shows something else.
    Redraw,
}

impl Default for Step {
    /// A step that does nothing (what is left in a list once a step is taken).
    fn default() -> Self {
        Self::Redraw
    }
}

/// A book coming: Calibre's `lpath`, the path it is to have below the library
/// folder, and its metadata as Calibre gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coming {
    pub lpath: String,
    pub path: String,
    pub metadata: String,
}

/// What the session asks of the device.
pub trait Backing {
    /// Whether Calibre sent the book at `lpath` (and it was not deleted
    /// since): only such a book is replaced or deleted when Calibre asks.
    ///
    /// # Errors
    /// `NoMemory`.
    fn sent(&self, lpath: &str) -> Result<bool, Error>;
    /// Keeps the metadata of the book at `lpath`.
    ///
    /// # Errors
    /// `NoMemory`.
    fn remember(&mut self, lpath: &str, metadata: &str) -> Result<(), Error>;
    /// Forgets the book at `lpath`; its `uuid`, if Calibre gave one.
    ///
    /// # Errors
    /// `NoMemory`.
    fn forget(&mut self, lpath: &str) -> Result<Option<String>, Error>;
    /// The ids of the books Calibre sent that are still in the library.
    ///
    /// # Errors
    /// `NoMemory`.
    fn books(&self) -> Result<Vec<u128>, Error>;
    /// The metadata of the book with `id`, as Calibre gave it.
    ///
    /// # Errors
    /// `NoMemory`.
    fn metadata(&self, id: u128) -> Result<Option<String>, Error>;
    /// What Calibre said about itself last.
    ///
    /// # Errors
    /// `NoMemory`.
    fn device_info(&self) -> Result<Option<String>, Error>;
    /// Keeps what Calibre says about itself.
    ///
    /// # Errors
    /// `NoMemory`.
    fn set_device_info(&mut self, info: &str) -> Result<(), Error>;
    /// The address of the computer the reader chose to trust, if any.
    ///
    /// # Errors
    /// `NoMemory`.
    fn trusted(&self) -> Result<Option<String>, Error>;
    /// Trusts the computer at `host` from now on, in place of another.
    ///
    /// # Errors
    /// `NoMemory`.
    fn trust(&mut self, host: &str) -> Result<(), Error>;
    /// The file extensions a format of the device reads.
    ///
    /// # Errors
    /// `NoMemory`.
    fn extensions(&self) -> Result<Vec<String>, Error>;
    /// The password set in Calibre, kept as a secret; the plugin wipes the
    /// copy it is given.
    ///
    /// # Errors
    /// `NoMemory`.
    fn password(&self) -> Result<Option<crate::native_keep::Secret>, Error>;
    /// The device's name as others see it.
    ///
    /// # Errors
    /// `NoMemory`.
    fn device_name(&self) -> Result<String, Error>;
    /// The version the device tells Calibre.
    fn version(&self) -> &str;
    /// Free and total space in bytes, where the storage tells.
    fn space(&self) -> (Option<u64>, Option<u64>);
}

/// The books listed to Calibre (`GET_BOOK_COUNT`): a part at a time, read from
/// the card as it goes, so a long list never waits whole in memory.
#[derive(Debug)]
struct Listing {
    books: Vec<u128>,
    next: usize,
}

/// The session.
#[derive(Debug)]
pub struct Session {
    pub state: State,
    framing: Framing,
    coming: Option<Coming>,
    listing: Option<Listing>,
    /// Calibre's name and library, once known.
    pub name: Option<String>,
    pub library: Option<String>,
    /// Books received in this session.
    pub received: u32,
    /// Broadcasts sent in this search.
    tries: u8,
    /// Whether PaperPaw manages the Wi-Fi (the words of "no network").
    managed: bool,
}

impl Session {
    /// A session for a device with `memory_bound` bytes for one message
    /// ([`model::max_message`]); `managed` where PaperPaw manages the Wi-Fi.
    #[must_use]
    pub fn new(memory_bound: usize, managed: bool) -> Self {
        Self {
            state: State::Finding,
            framing: Framing::new(memory_bound),
            coming: None,
            listing: None,
            name: None,
            library: None,
            received: 0,
            tries: 0,
            managed,
        }
    }

    /// How many bytes to read next of a message.
    #[must_use]
    pub fn wanted(&self) -> u32 {
        self.framing.wanted()
    }

    /// Whether a part of the book list is still to send.
    #[must_use]
    pub const fn listing(&self) -> bool {
        self.listing.is_some()
    }

    /// Starts again: finds Calibre, or connects to `address` (`host` or
    /// `host:port`) where one is set.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn start(&mut self, memory_bound: usize, address: &str) -> Result<Vec<Step>, Error> {
        self.framing = Framing::new(memory_bound);
        self.coming = None;
        self.listing = None;
        let mut steps = Vec::new();
        let address = address.trim();
        if address.is_empty() {
            self.state = State::Finding;
            self.tries = 1;
            push(&mut steps, Step::Find)?;
        } else {
            let (host, port) = match address.rsplit_once(':') {
                Some((host, port)) if port.parse::<u16>().is_ok() => {
                    (host, port.parse().unwrap_or(model::DEFAULT_PORT))
                }
                _ => (address, model::DEFAULT_PORT),
            };
            self.state = State::Connecting;
            push(
                &mut steps,
                Step::Connect {
                    host: copy(host)?,
                    port,
                },
            )?;
        }
        push(&mut steps, Step::Redraw)?;
        Ok(steps)
    }

    fn end(&mut self, ending: Ending) -> Result<Vec<Step>, Error> {
        self.state = State::Ended(ending);
        self.coming = None;
        self.listing = None;
        let mut steps = Vec::new();
        push(&mut steps, Step::Redraw)?;
        Ok(steps)
    }

    fn end_with(&mut self, ending: Ending, first: Step) -> Result<Vec<Step>, Error> {
        let mut steps = Vec::new();
        push(&mut steps, first)?;
        let more = self.end(ending)?;
        append(&mut steps, more)?;
        Ok(steps)
    }

    /// Calibre answered a broadcast with `text`, from the computer at `from`.
    ///
    /// # Errors
    /// `NoMemory`, or what the backing says.
    pub fn answer(
        &mut self,
        backing: &dyn Backing,
        text: &str,
        from: &str,
    ) -> Result<Vec<Step>, Error> {
        let mut steps = Vec::new();
        if !matches!(self.state, State::Finding | State::Found { .. }) {
            return Ok(steps);
        }
        let Some(port) = model::port_of_answer(text) else {
            return Ok(steps);
        };
        let trusted = backing.trusted()?.as_deref() == Some(from);
        if trusted || self.state == State::Finding {
            self.name = model::name_of_answer(text).map(copy).transpose()?;
        }
        if trusted {
            self.state = State::Connecting;
            push(
                &mut steps,
                Step::Connect {
                    host: copy(from)?,
                    port,
                },
            )?;
        } else if self.state == State::Finding {
            // Another computer may pose as Calibre: the reader says whether it
            // is theirs.
            self.state = State::Found {
                host: copy(from)?,
                port,
            };
        }
        push(&mut steps, Step::Redraw)?;
        Ok(steps)
    }

    /// The wait for answers to a broadcast is over; `no_network` where there
    /// was none.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn find_over(&mut self, no_network: bool) -> Result<Vec<Step>, Error> {
        if self.state != State::Finding {
            return Ok(Vec::new());
        }
        if !no_network && self.tries < FIND_TRIES {
            // Not heard (or not answered) yet: asked again.
            self.tries += 1;
            let mut steps = Vec::new();
            push(&mut steps, Step::Find)?;
            return Ok(steps);
        }
        let ending = if no_network {
            Ending::NoNetwork {
                managed: self.managed,
            }
        } else {
            Ending::NotFound
        };
        self.end(ending)
    }

    /// The reader chose to connect to the computer found: trusted from now on.
    ///
    /// # Errors
    /// `NoMemory`, or what the backing says.
    pub fn connect_found(&mut self, backing: &mut dyn Backing) -> Result<Vec<Step>, Error> {
        let State::Found { host, port } = &self.state else {
            return Ok(Vec::new());
        };
        let (host, port) = (copy(host)?, *port);
        backing.trust(&host)?;
        self.state = State::Connecting;
        let mut steps = Vec::new();
        push(&mut steps, Step::Connect { host, port })?;
        push(&mut steps, Step::Redraw)?;
        Ok(steps)
    }

    /// The connection was made.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn connected(&mut self) -> Result<Vec<Step>, Error> {
        self.state = State::Connected;
        let mut steps = Vec::new();
        push(&mut steps, Step::Read)?;
        push(&mut steps, Step::Redraw)?;
        Ok(steps)
    }

    /// The connection ended, by itself (`reason` says why where it failed).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn closed(&mut self, reason: Option<&str>) -> Result<Vec<Step>, Error> {
        self.coming = None;
        self.listing = None;
        if matches!(self.state, State::Ended(_) | State::Password) {
            return Ok(Vec::new());
        }
        let ending = match reason {
            Some(reason) if !reason.is_empty() => Ending::Failed(copy(reason)?),
            _ => Ending::Closed,
        };
        self.end(ending)
    }

    /// There was no network for the connection.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn no_network(&mut self) -> Result<Vec<Step>, Error> {
        self.coming = None;
        self.listing = None;
        self.end(Ending::NoNetwork {
            managed: self.managed,
        })
    }

    /// Calibre closed the connection: as it does after ejecting (said so
    /// already), or else said now.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn closed_by_calibre(&mut self) -> Result<Vec<Step>, Error> {
        let mut steps = Vec::new();
        push(&mut steps, Step::Disconnect)?;
        if !matches!(self.state, State::Ended(_)) {
            let more = self.end(Ending::Closed)?;
            append(&mut steps, more)?;
        }
        Ok(steps)
    }

    /// Bytes of Calibre's came (valid text). A message, once whole, is
    /// answered.
    ///
    /// # Errors
    /// `NoMemory`, or what the backing says.
    pub fn received(&mut self, backing: &mut dyn Backing, text: &str) -> Result<Vec<Step>, Error> {
        match self.framing.feed(text) {
            Ok(Some(message)) => self.handle(backing, message),
            Ok(None) => {
                let mut steps = Vec::new();
                push(&mut steps, Step::Read)?;
                Ok(steps)
            }
            Err(Refused::NoMemory) => Err(Error::NoMemory),
            Err(_) => self.end_with(Ending::NotCalibre, Step::Disconnect),
        }
    }

    /// What came is not text: not Calibre's protocol.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn not_text(&mut self) -> Result<Vec<Step>, Error> {
        self.end_with(Ending::NotCalibre, Step::Disconnect)
    }

    fn ok(steps: &mut Vec<Step>, args: &str) -> Result<(), Error> {
        push(steps, Step::Send(model::frame(op::OK, args)?))
    }

    /// Answers a message of Calibre's, then reads on (unless a book or the
    /// end comes).
    #[allow(
        clippy::too_many_lines,
        reason = "one match over the protocol's operations"
    )]
    fn handle(&mut self, backing: &mut dyn Backing, message: Message) -> Result<Vec<Step>, Error> {
        let mut steps = Vec::new();
        let args = message.args.as_str();
        match message.op {
            op::GET_INITIALIZATION_INFO => {
                let challenge = model::text(args, "passwordChallenge")?.unwrap_or_default();
                let password = backing.password()?;
                let extensions = backing.extensions()?;
                let extensions = borrowed(&extensions)?;
                let name = backing.device_name()?;
                let device = model::Device {
                    name: &name,
                    extensions: &extensions,
                    challenge: &challenge,
                    password: password.as_ref().map(crate::native_keep::Secret::as_str),
                    version: backing.version(),
                };
                let answer = model::initialization(&device)?;
                // The answer carries the hash by design; its copy is wiped
                // once it is framed (the frame is wiped after the write).
                let frame = model::frame(op::OK, &answer);
                paperpaw_ui_kit::mem::wipe_text(answer);
                push(&mut steps, Step::Send(frame?))?;
            }
            op::SET_CALIBRE_DEVICE_INFO => {
                backing.set_device_info(args)?;
                Self::ok(&mut steps, OK_EMPTY)?;
            }
            op::SET_CALIBRE_DEVICE_NAME | op::SET_LIBRARY_INFO => {
                if let Some(name) = model::text(args, "libraryName")? {
                    self.library = Some(name);
                    push(&mut steps, Step::Redraw)?;
                }
                Self::ok(&mut steps, OK_EMPTY)?;
            }
            op::GET_DEVICE_INFORMATION => {
                let stored = backing.device_info()?;
                let name = backing.device_name()?;
                let info = model::device_information(stored.as_deref(), &name, backing.version())?;
                Self::ok(&mut steps, &info)?;
            }
            op::TOTAL_SPACE => {
                let (free, total) = Self::space(backing);
                let _ = free;
                let mut digits = [0; 20];
                let reply = concat(&[
                    "{\"total_space_on_device\": ",
                    decimal_u64(total, &mut digits),
                    "}",
                ])?;
                Self::ok(&mut steps, &reply)?;
            }
            op::FREE_SPACE => {
                let (free, _) = Self::space(backing);
                let mut digits = [0; 20];
                let reply = concat(&[
                    "{\"free_space_on_device\": ",
                    decimal_u64(free, &mut digits),
                    "}",
                ])?;
                Self::ok(&mut steps, &reply)?;
            }
            op::GET_BOOK_COUNT => {
                let books = backing.books()?;
                let mut digits = [0; 20];
                let count = concat(&[
                    "{\"count\": ",
                    decimal(books.len(), &mut digits),
                    ", \"willStream\": true, \"willScan\": true}",
                ])?;
                Self::ok(&mut steps, &count)?;
                if !books.is_empty() {
                    // Their metadata in parts, then reading on.
                    self.listing = Some(Listing { books, next: 0 });
                    return Ok(steps);
                }
            }
            op::SEND_BOOK => return self.book(backing, args),
            op::DELETE_BOOK => self.delete(backing, args, &mut steps)?,
            op::NOOP => {
                if model::member(args, "ejecting").is_some() {
                    // Calibre closes once it has the answer: closed here first,
                    // the answer could be lost on the way.
                    Self::ok(&mut steps, OK_EMPTY)?;
                    push(&mut steps, Step::Read)?;
                    let more = self.end(Ending::Ejected)?;
                    append(&mut steps, more)?;
                    return Ok(steps);
                }
                // Without cached metadata Calibre counts nothing; a plain NOOP
                // keeps the connection alive.
                if model::member(args, "count").is_none() && model::member(args, "priKey").is_none()
                {
                    Self::ok(&mut steps, OK_EMPTY)?;
                }
            }
            op::DISPLAY_MESSAGE => {
                // Kind 1: the password is wrong or missing.
                if model::number(args, "messageKind") == Some(1) {
                    push(&mut steps, Step::Disconnect)?;
                    self.state = State::Password;
                    push(&mut steps, Step::Redraw)?;
                    return Ok(steps);
                }
            }
            op::GET_BOOK_FILE_SEGMENT | op::GET_BOOK_METADATA => {
                push(
                    &mut steps,
                    Step::Send(model::frame(
                        op::ERROR,
                        "{\"message\": \"PaperPaw does not send books\"}",
                    )?),
                )?;
            }
            op::ERROR => {
                let message = model::text(args, "message")?.unwrap_or_default();
                return self.end_with(Ending::Error(message), Step::Disconnect);
            }
            // Book lists and their metadata, busy, done: nothing to say.
            _ => {}
        }
        push(&mut steps, Step::Read)?;
        Ok(steps)
    }

    fn space(backing: &dyn Backing) -> (u64, u64) {
        let (free, total) = backing.space();
        let free = free.unwrap_or(SPACE);
        let total = total.unwrap_or(SPACE).max(free);
        (free, total)
    }

    fn delete(
        &mut self,
        backing: &mut dyn Backing,
        args: &str,
        steps: &mut Vec<Step>,
    ) -> Result<(), Error> {
        Self::ok(steps, OK_EMPTY)?;
        for lpath in model::strings(args, "lpaths")? {
            // Only books Calibre sent: never the reader's own.
            if backing.sent(&lpath)?
                && let Some(path) = model::safe_path(&lpath)?
            {
                push(steps, Step::Delete { path })?;
            }
            let uuid = backing.forget(&lpath)?.unwrap_or_default();
            let quoted = model::json_string(&uuid)?;
            let reply = concat(&["{\"uuid\": ", &quoted, "}"])?;
            Self::ok(steps, &reply)?;
        }
        Ok(())
    }

    /// A book is about to come: its bytes go to a file first.
    fn book(&mut self, backing: &dyn Backing, args: &str) -> Result<Vec<Step>, Error> {
        let mut steps = Vec::new();
        // Without its length, the bytes cannot be told from what follows, and
        // an empty book would replace the one sent before.
        let Some(length) = model::number(args, "length").filter(|n| *n > 0) else {
            // A book of no length has no bytes to take: nothing follows it.
            if model::number(args, "length").is_some() {
                push(&mut steps, Step::Read)?;
                return Ok(steps);
            }
            return self.end_with(Ending::NotCalibre, Step::Disconnect);
        };
        let lpath = model::text(args, "lpath")?.unwrap_or_default();
        let metadata = match model::member(args, "metadata") {
            Some(metadata) => copy(metadata)?,
            None => copy("{}")?,
        };
        let title = match model::text(&metadata, "title")? {
            Some(title) => title,
            None => copy(&lpath)?,
        };
        // Not a book PaperPaw reads, or a path out of the library: the bytes
        // are taken, then dropped.
        let extensions = backing.extensions()?;
        let extensions = borrowed(&extensions)?;
        let path = model::book_path(&lpath, &extensions)?;
        let number = model::number(args, "thisBook")
            .unwrap_or(0)
            .saturating_add(1);
        let total = model::number(args, "totalBooks").unwrap_or(1);
        let dropped = path.is_none();
        self.coming = path.map(|path| Coming {
            lpath,
            path,
            metadata,
        });
        self.state = State::Receiving {
            title,
            number,
            total,
        };
        push(
            &mut steps,
            Step::ReadFile {
                len: length,
                dropped,
            },
        )?;
        push(&mut steps, Step::Redraw)?;
        Ok(steps)
    }

    /// The bytes of the book are in the incoming file (or, dropped, in the
    /// file of dropped bytes).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn saved(&mut self) -> Result<Vec<Step>, Error> {
        let mut steps = Vec::new();
        match self.coming.take() {
            Some(coming) => push(&mut steps, Step::Import(coming))?,
            None => push(&mut steps, Step::RemoveDropped)?,
        }
        Ok(steps)
    }

    /// The book `coming` was put in the library, or `placed` is why it was not
    /// (`NotReplaced`: a file of that name is there that Calibre did not send).
    ///
    /// # Errors
    /// `NoMemory`, or what the backing says.
    pub fn imported(
        &mut self,
        backing: &mut dyn Backing,
        coming: &Coming,
        placed: Result<(), Placed>,
    ) -> Result<Vec<Step>, Error> {
        let mut steps = Vec::new();
        match placed {
            Ok(()) => {
                backing.remember(&coming.lpath, &coming.metadata)?;
                self.received = self.received.saturating_add(1);
            }
            Err(Placed::NotReplaced) => {
                let title = match model::text(&coming.metadata, "title")? {
                    Some(title) => title,
                    None => copy(&coming.lpath)?,
                };
                push(&mut steps, Step::NotReplaced { title })?;
            }
            Err(Placed::Failed) => {}
        }
        self.state = State::Connected;
        push(&mut steps, Step::Read)?;
        push(&mut steps, Step::Redraw)?;
        Ok(steps)
    }

    /// The rest of the book is dropped; reads on.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn dropped(&mut self) -> Result<Vec<Step>, Error> {
        self.state = State::Connected;
        let mut steps = Vec::new();
        push(&mut steps, Step::Read)?;
        push(&mut steps, Step::Redraw)?;
        Ok(steps)
    }

    /// The next part of the book list (the send queue is empty): about
    /// [`LIST_PART`] bytes of metadata, then reading on.
    ///
    /// # Errors
    /// `NoMemory`, or what the backing says.
    pub fn list_part(&mut self, backing: &dyn Backing) -> Result<Vec<Step>, Error> {
        let mut steps = Vec::new();
        let Some(mut listing) = self.listing.take() else {
            return Ok(steps);
        };
        let mut sent = 0;
        while sent < LIST_PART
            && let Some(book) = listing.books.get(listing.next)
        {
            listing.next += 1;
            let metadata = backing.metadata(*book)?;
            let metadata = metadata.as_deref().unwrap_or(OK_EMPTY);
            sent += metadata.len();
            Self::ok(&mut steps, metadata)?;
        }
        if listing.next < listing.books.len() {
            self.listing = Some(listing);
        } else {
            push(&mut steps, Step::Read)?;
        }
        Ok(steps)
    }

    /// Calibre asked for a password and the reader typed one: the connection
    /// starts again.
    #[must_use]
    pub const fn needs_password(&self) -> bool {
        matches!(self.state, State::Password)
    }
}

/// Why a book Calibre sent was not put in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placed {
    /// A file of that name is there that Calibre did not send (or that was
    /// moved since).
    NotReplaced,
    /// The library took no book (the card, memory): nothing is kept.
    Failed,
}

/// `more` moved to the end of `steps`, with the room made fallibly first.
fn append(steps: &mut Vec<Step>, mut more: Vec<Step>) -> Result<(), Error> {
    steps.try_reserve(more.len()).map_err(|_| Error::NoMemory)?;
    steps.append(&mut more);
    Ok(())
}

/// The strings as `&str`s, kept fallibly.
fn borrowed(strings: &[String]) -> Result<Vec<&str>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(strings.len())
        .map_err(|_| Error::NoMemory)?;
    for string in strings {
        out.push(string.as_str());
    }
    Ok(out)
}

/// `n` as decimal digits in `digits`.
fn decimal_u64(n: u64, digits: &mut [u8; 20]) -> &str {
    let mut at = digits.len();
    let mut rest = n;
    loop {
        at -= 1;
        // `rest % 10` is a digit: it fits a byte.
        if let Some(slot) = digits.get_mut(at) {
            *slot = b'0' + u8::try_from(rest % 10).unwrap_or(0);
        }
        rest /= 10;
        if rest == 0 || at == 0 {
            break;
        }
    }
    digits
        .get(at..)
        .and_then(|d| core::str::from_utf8(d).ok())
        .unwrap_or("0")
}
