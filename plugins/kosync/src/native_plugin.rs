//! The interface-7 progress sync: the settings, the account screen, the sync
//! engine (`native_sync`) carried out through the network, secrets, work and
//! user-interface services, and the offer to go where another device is.
//! The book identity and document position adapters are in `native_backing`.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    device::{Device, WifiState},
    events::{AnswerOutcome, Answered, Event, Incoming, ReadingView},
    formatting::Arg,
    net::{Header, HttpDone, Net, REPLY, REQUEST_INPUT, Reason, Request, RequestId},
    registrations,
    reply::Reply,
    secrets::Secrets,
    settings::{self, SettingValue},
    ui::{Instance, Notify, Open, Param, Question, RequestId as AskId, TextMode, TextWrite, Ui},
    work::{Moment, Work, WorkSpec},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    online,
    text::{concat, copy},
};

use crate::declarations::{
    ACCOUNT, BOOK_CLOSED, BOOK_OPENED, HTTP_DONE, PAGE_TURNED, PUSH_WORK, RESUME, SETTING_CHANGED,
    STARTED, SUSPEND, SYNC_NOW, WORK_ALLOWED, WORK_DUE, WORK_REFUSED,
};
use crate::native_account::{Account, Field, Message, Row, Todo};
use crate::native_backing::Backing;
use crate::native_flow::{Call, Pending, Purpose, Table, create_body};
use crate::native_model::{self, MAX_ANSWER, Remote, Secret, failure, percent_text, progress_body};
use crate::native_store as store;
use crate::native_sync::{
    Action, Answer, Answered as Chose, Ctx, Engine, Notice, Offer, PUSH_EVERY_MS, Position,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/kosync_declarations.rs"));
}

const USER: &str = "kosync.user";
const AUTO: &str = "kosync.auto";
const SERVER: &str = "kosync.server";
/// Offers waiting for the user's answer, at most (one a book, a few books).
const MOST_OFFERS: usize = 4;
/// The work that sends the progress comes due after this long at the latest.
const PUSH_WITHIN_MS: u32 = 30 * 60 * 1000;
/// Digits of a `u32`.
const DIGITS: usize = 10;

#[derive(Debug, Default)]
enum PushWork {
    #[default]
    Idle,
    Declared,
    Denied,
    Full(u64),
}

/// The screen: the account and the rows it shows.
struct View {
    account: Account,
    list: ItemList,
    note: String,
}

impl core::fmt::Debug for View {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("View")
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

/// An offer shown, and the question that asks it.
#[derive(Debug)]
struct Asked {
    request: AskId,
    offer: Offer,
}

/// The interface-7 progress sync.
#[derive(Debug, Default)]
pub struct NativeKosync {
    ui: Option<Ui>,
    net: Option<Net>,
    secrets: Option<Secrets>,
    work: Option<Work>,
    device: Option<Device>,
    /// The book's fingerprint and places, where the device offers the services.
    backing: Backing,
    /// The services were tried; `net` is `None` where the device has no network.
    opened: bool,
    screens: Screens<View>,
    engine: Option<Engine>,
    table: Table,
    /// The account's key, read from the secrets once.
    key: Option<Secret>,
    key_read: bool,
    /// The text being typed on the screen, and which one.
    typing: Option<(AskId, Field)>,
    offers: Vec<Asked>,
    /// Offline mode held *Sign in* (`false`) or *Create account* (`true`)
    /// back: the offer to go online for it, once until the reader chooses it
    /// again (as Beta 3's account screen did).
    online: online::Offer<bool>,
    /// The server the account belongs to; another one signs out.
    server: Option<String>,
    push_work: PushWork,
}

impl Plugin for NativeKosync {
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
        let event = incoming.decode()?;
        match event {
            Event::Started { .. } if token == STARTED => self.started(host),
            Event::SettingChanged { key, .. } if token == SETTING_CHANGED => {
                self.setting_changed(host, key)
            }
            Event::Book { kind, reading, .. } if token == BOOK_OPENED || token == BOOK_CLOSED => {
                self.book(host, kind, &reading)
            }
            Event::PageTurned(reading) if token == PAGE_TURNED => {
                self.with_engine(host, reading.now_ms(), |engine, ctx, out| {
                    engine.page_turned(ctx, position(&reading)?, out)
                })
            }
            Event::Suspend { now_ms, .. } if token == SUSPEND => {
                self.with_engine(host, now_ms, |engine, ctx, out| engine.send_now(ctx, out))
            }
            Event::Resume { now_ms, .. } if token == RESUME => {
                self.with_engine(host, now_ms, |engine, ctx, out| engine.resumed(ctx, out))
            }
            Event::WorkDue(due) if token == WORK_DUE && due.key() == PUSH_WORK => {
                self.push_work = PushWork::Idle;
                self.with_engine(host, due.now_ms(), |engine, ctx, out| {
                    engine.send_now(ctx, out)
                })
            }
            Event::WorkRefused(refused) if token == WORK_REFUSED && refused.key() == PUSH_WORK => {
                self.push_work = PushWork::Idle;
                Ok(())
            }
            Event::WorkAllowed { now_ms, .. } if token == WORK_ALLOWED => {
                self.push_work = PushWork::Idle;
                self.with_engine(host, now_ms, |engine, ctx, out| engine.allowed(ctx, out))
            }
            Event::ActionRun { now_ms, .. } if token == SYNC_NOW => self.sync_now(host, now_ms),
            Event::HttpDone(done) if token == HTTP_DONE => self.http_done(host, &done),
            Event::Answered { answer, now_ms, .. } => self.answered(host, now_ms, answer),
            Event::ScreenOpened { instance, .. } if token == ACCOUNT => {
                self.opened_account(host, instance)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == ACCOUNT => self.screens.shown(token, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = say.copy(host, "kosync.group")?;
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
            self.chosen(host, token, index)?;
        }
        Ok(ItemList::handled(step))
    }
}

/// The position a reading event tells.
fn position(reading: &ReadingView<'_>) -> Result<Position, Error> {
    Position::new(
        reading.path(),
        (reading.section(), reading.offset()),
        f64::from(reading.progress_ppm()) / 1_000_000.0,
    )
}

/// `n` in decimal, written into `buf`.
fn digits(mut n: u32, buf: &mut [u8; DIGITS]) -> &str {
    let mut at = DIGITS;
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    core::str::from_utf8(&buf[at..]).unwrap_or("0")
}

impl NativeKosync {
    /// Opens the services once. A device without a network has no `net`; one
    /// without secrets has no `secrets` (the account's key is then not kept).
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.server.is_none() {
            self.server = Some(Self::server(host)?);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if self.opened {
            return Ok(());
        }
        if self.net.is_none() {
            self.net = optional(Net::open(host))?;
        }
        if self.secrets.is_none() {
            self.secrets = optional(Secrets::open(host))?;
        }
        if self.work.is_none() {
            self.work = optional(Work::open(host))?;
        }
        if self.device.is_none() {
            self.device = optional(Device::open(host))?;
        }
        self.backing = Backing::open(host)?;
        self.opened = true;
        Ok(())
    }

    fn text_setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
        let mut scratch = [0; 1100];
        match settings::read_setting(host, key, &mut scratch)? {
            SettingValue::Text(text) => copy(text),
            _ => Ok(String::new()),
        }
    }

    fn auto(host: &Host<'_>) -> Result<bool, Error> {
        let mut scratch = [0; 64];
        Ok(matches!(
            settings::read_setting(host, AUTO, &mut scratch)?,
            SettingValue::Bool(true)
        ))
    }

    /// The server's base URL as set.
    fn server(host: &Host<'_>) -> Result<String, Error> {
        native_model::normalize_server(&Self::text_setting(host, SERVER)?)
    }

    /// Reads the account's key among the secrets, once.
    fn read_key(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.key_read {
            return Ok(());
        }
        self.key_read = true;
        if let Some(secrets) = &self.secrets {
            self.key = match store::read_key(secrets, host) {
                Ok(key) => key,
                Err(Error::NoMemory) => {
                    self.key_read = false;
                    return Err(Error::NoMemory);
                }
                Err(_) => None,
            };
        }
        Ok(())
    }

    /// What the host knows now, for the engine.
    fn ctx(&mut self, host: &Host<'_>, now_ms: u64) -> Result<Ctx, Error> {
        self.ensure(host)?;
        self.read_key(host)?;
        let signed_in = self.key.is_some() && !Self::text_setting(host, USER)?.is_empty();
        let mut ctx = Ctx::new(now_ms);
        ctx.network = self.net.is_some();
        ctx.auto = Self::auto(host)?;
        ctx.signed_in = signed_in;
        ctx.radio_on_anyway = ctx.active()
            && self
                .engine
                .as_ref()
                .is_none_or(|engine| engine.push_due(now_ms))
            && self.radio_on_anyway(host);
        Ok(ctx)
    }

    /// Whether the device is in a network already (sending turns on no radio);
    /// where that is not known, it is.
    fn radio_on_anyway(&self, host: &Host<'_>) -> bool {
        let Some(device) = &self.device else {
            return true;
        };
        let (Ok(mut input), Ok(mut out)) = (buffer(SERVICE_BUFFER), buffer(SERVICE_BUFFER)) else {
            return true;
        };
        match device.wifi(host, &mut input, &mut out) {
            Ok(info) => matches!(info.state(), WifiState::Online | WifiState::Hosting),
            Err(_) => true,
        }
    }

    /// The engine, made at first use with this device's id.
    fn engine(&mut self, host: &Host<'_>, now_ms: u64) -> Result<&mut Engine, Error> {
        if self.engine.is_none() {
            let mut seed = [0u8; 32];
            let drawn = self.secrets.as_ref().is_some_and(|secrets| {
                let (Ok(mut input), Ok(mut out)) = (buffer(128), buffer(512)) else {
                    return false;
                };
                let got = secrets.random(host, 16, &mut input, &mut out).map(|r| {
                    if let Some(part) = seed.get_mut(..16) {
                        part.copy_from_slice(r.bytes());
                    }
                });
                wipe(&mut out);
                got.is_ok()
            });
            if !drawn {
                // Not drawn: the clock stands in; the id only tells devices apart.
                if let Some(part) = seed.get_mut(..8) {
                    part.copy_from_slice(&now_ms.to_le_bytes());
                }
            }
            let id = store::device_id(host, &seed)?;
            self.engine = Some(Engine::new(&id)?);
        }
        self.engine.as_mut().ok_or(Error::State)
    }

    /// Runs `call` on the engine with a fresh context, then carries out the
    /// actions it gave. After `NoMemory` the actions are dropped.
    fn with_engine(
        &mut self,
        host: &Host<'_>,
        now_ms: u64,
        call: impl FnOnce(&mut Engine, Ctx, &mut Vec<Action>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let ctx = self.ctx(host, now_ms)?;
        let mut out = Vec::new();
        call(self.engine(host, now_ms)?, ctx, &mut out)?;
        self.run(host, ctx, out)
    }

    fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure(host)?;
        self.read_key(host)?;
        self.server = Some(Self::server(host)?);
        self.move_old_key(host)
    }

    /// A key an earlier version kept in `account.ini` moves to the secrets; the
    /// file goes once it is there (or the secrets hold a key already).
    fn move_old_key(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(old) = store::old_key(host)? else {
            return Ok(());
        };
        if self.key.is_none() {
            let Some(secrets) = &self.secrets else {
                // Kept where it is: moved at a start that has secrets.
                return Ok(());
            };
            let mut say = Messages::new()?;
            let label = say.copy(host, "kosync.secret")?;
            let kept = store::keep_key(secrets, host, &label, old.as_str());
            match kept {
                Ok(()) => self.key = Some(old),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(()),
            }
        }
        store::forget_old(host);
        Ok(())
    }

    /// The setting `key` changed: another server signs the account out.
    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key != SERVER && !key.is_empty() {
            return Ok(());
        }
        let unknown_change = self.server.is_none() && key == SERVER;
        self.ensure(host)?;
        self.read_key(host)?;
        let now = Self::server(host)?;
        let before = self.server.replace(copy(&now)?);
        let signed_in = self.key.is_some() && !Self::text_setting(host, USER)?.is_empty();
        if (unknown_change || before.as_deref() != Some(now.as_str())) && signed_in {
            self.sign_out(host)?;
        }
        Ok(())
    }

    fn book(
        &mut self,
        host: &Host<'_>,
        kind: paperpaw_sdk::native::events::BookKind,
        reading: &ReadingView<'_>,
    ) -> Result<(), Error> {
        use paperpaw_sdk::native::events::BookKind;
        let now = reading.now_ms();
        match kind {
            BookKind::Opened => self.with_engine(host, now, |engine, ctx, out| {
                engine.book_opened(ctx, position(reading)?, out)
            }),
            BookKind::Closed => self.with_engine(host, now, |engine, ctx, out| {
                engine.book_closed(ctx, position(reading)?, out)
            }),
            _ => Ok(()),
        }
    }

    /// *Sync progress*: the server's first; no account opens the account screen.
    fn sync_now(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        let ctx = self.ctx(host, now_ms)?;
        let mut out = Vec::new();
        if !self.engine(host, now_ms)?.sync_now(ctx, &mut out)? {
            let mut scratch = [0; 128];
            return CommandSender::new(host, &mut scratch).open(Open::screen("kosync.account")?);
        }
        self.run(host, ctx, out)
    }

    // ---- carrying out what the engine decided --------------------------------

    fn run(&mut self, host: &Host<'_>, ctx: Ctx, actions: Vec<Action>) -> Result<(), Error> {
        for action in actions {
            match action {
                Action::Fetch {
                    path,
                    local,
                    manual,
                } => self.fetch(host, ctx, &path, local, manual)?,
                Action::Send(position) => self.send(host, ctx, &position)?,
                Action::Offer(offer) => self.offer(host, offer)?,
                Action::Notice(notice) => self.notice(host, notice)?,
                Action::PushLater => self.push_later(host, ctx.now_ms)?,
            }
        }
        Ok(())
    }

    fn credentials(&self, host: &Host<'_>) -> Result<Option<(String, Secret)>, Error> {
        let Some(key) = &self.key else {
            return Ok(None);
        };
        let user = Self::text_setting(host, USER)?;
        if user.is_empty() {
            return Ok(None);
        }
        Ok(Some((user, Secret::copy(key.as_str())?)))
    }

    fn fetch(
        &mut self,
        host: &Host<'_>,
        ctx: Ctx,
        path: &str,
        local: f64,
        manual: bool,
    ) -> Result<(), Error> {
        let started = self.start_fetch(host, path, local, manual);
        match started {
            Ok(true) => Ok(()),
            // No fingerprint, no room or no network now: asked again at the next chance.
            Ok(false) | Err(Error::Busy | Error::NoMemory) => {
                if let Some(engine) = &mut self.engine {
                    engine.not_fetched(path);
                }
                if manual && !ctx.network {
                    self.notice_key(host, "kosync.no-network", 0)?;
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn start_fetch(
        &mut self,
        host: &Host<'_>,
        path: &str,
        local: f64,
        manual: bool,
    ) -> Result<bool, Error> {
        let Some(digest) = self.backing.digest(host, path)? else {
            return Ok(false);
        };
        let Some((user, key)) = self.credentials(host)? else {
            return Ok(false);
        };
        let target = concat(&["/syncs/progress/", &digest])?;
        let call = Call::server(
            &Self::server(host)?,
            "GET",
            &target,
            None,
            Some((&user, key.as_str())),
        )?;
        drop(key);
        let pending = Pending {
            id: RequestId::new(1)?,
            purpose: Purpose::Pull,
            path: copy(path)?,
            local,
            manual,
        };
        self.start(host, &call, pending)?;
        Ok(true)
    }

    fn send(&mut self, host: &Host<'_>, _ctx: Ctx, position: &Position) -> Result<(), Error> {
        self.push_work = PushWork::Idle;
        match self.start_send(host, position) {
            Ok(true) => Ok(()),
            Ok(false) | Err(Error::Busy | Error::NoMemory) => {
                if let Some(engine) = &mut self.engine {
                    engine.not_sent();
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn start_send(&mut self, host: &Host<'_>, position: &Position) -> Result<bool, Error> {
        let Some(digest) = self.backing.digest(host, &position.path)? else {
            return Ok(false);
        };
        let Some((user, key)) = self.credentials(host)? else {
            return Ok(false);
        };
        // The book gives none (a format without XPointers): the progress alone.
        let xpointer = self
            .backing
            .xpointer(host, &position.path, position.at)?
            .unwrap_or_default();
        let progress = position.progress;
        let model = self.model(host)?;
        let id = match &self.engine {
            Some(engine) => copy(engine.device_id())?,
            None => String::new(),
        };
        let body = progress_body(&digest, &xpointer, progress, &model, &id)?;
        let call = Call::server(
            &Self::server(host)?,
            "PUT",
            "/syncs/progress",
            Some(body),
            Some((&user, key.as_str())),
        )?;
        drop(key);
        let pending = Pending {
            id: RequestId::new(1)?,
            purpose: Purpose::Push,
            path: String::new(),
            local: 0.0,
            manual: false,
        };
        self.start(host, &call, pending)?;
        Ok(true)
    }

    /// The device's name, as the server shows it.
    fn model(&self, host: &Host<'_>) -> Result<String, Error> {
        let Some(device) = &self.device else {
            return copy("PaperPaw");
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match device.device(host, &mut input, &mut out) {
            Ok(identity) => copy(identity.model()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => copy("PaperPaw"),
        }
    }

    /// Starts `call` on the network service and notes what it is for. The
    /// buffers that held the credentials are wiped.
    fn start(&mut self, host: &Host<'_>, call: &Call, mut pending: Pending) -> Result<(), Error> {
        let net = self.net.as_ref().ok_or(Error::Unsupported)?;
        if !self.table.has_room() {
            return Err(Error::Busy);
        }
        let mut headers = [Header::new("Accept", "*/*")?; 4];
        let mut count = 0;
        for (name, value) in call.headers() {
            let slot = headers.get_mut(count).ok_or(Error::NoSpace)?;
            *slot = Header::new(name, value)?;
            count += 1;
        }
        let used = headers.get(..count).ok_or(Error::NoSpace)?;
        let mut request = Request::new(pending.purpose.token(), call.method, call.url())?
            .headers(used)?
            .max_body(u32::try_from(MAX_ANSWER).unwrap_or(0));
        if let Some(body) = call.body() {
            request = request.body(body.as_bytes())?;
        }
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let started = net.request(host, &request, &mut input, &mut out);
        wipe(&mut input);
        wipe(&mut out);
        pending.id = started?;
        self.table.add(pending)
    }

    fn push_later(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        if !self.backing.can_send()
            || match self.push_work {
                PushWork::Idle => false,
                PushWork::Full(at) => now_ms.saturating_sub(at) < PUSH_EVERY_MS,
                PushWork::Declared | PushWork::Denied => true,
            }
        {
            return Ok(());
        }
        let Some(work) = &self.work else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(256)?, buffer(4096)?);
        let spec = WorkSpec::new(PUSH_WORK, WORK_DUE)?
            .network()
            .within_ms(PUSH_WITHIN_MS)
            .at_moment(Moment::Online);
        match work.declare(host, &spec, &mut input, &mut out) {
            // No room for it: the progress goes with the next page or when the book closes.
            Ok(()) => {
                self.push_work = PushWork::Declared;
                Ok(())
            }
            Err(Error::Full) => {
                self.push_work = PushWork::Full(now_ms);
                Ok(())
            }
            Err(Error::Denied | Error::Unsupported) => {
                self.push_work = PushWork::Denied;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    // ---- the server's answers ---------------------------------------------------

    fn http_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let Some(pending) = self.table.take(done.id()) else {
            return Ok(());
        };
        let status = if done.answered() { done.status() } else { 0 };
        let now = done.now_ms();
        match pending.purpose {
            Purpose::Pull => self.pulled(host, now, &pending, done, status),
            Purpose::Push => {
                if status != 200
                    && let Some(engine) = &mut self.engine
                {
                    engine.not_sent();
                }
                Ok(())
            }
            Purpose::Authorize | Purpose::Create if done.reason() == Reason::Offline => {
                self.held_back(host, pending.purpose == Purpose::Create)
            }
            Purpose::Authorize => self.signed(host, false, status),
            Purpose::Create => self.signed(host, true, status),
        }
    }

    /// The answer to a fetch, read and given to the engine.
    fn pulled(
        &mut self,
        host: &Host<'_>,
        now_ms: u64,
        pending: &Pending,
        done: &HttpDone,
        status: u32,
    ) -> Result<(), Error> {
        let remote = if status == 200 {
            self.read_remote(host, done)?
        } else {
            None
        };
        let ctx = self.ctx(host, now_ms)?;
        let mut out = Vec::new();
        let answer = Answer::new(
            &pending.path,
            pending.manual,
            pending.local,
            status,
            remote.as_ref(),
        );
        self.engine(host, now_ms)?.pulled(ctx, &answer, &mut out)?;
        self.run(host, ctx, out)
    }

    /// The progress in the answer's body; `None` when it is no progress.
    fn read_remote(&self, host: &Host<'_>, done: &HttpDone) -> Result<Option<Remote>, Error> {
        let net = self.net.as_ref().ok_or(Error::State)?;
        let len = usize::try_from(done.body_len()).map_err(|_| Error::NoSpace)?;
        if len > MAX_ANSWER {
            return Ok(None);
        }
        let mut bytes = buffer(len)?;
        let (mut input, mut out) = (buffer(REPLY)?, buffer(REPLY)?);
        let mut at = 0;
        while at < len {
            let part = net.body(
                host,
                done.id(),
                u32::try_from(at).unwrap_or(u32::MAX),
                &mut input,
                &mut out,
            )?;
            if part.is_empty() {
                break;
            }
            let end = (at + part.len()).min(len);
            let take = end - at;
            if let (Some(to), Some(from)) = (bytes.get_mut(at..end), part.get(..take)) {
                to.copy_from_slice(from);
            }
            at = end;
        }
        if at != len {
            wipe(&mut bytes);
            return Ok(None);
        }
        let remote = match core::str::from_utf8(bytes.get(..at).unwrap_or_default()) {
            Ok(text) => native_model::read_answer(text),
            Err(_) => Ok(None),
        };
        wipe(&mut bytes);
        remote
    }

    // ---- notices and offers -----------------------------------------------------

    fn notice(&mut self, host: &Host<'_>, notice: Notice) -> Result<(), Error> {
        match notice {
            Notice::Failed(status) => self.notice_key(host, failure(status), status),
            Notice::Unreadable(status) => self.notice_key(host, "kosync.unreadable", status),
            Notice::UpToDate(status) => self.notice_key(host, "kosync.up-to-date", status),
        }
    }

    /// A notice of the text `key`, with the HTTP status where it has one.
    fn notice_key(&self, host: &Host<'_>, key: &str, status: u32) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let mut number = [0; DIGITS];
        let text = say.format(
            host,
            key,
            &[Arg::Text {
                name: "status",
                value: digits(status, &mut number),
            }],
        )?;
        let mut scratch = [0; 512];
        best_effort(CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?));
        Ok(())
    }

    /// Asks whether to go where another device is.
    fn offer(&mut self, host: &Host<'_>, offer: Offer) -> Result<(), Error> {
        if self.offers.len() >= MOST_OFFERS {
            // Too many open: this one is left, held back, offered again with the next fetch.
            return Ok(());
        }
        let mut say = Messages::new()?;
        let percent = offer
            .percentage
            .parse::<f64>()
            .map_or_else(|_| copy("?"), percent_text)?;
        let text = say.format(
            host,
            "kosync.further",
            &[
                Arg::Text {
                    name: "device",
                    value: &offer.device,
                },
                Arg::Text {
                    name: "percent",
                    value: &percent,
                },
            ],
        )?;
        let title = say.copy(host, "kosync.group")?;
        let go = say.copy(host, "kosync.go")?;
        let stay = say.copy(host, "kosync.stay")?;
        let labels = concat(&[&go, "\n", &stay])?;
        let question = Question::confirm(&title, &text)?.confirm_labels(&labels)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let request = ui.ask(host, question, &mut input, &mut out)?;
        push(&mut self.offers, Asked { request, offer })
    }

    /// An answer to a question the plugin asked.
    fn answered(&mut self, host: &Host<'_>, now_ms: u64, answer: Answered) -> Result<(), Error> {
        let chosen = match self.online.answered(host, answer) {
            Ok(chosen) => chosen,
            Err(error) => {
                self.needs_network(host)?;
                return Err(error);
            }
        };
        match chosen {
            Some(online::Chosen::Online(create)) => return self.call_again(host, create),
            Some(online::Chosen::NotNow(_)) => return self.needs_network(host),
            None => {}
        }
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        if let Some((request, field)) = self.typing
            && request == answer.request()
        {
            self.typing = None;
            if answer.outcome() == AnswerOutcome::Answered {
                self.typed(host, field, answer)?;
            }
            return Ok(());
        }
        let Some(at) = self
            .offers
            .iter()
            .position(|a| a.request == answer.request())
        else {
            return Ok(());
        };
        let asked = self.offers.swap_remove(at);
        self.chose(host, now_ms, asked.offer, answer)
    }

    /// The user's choice on an offer: *Go there*, *Stay here*, or closed.
    fn chose(
        &mut self,
        host: &Host<'_>,
        now_ms: u64,
        offer: Offer,
        answer: Answered,
    ) -> Result<(), Error> {
        let answered = answer.outcome() == AnswerOutcome::Answered;
        let go = answered && answer.choice() == 1;
        let stay = answered && answer.choice() == 0;
        let went = if go {
            match self.go_there(host, &offer) {
                Ok(went) => went,
                Err(Error::Busy | Error::Full) => {
                    self.engine(host, now_ms)?.retry_offer(&offer.path)?;
                    return self.notice_key(host, "kosync.book-busy", 0);
                }
                Err(error) => return Err(error),
            }
        } else {
            false
        };
        let ctx = self.ctx(host, now_ms)?;
        let mut out = Vec::new();
        let chose = Chose {
            path: &offer.path,
            percentage: &offer.percentage,
            went,
            stay,
        };
        self.engine(host, now_ms)?.answered(ctx, &chose, &mut out)?;
        self.run(host, ctx, out)
    }

    /// Opens the book where the other device is, in the reader the user bound.
    /// `false` where the place cannot be found (it is then as if closed).
    fn go_there(&mut self, host: &Host<'_>, offer: &Offer) -> Result<bool, Error> {
        let progress = offer.percentage.parse::<f64>().unwrap_or(0.0);
        let Some((section, offset)) =
            self.backing
                .locate(host, &offer.path, &offer.xpointer, progress)?
        else {
            return Ok(false);
        };
        let (mut s, mut o) = ([0; DIGITS], [0; DIGITS]);
        let params = [
            Param::new("path", &offer.path)?,
            Param::new("section", digits(section, &mut s))?,
            Param::new("offset", digits(offset, &mut o))?,
        ];
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .open(Open::slot("core.reader")?.handover(handover))?;
        Ok(true)
    }

    // ---- the account ------------------------------------------------------------

    fn opened_account(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.ensure(host)?;
        self.read_key(host)?;
        let user = if self.key.is_some() {
            Self::text_setting(host, USER)?
        } else {
            String::new()
        };
        let account = Account::new(&user, self.net.is_some())?;
        let mut view = View {
            account,
            list: ItemList::default(),
            note: String::new(),
        };
        Self::build(host, &Self::server(host)?, &mut view)?;
        self.online.reset();
        self.screens.open(instance, view)
    }

    /// The rows and the line below them, as the account is now.
    fn build(host: &Host<'_>, url: &str, view: &mut View) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        for row in view.account.rows()? {
            let item = match row {
                Row::Server => Item::row(
                    &say.copy(host, "kosync.server")?,
                    native_model::server_label(url),
                )?,
                Row::User => Item::row(&say.copy(host, "kosync.user")?, &view.account.user)?,
                Row::Password => {
                    let dots = if view.account.has_password() {
                        "••••••"
                    } else {
                        ""
                    };
                    Item::row(&say.copy(host, "kosync.password")?, dots)?
                }
                Row::SignIn => Item::row(&say.copy(host, "kosync.sign-in")?, "›")?,
                Row::Create => Item::row(&say.copy(host, "kosync.create")?, "›")?,
                Row::SignOut => Item::row(&say.copy(host, "kosync.sign-out")?, "›")?,
            };
            push(&mut items, item)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        view.note = Self::message_text(host, &mut say, &view.account, url)?;
        Ok(())
    }

    fn message_text(
        host: &Host<'_>,
        say: &mut Messages,
        account: &Account,
        url: &str,
    ) -> Result<String, Error> {
        let server = native_model::server_label(url);
        let mut number = [0; DIGITS];
        match account.message {
            None => Ok(String::new()),
            Some(Message::State) => {
                if !account.network {
                    say.copy(host, "kosync.no-network")
                } else if account.signed_in.is_empty() {
                    say.copy(host, "kosync.signed-out")
                } else {
                    say.format(
                        host,
                        "kosync.signed-in",
                        &[
                            Arg::Text {
                                name: "user",
                                value: &account.signed_in,
                            },
                            Arg::Text {
                                name: "server",
                                value: server,
                            },
                        ],
                    )
                }
            }
            Some(Message::NoNetwork) => say.copy(host, "kosync.no-network"),
            Some(Message::Missing) => say.copy(host, "kosync.missing"),
            Some(Message::Connecting) => say.copy(host, "kosync.connecting"),
            Some(Message::ServerSet { signed_out }) => say.format(
                host,
                if signed_out {
                    "kosync.server-changed"
                } else {
                    "kosync.server-set"
                },
                &[Arg::Text {
                    name: "server",
                    value: server,
                }],
            ),
            Some(Message::Taken) => say.copy(host, "kosync.taken"),
            Some(Message::Failed(status)) => say.format(
                host,
                failure(status),
                &[Arg::Text {
                    name: "status",
                    value: digits(status, &mut number),
                }],
            ),
        }
    }

    /// Changes the account screen's view and rebuilds its rows.
    fn change(
        &mut self,
        host: &Host<'_>,
        token: u32,
        change: impl FnOnce(&mut View) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let url = Self::server(host)?;
        let Some(view) = self.screens.top(token) else {
            return Ok(());
        };
        change(view)?;
        Self::build(host, &url, view)?;
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    fn chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(row) = view.account.rows()?.get(index).copied() else {
            return Ok(());
        };
        let todo = view.account.chosen(row);
        match todo {
            Todo::Nothing => {}
            Todo::Ask(field) => self.ask(host, token, field)?,
            Todo::Call { create } => {
                // Chosen anew: Offline mode may offer to go online again.
                self.online.reset();
                self.call(host, token, create)?;
            }
            Todo::SignOut => {
                self.sign_out(host)?;
                if let Some(view) = self.screens.top(token) {
                    view.list.set_focus(0);
                }
            }
        }
        self.change(host, token, |_| Ok(()))
    }

    /// Asks for the text of `field` (the password masked, the server as an address).
    fn ask(&mut self, host: &Host<'_>, _token: u32, field: Field) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.copy(
            host,
            match field {
                Field::Server => "kosync.server",
                Field::User => "kosync.user",
                _ => "kosync.password",
            },
        )?;
        let initial = match field {
            Field::Server => Some(Self::server(host)?),
            Field::User => self
                .screens
                .top(ACCOUNT)
                .map(|v| copy(&v.account.user))
                .transpose()?,
            _ => None,
        };
        let question = match field {
            Field::Server => Question::text(&title, "")?.mode(TextMode::Url)?,
            Field::Password => Question::text(&title, "")?.secret()?,
            _ => Question::text(&title, "")?,
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let asked = ui
            .ask(host, question, &mut input, &mut out)
            .and_then(|request| {
                if let Some(initial) = &initial {
                    ui.text_write(
                        host,
                        request.text(),
                        0,
                        initial,
                        TextWrite::Last,
                        &mut input,
                        &mut out,
                    )?;
                }
                Ok(request)
            });
        wipe(&mut input);
        wipe(&mut out);
        self.typing = Some((asked?, field));
        Ok(())
    }

    /// The text of a field was typed.
    fn typed(&mut self, host: &Host<'_>, field: Field, answer: Answered) -> Result<(), Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let read = ui.text_read(host, answer.request().text(), 0, &mut input, &mut out);
        let text = match read {
            Ok(part) if part.data().len() as u64 == u64::from(part.total()) => {
                Secret::copy(part.data())
            }
            Ok(_) => Err(Error::NoSpace),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut out);
        let text = text?;
        self.apply_typed(host, field, text.as_str())
    }

    fn apply_typed(&mut self, host: &Host<'_>, field: Field, text: &str) -> Result<(), Error> {
        let current = Self::server(host)?;
        let mut new_server = None;
        self.change(host, ACCOUNT, |view| {
            match field {
                Field::Server => new_server = view.account.typed_server(text, &current)?,
                Field::User => view.account.typed_user(text)?,
                _ => view.account.typed_password(text)?,
            }
            view.list.set_focus(Account::next_focus(field));
            Ok(())
        })?;
        if let Some(url) = new_server {
            let signed_in = self.key.is_some();
            let mut scratch = [0; 1200];
            CommandSender::new(host, &mut scratch).set_setting(SERVER, SettingValue::Text(&url))?;
            self.server = Some(url);
            if signed_in {
                self.clear_account(host)?;
            }
        }
        Ok(())
    }

    /// *Sign in* or *Create account*: the server is asked.
    fn call(&mut self, host: &Host<'_>, token: u32, create: bool) -> Result<(), Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(());
        };
        let (Some(key), user) = (
            view.account.key().map(Secret::copy).transpose()?,
            copy(&view.account.user)?,
        ) else {
            return Ok(());
        };
        let base = Self::server(host)?;
        let call = if create {
            Call::server(
                &base,
                "POST",
                "/users/create",
                Some(create_body(&user, key.as_str())?),
                None,
            )?
        } else {
            Call::server(
                &base,
                "GET",
                "/users/auth",
                None,
                Some((&user, key.as_str())),
            )?
        };
        drop(key);
        let purpose = if create {
            Purpose::Create
        } else {
            Purpose::Authorize
        };
        let pending = Pending {
            id: RequestId::new(1)?,
            purpose,
            path: String::new(),
            local: 0.0,
            manual: false,
        };
        match self.start(host, &call, pending) {
            Ok(()) => Ok(()),
            // Not started: said as an answer that never came.
            Err(Error::Busy | Error::NoMemory | Error::Unsupported) => {
                if let Some(view) = self.screens.top(token) {
                    view.account.answered(create, 0)?;
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Offline mode held *Sign in* or *Create account* back: the first time
    /// since the reader chose it, the offer to go online; else (or where the
    /// question cannot be asked) the line says the network is needed.
    fn held_back(&mut self, host: &Host<'_>, create: bool) -> Result<(), Error> {
        if !self.online.may_offer() || self.screens.top(ACCOUNT).is_none() {
            return self.needs_network(host);
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        match self.online.ask(host, ui, create) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.needs_network(host)?;
                if error == Error::NoMemory {
                    Err(error)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// After *Go online*: the server is asked again (the screen still says it
    /// is connecting).
    fn call_again(&mut self, host: &Host<'_>, create: bool) -> Result<(), Error> {
        self.call(host, ACCOUNT, create)?;
        self.change(host, ACCOUNT, |_| Ok(()))
    }

    /// Not sent: the account screen waits no longer and says the network is
    /// needed.
    fn needs_network(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.change(host, ACCOUNT, |view| {
            view.account.needs_network();
            Ok(())
        })
    }

    /// The server's answer to *Sign in* or *Create account*.
    fn signed(&mut self, host: &Host<'_>, created: bool, status: u32) -> Result<(), Error> {
        let mut kept = false;
        let key = self
            .screens
            .top(ACCOUNT)
            .and_then(|v| v.account.key().map(Secret::copy))
            .transpose()?;
        let mut user = String::new();
        self.change(host, ACCOUNT, |view| {
            let ok = view.account.answered(created, status)?;
            if ok {
                user = copy(&view.account.user)?;
                kept = true;
            }
            Ok(())
        })?;
        if kept && let Some(key) = key {
            let refused = !self.keep(host, key.as_str(), &user)?;
            drop(key);
            if refused {
                // Not kept: not signed in, and said.
                self.change(host, ACCOUNT, |view| {
                    view.account.sign_out();
                    Ok(())
                })?;
            }
        }
        Ok(())
    }

    /// Keeps the key of the account signed in and its user name. `false` where
    /// the device keeps no secrets (said with the shared text, not signed in).
    fn keep(&mut self, host: &Host<'_>, key: &str, user: &str) -> Result<bool, Error> {
        let Some(secrets) = &self.secrets else {
            best_effort(self.notice_key(host, "core.secrets-unsupported", 0));
            return Ok(false);
        };
        let mut say = Messages::new()?;
        let label = say.copy(host, "kosync.secret")?;
        match store::keep_key(secrets, host, &label, key) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                best_effort(self.notice_key(host, "core.secrets-unsupported", 0));
                return Ok(false);
            }
        }
        self.key = Some(Secret::copy(key)?);
        let mut scratch = [0; 1200];
        CommandSender::new(host, &mut scratch).set_setting(USER, SettingValue::Text(user))?;
        self.push_work = PushWork::Idle;
        if let Some(engine) = &mut self.engine {
            engine.account_changed();
        }
        Ok(true)
    }

    /// Signs out: the key and the user name go.
    fn sign_out(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.clear_account(host)
    }

    fn clear_account(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if let Some(secrets) = &self.secrets {
            best_effort(store::remove_key(secrets, host));
        }
        self.key = None;
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).set_setting(USER, SettingValue::Text(""))?;
        self.push_work = PushWork::Idle;
        if let Some(engine) = &mut self.engine {
            engine.account_changed();
        }
        Ok(())
    }
}

/// A notice is best effort: the work it reports is done all the same.
fn best_effort<T>(result: Result<T, Error>) {
    match result {
        Ok(_) | Err(_) => {}
    }
}

/// `Ok(None)` for a service the device does not offer or the plugin may not use.
fn optional<T>(opened: Result<T, Error>) -> Result<Option<T>, Error> {
    match opened {
        Ok(service) => Ok(Some(service)),
        Err(Error::Unsupported | Error::NotFound | Error::Denied) => Ok(None),
        Err(error) => Err(error),
    }
}
