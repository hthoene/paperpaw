//! The interface-7 Stats plugin: what the host tells it about reading goes to
//! the `Recorder`, over the plugin's data folder and the user's files.
use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    commands::CommandSender,
    events::{Event, ReadingView},
    formatting::copy_message,
    registrations,
    reply::Reply,
    settings::{self, SettingValue},
    snapshot::{Snapshot, capture_snapshot},
    storage::{self as host_storage, DataStorage, Root},
    ui::{Notify, Open, Ui},
};
use paperpaw_sdk::{DirEntry, Handled, Storage, StorageError};
use paperpaw_ui_kit::Screens;

use crate::files::DataFolder;
use crate::native_calendar::CalendarView;
use crate::native_overview::OverviewView;
use crate::native_screens::{self, BookView, BooksView};
use crate::recorder::{Asked, Limits, Reading, Recorder, Surroundings, Told};

mod image {
    include!(concat!(env!("OUT_DIR"), "/stats_declarations.rs"));
}

/// The plugin's callback tokens (see `declarations.rs`).
const EVENTS: u32 = crate::declarations::EVENTS_TOKEN;
/// The stage Reading goal runs to ask for today's reading.
const ASKED: &str = "reading-goal.read-today";

/// An open screen's view.
#[allow(
    clippy::large_enum_variant,
    reason = "one view per open screen, in a list that grows fallibly; a box would allocate infallibly"
)]
enum View {
    Overview(OverviewView),
    Books(BooksView),
    Book(BookView),
    Calendar(CalendarView),
}

/// Stats' interface-7 entry state.
#[derive(Default)]
pub struct NativeStats {
    recorder: Recorder,
    /// `core.ui`, opened by the first callback (opening needs a live frame).
    ui: Option<Ui>,
    screens: Screens<View>,
}

impl core::fmt::Debug for NativeStats {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NativeStats").finish_non_exhaustive()
    }
}

impl Plugin for NativeStats {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::UI).chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        // A host that does not admit stages yet leaves the plugin without
        // the stage; the rest works.
        for batch in image::STAGES {
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
        event: Event<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if let Event::StageCall {
            stage: ASKED,
            schema: 1,
            payload,
        } = event
            && token == crate::declarations::ASKED_TOKEN
        {
            // From memory: no card read and no write inside this callback.
            let answer = with_cx(host, 0, |cx| self.recorder.asked(cx, payload));
            return match answer {
                Ok(bytes) => reply.write(&bytes),
                Err(Asked::Unknown) => Err(Error::NotFound),
                Err(Asked::Invalid) => Err(Error::Invalid),
            };
        }
        match event {
            Event::ActionRun {
                action: "stats.open",
                ..
            } if token == crate::declarations::OPEN_TOKEN => return self.open_overview(host),
            Event::ScreenOpened {
                screen,
                instance,
                handover,
                ..
            } => return self.opened(host, screen, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => return self.screens.shown(token, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                return Ok(());
            }
            _ => {}
        }
        if token != EVENTS {
            return Ok(());
        }
        let told = match event {
            Event::Started { now_ms } => with_cx(host, now_ms, |cx| self.recorder.started(cx)),
            Event::Resume { now_ms } => {
                with_cx(host, now_ms, |cx| self.recorder.resumed(cx));
                return Ok(());
            }
            Event::Suspend { now_ms } | Event::Stopping { now_ms } => {
                with_cx(host, now_ms, |cx| self.recorder.suspended(cx));
                return Ok(());
            }
            Event::PageTurned(view) => with_cx(host, view.now_ms(), |cx| {
                self.recorder.page_turned(cx, &reading(&view))
            }),
            Event::Book {
                kind: paperpaw_sdk::native::events::BookKind::Opened,
                reading: view,
                ..
            } => with_cx(host, view.now_ms(), |cx| {
                self.recorder.book_opened(cx, &reading(&view))
            }),
            Event::Book {
                kind: paperpaw_sdk::native::events::BookKind::Closed,
                reading: view,
                ..
            } => with_cx(host, view.now_ms(), |cx| {
                self.recorder.book_closed(cx, view.path())
            }),
            Event::Book {
                kind: paperpaw_sdk::native::events::BookKind::Finished,
                reading: view,
                ..
            } => {
                with_cx(host, view.now_ms(), |cx| {
                    self.recorder.book_finished(cx, view.path());
                });
                return Ok(());
            }
            Event::BookMoved { now_ms, from, path } => {
                with_cx(host, now_ms, |cx| self.recorder.book_moved(cx, from, path))
            }
            Event::StatusChanged { now_ms } => {
                with_cx(host, now_ms, |cx| self.recorder.clock_changed(cx))
            }
            _ => return Ok(()),
        };
        tell(host, told)
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        match self.screens.top(token).ok_or(Error::State)? {
            View::Overview(view) => view.draw(host, &mut frame),
            View::Books(view) => view.draw(host, &mut frame),
            View::Book(view) => view.draw(host, &mut frame),
            View::Calendar(view) => view.draw(host, &mut frame),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ui(host)?;
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        match view {
            View::Overview(view) => {
                let ui = self.ui.as_ref().ok_or(Error::State)?;
                view.input(host, ui, &input)
            }
            View::Books(view) => {
                let ui = self.ui.as_ref().ok_or(Error::State)?;
                view.input(host, ui, &input)
            }
            View::Book(view) => view.input(host, &input),
            View::Calendar(view) => view.input(host, &input),
        }
    }
}

impl NativeStats {
    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    /// Opens the overview, on the book being read where there is one (the page
    /// being read is recorded first, so that the figures include it).
    fn open_overview(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let current = with_cx(host, 0, |cx| {
            self.recorder.settle(cx);
            self.recorder.current_book()
        });
        match current {
            Some(book) => {
                native_screens::open_with_book(host, self.ui(host)?, "stats.overview", book)
            }
            None => {
                let mut scratch = [0u8; 256];
                CommandSender::new(host, &mut scratch).open(Open::screen("stats.overview")?)
            }
        }
    }

    /// Builds the view of an opened screen over the data folder.
    fn opened(
        &mut self,
        host: &Host<'_>,
        screen: &str,
        instance: paperpaw_sdk::native::ui::Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        let storage = DataStorage::new(host);
        let data = DataFolder(&storage);
        let view = match screen {
            "stats.overview" => {
                let current = match handover {
                    Some(handover) => native_screens::book_of(host, self.ui(host)?, handover)?,
                    None => None,
                };
                let today = today_of(host)?;
                View::Overview(OverviewView::new(host, &data, today, current)?)
            }
            "stats.calendar" => View::Calendar(CalendarView::new(host, &data, today_of(host)?)?),
            "stats.books" => View::Books(BooksView::new(host, &data)?),
            "stats.book" => {
                let id = match handover {
                    Some(handover) => native_screens::book_of(host, self.ui(host)?, handover)?,
                    None => None,
                };
                View::Book(BookView::new(host, &data, id)?)
            }
            _ => return Ok(()),
        };
        self.screens.open(instance, view)
    }
}

/// The local day (UTC until draft 25), none until the device knows the time.
fn today_of(host: &Host<'_>) -> Result<Option<i32>, Error> {
    Ok(capture_snapshot(host)?
        .unix_seconds()
        .and_then(|unix| u32::try_from(unix).ok())
        .map(native_screens::utc_day))
}

/// What the recorder has to say: the card could not be written to, once.
/// (Today's reading is asked for through the stage, not sent.)
fn tell(host: &Host<'_>, told: Told) -> Result<(), Error> {
    if !told.not_saved {
        return Ok(());
    }
    let mut text = [0u8; 128];
    let message = copy_message(host, "stats.not-saved", &mut text)?;
    let mut scratch = [0u8; 256];
    CommandSender::new(host, &mut scratch).notify(Notify::new(message)?.announce())
}

fn reading<'a>(view: &ReadingView<'a>) -> Reading<'a> {
    Reading {
        path: view.path(),
        title: view.title(),
        progress_ppm: view.progress_ppm(),
        page: view.page(),
        pages: view.pages(),
        at: (view.section(), view.offset(), view.insert()),
    }
}

/// The host as the recorder's surroundings: the data folder, the user's files
/// read only, the moment of the event and the clock as one snapshot says it.
struct HostCx<'h, 'cb> {
    host: &'h Host<'cb>,
    data: DataFolder<'h>,
    files: FilesRead<'h, 'cb>,
    now_ms: u64,
    snapshot: Option<Snapshot>,
}

/// Runs `then` with the surroundings of an event at `now_ms` (0: a call
/// without an event time, made now).
fn with_cx<R>(host: &Host<'_>, now_ms: u64, then: impl FnOnce(&HostCx<'_, '_>) -> R) -> R {
    let storage = DataStorage::new(host);
    let snapshot = capture_snapshot(host).ok();
    let cx = HostCx {
        host,
        data: DataFolder(&storage),
        files: FilesRead { host },
        now_ms: if now_ms == 0 {
            snapshot.map_or(0, |s| s.instant().millis())
        } else {
            now_ms
        },
        snapshot,
    };
    then(&cx)
}

impl HostCx<'_, '_> {
    fn flag(&self, key: &str, default: bool) -> bool {
        let mut scratch = [0u8; 32];
        match settings::read_setting(self.host, key, &mut scratch) {
            Ok(SettingValue::Bool(value)) => value,
            _ => default,
        }
    }

    fn number(&self, key: &str, default: i64) -> i64 {
        let mut scratch = [0u8; 32];
        match settings::read_setting(self.host, key, &mut scratch) {
            Ok(SettingValue::Int(value)) => value,
            _ => default,
        }
    }
}

impl Surroundings for HostCx<'_, '_> {
    fn data(&self) -> &dyn Storage {
        &self.data
    }
    fn files(&self) -> &dyn Storage {
        &self.files
    }
    fn now_ms(&self) -> u64 {
        self.now_ms
    }
    fn unix_at(&self, at_ms: u64) -> Option<i64> {
        let snapshot = self.snapshot?;
        crate::recorder::unix_at(snapshot.unix_seconds()?, snapshot.instant().millis(), at_ms)
    }
    /// UTC days until the system time zone comes with the snapshot (draft 25).
    fn day_of(&self, unix: u32) -> i32 {
        (unix / 86_400) as i32
    }
    fn limits(&self) -> Limits {
        Limits {
            enabled: self.flag("stats.enabled", true),
            min: self.number("stats.min-seconds", 5),
            max: self.number("stats.max-seconds", 120),
        }
    }
}

/// The user's files (wire root 1), read only: the books.
struct FilesRead<'h, 'cb> {
    host: &'h Host<'cb>,
}

fn refused(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Invalid => StorageError::InvalidPath,
        _ => StorageError::Io,
    }
}

impl Storage for FilesRead<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        host_storage::read_at(self.host, Root::Files, path, offset, buf).map_err(refused)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        host_storage::size(self.host, Root::Files, path).map_err(refused)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}
