//! The interface-7 transfer screen: while it is open the device serves the web
//! page to the local network (`core.net` `serve`), shows the address as text and
//! as a QR code, and answers the page's requests (list, download, upload, make a
//! folder, move, rename, delete: `native_web`). The address carries a key drawn anew for each
//! time of serving from the device's random source (`core.secrets`); wrong keys
//! lock the page for a while. Closing the screen ends the serving. What is not
//! native yet (the cable) is named in `PORTING-7.md`.
use crate::{
    declarations::{
        INSTALL_DONE, OPEN_TOKEN, PHONE, SCREEN, SERVED_REQUEST, SERVER_EVENT, WIFI_DONE, WORK,
    },
    native_draw::{Join, draw_address},
    native_files::HostCard,
    native_install::{self, Installs},
    native_install_host::{self, HostPlugins},
    native_key, native_page,
    native_phone::{self, Dialog, MAX_TEXT},
    native_transfer::urls,
    native_web::{self, Answer, Body, Done, Parts, Request},
};
use alloc::string::String;
use paperpaw_qr::QrCode;
use paperpaw_sdk::{
    Handled, Instant,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        device::{Device, DeviceOutcome, InstallKind, WifiState},
        events::{Event, Incoming, InstallDone, kinds},
        files::Files,
        formatting::Arg,
        library::Library,
        net::{
            FileRoot, MAX_SERVED_BODY, Net, REQUEST_INPUT, Reason, Response, ServedRequest, Server,
            ServerState,
        },
        registrations,
        registry::Registry,
        reply::Reply,
        secrets::{Secrets, wipe},
        storage::{self, Root},
        ui::{Open, ScreenPolicy, Ui},
        work::Work,
    },
    served::Guard,
};
use paperpaw_ui_kit::{
    Footer, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, wipe_text},
    read_event,
    text::format,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/transfer_declarations.rs"));
}

/// The plugin's number for the transfer's server, and for the phone's.
const SERVER: u32 = 1;
const PHONE_SERVER: u32 = 2;
/// The port asked for; the host takes the next free one if it is in use.
const PORT: u16 = 8080;
mod own;
mod phone;

/// What the host's `serve` and `stop_serving` reply with, at most.
const SERVE_OUT: usize = 1536;

/// What the screen shows.
#[derive(Debug)]
enum View {
    /// Waiting for the server to start.
    Starting,
    /// Serving at `url`; the code carries the numeric address.
    Serving { url: String, qr: Option<QrCode> },
    /// Not serving, with the words for it.
    Failed(String),
    /// No network: `true` where PaperPaw runs the Wi-Fi (its settings join one).
    NoNetwork(bool),
    /// The offline power mode: the reader is offered the network for this.
    Offline,
}

/// One open transfer screen.
#[derive(Debug)]
struct Page {
    view: View,
    /// The empty list under the header: its back arrow and its draw.
    list: ItemList,
    /// The key of this time of serving and the wrong ones tried.
    guard: Option<Guard>,
    /// What happened last, as said on the screen.
    last: Option<String>,
    /// Whether serving was asked for (once for each time the screen opens).
    started: bool,
    /// The keyboard's question, for the phone's screen.
    phone: Option<Dialog>,
    /// The reader's own network, once chosen (*Without a router*).
    own: Option<own::Own>,
}

/// The interface-7 transfer plugin.
#[derive(Debug, Default)]
pub struct NativeTransfer {
    screens: Screens<Page>,
    net: Option<Net>,
    files: Option<Files>,
    /// The library (`None` where it cannot be had: books are then files).
    library: Option<Library>,
    device: Option<Device>,
    secrets: Option<Secrets>,
    registry: Option<Registry>,
    ui: Option<Ui>,
    work: Option<Work>,
    /// Whether the plugin opened the reader's own network and left it open.
    own_open: bool,
    /// The plugins sent from the computer: what is staged and how each stands.
    installs: Installs,
}

/// The handle in `slot`, opened on first use.
fn handle<T>(slot: &mut Option<T>, open: impl FnOnce() -> Result<T, Error>) -> Result<&T, Error> {
    if slot.is_none() {
        *slot = Some(open()?);
    }
    slot.as_ref().ok_or(Error::State)
}

impl Plugin for NativeTransfer {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::ActionRun,
            kinds::Server,
            kinds::ServedRequest,
            kinds::InstallDone,
            kinds::WifiDone,
            kinds::WorkDue,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed
        ) else {
            return Ok(());
        };
        match event {
            Event::ActionRun {
                action: "transfer.open",
                ..
            } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("transfer.screen")?)
            }
            Event::InstallDone(done) if token == INSTALL_DONE => self.install_done(host, &done),
            Event::WifiDone(done) if token == WIFI_DONE => self.wifi_done(host, &done),
            Event::WorkDue(due) if token == WORK => self.idle_due(host, due.key()),
            Event::ScreenOpened {
                instance, handover, ..
            } if token == SCREEN || token == PHONE => {
                let phone = if token == PHONE {
                    Some(self.open_phone(host, handover)?)
                } else {
                    None
                };
                let page = Page {
                    view: View::Starting,
                    list: ItemList::new(alloc::vec::Vec::new()),
                    guard: None,
                    last: None,
                    started: false,
                    phone,
                    own: None,
                };
                if page.phone.as_ref().is_some_and(|phone| phone.secret) {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch)
                        .screen_policy(Some(instance), ScreenPolicy::new().secret())?;
                }
                self.screens.open(instance, page)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN || token == PHONE => {
                self.screens.shown(token, instance)?;
                self.begin(host, token)
            }
            Event::ScreenClosed { instance, .. } if token == SCREEN || token == PHONE => {
                if let Some(Page {
                    phone: Some(phone), ..
                }) = self.screens.get(instance)
                {
                    phone.wipe();
                }
                if let Some(Page { own: Some(own), .. }) = self.screens.get(instance) {
                    own.wipe();
                }
                self.screens.close(instance);
                self.end(host, token)
            }
            Event::Server(event)
                if token == SERVER_EVENT && matches!(event.server(), SERVER | PHONE_SERVER) =>
            {
                self.on_server(host, event.server(), event.state())
            }
            Event::ServedRequest(request)
                if token == SERVED_REQUEST && matches!(request.server(), SERVER | PHONE_SERVER) =>
            {
                self.on_request(host, &request)
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let page = self.screens.top(token).ok_or(Error::State)?;
        let mut messages = Messages::new()?;
        let (title, hint) = if page.phone.is_some() {
            ("transfer.phone.title", "transfer.phone.hint")
        } else {
            ("transfer.title", "transfer.hint")
        };
        let (head, words) = match &page.view {
            View::Serving { url, qr } => {
                page.list
                    .draw_with(host, &mut frame, title, true, Footer::None)?;
                let joining = match &page.own {
                    Some(own) => Some((
                        messages.copy(host, "transfer.own.join")?,
                        own_note(host, own)?,
                    )),
                    None => None,
                };
                let join = joining
                    .as_ref()
                    .zip(page.own.as_ref())
                    .map(|((title, note), own)| Join {
                        qr: own.qr.as_ref(),
                        title,
                        note,
                    });
                return draw_address(
                    host,
                    &mut frame,
                    hint,
                    url,
                    qr.as_ref(),
                    page.last.as_deref(),
                    join.as_ref(),
                );
            }
            View::Offline => {
                let words = messages.copy(host, "core.offline.text")?;
                return page
                    .list
                    .draw_with(host, &mut frame, title, true, Footer::Note(&words));
            }
            View::Starting => ("transfer.starting", String::new()),
            View::NoNetwork(managed) => {
                let help = if *managed {
                    "transfer.no-network"
                } else {
                    "transfer.no-network.system"
                };
                ("transfer.no-network.title", messages.copy(host, help)?)
            }
            View::Failed(words) => ("transfer.failed.title", own(words)?),
        };
        let head = messages.copy(host, head)?;
        page.list.draw_with(
            host,
            &mut frame,
            title,
            true,
            Footer::Empty {
                title: &head,
                text: &words,
            },
        )
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(page) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = page.list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step
            && token == SCREEN
        {
            self.chosen(host, index)?;
        }
        Ok(ItemList::handled(step))
    }
}

/// The screen a server belongs to, and the server of a screen.
const fn token_of(server: u32) -> u32 {
    if server == PHONE_SERVER {
        PHONE
    } else {
        SCREEN
    }
}

const fn server_of(token: u32) -> u32 {
    if token == PHONE { PHONE_SERVER } else { SERVER }
}

/// The name and the password of the own network, to type where the code cannot
/// be scanned.
fn own_note(host: &Host<'_>, own: &own::Own) -> Result<String, Error> {
    let password = paperpaw_sdk::random::grouped(&own.password);
    let arguments = [
        Arg::Text {
            name: "ssid",
            value: &own.ssid,
        },
        Arg::Text {
            name: "password",
            value: &password,
        },
    ];
    Messages::new()?.format(host, "transfer.own.network", &arguments)
}

/// A copy of `words`.
fn own(words: &str) -> Result<String, Error> {
    paperpaw_ui_kit::text::copy(words)
}

impl NativeTransfer {
    /// The screen is shown the first time: its page is written and the server
    /// asked for (a server lives on a screen that shows, `network.md` 5.1).
    fn begin(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        let Some(page) = self.screens.top(token) else {
            return Ok(());
        };
        if page.started {
            return Ok(());
        }
        page.started = true;
        // What an earlier time of serving left staged goes.
        if token == SCREEN {
            self.clean_staging(host)?;
            // The offline power mode: the reader allows the network first.
            if self.is_offline(host)? {
                return self.show(host, token, View::Offline);
            }
        }
        let started = self.start(host, token);
        match started {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(error) => self.show(
                host,
                token,
                View::Failed(failed(host, "transfer.failed", word(error))?),
            ),
        }
    }

    /// A key for this time of serving: none where the source was not fully
    /// random; an error where there is no source.
    fn draw_key(&mut self, host: &Host<'_>) -> Result<Option<String>, Error> {
        native_key::key(&mut draw_from(&mut self.secrets, host))
    }

    /// Checks that a key can be drawn, writes the page and starts the server.
    fn start(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        // A device with no source of random numbers serves nothing: no weaker key.
        match self.draw_key(host) {
            Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                return self.show(host, token, View::Failed(no_random(host)?));
            }
            Err(error) => return Err(error),
            Ok(key) => key.map_or((), wipe_text),
        }
        let server = if token == PHONE {
            self.write_phone_page(host)?;
            Server::new(PHONE_SERVER, PORT)?.max_body(MAX_TEXT as u64)?
        } else {
            native_page::write(host)?;
            Server::new(SERVER, PORT)?.max_body(MAX_SERVED_BODY)?
        };
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        handle(&mut self.net, || Net::open(host))?.serve(host, &server, &mut input, &mut out)
    }

    /// The screen closed: the server stops (the host does it too; stopping is
    /// idempotent) and the page is removed.
    fn end(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        let (server, files) = if token == PHONE {
            (
                PHONE_SERVER,
                [native_phone::PAGE_FILE, native_phone::PAGE_FILE],
            )
        } else {
            self.clean_staging(host)?;
            self.leave_own(host)?;
            (
                SERVER,
                [native_web::PAGE_FILE, native_install::LISTING_FILE],
            )
        };
        if let Some(net) = &self.net {
            let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
            // Nothing to do where it was stopped already.
            if let Err(Error::NoMemory) = net.stop_serving(host, server, &mut input, &mut out) {
                return Err(Error::NoMemory);
            }
        }
        for file in files {
            if let Err(Error::NoMemory) = storage::remove(host, Root::Data, file) {
                return Err(Error::NoMemory);
            }
        }
        Ok(())
    }

    /// Removes the files of plugins staged for installing, but those the host
    /// is asked about (it copies them while it asks).
    fn clean_staging(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let files = handle(&mut self.files, || Files::open(host))?;
        self.installs.forget_idle();
        native_install_host::remove_unknown_staging(host, files, &self.installs)
    }

    /// Shows `view` on the screen that is open, and draws it.
    fn show(&mut self, host: &Host<'_>, token: u32, view: View) -> Result<(), Error> {
        let rows = if token == SCREEN {
            Self::rows(host, &view)?
        } else {
            alloc::vec::Vec::new()
        };
        if let Some(page) = self.screens.top(token) {
            page.view = view;
            page.list.replace(rows);
            redraw(host, RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The server started, failed or stopped.
    fn on_server(
        &mut self,
        host: &Host<'_>,
        server: u32,
        state: ServerState<'_>,
    ) -> Result<(), Error> {
        let token = token_of(server);
        let serving = matches!(
            self.screens.top(token).map(|page| &page.view),
            Some(View::Serving { .. })
        );
        match state {
            ServerState::Started { address } => self.serving(host, token, address),
            ServerState::Failed { reason } => {
                self.not_serving(host, token, "transfer.failed", reason)
            }
            // Asked for by the plugin: nothing to say. Ended by the network:
            // said as it was before it served.
            ServerState::Stopped { reason } => match stopped_key(serving, reason) {
                Some(key) => self.not_serving(host, token, key, reason),
                None => Ok(()),
            },
            _ => Ok(()),
        }
    }

    /// The server runs at `address`: the key is drawn now (on the X4 the radio
    /// makes it fully random) and the address shown with it.
    fn serving(&mut self, host: &Host<'_>, token: u32, address: &str) -> Result<(), Error> {
        let key = match self.draw_key(host) {
            Ok(Some(key)) => key,
            Ok(None) | Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                self.stop(host, server_of(token))?;
                return self.show(host, token, View::Failed(no_random(host)?));
            }
            Err(error) => return Err(error),
        };
        let reach = self.reach(host)?;
        let (url, scanned) = urls(reach.as_deref(), address, &key)?;
        let qr = QrCode::new(scanned.as_bytes());
        if let Some(page) = self.screens.top(token) {
            page.guard = Some(Guard::new(key));
        }
        self.show(host, token, View::Serving { url, qr })
    }

    fn stop(&mut self, host: &Host<'_>, server: u32) -> Result<(), Error> {
        let Some(net) = &self.net else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        net.stop_serving(host, server, &mut input, &mut out)
    }

    /// The server did not start or ended: no network is its own screen.
    fn not_serving(
        &mut self,
        host: &Host<'_>,
        token: u32,
        key: &str,
        reason: Reason,
    ) -> Result<(), Error> {
        if let Some(page) = self.screens.top(token) {
            page.guard = None;
        }
        if reason == Reason::NoNetwork {
            let managed = self.wifi(host)?.is_some();
            return self.show(host, token, View::NoNetwork(managed));
        }
        let words = failed(host, key, reason_word(reason))?;
        self.show(host, token, View::Failed(words))
    }

    /// The radio's state and addresses, none where PaperPaw does not manage it.
    fn wifi(&mut self, host: &Host<'_>) -> Result<Option<(WifiState, String, String)>, Error> {
        let device = handle(&mut self.device, || Device::open(host))?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match device.wifi(host, &mut input, &mut out) {
            Ok(info) => Ok(Some((
                info.state(),
                own(info.host_name())?,
                own(info.address())?,
            ))),
            Err(Error::NotFound | Error::Unsupported) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Where a browser on another device reaches this one: its name in the
    /// network where there is one, else its address (`WifiState::reach`).
    fn reach(&mut self, host: &Host<'_>) -> Result<Option<String>, Error> {
        Ok(match self.wifi(host)? {
            Some((WifiState::Online, name, _)) if !name.is_empty() => Some(name),
            Some((WifiState::Online | WifiState::Hosting, _, address)) if !address.is_empty() => {
                Some(address)
            }
            _ => None,
        })
    }

    /// A request came to the page. Whatever it asks, the answer is sent and
    /// the callback ends well: nothing a computer on the network sends may
    /// fail the plugin.
    fn on_request(&mut self, host: &Host<'_>, request: &ServedRequest<'_>) -> Result<(), Error> {
        if request.server() == SERVER {
            self.touched(host, request.now_ms())?;
        }
        let answered = if request.server() == PHONE_SERVER {
            self.answer_phone(host, request)
        } else {
            self.answer(host, request)
        };
        match answered {
            Ok(()) => Ok(()),
            Err(error) => {
                let status = if error == Error::NoMemory { 503 } else { 500 };
                let refused = Answer {
                    status,
                    kind: "text/plain; charset=utf-8",
                    body: Body::Text("failed, try again"),
                };
                // Nothing more to do where even that is not sent.
                let _ = self.send(host, request.request(), &refused);
                Ok(())
            }
        }
    }

    fn answer(&mut self, host: &Host<'_>, request: &ServedRequest<'_>) -> Result<(), Error> {
        let body = match request.body_len() {
            Some(_) => Some(format(format_args!(
                ".received/{}/{}",
                request.server(),
                request.request()
            ))?),
            None => None,
        };
        let asked = Request {
            method: request.method(),
            target: request.target(),
            body: body.as_deref(),
            body_len: request.body_len().unwrap_or(0),
            at: Instant(request.now_ms()),
        };
        let files = handle(&mut self.files, || Files::open(host))?;
        let library = match handle(&mut self.library, || Library::open(host)) {
            Ok(library) => Some(library),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // Not declared or not allowed: books are files then.
            Err(_) => None,
        };
        let mut card = HostCard::new(host, files, library)?;
        let Some(guard) = self
            .screens
            .top(SCREEN)
            .and_then(|page| page.guard.as_mut())
        else {
            // Before the key was drawn (or after the screen closed): not served.
            let words = Answer {
                status: 503,
                kind: "text/plain; charset=utf-8",
                body: Body::Text("not ready"),
            };
            return self.send(host, request.request(), &words);
        };
        let mut plugins = HostPlugins::new(host, files, &mut self.registry);
        let mut parts = Parts {
            card: &mut card,
            plugins: &mut plugins,
            installs: &mut self.installs,
        };
        let (answer, done) = native_web::answer(&mut parts, guard, &asked);
        self.send(host, request.request(), &answer)?;
        if let Some((done, name)) = done {
            let said = said(host, done, &name)?;
            if let Some(page) = self.screens.top(SCREEN) {
                page.last = Some(said);
                redraw(host, RedrawMode::Fast)?;
            }
        }
        Ok(())
    }

    /// The host ended an install this plugin asked for: the state the page
    /// reads is set, and the device says what came of it.
    fn install_done(&mut self, host: &Host<'_>, done: &InstallDone<'_>) -> Result<(), Error> {
        let Some(request) = done.request() else {
            return Ok(());
        };
        if done.kind() != InstallKind::Plugin {
            return Ok(());
        }
        let result = if done.status().is_ok() && done.outcome() == DeviceOutcome::Ok {
            Ok(done.at_next_start())
        } else {
            Err(native_install::why(done.status(), done.outcome()))
        };
        let files = handle(&mut self.files, || Files::open(host))?;
        let mut plugins = HostPlugins::new(host, files, &mut self.registry);
        let Some(ended) = self.installs.ended(&mut plugins, request.get(), result) else {
            return Ok(());
        };
        let key = match ended.result {
            Ok(false) => "transfer.plugin-installed",
            Ok(true) => "transfer.plugin-restart",
            Err(_) => "transfer.plugin-not-installed",
        };
        let said = say_name(host, key, &ended.id)?;
        if let Some(page) = self.screens.top(SCREEN) {
            page.last = Some(said);
            redraw(host, RedrawMode::Fast)?;
        }
        Ok(())
    }

    /// Sends `answer` to request `number`.
    fn send(&mut self, host: &Host<'_>, number: u32, answer: &Answer) -> Result<(), Error> {
        let mut response = Response::new(number, answer.status)?.content_type(answer.kind)?;
        response = match &answer.body {
            Body::Text(words) => response.body(words.as_bytes())?,
            Body::Owned(words) => response.body(words.as_bytes())?,
            Body::Data(file) => response.send_file(FileRoot::Data, file)?,
            Body::File(path) => response.send_file(FileRoot::Files, path)?,
        };
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(SERVE_OUT)?);
        let net = handle(&mut self.net, || Net::open(host))?;
        match net.respond(host, &response, &mut input, &mut out) {
            // Ended already (the client went, or the 120 s passed).
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Draws bytes from the device's source (`core.secrets` `random`, up to 256 a
/// call) for [`native_key::key`]; the bytes of the reply are wiped.
fn draw_from<'a>(
    secrets: &'a mut Option<Secrets>,
    host: &'a Host<'_>,
) -> impl FnMut(&mut [u8]) -> Result<bool, Error> + 'a {
    move |bytes: &mut [u8]| {
        let secrets = handle(secrets, || Secrets::open(host))?;
        let (mut input, mut out) = (buffer(128)?, buffer(512)?);
        let drawn = secrets.random(host, bytes.len(), &mut input, &mut out);
        let strong = drawn.map(|drawn| {
            bytes.copy_from_slice(drawn.bytes());
            drawn.strong()
        });
        wipe(&mut out);
        strong
    }
}

fn redraw(host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(mode)
}

/// What the screen says where no key can be drawn.
fn no_random(host: &Host<'_>) -> Result<String, Error> {
    Messages::new()?.copy(host, "transfer.no-random")
}

/// `key` (*could not start*, *stopped*) with the reason in the reader's words.
fn failed(host: &Host<'_>, key: &str, reason: &str) -> Result<String, Error> {
    let arguments = [Arg::Text {
        name: "reason",
        value: reason,
    }];
    Messages::new()?.format(host, key, &arguments)
}

/// The reason a server did not run, as interface 6 named it.
const fn reason_word(reason: Reason) -> &'static str {
    match reason {
        Reason::NoNetwork => "no network",
        Reason::Offline => "offline",
        Reason::Saving => "saving",
        Reason::NetworkOff => "network off",
        Reason::Refused => "refused",
        Reason::Ok => "ended",
        _ => "failed",
    }
}

/// Why the plugin's own start failed.
const fn word(error: Error) -> &'static str {
    match error {
        Error::Busy => "busy",
        Error::Denied => "not allowed",
        Error::Unsupported => "not supported",
        Error::Full | Error::NoSpace => "no room",
        _ => "failed",
    }
}

/// What a request did, as said on the screen.
fn said(host: &Host<'_>, done: Done, name: &str) -> Result<String, Error> {
    let key = match done {
        Done::Received => "transfer.received",
        Done::Sent => "transfer.sent",
        Done::Folder => "transfer.folder",
        Done::Deleted => "transfer.deleted",
        Done::Moved => "transfer.moved",
        Done::NotReceived => "transfer.not-received",
    };
    say_name(host, key, name)
}

/// The text `key`, with `name` where it names it.
fn say_name(host: &Host<'_>, key: &str, name: &str) -> Result<String, Error> {
    let arguments = [Arg::Text {
        name: "name",
        value: name,
    }];
    Messages::new()?.format(host, key, &arguments)
}

/// What a stop of the server says. Where the screen was serving, the address
/// on it is dead whoever ended it (the network, or the host with `Reason::Ok`),
/// so the *stopped* text takes its place. Before it served, a stop the plugin
/// asked for (`Ok`) says nothing; one the network ended says *could not start*.
fn stopped_key(serving: bool, reason: Reason) -> Option<&'static str> {
    if serving {
        Some("transfer.stopped")
    } else if reason == Reason::Ok {
        None
    } else {
        Some("transfer.failed")
    }
}

#[cfg(test)]
mod stop_tests {
    use super::{Reason, stopped_key};

    /// A stop the plugin asked for before it served says nothing; one that
    /// ends a serving screen clears the address (the *stopped* text).
    #[test]
    fn a_stop_with_ok_clears_a_serving_screen_only() {
        assert_eq!(stopped_key(true, Reason::Ok), Some("transfer.stopped"));
        assert_eq!(stopped_key(false, Reason::Ok), None);
    }

    #[test]
    fn a_stop_by_the_network_says_so() {
        assert_eq!(
            stopped_key(true, Reason::NoNetwork),
            Some("transfer.stopped")
        );
        assert_eq!(stopped_key(false, Reason::Offline), Some("transfer.failed"));
    }
}
