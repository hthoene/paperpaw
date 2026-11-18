//! The interface-7 Picture frame. Photos come from a folder of the card and
//! from the cache of a WebDAV server (`native_sync`); one is shown as the
//! sleep screen (offered to the slot `core.sleep`, chosen in the sleep
//! settings), in the slideshow screen and in the widget *Photo of the day*.
//! The host decodes and draws the photos (`image_prepare`, `draw_image`: in
//! bands, nothing of a photo is in this plugin's memory). Nothing here wakes
//! the radio: the server is asked on request, with the charger, or where
//! Wi-Fi is up anyway.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    Color, Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, WifiLinkState, kinds},
        files::Files,
        image::{ImageId, ImageReady, ImageStyle, PrepareImage, PrepareRoot},
        net::{HttpDone, Net, Reason},
        registrations,
        reply::Reply,
        secrets::Secrets,
        settings::{self, SettingValue},
        snapshot::capture_snapshot,
        storage::Root,
        ui::{Instance, Open, Ui, WidgetInputKind, WidgetTarget, stages::WidgetCall},
    },
};
use paperpaw_ui_kit::{
    Screens, geometry,
    mem::{SERVICE_BUFFER, buffer},
    paint, read_event,
    text::copy,
    widget::{Align, Cx, cx::zone_offset, layout::label, text::Wrapped},
};

use crate::declarations::{
    EVENTS_TOKEN, HOME, HTTP_TOKEN, OPEN_TOKEN, SHOW, SLEEP, SLEEP_SCREEN, SLIDESHOW_TOKEN,
    STOP_TOKEN, SYNC_TOKEN, TICK_TOKEN, WIDGET_GROUP,
};
use crate::native_cache as cache;
use crate::native_home::Home;
use crate::native_keep::Secret;
use crate::native_link::{Data, HostLink, folder_url};
use crate::native_photos::{Change, Fit, Found, Photos, choose, tone};
use crate::native_pics::{Answer, Pics, SLOTS, Slot};
use crate::native_state::Kept;
use crate::native_sync::{Done, End, Finish, Job, Step, Sync};
use crate::native_when::{Policy, Trigger, due};

mod image {
    include!(concat!(env!("OUT_DIR"), "/picture_frame_declarations.rs"));
}

pub(crate) const FOLDER: &str = "picture-frame.folder";
pub(crate) const CHANGE: &str = "picture-frame.change";
pub(crate) const FIT: &str = "picture-frame.fit";
pub(crate) const CONTRAST: &str = "picture-frame.contrast";
pub(crate) const INTERVAL: &str = "picture-frame.interval";
pub(crate) const CACHE_MB: &str = "picture-frame.cache-mb";
pub(crate) const ON_CHARGE: &str = "picture-frame.slideshow-on-charge";
pub(crate) const SYNC_WHEN: &str = "picture-frame.sync";
pub(crate) const SERVER: &str = "picture-frame.server";
pub(crate) const USER: &str = "picture-frame.user";
/// The sleep plugin's setting that chooses what the sleep screen shows.
const SLEEP_SHOW: &str = "sleep.show";
/// The one target of the widget: the whole of it.
const TARGET: u32 = 1;
/// The most bytes of a stage reply (sizes and up to 32 targets).
const WIDGET_REPLY: usize = 1280;
/// The most pages of a card folder read (32 entries each).
const MOST_PAGES: usize = 64;

/// A widget instance the host made.
#[derive(Debug)]
struct Item {
    instance: Instance,
    /// The widget's id as the host names it, with its placement.
    widget: String,
}

/// The interface-7 Picture frame.
#[derive(Debug, Default)]
pub struct NativePictureFrame {
    pub(crate) ui: Option<Ui>,
    pub(crate) secrets: Option<Secrets>,
    opened: bool,
    net: Option<Net>,
    sync: Sync,
    /// The account's header while a sync runs.
    account: Option<Secret>,
    /// What the last sync said, kept for the home screen.
    pub(crate) said: String,
    pub(crate) photos: Photos,
    loaded: bool,
    pub(crate) kept: Kept,
    pics: Pics,
    items: Vec<Item>,
    pub(crate) home: Screens<Home>,
    pub(crate) typing: Option<(paperpaw_sdk::native::ui::RequestId, crate::native_home::Row)>,
    show_open: bool,
    show_at: usize,
    show_minutes: u32,
    sleep_open: bool,
    /// Power is plugged in (charging or full on the charger).
    powered: bool,
    /// The day number last seen, for the minute tick.
    day: i64,
}

impl Plugin for NativePictureFrame {
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
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Answered, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::HttpDone, kinds::Stopping, kinds::Started, kinds::ImageReady, kinds::SettingChanged, kinds::ChargeChanged, kinds::WifiChanged, kinds::Tick, kinds::WidgetMade, kinds::WidgetDropped, kinds::StageCall)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                open_screen(host, "picture-frame.home")
            }
            Event::ActionRun { .. } if token == SLIDESHOW_TOKEN => {
                open_screen(host, "picture-frame.show")
            }
            Event::ActionRun { .. } if token == SYNC_TOKEN => self.sync_now(host),
            Event::HttpDone(done) if token == HTTP_TOKEN => self.request_done(host, &done),
            Event::Stopping { .. } if token == STOP_TOKEN => {
                self.cancel(host);
                Ok(())
            }
            Event::Started { .. } if token == EVENTS_TOKEN => self.started(host),
            Event::ImageReady(ready) => self.image_ready(host, &ready),
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
                if self.items.is_empty()
                    && let Some(old) = self.pics.forget(Slot::Widget)
                {
                    release_image(host, old);
                }
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened { instance, .. } => self.screen_opened(host, token, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == HOME => {
                self.home.shown(token, instance)?;
                self.refresh_home(host)
            }
            Event::ScreenClosed { instance, .. } => self.screen_closed(host, token, instance),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        match (frame.purpose, token) {
            (DrawPurpose::Screen, HOME) => self.draw_home(host, &mut frame),
            (DrawPurpose::Screen, SHOW) => self.draw_photo(host, &mut frame, Slot::Show),
            (DrawPurpose::Screen, SLEEP) => self.draw_photo(host, &mut frame, Slot::Sleep),
            (DrawPurpose::Widget, _) => self.draw_widget(host, &mut frame),
            _ => Err(Error::Unsupported),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match (token, input) {
            (HOME, input) => self.home_input(host, &input),
            (SHOW, input) => self.show_input(host, &input),
            (
                _,
                Input::Widget {
                    instance,
                    target: TARGET,
                    kind: WidgetInputKind::Tap,
                    ..
                },
            ) => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                open_screen(host, "picture-frame.show")?;
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

/// Gives a photo back; a refusal leaves it with the host, which ends it with
/// the plugin.
fn release_image(host: &Host<'_>, image: ImageId) {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .image_release(image)
        .ok();
}

fn redraw(host: &Host<'_>, mode: RedrawMode) {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(mode)
        .ok();
}

impl NativePictureFrame {
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

    pub(crate) fn text_setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
        let mut scratch = [0; 1100];
        match settings::read_setting(host, key, &mut scratch)? {
            SettingValue::Text(text) => copy(text),
            _ => Ok(String::new()),
        }
    }

    fn int_setting(host: &Host<'_>, key: &str, default: i64) -> i64 {
        let mut scratch = [0; 64];
        match settings::read_setting(host, key, &mut scratch) {
            Ok(SettingValue::Int(value)) => value,
            _ => default,
        }
    }

    fn bool_setting(host: &Host<'_>, key: &str) -> bool {
        let mut scratch = [0; 64];
        matches!(
            settings::read_setting(host, key, &mut scratch),
            Ok(SettingValue::Bool(true))
        )
    }

    /// The unix time and the day number by the local time (days since 1970);
    /// `None` where the device does not know the time.
    fn clock(host: &Host<'_>) -> Option<(i64, i64)> {
        let unix = capture_snapshot(host).ok()?.unix_seconds()?;
        let local = unix.saturating_add(i64::from(zone_offset(host, unix).ok()?));
        Some((unix, local.div_euclid(86_400)))
    }

    /// Whether the sleep screen's setting chooses this plugin's screen.
    fn sleep_chosen(host: &Host<'_>) -> bool {
        let mut bytes = [0u8; 64];
        matches!(
            settings::read_setting(host, SLEEP_SHOW, &mut bytes),
            Ok(SettingValue::Text(SLEEP_SCREEN))
        )
    }

    // --- the photos -------------------------------------------------------

    /// Reads the card's folder and the cache's list anew.
    fn reload(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let setting = Self::text_setting(host, FOLDER)?;
        let folder = copy(setting.trim().trim_matches('/'))?;
        let card = match folder_names(host, &folder) {
            Ok(names) => names,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => Vec::new(),
        };
        let mut cached = Vec::new();
        for entry in cache::read(&Data(host)) {
            paperpaw_ui_kit::mem::push(&mut cached, entry.name)?;
        }
        self.photos = Photos {
            folder,
            card,
            cached,
        };
        self.loaded = true;
        self.pics.forgive();
        Ok(())
    }

    fn photos(&mut self, host: &Host<'_>) -> Result<&Photos, Error> {
        if !self.loaded {
            self.reload(host)?;
        }
        Ok(&self.photos)
    }

    /// The photo for `slot`: the sleep screen's by the setting, the widget's
    /// by the day, the slideshow's where it has got to.
    fn index_for(&mut self, host: &Host<'_>, slot: Slot) -> Result<Option<usize>, Error> {
        let count = self.photos(host)?.len();
        let day = Self::clock(host).map_or(0, |(_, day)| day);
        Ok(match slot {
            Slot::Sleep => {
                let change = Change::of(&Self::text_setting(host, CHANGE)?);
                choose(count, change, self.kept.cursor, day)
            }
            Slot::Widget => choose(count, Change::Daily, 0, day),
            Slot::Show => (count > 0).then(|| self.show_at % count),
        })
    }

    /// Asks for the photo `slot` shows, if it is not held or asked for.
    fn want(&mut self, host: &Host<'_>, slot: Slot) {
        if let Ok(Some(index)) = self.index_for(host, slot)
            && self.pics.wants(slot, index)
        {
            self.ask(host, slot, index, false).ok();
        }
    }

    /// Asks the host to decode photo `index` for `slot`.
    fn ask(&mut self, host: &Host<'_>, slot: Slot, index: usize, retry: bool) -> Result<(), Error> {
        let Some((source, path)) = self.photos.at(index) else {
            return Err(Error::NotFound);
        };
        let root = match source {
            crate::native_photos::Source::Card => PrepareRoot::Library,
            crate::native_photos::Source::Cache => PrepareRoot::Data,
        };
        let table = tone(Self::int_setting(host, CONTRAST, 10));
        let cookie = self.pics.asked(slot, index, retry);
        let request = PrepareImage::new(root, &path, cookie).turn().tone(&table);
        let mut scratch = buffer(1024)?;
        CommandSender::new(host, &mut scratch).image_prepare(&request)
    }

    /// The host's answer to a photo asked for.
    fn image_ready(&mut self, host: &Host<'_>, ready: &ImageReady) -> Result<(), Error> {
        let size = (ready.width(), ready.height());
        match self
            .pics
            .ready(ready.cookie(), ready.status(), ready.image(), size)
        {
            Answer::Ignore(held) => {
                if let Some(image) = held {
                    release_image(host, image);
                }
            }
            Answer::Later => {}
            Answer::Ready { slot, release } => {
                if let Some(old) = release {
                    release_image(host, old);
                }
                self.shown(host, slot);
            }
            Answer::Failed { slot, index, again } => {
                let count = self.photos.len();
                if again && count > 1 {
                    self.ask(host, slot, (index + 1) % count, true).ok();
                }
            }
        }
        Ok(())
    }

    /// A photo is ready for `slot`: what shows it is drawn anew, in full (a
    /// photo is final, in grays).
    fn shown(&mut self, host: &Host<'_>, slot: Slot) {
        match slot {
            Slot::Sleep if self.sleep_open => redraw(host, RedrawMode::Full),
            Slot::Show if self.show_open => redraw(host, RedrawMode::Full),
            Slot::Widget => self.widgets_changed(host),
            _ => {}
        }
    }

    fn widgets_changed(&self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &self.items {
            commands.widget_changed(item.instance).ok();
        }
    }

    /// Gives every photo back and asks for those that are shown.
    fn renew(&mut self, host: &Host<'_>) {
        for slot in SLOTS {
            if let Some(old) = self.pics.forget(slot) {
                release_image(host, old);
            }
        }
        self.wanted(host);
    }

    /// Asks for the photos the places that exist show.
    fn wanted(&mut self, host: &Host<'_>) {
        if Self::sleep_chosen(host) || self.sleep_open {
            self.want(host, Slot::Sleep);
        }
        if self.show_open {
            self.want(host, Slot::Show);
        }
        if !self.items.is_empty() {
            self.want(host, Slot::Widget);
        }
    }

    // --- events -----------------------------------------------------------

    fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.kept = Kept::read(&Data(host));
        self.powered = capture_snapshot(host).is_ok_and(|s| s.is_charging());
        self.day = Self::clock(host).map_or(0, |(_, day)| day);
        self.reload(host)?;
        self.wanted(host);
        Ok(())
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        match key {
            FOLDER => {
                self.reload(host)?;
                self.renew(host);
            }
            CHANGE | CONTRAST => self.renew(host),
            FIT => redraw(host, RedrawMode::Full),
            SLEEP_SHOW => {
                if Self::sleep_chosen(host) {
                    self.want(host, Slot::Sleep);
                } else if !self.sleep_open
                    && let Some(old) = self.pics.forget(Slot::Sleep)
                {
                    release_image(host, old);
                }
            }
            SERVER | USER => {
                self.refresh_home(host).ok();
            }
            _ => {}
        }
        Ok(())
    }

    fn charge_changed(&mut self, host: &Host<'_>, powered: bool) -> Result<(), Error> {
        let came = powered && !self.powered;
        self.powered = powered;
        if !came {
            return Ok(());
        }
        self.maybe_sync(host, Trigger::Charger)?;
        if Self::bool_setting(host, ON_CHARGE) && !self.show_open {
            open_screen(host, "picture-frame.show")?;
        }
        Ok(())
    }

    /// Each minute: a new day changes the photo of the day (and of the sleep
    /// screen where it changes daily), and the slideshow goes on while the
    /// power is plugged in.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if let Some((_, day)) = Self::clock(host)
            && day != self.day
        {
            self.day = day;
            if Change::of(&Self::text_setting(host, CHANGE)?) == Change::Daily
                && Self::sleep_chosen(host)
            {
                self.want(host, Slot::Sleep);
            }
            if !self.items.is_empty() {
                self.want(host, Slot::Widget);
            }
        }
        if self.show_open {
            self.show_minutes = self.show_minutes.saturating_add(1);
            let interval = Self::int_setting(host, INTERVAL, 10).clamp(1, 240);
            if self.powered && i64::from(self.show_minutes) >= interval {
                self.step(host, 1);
            }
        }
        Ok(())
    }

    fn screen_opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
    ) -> Result<(), Error> {
        match token {
            HOME => {
                self.ensure(host)?;
                let mut view = Home::default();
                self.build_home(host, &mut view)?;
                self.home.open(instance, view)
            }
            SHOW => {
                self.show_open = true;
                self.show_minutes = 0;
                // The slideshow goes on from the photo after the one of the
                // last sleep.
                if self.show_at == 0 {
                    self.show_at = self.kept.cursor;
                }
                self.want(host, Slot::Show);
                Ok(())
            }
            SLEEP => {
                self.sleep_open = true;
                self.want(host, Slot::Sleep);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn screen_closed(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
    ) -> Result<(), Error> {
        match token {
            HOME => self.home.close(instance),
            SHOW => {
                self.show_open = false;
                if let Some(old) = self.pics.forget(Slot::Show) {
                    release_image(host, old);
                }
            }
            SLEEP => {
                self.sleep_open = false;
                // The next sleep shows the next photo (each sleep) and is
                // asked for now, while the device is awake.
                if Change::of(&Self::text_setting(host, CHANGE)?) == Change::Sleep {
                    self.kept.cursor = self.kept.cursor.wrapping_add(1);
                    self.kept.write(&Data(host)).ok();
                }
                if Self::sleep_chosen(host) {
                    self.want(host, Slot::Sleep);
                } else if let Some(old) = self.pics.forget(Slot::Sleep) {
                    release_image(host, old);
                }
            }
            _ => {}
        }
        Ok(())
    }

    // --- the slideshow ----------------------------------------------------

    /// Goes `by` photos on (or back) in the slideshow.
    fn step(&mut self, host: &Host<'_>, by: isize) {
        let count = self.photos.len();
        if count == 0 {
            return;
        }
        let at = (self.show_at % count) as isize;
        let step = isize::try_from(count).unwrap_or(isize::MAX);
        self.show_at = (at + by).rem_euclid(step).unsigned_abs();
        self.show_minutes = 0;
        self.want(host, Slot::Show);
    }

    fn show_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let by = match *input {
            Input::Action { action, .. } => match action {
                "core.right" | "core.next" | "core.confirm" => 1,
                "core.left" | "core.previous" => -1,
                _ => return Ok(Handled::No),
            },
            Input::Gesture {
                kind: InputGestureKind::SwipeLeft,
                ..
            } => 1,
            Input::Gesture {
                kind: InputGestureKind::SwipeRight,
                ..
            } => -1,
            Input::Gesture {
                kind: InputGestureKind::Tap,
                position,
                ..
            } => {
                let width = capture_snapshot(host).map_or(0, |s| i32::from(s.display_width()));
                if position.x < width / 3 { -1 } else { 1 }
            }
            _ => return Ok(Handled::No),
        };
        self.step(host, by);
        Ok(Handled::Yes)
    }

    // --- drawing ----------------------------------------------------------

    /// The sleep screen and the slideshow: the photo over white, fitted by the
    /// setting and drawn in grays at once; or what there is to say.
    fn draw_photo(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        slot: Slot,
    ) -> Result<(), Error> {
        let area = frame.rect;
        let rect = Rect::new(
            geometry::clamp_u16(i64::from(area.x)),
            geometry::clamp_u16(i64::from(area.y)),
            geometry::clamp_u16(i64::from(area.width)),
            geometry::clamp_u16(i64::from(area.height)),
        );
        frame.draw_canvas()?.fill_rectangle(area, Color::WHITE)?;
        let Some(held) = self.pics.held(slot) else {
            // A photo is on its way where there are photos; else the reason.
            let key = if self.photos.is_empty() {
                "picture-frame.none"
            } else {
                "picture-frame.loading"
            };
            return self.say(host, frame, rect, key);
        };
        let fit = Fit::of(&Self::text_setting(host, FIT)?);
        let style = if fit.fills(held.size, rect) {
            ImageStyle::new().fill().grays()
        } else {
            ImageStyle::new().grays()
        };
        // A photo the host refuses (an id it ended, no memory) leaves the white.
        frame
            .draw_canvas()?
            .draw_image(held.image, area, style)
            .ok();
        Ok(())
    }

    /// A few words in the middle of `rect`.
    fn say(
        &self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        rect: Rect,
        key: &str,
    ) -> Result<(), Error> {
        let mut cx = Cx::open(host, WIDGET_GROUP)?;
        let text = if key == "picture-frame.none" {
            cx.format(
                key,
                &[paperpaw_sdk::native::formatting::Arg::Text {
                    name: "folder",
                    value: &self.photos.folder,
                }],
            )?
        } else {
            cx.tr(key)?
        };
        let style = cx.style();
        let (ink, size, margin) = (style.ink, style.text, style.margin);
        let width = rect.width.saturating_sub(margin.saturating_mul(2));
        let line = geometry::line_px(&cx.font, size)?;
        let wrapped = Wrapped::new(
            &cx.font,
            &text,
            size,
            (width, line.saturating_mul(6)),
            false,
        )?;
        let height = wrapped.height(&cx.font)?;
        let top = rect
            .y
            .saturating_add(rect.height.saturating_sub(height) / 2);
        let mut out = Vec::new();
        wrapped.draw(
            &mut out,
            &cx.font,
            Rect::new(rect.x.saturating_add(margin), top, width, height),
            Align::Center,
            ink,
        )?;
        paint::replay(frame, &cx.font, out.iter())
    }

    // --- the widget -------------------------------------------------------

    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    fn made(&mut self, host: &Host<'_>, widget: &str, instance: Instance) -> Result<(), Error> {
        if paperpaw_ui_kit::widget::settings::base(widget) != WIDGET_GROUP {
            return Ok(());
        }
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            widget: copy(widget)?,
        });
        self.want(host, Slot::Widget);
        Ok(())
    }

    /// The host asks for the widget's height, where it sits and what a tap
    /// meets.
    fn stage(
        &mut self,
        _host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = buffer(WIDGET_REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                // A square, and the frame's edges.
                call.reply_size(width, width, &mut out)?
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
        let mut cx = Cx::open(host, &item.widget)?;
        let area = Rect::new(
            geometry::clamp_u16(i64::from(frame.rect.x)),
            geometry::clamp_u16(i64::from(frame.rect.y)),
            geometry::clamp_u16(i64::from(frame.rect.width)),
            geometry::clamp_u16(i64::from(frame.rect.height)),
        );
        let mut out = Vec::new();
        let look = cx.shape.paint(&mut out, area, 256)?;
        let inner = look.area;
        let held = self.pics.held(Slot::Widget);
        if held.is_none() {
            let key = if self.photos.is_empty() {
                "picture-frame.widget.none"
            } else {
                "picture-frame.loading"
            };
            let words = cx.tr(key)?;
            label(
                &mut out,
                &cx.font,
                (cx.style().small, Align::Center),
                inner,
                &words,
                look.ink,
            )?;
        }
        paint::replay(frame, &cx.font, &out)?;
        if let Some(held) = held {
            let fit = Fit::of(&Self::text_setting(host, FIT)?);
            let style = if fit.fills(held.size, inner) {
                ImageStyle::new().fill().grays()
            } else {
                ImageStyle::new().grays()
            };
            frame
                .draw_canvas()?
                .draw_image(held.image, paint::frame_rect(inner), style)
                .ok();
        }
        Ok(())
    }

    // --- the server -------------------------------------------------------

    /// The sync policy says to fetch now.
    fn maybe_sync(&mut self, host: &Host<'_>, trigger: Trigger) -> Result<(), Error> {
        if self.sync.running() || folder_url(&Self::text_setting(host, SERVER)?).is_none() {
            return Ok(());
        }
        let policy = Policy::of(&Self::text_setting(host, SYNC_WHEN)?);
        let now = Self::clock(host).map(|(unix, _)| unix);
        let synced = Kept::read(&Data(host)).synced;
        if due(policy, trigger, synced, now) {
            self.begin(host, false)?;
        }
        Ok(())
    }

    /// Fetches on request.
    pub(crate) fn sync_now(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.sync.running() {
            return Ok(());
        }
        self.ensure(host)?;
        self.begin(host, true)
    }

    fn begin(&mut self, host: &Host<'_>, asked: bool) -> Result<(), Error> {
        let Some(url) = folder_url(&Self::text_setting(host, SERVER)?) else {
            return self.finished(host, &Finish::failed_key("picture-frame.no-server"), asked);
        };
        self.ensure(host)?;
        let user = Self::text_setting(host, USER)?;
        let password = match &self.secrets {
            Some(secrets) => crate::native_keep::read(secrets, host, crate::native_keep::PASSWORD)?,
            None => None,
        };
        self.account = if user.is_empty() && password.is_none() {
            None
        } else {
            Some(Secret::basic(
                &user,
                password.as_ref().map_or("", Secret::as_str),
            )?)
        };
        self.kept = Kept::read(&Data(host));
        let cap =
            u64::try_from(Self::int_setting(host, CACHE_MB, 64).clamp(8, 1024)).unwrap_or(64) << 20;
        let job = Job {
            url,
            cap,
            window: self.kept.window,
        };
        if asked {
            let say = paperpaw_ui_kit::Messages::new()?.copy(host, "picture-frame.syncing")?;
            self.notice(host, &say);
        }
        let mut link = HostLink {
            host,
            net: &mut self.net,
            data: Data(host),
            auth: self.account.as_ref(),
        };
        match self.sync.start(&mut link, job) {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => {
                self.account = None;
                self.finished(host, &Finish::failed_key("picture-frame.failed"), asked)
            }
        }
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
        let done = Done {
            token: done.token(),
            end,
            status: done.status(),
            saved: done.saved(),
        };
        let mut link = HostLink {
            host,
            net: &mut self.net,
            data: Data(host),
            auth: self.account.as_ref(),
        };
        let step = self.sync.done(&mut link, &done);
        match step? {
            Step::Quiet => Ok(()),
            Step::Ended(finish) => self.finished(host, &finish, true),
        }
    }

    /// The plugin stops: a sync under way ends.
    fn cancel(&mut self, host: &Host<'_>) {
        let mut link = HostLink {
            host,
            net: &mut self.net,
            data: Data(host),
            auth: self.account.as_ref(),
        };
        self.sync.cancel(&mut link);
        self.account = None;
    }

    /// Says how a sync ended, keeps where it went on from and what the cache
    /// holds now.
    fn finished(&mut self, host: &Host<'_>, finish: &Finish, tell: bool) -> Result<(), Error> {
        self.account = None;
        if finish.ok {
            self.kept = Kept::read(&Data(host));
            self.kept.window = finish.window;
            if let Some((unix, _)) = Self::clock(host) {
                self.kept.synced = unix;
            }
            self.kept.write(&Data(host)).ok();
        }
        let mut say = paperpaw_ui_kit::Messages::new()?;
        let text = if finish.key == "picture-frame.synced" {
            say.format(
                host,
                finish.key,
                &[
                    paperpaw_sdk::native::formatting::Arg::Int {
                        name: "fetched",
                        value: finish.fetched as i64,
                    },
                    paperpaw_sdk::native::formatting::Arg::Int {
                        name: "cached",
                        value: finish.cached as i64,
                    },
                    paperpaw_sdk::native::formatting::Arg::Int {
                        name: "photos",
                        value: finish.photos as i64,
                    },
                ],
            )?
        } else {
            say.copy(host, finish.key)?
        };
        if tell || !finish.ok {
            self.notice(host, &text);
        }
        self.said = text;
        // The cache changed: the list of photos is read anew.
        self.reload(host)?;
        if finish.ok {
            self.renew(host);
        }
        self.refresh_home(host).ok();
        Ok(())
    }

    /// A notice, where it can be shown.
    pub(crate) fn notice(&self, host: &Host<'_>, text: &str) {
        let mut scratch = [0; 512];
        if let Ok(notify) = paperpaw_sdk::native::ui::Notify::new(text) {
            CommandSender::new(host, &mut scratch).notify(notify).ok();
        }
    }
}

/// The names of the photos of `folder` on the card, sorted, as many as are
/// kept. A folder that is not there, is a file or cannot be read by this
/// plugin (no consent for the card) has none.
fn folder_names(host: &Host<'_>, folder: &str) -> Result<Vec<String>, Error> {
    let files = Files::open(host)?;
    let found = read_folder(host, &files, folder);
    // The service is released whatever the listing did.
    let released = files.release(host);
    let found = found?;
    released?;
    Ok(found)
}

fn read_folder(host: &Host<'_>, files: &Files, folder: &str) -> Result<Vec<String>, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut found = Found::default();
    let mut cursor = 0;
    for _ in 0..MOST_PAGES {
        let page = match files.list(host, Root::Files, folder, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(
                Error::NotFound
                | Error::Invalid
                | Error::Denied
                | Error::Unsupported
                | Error::NoSpace,
            ) => break,
            Err(error) => return Err(error),
        };
        for entry in page.entries() {
            found.add(entry.name(), entry.is_folder())?;
        }
        match page.next() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(found.finish())
}
