//! The interface-7 transfer screen: while it is open the device serves the web
//! page to the local network (`core.net` `serve`), shows the address as text and
//! as a QR code, and answers the page's requests (list, download, upload, make a
//! folder, delete: `native_web`). The address carries a key drawn anew for each
//! time of serving from the device's random source (`core.secrets`); wrong keys
//! lock the page for a while. Closing the screen ends the serving. What is not
//! native yet (the phone's keyboard, the cable, the offer to go online, the
//! own network, installing plugins and moving files) is named in `PORTING-7.md`.
use crate::{
    declarations::{OPEN_TOKEN, SCREEN, SERVED_REQUEST, SERVER_EVENT},
    native_draw::draw_address,
    native_files::HostCard,
    native_key, native_page,
    native_transfer::urls,
    native_web::{self, Answer, Body, Done, Request},
};
use alloc::string::String;
use paperpaw_qr::QrCode;
use paperpaw_sdk::{
    Handled, Instant,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        device::{Device, WifiState},
        events::{Event, Incoming, kinds},
        files::Files,
        formatting::Arg,
        net::{
            FileRoot, MAX_SERVED_BODY, Net, REQUEST_INPUT, Reason, Response, ServedRequest, Server,
            ServerState,
        },
        registrations,
        reply::Reply,
        secrets::{Secrets, wipe},
        storage::{self, Root},
        ui::Open,
    },
    served::Guard,
};
use paperpaw_ui_kit::{
    Footer, ItemList, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer},
    read_event,
    text::format,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/transfer_declarations.rs"));
}

/// The plugin's number for its one server.
const SERVER: u32 = 1;
/// The port asked for; the host takes the next free one if it is in use.
const PORT: u16 = 8080;
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
}

/// The interface-7 transfer plugin.
#[derive(Debug, Default)]
pub struct NativeTransfer {
    screens: Screens<Page>,
    net: Option<Net>,
    files: Option<Files>,
    device: Option<Device>,
    secrets: Option<Secrets>,
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
            Event::ScreenOpened { instance, .. } if token == SCREEN => {
                let page = Page {
                    view: View::Starting,
                    list: ItemList::new(alloc::vec::Vec::new()),
                    guard: None,
                    last: None,
                    started: false,
                };
                self.screens.open(instance, page)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => {
                self.screens.shown(token, instance)?;
                self.begin(host)
            }
            Event::ScreenClosed { instance, .. } if token == SCREEN => {
                self.screens.close(instance);
                self.end(host)
            }
            Event::Server(event) if token == SERVER_EVENT && event.server() == SERVER => {
                self.on_server(host, event.state())
            }
            Event::ServedRequest(request)
                if token == SERVED_REQUEST && request.server() == SERVER =>
            {
                self.on_request(host, &request)
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let page = self.screens.top(token).ok_or(Error::State)?;
        let mut messages = Messages::new()?;
        let (head, words) = match &page.view {
            View::Serving { url, qr } => {
                page.list
                    .draw_with(host, &mut frame, "transfer.title", true, Footer::None)?;
                return draw_address(host, &mut frame, url, qr.as_ref(), page.last.as_deref());
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
            "transfer.title",
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
        Ok(ItemList::handled(step))
    }
}

/// A copy of `words`.
fn own(words: &str) -> Result<String, Error> {
    paperpaw_ui_kit::text::copy(words)
}

impl NativeTransfer {
    /// The screen is shown the first time: its page is written and the server
    /// asked for (a server lives on a screen that shows, `network.md` 5.1).
    fn begin(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(page) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        if page.started {
            return Ok(());
        }
        page.started = true;
        let started = self.start(host);
        match started {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(error) => self.show(
                host,
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
    fn start(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // A device with no source of random numbers serves nothing: no weaker key.
        match self.draw_key(host) {
            Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                return self.show(host, View::Failed(no_random(host)?));
            }
            Err(error) => return Err(error),
            Ok(_) => {}
        }
        native_page::write(host)?;
        let server = Server::new(SERVER, PORT)?.max_body(MAX_SERVED_BODY)?;
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        handle(&mut self.net, || Net::open(host))?.serve(host, &server, &mut input, &mut out)
    }

    /// The screen closed: the server stops (the host does it too; stopping is
    /// idempotent) and the page is removed.
    fn end(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if let Some(net) = &self.net {
            let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
            // Nothing to do where it was stopped already.
            if let Err(Error::NoMemory) = net.stop_serving(host, SERVER, &mut input, &mut out) {
                return Err(Error::NoMemory);
            }
        }
        match storage::remove(host, Root::Data, native_web::PAGE_FILE) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            _ => Ok(()),
        }
    }

    /// Shows `view` on the screen that is open, and draws it.
    fn show(&mut self, host: &Host<'_>, view: View) -> Result<(), Error> {
        if let Some(page) = self.screens.top(SCREEN) {
            page.view = view;
            redraw(host, RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The server started, failed or stopped.
    fn on_server(&mut self, host: &Host<'_>, state: ServerState<'_>) -> Result<(), Error> {
        let serving = matches!(
            self.screens.top(SCREEN).map(|page| &page.view),
            Some(View::Serving { .. })
        );
        match state {
            ServerState::Started { address } => self.serving(host, address),
            ServerState::Failed { reason } => self.not_serving(host, "transfer.failed", reason),
            // Asked for by the plugin: nothing to say. Ended by the network:
            // said as it was before it served.
            ServerState::Stopped { reason } if reason != Reason::Ok => {
                let key = if serving {
                    "transfer.stopped"
                } else {
                    "transfer.failed"
                };
                self.not_serving(host, key, reason)
            }
            _ => Ok(()),
        }
    }

    /// The server runs at `address`: the key is drawn now (on the X4 the radio
    /// makes it fully random) and the address shown with it.
    fn serving(&mut self, host: &Host<'_>, address: &str) -> Result<(), Error> {
        let key = match self.draw_key(host) {
            Ok(Some(key)) => key,
            Ok(None) | Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                self.stop(host)?;
                return self.show(host, View::Failed(no_random(host)?));
            }
            Err(error) => return Err(error),
        };
        let reach = self.reach(host)?;
        let (url, scanned) = urls(reach.as_deref(), address, &key)?;
        let qr = QrCode::new(scanned.as_bytes());
        if let Some(page) = self.screens.top(SCREEN) {
            page.guard = Some(Guard::new(key));
        }
        self.show(host, View::Serving { url, qr })
    }

    fn stop(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(net) = &self.net else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        net.stop_serving(host, SERVER, &mut input, &mut out)
    }

    /// The server did not start or ended: no network is its own screen.
    fn not_serving(&mut self, host: &Host<'_>, key: &str, reason: Reason) -> Result<(), Error> {
        if let Some(page) = self.screens.top(SCREEN) {
            page.guard = None;
        }
        if reason == Reason::NoNetwork {
            let managed = self.wifi(host)?.is_some();
            return self.show(host, View::NoNetwork(managed));
        }
        let words = failed(host, key, reason_word(reason))?;
        self.show(host, View::Failed(words))
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
        match self.answer(host, request) {
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
        let mut card = HostCard::new(host, files)?;
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
        let (answer, done) = native_web::answer(&mut card, guard, &asked);
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

    /// Sends `answer` to request `number`.
    fn send(&mut self, host: &Host<'_>, number: u32, answer: &Answer) -> Result<(), Error> {
        let mut response = Response::new(number, answer.status)?.content_type(answer.kind)?;
        response = match &answer.body {
            Body::Text(words) => response.body(words.as_bytes())?,
            Body::Owned(words) => response.body(words.as_bytes())?,
            Body::Page => response.send_file(FileRoot::Data, native_web::PAGE_FILE)?,
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
        Done::NotReceived => "transfer.not-received",
    };
    let arguments = [Arg::Text {
        name: "name",
        value: name,
    }];
    Messages::new()?.format(host, key, &arguments)
}
