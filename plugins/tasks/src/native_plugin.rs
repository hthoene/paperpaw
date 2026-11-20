//! The interface-7 Tasks plugin: the lists and tasks (`model`) kept in a file
//! of the data folder (`keep`), shown on two screens of `core.ui` rows
//! (`native_screens`, the dialogs in `native_asks`) and in four widgets
//! (`native_widgets`), and kept in step with a CalDAV server by `sync` over
//! `core.net`, run as declared work (`core.work`), the password kept by
//! `core.secrets`.
//!
//! This file is the glue: it routes the host's events and callbacks, reads
//! the clock and the settings, is the engine's [`Wire`], declares the work and
//! tells the screens and the widgets when the tasks changed (`PORTING-7.md`).
//! No key press waits for the network or reads the card: a change is made in
//! memory, written to the card once, and sent by a work run later.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input},
        clock,
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, kinds},
        formatting::Arg,
        net::{
            FileRoot, Header, HttpDone, Net, REPLY as NET_REPLY, REQUEST_INPUT, Reason, Request,
            RequestId,
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
    files::Volume,
    mem::{self, buffer, wipe},
    paint, read_event,
    text::copy,
    widget::{Cx, cx::zone_offset, settings::base},
};

use crate::declarations::{
    ACCOUNT_TOKEN, ADDRESS, COUNT_TOKEN, HTTP_TOKEN, KEEP_DONE, KEEP_DONE_DEFAULT, KEEP_DONE_MOST,
    LIFECYCLE_TOKEN, LIST_TOKEN, NEXT_TOKEN, OPEN_TOKEN, SCREEN_TOKEN, SLEEP_TOKEN, SYNC_TOKEN,
    TODAY_TOKEN, USER, WORK_TOKEN,
};
use crate::keep::{self, Loaded};
use crate::model::{FIRST, Seed, Store, View as Tasks};
use crate::native_asks::Asked;
use crate::native_screens::{self as screens, Account, Row, View};
use crate::native_secret::{self as secret, PASSWORD, Secret};
use crate::native_widgets::{self as widgets, OPEN_TARGET};
use crate::sync::{Body, Call, Ended, Outcome, Report, Step, Sync, Wire};

mod image {
    include!(concat!(env!("OUT_DIR"), "/tasks_declarations.rs"));
}

/// The most bytes of a stage reply (a size, a place, the targets of the rows).
const REPLY: usize = 1024;
/// The work that syncs, the one that does so only when the radio is on
/// anyway, and the one that tells the widgets the day has turned.
const SYNC_KEY: &str = "tasks.sync";
const REFRESH_KEY: &str = "tasks.refresh";
const DAY_KEY: &str = "tasks.day";
/// The screens' ids.
pub(crate) const SCREEN: &str = "tasks.screen";
pub(crate) const ACCOUNT: &str = "tasks.account";
/// What was changed is sent within half an hour, sooner when the network is
/// there anyway.
const LATER_MS: u32 = 30 * 60 * 1000;
/// The server is read again an hour on, when the radio is on anyway.
const REFRESH_MS: u32 = 60 * 60 * 1000;
/// Opening the lists syncs at most this often.
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

/// The text setting `key`, without blanks around it.
pub(crate) fn text_setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
    let mut scratch = buffer(1100)?;
    match read_setting(host, key, &mut scratch)? {
        SettingValue::Text(text) => copy(text.trim()),
        _ => Ok(String::new()),
    }
}

/// The server's address as requests use it: with `https://` unless it names
/// a scheme, and a `/` at its end (a collection's address has one). Empty
/// where none is set.
pub(crate) fn address(host: &Host<'_>) -> Result<String, Error> {
    let typed = text_setting(host, ADDRESS)?;
    let base = paperpaw_sdk::web::try_base_url(&typed).map_err(|_| Error::NoMemory)?;
    if base.is_empty() {
        return Ok(base);
    }
    paperpaw_ui_kit::text::concat(&[&base, "/"])
}

/// Seconds since 1970 if the clock is set, and today's number (days since
/// 1970 by the reader's own time; 0 without a clock).
pub(crate) fn clock_now(host: &Host<'_>) -> Result<(Option<i64>, i32), Error> {
    let Some(unix) = capture_snapshot(host)?.unix_seconds() else {
        return Ok((None, 0));
    };
    let local = unix.saturating_add(i64::from(zone_offset(host, unix)?));
    Ok((
        Some(unix),
        i32::try_from(local.div_euclid(86_400)).unwrap_or(0),
    ))
}

/// How many days a done task stays.
fn keep_days(host: &Host<'_>) -> Result<i32, Error> {
    let mut scratch = [0; 64];
    let days = match read_setting(host, KEEP_DONE, &mut scratch)? {
        SettingValue::Int(days) => days,
        _ => KEEP_DONE_DEFAULT,
    };
    Ok(i32::try_from(days).unwrap_or(0).clamp(0, KEEP_DONE_MOST))
}

/// The host as the engine uses it.
struct HostWire<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a mut Handles,
    data: Data<'a, 'h>,
}

impl<'a, 'h> HostWire<'a, 'h> {
    fn new(host: &'a Host<'h>, handles: &'a mut Handles) -> Self {
        Self {
            host,
            handles,
            data: Data(host),
        }
    }

    /// The header that says who asks, to be wiped: none without a name or a
    /// password (a server that asks for none).
    fn proof(&mut self) -> Result<Option<String>, Error> {
        let user = text_setting(self.host, USER)?;
        let password: Option<Secret> = match self.handles.secrets(self.host) {
            Ok(secrets) => secret::read(secrets, self.host, PASSWORD)?,
            // A device that keeps no secrets has no password.
            Err(Error::Unsupported | Error::Denied) => None,
            Err(error) => return Err(error),
        };
        let Some(password) = password.filter(|_| !user.is_empty()) else {
            return Ok(None);
        };
        let line = paperpaw_sdk::web::basic_auth(&user, password.as_str());
        let value = copy(&line);
        mem::wipe_text(line);
        value.map(Some)
    }

    fn request(
        &mut self,
        call: &Call<'_>,
        proof: Option<&str>,
        (input, out): (&mut [u8], &mut [u8]),
    ) -> Result<u32, Error> {
        let mut headers: Vec<Header<'_>> = Vec::new();
        headers.try_reserve(5).map_err(|_| Error::NoMemory)?;
        if let Some(proof) = proof {
            headers.push(Header::new("Authorization", proof)?);
        }
        if call.deep {
            headers.push(Header::new("Depth", "1")?);
        }
        if let Some(version) = call.if_match {
            headers.push(Header::new("If-Match", version)?);
        }
        if call.only_new {
            headers.push(Header::new("If-None-Match", "*")?);
        }
        if !call.content_type.is_empty() {
            headers.push(Header::new("Content-Type", call.content_type)?);
        }
        // Answers are saved to files or are a status alone: little is kept.
        let mut request = Request::new(HTTP_TOKEN, call.method, call.url)?
            .headers(&headers)?
            .max_body(256);
        request = match call.body {
            Body::None => request,
            Body::Text(text) => request.body(text.as_bytes())?,
            Body::File(path) => request.send_file(FileRoot::Data, path)?,
        };
        if let Some((path, most)) = call.save_to {
            request = request.save_to(FileRoot::Data, path)?.max_file(most)?;
        }
        let net = self.handles.net(self.host)?;
        Ok(net.request(self.host, &request, input, out)?.get())
    }
}

impl Wire for HostWire<'_, '_> {
    fn files(&self) -> &dyn Volume {
        &self.data
    }

    fn send(&mut self, call: &Call<'_>) -> Result<u32, Error> {
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(NET_REPLY)?);
        let proof = self.proof()?;
        let sent = self.request(call, proof.as_deref(), (&mut input, &mut out));
        // The input held the password in a header.
        if let Some(proof) = proof {
            mem::wipe_text(proof);
        }
        wipe(&mut input);
        wipe(&mut out);
        sent
    }

    fn etag(&mut self, id: u32) -> Result<String, Error> {
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(NET_REPLY)?);
        let net = self.handles.net(self.host)?;
        let answer = net.answer(self.host, RequestId::new(id)?, &mut input, &mut out)?;
        copy(answer.etag())
    }

    fn now(&mut self) -> Option<i64> {
        capture_snapshot(self.host).ok()?.unix_seconds()
    }

    fn zone(&mut self, unix: i64) -> i32 {
        zone_offset(self.host, unix).unwrap_or(0)
    }

    fn seed(&mut self) -> Seed {
        seed(self.host)
    }
}

/// What a new task's id is made of.
pub(crate) fn seed(host: &Host<'_>) -> Seed {
    Seed {
        unix: capture_snapshot(host)
            .ok()
            .and_then(|snapshot| snapshot.unix_seconds())
            .unwrap_or(0),
        micros: clock::micros(host).unwrap_or(0),
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

/// The interface-7 Tasks plugin.
#[derive(Debug, Default)]
pub struct NativeTasks {
    pub(crate) store: Store,
    kept: Kept,
    sync: Sync,
    pub(crate) handles: Handles,
    pub(crate) screens: Screens<View>,
    placed: Vec<Placed>,
    /// The question that is open, and what it was for.
    pub(crate) asked: Option<(AskId, Asked)>,
    /// What the lists' screen shows; the first list until one is chosen.
    pub(crate) shown: Option<Tasks>,
    /// How the last run went, in words; empty until one has.
    message: String,
    /// The run under way was asked for: how it ends is said.
    manual: bool,
    /// When opening the lists last started a run, on the host's clock.
    opened_ms: u64,
}

impl Plugin for NativeTasks {
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
        let ours = token == SCREEN_TOKEN || token == ACCOUNT_TOKEN;
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
            Event::ActionRun { .. } if token == OPEN_TOKEN => open(host, SCREEN),
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
        if token == SCREEN_TOKEN || token == ACCOUNT_TOKEN {
            return self.draw_screen(host, token, &mut frame);
        }
        if token == SLEEP_TOKEN {
            self.load(host)?;
            return crate::native_sleep::draw(host, &mut frame, &self.store);
        }
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        self.load(host)?;
        let placed = self.find(frame.id).ok_or(Error::NotFound)?;
        let (token, widget) = (placed.token, copy(&placed.widget)?);
        let (_, today) = clock_now(host)?;
        let mut cx = Cx::open(host, &widget)?;
        let mut out = Vec::new();
        widgets::draw(
            &mut cx,
            &mut out,
            area_of(frame.rect),
            token,
            (&self.store, today),
        )?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == SCREEN_TOKEN || token == ACCOUNT_TOKEN {
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
        // A tap on a task ticks it off; anything else opens the lists.
        match (kind, widgets::task_of(&self.store, target)) {
            (WidgetInputKind::Tap, Some(index)) if target != OPEN_TARGET => {
                self.toggle(host, index)?;
            }
            (WidgetInputKind::Tap | WidgetInputKind::LongPress, _) => open(host, SCREEN)?,
            _ => return Ok(Handled::No),
        }
        Ok(Handled::Yes)
    }
}

impl NativeTasks {
    fn find(&self, instance: u32) -> Option<&Placed> {
        self.placed
            .iter()
            .find(|placed| placed.instance.get() == instance)
    }

    // The file.

    /// Reads the file once. A card that cannot be read now is tried again.
    fn load(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.kept != Kept::NotRead {
            return Ok(());
        }
        match keep::load(&Data(host)) {
            Ok(Loaded::Store(store)) => {
                self.store = store;
                self.kept = Kept::Read;
                let (_, today) = clock_now(host)?;
                if today > 0 {
                    self.store.purge(today, keep_days(host)?);
                }
            }
            Ok(Loaded::Unreadable) => self.kept = Kept::Unreadable,
            // A host without a card: the tasks live while it runs.
            Err(Error::Unsupported) => self.kept = Kept::Read,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
        Ok(())
    }

    /// Whether the tasks can be changed; says why not, if not.
    pub(crate) fn usable(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        self.load(host)?;
        if self.kept != Kept::Read {
            notice(host, "tasks.unreadable");
        }
        Ok(self.kept == Kept::Read)
    }

    /// The tasks were changed on the reader: written to the card, shown anew,
    /// and sent when the network allows.
    pub(crate) fn changed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        match keep::save(&Data(host), &self.store) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => notice(host, "tasks.not-saved"),
        }
        self.refresh(host)?;
        if self.store.waiting() > 0 && !self.sync.blocked() {
            self.sync_later(host)?;
        }
        Ok(())
    }

    /// Ticks the task at `index` off, or on again.
    pub(crate) fn toggle(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        if !self.usable(host)? {
            return Ok(());
        }
        let (now, _) = clock_now(host)?;
        match self.store.toggle(index, now.unwrap_or(0)) {
            Ok(()) => self.changed(host),
            Err(Error::Unsupported) => {
                notice(host, "tasks.fixed");
                Ok(())
            }
            Err(_) => Ok(()),
        }
    }

    // What is shown.

    /// Builds the rows of the open screens anew, tells the widgets and asks
    /// for the screen on top to be drawn.
    pub(crate) fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let (_, today) = clock_now(host)?;
        if self.screens.top(SCREEN_TOKEN).is_some() {
            let view = screens::shown(&self.store, self.shown.unwrap_or(Tasks::List(0)));
            let built = screens::main_rows(host, &self.store, view, (today, self.sync.running()))?;
            if let Some(top) = self.screens.top(SCREEN_TOKEN) {
                top.set(built);
            }
        }
        if self.screens.top(ACCOUNT_TOKEN).is_some() {
            let built = self.account_rows(host)?;
            if let Some(top) = self.screens.top(ACCOUNT_TOKEN) {
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

    fn account_rows(&mut self, host: &Host<'_>) -> Result<screens::Built, Error> {
        let address = text_setting(host, ADDRESS)?;
        let user = text_setting(host, USER)?;
        let has_password = match self.handles.secrets(host) {
            Ok(secrets) => secret::present(secrets, host, PASSWORD)?,
            Err(Error::Unsupported | Error::Denied) => false,
            Err(error) => return Err(error),
        };
        screens::account_rows(
            host,
            &self.store,
            &Account {
                address: &address,
                user: &user,
                has_password,
                message: &self.message,
                syncing: self.sync.running(),
            },
        )
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
        if address(host)?.is_empty() {
            return Ok(());
        }
        if self.store.waiting() > 0 {
            self.sync_later(host)?;
        }
        self.refresh_later(host)
    }

    /// The day turned (or may have): what is due today is another set.
    fn day_turned(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.kept == Kept::Read {
            let (_, today) = clock_now(host)?;
            if today > 0 && self.store.purge(today, keep_days(host)?) {
                // What went is done long ago: the file follows at the next change.
                self.refresh(host)?;
            }
        }
        self.tell_widgets(host);
        self.watch_day(host)
    }

    /// Declares the work that comes due just after midnight, while a widget
    /// is placed.
    fn watch_day(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.placed.is_empty() {
            return Ok(());
        }
        let Some(unix) = capture_snapshot(host)?.unix_seconds() else {
            return Ok(());
        };
        let local = unix.saturating_add(i64::from(zone_offset(host, unix)?));
        let left = 86_400 - local.rem_euclid(86_400) + 30;
        let ms = u32::try_from(left * 1000)
            .unwrap_or(SPAN_MAX_MS)
            .min(SPAN_MAX_MS);
        let spec = WorkSpec::new(DAY_KEY, WORK_TOKEN)?.after_ms(ms);
        self.handles.declare(host, &spec).map(|_| ())
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if !(key.is_empty() || key.starts_with("tasks.")) {
            return Ok(());
        }
        if key == ADDRESS || key == USER {
            // Another account: what was known of the server no longer holds.
            self.sync.account_changed();
            self.message = String::new();
            if self.kept == Kept::Read {
                self.store.forget_server();
                keep::save(&Data(host), &self.store).ok();
            }
        }
        self.refresh(host)
    }

    fn made(&mut self, host: &Host<'_>, widget: &str, instance: Instance) -> Result<(), Error> {
        let token = match base(widget) {
            "tasks.today" => TODAY_TOKEN,
            "tasks.list" => LIST_TOKEN,
            "tasks.count" => COUNT_TOKEN,
            "tasks.next" => NEXT_TOKEN,
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
        self.watch_day(host)
    }

    // The screens.

    fn opened(&mut self, host: &Host<'_>, token: u32, instance: Instance) -> Result<(), Error> {
        self.load(host)?;
        let mut view = View::default();
        if token == SCREEN_TOKEN {
            self.first_list(host)?;
            let (_, today) = clock_now(host)?;
            let shown = screens::shown(&self.store, self.shown.unwrap_or(Tasks::List(0)));
            view.set(screens::main_rows(
                host,
                &self.store,
                shown,
                (today, self.sync.running()),
            )?);
            // The first task has the focus, where there is one.
            view.list.set_focus(if view.rows.len() > 2 { 2 } else { 1 });
        } else {
            view.set(self.account_rows(host)?);
        }
        self.screens.open(instance, view)?;
        if token == SCREEN_TOKEN {
            self.sync_on_open(host)?;
        }
        Ok(())
    }

    /// A reader without a list gets one, so that a task can be added at once.
    fn first_list(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.kept != Kept::Read || !self.store.lists.is_empty() {
            return Ok(());
        }
        let name = Messages::new()?.copy(host, "tasks.list.first")?;
        let id = self.store.add_list(&name, "")?;
        if let Some(list) = self.store.list_mut(id) {
            // Marked: it goes if the account's lists come before it is used.
            list.ctag = copy(FIRST)?;
        }
        keep::save(&Data(host), &self.store).ok();
        Ok(())
    }

    fn draw_screen(
        &mut self,
        host: &Host<'_>,
        token: u32,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let view = self.screens.top(token).ok_or(Error::State)?;
        if token == ACCOUNT_TOKEN {
            let title = say.copy(host, "tasks.account.title")?;
            let help = match view.rows.get(view.list.focus()) {
                Some(row) => say.copy(host, row.help())?,
                None => String::new(),
            };
            return view
                .list
                .draw_with(host, frame, &title, true, Footer::Note(&help));
        }
        let title = say.copy(host, "tasks.title")?;
        let footer = if view.rows.len() <= 2 {
            Footer::Empty {
                title: &view.empty.0,
                text: &view.empty.1,
            }
        } else {
            Footer::Note(&view.hint)
        };
        view.list.draw_with(host, frame, &title, true, footer)
    }

    fn screen_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // Select held, or the menu button, offers what else can be done with
        // a task.
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
            // On the account's screen the note below follows the focus.
            if token == ACCOUNT_TOKEN && matches!(step, ItemStep::Focus(_)) {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch)
                    .queue_redraw(RedrawMode::Partial)
                    .ok();
            }
            return Ok(ItemList::handled(step));
        };
        let Some(row) = view.rows.get(at).copied() else {
            return Ok(Handled::Yes);
        };
        let held = matches!(step, ItemStep::Held(_));
        match row {
            Row::Task(index) if held => self.offer(host, usize::from(index))?,
            Row::Task(index) => self.toggle(host, usize::from(index))?,
            Row::Lists => self.ask_lists(host)?,
            Row::Add => self.ask_add(host)?,
            Row::Sync => self.sync_now(host)?,
            Row::Address | Row::User | Row::Password => self.ask_field(host, row)?,
        }
        Ok(Handled::Yes)
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
        let mut out = buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { .. } => {
                let (_, today) = clock_now(host)?;
                let mut cx = Cx::open(host, &widget)?;
                let height = u32::from(widgets::height(&mut cx, token, &self.store, today)?);
                call.reply_size(height, height, &mut out)?
            }
            WidgetCall::Targets { rect, .. } => {
                let (_, today) = clock_now(host)?;
                let mut cx = Cx::open(host, &widget)?;
                let targets: Vec<WidgetTarget> =
                    widgets::targets(&mut cx, area_of(rect), token, (&self.store, today))?;
                call.reply_targets(&targets, &mut out)?
            }
            _ => call.reply_place(&mut out)?,
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    // Syncing.

    /// The account's password is another: the server is asked again.
    pub(crate) fn account_changed(&mut self) {
        self.sync.account_changed();
        self.message = String::new();
    }

    /// *Sync now*: a run that says how it ended.
    pub(crate) fn sync_now(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if address(host)?.is_empty() {
            notice(host, "tasks.sync.no-account");
            return open(host, ACCOUNT);
        }
        self.sync.account_changed();
        let spec = WorkSpec::new(SYNC_KEY, WORK_TOKEN)?.network();
        self.manual = self.handles.declare(host, &spec)?;
        if !self.manual {
            // The network is turned off for the plugin, or no work is taken.
            notice(host, "tasks.sync.offline");
        }
        Ok(())
    }

    /// What was changed is sent within half an hour, or when the network
    /// comes up.
    fn sync_later(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.sync.running() || self.manual || address(host)?.is_empty() {
            return Ok(());
        }
        let spec = WorkSpec::new(SYNC_KEY, WORK_TOKEN)?
            .network()
            .within_ms(LATER_MS)
            .at_moment(Moment::Online);
        self.handles.declare(host, &spec).map(|_| ())
    }

    /// The server is read again in an hour, if the radio is on then anyway.
    fn refresh_later(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let spec = WorkSpec::when_cheap(REFRESH_KEY, WORK_TOKEN)?
            .after_ms(REFRESH_MS)
            .at_moment(Moment::Online)
            .at_moment(Moment::Wake);
        self.handles.declare(host, &spec).map(|_| ())
    }

    /// The lists were opened: the server is read, but not every time.
    fn sync_on_open(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let now = clock::micros(host)? / 1000;
        let recent = self.opened_ms != 0 && now.saturating_sub(self.opened_ms) < OPEN_EVERY_MS;
        if recent || self.sync.running() || self.sync.blocked() || address(host)?.is_empty() {
            return Ok(());
        }
        self.opened_ms = now.max(1);
        let spec = WorkSpec::new(SYNC_KEY, WORK_TOKEN)?.network();
        self.handles.declare(host, &spec).map(|_| ())
    }

    /// The work is due: a run starts.
    fn run(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        if self.kept != Kept::Read || self.sync.running() {
            return Ok(());
        }
        let address = address(host)?;
        let days = keep_days(host)?;
        let mut wire = HostWire::new(host, &mut self.handles);
        let step = self.sync.start(&mut self.store, &mut wire, &address, days);
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
        }
        Ok(())
    }

    /// A run ended: said where it was asked for or the reader should know,
    /// kept for the account's screen, and the next one declared.
    fn ended(&mut self, host: &Host<'_>, report: &Report) -> Result<(), Error> {
        let manual = core::mem::take(&mut self.manual);
        let mut say = Messages::new()?;
        let count = |n: u16| [Arg::Count(i64::from(n))];
        let text = match report.outcome {
            Outcome::Synced if report.conflicts > 0 => {
                say.format(host, "tasks.sync.conflicts", &count(report.conflicts))?
            }
            Outcome::Synced if report.failed > 0 => {
                say.format(host, "tasks.sync.some-failed", &count(report.failed))?
            }
            Outcome::Synced if report.skipped > 0 => {
                say.format(host, "tasks.sync.skipped", &count(report.skipped))?
            }
            Outcome::Synced => say.copy(host, "tasks.sync.done")?,
            Outcome::NoAccount => say.copy(host, "tasks.sync.no-account")?,
            Outcome::Offline => say.copy(host, "tasks.sync.offline")?,
            Outcome::Denied => say.copy(host, "tasks.sync.denied")?,
            Outcome::NotFound => say.copy(host, "tasks.sync.not-found")?,
            Outcome::NoLists => say.copy(host, "tasks.sync.no-lists")?,
            Outcome::Failed(status) => say.format(
                host,
                "tasks.sync.failed",
                &[Arg::Int {
                    name: "status",
                    value: i64::from(status),
                }],
            )?,
        };
        // A run nobody asked for says only what the reader has to know: that
        // a task of theirs was kept beside the server's.
        if manual || (report.outcome == Outcome::Synced && report.conflicts > 0) {
            notify(host, &text);
        }
        self.message = text;
        if address(host)?.is_empty() {
            return Ok(());
        }
        if self.store.waiting() > 0 && !self.sync.blocked() {
            self.sync_later(host)?;
        }
        self.refresh_later(host)
    }
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
