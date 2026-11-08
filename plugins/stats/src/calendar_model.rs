//! The calendar's contents: a month and how long one read on each of its
//! days, the chosen day's date, time, pages and books, and how the keys and
//! taps move the chosen day. Host-free: the shell lays the grid out and draws.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;

use crate::room;
use crate::store::{self, Book, Day};
use crate::texts::{self, Words, month_start};
use crate::totals;

/// Most books listed for a day.
pub(crate) const DAY_BOOKS: usize = 3;

/// The last day the calendar shows: 9999-12-31. A day beyond (or before the
/// Unix epoch) comes from a hostile or broken cache and is not a day read.
pub(crate) const MAX_DAY: i32 = 2_932_896;

/// `day` held to the days the calendar shows (1970-01-01 to 9999-12-31).
pub(crate) fn clamp_day(day: i32) -> i32 {
    day.clamp(0, MAX_DAY)
}

/// Year and month of a day.
pub(crate) fn month_of(day: i32) -> (i32, u8) {
    let d = texts::date_of(day);
    (d.year, d.month)
}

/// The first day of the month after the one of `day`.
pub(crate) fn next_month(day: i32) -> i32 {
    // At most 31 days on, then back to that month's start.
    month_start(clamp_day(month_start(clamp_day(day)).saturating_add(31)))
}

/// The first day of the month before the one of `day`.
pub(crate) fn previous_month(day: i32) -> i32 {
    month_start(clamp_day(month_start(clamp_day(day)).saturating_sub(1)))
}

/// How the chosen day moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    DayBack,
    DayForward,
    WeekBack,
    WeekForward,
    MonthBack,
    MonthForward,
}

/// The day `step` leads to from `day`.
pub(crate) fn step(day: i32, step: Step) -> i32 {
    match step {
        Step::DayBack => clamp_day(day.saturating_sub(1)),
        Step::DayForward => clamp_day(day.saturating_add(1)),
        Step::WeekBack => clamp_day(day.saturating_sub(7)),
        Step::WeekForward => clamp_day(day.saturating_add(7)),
        Step::MonthBack => previous_month(day),
        Step::MonthForward => next_month(day),
    }
}

/// A key that moves the chosen day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    Left,
    Previous,
    Right,
    Next,
    Up,
    Down,
    PreviousPage,
    NextPage,
}

/// What a key does, as in interface 6: left and previous a day back, right
/// and next a day on, up and down a week, the page keys a month.
pub(crate) const fn key_step(key: Key) -> Step {
    match key {
        Key::Left | Key::Previous => Step::DayBack,
        Key::Right | Key::Next => Step::DayForward,
        Key::Up => Step::WeekBack,
        Key::Down => Step::WeekForward,
        Key::PreviousPage => Step::MonthBack,
        Key::NextPage => Step::MonthForward,
    }
}

/// A swipe across the calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Swipe {
    Left,
    Right,
}

/// A swipe turns the month like a page: left to the next, right to the
/// previous.
pub(crate) const fn swipe_step(swipe: Swipe) -> Step {
    match swipe {
        Swipe::Left => Step::MonthForward,
        Swipe::Right => Step::MonthBack,
    }
}

/// The day a tap on `tapped` chooses: one of the month shown; a day of the
/// weeks around it (another month) is ignored.
pub(crate) fn tap_day(chosen: i32, tapped: i32) -> Option<i32> {
    (month_of(tapped) == month_of(chosen)).then_some(tapped)
}

/// The first day shown: the start of the week the month of `day` begins in;
/// `week_start` is the first day of the week, 0 being Sunday.
pub(crate) fn first_shown(day: i32, week_start: u8) -> i32 {
    let start = month_start(clamp_day(day));
    let weekday = i32::from(texts::date_of(start).weekday);
    start.saturating_sub((weekday - i32::from(week_start)).rem_euclid(7))
}

/// The gray a day with `minutes` of reading is shaded in: darker the longer;
/// none where nothing was read.
pub(crate) fn shade(minutes: u32) -> Option<u8> {
    match minutes {
        0 => None,
        1..=14 => Some(220),
        15..=29 => Some(180),
        30..=59 => Some(120),
        _ => Some(40),
    }
}

/// The first day of the week the setting `core.week-start` names: 0 (Sunday)
/// or 1 (Monday); none where it is left to the language's habit.
pub(crate) fn week_start_set(setting: &str) -> Option<u8> {
    match setting {
        "core.week.sunday" => Some(0),
        "core.week.monday" => Some(1),
        _ => None,
    }
}

/// The first day of the week the translation `core.week-start-day` (`0` for
/// Sunday, `1` for Monday) says.
pub(crate) fn week_start_habit(habit: &str) -> u8 {
    u8::from(habit != "0")
}

/// A month's name shortened to its first three letters and a dot; one of
/// four letters or fewer as it is.
pub(crate) fn short_month(name: &str) -> Result<String, Error> {
    let mut out = String::new();
    if name.chars().count() <= 4 {
        out.try_reserve_exact(name.len())
            .map_err(|_| Error::NoMemory)?;
        out.push_str(name);
        return Ok(out);
    }
    let end = name.char_indices().nth(3).map_or(name.len(), |(at, _)| at);
    out.try_reserve_exact(end + 1)
        .map_err(|_| Error::NoMemory)?;
    out.push_str(name.get(..end).unwrap_or_default());
    out.push('.');
    Ok(out)
}

/// The month's name over the grid, whole and shortened: `October 2026` and
/// `Oct. 2026`.
pub(crate) fn title(words: &mut impl Words, day: i32) -> Result<[String; 2], Error> {
    let (year, month) = month_of(day);
    let name = texts::month(words, month)?;
    let short = short_month(&name)?;
    let year = YearText::new(year);
    Ok([
        room::string(&[&name, " ", year.as_str()]).map_err(|_| Error::NoMemory)?,
        room::string(&[&short, " ", year.as_str()]).map_err(|_| Error::NoMemory)?,
    ])
}

/// A year as text, made without allocating.
struct YearText {
    digits: [u8; 11],
    from: usize,
}

impl YearText {
    fn new(year: i32) -> Self {
        let mut digits = [0u8; 11];
        let mut at = digits.len();
        let mut rest = year.unsigned_abs();
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        if year < 0 {
            at -= 1;
            digits[at] = b'-';
        }
        Self { digits, from: at }
    }

    fn as_str(&self) -> &str {
        self.digits
            .get(self.from..)
            .and_then(|d| core::str::from_utf8(d).ok())
            .unwrap_or_default()
    }
}

/// The note by the title: the streak where it is more than a day.
pub(crate) fn streak_note(
    words: &mut impl Words,
    days: &[Day],
    today: Option<i32>,
) -> Result<Option<String>, Error> {
    let streak = today.map_or(0, |today| totals::streak(days, today));
    if streak <= 1 {
        return Ok(None);
    }
    words
        .format("stats.streak-note", &[Arg::Count(i64::from(streak))])
        .map(Some)
}

/// What was read on one day.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Detail {
    pub seconds: u32,
    /// Distinct pages.
    pub pages: u32,
    /// The books read longest first: title and seconds, at most
    /// [`DAY_BOOKS`].
    pub books: Vec<(String, u32)>,
}

/// What the calendar needs of a book to name it.
pub(crate) trait Titled {
    fn id(&self) -> u16;
    fn title(&self) -> &str;
}

impl Titled for Book {
    fn id(&self) -> u16 {
        self.id
    }
    fn title(&self) -> &str {
        &self.title
    }
}

/// A book's id and its title cut to what a line shows: all the calendar
/// keeps of the books (not their authors, paths or hashes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Named {
    pub id: u16,
    pub title: String,
}

impl Titled for Named {
    fn id(&self) -> u16 {
        self.id
    }
    fn title(&self) -> &str {
        &self.title
    }
}

/// The names of `books`, each title cut ([`crate::book_list::MAX_TEXT`]).
pub(crate) fn names(books: &[Book]) -> Result<Vec<Named>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(books.len())
        .map_err(|_| Error::NoMemory)?;
    for book in books {
        out.push(Named {
            id: book.id,
            title: crate::book_list::cut(&book.title)?,
        });
    }
    Ok(out)
}

/// What was read on local day `day`; `day_of` turns Unix seconds into local
/// days. A book of the log that has no entry is `?`.
pub(crate) fn detail(
    storage: &dyn Storage,
    books: &[impl Titled],
    days: &[Day],
    day: i32,
    day_of: impl Fn(u32) -> i32,
) -> Result<Detail, Error> {
    let seconds = texts::seconds_on(days, day);
    if seconds == 0 {
        return Ok(Detail::default());
    }
    // The local day, whatever the time zone.
    let unix = |day: i32| (i64::from(day) * 86_400).clamp(0, i64::from(u32::MAX)) as u32;
    let from = unix(day).saturating_sub(14 * 3600);
    let to = unix(day.saturating_add(1)).saturating_add(14 * 3600);
    // (book, seconds, first seen at)
    let mut per_book: Vec<(u16, u32, usize)> = Vec::new();
    let mut pages: Vec<(u16, u32)> = Vec::new();
    let mut refused = false;
    store::scan_range(storage, from, to, |stat| {
        if refused || day_of(stat.start) != day {
            return;
        }
        let seen = per_book.len();
        let known = per_book.iter().position(|(b, _, _)| *b == stat.book);
        if pages.try_reserve(1).is_err() || (known.is_none() && per_book.try_reserve(1).is_err()) {
            refused = true;
            return;
        }
        pages.push((stat.book, stat.page));
        match known.and_then(|at| per_book.get_mut(at)) {
            Some((_, s, _)) => *s = s.saturating_add(u32::from(stat.duration)),
            None => per_book.push((stat.book, u32::from(stat.duration), seen)),
        }
    });
    if refused {
        return Err(Error::NoMemory);
    }
    paperpaw_sdk::sort::sort(&mut pages);
    pages.dedup();
    paperpaw_sdk::sort::sort_by_key(&mut per_book, |(_, s, seen)| {
        (core::cmp::Reverse(*s), *seen)
    });
    let mut listed = Vec::new();
    listed
        .try_reserve_exact(per_book.len().min(DAY_BOOKS))
        .map_err(|_| Error::NoMemory)?;
    for (id, s, _) in per_book.iter().take(DAY_BOOKS) {
        let title = books
            .iter()
            .find(|b| b.id() == *id)
            .map_or("?", |b| b.title());
        listed.push((room::string(&[title]).map_err(|_| Error::NoMemory)?, *s));
    }
    Ok(Detail {
        seconds,
        pages: pages.len() as u32,
        books: listed,
    })
}

/// The chosen day in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DayText {
    /// `Tuesday, October 14` and, for where that does not fit, `Tu, Oct. 14`.
    pub long: String,
    pub short: String,
    /// `42 min · 12 pages`, or `Not read`.
    pub summary: String,
    /// Each book listed and its time.
    pub books: Vec<(String, String)>,
}

/// `detail` of `day` as the screen says it.
pub(crate) fn day_text(
    words: &mut impl Words,
    day: i32,
    detail: &Detail,
) -> Result<DayText, Error> {
    let month = texts::month(words, texts::date_of(day).month)?;
    let short_month = short_month(&month)?;
    let long = texts::long_date(words, day)?;
    let short = texts::short_date(words, day, &short_month)?;
    let summary = if detail.seconds == 0 {
        words.copy("stats.not-read")?
    } else {
        let time = texts::duration(words, u64::from(detail.seconds))?;
        let pages = words.format("stats.pages", &[Arg::Count(i64::from(detail.pages))])?;
        room::string(&[&time, " · ", &pages]).map_err(|_| Error::NoMemory)?
    };
    let mut books = Vec::new();
    books
        .try_reserve_exact(detail.books.len())
        .map_err(|_| Error::NoMemory)?;
    for (title, seconds) in &detail.books {
        books.push((
            room::string(&[title]).map_err(|_| Error::NoMemory)?,
            texts::duration(words, u64::from(*seconds))?,
        ));
    }
    Ok(DayText {
        long,
        short,
        summary,
        books,
    })
}
