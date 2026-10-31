//! Setting up Wi-Fi with a phone: the device looks for networks, then
//! opens a network of its own (`WifiCommand::Host`) and shows two QR
//! codes: one joins it, the other opens the page it serves there. On the
//! page the reader chooses the network and types its password with the
//! phone's keyboard; the device closes its own network and joins that
//! one, keeping the password once joined (as the Wi-Fi page does).
//!
//! It goes a step at a time: first the code that joins the own network,
//! with its name and password large; once a phone joined (it took an
//! address there, `WifiState::Hosting`'s guests), the code that opens the
//! page. *Next* goes on where the phone does not say so.
//!
//! The own network has a password drawn anew each time, of letters not
//! taken for one another (`random::READABLE`), shown with its code, so
//! only who sees the screen joins it. Joining fails: the own network opens
//! again, to try once more from the phone.

use alloc::format;
use alloc::string::{String, ToString};

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::device_name;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::qr::QrCode;
use paperpaw_sdk::random;
use paperpaw_sdk::replies::ServerNotice;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets;
use paperpaw_sdk::wifi::{self, WifiCommand, WifiState};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen, ServerResponse,
};

use crate::Shared;

const SERVER: Id = Id::new_static("wifi.phone-server");
const SERVER_EVENT: Event<ServerNotice> = Event::new("wifi.phone-server-event");
/// The page's port: the address alone opens it.
const PORT: u16 = 80;
/// The page around what it shows.
const PAGE: &str = include_str!("phone.html");
/// How long after the page answered the device leaves its own network,
/// so that the answer reaches the phone first.
const LEAVE_MS: u64 = 1500;
/// How long "Connected" shows before the screen closes.
const DONE_MS: u64 = 2500;
/// Longest form taken, in bytes.
const MAX_FORM: usize = 1024;
/// Without a request from the phone this long, the own network closes
/// (and the screen with it): it is not left open by mistake.
const IDLE_MS: u64 = 10 * 60 * 1000;
/// Letters of the own network's password: 62 bits, three groups of five.
const PASSWORD_LETTERS: usize = 15;
const _: () = assert!(PASSWORD_LETTERS >= random::readable_letters(45));

/// The step shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The phone joins the own network.
    Join,
    /// The phone opens the page.
    Open,
}

impl Step {
    /// What its button says (and does: the other step).
    fn button(self) -> &'static str {
        match self {
            Step::Join => "wifi.phone.next",
            Step::Open => "wifi.phone.back-step",
        }
    }
}

/// Not `Debug`: it holds a password.
enum State {
    /// Looking for networks, to list them on the page.
    Searching,
    /// The own network opens.
    Opening,
    /// The own network is open; the page's address once it is served.
    Hosting {
        page: Option<(String, Option<QrCode>)>,
    },
    /// The network chosen on the phone, joined once the answer went.
    Leaving {
        ssid: String,
        password: String,
    },
    Joining(String),
    Joined(String),
    Failed(String),
}

/// The own network: its name and password, and its code to join it.
struct Own {
    ssid: String,
    password: String,
    qr: Option<QrCode>,
}

pub(crate) struct PhoneScreen {
    policy: Shared,
    style: UiStyle,
    header: ui::Header,
    state: State,
    own: Option<Own>,
    /// Why joining failed last, said above the codes.
    problem: Option<String>,
    /// Whether the user had switched the radio on before (it stays on
    /// while this screen is open).
    manual: bool,
    started: bool,
    /// Drawn anew each time, in the page's form: a form sent from another
    /// page in the phone's browser does not have it.
    token: String,
    /// When the phone asked last (or the own network opened).
    active: paperpaw_sdk::Instant,
    step: Step,
    /// Whether a phone joined and the step went on by itself: once only,
    /// not again after the reader went back to the first.
    moved_on: bool,
    /// Where the step's button is, for taps.
    button: core::cell::Cell<Rect>,
}

impl PhoneScreen {
    pub(crate) fn new(env: &Env<'_>, policy: Shared) -> Self {
        let style = UiStyle::new(env);
        // Named as the device is, for the phone to find it.
        let own = env.random().readable(PASSWORD_LETTERS).map(|password| {
            let ssid = device_name::ssid(&env.device_name());
            let qr = QrCode::new(wifi::join_code(&ssid, &password).as_bytes());
            Own { ssid, password, qr }
        });
        let manual = policy.borrow().manual;
        Self {
            policy,
            header: ui::Header::new(env, &style, true),
            style,
            state: State::Searching,
            own,
            problem: None,
            manual,
            started: false,
            token: env.random().token(13).unwrap_or_default(),
            active: env.now(),
            step: Step::Join,
            moved_on: false,
            button: core::cell::Cell::new(Rect::new(0, 0, 0, 0)),
        }
    }

    /// The phone did something (or the own network opened): the idle
    /// time counts from now.
    fn touched(&mut self, cx: &mut Cx<'_, '_>) {
        self.active = cx.env().now();
        cx.wake_at(self.active.add_millis(IDLE_MS));
    }

    fn redraw(cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Opens the own network.
    fn host(&mut self, cx: &mut Cx<'_, '_>) {
        let Some(own) = &self.own else {
            return;
        };
        self.state = State::Opening;
        cx.command(Command::Wifi(WifiCommand::Host {
            ssid: own.ssid.clone(),
            password: own.password.clone(),
        }));
    }

    fn on_wifi(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(wifi) = env.wifi() else {
            return;
        };
        match (&self.state, &wifi.state) {
            (State::Opening, WifiState::Hosting { guests, .. }) => {
                self.state = State::Hosting { page: None };
                cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
                self.touched(cx);
                if *guests > 0 {
                    self.moved_on = true;
                    self.step = Step::Open;
                }
            }
            // A phone joined: on to opening the page, once.
            (State::Hosting { .. }, WifiState::Hosting { guests, .. })
                if *guests > 0 && !self.moved_on =>
            {
                self.moved_on = true;
                self.touched(cx);
                self.go(Step::Open, cx);
                return;
            }
            (State::Joining(ssid), WifiState::Online { ssid: joined, .. }) if ssid == joined => {
                self.state = State::Joined(joined.clone());
                cx.wake_at(env.now().add_millis(DONE_MS));
            }
            _ => return,
        }
        Self::redraw(cx);
    }

    fn on_server(&mut self, notice: ServerNotice, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match notice {
            ServerNotice::Started { address, .. } => {
                if let State::Hosting { page } = &mut self.state {
                    // Without the port where it is the usual one.
                    let address = address.strip_suffix(":80").unwrap_or(&address);
                    let url = format!("http://{address}/");
                    let qr = QrCode::new(url.as_bytes());
                    *page = Some((url, qr));
                }
            }
            ServerNotice::Failed { reason, .. } => {
                self.state = State::Failed(
                    env.i18n()
                        .format("wifi.phone.failed", &[("reason", Arg::Str(&reason))]),
                );
            }
            ServerNotice::Request {
                request,
                method,
                target,
                body_file,
                ..
            } => {
                self.touched(cx);
                let path = target.split_once('?').map_or(target.as_str(), |(p, _)| p);
                let form = body_file
                    .as_deref()
                    .and_then(|file| paperpaw_sdk::read_file(env.storage(), file, MAX_FORM).ok())
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                let token = self.token.as_str();
                let (status, body, chosen) = match (method.as_str(), path) {
                    // Only the page's own form: not one another page sent.
                    ("POST", "/join")
                        if token.is_empty()
                            || form.as_deref().and_then(|f| field(f, "t")).as_deref()
                                != Some(token) =>
                    {
                        (403, choose_page(&env, true, token), None)
                    }
                    ("POST", "/join") => match form.as_deref().and_then(chosen) {
                        Some((ssid, password)) => {
                            let text = env
                                .i18n()
                                .format("wifi.phone.joining-page", &[("ssid", Arg::Str(&ssid))]);
                            let body = format!("<h1>{}</h1>", escape(&text));
                            (200, page(&env, &body), Some((ssid, password)))
                        }
                        None => (400, choose_page(&env, true, token), None),
                    },
                    ("GET", "/") => (200, choose_page(&env, false, token), None),
                    _ => (404, page(&env, "<h1>404</h1>"), None),
                };
                cx.command(Command::respond(ServerResponse::new(
                    request,
                    status,
                    "text/html; charset=utf-8",
                    body.into_bytes(),
                )));
                let Some((ssid, password)) = chosen else {
                    return;
                };
                self.state = State::Leaving { ssid, password };
                cx.wake_at(env.now().add_millis(LEAVE_MS));
            }
            _ => return,
        }
        Self::redraw(cx);
    }

    fn on_failed(
        &mut self,
        ssid: &str,
        reason: paperpaw_sdk::wifi::JoinFailure,
        cx: &mut Cx<'_, '_>,
    ) {
        let env = *cx.env();
        // The own network could not open.
        if matches!(self.state, State::Opening)
            && self.own.as_ref().is_some_and(|own| own.ssid == ssid)
        {
            self.state = State::Failed(env.i18n().tr("wifi.phone.not-opened").into());
            self.touched(cx);
            return Self::redraw(cx);
        }
        if !matches!(&self.state, State::Joining(joining) if joining == ssid) {
            return;
        }
        self.problem = Some(crate::failure_text(&env, ssid, reason));
        self.host(cx);
        Self::redraw(cx);
    }

    /// Closes the screen: the page no longer served, the own network
    /// closed, the radio as the user left it.
    fn close(&mut self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::StopServing(SERVER));
        if cx.env().wifi().is_some_and(|w| w.state.is_hosting()) {
            cx.command(Command::Wifi(WifiCommand::Leave));
        }
        let mut policy = self.policy.borrow_mut();
        policy.manual = self.manual;
        policy.phone = false;
        policy.goal = crate::policy::Goal::Nothing;
        drop(policy);
        cx.command(Command::Back);
    }

    /// The page's address and code, once served.
    fn page(&self) -> Option<&(String, Option<QrCode>)> {
        match &self.state {
            State::Hosting { page } => page.as_ref(),
            _ => None,
        }
    }

    /// Goes to `step`, where it is another.
    fn go(&mut self, step: Step, cx: &mut Cx<'_, '_>) {
        if self.step != step {
            self.step = step;
            Self::redraw(cx);
        }
    }

    /// The step shown, its title and line at the top, its button at the
    /// bottom: joining shows the code to join and the network's name and
    /// password large; opening the page's code and its address.
    fn draw_step(&self, ui: &mut ui::Ui<'_>, area: Rect, own: &Own) {
        let env = ui.env();
        let style = self.style;
        let Some(font) = ui.font() else {
            return;
        };
        let margin = style.margin();
        let gap = style.gap();
        // The button at the bottom keeps the margin from the screen's edge.
        let inner = Rect::new(
            margin,
            area.y,
            area.width.saturating_sub(2 * margin),
            area.height.saturating_sub(margin),
        );
        let mut stack = ui::Stack::new(inner, gap);
        let line = widgets::line_height(env, style.text());
        let small = widgets::line_height(env, style.small());
        let width = f32::from(inner.width);
        // Lines of `text` wrapped, together one part of the stack.
        let lines = |ui: &mut ui::Ui<'_>,
                     stack: &mut ui::Stack,
                     text: &str,
                     size: f32,
                     high: u16,
                     ink: Color| {
            let wrapped = font.wrap(size, text, width);
            let block = stack.take(high * wrapped.len() as u16);
            for (i, range) in wrapped.into_iter().enumerate() {
                let row = Rect::new(block.x, block.y + i as u16 * high, block.width, high);
                ui::Label::new(text[range].trim())
                    .px(size)
                    .color(ink)
                    .draw_from(ui, f32::from(row.x), row);
            }
        };
        if let Some(problem) = &self.problem {
            lines(ui, &mut stack, problem, style.small(), small, Color::BLACK);
        }
        let (number, title, note) = match self.step {
            Step::Join => (1, "wifi.phone.join-title", "wifi.phone.join-note"),
            Step::Open => (2, "wifi.phone.open-title", "wifi.phone.open-note"),
        };
        let title = format!("{number}. {}", env.i18n().tr(title));
        lines(ui, &mut stack, &title, style.text(), line, Color::BLACK);
        let label = env.i18n().tr(self.step.button());
        let card = match self.step {
            Step::Join => join_card_height(env, &style) + gap,
            Step::Open => 0,
        };
        // Without touch, where the button leaves the card too little room
        // (the largest text on a small screen), none: the row of hints
        // under the screen names the button that goes on.
        let mut button_height = ui::button_height_for(env, &style, inner.width, label);
        let crowded = stack.rest().height < button_height + card + 2 * gap;
        if crowded && !env.device().input.touch && env.caps().button_row {
            button_height = 0;
        }
        // The note as large as the room above the button (and the card)
        // holds it, the small size at most; none where it does not fit
        // (the largest text on a small screen): there is no room for the
        // code it speaks of either.
        let below = button_height + gap + card;
        let room = stack.rest().height.saturating_sub(below + gap);
        let note = env.i18n().tr(note);
        let fits = |size: f32| {
            let high = widgets::line_height(env, size);
            high * font.wrap(size, note, width).len() as u16 <= room
        };
        if let Some(size) = widgets::largest_size(style.small(), style.small() * 0.6, fits) {
            let high = widgets::line_height(env, size);
            lines(ui, &mut stack, note, size, high, style.faint(ui::Ink::NOTE));
        }
        let button = if button_height > 0 {
            let button = stack.take_bottom(button_height);
            ui.button(button, label, false, false);
            button
        } else {
            Rect::default()
        };
        self.button.set(button);
        match self.step {
            Step::Join => self.draw_join(ui, &mut stack, own),
            Step::Open => self.draw_open(ui, &stack, area),
        }
    }

    /// The step to join, below its title: the network's name and password
    /// large on a card above the button, to be read and typed; its code
    /// above them.
    fn draw_join(&self, ui: &mut ui::Ui<'_>, stack: &mut ui::Stack, own: &Own) {
        let env = ui.env();
        let style = self.style;
        let Some(font) = ui.font() else {
            return;
        };
        let gap = style.gap();
        let small = widgets::line_height(env, style.small());
        // The name and the password large, to be read and typed.
        let big = style.text() * 1.2;
        let tall = widgets::line_height(env, big);
        let card = stack.take_bottom(join_card_height(env, &style));
        let inside = ui.card(card, widgets::Frame::Line);
        let password = random::grouped(&own.password);
        let x = f32::from(inside.x);
        let mut y = inside.y;
        for (key, value) in [
            ("wifi.phone.name", own.ssid.as_str()),
            ("wifi.phone.password", password.as_str()),
        ] {
            let row = Rect::new(inside.x, y, inside.width, small);
            ui::Label::new(env.i18n().tr(key))
                .px(style.small())
                .color(style.faint(ui::Ink::NOTE))
                .draw_from(ui, x, row);
            // Smaller than the small size where only that fits it whole
            // (the largest text): it is to be typed.
            let (size, value) = widgets::fit_line(
                &font,
                big,
                style.small() * 0.7,
                value,
                f32::from(inside.width),
            );
            let row = Rect::new(inside.x, y + small, inside.width, tall);
            ui::Label::new(&value)
                .px(size)
                .color(Color::BLACK)
                .draw_from(ui, x, row);
            y += small + tall + gap;
        }
        let rest = stack.rest();
        match own.qr.as_ref() {
            Some(qr) => {
                ui::draw_qr(ui.canvas(), qr, rest);
            }
            None => draw_opening(ui, rest),
        }
    }

    /// The step to open the page, below its title: its code with the
    /// address below, across the screen (its own margins).
    fn draw_open(&self, ui: &mut ui::Ui<'_>, stack: &ui::Stack, area: Rect) {
        let env = ui.env();
        let style = self.style;
        // The address's own margins: across the screen.
        let rest = stack.rest();
        let full = Rect::new(area.x, rest.y, area.width, rest.height);
        match self.page() {
            Some((url, qr)) => ui::draw_address(
                ui.canvas(),
                env,
                &style,
                full,
                url,
                qr.as_ref(),
                env.i18n().tr("wifi.phone.type-address"),
                None,
            ),
            None => draw_opening(ui, full),
        }
    }
}

/// The height of the card with the reader's network and its password.
fn join_card_height(env: &Env<'_>, style: &UiStyle) -> u16 {
    let small = widgets::line_height(env, style.small());
    let tall = widgets::line_height(env, style.text() * 1.2);
    2 * style.margin() + 2 * (small + tall) + style.gap()
}

/// That the reader's own network is opening, in `room` below a step: as a
/// message, or in the small size where the room is too short for that.
fn draw_opening(ui: &mut ui::Ui<'_>, room: Rect) {
    let (env, style) = (ui.env(), ui.style());
    let text = env.i18n().tr("wifi.phone.opening");
    if ui::message_height(ui.env(), ui.style(), room.width, text) <= room.height {
        return ui.message(room, text);
    }
    let Some(font) = ui.font() else {
        return;
    };
    let margin = style.margin();
    let width = room.width.saturating_sub(2 * margin);
    let line = font.line_metrics(style.small()).line_height as u16;
    let lines = font.wrap(style.small(), text, f32::from(width));
    let high = line * lines.len() as u16;
    let mut y = room.y + room.height.saturating_sub(high) / 2;
    for range in lines {
        let part = text[range].trim();
        let x = f32::from(room.x + margin)
            + (f32::from(width) - font.measure(style.small(), part)).max(0.0) / 2.0;
        let at = Rect::new(room.x, y, room.width, line);
        ui::Label::new(part)
            .px(style.small())
            .color(Color::BLACK)
            .draw_from(ui, x, at);
        y += line;
    }
}

/// Field `name` of a form sent, decoded.
fn field(form: &str, name: &str) -> Option<String> {
    form.split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| paperpaw_sdk::web::percent_decode(&value.replace('+', " ")))
}

/// The network and password sent from the page: a network listed, or
/// one named in the field below them.
fn chosen(form: &str) -> Option<(String, String)> {
    let field = |name: &str| field(form, name);
    let other = field("other").unwrap_or_default();
    let ssid = if other.trim().is_empty() {
        field("ssid")?
    } else {
        String::from(other.trim())
    };
    (!ssid.is_empty() && ssid.len() <= 32).then(|| (ssid, field("password").unwrap_or_default()))
}

/// `text` safe in HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    paperpaw_sdk::xml::escape(text, &mut out);
    out
}

/// A page with `body`.
fn page(env: &Env<'_>, body: &str) -> String {
    PAGE.replace("{lang}", env.i18n().language())
        .replace("{title}", &escape(env.i18n().tr("wifi.phone.title")))
        .replace("{body}", body)
}

/// The page to choose a network: those found, the strongest first, a
/// field for another one, the password. `again` after a form without a
/// network.
fn choose_page(env: &Env<'_>, again: bool, token: &str) -> String {
    let tr = |key| escape(env.i18n().tr(key));
    let mut body = format!("<h1>{}</h1>", tr("wifi.phone.title"));
    if again {
        body.push_str(&format!(
            "<p class=\"note\">{}</p>",
            tr("wifi.phone.choose-one")
        ));
    }
    body.push_str(&format!(
        "<form method=\"post\" action=\"/join\"><input type=\"hidden\" name=\"t\" value=\"{}\">",
        escape(token)
    ));
    let networks = env
        .wifi()
        .map(|w| w.networks.as_slice())
        .unwrap_or_default();
    for network in networks {
        let lock = if network.secured { "" } else { " ·" };
        let open = if network.secured {
            String::new()
        } else {
            tr("wifi.network.open")
        };
        body.push_str(&format!(
            "<label class=\"network\"><input type=\"radio\" name=\"ssid\" value=\"{name}\"> {name}<small>{open}{lock}</small></label>",
            name = escape(&network.ssid),
        ));
    }
    body.push_str(&format!(
        "<p><label>{}<input type=\"text\" name=\"other\" autocomplete=\"off\"></label></p>\
         <p><label>{}<input type=\"password\" name=\"password\" autocomplete=\"off\"></label></p>\
         <button type=\"submit\">{}</button></form><p class=\"note\">{}</p>",
        tr("wifi.phone.other"),
        tr("wifi.phone.password"),
        tr("wifi.phone.join"),
        tr("wifi.phone.page-note"),
    ));
    page(env, &body)
}

impl Screen for PhoneScreen {
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        let steps = matches!(self.state, State::Opening | State::Hosting { .. });
        match self.step {
            _ if !steps => paperpaw_sdk::Hint::only_back(action),
            _ if *action == actions::CONFIRM => {
                paperpaw_sdk::Hint::Text(env.i18n().tr(self.step.button()).into())
            }
            Step::Join if *action == actions::NEXT_PAGE => paperpaw_sdk::Hint::Default,
            Step::Open if *action == actions::PREV_PAGE => paperpaw_sdk::Hint::Default,
            _ => paperpaw_sdk::Hint::only_back(action),
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        canvas.clear(Color::WHITE);
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let title = env.i18n().tr("wifi.phone.title");
        self.header.draw(&mut ui, title, None, None);
        let display = env.device().display;
        let top = self.header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let message = |key| env.i18n().tr(key).to_string();
        let text = match (&self.state, &self.own) {
            (_, None) => message("wifi.phone.no-random"),
            (State::Searching, _) => message("wifi.state.searching"),
            (State::Opening, _) if self.problem.is_none() => message("wifi.phone.opening"),
            (State::Opening | State::Hosting { .. }, Some(own)) => {
                return self.draw_step(&mut ui, area, own);
            }
            (State::Leaving { ssid, .. } | State::Joining(ssid), _) => env
                .i18n()
                .format("wifi.state.joining", &[("ssid", Arg::Str(ssid))]),
            (State::Joined(ssid), _) => env
                .i18n()
                .format("wifi.phone.joined", &[("ssid", Arg::Str(ssid))]),
            (State::Failed(text), _) => text.clone(),
        };
        ui.message(area, &text);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.started {
            return;
        }
        self.started = true;
        if self.own.is_none() {
            Self::redraw(cx);
            return;
        }
        // On while this screen is open, whatever else wants it; this
        // screen says what it does.
        let mut policy = self.policy.borrow_mut();
        policy.manual = true;
        policy.phone = true;
        policy.goal = crate::policy::Goal::Nothing;
        drop(policy);
        let on = cx.env().wifi().is_some_and(|w| w.state.is_on());
        if !on {
            cx.command(Command::Wifi(WifiCommand::On));
        }
        cx.command(Command::Wifi(WifiCommand::Scan));
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Left alone: closed.
        let waiting = matches!(
            self.state,
            State::Opening | State::Hosting { .. } | State::Failed(_)
        );
        if waiting && env.now() >= self.active.add_millis(IDLE_MS) {
            return self.close(cx);
        }
        match core::mem::replace(&mut self.state, State::Searching) {
            State::Leaving { ssid, password } => {
                cx.command(Command::StopServing(SERVER));
                let state = env.wifi().map(|w| w.state.clone()).unwrap_or_default();
                let mut policy = self.policy.borrow_mut();
                policy.pending = Some((ssid.clone(), password.clone()));
                let commands = policy.choose(&state, &ssid, &password);
                drop(policy);
                for command in commands {
                    cx.command(Command::Wifi(command));
                }
                self.state = State::Joining(ssid);
                Self::redraw(cx);
            }
            State::Joined(ssid) => {
                self.state = State::Joined(ssid);
                self.close(cx);
            }
            state => self.state = state,
        }
        // The screen's one timer: the idle time again, where it counts.
        if matches!(
            self.state,
            State::Opening | State::Hosting { .. } | State::Failed(_)
        ) {
            cx.wake_at(self.active.add_millis(IDLE_MS));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let steps = matches!(self.state, State::Opening | State::Hosting { .. });
        if *action == actions::BACK {
            self.close(cx);
            Handled::Yes
        } else if steps && *action == actions::CONFIRM {
            let other = match self.step {
                Step::Join => Step::Open,
                Step::Open => Step::Join,
            };
            self.go(other, cx);
            Handled::Yes
        } else if steps && *action == actions::NEXT_PAGE {
            self.go(Step::Open, cx);
            Handled::Yes
        } else if steps && *action == actions::PREV_PAGE {
            self.go(Step::Join, cx);
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
        let steps = matches!(self.state, State::Opening | State::Hosting { .. });
        if steps && gesture.kind == GestureKind::Tap && self.button.get().contains(gesture.start) {
            let other = match self.step {
                Step::Join => Step::Open,
                Step::Open => Step::Join,
            };
            self.go(other, cx);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        use paperpaw_sdk::builtin::events;
        if *event == events::WIFI_SCANNED && matches!(self.state, State::Searching) {
            self.host(cx);
            Self::redraw(cx);
        } else if *event == events::WIFI_CHANGED {
            self.on_wifi(cx);
        } else if let Some(failed) = events::WIFI_FAILED.read(event, params) {
            let (ssid, reason) = (failed.ssid.clone(), failed.reason);
            self.on_failed(&ssid, reason, cx);
        } else if let Some(notice) = SERVER_EVENT.read(event, params)
            && *notice.server() == SERVER
        {
            self.on_server(notice, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_names_a_network_and_its_password() {
        assert_eq!(
            chosen("ssid=Home+Net&other=&password=p%26ss+w"),
            Some((String::from("Home Net"), String::from("p&ss w")))
        );
        // Another network named wins over one listed.
        assert_eq!(
            chosen("ssid=Home&other=+Cafe+&password="),
            Some((String::from("Cafe"), String::new()))
        );
        assert_eq!(chosen("other=&password=x"), None);
        assert_eq!(chosen(&format!("other={}", "n".repeat(33))), None);
    }
}
