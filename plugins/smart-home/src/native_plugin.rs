//! The interface-7 smart home plugin: the entities the reader chose, kept in a
//! small file and shown by the dashboard and four widgets; their states
//! fetched from Home Assistant over `core.net` (the whole list saved to a file
//! and read in pieces, only the chosen entities kept) and switched by a
//! service call; the entity picker, the connection and the keyboard.
//!
//! What the plugin decides without a host is in `model`, `states`, `link`; the
//! screens are `dash`, `pick` and `connect`, the widgets `native_widgets`, the
//! requests `fetch`. This file is the glue: it routes the host's events and
//! callbacks, reads the settings and the token, and tells the widgets and the
//! screens when what they show has changed (`PORTING-7.md`).
use crate::{
    connect::{self, Connect, Row as ConnectRow},
    dash::{self, Dash, Update},
    declarations::{
        ANYWHERE, CHOOSE_TOKEN, CONNECT_SCREEN, DASH_SCREEN, GROUP_TOKEN, HTTP_TOKEN,
        LIFECYCLE_TOKEN, OPEN_TOKEN, PICK_SCREEN, REFRESH, REFRESH_TOKEN, ROOM_TOKEN, SERVER,
        SWITCH_TOKEN, VALUE_TOKEN, WORK_TOKEN,
    },
    fetch::{self, Ended, Fetcher, Why},
    keep::{self, Secret, TOKEN},
    link::{self, Problem, Refused},
    model::{self, Entity, FILE, FILE_CAP, Kind, ROOM_MAX, Store},
    native_widgets,
    pick::{self, Finder, Load, Pick},
    say::{HostSay, Say, value_text},
    states::{Record, States},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input},
        clock,
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        formatting::Arg,
        net::HttpDone,
        registrations,
        reply::Reply,
        secrets::Secrets,
        settings::{SettingValue, read_setting},
        snapshot::capture_snapshot,
        storage::Root,
        ui::{
            Instance, Notify, Question, RequestId, TextMode, TextWrite, Ui, WidgetInputKind,
            WidgetTarget, stages::WidgetCall,
        },
    },
};
use paperpaw_ui_kit::{
    Screens, files,
    mem::{SERVICE_BUFFER, buffer, wipe},
    paint, read_event, text,
    widget::{
        Cx,
        settings::{base, text_of},
    },
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/smart_home_declarations.rs"));
}

/// The most bytes of a stage reply (a size and a target).
const REPLY: usize = 512;
/// The target of a widget: all of it.
const TARGET: u32 = 1;
/// The screens' ids, for opening them.
const DASH: &str = "smart-home.screen";
const PICK: &str = "smart-home.pick";
/// The longest token kept (the header must fit a request).
const MAX_TOKEN: usize = 900;
/// The longest a setting's text is read.
const SETTING_CAP: usize = 256;
/// A wake fetches only states older than this, in seconds.
const STALE_SECS: i64 = 120;

/// How the last fetch went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Last {
    #[default]
    Never,
    Done,
    Failed(Problem),
}

/// What the keyboard was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ask {
    Search,
    /// The room of the entity with this id.
    Room(String),
    Server,
    Token,
}

/// A widget instance the host made.
#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
}

/// The interface-7 smart home plugin.
#[derive(Debug, Default)]
pub struct NativeSmartHome {
    store: Store,
    loaded: bool,
    fetcher: Fetcher,
    items: Vec<Item>,
    dash: Screens<Dash>,
    pick: Screens<Pick>,
    connect: Screens<Connect>,
    ui: Option<Ui>,
    secrets: Option<Secrets>,
    secrets_tried: bool,
    asking: Option<(RequestId, Ask)>,
    /// Power is plugged in (charging or full on the charger).
    powered: bool,
    last: Last,
    /// A digest of what the widgets showed when they were last told.
    told: Option<u32>,
}

impl Plugin for NativeSmartHome {
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
            kinds::ChargeChanged,
            kinds::HttpDone,
            kinds::WorkDue,
            kinds::ActionRun,
            kinds::WidgetMade,
            kinds::WidgetDropped,
            kinds::StageCall,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::Answered
        ) else {
            return Ok(());
        };
        match event {
            Event::Started { .. } if token == LIFECYCLE_TOKEN => self.started(host),
            Event::Resume { .. } if token == LIFECYCLE_TOKEN => self.resumed(host),
            Event::SettingChanged { key, .. } if token == LIFECYCLE_TOKEN => {
                self.setting_changed(host, key)
            }
            Event::ChargeChanged { flags, .. } if token == LIFECYCLE_TOKEN => {
                self.charge_changed(host, flags.external_power() || flags.charging())
            }
            Event::HttpDone(done) if token == HTTP_TOKEN => self.http_done(host, &done),
            Event::WorkDue(_) if token == WORK_TOKEN => self.work_due(host),
            Event::ActionRun { .. } if token == OPEN_TOKEN => open(host, DASH),
            Event::ActionRun { .. } if token == CHOOSE_TOKEN => open(host, PICK),
            Event::ActionRun { .. } if token == REFRESH_TOKEN => self.refresh(host, Why::Asked),
            Event::WidgetMade {
                widget, instance, ..
            } => self.made(host, widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.items.retain(|item| item.instance != instance);
                self.reschedule(host)
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::ScreenOpened { instance, .. } => self.opened(host, token, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.shown(token, instance),
            Event::ScreenClosed { instance, .. } => self.closed(host, token, instance),
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        self.load(host)?;
        match token {
            DASH_SCREEN => {
                let mut say = HostSay::new(host)?;
                let title = say.copy("smart-home.title")?;
                let empty = (
                    say.copy("smart-home.empty.title")?,
                    say.copy("smart-home.empty.text")?,
                );
                let view = self.dash.top(token).ok_or(Error::State)?;
                view.draw(host, &mut frame, &title, (&empty.0, &empty.1))
            }
            PICK_SCREEN => {
                let mut say = HostSay::new(host)?;
                let title = say.copy("smart-home.pick.title")?;
                let view = self.pick.top(token).ok_or(Error::State)?;
                view.draw(host, &mut frame, &mut say, &title)
            }
            CONNECT_SCREEN => {
                let title = HostSay::new(host)?.copy("smart-home.connect.title")?;
                let view = self.connect.top(token).ok_or(Error::State)?;
                view.draw(host, &mut frame, &title)
            }
            _ => {
                if frame.purpose != DrawPurpose::Widget {
                    return Err(Error::Unsupported);
                }
                let item = self.find(frame.id).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let area = area_of(frame.rect);
                let mut out = Vec::new();
                native_widgets::draw(&mut cx, &mut out, area, item.token, &self.store)?;
                paint::replay(&mut frame, &cx.font, &out)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match token {
            DASH_SCREEN => return self.dash_input(host, &input),
            PICK_SCREEN => return self.pick_input(host, &input),
            CONNECT_SCREEN => return self.connect_input(host, &input),
            _ => {}
        }
        let Input::Widget {
            instance,
            target: TARGET,
            kind,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        let widget = self.find(instance.get()).ok_or(Error::NotFound)?;
        let (token, named) = (widget.token, widget.widget.clone());
        // A tap does the widget's one thing (a switch switches, a value or a
        // group fetches, a room opens the dashboard); a long press opens it.
        match (token, kind) {
            (SWITCH_TOKEN, WidgetInputKind::Tap) => {
                self.load(host)?;
                let name = Cx::open(host, &named)?.settings.string("entity", 100)?;
                let id = self.store.pick(&name, Kind::Switch).map(|e| e.id.clone());
                match id {
                    Some(id) => self.toggle(host, &id)?,
                    None => open(host, DASH)?,
                }
            }
            (VALUE_TOKEN | GROUP_TOKEN, WidgetInputKind::Tap) => {
                self.refresh(host, Why::Asked)?;
            }
            (ROOM_TOKEN, WidgetInputKind::Tap) | (_, WidgetInputKind::LongPress) => {
                open(host, DASH)?;
            }
            _ => return Ok(Handled::No),
        }
        Ok(Handled::Yes)
    }
}

impl NativeSmartHome {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    // What the host says: the clock, the settings, the secrets.

    fn now_ms(host: &Host<'_>) -> Result<u64, Error> {
        Ok(clock::micros(host)? / 1000)
    }

    fn text_setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
        text_of(host, key, |value| {
            let value = value.trim();
            text::copy(value.get(..value.len().min(SETTING_CAP)).unwrap_or(""))
        })?
    }

    fn flag(host: &Host<'_>, key: &str) -> Result<bool, Error> {
        let mut scratch = [0; 64];
        Ok(matches!(
            read_setting(host, key, &mut scratch)?,
            SettingValue::Bool(true)
        ))
    }

    /// How often to fetch while charging, in minutes; 0 for never.
    fn every_minutes(host: &Host<'_>) -> Result<u32, Error> {
        let mut scratch = [0; 64];
        Ok(match read_setting(host, REFRESH, &mut scratch)? {
            SettingValue::Text("smart-home.every.15") => 15,
            SettingValue::Text("smart-home.every.30") => 30,
            SettingValue::Text("smart-home.every.60") => 60,
            SettingValue::Text("smart-home.every.120") => 120,
            SettingValue::Text("smart-home.every.off") => 0,
            _ => 30,
        })
    }

    fn secrets(&mut self, host: &Host<'_>) -> Option<&Secrets> {
        if !self.secrets_tried {
            self.secrets_tried = true;
            self.secrets = Secrets::open(host).ok();
        }
        self.secrets.as_ref()
    }

    fn token_kept(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        match self.secrets(host) {
            Some(secrets) => keep::present(secrets, host, TOKEN),
            None => Ok(false),
        }
    }

    /// The server's base address and the `Authorization` header, or what is
    /// missing.
    fn access(&mut self, host: &Host<'_>) -> Result<Result<(String, Secret), Problem>, Error> {
        let server = Self::text_setting(host, SERVER)?;
        let anywhere = Self::flag(host, ANYWHERE)?;
        let base = match link::base(&server, anywhere) {
            Ok(base) => base,
            Err(Refused::Empty) => return Ok(Err(Problem::NoServer)),
            Err(refused) => return Ok(Err(Problem::Address(refused))),
        };
        let token = match self.secrets(host) {
            Some(secrets) => keep::read(secrets, host, TOKEN)?,
            None => None,
        };
        let Some(token) = token else {
            return Ok(Err(Problem::NoToken));
        };
        Ok(Ok((base, Secret::bearer(token.as_str())?)))
    }

    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    // The kept entities.

    fn load(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.loaded {
            return Ok(());
        }
        self.loaded = true;
        let bytes = files::read_all(host, Root::Data, FILE, FILE_CAP)
            .ok()
            .flatten();
        let kept = match bytes.as_deref().map(core::str::from_utf8) {
            Some(Ok(text)) => Store::from_text(text)?,
            _ => None,
        };
        if let Some(kept) = kept {
            self.store = kept;
            if self.store.synced > 0 || !self.store.entities.is_empty() {
                self.last = Last::Done;
            }
        }
        Ok(())
    }

    fn persist(&self, host: &Host<'_>) {
        if let Ok(text) = self.store.to_text() {
            files::replace(host, Root::Data, FILE, text.as_bytes()).ok();
        }
    }

    /// A digest of what the widgets show of the entities.
    fn digest(&self) -> u32 {
        let mut hash = 0x811c_9dc5_u32;
        let mut eat = |bytes: &[u8]| {
            for byte in bytes.iter().chain(&[0]) {
                hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
            }
        };
        for e in &self.store.entities {
            for part in [&e.id, &e.name, &e.room, &e.state, &e.unit, &e.class] {
                eat(part.as_bytes());
            }
            eat(&[u8::from(e.pending)]);
        }
        eat(&self.store.synced.to_le_bytes());
        hash
    }

    /// How old the states are, in seconds, if the clock says.
    fn age_secs(&self, host: &Host<'_>) -> Option<i64> {
        let now = capture_snapshot(host).ok()?.unix_seconds()?;
        (self.store.synced > 0).then(|| (now - self.store.synced).max(0))
    }

    fn watched(&self) -> bool {
        !self.items.is_empty() || self.dash.top_instance(DASH_SCREEN).is_some()
    }

    // Events.

    fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        for e in &mut self.store.entities {
            e.pending = false;
        }
        self.powered = capture_snapshot(host).is_ok_and(|s| s.is_charging());
        self.reschedule(host)
    }

    /// The wake: fetched only where Wi-Fi is up already, something shows the
    /// states and they are not new.
    fn resumed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        for e in &mut self.store.entities {
            e.pending = false;
        }
        self.powered = capture_snapshot(host).is_ok_and(|s| s.is_charging());
        if self.watched() && self.age_secs(host).is_none_or(|age| age >= STALE_SECS) {
            self.refresh(host, Why::Quiet)?;
        }
        self.reschedule(host)
    }

    fn charge_changed(&mut self, host: &Host<'_>, powered: bool) -> Result<(), Error> {
        let came = powered && !self.powered;
        self.powered = powered;
        self.load(host)?;
        if came && !self.items.is_empty() && self.age_secs(host).is_none_or(|age| age >= STALE_SECS)
        {
            self.refresh(host, Why::Quiet)?;
        }
        self.reschedule(host)
    }

    fn work_due(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        if self.powered && !self.items.is_empty() {
            self.refresh(host, Why::Quiet)?;
        }
        self.reschedule(host)
    }

    /// The fetch while charging: set where widgets show the states, power is
    /// plugged in and a time is set; else taken away. Nothing here wakes the
    /// device or switches the radio on: the work runs when it is on anyway.
    fn reschedule(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let minutes = Self::every_minutes(host)?;
        let after = (self.powered && minutes > 0 && !self.items.is_empty())
            .then(|| minutes.saturating_mul(60_000));
        // A timer that cannot be set costs the next fetch only.
        self.fetcher.schedule(host, after).ok();
        Ok(())
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key.is_empty() || key.starts_with("smart-home.") {
            self.load(host)?;
            self.reschedule(host)?;
            self.status(host)?;
            self.refresh_connect(host)?;
            self.changed(host, true)?;
        }
        Ok(())
    }

    fn made(&mut self, host: &Host<'_>, widget: &str, instance: Instance) -> Result<(), Error> {
        let token = match base(widget) {
            "smart-home.value" => VALUE_TOKEN,
            "smart-home.list" => GROUP_TOKEN,
            "smart-home.switch" => SWITCH_TOKEN,
            "smart-home.room" => ROOM_TOKEN,
            _ => return Ok(()),
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: text::copy(widget)?,
        });
        self.load(host)?;
        self.reschedule(host)
    }

    // The screens.

    fn opened(&mut self, host: &Host<'_>, token: u32, instance: Instance) -> Result<(), Error> {
        self.load(host)?;
        let mut say = HostSay::new(host)?;
        match token {
            DASH_SCREEN => {
                let mut view = Dash::default();
                view.rebuild(&self.store, &mut say)?;
                view.say(self.note(host, &mut say)?);
                self.dash.open(instance, view)?;
                // On open: fetched, wherever the network stands.
                self.refresh(host, Why::Open)
            }
            PICK_SCREEN => {
                let mut view = Pick::default();
                view.tick(
                    self.store
                        .entities
                        .iter()
                        .filter_map(|e| text::copy(&e.id).ok()),
                )?;
                self.pick.open(instance, view)?;
                self.pick_load(host)
            }
            CONNECT_SCREEN => {
                let mut view = Connect::default();
                let server = Self::text_setting(host, SERVER)?;
                let kept = self.token_kept(host)?;
                view.rebuild(&mut say, &server, kept)?;
                self.connect.open(instance, view)
            }
            _ => Ok(()),
        }
    }

    fn shown(&mut self, token: u32, instance: Instance) -> Result<(), Error> {
        match token {
            DASH_SCREEN => self.dash.shown(token, instance),
            PICK_SCREEN => self.pick.shown(token, instance),
            CONNECT_SCREEN => self.connect.shown(token, instance),
            _ => Ok(()),
        }
    }

    fn closed(&mut self, host: &Host<'_>, token: u32, instance: Instance) -> Result<(), Error> {
        match token {
            DASH_SCREEN => self.dash.close(instance),
            PICK_SCREEN => {
                self.pick.close(instance);
                if self.pick.top_instance(PICK_SCREEN).is_none() {
                    fetch::drop_saved(host);
                }
            }
            CONNECT_SCREEN => self.connect.close(instance),
            _ => {}
        }
        if matches!(token, PICK_SCREEN | CONNECT_SCREEN) {
            self.asking = None;
        }
        Ok(())
    }

    /// The dashboard's note: how the last fetch went, in plain words.
    fn note(&mut self, host: &Host<'_>, say: &mut impl Say) -> Result<String, Error> {
        let now = Self::now_ms(host)?;
        if self.fetcher.fetching(now) {
            return say.copy("smart-home.status.updating");
        }
        match self.last {
            Last::Failed(problem) => return problem_text(say, problem),
            Last::Never => {
                if let Err(problem) = self.access(host)? {
                    return problem_text(say, problem);
                }
                return Ok(String::new());
            }
            Last::Done => {}
        }
        Ok(match self.age_secs(host) {
            None => say.copy("smart-home.status.updated")?,
            Some(secs) if secs < 60 => say.copy("smart-home.status.now")?,
            Some(secs) if secs < 7200 => {
                say.format("smart-home.status.minutes", &[Arg::Count(secs / 60)])?
            }
            Some(secs) => say.format(
                "smart-home.status.hours",
                &[Arg::Count((secs / 3600).min(99))],
            )?,
        })
    }

    /// Says the dashboard's note anew and draws it where it changed.
    fn status(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.dash.top_instance(DASH_SCREEN).is_none() {
            return Ok(());
        }
        let mut say = HostSay::new(host)?;
        let note = self.note(host, &mut say)?;
        if let Some(view) = self.dash.top(DASH_SCREEN)
            && view.say(note)
        {
            redraw(host);
        }
        Ok(())
    }

    fn refresh_connect(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.connect.top_instance(CONNECT_SCREEN).is_none() {
            return Ok(());
        }
        let server = Self::text_setting(host, SERVER)?;
        let kept = self.token_kept(host)?;
        let mut say = HostSay::new(host)?;
        if let Some(view) = self.connect.top(CONNECT_SCREEN) {
            view.rebuild(&mut say, &server, kept)?;
        }
        redraw(host);
        Ok(())
    }

    // Fetching.

    /// Fetches the states. `Quiet` (a wake, the schedule) only where Wi-Fi is
    /// up already; the others where the reader is, and wait for the network.
    fn refresh(&mut self, host: &Host<'_>, why: Why) -> Result<(), Error> {
        self.load(host)?;
        let now = Self::now_ms(host)?;
        if self.fetcher.fetching(now) {
            return Ok(());
        }
        let (base, token) = match self.access(host)? {
            Ok(access) => access,
            Err(problem) => return self.failed(host, problem, why),
        };
        if why == Why::Quiet && !self.fetcher.online(host) {
            return Ok(());
        }
        match self.fetcher.send_states(host, &base, &token, why, now) {
            Ok(()) => self.status(host),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(Error::Unsupported) => self.failed(host, Problem::NoNetwork, why),
            Err(Error::Denied) => self.failed(host, Problem::Off, why),
            Err(_) => self.failed(host, Problem::Unreachable, why),
        }
    }

    /// A fetch did not come to an answer: kept for the note, told where the
    /// reader asked for it.
    fn failed(&mut self, host: &Host<'_>, problem: Problem, why: Why) -> Result<(), Error> {
        self.last = Last::Failed(problem);
        let mut say = HostSay::new(host)?;
        // Where the connection screen is open its note says it; a notice would say it twice.
        if why == Why::Asked && self.connect.top_instance(CONNECT_SCREEN).is_none() {
            let words = problem_text(&mut say, problem)?;
            notice(host, &words);
        }
        if why == Why::Picker
            && let Some(view) = self.pick.top(PICK_SCREEN)
        {
            view.loading(Load::Failed(problem));
            redraw(host);
        }
        if self.connect.top_instance(CONNECT_SCREEN).is_some() {
            let words = problem_text(&mut say, problem)?;
            if let Some(view) = self.connect.top(CONNECT_SCREEN) {
                view.say(&words)?;
            }
            redraw(host);
        }
        self.status(host)
    }

    fn http_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        match self.fetcher.ended(done) {
            Some(Ended::States(why)) => self.states_done(host, why, done),
            Some(Ended::Call(id)) => self.call_done(host, &id, done),
            Some(Ended::One(id)) => self.one_done(host, &id, done),
            None => Ok(()),
        }
    }

    /// The list of states arrived in a file: the chosen entities take their
    /// states from it, the rest is passed over.
    fn states_done(&mut self, host: &Host<'_>, why: Why, done: &HttpDone) -> Result<(), Error> {
        self.load(host)?;
        if let Some(problem) = fetch::problem_of(done) {
            fetch::drop_saved(host);
            return self.failed(host, problem, why);
        }
        if !done.saved() {
            return self.failed(host, Problem::Card, why);
        }
        let (mut seen, mut count) = (0u32, 0usize);
        let store = &mut self.store;
        let read = fetch::read_saved(host, &mut |record: Record| {
            count += 1;
            if let Some(at) = store.entities.iter().position(|e| e.id == record.id) {
                seen |= 1 << at;
                if let Some(entity) = store.entities.get_mut(at) {
                    apply(entity, record)?;
                }
            }
            Ok(())
        });
        match read {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                fetch::drop_saved(host);
                return self.failed(host, Problem::BadAnswer, why);
            }
        }
        if count == 0 {
            fetch::drop_saved(host);
            return self.failed(host, Problem::BadAnswer, why);
        }
        for (at, entity) in self.store.entities.iter_mut().enumerate() {
            if seen & (1 << at) == 0 {
                entity.state = text::copy("unavailable")?;
            }
        }
        self.store.synced = capture_snapshot(host)?.unix_seconds().unwrap_or(0);
        self.store.read = true;
        self.last = Last::Done;
        self.persist(host);
        if self.pick.top_instance(PICK_SCREEN).is_some() {
            let query = self.pick.top(PICK_SCREEN).map(|v| text::copy(v.query()));
            if let Some(query) = query {
                self.search(host, &query?)?;
            }
        } else {
            fetch::drop_saved(host);
        }
        if why == Why::Asked && self.connect.top_instance(CONNECT_SCREEN).is_some() {
            let mut say = HostSay::new(host)?;
            let words = say.format(
                "smart-home.connect.ok",
                &[Arg::Count(i64::try_from(count).unwrap_or(i64::MAX))],
            )?;
            notice(host, &words);
            if let Some(view) = self.connect.top(CONNECT_SCREEN) {
                view.say(&words)?;
            }
            redraw(host);
        }
        self.status(host)?;
        self.changed(host, false)?;
        self.reschedule(host)
    }

    // Switching.

    /// Calls the service that switches the entity `id` (a scene runs once).
    fn toggle(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        self.load(host)?;
        let Some(entity) = self.store.find(id) else {
            return Ok(());
        };
        if entity.pending || self.fetcher.calling(id) || entity.kind() == Kind::Value {
            return Ok(());
        }
        let (base, token) = match self.access(host)? {
            Ok(access) => access,
            Err(problem) => {
                let words = problem_text(&mut HostSay::new(host)?, problem)?;
                notice(host, &words);
                return Ok(());
            }
        };
        match self.fetcher.send_call(host, &base, &token, id) {
            Ok(()) => {
                if let Some(entity) = self.store.find_mut(id) {
                    entity.pending = true;
                }
                self.changed(host, false)
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(error) => {
                let problem = match error {
                    Error::Unsupported => Problem::NoNetwork,
                    Error::Denied => Problem::Off,
                    _ => Problem::Unreachable,
                };
                let words = problem_text(&mut HostSay::new(host)?, problem)?;
                notice(host, &words);
                Ok(())
            }
        }
    }

    /// The service call ended: a scene is told, a switch is confirmed by the
    /// state the server answered with (or asked for after).
    fn call_done(&mut self, host: &Host<'_>, id: &str, done: &HttpDone) -> Result<(), Error> {
        if let Some(problem) = fetch::problem_of(done) {
            return self.switch_failed(host, id, problem);
        }
        if self.store.find(id).map(Entity::kind) != Some(Kind::Switch) {
            if let Some(entity) = self.store.find_mut(id) {
                entity.pending = false;
            }
            self.said(host, id, "smart-home.notice.scene")?;
            return self.changed(host, false);
        }
        let record = self.read_state(host, id, done)?;
        match record {
            Some(record) => self.confirmed(host, id, record),
            None => {
                // The answer did not list it (it did not change, or the list was
                // cut): ask for its state.
                let (base, token) = match self.access(host)? {
                    Ok(access) => access,
                    Err(problem) => return self.switch_failed(host, id, problem),
                };
                match self.fetcher.send_one(host, &base, &token, id) {
                    Ok(()) => Ok(()),
                    Err(Error::NoMemory) => Err(Error::NoMemory),
                    Err(_) => self.switch_failed(host, id, Problem::Unreachable),
                }
            }
        }
    }

    fn one_done(&mut self, host: &Host<'_>, id: &str, done: &HttpDone) -> Result<(), Error> {
        if let Some(problem) = fetch::problem_of(done) {
            return self.switch_failed(host, id, problem);
        }
        match self.read_state(host, id, done)? {
            Some(record) => self.confirmed(host, id, record),
            None => self.switch_failed(host, id, Problem::BadAnswer),
        }
    }

    /// The record of `id` in the body of a finished request, if there is one.
    fn read_state(
        &mut self,
        host: &Host<'_>,
        id: &str,
        done: &HttpDone,
    ) -> Result<Option<Record>, Error> {
        let mut states = States::new();
        let mut found = None;
        let read = self.fetcher.read_body(host, done, &mut |piece| {
            states.feed(piece, &mut |record| {
                if record.id == id {
                    found = Some(record);
                }
                Ok(())
            })
        });
        match read {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // A body that cannot be read is no state.
            Ok(()) | Err(_) => Ok(found),
        }
    }

    /// The switch is as the server says: shown, kept, told.
    fn confirmed(&mut self, host: &Host<'_>, id: &str, record: Record) -> Result<(), Error> {
        if let Some(entity) = self.store.find_mut(id) {
            entity.pending = false;
            apply(entity, record)?;
        }
        self.persist(host);
        self.said(host, id, "smart-home.notice.state")?;
        self.changed(host, false)
    }

    fn switch_failed(&mut self, host: &Host<'_>, id: &str, problem: Problem) -> Result<(), Error> {
        if let Some(entity) = self.store.find_mut(id) {
            entity.pending = false;
        }
        let mut say = HostSay::new(host)?;
        let name = self
            .store
            .find(id)
            .map(|e| text::copy(e.title()))
            .transpose()?;
        let reason = problem_text(&mut say, problem)?;
        let words = say.format(
            "smart-home.notice.failed",
            &[
                Arg::Text {
                    name: "name",
                    value: name.as_deref().unwrap_or(id),
                },
                Arg::Text {
                    name: "reason",
                    value: &reason,
                },
            ],
        )?;
        notice(host, &words);
        self.changed(host, false)
    }

    /// The notice `key` (`{name}` and `{state}` filled in) for the entity `id`.
    fn said(&mut self, host: &Host<'_>, id: &str, key: &str) -> Result<(), Error> {
        let Some(entity) = self.store.find(id) else {
            return Ok(());
        };
        let mut say = HostSay::new(host)?;
        let state = value_text(&mut say, entity)?;
        let words = say.format(
            key,
            &[
                Arg::Text {
                    name: "name",
                    value: entity.title(),
                },
                Arg::Text {
                    name: "state",
                    value: &state,
                },
            ],
        )?;
        notice(host, &words);
        Ok(())
    }

    // What is drawn again.

    /// Tells the widgets whose picture changed, and brings the dashboard's and
    /// the picker's rows up to date: a changed state redraws its row only.
    /// `whole`: a setting changed (a look the digest does not show).
    fn changed(&mut self, host: &Host<'_>, whole: bool) -> Result<(), Error> {
        let digest = self.digest();
        if whole || self.told != Some(digest) {
            self.told = Some(digest);
            let mut scratch = [0; 64];
            let mut commands = CommandSender::new(host, &mut scratch);
            for item in &self.items {
                commands.widget_changed(item.instance).ok();
            }
        }
        if self.dash.top_instance(DASH_SCREEN).is_some() {
            let mut say = HostSay::new(host)?;
            let update = match self.dash.top(DASH_SCREEN) {
                Some(view) => view.update(&self.store, &mut say)?,
                None => Update::Same,
            };
            match update {
                Update::Same => {}
                Update::Values(area) => Dash::redraw(host, area)?,
                Update::Rebuilt => redraw(host),
            }
            self.status(host)?;
        }
        if self.pick.top_instance(PICK_SCREEN).is_some() {
            let ids: Vec<String> = self
                .store
                .entities
                .iter()
                .filter_map(|e| text::copy(&e.id).ok())
                .collect();
            if let Some(view) = self.pick.top(PICK_SCREEN) {
                view.tick(ids.into_iter())?;
            }
            redraw(host);
        }
        Ok(())
    }

    // The dashboard.

    fn dash_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.dash.top(DASH_SCREEN) else {
            return Ok(Handled::No);
        };
        let act = view.input(host, input)?;
        let handled = Dash::handled(&act);
        match act {
            dash::Act::Chosen(id) => {
                if self.store.find(&id).map(Entity::kind) == Some(Kind::Value) {
                    self.refresh(host, Why::Asked)?;
                } else {
                    self.toggle(host, &id)?;
                }
            }
            dash::Act::Refresh => self.refresh(host, Why::Asked)?,
            dash::Act::Choose => open(host, PICK)?,
            dash::Act::Connect => open(host, "smart-home.connect")?,
            dash::Act::Back => back(host),
            dash::Act::Ignored | dash::Act::Taken => {}
        }
        Ok(handled)
    }

    // The picker.

    /// The picker opened: the whole list is fetched and kept in its file.
    fn pick_load(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if let Some(view) = self.pick.top(PICK_SCREEN) {
            view.loading(Load::Loading);
        }
        self.refresh(host, Why::Picker)
    }

    /// Searches the saved list for `query` and shows the matches.
    fn search(&mut self, host: &Host<'_>, query: &str) -> Result<(), Error> {
        let mut finder = Finder::new(query)?;
        let read = fetch::read_saved(host, &mut |record| finder.add(record));
        let Some(view) = self.pick.top(PICK_SCREEN) else {
            return Ok(());
        };
        match read {
            Ok(()) => view.found(query, finder)?,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => view.loading(Load::Failed(Problem::BadAnswer)),
        }
        redraw(host);
        Ok(())
    }

    fn pick_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.pick.top(PICK_SCREEN) else {
            return Ok(Handled::No);
        };
        let act = view.input(host, input)?;
        let handled = Pick::handled(&act);
        match act {
            pick::Act::Ask => {
                let title = HostSay::new(host)?.copy("smart-home.pick.search")?;
                // Empty: the last search is in the first row's note.
                self.ask(host, Ask::Search, &title, "", TextMode::Plain)?;
            }
            pick::Act::Chosen(id) => self.chosen(host, &id)?,
            pick::Act::Back => back(host),
            pick::Act::Ignored | pick::Act::Taken => {}
        }
        Ok(handled)
    }

    /// An entity of the list was chosen: taken into the dashboard (and its room
    /// asked for), or taken away where it was there.
    fn chosen(&mut self, host: &Host<'_>, id: &str) -> Result<(), Error> {
        let mut say = HostSay::new(host)?;
        if self.store.remove(id) {
            self.persist(host);
            return self.changed(host, false);
        }
        // Its state, name and unit from the saved list.
        let mut entity = Entity {
            id: model::clean(id, model::ID_MAX)?,
            ..Entity::default()
        };
        let mut found = None;
        fetch::read_saved(host, &mut |record| {
            if record.id == id {
                found = Some(record);
            }
            Ok(())
        })
        .ok();
        if let Some(record) = found {
            apply(&mut entity, record)?;
        }
        match self.store.add(entity) {
            Ok(()) => {}
            Err(Error::NoSpace) => {
                let words = say.format(
                    "smart-home.pick.full",
                    &[Arg::Count(i64::try_from(model::MAX_ENTITIES).unwrap_or(0))],
                )?;
                notice(host, &words);
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        self.persist(host);
        self.changed(host, false)?;
        let title = say.copy("smart-home.pick.room")?;
        self.ask(
            host,
            Ask::Room(text::copy(id)?),
            &title,
            "",
            TextMode::Plain,
        )
    }

    // The connection.

    fn connect_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.connect.top(CONNECT_SCREEN) else {
            return Ok(Handled::No);
        };
        let act = view.input(host, input)?;
        let handled = Connect::handled(&act);
        match act {
            connect::Act::Ask(ConnectRow::Server) => {
                let title = HostSay::new(host)?.copy("smart-home.setting.server")?;
                let initial = Self::text_setting(host, SERVER)?;
                self.ask(host, Ask::Server, &title, &initial, TextMode::Url)?;
            }
            connect::Act::Ask(_) => {
                let title = HostSay::new(host)?.copy("smart-home.connect.token")?;
                self.ask(host, Ask::Token, &title, "", TextMode::Plain)?;
            }
            connect::Act::Test => self.refresh(host, Why::Asked)?,
            connect::Act::Back => back(host),
            connect::Act::Ignored | connect::Act::Taken => {}
        }
        Ok(handled)
    }

    // The keyboard.

    /// Asks for a text on the keyboard, `initial` first.
    fn ask(
        &mut self,
        host: &Host<'_>,
        what: Ask,
        title: &str,
        initial: &str,
        mode: TextMode,
    ) -> Result<(), Error> {
        let mut question = Question::text(title, "")?.mode(mode)?;
        if what == Ask::Token {
            question = question.secret()?;
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui(host)?;
        let asked = ui.ask(host, question, &mut input, &mut out);
        // The question stays open when the filling fails (the field is then
        // empty); it is tracked either way so that its answer is still taken.
        if let (Ok(request), false) = (&asked, initial.is_empty()) {
            ui.text_write(
                host,
                request.text(),
                0,
                initial,
                TextWrite::Last,
                &mut input,
                &mut out,
            )
            .ok();
        }
        wipe(&mut input);
        wipe(&mut out);
        self.asking = Some((asked?, what));
        Ok(())
    }

    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, what)) = self.asking.clone() else {
            return Ok(());
        };
        if request != answer.request() {
            return Ok(());
        }
        self.asking = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let read = self
            .ui(host)?
            .text_read(host, answer.request().text(), 0, &mut input, &mut out);
        let typed = match read {
            Ok(part) if part.data().len() as u64 == u64::from(part.total()) => {
                Secret::copy(part.data())
            }
            Ok(_) => Err(Error::NoSpace),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut out);
        match typed {
            Ok(typed) => self.apply(host, &what, typed.as_str()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // A text too long or that cannot be read is none.
            Err(_) => Ok(()),
        }
    }

    fn apply(&mut self, host: &Host<'_>, what: &Ask, typed: &str) -> Result<(), Error> {
        let text = typed.trim();
        match what {
            Ask::Search => self.search(host, text),
            Ask::Room(id) => {
                let room = model::clean(text, ROOM_MAX)?;
                if let Some(entity) = self.store.find_mut(id) {
                    entity.room = text::copy(&room)?;
                }
                self.persist(host);
                self.changed(host, false)
            }
            Ask::Server => {
                let anywhere = Self::flag(host, ANYWHERE)?;
                if !text.is_empty()
                    && let Err(refused) = link::base(text, anywhere)
                {
                    let words = problem_text(&mut HostSay::new(host)?, Problem::Address(refused))?;
                    notice(host, &words);
                    return Ok(());
                }
                let mut scratch = [0; 512];
                CommandSender::new(host, &mut scratch)
                    .set_setting(SERVER, SettingValue::Text(text))?;
                if let Some(view) = self.connect.top(CONNECT_SCREEN) {
                    view.focus_on(ConnectRow::Token);
                }
                Ok(())
            }
            Ask::Token => {
                // An empty text keeps what is kept (the field never shows it):
                // a confirm by mistake loses nothing; one space takes it away.
                if typed.is_empty() {
                    return Ok(());
                }
                self.keep_token(host, text)?;
                if let Some(view) = self.connect.top(CONNECT_SCREEN) {
                    view.focus_on(ConnectRow::Test);
                }
                self.refresh_connect(host)?;
                self.status(host)
            }
        }
    }

    /// Keeps the token (an empty text takes it away); said where it cannot be.
    fn keep_token(&mut self, host: &Host<'_>, token: &str) -> Result<(), Error> {
        let mut say = HostSay::new(host)?;
        let key = if token.len() > MAX_TOKEN {
            Some("smart-home.connect.not-kept")
        } else if let Some(secrets) = self.secrets(host) {
            let label = say.copy("smart-home.secret.token")?;
            match keep::keep(secrets, host, TOKEN, &label, token) {
                Ok(()) => None,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(Error::Unsupported) => Some("core.secrets-unsupported"),
                Err(_) => Some("smart-home.connect.not-kept"),
            }
        } else {
            Some("core.secrets-unsupported")
        };
        if let Some(key) = key {
            let words = say.copy(key)?;
            notice(host, &words);
        }
        if self.last == Last::Failed(Problem::NoToken) {
            self.last = Last::Never;
        }
        Ok(())
    }

    // The widgets' stage: their height and where they sit.

    fn stage(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, .. } => {
                self.load(host)?;
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let (token, widget) = (item.token, text::copy(&item.widget)?);
                let mut cx = Cx::open(host, &widget)?;
                let height = native_widgets::height(&mut cx, token, &self.store)?;
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_targets(&[WidgetTarget::new(TARGET, rect)], &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }
}

/// What a record of the server tells of an entity: its state, unit and class
/// (the name only where none was kept).
fn apply(entity: &mut Entity, record: Record) -> Result<(), Error> {
    entity.state = record.state;
    entity.unit = record.unit;
    entity.class = record.class;
    if entity.name.is_empty() {
        entity.name = record.name;
    }
    Ok(())
}

/// A problem in plain words.
fn problem_text(say: &mut impl Say, problem: Problem) -> Result<String, Error> {
    if let Problem::Status(code) = problem {
        let mut digits = [0u8; 20];
        let code = text::decimal(code as usize, &mut digits);
        return say.format(
            problem.key(),
            &[Arg::Text {
                name: "status",
                value: code,
            }],
        );
    }
    say.copy(problem.key())
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

/// Opens the screen `id`.
fn open(host: &Host<'_>, id: &str) -> Result<(), Error> {
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open_screen(id)
}

/// The back arrow.
fn back(host: &Host<'_>) {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).back().ok();
}

/// Asks for the screen to be drawn again. A refusal (it is not on top any more)
/// is not an error of what changed it.
fn redraw(host: &Host<'_>) {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(RedrawMode::Partial)
        .ok();
}

/// The notice `words`, shown to the reader. A courtesy: one that cannot be
/// shown changes nothing else.
fn notice(host: &Host<'_>, words: &str) {
    let shown = (|| {
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(Notify::new(words)?)
    })();
    let _ = shown;
}
