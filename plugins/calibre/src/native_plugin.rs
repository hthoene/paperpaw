//! The interface-7 wireless library: while its screen is open, the device
//! finds Calibre in the local network (or connects to the address set), and
//! Calibre sends books to it, lists them and deletes them, as with a reader on
//! a cable. Books go into the library folder, where Calibre places them
//! (`Author/Title.epub`).
//!
//! The rules are `native_session`'s (host-free, tested); this file feeds it
//! what comes (the events of the socket and of the broadcast) and does what it
//! says: sends frames (a queue, pumped as the host's send queue drains), reads
//! the next piece of a message, receives a book into a file of the plugin's
//! data and puts it in the library (`Library::import`), deletes books Calibre
//! sent. `PORTING-7.md` lists what is not ported.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    device::{Device, DeviceCapability, DeviceClass},
    document::{Documents, FormatCursor},
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    files::Files,
    formatting::Arg,
    library::{Existing, Import, Library},
    registrations,
    reply::Reply,
    secrets::Secrets,
    settings::{self, SettingValue},
    sockets::{
        Broadcast, BroadcastEvent, BroadcastKind, Connect, FileRead, Outbox, PIECE, Progress,
        Reason, Root as SockRoot, SocketEvent, SocketId, SocketKind, Sockets,
    },
    storage::{self, Root},
    ui::{Notify, Open, Question, RequestId, Ui},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    read_event,
    text::{concat, copy},
};

use crate::declarations::{BROADCAST_TOKEN, OPEN_TOKEN, SCREEN, SOCKET_TOKEN};
use crate::native_keep::{self as keep, Secret};
use crate::native_model::{BROADCAST_PORTS, max_message};
use crate::native_session::{Backing, Coming, Ending, FIND_MS, Placed, Session, State, Step};
use crate::native_store::{Store, library_path};

mod image {
    include!(concat!(env!("OUT_DIR"), "/calibre_declarations.rs"));
}

/// Calibre's address, where it is not found by itself.
const ADDRESS: &str = "calibre.address";
/// The file a book's bytes are received into, and the one dropped bytes go to.
const INCOMING: &str = "incoming";
const DROPPED: &str = "dropped";
const LIBRARY_FOLDER: &str = "core.library-folder";
const DEVICE_NAME: &str = "core.device-name";
/// The longest device name Calibre is told, in characters.
const NAME_CHARS: usize = 40;

/// The records, the secret and the device facts, over `host`: built from the
/// fields it needs, so that the session (another field) stays borrowable.
macro_rules! store {
    ($plugin:ident, $host:expr) => {
        Store {
            host: $host,
            files: $plugin.files.as_ref(),
            secrets: $plugin.secrets.as_ref(),
            extensions: &$plugin.extensions,
            folder: &$plugin.folder,
            name: &$plugin.name,
            version: &$plugin.version,
        }
    };
}

/// What a row of the screen does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// Connect to the computer found.
    Connect,
    /// Try again, after it ended.
    Again,
    /// Type the password.
    Password,
}

/// The screen: its rows (what a tap on each does) and the hint below them.
#[derive(Debug, Default)]
struct View {
    list: ItemList,
    acts: Vec<Option<Act>>,
    note: String,
}

/// The interface-7 wireless library.
#[derive(Debug, Default)]
pub struct NativeCalibre {
    ui: Option<Ui>,
    sockets: Option<Sockets>,
    secrets: Option<Secrets>,
    library: Option<Library>,
    files: Option<Files>,
    /// Whether the services were looked for (one that is not there is not
    /// asked for again).
    opened: bool,
    screens: Screens<View>,
    session: Option<Session>,
    socket: Option<SocketId>,
    broadcast: Option<SocketId>,
    /// A read of the socket is under way (one at a time).
    reading: bool,
    /// Frames not queued to the host yet, and how many bytes of them are.
    outgoing: Vec<u8>,
    queued: usize,
    /// The end of a character that a read cut in two.
    carry: Vec<u8>,
    asked: Option<RequestId>,
    /// What the device is, read when the screen opens.
    small: bool,
    managed: bool,
    name: String,
    version: String,
    folder: String,
    extensions: Vec<String>,
}

impl Plugin for NativeCalibre {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Answered, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::Socket, kinds::Broadcast)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("calibre.screen")?)
            }
            Event::ScreenOpened { instance, .. } if token == SCREEN => {
                self.open(host)?;
                self.screens.open(instance, View::default())?;
                self.begin(host)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.screens.shown(token, instance),
            Event::ScreenClosed { instance, .. } => {
                if self.screens.top_instance(SCREEN) == Some(instance) {
                    self.end(host);
                }
                self.screens.close(instance);
                Ok(())
            }
            Event::Socket(event) if token == SOCKET_TOKEN => self.on_socket(host, &event),
            Event::Broadcast(event) if token == BROADCAST_TOKEN => self.on_broadcast(host, &event),
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = say.copy(host, "calibre.title")?;
        let footer = if view.note.is_empty() {
            Footer::None
        } else {
            Footer::Note(&view.note)
        };
        view.list.draw_with(host, &mut frame, &title, true, footer)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step {
            let act = view.acts.get(index).copied().flatten();
            match act {
                Some(Act::Connect) => self.connect_found(host)?,
                Some(Act::Again) => self.restart(host)?,
                Some(Act::Password) => self.ask_password(host)?,
                None => {}
            }
        }
        Ok(ItemList::handled(step))
    }
}

/// The name Calibre is told: the reader's (`core.device-name`) as far as it
/// shows something, else the device's model (an Xteink's as "PaperPaw X4").
fn device_name(set: &str, model: &str) -> Result<String, Error> {
    let mut name = String::new();
    for c in set.chars().filter(|c| !c.is_control()).take(NAME_CHARS) {
        name.try_reserve(c.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        name.push(c);
    }
    let name = name.trim();
    if !name.is_empty() {
        return copy(name);
    }
    match model.strip_prefix("Xteink ") {
        Some(rest) => concat(&["PaperPaw ", rest]),
        None if model.is_empty() => copy("PaperPaw"),
        None => copy(model),
    }
}

/// Short words for why a connection failed, as the interface-6 kernel gave
/// them.
const fn reason_words(reason: Reason) -> &'static str {
    match reason {
        Reason::Dns => "name not found",
        Reason::Tls | Reason::Name | Reason::Expired => "secure connection failed",
        Reason::Timeout => "timed out",
        Reason::Refused => "refused",
        Reason::Io => "card error",
        Reason::Full => "card full",
        Reason::Busy | Reason::SessionNeeded => "busy",
        Reason::Lost => "connection lost",
        Reason::ClosedEarly => "closed early",
        Reason::WorkEnded => "time ended",
        Reason::NoMemory => "no memory",
        _ => "error",
    }
}

impl NativeCalibre {
    /// Opens the services (the first callback has a live frame) and learns
    /// what the device is: its class, its name, the library folder, the
    /// extensions its formats read.
    fn open(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if !self.opened {
            self.opened = true;
            self.sockets = Self::service(Sockets::open(host))?;
            self.secrets = Self::service(Secrets::open(host))?;
            self.library = Self::service(Library::open(host))?;
            self.files = Self::service(Files::open(host))?;
            if let Err(error) = self.learn(host) {
                self.opened = false;
                return Err(error);
            }
        }
        Ok(())
    }

    /// A service that is not there is `None` (what needs it says so); a
    /// refused allocation is not an answer.
    fn service<T>(opened: Result<T, Error>) -> Result<Option<T>, Error> {
        match opened {
            Ok(service) => Ok(Some(service)),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(None),
        }
    }

    fn learn(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut model = String::new();
        if let Ok(device) = Device::open(host) {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            if let Ok(row) = device.device(host, &mut input, &mut out) {
                self.small = !matches!(row.class(), DeviceClass::Medium | DeviceClass::Large);
                self.managed = row.supports(DeviceCapability::Wifi);
                model = copy(row.model())?;
            } else {
                self.small = true;
            }
            // A handle that cannot be released is no reason to lose the facts.
            device.release(host).ok();
        } else {
            self.small = true;
        }
        let mut scratch = [0; 256];
        let set = match settings::read_setting(host, DEVICE_NAME, &mut scratch) {
            Ok(SettingValue::Text(text)) => copy(text)?,
            _ => String::new(),
        };
        self.name = device_name(&set, &model)?;
        self.version = copy(&alloc::format!("{}", paperpaw_sdk::SDK_VERSION))?;
        let mut scratch = [0; 256];
        self.folder = match settings::read_setting(host, LIBRARY_FOLDER, &mut scratch) {
            Ok(SettingValue::Text(text)) => copy(text.trim_matches('/'))?,
            _ => copy("books")?,
        };
        self.extensions = Vec::new();
        if let Ok(documents) = Documents::open(host) {
            let found = self.read_extensions(host, &documents);
            documents.release(host).ok();
            found?;
        }
        Ok(())
    }

    /// The extensions of every format, a page at a time.
    fn read_extensions(&mut self, host: &Host<'_>, documents: &Documents) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
        let mut cursor = FormatCursor::start();
        let mut restarted = false;
        loop {
            let page = match documents.formats(host, cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::State) if !restarted => {
                    restarted = true;
                    self.extensions.clear();
                    cursor = FormatCursor::start();
                    continue;
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(()),
            };
            for format in page.rows() {
                for extension in format.extensions() {
                    push(&mut self.extensions, copy(extension)?)?;
                }
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => return Ok(()),
            }
        }
    }

    fn address(host: &Host<'_>) -> Result<String, Error> {
        let mut scratch = [0; 280];
        match settings::read_setting(host, ADDRESS, &mut scratch) {
            Ok(SettingValue::Text(text)) => copy(text),
            _ => Ok(String::new()),
        }
    }

    /// The screen opened: a new session, started.
    fn begin(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.reset_wire(host);
        self.session = Some(Session::new(max_message(self.small), self.managed));
        let steps = self.start_steps(host)?;
        self.run(host, steps)
    }

    fn start_steps(&mut self, host: &Host<'_>) -> Result<Vec<Step>, Error> {
        let address = Self::address(host)?;
        let bound = max_message(self.small);
        match &mut self.session {
            Some(session) => session.start(bound, &address),
            None => Ok(Vec::new()),
        }
    }

    /// Tries again: the old connection closed, a new one started.
    fn restart(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.reset_wire(host);
        let steps = self.start_steps(host)?;
        self.run(host, steps)
    }

    /// The connection and the broadcast closed, what was waiting forgotten.
    fn reset_wire(&mut self, host: &Host<'_>) {
        if let (Some(sockets), Some(id)) = (&self.sockets, self.socket.take()) {
            let (Ok(mut input), Ok(mut out)) = (buffer(256), buffer(256)) else {
                return;
            };
            sockets.close(host, id, &mut input, &mut out).ok();
        }
        self.broadcast = None;
        self.reading = false;
        wipe(&mut self.outgoing);
        self.outgoing.clear();
        self.queued = 0;
        wipe(&mut self.carry);
        self.carry.clear();
        // What was received half-way is not kept.
        storage::remove(host, Root::Data, INCOMING).ok();
        storage::remove(host, Root::Data, DROPPED).ok();
    }

    /// The screen closed: the connection ends with it.
    fn end(&mut self, host: &Host<'_>) {
        self.reset_wire(host);
        self.session = None;
        self.asked = None;
    }

    fn connect_found(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let steps = {
            let mut store = store!(self, host);
            match &mut self.session {
                Some(session) => session.connect_found(&mut store)?,
                None => return Ok(()),
            }
        };
        self.run(host, steps)
    }

    /// Does the steps, and those they lead to; the screen is drawn anew once
    /// where any asked.
    fn run(&mut self, host: &Host<'_>, steps: Vec<Step>) -> Result<(), Error> {
        let mut steps = steps;
        let mut at = 0;
        let mut redraw = false;
        while let Some(step) = steps.get_mut(at).map(core::mem::take) {
            at += 1;
            if matches!(step, Step::Redraw) {
                redraw = true;
                continue;
            }
            for more in self.step(host, step)? {
                push(&mut steps, more)?;
            }
        }
        if redraw {
            self.refresh(host)?;
        }
        Ok(())
    }

    /// Does one step; the steps it leads to.
    fn step(&mut self, host: &Host<'_>, step: Step) -> Result<Vec<Step>, Error> {
        match step {
            Step::Send(mut frame) => {
                self.outgoing
                    .try_reserve(frame.len())
                    .map_err(|_| Error::NoMemory)?;
                self.outgoing.extend_from_slice(&frame);
                // The frame may hold the password's hash: wiped at once.
                wipe(&mut frame);
                self.pump(host)?;
                Ok(Vec::new())
            }
            Step::Read => self.read(host),
            Step::ReadFile { len, dropped } => self.read_file(host, len, dropped),
            Step::Disconnect => {
                self.reset_wire(host);
                Ok(Vec::new())
            }
            Step::Connect {
                host: address,
                port,
            } => self.connect(host, &address, port),
            Step::Find => self.find(host),
            Step::NotReplaced { title } => {
                let text = Messages::new()?.format(
                    host,
                    "calibre.not-replaced",
                    &[Arg::Text {
                        name: "title",
                        value: &title,
                    }],
                )?;
                self.notify(host, &text)?;
                Ok(Vec::new())
            }
            Step::Delete { path } => {
                self.delete(host, &path)?;
                Ok(Vec::new())
            }
            Step::Import(coming) => self.import(host, &coming),
            Step::RemoveDropped => {
                storage::remove(host, Root::Data, DROPPED).ok();
                match &mut self.session {
                    Some(session) => session.dropped(),
                    None => Ok(Vec::new()),
                }
            }
            Step::Redraw => Ok(Vec::new()),
        }
    }

    /// A notice, where it can be shown (best effort).
    fn notify(&self, host: &Host<'_>, text: &str) -> Result<(), Error> {
        let mut scratch = [0; 512];
        match Notify::new(text) {
            Ok(notice) => {
                CommandSender::new(host, &mut scratch).notify(notice).ok();
                Ok(())
            }
            Err(_) => Ok(()),
        }
    }

    /// Queues what waits to be sent, as far as the host's queue takes it.
    fn pump(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let (Some(sockets), Some(id)) = (&self.sockets, self.socket) else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(PIECE + 256)?, buffer(256)?);
        let progress = {
            let Some(rest) = self.outgoing.get(self.queued..) else {
                return Ok(());
            };
            if rest.is_empty() {
                return Ok(());
            }
            let mut outbox = Outbox::new(rest);
            let before = outbox.remaining();
            let progress = outbox.pump(sockets, host, id, &mut input, &mut out);
            (progress, before - outbox.remaining())
        };
        wipe(&mut input);
        let (progress, queued) = progress;
        self.queued += queued;
        match progress {
            Ok(Progress::Queued) => {
                // Everything is with the host: nothing is kept.
                wipe(&mut self.outgoing);
                self.outgoing.clear();
                self.queued = 0;
                Ok(())
            }
            // The rest goes when the host says its queue drained (`sent`).
            Ok(_) => Ok(()),
            // The socket ended meanwhile: its `closed` says so.
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(()),
        }
    }

    /// Asks for what comes next of a message.
    fn read(&mut self, host: &Host<'_>) -> Result<Vec<Step>, Error> {
        if self.reading {
            return Ok(Vec::new());
        }
        let (Some(sockets), Some(id), Some(session)) = (&self.sockets, self.socket, &self.session)
        else {
            return Ok(Vec::new());
        };
        // What a cut character already holds is not read again.
        let want = usize::try_from(session.wanted())
            .unwrap_or(usize::MAX)
            .saturating_sub(self.carry.len());
        if want == 0 {
            return match &mut self.session {
                Some(session) => session.not_text(),
                None => Ok(Vec::new()),
            };
        }
        let want = u32::try_from(want.min(PIECE)).unwrap_or(1);
        let (mut input, mut out) = (buffer(256)?, buffer(256)?);
        match sockets.read_bytes(host, id, want, true, &mut input, &mut out) {
            Ok(()) => {
                self.reading = true;
                Ok(Vec::new())
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // The socket is going: its `closed` or `failed` says why.
            Err(_) => Ok(Vec::new()),
        }
    }

    fn read_file(&mut self, host: &Host<'_>, len: u64, dropped: bool) -> Result<Vec<Step>, Error> {
        if self.reading {
            return Ok(Vec::new());
        }
        let (Some(sockets), Some(id)) = (&self.sockets, self.socket) else {
            return Ok(Vec::new());
        };
        let name = if dropped { DROPPED } else { INCOMING };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(256)?);
        let started = FileRead::new(SockRoot::Data, name, len)
            .and_then(|file| sockets.read_file(host, id, &file, &mut input, &mut out));
        match started {
            Ok(()) => {
                self.reading = true;
                Ok(Vec::new())
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // A length no file has: not Calibre's protocol.
            Err(Error::Invalid | Error::NoSpace) => match &mut self.session {
                Some(session) => session.not_text(),
                None => Ok(Vec::new()),
            },
            Err(_) => Ok(Vec::new()),
        }
    }

    fn connect(&mut self, host: &Host<'_>, address: &str, port: u16) -> Result<Vec<Step>, Error> {
        let Some(sockets) = &self.sockets else {
            return self.no_network();
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(256)?);
        let started = Connect::new(SOCKET_TOKEN, address, port)
            .and_then(|connect| sockets.connect(host, &connect, &mut input, &mut out));
        match started {
            Ok(id) => {
                self.socket = Some(id);
                Ok(Vec::new())
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(error) => {
                let words = match error {
                    Error::Invalid => "refused",
                    _ => "error",
                };
                match &mut self.session {
                    Some(session) => session.closed(Some(words)),
                    None => Ok(Vec::new()),
                }
            }
        }
    }

    fn no_network(&mut self) -> Result<Vec<Step>, Error> {
        match &mut self.session {
            Some(session) => session.no_network(),
            None => Ok(Vec::new()),
        }
    }

    fn find(&mut self, host: &Host<'_>) -> Result<Vec<Step>, Error> {
        let Some(sockets) = &self.sockets else {
            return self.no_network();
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(256)?);
        let started = Broadcast::new(BROADCAST_TOKEN, b"hello", FIND_MS, &BROADCAST_PORTS)
            .and_then(|find| sockets.broadcast(host, &find, &mut input, &mut out));
        match started {
            Ok(id) => {
                self.broadcast = Some(id);
                Ok(Vec::new())
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // Refused (power, the network off): there is no network to ask.
            Err(_) => match &mut self.session {
                Some(session) => session.find_over(true),
                None => Ok(Vec::new()),
            },
        }
    }

    /// Deletes the book at `path` (below the library folder) that Calibre
    /// sent: through the library, with what is kept for it; a file the library
    /// does not take for a book goes from the card.
    fn delete(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let full = library_path(&self.folder, path)?;
        if let Some(library) = &self.library {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            match library.delete(host, &[&full], true, &mut input, &mut out) {
                Ok(report) => {
                    let status = report.rows().next().map(|row| row.status());
                    if matches!(status, Some(Ok(()) | Err(Error::NotFound))) {
                        return Ok(());
                    }
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        // Not a book to the library, or no library: the file goes all the same.
        storage::remove(host, Root::Files, &full).ok();
        Ok(())
    }

    /// Puts the received book in the library, `Replace`ing the one Calibre
    /// sent before; a name that a file Calibre did not send has is not
    /// replaced.
    fn import(&mut self, host: &Host<'_>, coming: &Coming) -> Result<Vec<Step>, Error> {
        let sent = store!(self, host).sent(&coming.lpath)?;
        let (folder, name) = match coming.path.rsplit_once('/') {
            Some((folder, name)) => (folder, name),
            None => ("", coming.path.as_str()),
        };
        let placed = match &self.library {
            Some(library) => {
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                let existing = if sent {
                    Existing::Replace
                } else {
                    Existing::Refuse
                };
                let import = Import::new(INCOMING, name)
                    .folder(folder)
                    .existing(existing);
                match library.import(host, &import, &mut input, &mut out) {
                    Ok(_) => Ok(()),
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(Error::State | Error::Denied) => Err(Placed::NotReplaced),
                    Err(_) => Err(Placed::Failed),
                }
            }
            None => Err(Placed::Failed),
        };
        if placed.is_err() {
            storage::remove(host, Root::Data, INCOMING).ok();
        }
        let steps = {
            let mut store = store!(self, host);
            match &mut self.session {
                Some(session) => session.imported(&mut store, coming, placed)?,
                None => Vec::new(),
            }
        };
        Ok(steps)
    }

    fn on_socket(&mut self, host: &Host<'_>, event: &SocketEvent) -> Result<(), Error> {
        if self.socket != Some(event.id()) {
            return Ok(());
        }
        let steps = match event.kind() {
            SocketKind::Connected => match &mut self.session {
                Some(session) => session.connected()?,
                None => Vec::new(),
            },
            SocketKind::Received => {
                self.reading = false;
                self.received(host, event.len())?
            }
            SocketKind::Saved => {
                self.reading = false;
                match &mut self.session {
                    Some(session) => session.saved()?,
                    None => Vec::new(),
                }
            }
            SocketKind::Sent => {
                self.pump(host)?;
                let drained = self.outgoing.is_empty();
                match &self.session {
                    Some(session) if drained && session.listing() => {
                        let store = store!(self, host);
                        match &mut self.session {
                            Some(session) => session.list_part(&store)?,
                            None => Vec::new(),
                        }
                    }
                    _ => Vec::new(),
                }
            }
            SocketKind::Closed | SocketKind::Failed => {
                self.socket = None;
                self.reading = false;
                wipe(&mut self.outgoing);
                self.outgoing.clear();
                self.queued = 0;
                storage::remove(host, Root::Data, INCOMING).ok();
                let reason = event.reason();
                match &mut self.session {
                    Some(session) => match reason {
                        Reason::NoNetwork
                        | Reason::Offline
                        | Reason::NetworkOff
                        | Reason::Saving => session.no_network()?,
                        Reason::None | Reason::ScreenClosed => session.closed(None)?,
                        other => session.closed(Some(reason_words(other)))?,
                    },
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        self.run(host, steps)
    }

    /// `len` bytes of Calibre's came: taken, and as much of the message as is
    /// text given to the session (a cut character waits for the rest of it).
    fn received(&mut self, host: &Host<'_>, len: u32) -> Result<Vec<Step>, Error> {
        let Some(id) = self.socket else {
            return Ok(Vec::new());
        };
        if len == 0 {
            // The peer closed its side.
            return match &mut self.session {
                Some(session) => session.closed_by_calibre(),
                None => Ok(Vec::new()),
            };
        }
        let Some(sockets) = &self.sockets else {
            return Ok(Vec::new());
        };
        let wanted = usize::try_from(len).map_err(|_| Error::NoSpace)?;
        let mut bytes: Vec<u8> = Vec::new();
        bytes
            .try_reserve(self.carry.len().saturating_add(wanted))
            .map_err(|_| Error::NoMemory)?;
        bytes.extend_from_slice(&self.carry);
        self.carry.clear();
        let (mut input, mut out) = (buffer(256)?, buffer(PIECE + 256)?);
        let mut offset = 0usize;
        while offset < wanted {
            let at = u32::try_from(offset).map_err(|_| Error::NoSpace)?;
            let part = match sockets.data(host, id, at, &mut input, &mut out) {
                Ok(part) if !part.is_empty() => part,
                Ok(_) => return Err(Error::Invalid),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // The socket is going: its `closed` says why.
                Err(_) => return Ok(Vec::new()),
            };
            let take = part.len().min(wanted - offset);
            bytes.extend_from_slice(part.get(..take).unwrap_or(&[]));
            offset += take;
        }
        // The valid text, and the start of a character that the read cut.
        let (text_len, cut) = match core::str::from_utf8(&bytes) {
            Ok(_) => (bytes.len(), None),
            Err(error) if error.error_len().is_none() => (error.valid_up_to(), Some(())),
            Err(_) => {
                return match &mut self.session {
                    Some(session) => session.not_text(),
                    None => Ok(Vec::new()),
                };
            }
        };
        if cut.is_some() {
            self.carry
                .try_reserve(bytes.len() - text_len)
                .map_err(|_| Error::NoMemory)?;
            self.carry
                .extend_from_slice(bytes.get(text_len..).unwrap_or(&[]));
        }
        let text = bytes
            .get(..text_len)
            .and_then(|valid| core::str::from_utf8(valid).ok())
            .unwrap_or_default();
        let steps = {
            let mut store = store!(self, host);
            match &mut self.session {
                Some(session) => session.received(&mut store, text)?,
                None => Vec::new(),
            }
        };
        Ok(steps)
    }

    fn on_broadcast(&mut self, host: &Host<'_>, event: &BroadcastEvent<'_>) -> Result<(), Error> {
        if self.broadcast != Some(event.id()) {
            return Ok(());
        }
        let steps = match event.kind() {
            BroadcastKind::Answer => {
                let Some(sockets) = &self.sockets else {
                    return Ok(());
                };
                let (mut input, mut out) = (buffer(256)?, buffer(PIECE + 256)?);
                let answer = match sockets.data(host, event.id(), 0, &mut input, &mut out) {
                    Ok(bytes) => core::str::from_utf8(bytes).ok().map(copy),
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => None,
                };
                let Some(answer) = answer.transpose()? else {
                    return Ok(());
                };
                // The sender's address without its port.
                let from = event.from();
                let from = from.rsplit_once(':').map_or(from, |(address, _)| address);
                let store = store!(self, host);
                match &mut self.session {
                    Some(session) => session.answer(&store, &answer, from)?,
                    None => Vec::new(),
                }
            }
            BroadcastKind::Done => {
                self.broadcast = None;
                let no_network = matches!(
                    event.reason(),
                    Reason::NoNetwork | Reason::Offline | Reason::NetworkOff | Reason::Saving
                );
                match &mut self.session {
                    Some(session) => session.find_over(no_network)?,
                    None => Vec::new(),
                }
            }
            BroadcastKind::Failed => {
                self.broadcast = None;
                match &mut self.session {
                    Some(session) => session.find_over(true)?,
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        self.run(host, steps)
    }

    /// Asks for the password (typed masked).
    fn ask_password(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "calibre.password")?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let asked = ui.ask(
            host,
            Question::text(&title, "")?.secret()?,
            &mut input,
            &mut out,
        );
        wipe(&mut input);
        wipe(&mut out);
        self.asked = Some(asked?);
        Ok(())
    }

    /// The password typed: kept as a secret, and the connection starts again.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        if self.asked != Some(answer.request()) {
            return Ok(());
        }
        self.asked = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let read = ui.text_read(host, answer.request().text(), 0, &mut input, &mut out);
        let typed = match read {
            Ok(part) if part.data().len() as u64 == u64::from(part.total()) => {
                Secret::copy(part.data())
            }
            Ok(_) => Err(Error::NoSpace),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut out);
        let typed = typed?;
        // An empty answer keeps what was kept and changes nothing.
        if typed.as_str().is_empty() {
            return Ok(());
        }
        let label = Messages::new()?.copy(host, "calibre.password")?;
        let kept = match &self.secrets {
            Some(secrets) => keep::keep(secrets, host, keep::PASSWORD, &label, typed.as_str()),
            None => Err(Error::Unsupported),
        };
        match kept {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(Error::Unsupported) => {
                // A device that keeps no secrets says so; the password counts
                // for this connection only if it is asked for again.
                let text = Messages::new()?.copy(host, "core.secrets-unsupported")?;
                self.notify(host, &text)?;
            }
            Err(_) => {}
        }
        self.restart(host)
    }

    /// The rows and the hint for the state, drawn anew.
    #[allow(clippy::too_many_lines, reason = "one drawing of the state's rows")]
    fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(session) = &self.session else {
            return Ok(());
        };
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        let mut acts: Vec<Option<Act>> = Vec::new();
        let name = match &session.name {
            Some(name) => copy(name)?,
            None => copy("Calibre")?,
        };
        let status = match &session.state {
            State::Finding => say.copy(host, "calibre.finding")?,
            State::Found { host: address, .. } => {
                let name = session.name.as_deref().unwrap_or(address);
                say.format(
                    host,
                    "calibre.found",
                    &[
                        Arg::Text {
                            name: "name",
                            value: name,
                        },
                        Arg::Text {
                            name: "address",
                            value: address,
                        },
                    ],
                )?
            }
            State::Connecting => say.copy(host, "calibre.connecting")?,
            State::Connected => say.format(
                host,
                "calibre.connected",
                &[Arg::Text {
                    name: "name",
                    value: &name,
                }],
            )?,
            State::Receiving {
                title,
                number,
                total,
            } => say.format(
                host,
                "calibre.receiving",
                &[
                    Arg::Text {
                        name: "title",
                        value: title,
                    },
                    Arg::Int {
                        name: "number",
                        value: i64::try_from(*number).unwrap_or(i64::MAX),
                    },
                    Arg::Int {
                        name: "total",
                        value: i64::try_from(*total).unwrap_or(i64::MAX),
                    },
                ],
            )?,
            State::Password => say.copy(host, "calibre.password-needed")?,
            State::Ended(ending) => match ending {
                Ending::NoNetwork { managed: true } => say.copy(host, "calibre.no-network")?,
                Ending::NoNetwork { managed: false } => {
                    say.copy(host, "calibre.no-network.system")?
                }
                Ending::NotFound => say.copy(host, "calibre.not-found")?,
                Ending::Closed => say.copy(host, "calibre.closed")?,
                Ending::Ejected => say.copy(host, "calibre.ejected")?,
                Ending::NotCalibre => say.copy(host, "calibre.not-calibre")?,
                Ending::Failed(reason) | Ending::Error(reason) => say.format(
                    host,
                    "calibre.failed",
                    &[Arg::Text {
                        name: "reason",
                        value: reason,
                    }],
                )?,
            },
        };
        push(&mut items, Item::heading(&status)?)?;
        push(&mut acts, None)?;
        if let Some(library) = &session.library {
            let text = say.format(
                host,
                "calibre.library",
                &[Arg::Text {
                    name: "library",
                    value: library,
                }],
            )?;
            let mut row = Item::row(&text, "")?;
            if let Item::Row(row) = &mut row {
                row.disabled = true;
            }
            push(&mut items, row)?;
            push(&mut acts, None)?;
        }
        if session.received > 0 {
            let text = say.format(
                host,
                "calibre.received",
                &[Arg::Count(i64::from(session.received))],
            )?;
            let mut row = Item::row(&text, "")?;
            if let Item::Row(row) = &mut row {
                row.disabled = true;
            }
            push(&mut items, row)?;
            push(&mut acts, None)?;
        }
        let (act, label, hint) = match &session.state {
            State::Found { .. } => (
                Some(Act::Connect),
                Some("calibre.connect"),
                Some("calibre.found-hint"),
            ),
            State::Ended(_) => (
                Some(Act::Again),
                Some("calibre.try-again"),
                Some("calibre.hint"),
            ),
            State::Password => (Some(Act::Password), Some("calibre.type-password"), None),
            State::Finding => (None, None, Some("calibre.hint")),
            State::Connected => (None, None, Some("calibre.connected-hint")),
            _ => (None, None, None),
        };
        if let (Some(act), Some(label)) = (act, label) {
            let title = say.copy(host, label)?;
            push(&mut items, Item::row(&title, "›")?)?;
            push(&mut acts, Some(act))?;
        }
        let note = match hint {
            Some(key) => say.copy(host, key)?,
            None => String::new(),
        };
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let focus = view.list.focus();
        // The focus rests on the action where there is one.
        let target = acts
            .iter()
            .position(Option::is_some)
            .unwrap_or_else(|| focus.min(items.len().saturating_sub(1)));
        view.list = ItemList::new(items);
        view.list.set_focus(target);
        view.acts = acts;
        view.note = note;
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }
}
