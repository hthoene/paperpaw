//! The interface-7 File sync. Folders of the card are kept in sync with
//! folders of a WebDAV server (`native_sync`), one pair after the other, on
//! request, with the charger or where Wi-Fi is up anyway, never at a wake. The
//! host streams every file (`save_to`, `send_file`); nothing of a file is in
//! this plugin's memory.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::CommandSender,
        events::{Event, Incoming, WifiLinkState, kinds},
        files::Files,
        formatting::Arg,
        net::{HttpDone, Net, Reason},
        registrations,
        reply::Reply,
        secrets::Secrets,
        settings::{self, SettingValue},
        snapshot::capture_snapshot,
        ui::{Instance, Open, Ui, WidgetInputKind, WidgetTarget, stages::WidgetCall},
    },
};
use paperpaw_ui_kit::{
    Messages, Screens,
    dav::{
        listing::etag_hash,
        secret::{self as keep, Secret},
    },
    geometry,
    mem::buffer,
    paint, read_event,
    text::copy,
    widget::{Cx, cx::zone_offset},
};

use crate::declarations::{
    BUTTON_TOKEN, EVENTS_TOKEN, HOME, HTTP_TOKEN, LAST_TOKEN, OPEN_TOKEN, PAIR, PENDING_TOKEN,
    STOP_TOKEN, SYNC_TOKEN, TICK_TOKEN,
};
use crate::native_home::{Row, View};
use crate::native_names;
use crate::native_pair::{self as pairs, Pair};
use crate::native_run::{Data, HostLink, stamp};
use crate::native_state::Last;
use crate::native_sync::{Done, End, Finish, Job, Step, Sync, Totals};
use crate::native_when::{Policy, Trigger, due};
use crate::native_widgets::{self, Shown};

mod image {
    include!(concat!(env!("OUT_DIR"), "/file_sync_declarations.rs"));
}

const WHEN: &str = "file-sync.when";
const DEVICE_NAME: &str = "core.device-name";
/// The one target of a widget: the whole of it.
const TARGET: u32 = 1;
/// The most bytes of a stage reply (sizes and up to 32 targets).
const WIDGET_REPLY: usize = 1280;
/// How many minutes pass between two looks at the card for the widget
/// *Pending changes*.
const LOOK_MINUTES: u32 = 15;

/// A widget instance the host made.
#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
}

/// The interface-7 File sync.
#[derive(Debug, Default)]
pub struct NativeFileSync {
    pub(crate) ui: Option<Ui>,
    pub(crate) secrets: Option<Secrets>,
    opened: bool,
    net: Option<Net>,
    files: Option<Files>,
    pub(crate) sync: Sync,
    /// The account's header while a pair is synced.
    account: Option<Secret>,
    /// The pairs still to sync after the one under way, the next first.
    queue: Vec<usize>,
    total: Totals,
    problem: &'static str,
    /// Every pair of the run so far was synced ([`Finish::synced`]).
    all_synced: bool,
    /// Whether the sync under way was asked for (it says how it ended).
    asked: bool,
    /// What the last sync said, kept for the home screen.
    pub(crate) said: String,
    pub(crate) last: Last,
    pub(crate) home: Screens<View>,
    pub(crate) pair_view: Screens<View>,
    /// The pair its screen edits.
    pub(crate) editing: usize,
    pub(crate) typing: Option<(paperpaw_sdk::native::ui::RequestId, Row)>,
    items: Vec<Item>,
    /// Power is plugged in (charging or full on the charger).
    powered: bool,
    /// Minutes since the card was looked at; `None`: not yet.
    looked: Option<u32>,
}

impl Plugin for NativeFileSync {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::WIDGETS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self {
            editing: 1,
            ..Self::default()
        })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Answered, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::HttpDone, kinds::Stopping, kinds::Started, kinds::SettingChanged, kinds::ChargeChanged, kinds::WifiChanged, kinds::Tick, kinds::WidgetMade, kinds::WidgetDropped, kinds::StageCall)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => open_screen(host, "file-sync.home"),
            Event::ActionRun { .. } if token == SYNC_TOKEN => self.sync_now(host),
            Event::HttpDone(done) if token == HTTP_TOKEN => self.request_done(host, &done),
            Event::Stopping { .. } if token == STOP_TOKEN => {
                self.cancel(host);
                Ok(())
            }
            Event::Started { .. } if token == EVENTS_TOKEN => self.started(host),
            Event::SettingChanged { key, .. } if token == EVENTS_TOKEN => {
                self.setting_changed(host, key)
            }
            Event::ChargeChanged { flags, .. } if token == EVENTS_TOKEN => {
                self.charge_changed(host, flags.external_power() || flags.charging())
            }
            Event::WifiChanged { state, .. } if token == EVENTS_TOKEN => {
                if state == WifiLinkState::Connected {
                    self.maybe_sync(host, Trigger::Network)?;
                }
                Ok(())
            }
            Event::Tick { .. } if token == TICK_TOKEN => self.tick(host),
            Event::WidgetMade {
                widget, instance, ..
            } => self.made(host, widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.items.retain(|item| item.instance != instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened { instance, .. } => match token {
                HOME => self.open_home(host, instance),
                PAIR => self.open_pair(host, instance),
                _ => Ok(()),
            },
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == HOME || token == PAIR => {
                if token == HOME {
                    self.home.shown(token, instance)?;
                } else {
                    self.pair_view.shown(token, instance)?;
                }
                self.refresh_home(host)
            }
            Event::ScreenClosed { instance, .. } => {
                self.home.close(instance);
                self.pair_view.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        match (frame.purpose, token) {
            (DrawPurpose::Screen, HOME | PAIR) => self.draw_screen(host, &mut frame, token),
            (DrawPurpose::Widget, _) => self.draw_widget(host, &mut frame),
            _ => Err(Error::Unsupported),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match (token, input) {
            (HOME | PAIR, input) => self.screen_input(host, token, &input),
            (
                _,
                Input::Widget {
                    instance,
                    target: TARGET,
                    kind: WidgetInputKind::Tap,
                    ..
                },
            ) => {
                let widget = self.find(instance.get()).ok_or(Error::NotFound)?.token;
                if widget == BUTTON_TOKEN {
                    self.sync_now(host)?;
                } else {
                    open_screen(host, "file-sync.home")?;
                }
                Ok(Handled::Yes)
            }
            _ => Ok(Handled::No),
        }
    }
}

/// Opens one of the plugin's screens.
fn open_screen(host: &Host<'_>, id: &str) -> Result<(), Error> {
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch).open(Open::screen(id)?)
}

impl NativeFileSync {
    /// Opens the services once. A device without secrets keeps none (said when
    /// one is typed).
    pub(crate) fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if !self.opened {
            self.opened = true;
            self.secrets = match Secrets::open(host) {
                Ok(secrets) => Some(secrets),
                Err(Error::NoMemory) => {
                    self.opened = false;
                    return Err(Error::NoMemory);
                }
                Err(_) => None,
            };
        }
        Ok(())
    }

    fn text_setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
        let mut scratch = [0; 256];
        match settings::read_setting(host, key, &mut scratch)? {
            SettingValue::Text(text) => copy(text),
            _ => Ok(String::new()),
        }
    }

    /// The unix time and the local time in seconds; `None` where the device
    /// does not know the time.
    fn clock(host: &Host<'_>) -> Option<(i64, i64)> {
        let unix = capture_snapshot(host).ok()?.unix_seconds()?;
        let local = unix.saturating_add(i64::from(zone_offset(host, unix).ok()?));
        Some((unix, local))
    }

    // --- events -----------------------------------------------------------

    fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.last = Last::read(&Data(host));
        self.powered = capture_snapshot(host).is_ok_and(|s| s.is_charging());
        Ok(())
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key.starts_with("file-sync.") && key != WHEN {
            // A folder, server or mode changed: the card is looked at anew.
            self.looked = None;
            self.refresh_home(host).ok();
            self.widgets_changed(host);
        }
        Ok(())
    }

    fn charge_changed(&mut self, host: &Host<'_>, powered: bool) -> Result<(), Error> {
        let came = powered && !self.powered;
        self.powered = powered;
        if came {
            self.maybe_sync(host, Trigger::Charger)?;
        }
        Ok(())
    }

    /// Each minute: the card is looked at for the pending changes (the first
    /// time, then now and then), where nothing is syncing.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.sync.running() || self.items.is_empty() && self.home.top(HOME).is_none() {
            return Ok(());
        }
        let minutes = self.looked.map_or(LOOK_MINUTES, |m| m.saturating_add(1));
        if minutes >= LOOK_MINUTES {
            self.look(host)?;
        } else {
            self.looked = Some(minutes);
        }
        Ok(())
    }

    /// Counts the changes of the card since the last sync.
    fn look(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.looked = Some(0);
        let count = self.pending(host)?;
        if count != self.last.pending {
            self.last.pending = count;
            self.last.write(&Data(host)).ok();
            self.widgets_changed(host);
            self.refresh_home(host).ok();
        }
        Ok(())
    }

    fn pending(&mut self, host: &Host<'_>) -> Result<u32, Error> {
        let mut count = 0u32;
        let mut link = HostLink {
            host,
            net: &mut self.net,
            files: &mut self.files,
            data: Data(host),
            auth: None,
        };
        for pair in pairs::ready(host)? {
            if pair.mode == crate::native_plan::Mode::TwoWay {
                let of = crate::native_state::Of::new(pair.n, &pair.folder, &pair.url)?;
                let found = crate::native_look::pending(&mut link, of, &pair.folder)?;
                count = count.saturating_add(found);
            }
        }
        Ok(count)
    }

    // --- the widgets ------------------------------------------------------

    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    fn made(&mut self, host: &Host<'_>, widget: &str, instance: Instance) -> Result<(), Error> {
        let token = match paperpaw_ui_kit::widget::settings::base(widget) {
            "file-sync.last" => LAST_TOKEN,
            "file-sync.pending" => PENDING_TOKEN,
            "file-sync.button" => BUTTON_TOKEN,
            _ => return Ok(()),
        };
        if self.last == Last::default() {
            self.last = Last::read(&Data(host));
        }
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: copy(widget)?,
        });
        Ok(())
    }

    fn widgets_changed(&self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &self.items {
            commands.widget_changed(item.instance).ok();
        }
    }

    /// What the widgets show now.
    fn shown(&self, host: &Host<'_>) -> Result<Shown, Error> {
        let when = if self.last.time == 0 {
            String::new()
        } else {
            let local = self
                .last
                .time
                .saturating_add(zone_offset(host, self.last.time).map_or(0, i64::from));
            stamp(local)?
        };
        Ok(Shown {
            when,
            line: String::new(),
            pending: self.last.pending,
            running: self.sync.running(),
        })
    }

    /// The host asks for the widget's height, where it sits and what a tap
    /// meets.
    fn stage(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = buffer(WIDGET_REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, .. } => {
                let widget = copy(&self.find(instance.get()).ok_or(Error::NotFound)?.widget)?;
                let cx = Cx::open(host, &widget)?;
                let height = u32::from(native_widgets::height(&cx)?);
                call.reply_size(height, height, &mut out)?
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

    fn draw_widget(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let token = item.token;
        let mut cx = Cx::open(host, &item.widget)?;
        let area = Rect::new(
            geometry::clamp_u16(i64::from(frame.rect.x)),
            geometry::clamp_u16(i64::from(frame.rect.y)),
            geometry::clamp_u16(i64::from(frame.rect.width)),
            geometry::clamp_u16(i64::from(frame.rect.height)),
        );
        let mut out = Vec::new();
        native_widgets::draw(&mut cx, &mut out, area, token, &self.shown(host)?)?;
        paint::replay(frame, &cx.font, &out)
    }

    // --- the sync ---------------------------------------------------------

    /// The sync policy says to sync now.
    fn maybe_sync(&mut self, host: &Host<'_>, trigger: Trigger) -> Result<(), Error> {
        if self.sync.running() || !self.queue.is_empty() {
            return Ok(());
        }
        let policy = Policy::of(&Self::text_setting(host, WHEN)?);
        let now = Self::clock(host).map(|(unix, _)| unix);
        if due(policy, trigger, self.known_last(host).time, now) && !pairs::ready(host)?.is_empty()
        {
            self.begin(host, false)?;
        }
        Ok(())
    }

    /// What the last sync said, read where it is not known yet.
    fn known_last(&mut self, host: &Host<'_>) -> Last {
        if self.last.time == 0 {
            self.last = Last::read(&Data(host));
        }
        self.last
    }

    /// Syncs on request.
    pub(crate) fn sync_now(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.sync.running() {
            return Ok(());
        }
        self.ensure(host)?;
        if pairs::ready(host)?.is_empty() {
            let text = Messages::new()?.copy(host, "file-sync.no-pair")?;
            self.said = copy(&text)?;
            self.notice(host, &text);
            return self.refresh_home(host);
        }
        self.begin(host, true)
    }

    fn begin(&mut self, host: &Host<'_>, asked: bool) -> Result<(), Error> {
        self.ensure(host)?;
        self.asked = asked;
        self.total = Totals::default();
        self.problem = "";
        self.all_synced = true;
        self.queue.clear();
        for pair in pairs::ready(host)? {
            self.queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
            self.queue.push(pair.n);
        }
        self.queue.reverse();
        self.next_pair(host)
    }

    /// Starts the next pair of the queue, or ends the run.
    fn next_pair(&mut self, host: &Host<'_>) -> Result<(), Error> {
        match self.start_next(host) {
            Ok(Some(Step::Quiet)) => self.refresh_home(host),
            Ok(Some(Step::Ended(finish))) => self.pair_finished(host, &finish),
            Ok(None) => self.finish_all(host),
            Err(_) => self.broke_off(host),
        }
    }

    /// Starts the next pair of the queue: how it began; `None` where no pair
    /// is left.
    fn start_next(&mut self, host: &Host<'_>) -> Result<Option<Step>, Error> {
        while let Some(n) = self.queue.pop() {
            let pair = pairs::read(host, n)?;
            if !pair.ready() {
                continue;
            }
            let job = self.job(host, &pair)?;
            let mut link = HostLink {
                host,
                net: &mut self.net,
                files: &mut self.files,
                data: Data(host),
                auth: self.account.as_ref(),
            };
            return self.sync.start(&mut link, job).map(Some);
        }
        Ok(None)
    }

    /// The job of `pair`; the account's header is made of the user and the
    /// password among the secrets.
    fn job(&mut self, host: &Host<'_>, pair: &Pair) -> Result<Job, Error> {
        let password = match &self.secrets {
            Some(secrets) => keep::read(secrets, host, &pairs::secret_name(pair.n)?)?,
            None => None,
        };
        self.account = if pair.user.is_empty() && password.is_none() {
            None
        } else {
            Some(Secret::basic(
                &pair.user,
                password.as_ref().map_or("", Secret::as_str),
            )?)
        };
        let device = native_names::device(&Self::text_setting(host, DEVICE_NAME)?)?;
        let date = match Self::clock(host) {
            Some((_, local)) => native_names::date(local.div_euclid(86_400))?,
            None => copy("0000-00-00")?,
        };
        Ok(Job {
            pair: pair.n,
            root: copy(&pair.folder)?,
            url: copy(&pair.url)?,
            mode: pair.mode,
            device,
            date,
        })
    }

    /// The end of a request of the sync.
    fn request_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let end = match done.reason() {
            Reason::Ok => End::Answered,
            Reason::Cancelled | Reason::Replaced => End::Cancelled,
            Reason::Full => End::Full,
            Reason::NoNetwork
            | Reason::Dns
            | Reason::Tls
            | Reason::Timeout
            | Reason::Refused
            | Reason::Offline
            | Reason::Saving
            | Reason::NetworkOff => End::Network,
            _ => End::Other,
        };
        let version = self.etag(host, done);
        let done = Done {
            end,
            status: done.status(),
            saved: done.saved(),
            version,
        };
        let mut link = HostLink {
            host,
            net: &mut self.net,
            files: &mut self.files,
            data: Data(host),
            auth: self.account.as_ref(),
        };
        match self.sync.done(&mut link, &done) {
            Ok(Step::Quiet) => self.refresh_progress(host),
            Ok(Step::Ended(finish)) => self.pair_finished(host, &finish),
            Err(_) => self.broke_off(host),
        }
    }

    /// The run cannot go on (memory, the card or the secrets failed): the
    /// pairs left are dropped, the account's header is wiped and the end is
    /// said, so the next sync, asked for or automatic, starts anew.
    fn broke_off(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.queue.clear();
        self.account = None;
        self.all_synced = false;
        // It says why the run ended, before what went wrong in a folder.
        self.problem = "file-sync.broke-off";
        self.finish_all(host)
    }

    /// A number made of the `ETag` of an answer; 0 for none or where it
    /// cannot be read.
    fn etag(&self, host: &Host<'_>, done: &HttpDone) -> u64 {
        let Some(net) = &self.net else {
            return 0;
        };
        if !done.succeeded() {
            return 0;
        }
        let (Ok(mut input), Ok(mut out)) = (buffer(4096), buffer(4096)) else {
            return 0;
        };
        net.answer(host, done.id(), &mut input, &mut out)
            .map_or(0, |answer| etag_hash(answer.etag()))
    }

    fn pair_finished(&mut self, host: &Host<'_>, finish: &Finish) -> Result<(), Error> {
        self.account = None;
        self.total = self.total.plus(finish.totals);
        self.all_synced &= finish.synced;
        if self.problem.is_empty() {
            self.problem = finish.problem;
        }
        if finish.stopped {
            self.queue.clear();
        }
        self.next_pair(host)
    }

    /// The plugin stops: a sync under way ends.
    fn cancel(&mut self, host: &Host<'_>) {
        let mut link = HostLink {
            host,
            net: &mut self.net,
            files: &mut self.files,
            data: Data(host),
            auth: self.account.as_ref(),
        };
        self.sync.cancel(&mut link);
        self.account = None;
        self.queue.clear();
    }

    /// Says how the run ended and keeps what the last sync did.
    fn finish_all(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.account = None;
        let total = self.total;
        let pending = self.pending(host).unwrap_or(0);
        let synced = Self::clock(host)
            .map(|(unix, _)| unix)
            .filter(|_| self.all_synced);
        self.last = self.known_last(host).ended(total, pending, synced);
        self.last.write(&Data(host)).ok();
        self.looked = Some(0);
        let mut say = Messages::new()?;
        let mut text = say.format(
            host,
            "file-sync.summary",
            &[
                Arg::Int {
                    name: "down",
                    value: i64::from(total.down),
                },
                Arg::Int {
                    name: "up",
                    value: i64::from(total.up),
                },
                Arg::Int {
                    name: "deleted",
                    value: i64::from(total.deleted),
                },
                Arg::Int {
                    name: "conflicts",
                    value: i64::from(total.conflicts),
                },
                Arg::Int {
                    name: "errors",
                    value: i64::from(total.errors),
                },
            ],
        )?;
        for key in [
            self.problem,
            if total.held > 0 { "file-sync.held" } else { "" },
        ] {
            if !key.is_empty() {
                text.push(' ');
                text.push_str(&say.copy(host, key)?);
            }
        }
        if self.asked || !self.problem.is_empty() {
            self.notice(host, &text);
        }
        self.said = text;
        self.widgets_changed(host);
        self.refresh_home(host)
    }

    /// A notice, where it can be shown.
    pub(crate) fn notice(&self, host: &Host<'_>, text: &str) {
        let mut scratch = [0; 512];
        if let Ok(notify) = paperpaw_sdk::native::ui::Notify::new(text) {
            CommandSender::new(host, &mut scratch).notify(notify).ok();
        }
    }
}
