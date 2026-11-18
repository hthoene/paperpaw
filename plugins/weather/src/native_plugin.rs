//! The interface-7 weather plugin: the weather fetched over `core.net` (looked
//! at again by `core.work`, kept in a small file), shown by the widgets *Weather
//! now* and *Forecast*, and the place chosen on its own screen.
//!
//! What the plugin decides without a host is in `native_weather`,
//! `native_answers` and `native_fetch`; the widgets are drawn by `native_now`
//! and `native_forecast` and the place's screen by `native_place`. This file
//! is the glue: it routes the host's events and callbacks to them and tells the
//! widgets when what they show has changed (`PORTING-7.md`).
use crate::{
    declarations::{
        CHOOSE_PLACE_TOKEN, FORECAST_TOKEN, HTTP_TOKEN, LIFECYCLE_TOKEN, NOW_TOKEN, PLACE_SCREEN,
        REFRESH_TOKEN, WORK_TOKEN,
    },
    native_fetch::{COORDINATES, Fetched, Fetcher, Looked, PLACE, Searched, Why, setting},
    native_forecast, native_now,
    native_place::{Act, PlaceView, Search},
    native_weather::{self as model},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        i18n, registrations,
        reply::Reply,
        settings::SettingValue,
        ui::{
            Instance, Notify, Question, RequestId, Ui, WidgetInputKind, WidgetTarget,
            stages::WidgetCall,
        },
    },
};
use paperpaw_ui_kit::{
    Messages, Screens,
    mem::{SERVICE_BUFFER, buffer},
    paint, read_event, uitext,
    widget::{Cx, settings::base},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/weather_declarations.rs"));
}

/// The most bytes of a stage reply (a size and a target).
const REPLY: usize = 512;
/// The longest name searched for, in bytes.
const NAME_CAP: usize = 256;
/// How long *Go online* keeps the network for the search, in milliseconds.
const ONLINE_MS: u32 = 600_000;
/// The target of a widget: all of it.
const TARGET: u32 = 1;

/// A widget instance the host made.
#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
}

/// The interface-7 weather plugin.
#[derive(Debug, Default)]
pub struct NativeWeather {
    fetcher: Fetcher,
    items: Vec<Item>,
    ui: Option<Ui>,
    screens: Screens<PlaceView>,
    /// The name being asked for on the keyboard.
    asking: Option<RequestId>,
}

impl Plugin for NativeWeather {
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
            Event::Started { .. } | Event::Resume { .. } if token == LIFECYCLE_TOKEN => {
                self.look(host, Why::Due)
            }
            Event::WorkDue(_) if token == WORK_TOKEN => self.look(host, Why::Due),
            Event::SettingChanged { key, .. } if token == LIFECYCLE_TOKEN => {
                self.setting_changed(host, key)
            }
            Event::HttpDone(done) if token == HTTP_TOKEN => self.http_done(host, &done),
            Event::ActionRun { .. } if token == CHOOSE_PLACE_TOKEN => open_place(host),
            Event::ActionRun { .. } if token == REFRESH_TOKEN => self.refresh(host),
            Event::WidgetMade {
                widget, instance, ..
            } => {
                self.made(widget, instance)?;
                // A widget that is placed is a reason to look.
                self.look(host, Why::Due)
            }
            Event::WidgetDropped { instance, .. } => {
                self.items.retain(|item| item.instance != instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::ScreenOpened { instance, .. } if token == PLACE_SCREEN => {
                self.opened(host, instance)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == PLACE_SCREEN => {
                self.screens.shown(token, instance)?;
                self.shown(host)
            }
            Event::ScreenClosed { instance, .. } if token == PLACE_SCREEN => {
                self.fetcher.cancel_search(host);
                self.asking = None;
                self.screens.close(instance);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token == PLACE_SCREEN {
            return self
                .screens
                .top(token)
                .ok_or(Error::State)?
                .draw(host, &mut frame);
        }
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self
            .items
            .iter()
            .find(|item| item.instance.get() == frame.id)
            .ok_or(Error::NotFound)?;
        let context = self.fetcher.context(host)?;
        let shown = self.fetcher.shown(&context);
        let mut cx = Cx::open(host, &item.widget)?;
        let area = area_of(frame.rect);
        let mut out = Vec::new();
        match item.token {
            NOW_TOKEN => native_now::draw(&mut cx, &mut out, area, &shown)?,
            FORECAST_TOKEN => native_forecast::draw(&mut cx, &mut out, area, &shown)?,
            _ => return Err(Error::NotFound),
        }
        paint::replay(&mut frame, &cx.font, &out)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == PLACE_SCREEN {
            return self.place_input(host, token, &input);
        }
        let Input::Widget {
            instance,
            target: TARGET,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        self.find(instance.get()).ok_or(Error::NotFound)?;
        // A tap chooses the place if none is set, else fetches the weather now.
        if setting(host, COORDINATES)?.is_empty() {
            open_place(host)?;
        } else {
            self.refresh(host)?;
        }
        Ok(Handled::Yes)
    }
}

impl NativeWeather {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        let token = match base(widget) {
            "weather.now" => NOW_TOKEN,
            "weather.forecast" => FORECAST_TOKEN,
            _ => return Ok(()),
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: paperpaw_ui_kit::text::copy(widget)?,
        });
        Ok(())
    }

    /// Tells every weather widget that what it shows has changed. A widget the
    /// host cannot be told about now is left; the next change tells it.
    fn changed(&self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &self.items {
            commands.widget_changed(item.instance).ok();
        }
    }

    /// Looks whether the weather is due (and fetches it if `why` calls for it).
    fn look(&mut self, host: &Host<'_>, why: Why) -> Result<(), Error> {
        let looked = self.fetcher.check(host, why, !self.items.is_empty())?;
        if let Looked::Fetching { told } = looked {
            // The widgets say it is being fetched.
            self.changed(host);
            if told {
                notice(host, "weather.fetching");
            }
        }
        Ok(())
    }

    /// The action *Refresh*: fetches now, or asks for a place where none is set.
    fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if model::split_coordinates(&setting(host, COORDINATES)?).is_none() {
            return open_place(host);
        }
        self.look(host, Why::Asked)
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key == COORDINATES {
            let has = model::split_coordinates(&setting(host, COORDINATES)?).is_some();
            self.fetcher.place_changed(host, has);
            self.changed(host);
            if has {
                return self.look(host, Why::Asked);
            }
            return Ok(());
        }
        // The units, how often, the place's name, a widget's own settings: the
        // widgets show them anew.
        if key.is_empty() || key.starts_with("weather.") {
            self.changed(host);
            if key == crate::native_fetch::EVERY || key.is_empty() {
                return self.look(host, Why::Due);
            }
        }
        Ok(())
    }

    fn http_done(
        &mut self,
        host: &Host<'_>,
        done: &paperpaw_sdk::native::net::HttpDone,
    ) -> Result<(), Error> {
        if let Some(fetched) = self.fetcher.fetched(host, done)? {
            // Shown at once, the new weather or that it failed.
            self.changed(host);
            if fetched == (Fetched::Failed { told: true }) {
                notice(host, "weather.failed");
            }
            return Ok(());
        }
        let Some(searched) = self.fetcher.searched(host, done)? else {
            return Ok(());
        };
        let Some(view) = self.screens.top(PLACE_SCREEN) else {
            return Ok(());
        };
        let name = paperpaw_ui_kit::text::copy(view.initial())?;
        match searched {
            Searched::Found(places) => view.found(places, name)?,
            Searched::Offline => view.offer(name),
            Searched::Failed => view.ended(Search::Failed)?,
        }
        redraw(host)
    }

    // The place's screen.

    fn opened(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let place = setting(host, PLACE)?;
        let has = !setting(host, COORDINATES)?.is_empty();
        let view = PlaceView::new(host, &place, has)?;
        self.screens.open(instance, view)
    }

    /// The place screen is shown: the keyboard opens at once where no place
    /// is set.
    fn shown(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(view) = self.screens.top(PLACE_SCREEN) else {
            return Ok(());
        };
        if view.asks_at_once() {
            let initial = String::new();
            self.ask(host, &initial)?;
        }
        Ok(())
    }

    fn place_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let act = view.input(host, input)?;
        let handled = PlaceView::handled(&act);
        self.act(host, act)?;
        Ok(handled)
    }

    fn act(&mut self, host: &Host<'_>, act: Act) -> Result<(), Error> {
        match act {
            Act::Ignored => Ok(()),
            Act::Taken => redraw(host),
            Act::Ask(initial) => self.ask(host, &initial),
            Act::Pick(index) => {
                let chosen = self
                    .screens
                    .top(PLACE_SCREEN)
                    .and_then(|view| view.place(index));
                let Some(chosen) = chosen else {
                    return Ok(());
                };
                let (name, coordinates) = (
                    paperpaw_ui_kit::text::copy(&chosen.name)?,
                    chosen.coordinates()?,
                );
                self.set_place(host, &name, &coordinates)
            }
            Act::Clear => self.set_place(host, "", ""),
            Act::Online(name) => {
                // Refused (a device that cannot): the search is sent all the
                // same, and says again that the device is offline.
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch)
                    .go_online(ONLINE_MS)
                    .ok();
                self.search(host, name)
            }
            Act::NotNow => {
                if let Some(view) = self.screens.top(PLACE_SCREEN) {
                    view.ended(Search::NoNetwork)?;
                }
                redraw(host)
            }
            Act::Back => {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).back().ok();
                Ok(())
            }
        }
    }

    /// The place chosen (or none): its name and coordinates are the settings,
    /// and the screen goes.
    fn set_place(&mut self, host: &Host<'_>, name: &str, coordinates: &str) -> Result<(), Error> {
        let mut scratch = buffer(1024)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        commands.set_setting(PLACE, SettingValue::Text(name))?;
        commands.set_setting(COORDINATES, SettingValue::Text(coordinates))?;
        commands.back()
    }

    /// Asks for the name on the keyboard, `initial` first.
    fn ask(&mut self, host: &Host<'_>, initial: &str) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "weather.place.name")?;
        let ui = self.ui(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = ui.ask(host, Question::text(&title, "")?, &mut input, &mut out)?;
        if !initial.is_empty() {
            uitext::write_all(ui, host, request.text(), initial)?;
        }
        self.asking = Some(request);
        Ok(())
    }

    /// The name was typed: searched for.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if self.asking != Some(answer.request()) || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        self.asking = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let ui = self.ui(host)?;
        let typed = uitext::read_all(ui, host, answer.request().text(), NAME_CAP);
        let name = match typed {
            Ok(typed) => typed.trim().into(),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // A name that is too long or cannot be read is none.
            Err(_) => String::new(),
        };
        if name.is_empty() {
            return Ok(());
        }
        self.search(host, name)
    }

    /// Sends the search for `name`; the view says so.
    fn search(&mut self, host: &Host<'_>, name: String) -> Result<(), Error> {
        let mut scratch = [0; 32];
        let language = i18n::language(host, &mut scratch).unwrap_or("en");
        let Some(view) = self.screens.top(PLACE_SCREEN) else {
            return Ok(());
        };
        let sent = self.fetcher.search(host, &name, language);
        match sent {
            Ok(()) => view.searching(name)?,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // No network at all (or this plugin's network off).
            Err(Error::Unsupported | Error::Denied) => view.ended(Search::NoNetwork)?,
            Err(_) => view.ended(Search::Failed)?,
        }
        redraw(host)
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
            WidgetCall::Measure { instance, width } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let (token, widget) = (item.token, paperpaw_ui_kit::text::copy(&item.widget)?);
                let context = self.fetcher.context(host)?;
                let shown = self.fetcher.shown(&context);
                let mut cx = Cx::open(host, &widget)?;
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                let height = match token {
                    NOW_TOKEN => native_now::height(&mut cx, &shown, width)?,
                    FORECAST_TOKEN => native_forecast::height(&mut cx, &shown, width)?,
                    _ => return Err(Error::NotFound),
                };
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

/// Opens the place's screen.
fn open_place(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open_screen("weather.place-screen")
}

/// The notice `key`, shown to the reader. A courtesy: one that cannot be shown
/// (no text, no room, the host refuses) changes nothing else.
fn notice(host: &Host<'_>, key: &str) {
    let shown = (|| {
        let text = Messages::new()?.copy(host, key)?;
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    })();
    let _ = shown;
}

/// Asks for the screen to be drawn again. A refusal (it is not on top any more)
/// is not an error of what changed it.
#[allow(clippy::unnecessary_wraps, reason = "reads as the other steps do")]
fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(RedrawMode::Partial)
        .ok();
    Ok(())
}
