//! The interface-7 news plugin, so far without a screen: the feeds fetched
//! over `core.net`, one after the other, made into pages (`native_fetch`,
//! `native_book`), and the work that fetches them in the background. The
//! screen, the library import (the shelf) and the headlines widget follow
//! (`PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, DrawPurpose, Input},
    commands::CommandSender,
    device::{Device, DeviceClass},
    events::{Event, Incoming, kinds},
    formatting::Arg,
    net::{FileRoot, Header, HttpDone, Net, REPLY, REQUEST_INPUT, Request, RequestId},
    registrations,
    reply::Reply,
    storage::{self, Root},
    ui::{Notify, Open, RequestId as AskId, Ui},
    work::{Moment, Work, WorkSpec},
};
use paperpaw_ui_kit::{Messages, Screens, files::Volume, mem, read_event, text, widget::Settings};

use crate::declarations::{FETCH_TOKEN, HTTP_TOKEN, OPEN_TOKEN, SCREEN, SETTING_TOKEN, WORK_TOKEN};

/// The key of the work that fetches the news in the background.
const WORK_KEY: &str = "news.fetch";
/// The `User-Agent` of interface 6 where the kernel is not listed.
const USER_AGENT: &str = "PaperPaw/0.1 (e-reader)";
/// The background fetch comes no sooner than this, in milliseconds.
const BACKGROUND_AFTER_MS: u32 = 3_600_000;
use crate::native_fetch::{Done, Ended, FEED_REQUEST, Fetcher, Started, Wire};
use crate::native_news::{MAX_FEED_FILE, MAX_TEXT, MAX_TEXT_SMALL};
use crate::native_screen::{Ask, View};
use crate::native_shelf::Shelved;
use crate::native_widgets::{Tap, Widgets};

mod image {
    include!(concat!(env!("OUT_DIR"), "/news_declarations.rs"));
}

/// The news plugin's state: made at the first need ([`NativeNews`]).
#[derive(Debug, Default)]
pub(crate) struct News {
    fetcher: Fetcher,
    shelf: Shelved,
    pub(crate) handles: Handles,
    pub(crate) screens: Screens<View>,
    pub(crate) widgets: Widgets,
    /// The question of the screen that is open, and what it is for.
    pub(crate) asked: Option<(AskId, Ask)>,
}

/// The handles of the services, opened when first needed.
#[derive(Debug, Default)]
pub(crate) struct Handles {
    ui: Option<Ui>,
    net: Option<Net>,
    work: Option<Work>,
    /// The text kept of an article, read from the device's memory class once.
    text_cap: Option<usize>,
}

impl Handles {
    /// The UI service, opened when first needed.
    pub(crate) fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }
}

/// The text kept of an article: [`MAX_TEXT`] on a medium or large device, as
/// in interface 6; [`MAX_TEXT_SMALL`] on a small one (the X4: the peak is about
/// four times it) and wherever the class cannot be read.
fn text_cap(host: &Host<'_>, handles: &mut Handles) -> usize {
    if let Some(cap) = handles.text_cap {
        return cap;
    }
    let cap = cap_of(read_cap(host));
    handles.text_cap = Some(cap);
    cap
}

/// The cap for a class: larger only where the device is known to be.
pub(crate) fn cap_of(class: Result<DeviceClass, Error>) -> usize {
    match class {
        Ok(DeviceClass::Medium | DeviceClass::Large) => MAX_TEXT,
        _ => MAX_TEXT_SMALL,
    }
}

fn read_cap(host: &Host<'_>) -> Result<DeviceClass, Error> {
    let device = Device::open(host)?;
    let class = read_class(host, &device);
    // A handle that cannot be released is no reason to lose the class.
    let _ = device.release(host);
    class
}

fn read_class(host: &Host<'_>, device: &Device) -> Result<DeviceClass, Error> {
    let mut input = mem::buffer(mem::SERVICE_BUFFER)?;
    let mut out = mem::buffer(mem::SERVICE_BUFFER)?;
    Ok(device.device(host, &mut input, &mut out)?.class())
}

/// The host as the fetch uses it.
struct HostWire<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a mut Handles,
    volume: DataVolume<'a, 'h>,
}

impl<'a, 'h> HostWire<'a, 'h> {
    fn new(host: &'a Host<'h>, handles: &'a mut Handles) -> Self {
        Self {
            host,
            handles,
            volume: DataVolume(host),
        }
    }

    fn net(&mut self) -> Result<&Net, Error> {
        if self.handles.net.is_none() {
            self.handles.net = Some(Net::open(self.host)?);
        }
        self.handles.net.as_ref().ok_or(Error::State)
    }

    fn work(&mut self) -> Result<&Work, Error> {
        if self.handles.work.is_none() {
            self.handles.work = Some(Work::open(self.host)?);
        }
        self.handles.work.as_ref().ok_or(Error::State)
    }
}

impl Wire for HostWire<'_, '_> {
    fn volume(&self) -> &dyn Volume {
        &self.volume
    }

    fn get(&mut self, url: &str, file: &str) -> Result<RequestId, Error> {
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        let headers = [Header::new("User-Agent", USER_AGENT)?];
        let request = Request::get(FEED_REQUEST, url)?
            .headers(&headers)?
            .save_to(FileRoot::Data, file)?
            .max_file(MAX_FEED_FILE)?;
        let host = self.host;
        self.net()?.request(host, &request, &mut input, &mut out)
    }

    fn cancel(&mut self) {
        let host = self.host;
        if let (Ok(mut input), Ok(mut out)) = (mem::buffer(REQUEST_INPUT), mem::buffer(REPLY))
            && let Ok(net) = self.net()
        {
            net.cancel(host, FEED_REQUEST, &mut input, &mut out).ok();
        }
    }

    fn declare_work(&mut self) -> Result<(), Error> {
        let spec = WorkSpec::when_cheap(WORK_KEY, WORK_TOKEN)?
            .after_ms(BACKGROUND_AFTER_MS)
            .at_moment(Moment::Wake)
            .at_moment(Moment::Online);
        let mut input = mem::buffer(256)?;
        let mut out = mem::buffer(64)?;
        let host = self.host;
        self.work()?.declare(host, &spec, &mut input, &mut out)
    }
}

/// The storage calls on the plugin's data folder.
pub(crate) struct DataVolume<'a, 'h>(pub(crate) &'a Host<'h>);

impl Volume for DataVolume<'_, '_> {
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

/// The interface-7 news plugin. Idle, only the pointer to the state is held;
/// the state (the fetch, the shelf, the handles, the screens and the widget) is
/// made by the first event that needs it and kept.
#[derive(Debug, Default)]
pub struct NativeNews {
    /// None before the first need; at most one.
    state: Vec<News>,
}

impl NativeNews {
    fn state(&mut self) -> Result<&mut News, Error> {
        if self.state.is_empty() {
            self.state
                .try_reserve_exact(1)
                .map_err(|_| Error::NoMemory)?;
            // Made in place: built as a value first, it would be on the stack
            // as large as it is, deep in a callback.
            self.state.resize_with(1, News::default);
        }
        self.state.first_mut().ok_or(Error::State)
    }
}

impl Plugin for NativeNews {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::UI)
            .chain(image::SCREENS)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // The home widget, and the subscription to the settings that change (it
        // shows them anew): a host that does not admit a part of them yet answers
        // `Unsupported`, and the plugin goes on without it.
        for batch in image::WIDGETS.iter().chain(image::WIDGET_EVENTS) {
            match registrations::submit(host, batch) {
                Ok(()) | Err(Error::Unsupported) => {}
                Err(error) => return Err(error),
            }
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
        // Without state, the action that opens the list needs none.
        if self.state.is_empty() && token == OPEN_TOKEN {
            return open_list(host);
        }
        self.state()?.event(host, token, incoming, reply)
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, frame: DrawFrame<'_>) -> Result<(), Error> {
        self.state
            .first_mut()
            .ok_or(Error::State)?
            .draw(host, token, frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match self.state.first_mut() {
            Some(state) => state.input(host, token, input),
            None => Ok(Handled::No),
        }
    }
}

/// Asks for the list of feeds.
fn open_list(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch).open(Open::screen("news.list")?)
}

impl News {
    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::ActionRun,
            kinds::WorkDue,
            kinds::HttpDone,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::Answered,
            kinds::SettingChanged,
            kinds::WidgetMade,
            kinds::WidgetDropped,
            kinds::StageCall
        ) else {
            return Ok(());
        };
        match event {
            // The action (a rule's background action): fetch when it is cheap.
            Event::ActionRun { .. } if token == FETCH_TOKEN => {
                HostWire::new(host, &mut self.handles)
                    .declare_work()
                    .or_else(|error| match error {
                        Error::NoMemory => Err(error),
                        // A work that cannot be declared (a full table, the network
                        // turned off) costs this one fetch.
                        _ => Ok(()),
                    })
            }
            Event::ActionRun { .. } if token == OPEN_TOKEN => open_list(host),
            Event::WorkDue(_) if token == WORK_TOKEN => self.start(host, 0, true),
            Event::HttpDone(done) if token == HTTP_TOKEN => {
                let ended = self.ended(host, &done);
                // The names of the feeds and their books may have changed.
                self.widgets.refresh(host);
                ended.and(self.refresh(host))
            }
            Event::WidgetMade {
                widget, instance, ..
            } => self.widgets.made(widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.widgets.dropped(instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.widgets.stage(host, payload, reply),
            Event::SettingChanged { key, .. } if token == SETTING_TOKEN => {
                if key.starts_with("news.widget.") {
                    self.widgets.refresh(host);
                }
                Ok(())
            }
            Event::ScreenOpened { instance, .. } if token == SCREEN => {
                self.screen_opened(host, instance)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.screen_shown(host, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose == DrawPurpose::Widget {
            return self.widgets.draw(host, frame);
        }
        if token != SCREEN {
            return Err(Error::Unsupported);
        }
        self.draw_screen(host, frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        // A tap on a headline opens its book, a tap on the empty widget the list.
        if let Some(tap) = self.widgets.tapped(host, &input)? {
            match tap {
                Tap::Book(path) => self.open_book(host, &path)?,
                Tap::List => {
                    let mut scratch = [0; 128];
                    CommandSender::new(host, &mut scratch).open(Open::screen("news.list")?)?;
                }
            }
            return Ok(Handled::Yes);
        }
        if token != SCREEN {
            return Ok(Handled::No);
        }
        self.input_screen(host, &input)
    }
}

impl News {
    /// Starts a fetch and says how it went.
    pub(crate) fn start(&mut self, host: &Host<'_>, from: usize, all: bool) -> Result<(), Error> {
        let mut wire = HostWire::new(host, &mut self.handles);
        match self.fetcher.start(&mut wire, from, all)? {
            Started::Fetching => notice(host, "news.fetching"),
            Started::NoNetwork => notice(host, "news.no-network"),
            Started::Unreadable => notice(host, "news.unreadable"),
            Started::None => notice(host, "news.none"),
        }
    }

    fn ended(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let settings = Settings::new(host, "news");
        let articles = usize::try_from(settings.int("articles", 30)?.clamp(1, 200)).unwrap_or(30);
        let messages = RefCell::new(Messages::new()?);
        let date = |year: i32, month: u8, day: u8| date_text(host, &messages, year, month, day);
        let cap = text_cap(host, &mut self.handles);
        let mut wire = HostWire::new(host, &mut self.handles);
        let end = Done::new(done.id(), done.token(), done.reason(), done.saved());
        let mut shelf = self.shelf.on(host);
        let ended = self
            .fetcher
            .ended(&mut wire, &mut shelf, &end, (articles, cap), &date)?;
        match ended {
            Ended::Ignored | Ended::Next => Ok(()),
            Ended::Unreadable => notice(host, "news.unreadable"),
            Ended::Memory => notice(host, "news.no-memory"),
            Ended::NoNetwork { .. } => notice(host, "news.no-network"),
            Ended::Finished { done, failed } => {
                let mut messages = messages.into_inner();
                let count = i64::try_from(done).unwrap_or(i64::MAX);
                let message = if failed == 0 {
                    messages.format(host, "news.fetched", &[Arg::Count(count)])?
                } else {
                    let failed = i64::try_from(failed).unwrap_or(i64::MAX);
                    messages.format(
                        host,
                        "news.fetched-failed",
                        &[
                            Arg::Count(failed),
                            Arg::Int {
                                name: "done",
                                value: count,
                            },
                        ],
                    )?
                };
                tell(host, &message)
            }
        }
    }
}

/// A day as the reader's language writes it short, with the year:
/// `Sat 24 Oct 2026`.
fn date_text(
    host: &Host<'_>,
    messages: &RefCell<Messages>,
    year: i32,
    month: u8,
    day: u8,
) -> Result<String, Error> {
    const WEEKDAYS: [&str; 7] = [
        "core.weekday.short.0",
        "core.weekday.short.1",
        "core.weekday.short.2",
        "core.weekday.short.3",
        "core.weekday.short.4",
        "core.weekday.short.5",
        "core.weekday.short.6",
    ];
    const MONTHS: [&str; 13] = [
        "core.month.short.0",
        "core.month.short.1",
        "core.month.short.2",
        "core.month.short.3",
        "core.month.short.4",
        "core.month.short.5",
        "core.month.short.6",
        "core.month.short.7",
        "core.month.short.8",
        "core.month.short.9",
        "core.month.short.10",
        "core.month.short.11",
        "core.month.short.12",
    ];
    let weekday = WEEKDAYS
        .get(usize::from(paperpaw_sdk::time::weekday(year, month, day)))
        .copied()
        .unwrap_or(WEEKDAYS[0]);
    let month = MONTHS.get(usize::from(month)).copied().unwrap_or(MONTHS[0]);
    let mut messages = messages.borrow_mut();
    let (weekday, month) = (messages.copy(host, weekday)?, messages.copy(host, month)?);
    let short = messages.format(
        host,
        "core.date.short",
        &[
            Arg::Text {
                name: "weekday",
                value: &weekday,
            },
            Arg::Int {
                name: "day",
                value: i64::from(day),
            },
            Arg::Text {
                name: "month",
                value: &month,
            },
        ],
    )?;
    text::format(format_args!("{short} {year}"))
}

/// The notice `key`, shown to the reader (a run's notices go to its note).
pub(crate) fn notice(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let message = Messages::new()?.copy(host, key)?;
    tell(host, &message)
}

/// A courtesy: a notice that cannot be shown (no room, the host refuses)
/// changes nothing else.
fn tell(host: &Host<'_>, message: &str) -> Result<(), Error> {
    let mut scratch = [0; 512];
    CommandSender::new(host, &mut scratch)
        .notify(Notify::new(message)?)
        .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_follows_the_memory_class_and_is_small_when_unknown() {
        assert_eq!(cap_of(Ok(DeviceClass::Small)), 8 * 1024);
        assert_eq!(cap_of(Ok(DeviceClass::Medium)), 64 * 1024);
        assert_eq!(cap_of(Ok(DeviceClass::Large)), 64 * 1024);
        assert_eq!(cap_of(Err(Error::NotFound)), 8 * 1024);
        assert_eq!(cap_of(Err(Error::NoMemory)), 8 * 1024);
    }
}
