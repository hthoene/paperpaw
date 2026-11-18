//! The setup with a phone on the native plugin: the device's own network, the
//! page it serves on port 80 (`core.net` `serve` and `respond`), the codes to
//! join and to open it, and the network the user chooses on the phone. The
//! decisions are [`Phone`] (`native_phone`); this module reads the host for
//! them, does what they say and draws the screen.
//!
//! Passwords: the own network's password and the page's token are drawn from
//! `core.secrets` (a weak draw makes no network, `native_own`), the form sent
//! from the phone is read into a buffer that is wiped and its file in the
//! host's `.received/` folder removed at once (the plugin cannot overwrite it:
//! the folder is the host's), and the chosen network's password goes to the
//! policy in a [`Password`], wiped when dropped.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_qr::{QrCode, QrError};
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, RedrawMode},
    events::WifiFailure,
    formatting::Arg,
    net::{
        FileRoot, MAX_INLINE_BODY, Net, REQUEST_INPUT, Reason, Response, ServedRequest, Server,
        ServerState,
    },
    storage::DataStorage,
    work::WorkSpec,
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, Messages,
    mem::{SERVICE_BUFFER, buffer, push, wipe, wipe_text},
    text::{concat, copy},
};

use super::{Named, NativeWifi, PHONE_TOKEN, View};
use crate::native_own::{self, Own};
use crate::native_phone::{
    Action, Effect, Failed, MAX_FORM, PORT, Page, Phone, Radio as PhoneRadio, Stage, Step,
    page::{Found, MOST_PAGE, Texts, choose_page_fit, line_page, not_found_page},
};
use crate::native_policy::{Goal, Password, Radio};

/// The plugin's number for its one server.
const SERVER: u32 = 1;
/// The work that closes the screen when left alone, and the steps of leaving.
pub(super) const WAKE_KEY: &str = "wifi.phone-wake";
/// The token the wake comes back with.
pub(super) const WAKE_WORK: u32 = 3;
/// The page that is too long to answer inline, in the data folder.
const PAGE_FILE: &str = "phone-page.html";
/// The reply buffer of `serve`, `stop_serving` and `respond`.
const SERVE_OUT: usize = 1_536;
/// The longest body the server takes: a form is at most [`MAX_FORM`], one
/// that is longer counts as none (as in interface 6), not as a refusal.
const MAX_BODY: u64 = 4_096;

/// What asking the host to serve came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Serving {
    Started,
    Busy,
    Hidden,
}

/// The setup screen's state.
#[derive(Debug)]
pub(super) struct PhoneView {
    pub(super) list: ItemList,
    phone: Phone,
    own: Option<Own>,
    /// The code that joins the own network.
    join_qr: Option<QrCode>,
    /// The code that opens the page, once it is served.
    page_qr: Option<QrCode>,
}

impl PhoneView {
    /// The list to draw and what it shows under its rows.
    pub(super) fn drawn(&mut self) -> (&mut ItemList, Footer<'_>) {
        let footer = match (self.phone.stage(), self.phone.step()) {
            (Stage::Opening | Stage::Hosting { .. }, Step::Join) => match &self.join_qr {
                Some(code) => Footer::Code { code, caption: "" },
                None => Footer::None,
            },
            (Stage::Hosting { page: Some(_) }, Step::Open) => match &self.page_qr {
                Some(code) => Footer::Code { code, caption: "" },
                None => Footer::None,
            },
            _ => Footer::None,
        };
        (&mut self.list, footer)
    }

    /// The password and the token are wiped, not only dropped.
    pub(super) fn wipe(&mut self) {
        self.list.wipe();
        if let Some(code) = &mut self.join_qr {
            // The join code carries the password of the own network.
            code.wipe();
        }
        self.join_qr = None;
    }
}

/// `value` in decimal.
fn decimal(value: u32, digits: &mut [u8; 10]) -> &str {
    let mut at = digits.len();
    let mut rest = value;
    loop {
        at -= 1;
        digits[at] = b'0' + u8::try_from(rest % 10).unwrap_or(0);
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    core::str::from_utf8(digits.get(at..).unwrap_or(&[])).unwrap_or("0")
}

/// What a server's failure is in words, for `wifi.phone.failed`.
fn reason_key(reason: Reason) -> &'static str {
    match reason {
        Reason::NoNetwork | Reason::NetworkOff | Reason::Offline => "wifi.phone.reason-network",
        Reason::Refused => "wifi.phone.reason-refused",
        _ => "wifi.phone.reason-other",
    }
}

impl NativeWifi {
    /// The setup screen is opened: the own network is made (from a fully
    /// random draw, or not at all) and the rows are laid out.
    pub(super) fn phone_view(&mut self, host: &Host<'_>, now: u64) -> Result<PhoneView, Error> {
        let name = {
            let device = self.device.as_ref().ok_or(Error::State)?;
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            copy(device.device(host, &mut input, &mut out)?.model())?
        };
        let own = match self.secrets(host)? {
            Some(secrets) => {
                let mut draw = |bytes: &mut [u8]| -> Result<bool, Error> {
                    let (mut input, mut out) = (buffer(256)?, buffer(1_280)?);
                    let drawn = secrets.random(host, bytes.len(), &mut input, &mut out);
                    let result = drawn.map(|drawn| {
                        bytes.copy_from_slice(drawn.bytes());
                        drawn.strong()
                    });
                    wipe(&mut out);
                    result
                };
                Own::draw(&name, &mut draw)?
            }
            None => None,
        };
        let join_qr = match &own {
            Some(own) => {
                let code = native_own::join_code(own.ssid(), own.password().as_str())?;
                let qr = QrCode::try_new(code.as_bytes());
                wipe_text(code);
                // No code where it cannot be made: the name and the password
                // are on the screen to be typed.
                match qr {
                    Ok(code) => Some(code),
                    Err(QrError::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => None,
                }
            }
            None => None,
        };
        let phone = Phone::new(
            own.as_ref().map(Own::ssid),
            own.as_ref().map_or("", |own| own.token().as_str()),
            self.policy.manual,
            now,
        )?;
        let mut view = PhoneView {
            list: ItemList::default(),
            phone,
            own,
            join_qr,
            page_qr: None,
        };
        Self::phone_build(host, &mut view)?;
        Ok(view)
    }

    /// The rows of the two setup steps (join the own network, then open the
    /// page); the index of the button that goes on.
    fn setup_rows(
        host: &Host<'_>,
        say: &mut Messages,
        phone: &Phone,
        own: &Own,
        items: &mut Vec<Item>,
    ) -> Result<usize, Error> {
        if let Some(problem) = phone.problem() {
            push(items, Item::noted(problem, "", "")?)?;
        }
        let (number, title, note, button) = match phone.step() {
            Step::Join => (
                "1. ",
                "wifi.phone.join-title",
                "wifi.phone.join-note",
                "wifi.phone.next",
            ),
            Step::Open => (
                "2. ",
                "wifi.phone.open-title",
                "wifi.phone.open-note",
                "wifi.phone.back-step",
            ),
        };
        let heading = concat(&[number, &say.copy(host, title)?])?;
        push(items, Item::heading(&heading)?)?;
        let text = say.copy(host, note)?;
        push(items, Item::noted(&text, "", "")?)?;
        match phone.step() {
            Step::Join => {
                let title = say.copy(host, "wifi.phone.name")?;
                push(items, Item::row(&title, own.ssid())?)?;
                let title = say.copy(host, "wifi.phone.password")?;
                let grouped = native_own::grouped(own.password().as_str())?;
                let item = Item::row(&title, &grouped);
                wipe_text(grouped);
                push(items, item?)?;
            }
            Step::Open if phone.page_address().is_none() => {
                let text = say.copy(host, "wifi.phone.opening")?;
                push(items, Item::noted(&text, "", "")?)?;
            }
            // The address in words, whether or not a code is drawn for it (none
            // where it could not be made, or the rows take several pages).
            Step::Open => {
                if let Some(url) = phone.page_address() {
                    let text = concat(&[&say.copy(host, "wifi.phone.type-address")?, " ", url])?;
                    push(items, Item::noted(&text, "", "")?)?;
                }
            }
        }
        let label = say.copy(host, button)?;
        let next = items.len();
        push(items, Item::row(&label, "›")?)?;
        Ok(next)
    }

    /// The rows for what the setup is doing now.
    fn phone_build(host: &Host<'_>, view: &mut PhoneView) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        let phone = &view.phone;
        let mut next = None;
        match (phone.stage(), &view.own) {
            (_, None) => {
                let text = say.copy(host, "wifi.phone.no-random")?;
                push(&mut items, Item::noted(&text, "", "")?)?;
            }
            (Stage::Searching, _) => {
                let text = say.copy(host, "wifi.state.searching")?;
                push(&mut items, Item::noted(&text, "", "")?)?;
            }
            (Stage::Opening, Some(_)) if phone.problem().is_none() => {
                let text = say.copy(host, "wifi.phone.opening")?;
                push(&mut items, Item::noted(&text, "", "")?)?;
            }
            (Stage::Opening | Stage::Hosting { .. }, Some(own)) => {
                next = Some(Self::setup_rows(host, &mut say, phone, own, &mut items)?);
            }
            (Stage::Leaving { ssid, .. } | Stage::Joining(ssid), _) => {
                let text = say.format(
                    host,
                    "wifi.state.joining",
                    &[Arg::Text {
                        name: "ssid",
                        value: ssid,
                    }],
                )?;
                push(&mut items, Item::noted(&text, "", "")?)?;
            }
            (Stage::Joined(ssid), _) => {
                let text = say.format(
                    host,
                    "wifi.phone.joined",
                    &[Arg::Text {
                        name: "ssid",
                        value: ssid,
                    }],
                )?;
                push(&mut items, Item::noted(&text, "", "")?)?;
            }
            (Stage::Failed(Failed::NotOpened), _) => {
                let text = say.copy(host, "wifi.phone.not-opened")?;
                push(&mut items, Item::noted(&text, "", "")?)?;
            }
            (Stage::Failed(Failed::Server(reason)), _) => {
                let text = say.format(
                    host,
                    "wifi.phone.failed",
                    &[Arg::Text {
                        name: "reason",
                        value: reason,
                    }],
                )?;
                push(&mut items, Item::noted(&text, "", "")?)?;
            }
        }
        view.list.wipe();
        view.list = ItemList::new(items);
        if let Some(next) = next {
            view.list.set_focus(next);
        }
        Ok(())
    }

    fn redraw_now(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// What the radio says, for the setup's decisions.
    fn with_radio<T>(
        info: &Named,
        f: impl FnOnce(&PhoneRadio<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let radio = PhoneRadio {
            on: info.radio.is_on(),
            hosting: info.radio == Radio::Hosting,
            guests: info.guests,
            online: (info.radio == Radio::Online).then_some(info.ssid.as_str()),
        };
        f(&radio)
    }

    /// Feeds the setup what happened and does what it answers.
    fn phone_step(
        &mut self,
        host: &Host<'_>,
        f: impl FnOnce(&mut Phone) -> Result<Vec<Effect>, Error>,
    ) -> Result<(), Error> {
        let Some(View::Phone(view)) = self.screens.top(PHONE_TOKEN) else {
            return Ok(());
        };
        let effects = f(&mut view.phone)?;
        self.phone_run(host, effects)
    }

    /// The screen is shown: the radio is held on and networks are looked for.
    pub(super) fn phone_shown(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let on = self.read_info(host)?.radio.is_on();
        self.phone_step(host, |phone| phone.shown(on))
    }

    /// Networks were found.
    pub(super) fn phone_scanned(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.phone_step(host, Phone::scanned)
    }

    /// The radio changed: the own network opened, a phone joined, a network
    /// joined.
    pub(super) fn phone_changed(&mut self, host: &Host<'_>, now: u64) -> Result<(), Error> {
        let info = self.read_named(host)?;
        self.phone_step(host, |phone| {
            Self::with_radio(&info, |radio| phone.radio_changed(now, radio))
        })
    }

    /// A join or the own network failed.
    pub(super) fn phone_failed(
        &mut self,
        host: &Host<'_>,
        now: u64,
        ssid: &str,
        failure: WifiFailure,
    ) -> Result<(), Error> {
        if !matches!(self.screens.top(PHONE_TOKEN), Some(View::Phone(_))) {
            return Ok(());
        }
        let key = match crate::native_policy::Failure::from(failure) {
            crate::native_policy::Failure::Password => "wifi.failed.password",
            crate::native_policy::Failure::NotFound => "wifi.failed.not-found",
            crate::native_policy::Failure::Timeout => "wifi.failed.timeout",
            _ => "wifi.failed.other",
        };
        let mut say = Messages::new()?;
        let problem = say.format(
            host,
            key,
            &[Arg::Text {
                name: "ssid",
                value: ssid,
            }],
        )?;
        self.phone_step(host, |phone| phone.failed(now, ssid, &problem))
    }

    /// The screen's timer came due.
    pub(super) fn phone_wake(&mut self, host: &Host<'_>, now: u64) -> Result<(), Error> {
        let info = self.read_named(host)?;
        self.phone_step(host, |phone| {
            Self::with_radio(&info, |radio| phone.timer(now, radio))
        })
    }

    /// A button: `Some` where it was the setup's.
    pub(super) fn phone_action(&mut self, host: &Host<'_>, action: Action) -> Result<bool, Error> {
        let info = self.read_named(host)?;
        let Some(View::Phone(view)) = self.screens.top(PHONE_TOKEN) else {
            return Ok(false);
        };
        let Some(effects) = Self::with_radio(&info, |radio| view.phone.action(action, radio))?
        else {
            return Ok(false);
        };
        self.phone_run(host, effects)?;
        Ok(true)
    }

    /// The server started, failed or stopped.
    pub(super) fn phone_server(
        &mut self,
        host: &Host<'_>,
        state: ServerState<'_>,
    ) -> Result<(), Error> {
        match state {
            ServerState::Started { address } => {
                self.phone_step(host, |phone| phone.server_started(address))?;
                self.phone_page_code()
            }
            ServerState::Failed { reason } => {
                let mut say = Messages::new()?;
                let words = say.copy(host, reason_key(reason))?;
                self.phone_step(host, |phone| phone.server_failed(&words))
            }
            // Stopped by the screen or by the network going: the page is no
            // longer there, and the screen says nothing more.
            _ => Ok(()),
        }
    }

    /// The code that opens the page, from its address.
    fn phone_page_code(&mut self) -> Result<(), Error> {
        let Some(View::Phone(view)) = self.screens.top(PHONE_TOKEN) else {
            return Ok(());
        };
        let Some(url) = view.phone.page_address() else {
            return Ok(());
        };
        // No code where it cannot be made (the address is in words on the
        // screen): a short heap is not a failure of the callback, which would
        // drop the redraw already staged.
        view.page_qr = QrCode::try_new(url.as_bytes()).ok();
        Ok(())
    }

    /// The screen closed: the page is not served, the own network closed, the
    /// radio as the user left it.
    pub(super) fn phone_closed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let info = self.read_named(host)?;
        let Some(View::Phone(view)) = self.screens.top(PHONE_TOKEN) else {
            return Ok(());
        };
        let effects = Self::with_radio(&info, |radio| view.phone.action(Action::Back, radio))?;
        // The screen is gone already: no leaving it.
        let effects: Vec<Effect> = effects
            .unwrap_or_default()
            .into_iter()
            .filter(|effect| !matches!(effect, Effect::Back))
            .collect();
        let _ = DataStorage::new(host).remove(PAGE_FILE);
        self.phone_run(host, effects)
    }

    /// Does what the setup says, in order.
    fn phone_run(&mut self, host: &Host<'_>, effects: Vec<Effect>) -> Result<(), Error> {
        let mut redraw = false;
        let mut server_failed = false;
        for effect in effects {
            match effect {
                Effect::Redraw => redraw = true,
                Effect::Policy { manual, phone } => {
                    self.policy.manual = manual;
                    self.policy.phone = phone;
                    if phone {
                        self.policy.goal = Goal::Nothing;
                    }
                }
                Effect::RadioOn => CommandSender::new(host, &mut [0; 64]).wifi_on()?,
                Effect::Scan => CommandSender::new(host, &mut [0; 64]).wifi_scan()?,
                Effect::Leave => CommandSender::new(host, &mut [0; 64]).wifi_leave()?,
                Effect::Host => self.phone_host(host)?,
                Effect::Serve => server_failed |= self.phone_serve(host)? == Serving::Busy,
                Effect::StopServing => self.phone_stop(host)?,
                Effect::Respond {
                    request,
                    status,
                    page,
                } => {
                    self.phone_respond(host, request, status, &page)?;
                }
                Effect::Choose { ssid, password } => {
                    let radio = self.read_info(host)?.radio;
                    let steps = self.policy.choose(radio, &ssid, password.as_str())?;
                    self.policy.pending = Some((ssid, Password::new(password.as_str())?));
                    self.run(host, steps)?;
                }
                Effect::WakeAt(at) => {
                    crate::native_model::best_effort(self.phone_set_wake(host, at))
                }
                Effect::Back => CommandSender::new(host, &mut [0; 64]).back()?,
            }
        }
        if server_failed {
            // Another page is served already (the X4 serves one at a time).
            let mut say = Messages::new()?;
            let words = say.copy(host, "wifi.phone.reason-busy")?;
            self.phone_step(host, |phone| phone.server_failed(&words))?;
            redraw = true;
        }
        if redraw {
            self.phone_refresh(host)?;
        }
        Ok(())
    }

    /// The rows as the setup is now, and a redraw.
    fn phone_refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if let Some(View::Phone(view)) = self.screens.top(PHONE_TOKEN) {
            Self::phone_build(host, view)?;
        }
        Self::redraw_now(host)
    }

    /// Opens the own network.
    fn phone_host(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(View::Phone(view)) = self.screens.top(PHONE_TOKEN) else {
            return Ok(());
        };
        let Some(own) = &view.own else {
            return Ok(());
        };
        self.request = self.request.wrapping_add(1).max(1);
        let mut scratch = [0; 512];
        let result = CommandSender::new(host, &mut scratch).wifi_host(
            self.request,
            own.ssid(),
            own.password().as_str(),
        );
        // The command buffer held the password.
        wipe(&mut scratch);
        result
    }

    /// Starts serving the page. `Busy` where the device serves one already;
    /// `Hidden` where the host takes no screen of the plugin to be showing.
    fn phone_serve(&mut self, host: &Host<'_>) -> Result<Serving, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        let net = self.net.as_ref().ok_or(Error::State)?;
        let server = Server::new(SERVER, PORT)?.max_body(MAX_BODY)?;
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        match net.serve(host, &server, &mut input, &mut out) {
            Ok(()) => Ok(Serving::Started),
            Err(Error::Busy) => Ok(Serving::Busy),
            Err(Error::State) => Ok(Serving::Hidden),
            Err(error) => Err(error),
        }
    }

    fn phone_stop(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(net) = &self.net else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        match net.stop_serving(host, SERVER, &mut input, &mut out) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            _ => Ok(()),
        }
    }

    /// Sets the one timer of the screen: work due at `at`.
    fn phone_set_wake(&mut self, host: &Host<'_>, at: u64) -> Result<(), Error> {
        let Some(work) = self.work(host)? else {
            return Ok(());
        };
        let spec = WorkSpec::at(WAKE_KEY, WAKE_WORK, at.max(1))?;
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        work.declare(host, &spec, &mut input, &mut out)
    }

    /// A request came to the page. Whatever it asks, an answer is sent and the
    /// callback ends well: nothing a phone on the network sends may fail the
    /// plugin.
    pub(super) fn phone_request(
        &mut self,
        host: &Host<'_>,
        request: &ServedRequest<'_>,
    ) -> Result<(), Error> {
        match self.phone_answer(host, request) {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => {
                let _ = self.phone_send(host, request.request(), 503, b"try again");
                Ok(())
            }
            Err(_) => {
                let _ = self.phone_send(host, request.request(), 500, b"failed");
                Ok(())
            }
        }
    }

    fn phone_answer(&mut self, host: &Host<'_>, request: &ServedRequest<'_>) -> Result<(), Error> {
        let storage = DataStorage::new(host);
        // The form: read into a buffer that is wiped, its file removed.
        let mut form = Vec::new();
        let mut path = String::new();
        if request.body_len().is_some() {
            let (mut a, mut b) = ([0u8; 10], [0u8; 10]);
            path = concat(&[
                ".received/",
                decimal(request.server(), &mut a),
                "/",
                decimal(request.request(), &mut b),
            ])?;
            form = match buffer(MAX_FORM + 1) {
                Ok(form) => form,
                Err(error) => {
                    // The body is not kept for want of a buffer.
                    let _ = storage.remove(&path);
                    return Err(error);
                }
            };
            let read = storage.read_at(&path, 0, &mut form).unwrap_or(0);
            form.truncate(read);
        }
        // A body over the most taken, or not text, counts as none.
        let text = core::str::from_utf8(&form)
            .ok()
            .filter(|text| text.len() <= MAX_FORM);
        let result = match self.screens.top(PHONE_TOKEN) {
            Some(View::Phone(_)) => {
                let (number, method, target) =
                    (request.request(), request.method(), request.target());
                let now = request.now_ms();
                self.phone_step(host, |phone| {
                    phone.request(now, number, method, target, text)
                })
            }
            _ => self.phone_send(host, request.request(), 503, b"not ready"),
        };
        wipe(&mut form);
        if !path.is_empty() {
            let _ = storage.remove(&path);
        }
        result
    }

    /// Answers request `number` with the page `page` is.
    fn phone_respond(
        &mut self,
        host: &Host<'_>,
        number: u32,
        status: u16,
        page: &Page,
    ) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let language = match Self::text(host, "core.language") {
            Ok(Some(language)) => language,
            Ok(None) | Err(Error::NotFound | Error::Denied | Error::NoSpace) => String::new(),
            Err(error) => return Err(error),
        };
        let (title, choose_one, other) = (
            say.copy(host, "wifi.phone.title")?,
            say.copy(host, "wifi.phone.choose-one")?,
            say.copy(host, "wifi.phone.other")?,
        );
        let (password, join, note, open) = (
            say.copy(host, "wifi.phone.password")?,
            say.copy(host, "wifi.phone.join")?,
            say.copy(host, "wifi.phone.page-note")?,
            say.copy(host, "wifi.network.open")?,
        );
        let texts = Texts {
            language: if language.is_empty() { "en" } else { &language },
            title: &title,
            choose_one: &choose_one,
            other: &other,
            password: &password,
            join: &join,
            note: &note,
            open: &open,
        };
        let html = match page {
            Page::Choose { again } => {
                let networks = self.read_networks(host)?;
                let mut found = Vec::new();
                found
                    .try_reserve_exact(networks.len())
                    .map_err(|_| Error::NoMemory)?;
                for network in &networks {
                    found.push(Found {
                        ssid: &network.ssid,
                        secured: network.secured,
                    });
                }
                let token = match self.screens.top(PHONE_TOKEN) {
                    Some(View::Phone(view)) => copy(view.phone.token())?,
                    _ => return Ok(()),
                };
                choose_page_fit(&texts, &found, *again, &token, MOST_PAGE)?
            }
            Page::Joining(ssid) => {
                let line = say.format(
                    host,
                    "wifi.phone.joining-page",
                    &[Arg::Text {
                        name: "ssid",
                        value: ssid,
                    }],
                )?;
                line_page(&texts, &line)?
            }
            Page::NotFound => not_found_page(&texts)?,
        };
        self.phone_send(host, number, status, html.as_bytes())
    }

    /// Sends `body` (inline where it fits, else from a file of the data
    /// folder) as the answer to request `number`.
    fn phone_send(
        &mut self,
        host: &Host<'_>,
        number: u32,
        status: u16,
        body: &[u8],
    ) -> Result<(), Error> {
        let net = self.net.as_ref().ok_or(Error::State)?;
        let mut response =
            Response::new(number, status)?.content_type("text/html; charset=utf-8")?;
        response = if body.len() <= MAX_INLINE_BODY {
            response.body(body)?
        } else {
            DataStorage::new(host)
                .write(PAGE_FILE, body)
                .map_err(|_| Error::Io)?;
            response.send_file(FileRoot::Data, PAGE_FILE)?
        };
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(SERVE_OUT)?);
        match net.respond(host, &response, &mut input, &mut out) {
            // Ended already (the phone went, or the 120 s passed).
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}
