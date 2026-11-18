//! Reading the sources, one request at a time, with no host: what is asked of
//! the network and the card goes through [`Wire`], so the same code runs
//! against the device and against a server in a test.
//!
//! A run reads each source in turn. A CalDAV or CardDAV source is looked up
//! (`PROPFIND`, following the account's own address to where its calendars
//! are, three steps at most) and each calendar or address book is read with
//! one `REPORT` for the window of days that is kept, saved to a file and read
//! as it streams: never in memory. A subscription is one `GET` saved to a
//! file; a calendar file of the card is read in place. Whatever a source gave
//! before is replaced only once its answer is there, so a source that fails
//! keeps what the reader knew.
//!
//! Nothing is sent: this plugin reads calendars, it does not change them.
use alloc::{string::String, vec::Vec};

use paperpaw_caldav::{
    date::days_from_civil,
    dav::{self, Item, Multistatus, Visit},
    event::{Event, EventReader},
    ical::Lines,
    rrule::Rule,
    vcard::{Card, CardReader, Day},
    xml::Scanner,
};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    files::Volume,
    mem::{buffer, push},
    text::{concat, copy},
};

use crate::keep;
use crate::model::{
    Entry, Kind, Mark, NO_TIME, PLACE_MAX, SKIP_MAX, Store, TITLE_MAX, WINDOW_DAYS, clean, hash,
    state,
};

/// The file an answer with many parts is saved to and read from, and the one
/// a subscription is saved to.
pub const ANSWER: &str = "sync.xml";
pub const FEED: &str = "feed.ics";
/// The most bytes of an answer.
pub const ANSWER_MAX: u64 = 2 * 1024 * 1024;
/// The most calendars read from one account.
const CALENDARS_MAX: usize = 8;
/// How much of a file is read at a time.
const CHUNK: usize = 2048;

/// An error of the reader or of the shared CalDAV code, as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused(pub Error);

impl From<Error> for Refused {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl From<paperpaw_caldav::Error> for Refused {
    fn from(error: paperpaw_caldav::Error) -> Self {
        Self(match error {
            paperpaw_caldav::Error::NoMemory => Error::NoMemory,
            _ => Error::Invalid,
        })
    }
}

fn dav<T>(result: Result<T, paperpaw_caldav::Error>) -> Result<T, Error> {
    result.map_err(|error| Refused::from(error).0)
}

/// A request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call<'a> {
    pub method: &'a str,
    pub url: &'a str,
    /// `Depth: 1`.
    pub deep: bool,
    /// The body's `Content-Type`.
    pub content_type: &'a str,
    pub body: Option<&'a str>,
    /// The file of the data folder the answer is saved to, and the most
    /// bytes of it.
    pub save_to: (&'a str, u64),
    /// The source it is for: its name and password go with it.
    pub source: u8,
}

/// What the engine asks of the device.
pub trait Wire {
    /// The plugin's data folder.
    fn files(&self) -> &dyn Volume;
    /// The user's files on the card.
    fn card(&self) -> &dyn Volume;
    /// Starts a request, with who asks; its number, which its end is told
    /// with.
    ///
    /// # Errors
    /// What the host says.
    fn send(&mut self, call: &Call<'_>, store: &Store) -> Result<u32, Error>;
    /// Seconds since 1970, if the clock is set.
    fn now(&mut self) -> Option<i64>;
    /// How far the reader's clock is east of UTC at `unix`, in seconds.
    fn zone(&mut self, unix: i64) -> i32;
}

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Status(u32),
    /// There is no network.
    Offline,
    /// It failed otherwise (a name not found, a certificate, a full card).
    Failed,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Outcome {
    #[default]
    Synced,
    /// There is no source (that a run starting by itself may read).
    NoSource,
    /// No network.
    Offline,
    /// The clock is not set: which days to keep is not known.
    NoClock,
    /// The run stopped on an error (the card, no memory).
    Failed,
}

/// What a run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    pub outcome: Outcome,
    /// Sources read, and sources that were not (refused, not found).
    pub read: u8,
    pub failed: u8,
    pub denied: u8,
    /// Events not kept: more than the reader holds.
    pub skipped: u16,
}

/// What an engine call came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Waiting,
    Ended(Report),
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Idle,
    /// The calendars are being looked up at `url`.
    Discover { url: String, hops: u8 },
    /// The calendar `cal` of the found ones is being read.
    Pull { cal: usize },
    /// A subscription is being fetched.
    Feed,
}

/// How far the reader's clock is from UTC over the window: one change at
/// most (the clocks going forward or back), found by halving.
#[derive(Debug, Clone, Copy)]
pub struct Zoner {
    change: i64,
    before: i32,
    after: i32,
}

impl Zoner {
    /// A zone that does not change.
    #[must_use]
    pub const fn fixed(offset: i32) -> Self {
        Self {
            change: i64::MAX,
            before: offset,
            after: offset,
        }
    }

    fn probe(wire: &mut dyn Wire, from: i64, to: i64) -> Self {
        let (before, after) = (wire.zone(from), wire.zone(to));
        if before == after {
            return Self::fixed(before);
        }
        let (mut low, mut high) = (from, to);
        while high - low > 3600 {
            let mid = low + (high - low) / 2;
            if wire.zone(mid) == before {
                low = mid;
            } else {
                high = mid;
            }
        }
        Self {
            change: high,
            before,
            after,
        }
    }

    /// Seconds east of UTC at `utc`.
    #[must_use]
    pub const fn at(&self, utc: i64) -> i32 {
        if utc < self.change {
            self.before
        } else {
            self.after
        }
    }
}

/// The engine.
#[derive(Debug, Default)]
pub struct Sync {
    state: State,
    request: u32,
    /// The sources still to read.
    queue: Vec<u8>,
    /// The source being read, the address its calendars were found at and
    /// where each is.
    src: u8,
    base: String,
    found: Vec<String>,
    report: Report,
    /// The days kept: from, to, and today.
    window: (i32, i32, i32),
    zoner: Option<Zoner>,
    /// This run began by itself: a source that refused its name is left.
    auto: bool,
    /// Something changed since this was asked last.
    dirty: bool,
    /// Whether anything of the source was read in this run, and whether some
    /// of it was not.
    got: bool,
    partly: bool,
}

/// A calendar or address book an answer names, and where to look instead.
#[derive(Debug, Default)]
struct Found {
    book: bool,
    hrefs: Vec<String>,
    home: String,
    principal: String,
}

impl Visit for Found {
    type Error = Refused;

    fn data(&mut self, _: &[u8]) -> Result<(), Refused> {
        Ok(())
    }

    fn item(&mut self, item: &Item<'_>) -> Result<(), Refused> {
        if item.cut {
            return Ok(());
        }
        if self.home.is_empty() {
            self.home = copy(item.home)?;
        }
        if self.principal.is_empty() {
            self.principal = copy(item.principal)?;
        }
        let href = dav::path(item.href);
        let wanted = if self.book {
            item.addressbook
        } else {
            item.calendar && item.events
        };
        if wanted && !href.is_empty() && self.hrefs.len() < CALENDARS_MAX {
            push(&mut self.hrefs, copy(href)?)?;
        }
        Ok(())
    }
}

/// What a read of events or contacts adds to the store.
struct Collect<'a> {
    store: &'a mut Store,
    src: u8,
    zoner: Zoner,
    lines: Lines,
    events: EventReader,
    cards: CardReader,
    /// The days kept: from, to, and today.
    window: (i32, i32, i32),
    skipped: u16,
}

impl<'a> Collect<'a> {
    fn new(store: &'a mut Store, src: u8, zoner: Zoner, window: (i32, i32, i32)) -> Self {
        Self {
            store,
            src,
            zoner,
            window,
            lines: Lines::new(),
            events: EventReader::new(TITLE_MAX.max(PLACE_MAX)),
            cards: CardReader::new(TITLE_MAX),
            skipped: 0,
        }
    }

    fn line(&mut self, line: &[u8]) -> Result<(), Refused> {
        let event = self.events.line(line)?;
        let card = self.cards.line(line)?;
        if let Some(event) = event {
            self.event(&event)?;
        }
        if let Some(card) = card {
            self.card(&card)?;
        }
        Ok(())
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<(), Refused> {
        let mut lines = core::mem::take(&mut self.lines);
        let result = lines.feed(bytes, &mut |line: &[u8]| self.line(line));
        self.lines = lines;
        result
    }

    fn end(&mut self) -> Result<(), Refused> {
        let mut lines = core::mem::take(&mut self.lines);
        let result = lines.finish(&mut |line: &[u8]| self.line(line));
        self.lines = lines;
        self.events.reset();
        self.cards.reset();
        result
    }

    fn add(&mut self, entry: Entry) -> Result<(), Error> {
        match self.store.add(entry) {
            Err(Error::Full) => {
                self.skipped = self.skipped.saturating_add(1);
                Ok(())
            }
            other => other,
        }
    }

    /// An event of the calendar, kept as an entry.
    fn event(&mut self, event: &Event) -> Result<(), Error> {
        let Some(start) = event.start else {
            return Ok(());
        };
        let zoner = self.zoner;
        let mut offset = |utc: i64| zoner.at(utc);
        let (day, secs) = start.local(&mut offset);
        let span = match (secs, event.end) {
            (None, Some(end)) => {
                u32::try_from((end.at.day - start.at.day).clamp(1, 366)).unwrap_or(1)
            }
            (None, None) => event
                .duration
                .map_or(1, |secs| (secs / 86_400).clamp(1, 366)),
            (Some(secs), Some(end)) => match end.local(&mut offset) {
                (end_day, Some(end_secs)) => {
                    let total =
                        i64::from(end_day - day) * 86_400 + i64::from(end_secs) - i64::from(secs);
                    u32::try_from((total / 60).clamp(0, 60 * 24 * 31)).unwrap_or(0)
                }
                _ => 0,
            },
            (Some(_), None) => event
                .duration
                .map_or(0, |secs| (secs / 60).min(60 * 24 * 31)),
        };
        let rule = if event.rule.is_empty() || Rule::parse(&event.rule).is_none() {
            String::new()
        } else {
            copy(&event.rule)?
        };
        let mut skip = Vec::new();
        for when in event.exceptions.iter().take(SKIP_MAX) {
            push(&mut skip, when.local(&mut offset).0)?;
        }
        let over = event.instead_of.map(|when| when.local(&mut offset).0);
        let mark = if event.cancelled {
            if over.is_none() {
                return Ok(());
            }
            Mark::Cancelled
        } else {
            Mark::Event
        };
        let entry = Entry {
            src: self.src,
            mark,
            day,
            secs: secs.unwrap_or(NO_TIME),
            span,
            known: false,
            uid: hash(&event.uid),
            over,
            rule,
            skip,
            title: clean(&event.summary, TITLE_MAX)?,
            place: clean(&event.location, PLACE_MAX)?,
        };
        if entry.over.is_none() && !entry.falls_in(self.window.0, self.window.1) {
            return Ok(());
        }
        self.add(entry)
    }

    /// A contact with a birthday or an anniversary: an entry for each.
    fn card(&mut self, card: &Card) -> Result<(), Error> {
        for (mark, day) in [
            (Mark::Birthday, card.birthday),
            (Mark::Anniversary, card.anniversary),
        ] {
            let Some(Day { year, month, day }) = day else {
                continue;
            };
            let counted = if year == 0 { 2000 } else { year };
            if day > paperpaw_caldav::date::days_in_month(counted, month) {
                continue;
            }
            let entry = Entry {
                src: self.src,
                mark,
                day: i32::try_from(days_from_civil(counted, month, day)).unwrap_or(0),
                secs: NO_TIME,
                span: 1,
                known: year != 0,
                uid: hash(&card.uid),
                over: None,
                rule: String::new(),
                skip: Vec::new(),
                title: clean(&card.name, TITLE_MAX)?,
                place: String::new(),
            };
            self.add(entry)?;
        }
        Ok(())
    }
}

impl Visit for Collect<'_> {
    type Error = Refused;

    fn data(&mut self, bytes: &[u8]) -> Result<(), Refused> {
        self.feed(bytes)
    }

    fn item(&mut self, _: &Item<'_>) -> Result<(), Refused> {
        self.end()
    }
}

/// Reads the saved answer through `visit`; whether it was read to its end.
fn read_answer<V: Visit<Error = Refused>>(
    files: &dyn Volume,
    visit: V,
) -> Result<(V, bool), Error> {
    let mut chunk = buffer(CHUNK)?;
    let mut scanner = Scanner::new();
    let mut reader = Multistatus::new(visit);
    let mut at = 0u64;
    loop {
        let n = files.read_at(ANSWER, at, &mut chunk)?;
        let Some(part) = chunk.get(..n).filter(|part| !part.is_empty()) else {
            break;
        };
        scanner
            .feed(part, &mut reader)
            .map_err(|refused| refused.0)?;
        at += n as u64;
    }
    let complete = reader.complete();
    Ok((reader.visit, complete))
}

/// Reads a calendar text from the file `name` of `volume` into `collect`.
fn read_text(volume: &dyn Volume, name: &str, collect: &mut Collect<'_>) -> Result<(), Error> {
    let mut chunk = buffer(CHUNK)?;
    let mut at = 0u64;
    loop {
        let n = volume.read_at(name, at, &mut chunk)?;
        let Some(part) = chunk.get(..n).filter(|part| !part.is_empty()) else {
            break;
        };
        collect.feed(part).map_err(|refused| refused.0)?;
        at += n as u64;
    }
    collect.end().map_err(|refused| refused.0)
}

/// The address a subscription or a server is read at: `webcal://` is `https://`.
#[must_use]
pub fn web_address(typed: &str) -> String {
    let typed = typed.trim();
    let typed = typed
        .strip_prefix("webcals://")
        .or_else(|| typed.strip_prefix("webcal://"))
        .map_or_else(
            || String::from(typed),
            |rest| {
                let mut text = String::from("https://");
                text.push_str(rest);
                text
            },
        );
    if typed.contains("://") || typed.is_empty() {
        typed
    } else {
        let mut text = String::from("https://");
        text.push_str(&typed);
        text
    }
}

impl Sync {
    /// Whether a run is under way.
    #[must_use]
    pub fn running(&self) -> bool {
        !matches!(self.state, State::Idle) || !self.queue.is_empty()
    }

    /// Whether the events or sources changed since this was asked last.
    pub fn take_dirty(&mut self) -> bool {
        core::mem::take(&mut self.dirty)
    }

    fn end(&mut self, outcome: Outcome) -> Step {
        self.state = State::Idle;
        self.queue = Vec::new();
        self.found = Vec::new();
        self.base = String::new();
        self.report.outcome = outcome;
        Step::Ended(self.report)
    }

    fn settle(&mut self, step: Result<Step, Error>) -> Step {
        match step {
            Ok(step) => step,
            Err(_) => {
                self.report.failed = self.report.failed.saturating_add(1);
                self.end(Outcome::Failed)
            }
        }
    }

    /// Starts a run over `only` (one source) or every source. A run that
    /// `auto`matically starts leaves out a source that refused its name until
    /// it is changed.
    pub fn start(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        only: Option<u8>,
        auto: bool,
    ) -> Step {
        if self.running() {
            return Step::Waiting;
        }
        self.report = Report::default();
        self.auto = auto;
        let Some(now) = wire.now() else {
            return self.end(Outcome::NoClock);
        };
        let local = now + i64::from(wire.zone(now));
        let today = i32::try_from(local.div_euclid(86_400)).unwrap_or(0);
        self.window = (today - WINDOW_DAYS, today + WINDOW_DAYS, today);
        self.zoner = Some(Zoner::probe(
            wire,
            i64::from(self.window.0 - 1) * 86_400,
            i64::from(self.window.1 + 1) * 86_400,
        ));
        let ids: Vec<u8> = store
            .sources
            .iter()
            .filter(|source| only.is_none_or(|id| id == source.id))
            .filter(|source| !(auto && source.state == state::DENIED))
            .filter(|source| !source.url.is_empty())
            .map(|source| source.id)
            .collect();
        if ids.is_empty() {
            return self.end(Outcome::NoSource);
        }
        self.queue = ids;
        self.queue.reverse();
        let step = self.next_source(store, wire);
        self.settle(step)
    }

    fn zoner(&self) -> Zoner {
        self.zoner.unwrap_or(Zoner::fixed(0))
    }

    /// The source `src` is over: how it went is noted.
    fn finished(&mut self, store: &mut Store, wire: &mut dyn Wire, how: u8) -> Result<(), Error> {
        let now = wire.now().unwrap_or(0);
        if let Some(source) = store.source_mut(self.src) {
            source.state = how;
            if how == state::OK || how == state::PARTLY {
                source.synced = now;
            }
        }
        match how {
            state::OK | state::PARTLY => self.report.read = self.report.read.saturating_add(1),
            state::DENIED => self.report.denied = self.report.denied.saturating_add(1),
            _ => self.report.failed = self.report.failed.saturating_add(1),
        }
        self.state = State::Idle;
        self.found = Vec::new();
        self.dirty = true;
        keep::save(wire.files(), store)
    }

    /// The next source, or the end of the run.
    fn next_source(&mut self, store: &mut Store, wire: &mut dyn Wire) -> Result<Step, Error> {
        while let Some(id) = self.queue.pop() {
            let Some(source) = store.source(id) else {
                continue;
            };
            self.src = id;
            (self.got, self.partly) = (false, false);
            match source.kind {
                Kind::File => {
                    let path = copy(&source.url)?;
                    let how = self.read_file(store, wire, &path)?;
                    self.finished(store, wire, how)?;
                }
                Kind::Ics => {
                    let url = web_address(&source.url);
                    let call = Call {
                        method: "GET",
                        url: &url,
                        deep: false,
                        content_type: "",
                        body: None,
                        save_to: (FEED, ANSWER_MAX),
                        source: id,
                    };
                    self.request = wire.send(&call, store)?;
                    self.state = State::Feed;
                    return Ok(Step::Waiting);
                }
                Kind::Caldav | Kind::Contacts => {
                    let url = web_address(&source.url);
                    let url = if url.ends_with('/') {
                        url
                    } else {
                        concat(&[&url, "/"])?
                    };
                    return self.discover(store, wire, url, 0);
                }
            }
        }
        Ok(self.end(Outcome::Synced))
    }

    /// A calendar file of the card: replaces what the source gave.
    fn read_file(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        path: &str,
    ) -> Result<u8, Error> {
        match wire.card().size(path) {
            Ok(size) if size > 0 && size <= ANSWER_MAX => {}
            _ => return Ok(state::FAILED),
        }
        self.replace_from(store, wire.card(), path)
    }

    /// Reads a calendar text of `volume` as the source's events.
    fn replace_from(
        &mut self,
        store: &mut Store,
        volume: &dyn Volume,
        name: &str,
    ) -> Result<u8, Error> {
        store.entries.retain(|entry| entry.src != self.src);
        let mut collect = Collect::new(store, self.src, self.zoner(), self.window);
        let read = read_text(volume, name, &mut collect);
        let skipped = collect.skipped;
        self.report.skipped = self.report.skipped.saturating_add(skipped);
        read?;
        store.finish_source(self.src, self.window.0, self.window.1);
        Ok(if skipped > 0 {
            state::PARTLY
        } else {
            state::OK
        })
    }

    fn discover(
        &mut self,
        store: &Store,
        wire: &mut dyn Wire,
        url: String,
        hops: u8,
    ) -> Result<Step, Error> {
        let book = store
            .source(self.src)
            .is_some_and(|s| s.kind == Kind::Contacts);
        let call = Call {
            method: "PROPFIND",
            url: &url,
            deep: true,
            content_type: dav::XML,
            body: Some(if book {
                dav::PROPFIND_BOOKS
            } else {
                dav::PROPFIND_CALENDARS
            }),
            save_to: (ANSWER, ANSWER_MAX),
            source: self.src,
        };
        self.request = wire.send(&call, store)?;
        self.state = State::Discover { url, hops };
        Ok(Step::Waiting)
    }

    /// The request `id` ended.
    pub fn done(&mut self, store: &mut Store, wire: &mut dyn Wire, id: u32, ended: Ended) -> Step {
        if !self.running() || id != self.request {
            return Step::Waiting;
        }
        let state = core::mem::take(&mut self.state);
        let status = match ended {
            Ended::Status(status) => status,
            Ended::Offline => return self.end(Outcome::Offline),
            Ended::Failed => 0,
        };
        let step = (|| {
            if status == 0 || matches!(status, 401 | 403) {
                let how = if status == 0 {
                    state::FAILED
                } else {
                    state::DENIED
                };
                self.finished(store, wire, how)?;
                return self.next_source(store, wire);
            }
            match state {
                State::Idle => Ok(Step::Waiting),
                State::Feed => self.fed(store, wire, status),
                State::Discover { url, hops } => self.discovered(store, wire, url, hops, status),
                State::Pull { cal } => self.pulled(store, wire, cal, status),
            }
        })();
        self.settle(step)
    }

    fn fed(&mut self, store: &mut Store, wire: &mut dyn Wire, status: u32) -> Result<Step, Error> {
        let how = if status == 200 {
            self.replace_from(store, wire.files(), FEED)?
        } else {
            state::FAILED
        };
        self.finished(store, wire, how)?;
        self.next_source(store, wire)
    }

    fn discovered(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        url: String,
        hops: u8,
        status: u32,
    ) -> Result<Step, Error> {
        let book = store
            .source(self.src)
            .is_some_and(|s| s.kind == Kind::Contacts);
        let found = if status == 207 {
            let (found, complete) = read_answer(
                wire.files(),
                Found {
                    book,
                    ..Found::default()
                },
            )?;
            if !complete {
                self.finished(store, wire, state::FAILED)?;
                return self.next_source(store, wire);
            }
            found
        } else {
            self.finished(store, wire, state::FAILED)?;
            return self.next_source(store, wire);
        };
        if found.hrefs.is_empty() {
            // Not where the calendars are: the server may say where they are.
            let next = [&found.home, &found.principal]
                .into_iter()
                .find(|href| !href.is_empty() && !dav::same_path(href, &url));
            if let Some(href) = next.filter(|_| hops < 3) {
                let next = dav(dav::resolve(&url, href))?;
                return self.discover(store, wire, next, hops + 1);
            }
            self.finished(store, wire, state::FAILED)?;
            return self.next_source(store, wire);
        }
        self.base = url;
        self.found = found.hrefs;
        self.pull(store, wire, 0)
    }

    /// Reads calendar `cal` of the found ones, or ends the source.
    fn pull(&mut self, store: &mut Store, wire: &mut dyn Wire, cal: usize) -> Result<Step, Error> {
        let Some(href) = self.found.get(cal) else {
            let how = match (self.got, self.partly) {
                (false, _) => state::FAILED,
                (true, true) => state::PARTLY,
                (true, false) => state::OK,
            };
            store.finish_source(self.src, self.window.0, self.window.1);
            self.finished(store, wire, how)?;
            return self.next_source(store, wire);
        };
        let url = dav(dav::resolve(&self.base, href))?;
        let book = store
            .source(self.src)
            .is_some_and(|s| s.kind == Kind::Contacts);
        let body = if book {
            None
        } else {
            Some(dav(dav::report_events(self.window.0, self.window.1))?)
        };
        let call = Call {
            method: "REPORT",
            url: &url,
            deep: true,
            content_type: dav::XML,
            body: Some(body.as_deref().unwrap_or(dav::REPORT_CONTACTS)),
            save_to: (ANSWER, ANSWER_MAX),
            source: self.src,
        };
        self.request = wire.send(&call, store)?;
        self.state = State::Pull { cal };
        Ok(Step::Waiting)
    }

    fn pulled(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        cal: usize,
        status: u32,
    ) -> Result<Step, Error> {
        if status != 207 {
            self.partly = true;
        } else {
            if !self.got {
                // The first answer that is there takes the place of what the
                // source gave before.
                store.entries.retain(|entry| entry.src != self.src);
                self.got = true;
            }
            let collect = Collect::new(store, self.src, self.zoner(), self.window);
            let (collect, complete) = read_answer(wire.files(), collect)?;
            self.report.skipped = self.report.skipped.saturating_add(collect.skipped);
            self.partly |= !complete || collect.skipped > 0;
        }
        self.pull(store, wire, cal + 1)
    }
}
