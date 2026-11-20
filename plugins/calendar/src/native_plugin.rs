//! The interface-7 Calendar plugin: the sources and events (`model`) kept in a
//! file of the data folder (`keep`), shown on three screens (`native_screens`
//! and `native_month` for the calendar and the sources, `native_sleep` for
//! the sleep screen, the dialogs in `native_asks`) and in five widgets
//! (`native_widgets`), and read from the sources by `sync` over `core.net`,
//! run as declared work (`core.work`), the passwords kept by `core.secrets`.
//!
//! This file is the glue: it routes the host's events and callbacks, reads
//! the clock and the settings, is the engine's [`Wire`], declares the work and
//! tells the screens and the widgets when the events changed
//! (`PORTING-7.md`). No key press waits for the network or reads the card: the
//! events are in memory, and reading the sources is work.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input, InputGestureKind},
        clock,
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, kinds},
        formatting::Arg,
        net::{
            FileRoot, Header, HttpDone, Net, REPLY as NET_REPLY, REQUEST_INPUT, Reason, Request,
        },
        registrations,
        reply::Reply,
        secrets::Secrets,
        settings::{SettingValue, read_setting},
        snapshot::capture_snapshot,
        storage::{self, Root},
        ui::{
            Instance, Notify, RequestId as AskId, Ui, WidgetInputKind, WidgetTarget,
            stages::WidgetCall,
        },
        work::{Moment, SPAN_MAX_MS, Work, WorkSpec},
    },
};
use paperpaw_ui_kit::{
    Footer, ItemList, ItemStep, Messages, Screens,
    dav::secret::{self, Secret},
    files::Volume,
    mem::{self, buffer, wipe},
    paint, read_event,
    text::copy,
    widget::{Cx, cx::zone_offset, settings::base},
};

use crate::declarations::{
    AGENDA_TOKEN, BIRTHDAYS_TOKEN, DAYS, DAYS_DEFAULT, DAYS_MOST, HTTP_TOKEN, LIFECYCLE_TOKEN,
    MONTH_TOKEN, NEXT_TOKEN, OPEN_TOKEN, SCREEN_TOKEN, SLEEP_TOKEN, SOURCES_TOKEN, SYNC,
    SYNC_CHOICES, SYNC_TOKEN, TODAY_TOKEN, WORK_TOKEN,
};
use crate::keep::{self, Loaded};
use crate::model::{NO_TIME, Show, Store, state};
use crate::native_asks::{Asked, Draft};
use crate::native_month::{self, Month, Shown};
use crate::native_screens::{self as screens, Built, Ctx, Mode, Pick, Row, View, month_step};
use crate::native_sleep;
use crate::native_text::{aged, month_name, time_text, twelve_hours, weekday_name};
use crate::native_widgets::{self as widgets, Env, OPEN_TARGET};
use crate::sync::{Call, Ended, Outcome, Report, Step, Sync, Wire};

mod image {
    include!(concat!(env!("OUT_DIR"), "/calendar_declarations.rs"));
}

/// The most bytes of a stage reply (a size, a place, the targets of the rows).
const REPLY: usize = 1024;
/// The work that syncs, the one that does so only when the radio is on
/// anyway, and the one that tells the widgets the day has turned.
const SYNC_KEY: &str = "calendar.sync";
const REFRESH_KEY: &str = "calendar.refresh";
const DAY_KEY: &str = "calendar.day";
/// The screens' ids.
pub(crate) const SCREEN: &str = "calendar.screen";
pub(crate) const SOURCES: &str = "calendar.sources";
/// The calendars are read again an hour on, when the radio is on anyway.
const REFRESH_MS: u32 = 60 * 60 * 1000;
/// Opening the calendar syncs at most this often.
const OPEN_EVERY_MS: u64 = 5 * 60 * 1000;

/// A widget instance the host made.
#[derive(Debug)]
struct Placed {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
}

/// The handles of the services, opened when first needed.
#[derive(Debug, Default)]
pub(crate) struct Handles {
    net: Option<Net>,
    secrets: Option<Secrets>,
    work: Option<Work>,
    ui: Option<Ui>,
}

impl Handles {
    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    pub(crate) fn secrets(&mut self, host: &Host<'_>) -> Result<&Secrets, Error> {
        if self.secrets.is_none() {
            self.secrets = Some(Secrets::open(host)?);
        }
        self.secrets.as_ref().ok_or(Error::State)
    }

    fn work(&mut self, host: &Host<'_>) -> Result<&Work, Error> {
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        self.work.as_ref().ok_or(Error::State)
    }

    pub(crate) fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    /// Declares `spec`; whether it was taken. Work that cannot be declared (a
    /// full table, the network turned off for the plugin) costs that run,
    /// nothing else.
    fn declare(&mut self, host: &Host<'_>, spec: &WorkSpec<'_>) -> Result<bool, Error> {
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        let work = match self.work(host) {
            Ok(work) => work,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(false),
        };
        match work.declare(host, spec, &mut input, &mut out) {
            Ok(()) => Ok(true),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(false),
        }
    }
}

/// The plugin's data folder.
pub(crate) struct Data<'a, 'h>(pub &'a Host<'h>);

impl Volume for Data<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.0, Root::Data, name)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.0, Root::Data, name, at, out)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::write(self.0, Root::Data, name, data)
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::append(self.0, Root::Data, name, data)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        storage::rename(self.0, Root::Data, from, to)
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.0, Root::Data, name)
    }
    fn sync(&self) -> Result<(), Error> {
        storage::sync(self.0)
    }
}

/// The user's files on the card: read only, for a calendar file.
struct Card<'a, 'h>(&'a Host<'h>);

impl Volume for Card<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.0, Root::Files, name)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.0, Root::Files, name, at, out)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn rename(&self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }
}

/// The name of the secret that holds a source's password.
pub(crate) fn secret_name(id: u8) -> Result<String, Error> {
    let mut digits = [0u8; 20];
    paperpaw_ui_kit::text::concat(&[
        "pw",
        paperpaw_ui_kit::text::decimal(usize::from(id), &mut digits),
    ])
}

fn int_setting(host: &Host<'_>, key: &str, default: i64) -> i64 {
    let mut scratch = [0; 64];
    match read_setting(host, key, &mut scratch) {
        Ok(SettingValue::Int(value)) => value,
        _ => default,
    }
}

/// What the reader's clock says.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Now {
    /// Seconds since 1970, if the clock is set.
    pub unix: Option<i64>,
    /// Days since 1970 by the reader's own time (0 without a clock).
    pub today: i32,
    /// The second of the day (0 without a clock).
    pub secs: u32,
    /// How far the reader's clock is east of UTC.
    pub zone: i32,
}

pub(crate) fn clock_now(host: &Host<'_>) -> Result<Now, Error> {
    let Some(unix) = capture_snapshot(host)?.unix_seconds() else {
        return Ok(Now::default());
    };
    let zone = zone_offset(host, unix)?;
    let local = unix.saturating_add(i64::from(zone));
    Ok(Now {
        unix: Some(unix),
        today: i32::try_from(local.div_euclid(86_400)).unwrap_or(0),
        secs: u32::try_from(local.rem_euclid(86_400)).unwrap_or(0),
        zone,
    })
}

/// The first day of the week: 0 (Sunday) or 1 (Monday), as the reader set it
/// or as the language's habit says.
fn week_start(host: &Host<'_>, say: &mut Messages) -> Result<u8, Error> {
    let set =
        paperpaw_ui_kit::widget::settings::text_of(host, "core.week-start", |value| match value {
            "core.week.sunday" => Some(0),
            "core.week.monday" => Some(1),
            _ => None,
        })?;
    match set {
        Some(day) => Ok(day),
        None => Ok(u8::from(say.copy(host, "core.week-start-day")? != "0")),
    }
}

/// The host as the engine uses it.
struct HostWire<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a mut Handles,
    data: Data<'a, 'h>,
    card: Card<'a, 'h>,
}

impl<'a, 'h> HostWire<'a, 'h> {
    fn new(host: &'a Host<'h>, handles: &'a mut Handles) -> Self {
        Self {
            host,
            handles,
            data: Data(host),
            card: Card(host),
        }
    }

    /// The header that says who asks, none for a source without a name or a
    /// password (a server that asks for none).
    fn proof(&mut self, store: &Store, src: u8) -> Result<Option<Secret>, Error> {
        let Some(source) = store
            .source(src)
            .filter(|s| s.kind.account() && !s.user.is_empty())
        else {
            return Ok(None);
        };
        let password = match self.handles.secrets(self.host) {
            Ok(secrets) => secret::read(secrets, self.host, &secret_name(src)?)?,
            // A device that keeps no secrets has no password.
            Err(Error::Unsupported | Error::Denied) => None,
            Err(error) => return Err(error),
        };
        match password {
            Some(password) => Secret::basic(&source.user, password.as_str()).map(Some),
            None => Ok(None),
        }
    }

    fn request(
        &mut self,
        call: &Call<'_>,
        proof: Option<&Secret>,
        (input, out): (&mut [u8], &mut [u8]),
    ) -> Result<u32, Error> {
        let mut headers: Vec<Header<'_>> = Vec::new();
        headers.try_reserve(4).map_err(|_| Error::NoMemory)?;
        if let Some(proof) = proof {
            headers.push(Header::new("Authorization", proof.as_str())?);
        }
        if call.deep {
            headers.push(Header::new("Depth", "1")?);
        }
        if !call.content_type.is_empty() {
            headers.push(Header::new("Content-Type", call.content_type)?);
        }
        // Answers are saved to files: little is kept in the reply.
        let mut request = Request::new(HTTP_TOKEN, call.method, call.url)?
            .headers(&headers)?
            .max_body(256);
        if let Some(body) = call.body {
            request = request.body(body.as_bytes())?;
        }
        request = request
            .save_to(FileRoot::Data, call.save_to.0)?
            .max_file(call.save_to.1)?;
        let net = self.handles.net(self.host)?;
        Ok(net.request(self.host, &request, input, out)?.get())
    }
}

impl Wire for HostWire<'_, '_> {
    fn files(&self) -> &dyn Volume {
        &self.data
    }

    fn card(&self) -> &dyn Volume {
        &self.card
    }

    fn send(&mut self, call: &Call<'_>, store: &Store) -> Result<u32, Error> {
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(NET_REPLY)?);
        let proof = self.proof(store, call.source)?;
        let sent = self.request(call, proof.as_ref(), (&mut input, &mut out));
        // The input held the password in a header.
        wipe(&mut input);
        wipe(&mut out);
        sent
    }

    fn now(&mut self) -> Option<i64> {
        capture_snapshot(self.host).ok()?.unix_seconds()
    }

    fn zone(&mut self, unix: i64) -> i32 {
        zone_offset(self.host, unix).unwrap_or(0)
    }
}

/// Whether the file was read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Kept {
    /// Not yet (or the card could not be read: tried again).
    #[default]
    NotRead,
    Read,
    /// The file is not this plugin's: it is left alone, nothing is changed.
    Unreadable,
}

/// The interface-7 Calendar plugin.
#[derive(Debug, Default)]
pub struct NativeCalendar {
    pub(crate) store: Store,
    kept: Kept,
    sync: Sync,
    pub(crate) handles: Handles,
    pub(crate) screens: Screens<View>,
    placed: Vec<Placed>,
    /// The question that is open, and what it was for.
    pub(crate) asked: Option<(AskId, Asked)>,
    /// A source being made, over its questions.
    pub(crate) draft: Option<Draft>,
    /// What the calendar screen shows, and where Back from it goes.
    pub(crate) mode: Mode,
    pub(crate) before: Option<Mode>,
    /// The event shown whole.
    pub(crate) detail: Option<Pick>,
    /// How the last run went, in words; empty until one has.
    message: String,
    /// The run under way was asked for: how it ends is said.
    manual: bool,
    /// A run for one source only (that was asked for).
    only: Option<u8>,
    /// When opening the calendar last started a run, on the host's clock.
    opened_ms: u64,
}

impl Plugin for NativeCalendar {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::UI)
            .chain(image::NAVIGATION)
            .chain(image::WIDGETS)
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
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::Started,
            kinds::Resume,
            kinds::SettingChanged,
            kinds::WorkDue,
            kinds::HttpDone,
            kinds::ActionRun,
            kinds::Answered,
            kinds::WidgetMade,
            kinds::WidgetDropped,
            kinds::StageCall,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed
        ) else {
            return Ok(());
        };
        let ours = token == SCREEN_TOKEN || token == SOURCES_TOKEN;
        match event {
            Event::Started { .. } if token == LIFECYCLE_TOKEN => self.started(host),
            // The day may have turned while the reader slept.
            Event::Resume { .. } if token == LIFECYCLE_TOKEN => self.day_turned(host),
            Event::SettingChanged { key, .. } if token == LIFECYCLE_TOKEN => {
                self.setting_changed(host, key)
            }
            Event::WorkDue(due) if token == WORK_TOKEN => match due.key() {
                SYNC_KEY | REFRESH_KEY => self.run(host),
                DAY_KEY => self.day_turned(host),
                _ => Ok(()),
            },
            Event::HttpDone(done) if token == HTTP_TOKEN => self.http_done(host, &done),
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                self.mode = Mode::Agenda;
                self.before = None;
                open(host, SCREEN)
            }
            Event::ActionRun { .. } if token == SYNC_TOKEN => self.sync_now(host),
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::WidgetMade {
                widget, instance, ..
            } => self.made(host, widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.placed.retain(|placed| placed.instance != instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::ScreenOpened { instance, .. } if ours => self.opened(host, token, instance),
            // The sleep screen draws from the kept events: read when it opens.
            Event::ScreenOpened { .. } if token == SLEEP_TOKEN => self.load(host),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if ours => self.screens.shown(token, instance),
            Event::ScreenClosed { instance, .. } if ours => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        match token {
            SCREEN_TOKEN | SOURCES_TOKEN => return self.draw_screen(host, token, &mut frame),
            SLEEP_TOKEN => {
                let env = self.env(host)?;
                return native_sleep::draw(host, &mut frame, &self.store, &env);
            }
            _ => {}
        }
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        self.load(host)?;
        let placed = self.find(frame.id).ok_or(Error::NotFound)?;
        let (token, widget) = (placed.token, copy(&placed.widget)?);
        let env = self.env(host)?;
        let mut cx = Cx::open(host, &widget)?;
        let mut out = Vec::new();
        widgets::draw(
            &mut cx,
            &mut out,
            area_of(frame.rect),
            token,
            (&self.store, &env),
        )?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == SCREEN_TOKEN || token == SOURCES_TOKEN {
            return self.screen_input(host, token, &input);
        }
        let Input::Widget {
            instance,
            target,
            kind,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        if self.find(instance.get()).is_none() {
            return Ok(Handled::No);
        }
        if !matches!(kind, WidgetInputKind::Tap | WidgetInputKind::LongPress) {
            return Ok(Handled::No);
        }
        // A tap on an event shows it, on a day of the month opens the day,
        // anything else opens the agenda.
        self.before = None;
        self.mode = Mode::Agenda;
        let env = self.env(host)?;
        if let Some(day) = widgets::day_of(target) {
            self.mode = Mode::Day(day);
        } else if target != OPEN_TARGET
            && let Some(pick) = widgets::pick_of(&self.store, target, &env)?
        {
            self.detail = Some(pick);
            self.mode = Mode::Detail;
        }
        open(host, SCREEN)?;
        Ok(Handled::Yes)
    }
}

impl NativeCalendar {
    fn find(&self, instance: u32) -> Option<&Placed> {
        self.placed
            .iter()
            .find(|placed| placed.instance.get() == instance)
    }

    /// What the widgets and the sleep screen are drawn with.
    fn env(&self, host: &Host<'_>) -> Result<Env, Error> {
        let mut say = Messages::new()?;
        Ok(Env {
            today: clock_now(host)?.today,
            secs: clock_now(host)?.secs,
            twelve: twelve_hours(host)?,
            week_start: week_start(host, &mut say)?,
            full: false,
        })
    }

    /// What the rows are built with.
    fn ctx(&self, host: &Host<'_>) -> Result<Ctx, Error> {
        let mut say = Messages::new()?;
        let now = clock_now(host)?;
        let days = int_setting(host, DAYS, DAYS_DEFAULT).clamp(1, i64::from(DAYS_MOST));
        Ok(Ctx {
            today: now.today,
            secs: now.secs,
            twelve: twelve_hours(host)?,
            days: i32::try_from(days).unwrap_or(7),
            week_start: week_start(host, &mut say)?,
            syncing: self.sync.running(),
            zone: now.zone,
        })
    }

    // The file.

    /// Reads the file once. A card that cannot be read now is tried again.
    pub(crate) fn load(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.kept != Kept::NotRead {
            return Ok(());
        }
        match keep::load(&Data(host)) {
            Ok(Loaded::Store(store)) => {
                self.store = store;
                self.kept = Kept::Read;
            }
            Ok(Loaded::Unreadable) => self.kept = Kept::Unreadable,
            // A host without a card: the calendar lives while it runs.
            Err(Error::Unsupported) => self.kept = Kept::Read,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
        Ok(())
    }

    /// Whether the sources can be changed; says why not, if not.
    pub(crate) fn usable(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        self.load(host)?;
        if self.kept != Kept::Read {
            notice(host, "calendar.unreadable");
        }
        Ok(self.kept == Kept::Read)
    }

    /// The sources were changed on the reader: written to the card and shown
    /// anew.
    pub(crate) fn changed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        match keep::save(&Data(host), &self.store) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => notice(host, "calendar.not-saved"),
        }
        self.refresh(host)
    }

    // What is shown.

    /// The rows of the calendar screen for what it shows now.
    fn calendar_rows(&self, host: &Host<'_>) -> Result<Built, Error> {
        let ctx = self.ctx(host)?;
        match (self.mode, self.detail) {
            (Mode::Detail, Some(pick)) => screens::detail_rows(host, &self.store, pick, &ctx),
            _ => screens::calendar_rows(host, &self.store, self.mode, &ctx),
        }
    }

    /// Builds the rows of the open screens anew, tells the widgets and asks
    /// for the screen on top to be drawn.
    pub(crate) fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.screens.top(SCREEN_TOKEN).is_some() {
            let built = self.calendar_rows(host)?;
            if let Some(top) = self.screens.top(SCREEN_TOKEN) {
                top.set(built);
            }
        }
        if self.screens.top(SOURCES_TOKEN).is_some() {
            let ctx = self.ctx(host)?;
            let built = screens::sources_rows(host, &self.store, &ctx, &self.message)?;
            if let Some(top) = self.screens.top(SOURCES_TOKEN) {
                top.set(built);
            }
        }
        self.tell_widgets(host);
        if !self.screens.is_empty() {
            let mut scratch = [0; 64];
            // A refusal (another screen is on top) is no error of the change.
            CommandSender::new(host, &mut scratch)
                .queue_redraw(RedrawMode::Partial)
                .ok();
        }
        Ok(())
    }

    fn tell_widgets(&self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for placed in &self.placed {
            commands.widget_changed(placed.instance).ok();
        }
    }

    // Events.

    fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        self.refresh_later(host)?;
        self.watch_time(host)
    }

    /// The day turned (or may have): what is today is another set.
    fn day_turned(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.tell_widgets(host);
        self.watch_time(host)
    }

    /// Declares the work that comes due when what the widgets show changes:
    /// the next event's start or end today, or just after midnight; while a
    /// widget is placed.
    fn watch_time(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.placed.is_empty() {
            return Ok(());
        }
        self.load(host)?;
        let now = clock_now(host)?;
        if now.unix.is_none() {
            return Ok(());
        }
        let mut next = 86_400 + 30;
        for occ in self.store.occurrences(now.today, now.today, Show::All)? {
            for at in [occ.secs, occ.end] {
                if at != NO_TIME && at > now.secs && at < next {
                    next = at;
                }
            }
        }
        let ms = u32::try_from(u64::from(next - now.secs.min(next)).max(60) * 1000)
            .unwrap_or(SPAN_MAX_MS)
            .min(SPAN_MAX_MS);
        let spec = WorkSpec::new(DAY_KEY, WORK_TOKEN)?.after_ms(ms);
        self.handles.declare(host, &spec).map(|_| ())
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let ours = key.is_empty()
            || key.starts_with("calendar.")
            || matches!(key, "core.week-start" | "core.clock-format");
        if !ours {
            return Ok(());
        }
        if key == SYNC {
            self.refresh_later(host)?;
        }
        self.refresh(host)
    }

    fn made(&mut self, host: &Host<'_>, widget: &str, instance: Instance) -> Result<(), Error> {
        let token = match base(widget) {
            "calendar.today" => TODAY_TOKEN,
            "calendar.next" => NEXT_TOKEN,
            "calendar.agenda" => AGENDA_TOKEN,
            "calendar.month" => MONTH_TOKEN,
            "calendar.birthdays" => BIRTHDAYS_TOKEN,
            _ => return Ok(()),
        };
        self.placed.retain(|placed| placed.instance != instance);
        mem::push(
            &mut self.placed,
            Placed {
                instance,
                token,
                widget: copy(widget)?,
            },
        )?;
        self.load(host)?;
        self.watch_time(host)
    }

    // The screens.

    fn opened(&mut self, host: &Host<'_>, token: u32, instance: Instance) -> Result<(), Error> {
        self.load(host)?;
        let mut view = View::default();
        let ctx = self.ctx(host)?;
        if token == SCREEN_TOKEN {
            if self.mode == Mode::Detail && self.detail.is_none() {
                self.mode = Mode::Agenda;
            }
            view.set(self.calendar_rows(host)?);
            // The first event has the focus, where there is one.
            view.list.set_focus(first_event(&view.rows));
        } else {
            view.set(screens::sources_rows(
                host,
                &self.store,
                &ctx,
                &self.message,
            )?);
        }
        self.screens.open(instance, view)?;
        if token == SCREEN_TOKEN {
            self.sync_on_open(host)?;
        }
        Ok(())
    }

    fn draw_screen(
        &mut self,
        host: &Host<'_>,
        token: u32,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        if token == SCREEN_TOKEN
            && let Mode::Month(day) = self.mode
        {
            return self.draw_month(host, frame, day);
        }
        let mut say = Messages::new()?;
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = say.copy(
            host,
            if token == SOURCES_TOKEN {
                "calendar.sources.title"
            } else {
                "calendar.title"
            },
        )?;
        let empty = token == SCREEN_TOKEN
            && self.mode != Mode::Detail
            && !view.rows.iter().any(|row| matches!(row, Row::Event(_)));
        let footer = if empty {
            Footer::Empty {
                title: &view.empty.0,
                text: &view.empty.1,
            }
        } else {
            Footer::Note(&view.hint)
        };
        view.list.draw_with(host, frame, &title, true, footer)
    }

    /// The Month view: the grid, and what the chosen day holds.
    fn draw_month(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        day: i32,
    ) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let ctx = self.ctx(host)?;
        let (year, month, _) = paperpaw_caldav::date::civil_from_days(i64::from(day));
        let first =
            i32::try_from(paperpaw_caldav::date::days_from_civil(year, month, 1)).unwrap_or(0);
        let last = first + i32::from(paperpaw_caldav::date::days_in_month(year, month)) - 1;
        let busy = self.store.busy(first, last)?;
        let mut names = Vec::new();
        for n in 0..7 {
            mem::push(&mut names, weekday_name_of(host, &mut say, n)?)?;
        }
        let lines = self.day_lines(host, &mut say, day, &ctx)?;
        let name = month_name(host, &mut say, month, false)?;
        let mut digits = [0u8; 20];
        let number =
            paperpaw_ui_kit::text::decimal(usize::try_from(year).unwrap_or(0), &mut digits);
        let title = paperpaw_ui_kit::text::concat(&[&name, " ", number])?;
        let empty = say.copy(host, "calendar.empty.day")?;
        let shown = Shown {
            title: &title,
            month: Month {
                year,
                month,
                week_start: ctx.week_start,
                today: ctx.today,
                selected: Some(day),
                busy: &busy,
                dots: true,
                names: &names,
            },
            lines: &lines,
            empty: &empty,
        };
        let view = self.screens.top(SCREEN_TOKEN).ok_or(Error::State)?;
        native_month::draw(host, frame, &mut view.grid, &shown)
    }

    /// What a day holds, a line each: its time and its title.
    fn day_lines(
        &self,
        host: &Host<'_>,
        say: &mut Messages,
        day: i32,
        ctx: &Ctx,
    ) -> Result<Vec<String>, Error> {
        let mut lines = Vec::new();
        for occ in self.store.occurrences(day, day, Show::All)? {
            let Some(entry) = self.store.entries.get(usize::from(occ.entry)) else {
                continue;
            };
            let title = aged(host, say, &entry.title, occ.age)?;
            let line = if occ.whole_day() {
                title
            } else {
                let time = time_text(host, say, ctx.twelve, occ.secs)?;
                paperpaw_ui_kit::text::concat(&[&time, "  ", &title])?
            };
            mem::push(&mut lines, line)?;
        }
        Ok(lines)
    }

    fn screen_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        if token == SCREEN_TOKEN
            && let Some(handled) = self.navigation(host, input)?
        {
            return Ok(handled);
        }
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // Select held, or the menu button, offers the views and the rest.
        let menu = matches!(
            input,
            Input::Action {
                action: "core.menu" | "core.long-confirm",
                ..
            }
        );
        let step = if menu {
            ItemStep::Held(view.list.focus())
        } else {
            view.list.input(host, input)?
        };
        let (ItemStep::Chosen(at) | ItemStep::Held(at)) = step else {
            return Ok(ItemList::handled(step));
        };
        let Some(row) = view.rows.get(at).copied() else {
            return Ok(Handled::Yes);
        };
        let held = matches!(step, ItemStep::Held(_));
        match row {
            Row::View => self.ask_views(host)?,
            Row::Event(_) if held => self.ask_views(host)?,
            Row::Event(pick) => self.show_event(host, pick)?,
            Row::Prev => self.step_view(host, -1)?,
            Row::Next => self.step_view(host, 1)?,
            Row::Back => self.go_back(host)?,
            Row::Source(id) => self.ask_source(host, id)?,
            Row::Add => self.ask_kind(host)?,
            Row::Sync => self.sync_now(host)?,
            Row::Heading | Row::Info => {}
        }
        Ok(Handled::Yes)
    }

    /// Back, the Month view's keys and touches, and the page keys of the
    /// day and week views; `None` for what is the list's.
    fn navigation(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Option<Handled>, Error> {
        if let Input::Action { action, .. } = input {
            let moved = match (*action, self.mode) {
                ("core.back", mode) if mode != Mode::Agenda => {
                    self.go_back(host)?;
                    return Ok(Some(Handled::Yes));
                }
                ("core.next-page", Mode::Day(_) | Mode::Week(_)) => Some(1),
                ("core.prev-page", Mode::Day(_) | Mode::Week(_)) => Some(-1),
                _ => None,
            };
            if let Some(by) = moved {
                self.step_view(host, by)?;
                return Ok(Some(Handled::Yes));
            }
        }
        let Mode::Month(day) = self.mode else {
            return Ok(None);
        };
        Ok(Some(self.month_input(host, day, input)?))
    }

    /// A key or a tap on the Month view.
    fn month_input(
        &mut self,
        host: &Host<'_>,
        day: i32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let to = match *input {
            Input::Action { action, .. } => match action {
                "core.left" | "core.previous" => day - 1,
                "core.right" | "core.next" => day + 1,
                "core.up" => day - 7,
                "core.down" => day + 7,
                "core.prev-page" => month_step(day, -1),
                "core.next-page" => month_step(day, 1),
                "core.confirm" => return self.open_day(host, day),
                "core.menu" | "core.long-confirm" => {
                    self.ask_views(host)?;
                    return Ok(Handled::Yes);
                }
                _ => return Ok(Handled::No),
            },
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => {
                let Some(view) = self.screens.top(SCREEN_TOKEN) else {
                    return Ok(Handled::No);
                };
                if view.grid.page.action(position) == Some("core.back") {
                    self.go_back(host)?;
                    return Ok(Handled::Yes);
                }
                let tapped = view.grid.day_at(Point::new(position.x, position.y));
                return match tapped {
                    Some(day) => self.open_day(host, day),
                    None => Ok(Handled::No),
                };
            }
            _ => return Ok(Handled::No),
        };
        self.mode = Mode::Month(to);
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch)
            .queue_redraw(RedrawMode::Partial)
            .ok();
        Ok(Handled::Yes)
    }

    /// From the Month view into a day; Back comes here again.
    fn open_day(&mut self, host: &Host<'_>, day: i32) -> Result<Handled, Error> {
        self.before = Some(Mode::Month(day));
        self.mode = Mode::Day(day);
        self.refresh(host)?;
        self.focus_first();
        Ok(Handled::Yes)
    }

    fn focus_first(&mut self) {
        if let Some(top) = self.screens.top(SCREEN_TOKEN) {
            let at = first_event(&top.rows);
            top.list.set_focus(at);
        }
    }

    /// An event shown whole.
    fn show_event(&mut self, host: &Host<'_>, pick: Pick) -> Result<(), Error> {
        self.before = Some(self.mode);
        self.detail = Some(pick);
        self.mode = Mode::Detail;
        self.refresh(host)?;
        if let Some(top) = self.screens.top(SCREEN_TOKEN) {
            top.list.set_focus(0);
        }
        Ok(())
    }

    /// Back from an event or a day: to where it was opened from, else to the
    /// agenda.
    fn go_back(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.mode = self.before.take().unwrap_or(Mode::Agenda);
        self.refresh(host)?;
        self.focus_first();
        Ok(())
    }

    /// A step to the day or week before (-1) or after (1).
    fn step_view(&mut self, host: &Host<'_>, by: i32) -> Result<(), Error> {
        self.mode = match self.mode {
            Mode::Day(day) => Mode::Day(day + by),
            Mode::Week(start) => Mode::Week(start + 7 * by),
            other => other,
        };
        self.refresh(host)?;
        if let Some(top) = self.screens.top(SCREEN_TOKEN) {
            // The step stays under the finger.
            let at = top.list.focus();
            top.list.set_focus(at);
        }
        Ok(())
    }

    /// A view chosen from the menu.
    pub(crate) fn set_view(&mut self, host: &Host<'_>, mode: Mode) -> Result<(), Error> {
        self.before = None;
        self.mode = mode;
        self.refresh(host)?;
        self.focus_first();
        Ok(())
    }

    /// The days a view chosen from the menu starts at.
    pub(crate) fn view_start(&self, host: &Host<'_>) -> Result<(i32, i32), Error> {
        let ctx = self.ctx(host)?;
        let day = match self.mode {
            Mode::Day(day) | Mode::Month(day) => day,
            Mode::Week(start) => start,
            _ => ctx.today,
        };
        Ok((day, ctx.week_start.into()))
    }

    // The widgets' stage: their height, their place and what a tap means.

    fn stage(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let instance = match call {
            WidgetCall::Measure { instance, .. }
            | WidgetCall::Place { instance, .. }
            | WidgetCall::Targets { instance, .. } => instance,
            _ => return Err(Error::Unsupported),
        };
        self.load(host)?;
        let placed = self.find(instance.get()).ok_or(Error::NotFound)?;
        let (token, widget) = (placed.token, copy(&placed.widget)?);
        let env = self.env(host)?;
        let mut out = buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { .. } => {
                let mut cx = Cx::open(host, &widget)?;
                let height = u32::from(widgets::height(&mut cx, token, &self.store, &env)?);
                call.reply_size(height, height, &mut out)?
            }
            WidgetCall::Targets { rect, .. } => {
                let mut cx = Cx::open(host, &widget)?;
                let targets: Vec<WidgetTarget> =
                    widgets::targets(&mut cx, area_of(rect), token, (&self.store, &env))?;
                call.reply_targets(&targets, &mut out)?
            }
            _ => call.reply_place(&mut out)?,
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    // Syncing.

    /// What a source's password or address changed: it is read again.
    pub(crate) fn source_changed(&mut self, id: u8) {
        if let Some(source) = self.store.source_mut(id) {
            source.state = state::NEVER;
        }
        self.message = String::new();
    }

    /// Whether some source is read over the network.
    fn online_sources(&self) -> bool {
        self.store.sources.iter().any(|source| source.kind.online())
    }

    /// *Sync now* (`only`: one source): a run that says how it ended.
    pub(crate) fn sync_now(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.sync_source(host, None)
    }

    pub(crate) fn sync_source(&mut self, host: &Host<'_>, only: Option<u8>) -> Result<(), Error> {
        if self.store.sources.is_empty() {
            notice(host, "calendar.sync.no-source");
            return open(host, SOURCES);
        }
        if self.sync.running() {
            return Ok(());
        }
        self.only = only;
        // A calendar file of the card is read with the radio off; only a run
        // that reaches over the network asks for it.
        let spec = WorkSpec::new(SYNC_KEY, WORK_TOKEN)?;
        let online = self
            .store
            .sources
            .iter()
            .any(|source| only.is_none_or(|id| id == source.id) && source.kind.online());
        let spec = if online { spec.network() } else { spec };
        self.manual = self.handles.declare(host, &spec)?;
        if !self.manual {
            // The network is turned off for the plugin, or no work is taken.
            notice(host, "calendar.sync.offline");
        }
        Ok(())
    }

    /// The sources are read again in an hour, if the radio is on then
    /// anyway (and when charging begins, as the reader chose).
    fn refresh_later(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut choice = [0; 128];
        let chosen = match read_setting(host, SYNC, &mut choice)? {
            SettingValue::Text(text) => text,
            _ => SYNC_CHOICES[1],
        };
        if chosen == SYNC_CHOICES[0] || !self.online_sources() {
            return Ok(());
        }
        let mut spec = WorkSpec::when_cheap(REFRESH_KEY, WORK_TOKEN)?
            .after_ms(REFRESH_MS)
            .at_moment(Moment::Online)
            .at_moment(Moment::Wake);
        if chosen == SYNC_CHOICES[2] {
            spec = spec.at_moment(Moment::Charging);
        }
        self.handles.declare(host, &spec).map(|_| ())
    }

    /// The calendar was opened: the sources are read, but not every time.
    fn sync_on_open(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let now = clock::micros(host)? / 1000;
        let recent = self.opened_ms != 0 && now.saturating_sub(self.opened_ms) < OPEN_EVERY_MS;
        if recent || self.sync.running() || !self.online_sources() {
            return Ok(());
        }
        self.opened_ms = now.max(1);
        self.only = None;
        let spec = WorkSpec::new(SYNC_KEY, WORK_TOKEN)?.network();
        self.handles.declare(host, &spec).map(|_| ())
    }

    /// The work is due: a run starts.
    fn run(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        if self.kept != Kept::Read || self.sync.running() {
            return Ok(());
        }
        let only = self.only.take();
        let auto = !self.manual;
        let mut wire = HostWire::new(host, &mut self.handles);
        let step = self.sync.start(&mut self.store, &mut wire, only, auto);
        self.stepped(host, step)
    }

    fn http_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let ended = match done.reason() {
            Reason::Ok => Ended::Status(done.status()),
            Reason::NoNetwork
            | Reason::Offline
            | Reason::Saving
            | Reason::NetworkOff
            | Reason::WorkEnded => Ended::Offline,
            _ => Ended::Failed,
        };
        let mut wire = HostWire::new(host, &mut self.handles);
        let step = self
            .sync
            .done(&mut self.store, &mut wire, done.id().get(), ended);
        self.stepped(host, step)
    }

    fn stepped(&mut self, host: &Host<'_>, step: Step) -> Result<(), Error> {
        let report = match step {
            Step::Waiting => None,
            Step::Ended(report) => Some(report),
        };
        if let Some(report) = report {
            self.ended(host, &report)?;
        }
        if self.sync.take_dirty() || report.is_some() {
            self.refresh(host)?;
            self.watch_time(host)?;
        }
        Ok(())
    }

    /// A run ended: said where it was asked for, kept for the sources'
    /// screen, and the next one declared.
    fn ended(&mut self, host: &Host<'_>, report: &Report) -> Result<(), Error> {
        let manual = core::mem::take(&mut self.manual);
        let mut say = Messages::new()?;
        let count = |n: u8| [Arg::Count(i64::from(n))];
        let text = match report.outcome {
            Outcome::Synced if report.denied > 0 => {
                say.format(host, "calendar.sync.denied", &count(report.denied))?
            }
            Outcome::Synced if report.failed > 0 => {
                say.format(host, "calendar.sync.some-failed", &count(report.failed))?
            }
            Outcome::Synced if report.skipped > 0 => say.format(
                host,
                "calendar.sync.skipped",
                &[Arg::Count(i64::from(report.skipped))],
            )?,
            Outcome::Synced => say.copy(host, "calendar.sync.done")?,
            Outcome::NoSource => say.copy(host, "calendar.sync.no-source")?,
            Outcome::Offline => say.copy(host, "calendar.sync.offline")?,
            Outcome::NoClock => say.copy(host, "calendar.sync.no-clock")?,
            Outcome::Failed => say.copy(host, "calendar.sync.failed")?,
        };
        if manual {
            notify(host, &text);
        }
        self.message = text;
        self.refresh_later(host)
    }
}

/// The place of the first event of a list's rows (else of its first row).
fn first_event(rows: &[Row]) -> usize {
    rows.iter()
        .position(|row| matches!(row, Row::Event(_)))
        .unwrap_or(0)
}

fn weekday_name_of(host: &Host<'_>, say: &mut Messages, n: u8) -> Result<String, Error> {
    weekday_name(host, say, n, true)
}

/// The rectangle of a drawing as the widgets' own.
fn area_of(rect: FrameRect) -> Rect {
    let clamp = paperpaw_ui_kit::geometry::clamp_u16;
    Rect::new(
        clamp(i64::from(rect.x)),
        clamp(i64::from(rect.y)),
        clamp(i64::from(rect.width)),
        clamp(i64::from(rect.height)),
    )
}

/// Opens one of the plugin's screens.
pub(crate) fn open(host: &Host<'_>, screen: &str) -> Result<(), Error> {
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open_screen(screen)
}

/// The notice `key`, shown to the reader. A courtesy: one that cannot be shown
/// (no text, no room, the host refuses) changes nothing else.
pub(crate) fn notice(host: &Host<'_>, key: &str) {
    let text = Messages::new().and_then(|mut say| say.copy(host, key));
    if let Ok(text) = text {
        notify(host, &text);
    }
}

pub(crate) fn notify(host: &Host<'_>, text: &str) {
    let Ok(mut scratch) = buffer(1024) else {
        return;
    };
    if let Ok(notice) = Notify::new(text) {
        let _ = CommandSender::new(host, &mut scratch).notify(notice);
    }
}
