//! Reading statistics: how long, how much and on which days one reads,
//! with an overview, a calendar and every book's numbers.
//!
//! Recorded as KOReader records its statistics, so the two can exchange
//! them: every stay on a page with its start, duration and the book's
//! pages then; stays shorter than *Shortest page* (5 s) are left out and
//! longer ones count as *Longest page* (2 min), so a book left open does
//! not count as read. Books are recognised by KOReader's fingerprint of
//! the file (a partial MD5). Reading pauses while the app is in the
//! background and ends when the book is closed. Stays are timed on the
//! device's monotonic clock and dated by the wall clock: read before the
//! device knows the time (an X4 until Wi-Fi sets its clock), they wait
//! in memory and are recorded once it does. See `store` for the files.
//!
//! The statistics are shared with KOReader's: through the cloud folder
//! KOReader syncs them to (WebDAV), and by taking over those of KOReader
//! on the same device (see `sync`); its database format is in
//! `koreader`, the merge in `merge`.

#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod book;
mod books;
mod calendar;
mod chart;
mod extsort;
mod files;
mod fmt;
mod koreader;
mod left;
mod merge;
mod overview;
mod room;
mod store;
mod sync;
mod totals;
mod widget;

#[cfg(test)]
mod testing;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, menus, params};
use paperpaw_sdk::digest::md5;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, FixedScreen, Id, Instant, Manifest, MenuEntry, Params,
    Plugin, RegisterError, Registrar, SettingSpec, StorageError, time,
};

use paperpaw_sdk::setting::Setting;
use store::{Book, PageStat};

pub const PLUGIN: Id = Id::new_static("stats");
/// Records reading.
pub const ENABLED: Setting<bool> = Setting::new("stats.enabled");
/// Stays on a page shorter than this many seconds are not counted.
pub const MIN_SECONDS: Setting<i64> = Setting::new("stats.min-seconds");
/// Stays on a page count at most this many seconds.
pub const MAX_SECONDS: Setting<i64> = Setting::new("stats.max-seconds");
/// Opens the overview.
pub const OPEN: Id = Id::new_static("stats.open");

pub(crate) const GROUP: Id = Id::new_static("stats.group");
const OVERVIEW: Id = Id::new_static("stats.overview");
const CALENDAR: Id = Id::new_static("stats.calendar");
const BOOKS: Id = Id::new_static("stats.books");
const BOOK: Id = Id::new_static("stats.book");

/// Most stays kept while the device does not know the time (about
/// 4 KiB): an hour or two of reading.
const UNDATED: usize = 128;

/// The book being read and the page it is on.
#[derive(Debug, Clone)]
struct Session {
    /// Its id in `books.ini`; `None` until the device knows the time (a
    /// book is added with when it was opened).
    book: Option<u16>,
    path: Rc<str>,
    page: u32,
    pages: u32,
    /// Where the page begins: pages counted by the progress may share a
    /// number (see [`page_of`]).
    at: Locator,
    /// When reading the page began; `None` while paused.
    since: Option<Instant>,
}

/// A stay on a page, timed on the monotonic clock: dated once the device
/// knows the time.
#[derive(Debug, Clone)]
struct Stay {
    path: Rc<str>,
    page: u32,
    pages: u32,
    start: Instant,
    /// Seconds, as counted (see [`counted`]).
    duration: u16,
}

#[derive(Debug, Default)]
struct State {
    session: Option<Session>,
    /// Stays read while the device did not know the time (or not written
    /// yet), oldest first, at most [`UNDATED`]; gone if the device is
    /// switched off before.
    undated: Vec<Stay>,
    /// Writing the statistics failed (e.g. the card is full): `Some(false)`
    /// until that was said, `Some(true)` after; `None` while writing works.
    write_failed: Option<bool>,
    /// The day and the seconds read on it, summed once from the records
    /// and counted on as stays are recorded (see [`State::tell`]).
    today: Option<(i32, u32)>,
    /// What was told last of today ([`events::READ_TODAY`]).
    told: Option<(i32, u32)>,
}

type Shared = Rc<RefCell<State>>;

/// The page and the number of pages of a reading event: as the reader
/// counted them, else the progress in tenths of a percent of 1000 pages
/// (KOReader rescales pages to the book's count when it sums them).
fn page_of(reading: &events::Reading) -> (u32, u32) {
    if let (Some(page), Some(pages)) = (reading.page, reading.pages)
        && pages > 0
    {
        return (page.clamp(1, pages), pages);
    }
    let page = (reading.progress * 10.0) as u32 + 1;
    (page.clamp(1, 1000), 1000)
}

/// How long a stay counts: nothing if shorter than the minimum, at most
/// the maximum (KOReader's rule).
fn counted(seconds: i64, min: i64, max: i64) -> u16 {
    if seconds < min {
        0
    } else {
        seconds.min(max).clamp(0, i64::from(u16::MAX)) as u16
    }
}

impl State {
    /// What `result` of writing the statistics holds; `None` where it
    /// failed, which is said once (see [`State::tell`]).
    fn wrote<T>(&mut self, result: Result<T, StorageError>) -> Option<T> {
        match result {
            Ok(value) => {
                self.write_failed = None;
                Some(value)
            }
            Err(_) => {
                self.write_failed.get_or_insert(false);
                None
            }
        }
    }

    /// Says that the statistics could not be saved, once until writing
    /// works again.
    fn tell(&mut self, env: &Env<'_>, out: &mut Commands) {
        if self.write_failed == Some(false) {
            out.push(Command::Notify(
                env.i18n().tr("stats.not-saved").to_string(),
            ));
            self.write_failed = Some(true);
        }
        // How long was read today, for those who show it (a reading goal),
        // when it changed.
        if let Some(today) = self.read_today(env)
            && self.told != Some(today)
        {
            self.told = Some(today);
            let (day, seconds) = today;
            out.push(events::READ_TODAY.emit(events::ReadToday::new(day, seconds)));
        }
    }

    /// Today and the seconds read on it, if the device knows the date:
    /// summed from the records on a new day, else as counted on.
    fn read_today(&mut self, env: &Env<'_>) -> Option<(i32, u32)> {
        let day = fmt::today(env)?;
        if let Some(today) = self.today.filter(|(d, _)| *d == day) {
            return Some(today);
        }
        // Not read: nothing is cached (a zero would be told as today's).
        let days = store::days(env.storage(), |unix| fmt::day_of(env, unix)).ok()?;
        let seconds = days.iter().find(|d| d.day == day).map_or(0, |d| d.seconds);
        self.today = Some((day, seconds));
        self.today
    }

    /// Ends the stay on the current page now: recorded, or kept until the
    /// device knows the time.
    fn finish_page(&mut self, env: &Env<'_>) {
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(since) = session.since.take() else {
            return;
        };
        let min = env.settings().read(&MIN_SECONDS);
        let max = env.settings().read(&MAX_SECONDS);
        let seconds = i64::try_from(env.now().since(since) / 1000).unwrap_or(i64::MAX);
        let duration = counted(seconds, min, max);
        if duration == 0 || !enabled(env) {
            return;
        }
        if self.undated.len() < UNDATED && self.undated.try_reserve(1).is_ok() {
            self.undated.push(Stay {
                path: session.path.clone(),
                page: session.page,
                pages: session.pages,
                start: since,
                duration,
            });
        }
        self.date(env);
    }

    /// Once the device knows the time: records the stays kept until then,
    /// and adds the book being read to the books if it is not yet.
    fn date(&mut self, env: &Env<'_>) {
        let Some(clock) = env.status().clock else {
            return;
        };
        if self.undated.is_empty() && self.session.as_ref().is_none_or(|s| s.book.is_some()) {
            return;
        }
        let unix = |at: Instant| clock.unix_millis_at(at).div_euclid(1000);
        // Each book's id, looked up (or the book added) once.
        let mut ids: Vec<(Rc<str>, Option<u16>)> = Vec::new();
        if let Some(Session {
            path,
            book: Some(book),
            ..
        }) = &self.session
        {
            ids.push((path.clone(), Some(*book)));
        }
        let mut stays = core::mem::take(&mut self.undated).into_iter();
        while let Some(stay) = stays.next() {
            let start = unix(stay.start);
            let book = match ids.iter().find(|(path, _)| *path == stay.path) {
                Some(&(_, id)) => id,
                None => {
                    let id = register_book(env, &stay.path, start, stay.pages);
                    ids.push((stay.path.clone(), id));
                    id
                }
            };
            if let (Some(book), Ok(start @ 1..)) = (book, u32::try_from(start)) {
                let stat = PageStat {
                    book,
                    page: stay.page,
                    start,
                    duration: stay.duration,
                    pages: stay.pages,
                };
                let recorded = self.wrote(store::append(env.storage(), stat));
                if let Some((day, seconds)) = &mut self.today
                    && recorded.is_some()
                    && fmt::day_of(env, start) == *day
                {
                    *seconds = seconds.saturating_add(u32::from(stay.duration));
                }
                if recorded.is_none() {
                    // Kept to be written with the next page.
                    self.undated.push(stay);
                    self.undated.extend(stays);
                    break;
                }
            }
        }
        if let Some(session) = &mut self.session
            && session.book.is_none()
        {
            session.book = match ids.iter().find(|(path, _)| *path == session.path) {
                Some(&(_, id)) => id,
                None => register_book(env, &session.path, unix(env.now()), session.pages),
            };
            // A book that cannot be read is not recorded.
            if session.book.is_none() {
                self.session = None;
            }
        }
    }

    /// The reader shows the page of `reading`.
    fn turn(&mut self, env: &Env<'_>, reading: &events::Reading) {
        let ((page, pages), at) = (page_of(reading), reading.at());
        let unchanged = self
            .session
            .as_ref()
            .is_none_or(|s| (s.page, s.pages, s.at) == (page, pages, at) && s.since.is_some());
        if unchanged {
            return;
        }
        self.finish_page(env);
        let Some(session) = &mut self.session else {
            return;
        };
        // The reader has counted the pages (or counted anew).
        let counted = session
            .book
            .filter(|_| pages != session.pages && pages != 1000);
        session.page = page;
        session.pages = pages;
        session.at = at;
        session.since = Some(env.now());
        if let Some(id) = counted {
            let result = store::update_books(env.storage(), |books| {
                match books.iter_mut().find(|b| b.id == id) {
                    Some(book) => {
                        book.pages = pages;
                        true
                    }
                    None => false,
                }
            });
            self.wrote(result);
        }
    }

    /// A book is shown: the same one again (after a menu), or another.
    fn open(&mut self, env: &Env<'_>, reading: &events::Reading) {
        if self
            .session
            .as_ref()
            .is_some_and(|s| *s.path == *reading.path)
        {
            return self.turn(env, reading);
        }
        self.finish_page(env);
        let (page, pages) = page_of(reading);
        self.session = Some(Session {
            book: None,
            path: Rc::from(reading.path.as_str()),
            page,
            pages,
            at: reading.at(),
            since: Some(env.now()),
        });
        self.date(env);
    }

    /// The book at `path` was closed: its page ends, and nothing is read
    /// until a book is shown again.
    fn close(&mut self, env: &Env<'_>, path: &str) {
        if self.session.as_ref().is_some_and(|s| *s.path == *path) {
            self.finish_page(env);
            self.session = None;
        }
    }
}

/// The book at `path` in `books.ini`, added if new: recognised by its
/// fingerprint, with title and authors as KOReader takes them. `None`
/// where `books.ini` cannot be read or written: it then stays as it was,
/// and nothing is recorded for the book.
fn register_book(env: &Env<'_>, path: &str, now: i64, pages: u32) -> Option<u16> {
    let mut digits = [0u8; 32];
    let md5 = md5::partial_into(env.storage(), path, &mut digits)?;
    let mut id = None;
    store::update_books(env.storage(), |books| {
        let at = match books.iter().position(|b| b.md5 == md5) {
            Some(at) => at,
            None => {
                let Ok(new) = store::new_id(env.storage(), books) else {
                    return false;
                };
                let Ok(md5) = room::string(&[md5]) else {
                    return false;
                };
                let document = env.formats().open(env.storage(), path).ok();
                let metadata = document.as_ref().map(|d| d.metadata().clone());
                let file = path.rsplit('/').next().unwrap_or(path);
                let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
                let (title, authors, language) = match metadata {
                    Some(m) => (
                        m.title.unwrap_or_else(|| stem.to_string()),
                        m.authors.join("\n"),
                        m.language.unwrap_or_default(),
                    ),
                    None => (stem.to_string(), String::new(), String::new()),
                };
                let na = |text: String| {
                    if text.is_empty() {
                        String::from("N/A")
                    } else {
                        text
                    }
                };
                books.push(Book {
                    id: new,
                    title,
                    authors: na(authors),
                    md5,
                    pages: 0,
                    language: na(language),
                    series: String::from("N/A"),
                    last_open: 0,
                    path: String::new(),
                    finished: 0,
                });
                books.len() - 1
            }
        };
        let book = &mut books[at];
        book.last_open = now;
        book.path = path.to_string();
        // Counted pages, not the per-mille stand-in.
        if pages != 1000 || book.pages == 0 {
            book.pages = pages;
        }
        id = Some(book.id);
        true
    })
    .ok()?;
    id
}

/// The book at `path` was read to the end `now`: kept as when it was
/// finished, unless it was finished earlier the same year (its last page
/// reached again), so that a book read again in a later year counts then
/// too. Where `books.ini` cannot be read or written, it stays as it was.
fn mark_finished(env: &Env<'_>, path: &str, now: i64) {
    let year = |unix: i64| fmt::date_of(fmt::day_of(env, unix.max(0) as u32)).year;
    #[expect(
        clippy::let_underscore_must_use,
        reason = "books.ini that cannot be read or written stays as it was, as said above"
    )]
    let _ = store::update_books(env.storage(), |books| {
        match books.iter_mut().find(|b| b.path == path) {
            Some(book) if book.finished <= 0 || year(book.finished) != year(now) => {
                book.finished = now;
                true
            }
            _ => false,
        }
    });
}

/// The settings kept in the program (see [`SettingSpec::fixed`]).
static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::switch("stats.enabled", "stats.setting.enabled", true, GROUP)
        .description("stats.setting.enabled.help"),
    SettingSpec::number(
        MIN_SECONDS.static_str(),
        "stats.setting.min",
        1,
        60,
        5,
        GROUP,
    )
    .with_extra(&SECONDS)
    .advanced()
    .description("stats.setting.min.help"),
    SettingSpec::number(
        MAX_SECONDS.static_str(),
        "stats.setting.max",
        30,
        600,
        120,
        GROUP,
    )
    .with_extra(&SECONDS_10)
    .advanced()
    .description("stats.setting.max.help"),
];

/// Seconds, in steps of one and of ten.
static SECONDS: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().unit("stats.unit.seconds");
static SECONDS_10: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .unit("stats.unit.seconds")
    .step(10);

/// The statistics plugin.
#[derive(Debug, Default)]
pub struct Stats;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ENTRIES: [MenuEntry; 2] = [
    MenuEntry::new(menus::TOOLS, OPEN).order(35),
    MenuEntry::new(menus::READER_MORE, OPEN).order(30),
];
static ICONS: [(Id, &str); 2] = [(GROUP, "chart-bar"), (OPEN, "chart-bar")];

static SCREENS: [FixedScreen; 4] = [
    FixedScreen::new(OVERVIEW, "stats.title", |env, params| {
        Box::new(overview::Overview::new(env, params))
    }),
    FixedScreen::new(CALENDAR, "stats.calendar", |env, _| {
        Box::new(calendar::Calendar::new(env))
    }),
    FixedScreen::new(BOOKS, "stats.books", |env, _| {
        Box::new(books::BookList::new(env))
    }),
    FixedScreen::new(BOOK, "stats.title", |env, params| {
        Box::new(book::BookView::new(env, params))
    }),
];

impl Plugin for Stats {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Reading statistics",
            paperpaw_sdk::package_version!(),
        )
        .description(
            "Records how long and how much you read, with a calendar and every book's numbers.",
        )
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::new(RefCell::new(State::default()));
        widget::register(r)?;
        left::register(r)?;
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "stats.group").order(25);
            &SPEC
        })?;
        r.add_icons(&ICONS)?;
        r.add_settings(&SETTINGS)?;

        // Recording.
        let s = state.clone();
        r.on(events::BOOK_OPENED, move |env, reading, out| {
            let mut state = s.borrow_mut();
            if enabled(env) {
                state.open(env, reading);
            }
            state.tell(env, out);
        })?;
        let s = state.clone();
        r.on(events::PAGE_TURNED, move |env, reading, out| {
            let mut state = s.borrow_mut();
            let same = state
                .session
                .as_ref()
                .is_some_and(|session| *session.path == *reading.path);
            if same && enabled(env) {
                state.turn(env, reading);
            }
            state.tell(env, out);
        })?;
        let s = state.clone();
        r.on(events::BOOK_CLOSED, move |env, reading, out| {
            let mut state = s.borrow_mut();
            state.close(env, &reading.path);
            state.tell(env, out);
        })?;
        let s = state.clone();
        r.on(events::BOOK_MOVED, move |env, moved, out| {
            let result = store::update_books(env.storage(), |books| {
                let mut changed = false;
                for book in books.iter_mut().filter(|b| b.path == moved.from) {
                    book.path = moved.path.clone();
                    changed = true;
                }
                changed
            });
            let mut state = s.borrow_mut();
            state.wrote(result);
            state.tell(env, out);
        })?;
        r.on(events::BOOK_FINISHED, |env, reading, _| {
            if let Some(now) = time::unix_seconds(env).filter(|_| enabled(env)) {
                mark_finished(env, &reading.path, now);
            }
        })?;
        for event in [events::SUSPEND, events::STOPPING] {
            let s = state.clone();
            r.on(event, move |env, _, _| {
                s.borrow_mut().finish_page(env);
            })?;
        }
        // Files a cut left beside their place are put there before
        // anything is recorded; today's reading is told at the start, for
        // those who show it.
        let s = state.clone();
        r.on(events::STARTED, move |env, _, out| {
            store::recover(env.storage());
            s.borrow_mut().tell(env, out)
        })?;
        // Only a book still open goes on being read.
        let s = state.clone();
        r.on(events::RESUME, move |env, _, _| {
            if let Some(session) = &mut s.borrow_mut().session {
                session.since.get_or_insert(env.now());
            }
        })?;
        // The time became known (or was set anew): what was read before
        // is recorded.
        let s = state.clone();
        r.on(events::STATUS_CHANGED, move |env, _, out| {
            let mut state = s.borrow_mut();
            state.date(env);
            state.tell(env, out);
        })?;

        // Sharing with KOReader.
        sync::register(r, Rc::new(RefCell::new(sync::Sync::default())))?;

        // The views.
        r.add_screens(&SCREENS)?;
        let s = state;
        r.add_action(ActionSpec::new(OPEN, "stats.title").handler(
            move |env: &Env<'_>, _, out: &mut Commands| {
                // Recorded so far, so the views include the current page.
                let mut state = s.borrow_mut();
                let reading = state.session.as_ref().is_some_and(|s| s.since.is_some());
                state.finish_page(env);
                if let Some(session) = &mut state.session
                    && reading
                {
                    session.since = Some(env.now());
                }
                let current = state.session.as_ref().and_then(|session| session.book);
                let params = match current {
                    Some(book) => Params::new().with(params::ID, alloc::format!("{book}")),
                    None => Params::new(),
                };
                out.push(Command::OpenScreen(OVERVIEW, params));
            },
        ))?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

fn enabled(env: &Env<'_>) -> bool {
    env.settings().read(&ENABLED)
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    use super::*;

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn stays_count_as_koreader_counts_them() {
        assert_eq!(counted(3, 5, 120), 0);
        assert_eq!(counted(5, 5, 120), 5);
        assert_eq!(counted(45, 5, 120), 45);
        assert_eq!(counted(3600, 5, 120), 120);
    }

    #[test]
    fn pages_come_from_the_reader_or_the_progress() {
        let counted = paperpaw_sdk::builtin::events::Reading::new(
            "books/a.epub".into(),
            String::new(),
            4.0,
            3,
            120,
            String::new(),
        )
        .page(12)
        .pages(300);
        assert_eq!(page_of(&counted), (12, 300));
        let estimated = {
            let mut reading = counted;
            reading.page = None;
            reading.pages = None;
            reading.progress = 42.5;
            reading
        };
        assert_eq!(page_of(&estimated), (426, 1000));
    }
}
