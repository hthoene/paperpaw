//! Transfer by browser: while the screen is open, the device serves a
//! web page to the local network for putting books and other files on
//! it, fetching them and tidying up, from a phone or a computer, no
//! cable or app needed.
//!
//! The screen shows the page's address as text and as a QR code. The
//! address carries a key drawn anew each time from the platform's random
//! source, so others in the network cannot get at the files; a few wrong
//! keys, and no request is answered for a while (see `paperpaw_sdk::served`). Closing
//! the screen ends the transfer.
//!
//! The same way, the plugin lets one type text on a phone for the
//! keyboard (slot `core.remote-keyboard`, see `phone`).

#![no_std]

extern crate alloc;

mod cable;
mod phone;
mod plugins;
mod web;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

use paperpaw_qr::QrCode;
use paperpaw_sdk::builtin::{actions, events, menus, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{Confirmed, ServerNotice};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, FixedAction, FixedScreen, Gesture, GestureKind, Handled,
    Id, Manifest, MenuEntry, Params, Plugin, Priority, Rect, RefreshMode, RegisterError, Registrar,
    Screen, ServerResponse,
};

use paperpaw_sdk::served::{self, Guard};

pub const PLUGIN: Id = Id::new_static("transfer");
/// Opens the transfer screen.
pub const OPEN: Id = Id::new_static("transfer.open");
pub(crate) const SCREEN: Id = Id::new_static("transfer.screen");
const SERVER: Id = Id::new_static("transfer.server");
const SERVER_EVENT: Event<ServerNotice> = Event::new("transfer.server-event");
/// The reader's answer whether to install a plugin's code sent from the
/// computer.
const CODE_APPROVED: Event<Confirmed> = Event::new("transfer.code-approved");
/// Typing on a phone for the keyboard.
const PHONE_SCREEN: Id = Id::new_static("transfer.phone");
/// Transfer over the cable (USB).
pub(crate) const CABLE_SCREEN: Id = Id::new_static("transfer.cable");

/// The most books of a folder moved whose data is told to go along (as
/// the file manager does); the catalog is made anew past them.
const MAX_TOLD: usize = 2000;
/// The port asked for; the next free one is taken if it is in use.
pub(crate) const PORT: u16 = 8080;
/// Without a request this long, the reader's own network closes (and
/// the screen with it): it is not left open by mistake.
const OWN_IDLE_MS: u64 = 10 * 60 * 1000;

/// The web page; its texts come from the translations.
const PAGE: &str = include_str!("page.html");

/// The transfer plugin.
#[derive(Debug, Default)]
pub struct Transfer;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "transfer.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(46)];
static ICONS: [(Id, &str); 1] = [(OPEN, "transfer")];

static SCREENS: [FixedScreen; 3] = [
    FixedScreen::new(PHONE_SCREEN, "transfer.phone.title", |env, params| {
        Box::new(phone::PhoneScreen::new(env, params))
    }),
    FixedScreen::new(SCREEN, "transfer.title", |env, _| {
        Box::new(TransferScreen::new(env))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
    FixedScreen::new(CABLE_SCREEN, "transfer.cable.title", |env, _| {
        Box::new(cable::CableScreen::new(env))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
];

impl Plugin for Transfer {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Transfer", paperpaw_sdk::package_version!())
            .description(
                "Put books and other files on the device from a browser in the same network.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.bind_slot(slots::REMOTE_KEYBOARD, PHONE_SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::CABLE, CABLE_SCREEN, Priority::DEFAULT)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[derive(Debug)]
enum State {
    Starting,
    /// Serving at `url`.
    Serving {
        url: String,
        qr: Option<QrCode>,
    },
    Failed(String),
    /// Without a network: how to get one.
    NoNetwork,
    /// Offline mode: the offer to go online for the transfer.
    Offline(ui::offline::OfflineOffer),
}

/// The reader's own network, for a transfer without a router: its name
/// and password, and the code to join it.
struct OwnNetwork {
    ssid: String,
    password: String,
    qr: Option<QrCode>,
}

struct TransferScreen {
    style: UiStyle,
    header: ui::Header,
    state: State,
    /// Without a router: the reader's own network (once chosen), and when
    /// a request came last.
    own: Option<OwnNetwork>,
    active: paperpaw_sdk::Instant,
    /// The key in the address, drawn once the server runs.
    guard: Option<Guard>,
    /// The web page with its texts.
    page: String,
    /// What happened last.
    last: Option<String>,
    /// Plugins installed from the computer.
    installs: plugins::Installs,
    started: bool,
}

impl TransferScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        Self {
            header: ui::Header::new(env, &style, true),
            style,
            state: State::Starting,
            own: None,
            active: env.now(),
            guard: None,
            page: page(env),
            last: None,
            installs: plugins::Installs::default(),
            started: false,
        }
    }

    /// What the reader chose on the offer to go online: served now, or
    /// without a network as without one.
    fn offered(&mut self, chosen: Option<ui::offline::Offer>, cx: &mut Cx<'_, '_>) {
        match chosen {
            Some(ui::offline::Offer::Online) => {
                self.state = State::Starting;
                cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
            }
            // Not now, and any answer to come: on without the network.
            Some(_) => self.state = State::NoNetwork,
            None => return,
        }
        self.redraw(cx);
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// The *Without a router* button at the bottom: where PaperPaw runs
    /// Wi-Fi, before it was chosen.
    fn own_button(&self, env: &Env<'_>) -> Option<Rect> {
        if env.wifi().is_none() || self.own.is_some() || !env.random().available() {
            return None;
        }
        let area = self.content(env);
        let style = &self.style;
        let row = style.touch_row(u32::from(style.control_row())) as u16;
        let margin = style.margin();
        let width = area.width.saturating_sub(2 * margin);
        // A touch row, higher where its words take more lines.
        let label = env.i18n().tr("transfer.without-router");
        let slot = ui::button_slot_height(env, style, width, label);
        let bottom = area.y + area.height - ui::edge_room(style, row);
        Some(ui::button_in_slot(
            style,
            Rect::new(margin, bottom.saturating_sub(slot), width, slot),
        ))
    }

    /// Opens the reader's own network and serves the page there: for a
    /// transfer where there is no router (or the computer is in another
    /// network).
    fn without_router(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Letters not taken for one another: read off the screen, typed.
        let Some(password) = env.random().readable(15) else {
            return;
        };
        // Named as the device is, for the phone to find it.
        let ssid = paperpaw_sdk::device_name::ssid(&env.device_name());
        let qr = QrCode::new(paperpaw_sdk::wifi::join_code(&ssid, &password).as_bytes());
        cx.command(Command::StopServing(SERVER));
        if !env.wifi().is_some_and(|w| w.state.is_on()) {
            cx.command(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::On));
        }
        cx.command(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::Host {
            ssid: ssid.clone(),
            password: password.clone(),
        }));
        self.own = Some(OwnNetwork { ssid, password, qr });
        self.guard = None;
        self.state = State::Starting;
        self.touched(cx);
        self.redraw(cx);
    }

    /// A request came (or the own network opened): the idle time counts
    /// from now.
    fn touched(&mut self, cx: &mut Cx<'_, '_>) {
        self.active = cx.env().now();
        if self.own.is_some() {
            cx.wake_at(self.active.add_millis(OWN_IDLE_MS));
        }
    }

    /// Closes the screen, and the reader's own network if it is open.
    fn close(&mut self, cx: &mut Cx<'_, '_>) {
        if self.own.is_some() && cx.env().wifi().is_some_and(|w| w.state.is_hosting()) {
            cx.command(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::Leave));
        }
        cx.command(Command::Back);
    }

    /// The area below the header.
    fn content(&self, env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let top = self.header.area().height + self.style.gap();
        Rect::new(0, top, display.width, display.height.saturating_sub(top))
    }

    fn on_server_event(&mut self, notice: ServerNotice, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match notice {
            ServerNotice::Started { address, .. } => {
                // Drawn now: on the X4, the radio makes it fully random.
                self.state = match served::key(&env.random()) {
                    Some(key) => {
                        let url = format!("http://{address}/?k={key}");
                        self.guard = Some(Guard::new(key));
                        let qr = QrCode::new(url.as_bytes());
                        State::Serving { url, qr }
                    }
                    None => {
                        cx.command(Command::StopServing(SERVER));
                        State::Failed(no_random(&env))
                    }
                };
                self.redraw(cx);
            }
            ServerNotice::Failed { reason, .. } => {
                // Before it served, or after (e.g. the network went).
                let key = if matches!(self.state, State::Serving { .. }) {
                    "transfer.stopped"
                } else {
                    "transfer.failed"
                };
                self.state = if reason == "no network" {
                    State::NoNetwork
                } else {
                    State::Failed(env.i18n().format(key, &[("reason", Arg::Str(&reason))]))
                };
                self.redraw(cx);
            }
            ServerNotice::Request {
                request,
                method,
                target,
                body_file,
                ..
            } => {
                self.touched(cx);
                let Some(guard) = &mut self.guard else {
                    return;
                };
                let path = target.split_once('?').map_or(target.as_str(), |(p, _)| p);
                if plugins::is_plugin_request(path) {
                    return self.plugin_request(request, &method, &target, body_file, cx);
                }
                let request = web::Request {
                    number: request,
                    method: &method,
                    target: &target,
                    body_file: body_file.as_deref(),
                    at: env.now(),
                };
                let (response, done) = web::answer(env.storage(), guard, &self.page, request);
                cx.command(Command::respond(response));
                if let Some(done) = done {
                    library_follows(&env, &done, cx);
                    let (key, name) = match &done {
                        web::Done::Received(path) => {
                            ("transfer.received", paperpaw_sdk::files::name(path))
                        }
                        web::Done::Sent(name) => ("transfer.sent", name.as_str()),
                        web::Done::Folder(name) => ("transfer.folder", name.as_str()),
                        web::Done::Deleted(path) | web::Done::DeletedFolder(path) => {
                            ("transfer.deleted", paperpaw_sdk::files::name(path))
                        }
                        web::Done::Moved(_, to) => {
                            ("transfer.moved", paperpaw_sdk::files::name(to))
                        }
                        web::Done::NotReceived(name) => ("transfer.not-received", name.as_str()),
                    };
                    self.last = Some(env.i18n().format(key, &[("name", Arg::Str(name))]));
                    let area = self.content(&env);
                    cx.request_redraw_area(area);
                    cx.request_refresh(RefreshMode::Fast);
                }
            }
            _ => {}
        }
    }
}

impl TransferScreen {
    /// A request about plugins (see `plugins`), its key checked first.
    fn plugin_request(
        &mut self,
        number: u32,
        method: &str,
        target: &str,
        body_file: Option<String>,
        cx: &mut Cx<'_, '_>,
    ) {
        let env = *cx.env();
        let Some(guard) = &mut self.guard else {
            return;
        };
        let text = |status, body: &str| {
            ServerResponse::new(number, status, "text/plain; charset=utf-8", body.into())
        };
        let refused = match guard.check_target(target, env.now()) {
            served::Access::Granted => None,
            served::Access::Locked => {
                Some(text(429, "Too many wrong keys: try again in a minute."))
            }
            _ => Some(text(
                403,
                "The key is missing or wrong: open the address shown on the device.",
            )),
        };
        if let Some(response) = refused {
            if let Some(body) = body_file {
                let _ = env.storage().remove(&body);
            }
            return cx.command(Command::respond(response));
        }
        let mut out = paperpaw_sdk::Commands::new();
        let (response, ask) =
            self.installs
                .answer(&env, number, method, target, body_file.as_deref(), &mut out);
        out.push(Command::respond(response));
        for command in out {
            cx.command(command);
        }
        match ask {
            Some(plugins::Ask::Code(id)) => {
                let question = String::from(env.i18n().tr("transfer.code.question"));
                let text = env
                    .i18n()
                    .format("transfer.code.text", &[("name", Arg::Str(&id))]);
                cx.command(
                    paperpaw_sdk::ask::confirm(question, &CODE_APPROVED)
                        .text(text)
                        .yes(env.i18n().tr("transfer.code.install"))
                        .with(paperpaw_sdk::builtin::params::ID, id)
                        .command(),
                );
            }
            Some(plugins::Ask::Installed(id)) => self.installed(&env, &id, cx),
            None => {}
        }
    }

    /// Says on the device that plugin `id` was installed.
    fn installed(&mut self, env: &Env<'_>, id: &str, cx: &mut Cx<'_, '_>) {
        self.last = Some(
            env.i18n()
                .format("transfer.plugin-installed", &[("name", Arg::Str(id))]),
        );
        let area = self.content(env);
        cx.request_redraw_area(area);
        cx.request_refresh(RefreshMode::Fast);
    }
}

/// What the screen says where no key can be drawn: the platform has no
/// source of random numbers, and nothing is served.
pub(crate) fn no_random(env: &Env<'_>) -> String {
    env.i18n().tr("transfer.no-random").into()
}

/// The library follows what a request did: a book added, a book removed
/// (what plugins keep for it goes too), a folder removed (its books'
/// files only: the reader's places and notes in them stay, the catalog is
/// made anew), a book or folder moved (what is kept for its books goes
/// along).
fn library_follows(env: &Env<'_>, done: &web::Done, cx: &mut Cx<'_, '_>) {
    match done {
        web::Done::Received(path) => {
            cx.command(paperpaw_sdk::library::added(path, "transfer"));
        }
        web::Done::Deleted(path) => {
            let removed = events::BookRemoved::new(path.clone());
            cx.command(events::BOOK_REMOVED.emit(removed));
            cx.command(paperpaw_sdk::library::changed());
        }
        web::Done::DeletedFolder(_) => {
            cx.command(paperpaw_sdk::library::request_catalog_anew());
            cx.command(paperpaw_sdk::library::changed());
        }
        // Moved: what is kept for its books goes along.
        web::Done::Moved(from, to) => {
            let more = paperpaw_sdk::library::tell_moved(env, from, to, MAX_TOLD, &mut |command| {
                cx.command(command)
            });
            if more {
                cx.command(paperpaw_sdk::library::request_catalog_anew());
            }
        }
        web::Done::Folder(_) | web::Done::Sent(_) | web::Done::NotReceived(_) => {}
    }
}

/// The web page, with its texts in the device's language.
fn page(env: &Env<'_>) -> String {
    let texts = [
        ("title", "transfer.web.title"),
        ("device", "transfer.web.device"),
        ("upload", "transfer.web.upload"),
        ("drop", "transfer.web.drop"),
        ("newFolder", "transfer.web.new-folder"),
        ("folderName", "transfer.web.folder-name"),
        ("delete", "transfer.web.delete"),
        ("deleteQuestion", "transfer.web.delete-question"),
        ("empty", "transfer.web.empty"),
        ("done", "transfer.web.done"),
        ("failed", "transfer.web.failed"),
        ("target", "transfer.web.target"),
        ("outside", "transfer.web.outside"),
        ("toLibrary", "transfer.web.to-library"),
        ("here", "transfer.web.here"),
        ("move", "transfer.web.move"),
        ("rename", "transfer.web.rename"),
        ("moving", "transfer.web.moving"),
        ("moveHere", "transfer.web.move-here"),
        ("cancel", "transfer.web.cancel"),
        ("newName", "transfer.web.new-name"),
        ("waiting", "transfer.web.waiting"),
        ("sent", "transfer.web.sent"),
        ("notSent", "transfer.web.not-sent"),
        ("retry", "transfer.web.retry"),
    ];
    let mut json = String::from("{");
    for (i, (name, key)) in texts.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            "{name}:{}",
            served::json_string(env.i18n().tr(key))
        ));
    }
    // The page's title: the device, as it is called.
    json.push_str(&format!(
        ",name:{}",
        served::json_string(&env.device_name())
    ));
    // Where it opens: the library's folder.
    let library = env
        .settings()
        .read(&paperpaw_sdk::builtin::settings::LIBRARY_FOLDER);
    json.push_str(&format!(
        ",library:{}",
        served::json_string(library.trim_matches('/'))
    ));
    // What the library reads, by extension: for the hint where books are
    // sent elsewhere.
    let books: alloc::vec::Vec<String> = env
        .formats()
        .iter()
        .flat_map(|format| format.extensions.iter())
        .map(|extension| served::json_string(extension))
        .collect();
    json.push_str(&format!(",books:[{}]", books.join(",")));
    json.push('}');
    PAGE.replace("/*STRINGS*/{}", &json)
}

/// The code to join the reader's own network in `area`, its name and
/// password below it, to type where the code cannot be scanned.
fn draw_join(ui: &mut ui::Ui<'_>, area: Rect, own: &OwnNetwork) {
    let (env, style) = (ui.env(), ui.style());
    let Some(font) = ui.font() else {
        return;
    };
    let margin = style.margin();
    let width = area.width.saturating_sub(2 * margin);
    let line = font.line_metrics(style.text()).line_height as u16;
    let small = font.line_metrics(style.small()).line_height as u16;
    let title = env.i18n().tr("transfer.own.join");
    ui::Label::new(title)
        .px(style.text())
        .color(Color::BLACK)
        .draw_from(
            ui,
            f32::from(margin),
            Rect::new(margin, area.y, width, line),
        );
    let note = env.i18n().format(
        "transfer.own.network",
        &[
            ("ssid", Arg::Str(&own.ssid)),
            (
                "password",
                Arg::Str(&paperpaw_sdk::random::grouped(&own.password)),
            ),
        ],
    );
    let bottom = area.y + area.height;
    ui::Label::new(&note)
        .px(style.small())
        .color(style.faint(ui::Ink::NOTE))
        .draw_from(
            ui,
            f32::from(margin),
            Rect::new(margin, bottom.saturating_sub(small), width, small),
        );
    if let Some(qr) = &own.qr {
        let room = Rect::new(
            margin,
            area.y + line,
            width,
            area.height.saturating_sub(line + small),
        );
        ui::draw_qr(ui.canvas(), qr, room);
    }
}

impl Screen for TransferScreen {
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        // Back, and Select for the transfer without a router.
        if *action == actions::CONFIRM && self.own_button(env).is_some() {
            return paperpaw_sdk::Hint::Text(env.i18n().tr("transfer.without-router").into());
        }
        paperpaw_sdk::Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let title = env.i18n().tr("transfer.title");
        self.header.draw(&mut ui, title, None, None);
        let area = self.content(env);
        let own_button = self.own_button(env);
        let (url, qr) = match &mut self.state {
            State::Starting => {
                // While it waits for a network: or without a router.
                let button = self.own_button(env);
                let room = match button {
                    Some(b) => Rect::new(area.x, area.y, area.width, b.y.saturating_sub(area.y)),
                    None => area,
                };
                ui.message(room, env.i18n().tr("transfer.starting"));
                if let Some(button) = button {
                    let label = env.i18n().tr("transfer.without-router");
                    let focused = ui::focus_shown(env);
                    ui.button(button, label, false, focused);
                }
                return;
            }
            State::Failed(message) => {
                return ui.message(area, message);
            }
            State::NoNetwork => {
                // Where PaperPaw runs Wi-Fi, its settings join a network;
                // elsewhere the system's.
                let text = if env.wifi().is_some() {
                    "transfer.no-network"
                } else {
                    "transfer.no-network.system"
                };
                // Or without a router, through the reader's own network.
                let button = self.own_button(env);
                let room = match button {
                    Some(b) => Rect::new(area.x, area.y, area.width, b.y.saturating_sub(area.y)),
                    None => area,
                };
                ui.empty(
                    room,
                    "wifi-off",
                    env.i18n().tr("transfer.no-network.title"),
                    env.i18n().tr(text),
                );
                if let Some(button) = button {
                    let label = env.i18n().tr("transfer.without-router");
                    let focused = ui::focus_shown(env);
                    ui.button(button, label, false, focused);
                }
                return;
            }
            State::Offline(offer) => {
                return offer.draw(&mut ui, area);
            }
            State::Serving { url, qr } => (url, qr),
        };
        // Without a router: first the code to join the reader's network.
        let area = match &self.own {
            Some(own) => {
                let half = area.height / 2;
                draw_join(&mut ui, Rect::new(area.x, area.y, area.width, half), own);
                Rect::new(area.x, area.y + half, area.width, area.height - half)
            }
            None => area,
        };
        let button = own_button;
        let area = match button {
            Some(button) => Rect::new(
                area.x,
                area.y,
                area.width,
                button.y.saturating_sub(area.y + style.gap()),
            ),
            None => area,
        };
        let hint = env.i18n().tr("transfer.hint");
        ui::draw_address(
            ui.canvas(),
            env,
            &style,
            area,
            url,
            qr.as_ref(),
            hint,
            self.last.as_deref(),
        );
        if let Some(button) = button {
            let label = env.i18n().tr("transfer.without-router");
            let focused = ui::focus_shown(env);
            ui.button(button, label, false, focused);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        // The own network left alone: closed.
        if self.own.is_some() {
            if cx.env().now() >= self.active.add_millis(OWN_IDLE_MS) {
                self.close(cx);
            } else {
                cx.wake_at(self.active.add_millis(OWN_IDLE_MS));
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.started {
            self.started = true;
            if !cx.env().random().available() {
                self.state = State::Failed(no_random(cx.env()));
                self.redraw(cx);
                return;
            }
            let offer = ui::offline::OfflineOffer::new();
            if offer.shows(cx.env()) {
                self.state = State::Offline(offer);
                self.redraw(cx);
                return;
            }
            cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            // Closing the screen also ends the transfer.
            self.close(cx);
            Handled::Yes
        } else if *action == actions::CONFIRM && self.own_button(cx.env()).is_some() {
            self.without_router(cx);
            Handled::Yes
        } else if let State::Offline(offer) = &mut self.state {
            let chosen = offer.on_action(action, cx).done();
            self.offered(chosen, cx);
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None)
        {
            if action == actions::BACK {
                self.close(cx);
            } else {
                cx.command(Command::run(action));
            }
            return Handled::Yes;
        }
        if gesture.kind == GestureKind::Tap
            && self
                .own_button(cx.env())
                .is_some_and(|b| b.contains(gesture.start))
        {
            self.without_router(cx);
            return Handled::Yes;
        }
        if gesture.kind == GestureKind::Tap
            && let State::Offline(offer) = &mut self.state
        {
            let chosen = offer.on_gesture(&gesture, cx).done();
            self.offered(chosen, cx);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(notice) = SERVER_EVENT.read(event, params)
            && *notice.server() == SERVER
        {
            self.on_server_event(notice, cx);
        } else if let Some(answer) = CODE_APPROVED.read(event, params) {
            let env = *cx.env();
            let Some(id) = answer.context.get(paperpaw_sdk::builtin::params::ID) else {
                return;
            };
            let id = String::from(id);
            let mut out = paperpaw_sdk::Commands::new();
            let installed = self.installs.approved(&env, &id, answer.yes, &mut out);
            for command in out {
                cx.command(command);
            }
            if installed {
                self.installed(&env, &id, cx);
            }
        } else if *event == events::WIFI_CHANGED
            && self.own.is_some()
            && matches!(self.state, State::Starting)
            && cx.env().wifi().is_some_and(|w| w.state.is_hosting())
        {
            // The own network open: the page served there.
            cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
        }
    }
}

#[cfg(test)]
mod tests;
