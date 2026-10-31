//! Calibre's wireless device connection: while the screen is open, the
//! device finds Calibre in the local network (or connects to the address
//! set), and Calibre sends books to it, lists them and deletes them, as
//! with a reader on a cable. Books go into the library folder, where
//! Calibre places them (`Author/Title.epub`).
//!
//! In Calibre: *Connect/share*, *Start wireless device connection*. A
//! password set there is asked for once and kept with the other passwords.
//!
//! Calibre adds and deletes books here, so the device connects only to a
//! computer the reader chose: one found in the network is shown with its
//! address and trusted once the reader connects to it (the address set
//! by hand is trusted as it is). Calibre writes only books of formats
//! PaperPaw reads, below the library folder, replaces and deletes only
//! books it sent itself, and never touches the reader's own files.
//!
//! The protocol is in `protocol`, the books Calibre knows in `library`.
//! The connection is a socket (ADR 0014); books come straight into
//! storage, so their size does not matter.

#![no_std]

extern crate alloc;

mod library;
pub mod protocol;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, groups, menus};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::json;
use paperpaw_sdk::replies::{SocketNotice, Typed};
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, FixedAction, FixedScreen, Gesture, GestureKind, Handled,
    Hint, Id, Manifest, MenuEntry, Params, Plugin, Rect, RefreshMode, RegisterError, Registrar,
    SDK_VERSION, Screen, SettingSpec, SocketRead, Storage,
};

use paperpaw_sdk::setting::{Setting, Text};
use protocol::{Message, op};

pub const PLUGIN: Id = Id::new_static("calibre");
/// Opens the connection screen.
pub const OPEN: Id = Id::new_static("calibre.open");
const SCREEN: Id = Id::new_static("calibre.screen");
const SOCKET: Id = Id::new_static("calibre.socket");
const FIND: Id = Id::new_static("calibre.find");
const EVENTS: Event<SocketNotice> = Event::new("calibre.events");
const PASSWORD_TYPED: Event<Typed> = Event::new("calibre.password-typed");
/// Calibre's address, if it is not found by itself: `host` or `host:port`.
pub const ADDRESS: Setting<Text> = Setting::new("calibre.address");
/// The password set in Calibre, kept as a secret.
const PASSWORD: &str = "calibre.password";
/// How long to wait for Calibre's answer to a broadcast.
const FIND_MS: u32 = 1500;
/// Broadcasts sent before Calibre counts as not found: a datagram may be
/// lost on the way, and nothing says so.
const FIND_TRIES: u8 = 4;
/// Space reported to Calibre where the storage cannot tell (yet): Calibre
/// refuses to send to a device it thinks full.
const SPACE: u64 = 64 << 30;
/// Bytes of the book list sent to Calibre in one turn, about (see
/// [`Listing`]).
const LIST_PART: usize = 16 * 1024;

/// The space told to Calibre, free and in all: what the storage says,
/// [`SPACE`] where it cannot tell (the size never less than what is free).
fn space(storage: &dyn Storage) -> (u64, u64) {
    let free = storage.free_space().unwrap_or(SPACE);
    let total = storage.total_space().unwrap_or(SPACE).max(free);
    (free, total)
}

/// What Calibre's metadata of a book says of it, for the library.
fn provided(metadata: &str) -> paperpaw_sdk::library::Provided {
    let text = |key: &str| json::raw_member(metadata, key).and_then(json::unquote);
    let list = |key: &str| {
        json::raw_member(metadata, key)
            .map(json::string_array)
            .unwrap_or_default()
    };
    paperpaw_sdk::library::Provided::default()
        .title(text("title"))
        .author(list("authors").into_iter().next())
        .series(text("series"))
        .series_index(
            json::raw_member(metadata, "series_index")
                .filter(|n| *n != "null")
                .map(|n| n.trim_end_matches(".0").into()),
        )
        .tags(list("tags"))
}
/// The Calibre plugin.
#[derive(Debug, Default)]
pub struct Calibre;

/// In the library's settings, in a part named after it.
static IN_CALIBRE: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().section("calibre.plugin.name");

/// Calibre's address, where it is not found by itself.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::text(
    ADDRESS.static_str(),
    "calibre.setting.address",
    "",
    groups::LIBRARY,
)
.with_extra(&IN_CALIBRE)
.description("calibre.setting.address.help")
.advanced()];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "calibre.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(47)];
static ICONS: [(Id, &str); 1] = [(OPEN, "device-desktop")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(SCREEN, "calibre.title", |env, _| {
    Box::new(CalibreScreen::new(env))
})
.instances(paperpaw_sdk::Instances::SINGLE)];

impl Plugin for Calibre {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Calibre connection",
            paperpaw_sdk::package_version!(),
        )
        .description("Receive books from Calibre over the network, as its wireless device.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.add_settings(&SETTINGS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Where the connection is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
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
    /// It ended: why (a translated text).
    Ended(String),
    /// Offline mode: the offer to go online shows.
    Offline,
}

/// The books listed to Calibre (`GET_BOOK_COUNT`): a part each turn
/// ([`LIST_PART`]), read from the card as it goes, so a long list never
/// waits whole in memory.
#[derive(Debug)]
struct Listing {
    books: Vec<u128>,
    next: usize,
}

/// A book coming: written to `part`, then moved to `path`.
#[derive(Debug)]
struct Coming {
    lpath: String,
    path: String,
    part: String,
    metadata: String,
}

struct CalibreScreen {
    style: UiStyle,
    header: ui::Header,
    state: State,
    framing: protocol::Framing,
    coming: Option<Coming>,
    listing: Option<Listing>,
    /// Calibre's name and library, once known.
    name: Option<String>,
    library: Option<String>,
    received: u32,
    /// Broadcasts sent in this search.
    tries: u8,
    started: bool,
    /// The *Type the password* row, where it shows.
    password_row: Option<Rect>,
    /// Offline mode: the offer to go online.
    offer: ui::offline::OfflineOffer,
}

impl CalibreScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        Self {
            header: ui::Header::new(env, &style, true),
            style,
            state: State::Finding,
            framing: framing(env),
            coming: None,
            listing: None,
            name: None,
            library: None,
            received: 0,
            tries: 0,
            started: false,
            password_row: None,
            offer: ui::offline::OfflineOffer::new(),
        }
    }

    /// What the reader chose on the offer to go online: Calibre found
    /// now, or said to need the network.
    fn offered(&mut self, chosen: Option<ui::offline::Offer>, cx: &mut Cx<'_, '_>) {
        match chosen {
            Some(ui::offline::Offer::Online) => self.start(cx),
            // Not now, and any answer to come: on without the network.
            Some(_) => {
                let env = *cx.env();
                let key = if env.wifi().is_some() {
                    "calibre.no-network"
                } else {
                    "calibre.no-network.system"
                };
                self.state = State::Ended(env.i18n().tr(key).to_string());
                self.redraw(cx);
            }
            None => {}
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Finds Calibre, or connects to the address set.
    fn start(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.framing = framing(&env);
        self.coming = None;
        self.listing = None;
        let address = env.settings().read(&ADDRESS).trim();
        if address.is_empty() {
            self.state = State::Finding;
            self.tries = 0;
            self.find(cx);
        } else {
            let (host, port) = match address.rsplit_once(':') {
                Some((host, port)) if port.parse::<u16>().is_ok() => {
                    (host, port.parse().unwrap_or(protocol::DEFAULT_PORT))
                }
                _ => (address, protocol::DEFAULT_PORT),
            };
            self.connect(host, port, cx);
        }
        self.redraw(cx);
    }

    /// Asks for Calibre in the local network: a broadcast to its ports.
    fn find(&mut self, cx: &mut Cx<'_, '_>) {
        self.tries += 1;
        cx.command(Command::broadcast(
            FIND,
            protocol::BROADCAST_PORTS.to_vec(),
            b"hello".to_vec(),
            FIND_MS,
            EVENTS.into(),
        ));
    }

    fn connect(&mut self, host: &str, port: u16, cx: &mut Cx<'_, '_>) {
        self.state = State::Connecting;
        cx.command(Command::connect(
            SOCKET,
            host.to_string(),
            port,
            EVENTS.into(),
        ));
    }

    fn end(&mut self, text: String, cx: &mut Cx<'_, '_>) {
        self.state = State::Ended(text);
        self.coming = None;
        self.listing = None;
        self.redraw(cx);
    }

    /// Connects to the computer found, trusted from now on.
    fn connect_found(&mut self, cx: &mut Cx<'_, '_>) {
        if let State::Found { host, port } = &self.state {
            let (host, port) = (host.clone(), *port);
            library::trust(cx.env().storage(), &host);
            self.connect(&host, port, cx);
            self.redraw(cx);
        }
    }

    fn send(&self, op: u8, args: &str, cx: &mut Cx<'_, '_>) {
        cx.command(Command::socket_send(SOCKET, protocol::frame(op, args)));
    }

    /// Asks for what comes next of a message.
    fn read_on(&self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::socket_read(
            SOCKET,
            SocketRead::Text {
                max: self.framing.wanted(),
            },
        ));
    }

    /// Calibre closed the connection: as it does after ejecting (said so
    /// already), or else said now.
    fn closed_by_calibre(&mut self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::Disconnect(SOCKET));
        if !matches!(self.state, State::Ended(_)) {
            let ended = cx.env().i18n().tr("calibre.closed").to_string();
            self.end(ended, cx);
        }
    }

    fn on_socket(&mut self, notice: SocketNotice, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match notice {
            SocketNotice::Answer { text, from, .. }
                if matches!(self.state, State::Finding | State::Found { .. }) =>
            {
                let Some(port) = protocol::port_of_answer(&text) else {
                    return;
                };
                let trusted = library::trusted(env.storage()).as_deref() == Some(from.as_str());
                if trusted || self.state == State::Finding {
                    self.name = protocol::name_of_answer(&text).map(String::from);
                }
                if trusted {
                    self.connect(&from, port, cx);
                } else if self.state == State::Finding {
                    // Another computer may pose as Calibre: the reader
                    // says whether it is theirs.
                    self.state = State::Found { host: from, port };
                }
                self.redraw(cx);
            }
            SocketNotice::Done { reason, .. }
                if self.state == State::Finding && reason.is_empty() && self.tries < FIND_TRIES =>
            {
                // Not heard (or not answered) yet: asked again.
                self.find(cx);
            }
            SocketNotice::Done { reason, .. } if self.state == State::Finding => {
                let text = if reason == "no network" && env.wifi().is_some() {
                    env.i18n().tr("calibre.no-network")
                } else if reason == "no network" {
                    env.i18n().tr("calibre.no-network.system")
                } else {
                    env.i18n().tr("calibre.not-found")
                };
                self.end(text.to_string(), cx);
            }
            SocketNotice::Connected { .. } => {
                self.state = State::Connected;
                self.read_on(cx);
                self.redraw(cx);
            }
            SocketNotice::Received { text, .. } => {
                if text.is_empty() {
                    return self.closed_by_calibre(cx);
                }
                match self.framing.feed(&text) {
                    Ok(Some(message)) => self.handle(message, cx),
                    Ok(None) => self.read_on(cx),
                    Err(_) => {
                        cx.command(Command::Disconnect(SOCKET));
                        let ended = env.i18n().tr("calibre.not-calibre").to_string();
                        self.end(ended, cx);
                    }
                }
            }
            SocketNotice::Saved { path, .. } => {
                match self.coming.take() {
                    Some(coming) => {
                        // The book there stays until the new one is in place.
                        let placed = paperpaw_sdk::files::put_in_place(
                            env.storage(),
                            &coming.part,
                            &coming.path,
                        );
                        if placed.is_ok() {
                            library::remember(env.storage(), &coming.lpath, &coming.metadata);
                            paperpaw_sdk::library::provide(
                                env.storage(),
                                &coming.path,
                                provided(&coming.metadata),
                            );
                            self.received += 1;
                            cx.command(paperpaw_sdk::library::added(&coming.path, "calibre"));
                        }
                    }
                    // A book dropped (its path led out of the library).
                    None => {
                        let _ = env.storage().remove(&path);
                    }
                }
                self.state = State::Connected;
                self.read_on(cx);
                self.redraw(cx);
            }
            SocketNotice::Closed { socket, reason } | SocketNotice::Failed { socket, reason }
                if socket == SOCKET =>
            {
                if let Some(coming) = self.coming.take() {
                    let _ = env.storage().remove(&coming.part);
                }
                self.listing = None;
                if !matches!(self.state, State::Ended(_) | State::Password) {
                    let text = if reason.is_empty() {
                        env.i18n().tr("calibre.closed").to_string()
                    } else {
                        env.i18n()
                            .format("calibre.failed", &[("reason", Arg::Str(&reason))])
                    };
                    self.end(text, cx);
                }
            }
            _ => {}
        }
    }

    /// Answers a message of Calibre's, then reads on (unless a book or
    /// the end comes).
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn handle(&mut self, message: Message, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let args = message.args.as_str();
        match message.op {
            op::GET_INITIALIZATION_INFO => {
                let challenge = protocol::text(args, "passwordChallenge").unwrap_or_default();
                let password = env.secrets().get(PASSWORD).map(String::from);
                let extensions: Vec<&str> = env
                    .formats()
                    .iter()
                    .flat_map(|f| f.extensions.iter().map(|e| e.as_ref()))
                    .collect();
                let version = format!("{}", SDK_VERSION);
                let name = env.device_name();
                let device = protocol::Device {
                    name: &name,
                    extensions: &extensions,
                    challenge: &challenge,
                    password: password.as_deref(),
                    version: &version,
                };
                self.send(op::OK, &protocol::initialization(&device), cx);
            }
            op::SET_CALIBRE_DEVICE_INFO => {
                library::set_device_info(env.storage(), args);
                self.send(op::OK, "{}", cx);
            }
            op::SET_CALIBRE_DEVICE_NAME | op::SET_LIBRARY_INFO => {
                if let Some(name) = protocol::text(args, "libraryName") {
                    self.library = Some(name);
                    self.redraw(cx);
                }
                self.send(op::OK, "{}", cx);
            }
            op::GET_DEVICE_INFORMATION => {
                let stored = library::device_info(env.storage());
                let version = format!("{}", SDK_VERSION);
                let name = env.device_name();
                let info = protocol::device_information(stored.as_deref(), &name, &version);
                self.send(op::OK, &info, cx);
            }
            op::TOTAL_SPACE => {
                let (_, total) = space(env.storage());
                let reply = format!("{{\"total_space_on_device\": {total}}}");
                self.send(op::OK, &reply, cx);
            }
            op::FREE_SPACE => {
                let (free, _) = space(env.storage());
                let reply = format!("{{\"free_space_on_device\": {free}}}");
                self.send(op::OK, &reply, cx);
            }
            op::GET_BOOK_COUNT => {
                let books = library::books(env.storage(), library_folder(&env));
                let count = format!(
                    "{{\"count\": {}, \"willStream\": true, \"willScan\": true}}",
                    books.len()
                );
                self.send(op::OK, &count, cx);
                if !books.is_empty() {
                    // Their metadata in parts, then reading on.
                    self.listing = Some(Listing { books, next: 0 });
                    cx.wake_at(env.now());
                    return;
                }
            }
            op::SEND_BOOK => return self.book(args, cx),
            op::DELETE_BOOK => {
                self.send(op::OK, "{}", cx);
                let mut removed = false;
                for lpath in protocol::strings(args, "lpaths") {
                    // Only books Calibre sent: never the reader's own.
                    if library::sent(env.storage(), &lpath)
                        && let Some(path) = protocol::safe_path(&lpath)
                    {
                        let path = paperpaw_sdk::files::join(library_folder(&env), &path);
                        // Only the file: the reader's place and notes in it
                        // stay, and are there again when Calibre sends it
                        // again.
                        removed |= env.storage().remove(&path).is_ok();
                    }
                    let uuid = library::forget(env.storage(), &lpath).unwrap_or_default();
                    let reply = format!("{{\"uuid\": {}}}", paperpaw_sdk::json::string(&uuid));
                    self.send(op::OK, &reply, cx);
                }
                if removed {
                    // Gone without a word: the catalog is made anew.
                    cx.command(paperpaw_sdk::library::request_catalog_anew());
                    cx.command(paperpaw_sdk::library::changed());
                }
            }
            op::NOOP => {
                if protocol::member(args, "ejecting").is_some() {
                    // Calibre closes once it has the answer: closed here
                    // first, the answer could be lost on the way.
                    self.send(op::OK, "{}", cx);
                    self.read_on(cx);
                    let ended = env.i18n().tr("calibre.ejected").to_string();
                    return self.end(ended, cx);
                }
                // Without cached metadata Calibre counts nothing; a plain
                // NOOP keeps the connection alive.
                if protocol::member(args, "count").is_none()
                    && protocol::member(args, "priKey").is_none()
                {
                    self.send(op::OK, "{}", cx);
                }
            }
            op::DISPLAY_MESSAGE => {
                // Kind 1: the password is wrong or missing.
                if protocol::number(args, "messageKind") == Some(1) {
                    cx.command(Command::Disconnect(SOCKET));
                    self.state = State::Password;
                    self.redraw(cx);
                    return;
                }
            }
            op::GET_BOOK_FILE_SEGMENT | op::GET_BOOK_METADATA => {
                let error = "{\"message\": \"PaperPaw does not send books\"}";
                self.send(op::ERROR, error, cx);
            }
            op::ERROR => {
                let message = protocol::text(args, "message").unwrap_or_default();
                let text = env
                    .i18n()
                    .format("calibre.failed", &[("reason", Arg::Str(&message))]);
                cx.command(Command::Disconnect(SOCKET));
                return self.end(text, cx);
            }
            // Book lists and their metadata, busy, done: nothing to say.
            _ => {}
        }
        self.read_on(cx);
    }

    /// A book is about to come: its bytes go to a part file first.
    fn book(&mut self, args: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Without its length, the bytes cannot be told from what follows,
        // and an empty book would replace the one sent before.
        let Some(length) = protocol::number(args, "length") else {
            cx.command(Command::Disconnect(SOCKET));
            let ended = env.i18n().tr("calibre.not-calibre").to_string();
            return self.end(ended, cx);
        };
        let lpath = protocol::text(args, "lpath").unwrap_or_default();
        let metadata = protocol::member(args, "metadata")
            .unwrap_or("{}")
            .to_string();
        let title = protocol::text(&metadata, "title").unwrap_or_else(|| lpath.clone());
        // Not a book PaperPaw reads, a path out of the library, or a file
        // there that Calibre did not send: the bytes are taken, then
        // dropped.
        let extensions: Vec<&str> = env
            .formats()
            .iter()
            .flat_map(|f| f.extensions.iter().map(|e| e.as_ref()))
            .collect();
        let target = protocol::book_path(&lpath, &extensions)
            .map(|safe| paperpaw_sdk::files::join(library_folder(&env), &safe));
        let target = match target {
            Some(path) if env.storage().exists(&path) && !library::sent(env.storage(), &lpath) => {
                let text = env
                    .i18n()
                    .format("calibre.not-replaced", &[("title", Arg::Str(&title))]);
                cx.command(Command::Notify(text));
                None
            }
            target => target,
        };
        let (path, part) = match target {
            Some(path) => {
                if let Some(dir) = path.rsplit_once('/').map(|(d, _)| d) {
                    let _ = env.storage().create_dir(dir);
                }
                (path.clone(), format!("{path}.part"))
            }
            None => (
                String::new(),
                format!("{}/dropped.part", Env::plugin_dir(PLUGIN.as_str())),
            ),
        };
        self.state = State::Receiving {
            title,
            number: protocol::number(args, "thisBook").unwrap_or(0) + 1,
            total: protocol::number(args, "totalBooks").unwrap_or(1),
        };
        self.coming = (!path.is_empty()).then(|| Coming {
            lpath,
            path,
            part: part.clone(),
            metadata,
        });
        cx.command(Command::socket_read(
            SOCKET,
            SocketRead::File {
                path: part,
                len: length,
            },
        ));
        self.redraw(cx);
    }

    fn type_password(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        cx.command(
            paperpaw_sdk::ask::text(env.i18n().tr("calibre.password"), &PASSWORD_TYPED)
                .password()
                .command(),
        );
    }

    /// The area below the header.
    /// The size of the hint below `status`: the small one, or smaller
    /// where the screen holds it whole so (the largest text on a small
    /// screen).
    fn hint_size(&self, env: &Env<'_>, status: &str, hint: &str, again: Option<&str>) -> f32 {
        let style = &self.style;
        let small = style.small();
        let Some(font) = style.font(env) else {
            return small;
        };
        let area = self.content(env);
        let width = f32::from(area.width) - 2.0 * style.margin() as f32;
        let high = |size: f32, text: &str| {
            font.line_metrics(size).line_height * font.wrap(size, text, width).len() as f32
                + f32::from(style.gap() / 2)
        };
        let mut above = f32::from(style.gap()) + high(style.title(), status);
        if let Some(library) = &self.library {
            let text = env
                .i18n()
                .format("calibre.library", &[("library", Arg::Str(library))]);
            above += high(style.text(), &text);
        }
        if self.received > 0 {
            let text = env
                .i18n()
                .plural("calibre.received", i64::from(self.received), &[]);
            above += high(style.text(), &text);
        }
        if let Some(again) = again {
            above += high(small, env.i18n().tr(again));
        }
        let room = f32::from(area.height) - above;
        paperpaw_sdk::widgets::largest_size(small, small * 0.6, |size| high(size, hint) <= room)
            .unwrap_or(small * 0.6)
    }

    fn content(&self, env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let top = self.header.area().height + self.style.gap();
        Rect::new(0, top, display.width, display.height.saturating_sub(top))
    }
}

/// The framing of Calibre's messages, for the device's memory.
fn framing(env: &Env<'_>) -> protocol::Framing {
    protocol::Framing::new(protocol::max_message(env.device().memory))
}

/// The library folder, where Calibre's books go.
fn library_folder<'e>(env: &Env<'e>) -> &'e str {
    paperpaw_sdk::library::folder(env)
}

impl Screen for CalibreScreen {
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, env.i18n().tr("calibre.title"), None, None);
        if self.state == State::Offline {
            let area = self.content(env);
            return self.offer.draw(&mut ui, area);
        }
        let Some(font) = style.font(env) else {
            return;
        };
        let area = self.content(env);
        let margin = style.margin() as f32;
        let width = f32::from(area.width) - 2.0 * margin;
        let mut y = area.y + style.gap();
        let mut line = |ui: &mut ui::Ui<'_>, size: f32, text: &str, color: Color| {
            let height = font.line_metrics(size).line_height as u16;
            for range in font.wrap(size, text, width) {
                let row = Rect::new(area.x, y, area.width, height);
                ui::Label::new(&text[range])
                    .px(size)
                    .color(color)
                    .draw_from(ui, margin, row);
                y += height;
            }
            y += style.gap() / 2;
        };
        let name = self.name.clone().unwrap_or_else(|| String::from("Calibre"));
        let status = match &self.state {
            State::Finding => env.i18n().tr("calibre.finding").to_string(),
            State::Found { host, .. } => {
                let name = self.name.as_deref().unwrap_or(host);
                env.i18n().format(
                    "calibre.found",
                    &[("name", Arg::Str(name)), ("address", Arg::Str(host))],
                )
            }
            State::Connecting => env.i18n().tr("calibre.connecting").to_string(),
            State::Connected => env
                .i18n()
                .format("calibre.connected", &[("name", Arg::Str(&name))]),
            State::Receiving {
                title,
                number,
                total,
            } => env.i18n().format(
                "calibre.receiving",
                &[
                    ("title", Arg::Str(title)),
                    ("number", Arg::Int(*number as i64)),
                    ("total", Arg::Int(*total as i64)),
                ],
            ),
            State::Password => env.i18n().tr("calibre.password-needed").to_string(),
            State::Ended(text) => text.clone(),
            State::Offline => String::new(),
        };
        line(&mut ui, style.title(), &status, Color::BLACK);
        if let Some(library) = &self.library {
            let text = env
                .i18n()
                .format("calibre.library", &[("library", Arg::Str(library))]);
            line(&mut ui, style.text(), &text, style.faint(ui::Ink::DETAIL));
        }
        if self.received > 0 {
            let text = env
                .i18n()
                .plural("calibre.received", i64::from(self.received), &[]);
            line(&mut ui, style.text(), &text, Color::BLACK);
        }
        let hint = match self.state {
            State::Finding | State::Ended(_) => Some("calibre.hint"),
            State::Found { .. } => Some("calibre.found-hint"),
            State::Connected => Some("calibre.connected-hint"),
            _ => None,
        };
        // How to connect or try again: by touch, or the button (named in
        // the row of hints under the screen, where there is one).
        let keys = match self.state {
            State::Found { .. } => Some(("calibre.connect.touch", "calibre.connect.buttons")),
            State::Ended(_) => Some(("calibre.again.touch", "calibre.again.buttons")),
            _ => None,
        };
        let again = keys.and_then(|(touch, buttons)| {
            if env.device().input.touch {
                Some(touch)
            } else if !env.caps().button_row {
                Some(buttons)
            } else {
                None
            }
        });
        if let Some(hint) = hint {
            let hint = env.i18n().tr(hint);
            let size = self.hint_size(env, &status, hint, again);
            line(&mut ui, size, hint, style.faint(ui::Ink::DETAIL));
        }
        if let Some(again) = again {
            line(&mut ui, style.small(), env.i18n().tr(again), Color::BLACK);
        }
        self.password_row = None;
        if self.state == State::Password {
            let row = Rect::new(area.x, y, area.width, style.row());
            ui::Row::value(env.i18n().tr("calibre.type-password"), "›").draw(&mut ui, row);
            self.password_row = Some(row);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.started {
            self.started = true;
            // Offline mode: asked first, once.
            if self.offer.shows(cx.env()) {
                self.state = State::Offline;
                return self.redraw(cx);
            }
            self.start(cx);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let Some(mut listing) = self.listing.take() else {
            return;
        };
        let env = *cx.env();
        let mut sent = 0;
        while sent < LIST_PART
            && let Some(book) = listing.books.get(listing.next)
        {
            listing.next += 1;
            let metadata = library::metadata(env.storage(), *book);
            let metadata = metadata.as_deref().unwrap_or("{}");
            sent += metadata.len();
            self.send(op::OK, metadata, cx);
        }
        if listing.next < listing.books.len() {
            self.listing = Some(listing);
            cx.wake_at(env.now());
        } else {
            self.read_on(cx);
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(notice) = EVENTS.read(event, params) {
            self.on_socket(notice, cx);
        } else if let Some(typed) = PASSWORD_TYPED.read(event, params)
            && !typed.text.is_empty()
        {
            cx.command(Command::set_secret(SecretChange::keep(
                PASSWORD,
                "calibre.password",
                &typed.text,
            )));
            self.start(cx);
        }
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        if *action != actions::CONFIRM {
            return Hint::Default;
        }
        match self.state {
            State::Password => Hint::Text(env.i18n().tr("calibre.type-password").to_string()),
            State::Found { .. } => Hint::Text(env.i18n().tr("calibre.connect").to_string()),
            State::Ended(_) => Hint::Text(env.i18n().tr("calibre.try-again").to_string()),
            _ => Hint::Hidden,
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if self.state == State::Offline && *action != actions::BACK {
            let chosen = self.offer.on_action(action, cx).done();
            self.offered(chosen, cx);
            return Handled::Yes;
        }
        if *action == actions::CONFIRM {
            match self.state {
                State::Password => self.type_password(cx),
                State::Found { .. } => self.connect_found(cx),
                // Again, after it ended.
                State::Ended(_) => self.start(cx),
                _ => {}
            }
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if self.state == State::Offline {
            let chosen = self.offer.on_gesture(&gesture, cx).done();
            self.offered(chosen, cx);
            return Handled::Yes;
        }
        if self.password_row.is_some_and(|r| r.contains(gesture.start)) {
            self.type_password(cx);
            return Handled::Yes;
        }
        match self.state {
            State::Found { .. } => self.connect_found(cx),
            State::Ended(_) => self.start(cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }
}

#[cfg(test)]
mod tests;
