//! The sources and the events kept on the reader, in memory and as the text
//! of one file: no host, no clock, no card here.
//!
//! An event is kept as the calendar says it (its first start, how long it
//! lasts, its rule) and expanded for a range of days when something is shown
//! ([`Store::occurrences`]); nothing of a repeat is kept. Times are the
//! reader's own clock: a time in another zone was moved to it when it was
//! read.
use alloc::{string::String, vec::Vec};
use core::fmt::Write as _;
use paperpaw_caldav::{
    date::{civil_from_days, days_from_civil, days_in_month},
    rrule::Rule,
};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::mem::push;

/// The first line of the file.
pub const HEADER: &str = "calendar 1";
/// The most sources, events kept in all, bytes of a title and of a place, of
/// a source's name, address and user name.
pub const MAX_SOURCES: usize = 6;
pub const MAX_ENTRIES: usize = 240;
pub const TITLE_MAX: usize = 56;
pub const PLACE_MAX: usize = 36;
pub const NAME_MAX: usize = 32;
pub const URL_MAX: usize = 240;
pub const USER_MAX: usize = 64;
/// The most days of exceptions a repeating event keeps.
pub const SKIP_MAX: usize = 16;
/// [`Entry::cal`] of an event whose source is one calendar (a subscription, a
/// file, an address book read whole), or one kept before the calendars of an
/// account were told apart.
pub const WHOLE_SOURCE: u32 = 0;
/// The most occurrences expanded for one view.
pub const OCCURRENCES_MAX: usize = 400;
/// How far from today events are kept, both ways, in days.
pub const WINDOW_DAYS: i32 = 60;
/// The second of the day of an event that has no time (it lasts whole days).
pub const NO_TIME: u32 = u32::MAX;

/// What a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A CalDAV account: every calendar of events at the address.
    Caldav,
    /// A calendar at an address that is only read (a subscription, `.ics`).
    Ics,
    /// A calendar file on the card.
    File,
    /// A CardDAV address book: birthdays and anniversaries.
    Contacts,
}

impl Kind {
    const fn code(self) -> u8 {
        match self {
            Self::Caldav => b'D',
            Self::Ics => b'I',
            Self::File => b'F',
            Self::Contacts => b'C',
        }
    }

    fn of(code: &str) -> Option<Self> {
        Some(match code {
            "D" => Self::Caldav,
            "I" => Self::Ics,
            "F" => Self::File,
            "C" => Self::Contacts,
            _ => return None,
        })
    }

    /// Whether the source is a server that wants a name and password.
    #[must_use]
    pub const fn account(self) -> bool {
        matches!(self, Self::Caldav | Self::Contacts)
    }

    /// Whether the source is read over the network.
    #[must_use]
    pub const fn online(self) -> bool {
        !matches!(self, Self::File)
    }
}

/// How the last run with a source went.
pub mod state {
    /// Not read yet.
    pub const NEVER: u8 = 0;
    pub const OK: u8 = 1;
    /// Read, but some of it was not (a calendar that failed, events too many).
    pub const PARTLY: u8 = 2;
    /// The name or password was refused.
    pub const DENIED: u8 = 3;
    /// Nothing there, or it was not a calendar.
    pub const FAILED: u8 = 4;
}

/// A calendar the events come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: u8,
    pub kind: Kind,
    /// How its events are marked in a view: 0 to 3 (plain, light, dark, bold).
    pub shade: u8,
    /// When it was read last (Unix time), 0 for never.
    pub synced: i64,
    pub state: u8,
    pub name: String,
    pub url: String,
    pub user: String,
}

/// What an event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Event,
    Birthday,
    Anniversary,
    /// A cancelled time of an event that repeats: only read, never kept.
    Cancelled,
}

impl Mark {
    const fn code(self) -> u8 {
        match self {
            Self::Event => b'E',
            Self::Birthday => b'B',
            Self::Anniversary => b'A',
            Self::Cancelled => b'X',
        }
    }

    fn of(code: &str) -> Option<Self> {
        Some(match code {
            "E" => Self::Event,
            "B" => Self::Birthday,
            "A" => Self::Anniversary,
            _ => return None,
        })
    }
}

/// An event as the calendar says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub src: u8,
    pub mark: Mark,
    /// The day it starts on (days since 1970); of a birthday, the day
    /// counted from (the year 2000 where the year is not known).
    pub day: i32,
    /// The second of the day it starts at, [`NO_TIME`] for whole days.
    pub secs: u32,
    /// How long: minutes for one with a time, days for whole days.
    pub span: u32,
    /// A birthday's year is known (its age can be told).
    pub known: bool,
    /// A hash of its `UID`, to tell which event a changed time belongs to.
    pub uid: u32,
    /// The calendar of the source it came from (a hash of its address), so
    /// that one calendar's answer replaces that calendar's events alone;
    /// [`WHOLE_SOURCE`] for none of its own.
    pub cal: u32,
    /// The day of the repeat this one stands in for (until it is linked).
    pub over: Option<i32>,
    /// The rule it repeats by, as written; empty for none.
    pub rule: String,
    /// Days of its repeats that are left out.
    pub skip: Vec<i32>,
    pub title: String,
    pub place: String,
}

/// A hash for telling ids apart (FNV-1a).
#[must_use]
pub fn hash(text: &str) -> u32 {
    let mut h = 0x811c_9dc5_u32;
    for byte in text.bytes() {
        h = (h ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    h
}

/// `text` on one line, at most `max` bytes, without blanks around it.
///
/// # Errors
/// `NoMemory`.
pub fn clean(text: &str, max: usize) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len().min(max))
        .map_err(|_| Error::NoMemory)?;
    for c in text.trim().chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out.truncate(out.trim_end().len());
    Ok(out)
}

/// What is seen at a time of a view: one occurrence of an event on one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occ {
    pub day: i32,
    /// The second it starts at on this day; [`NO_TIME`] for whole days and
    /// for a day an event goes on into.
    pub secs: u32,
    /// The second it ends at on this day ([`NO_TIME`] where it does not say
    /// or goes on past midnight).
    pub end: u32,
    /// The event, as a place in [`Store::entries`].
    pub entry: u16,
    /// The age a birthday makes, 0 where it is not known.
    pub age: u16,
    /// The event started on an earlier day.
    pub goes_on: bool,
}

impl Occ {
    /// Whether it has no time on this day.
    #[must_use]
    pub const fn whole_day(&self) -> bool {
        self.secs == NO_TIME
    }
}

/// Which events a view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    All,
    /// Birthdays and anniversaries only.
    Birthdays,
}

/// The days an event takes beyond its first.
fn extra_days(entry: &Entry) -> i32 {
    if entry.secs == NO_TIME {
        i32::try_from(entry.span.max(1) - 1).unwrap_or(0)
    } else if entry.span == 0 {
        0
    } else {
        i32::try_from((u64::from(entry.secs) + u64::from(entry.span) * 60 - 1) / 86_400)
            .unwrap_or(0)
    }
}

impl Entry {
    /// Tells `visit` each day an event of the entry starts on from `from` to
    /// `to`; `visit` says whether to go on.
    pub fn starts(&self, from: i32, to: i32, visit: &mut dyn FnMut(i32) -> bool) {
        match Rule::parse(&self.rule).filter(|_| !self.rule.is_empty()) {
            Some(rule) => rule.each(self.day, (from, to), &mut |day| {
                self.skip.contains(&day) || visit(day)
            }),
            None => {
                if (from..=to).contains(&self.day) {
                    visit(self.day);
                }
            }
        }
    }

    /// Whether something of it falls from `from` to `to`.
    #[must_use]
    pub fn falls_in(&self, from: i32, to: i32) -> bool {
        if matches!(self.mark, Mark::Birthday | Mark::Anniversary) {
            return true;
        }
        let mut found = false;
        self.starts(from.saturating_sub(extra_days(self)), to, &mut |_| {
            found = true;
            false
        });
        found
    }

    /// The year a birthday counts from, and its month and day.
    fn birth(&self) -> (i32, u8, u8) {
        civil_from_days(i64::from(self.day))
    }
}

fn leap(year: i32) -> bool {
    days_in_month(year, 2) == 29
}

/// The sources and the events.
#[derive(Debug, Default)]
pub struct Store {
    pub sources: Vec<Source>,
    pub entries: Vec<Entry>,
}

impl Store {
    #[must_use]
    pub fn source(&self, id: u8) -> Option<&Source> {
        self.sources.iter().find(|source| source.id == id)
    }

    pub fn source_mut(&mut self, id: u8) -> Option<&mut Source> {
        self.sources.iter_mut().find(|source| source.id == id)
    }

    /// Adds a source; its id.
    ///
    /// # Errors
    /// `Full` where there are [`MAX_SOURCES`]; `NoMemory`.
    pub fn add_source(
        &mut self,
        kind: Kind,
        name: &str,
        url: &str,
        user: &str,
    ) -> Result<u8, Error> {
        if self.sources.len() >= MAX_SOURCES {
            return Err(Error::Full);
        }
        let id = (1..=u8::MAX)
            .find(|id| self.source(*id).is_none())
            .ok_or(Error::Full)?;
        // Shades go round, so that two calendars differ.
        let shade = u8::try_from(self.sources.len() % 4).unwrap_or(0);
        let source = Source {
            id,
            kind,
            shade,
            synced: 0,
            state: state::NEVER,
            name: clean(name, NAME_MAX)?,
            url: clean(url, URL_MAX)?,
            user: clean(user, USER_MAX)?,
        };
        push(&mut self.sources, source)?;
        Ok(id)
    }

    /// Removes a source and its events.
    pub fn remove_source(&mut self, id: u8) {
        self.sources.retain(|source| source.id != id);
        self.entries.retain(|entry| entry.src != id);
    }

    /// The shade of the source of an event.
    #[must_use]
    pub fn shade_of(&self, entry: &Entry) -> u8 {
        self.source(entry.src).map_or(0, |source| source.shade)
    }

    /// Adds an event.
    ///
    /// # Errors
    /// `Full` at [`MAX_ENTRIES`]; `NoMemory`.
    pub fn add(&mut self, entry: Entry) -> Result<(), Error> {
        if self.entries.len() >= MAX_ENTRIES {
            return Err(Error::Full);
        }
        push(&mut self.entries, entry)
    }

    /// The events of a source read: the changed times are linked to the
    /// repeats they stand in for, cancelled times go, and what does not fall
    /// in `from..=to` goes.
    pub fn finish_source(&mut self, src: u8, from: i32, to: i32) {
        let overrides: Vec<(u32, i32)> = self
            .entries
            .iter()
            .filter(|entry| entry.src == src)
            .filter_map(|entry| Some((entry.uid, entry.over?)))
            .collect();
        for (uid, day) in overrides {
            if let Some(master) = self.entries.iter_mut().find(|entry| {
                entry.src == src
                    && entry.uid == uid
                    && entry.over.is_none()
                    && !entry.rule.is_empty()
            }) && master.skip.len() < SKIP_MAX
                && !master.skip.contains(&day)
            {
                master.skip.push(day);
            }
        }
        self.entries.retain_mut(|entry| {
            if entry.src != src {
                return true;
            }
            entry.over = None;
            entry.mark != Mark::Cancelled && entry.falls_in(from, to)
        });
    }

    /// The events from `from` to `to`, in the order of the days, whole days
    /// first, then by time.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn occurrences(&self, from: i32, to: i32, show: Show) -> Result<Vec<Occ>, Error> {
        let mut out = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let entry_at = u16::try_from(index).unwrap_or(u16::MAX);
            match entry.mark {
                Mark::Cancelled => {}
                Mark::Birthday | Mark::Anniversary => {
                    let (born, month, day) = entry.birth();
                    let (y0, _, _) = civil_from_days(i64::from(from));
                    let (y1, _, _) = civil_from_days(i64::from(to));
                    for year in y0..=y1 {
                        let shown = if month == 2 && day == 29 && !leap(year) {
                            28
                        } else {
                            day
                        };
                        let at = i32::try_from(days_from_civil(year, month, shown)).unwrap_or(0);
                        if !(from..=to).contains(&at) || (entry.known && year <= born) {
                            continue;
                        }
                        let age = if entry.known {
                            u16::try_from(year - born).unwrap_or(0)
                        } else {
                            0
                        };
                        if out.len() < OCCURRENCES_MAX {
                            push(
                                &mut out,
                                Occ {
                                    day: at,
                                    secs: NO_TIME,
                                    end: NO_TIME,
                                    entry: entry_at,
                                    age,
                                    goes_on: false,
                                },
                            )?;
                        }
                    }
                }
                Mark::Event if show == Show::All => {
                    let extra = extra_days(entry);
                    let mut failed = None;
                    entry.starts(from.saturating_sub(extra), to, &mut |start| {
                        for part in 0..=extra {
                            let day = start + part;
                            if !(from..=to).contains(&day) || out.len() >= OCCURRENCES_MAX {
                                continue;
                            }
                            let first = part == 0;
                            let last = part == extra;
                            let end = if entry.secs == NO_TIME || entry.span == 0 {
                                NO_TIME
                            } else if last {
                                u32::try_from(
                                    (u64::from(entry.secs) + u64::from(entry.span) * 60)
                                        .saturating_sub(u64::from(part.unsigned_abs()) * 86_400),
                                )
                                .unwrap_or(NO_TIME)
                            } else {
                                NO_TIME
                            };
                            let occ = Occ {
                                day,
                                secs: if first { entry.secs } else { NO_TIME },
                                end: if first && !last && entry.secs != NO_TIME {
                                    NO_TIME
                                } else {
                                    end
                                },
                                entry: entry_at,
                                age: 0,
                                goes_on: !first,
                            };
                            if let Err(error) = push(&mut out, occ) {
                                failed = Some(error);
                                return false;
                            }
                        }
                        true
                    });
                    if let Some(error) = failed {
                        return Err(error);
                    }
                }
                Mark::Event => {}
            }
        }
        out.sort_unstable_by_key(|occ| {
            (
                occ.day,
                if occ.secs == NO_TIME { 0 } else { occ.secs + 1 },
                occ.entry,
            )
        });
        Ok(out)
    }

    /// Whether some event falls on each day of `from..=to`: one bit a day.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn busy(&self, from: i32, to: i32) -> Result<Vec<u8>, Error> {
        let mut days = Vec::new();
        days.try_reserve(usize::try_from(to - from + 1).unwrap_or(0))
            .map_err(|_| Error::NoMemory)?;
        days.resize(usize::try_from(to - from + 1).unwrap_or(0), 0u8);
        for occ in self.occurrences(from, to, Show::All)? {
            if let Some(slot) = usize::try_from(occ.day - from)
                .ok()
                .and_then(|at| days.get_mut(at))
            {
                *slot = slot.saturating_add(1);
            }
        }
        Ok(days)
    }

    /// The text of the file.
    ///
    /// # Errors
    /// What `put` says, `NoMemory`.
    pub fn write(&self, put: &mut dyn FnMut(&str) -> Result<(), Error>) -> Result<(), Error> {
        put(HEADER)?;
        put("\n")?;
        let mut line = String::new();
        for source in &self.sources {
            line.clear();
            let _ = writeln!(
                line,
                "S\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                source.id,
                char::from(source.kind.code()),
                source.shade,
                source.synced,
                source.state,
                source.name,
                source.url,
                source.user
            );
            put(&line)?;
        }
        for entry in &self.entries {
            line.clear();
            let secs = if entry.secs == NO_TIME {
                String::from("-")
            } else {
                let mut text = String::new();
                let _ = write!(text, "{}", entry.secs);
                text
            };
            let _ = write!(
                line,
                "E\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t",
                entry.src,
                char::from(entry.mark.code()),
                entry.day,
                secs,
                entry.span,
                u8::from(entry.known),
                entry.uid
            );
            line.push_str(if entry.rule.is_empty() {
                "-"
            } else {
                &entry.rule
            });
            line.push('\t');
            if entry.skip.is_empty() {
                line.push('-');
            }
            for (at, day) in entry.skip.iter().enumerate() {
                if at > 0 {
                    line.push(',');
                }
                let _ = write!(line, "{day}");
            }
            let _ = writeln!(line, "\t{}\t{}\t{}", entry.title, entry.place, entry.cal);
            put(&line)?;
        }
        line.clear();
        let _ = writeln!(line, "X\t{}\t{}", self.sources.len(), self.entries.len());
        put(&line)
    }

    /// Takes a line of the file after the first; `Some(true)` for its last
    /// line when the counts agree.
    ///
    /// # Errors
    /// `NoMemory`; a line that is not understood is left out.
    pub fn read_line(&mut self, line: &str) -> Result<Option<bool>, Error> {
        let mut parts = line.split('\t');
        match parts.next() {
            Some("S") => {
                if let Some(source) = read_source(&mut parts)? {
                    push(&mut self.sources, source)?;
                }
            }
            Some("E") => {
                if let Some(entry) = read_entry(&mut parts)? {
                    push(&mut self.entries, entry)?;
                }
            }
            Some("X") => {
                let counts = (
                    parts.next().and_then(|n| n.parse::<usize>().ok()),
                    parts.next().and_then(|n| n.parse::<usize>().ok()),
                );
                return Ok(Some(
                    counts == (Some(self.sources.len()), Some(self.entries.len())),
                ));
            }
            _ => {}
        }
        Ok(None)
    }
}

fn read_source<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Result<Option<Source>, Error> {
    let (Some(id), Some(kind), Some(shade), Some(synced), Some(state)) = (
        parts.next().and_then(|n| n.parse().ok()),
        parts.next().and_then(Kind::of),
        parts.next().and_then(|n| n.parse::<u8>().ok()),
        parts.next().and_then(|n| n.parse().ok()),
        parts.next().and_then(|n| n.parse().ok()),
    ) else {
        return Ok(None);
    };
    let (name, url, user) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
    );
    Ok(Some(Source {
        id,
        kind,
        shade: shade.min(3),
        synced,
        state,
        name: clean(name, NAME_MAX)?,
        url: clean(url, URL_MAX)?,
        user: clean(user, USER_MAX)?,
    }))
}

fn read_entry<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Result<Option<Entry>, Error> {
    let (Some(src), Some(mark), Some(day), Some(secs), Some(span), Some(known), Some(uid)) = (
        parts.next().and_then(|n| n.parse().ok()),
        parts.next().and_then(Mark::of),
        parts.next().and_then(|n| n.parse().ok()),
        parts.next(),
        parts.next().and_then(|n| n.parse().ok()),
        parts.next(),
        parts.next().and_then(|n| n.parse().ok()),
    ) else {
        return Ok(None);
    };
    let rule = parts.next().unwrap_or("-");
    let skip = parts.next().unwrap_or("-");
    let (title, place) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    // A file written before the calendars were told apart has no column.
    let cal = parts
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(WHOLE_SOURCE);
    let mut days = Vec::new();
    for day in skip.split(',').filter_map(|day| day.parse::<i32>().ok()) {
        if days.len() < SKIP_MAX {
            push(&mut days, day)?;
        }
    }
    Ok(Some(Entry {
        src,
        mark,
        day,
        secs: secs.parse().unwrap_or(NO_TIME),
        span,
        known: known == "1",
        uid,
        cal,
        over: None,
        rule: if rule == "-" {
            String::new()
        } else {
            clean(rule, paperpaw_caldav::event::RULE_MAX)?
        },
        skip: days,
        title: clean(title, TITLE_MAX)?,
        place: clean(place, PLACE_MAX)?,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::string::ToString;

    fn day(y: i32, m: u8, d: u8) -> i32 {
        days_from_civil(y, m, d) as i32
    }

    fn event(title: &str, start: i32, secs: u32, span: u32, rule: &str) -> Entry {
        Entry {
            src: 1,
            mark: Mark::Event,
            day: start,
            secs,
            span,
            known: false,
            uid: hash(title),
            cal: WHOLE_SOURCE,
            over: None,
            rule: rule.to_string(),
            skip: Vec::new(),
            title: title.to_string(),
            place: String::new(),
        }
    }

    fn titles(store: &Store, from: i32, to: i32) -> Vec<(i32, String)> {
        store
            .occurrences(from, to, Show::All)
            .unwrap()
            .iter()
            .map(|occ| {
                (
                    occ.day - from,
                    store.entries[usize::from(occ.entry)].title.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn repeats_skips_and_days_that_go_on() {
        let monday = day(2026, 11, 16);
        let mut store = Store::default();
        let mut weekly = event("Standup", monday, 9 * 3600, 15, "FREQ=WEEKLY;BYDAY=MO,WE");
        weekly.skip.push(monday + 2);
        store.entries.push(weekly);
        store
            .entries
            .push(event("Trip", monday + 1, NO_TIME, 3, ""));
        store
            .entries
            .push(event("Night shift", monday + 3, 22 * 3600, 8 * 60, ""));
        let found = titles(&store, monday, monday + 7);
        assert_eq!(
            found,
            [
                (0, "Standup".into()),
                (1, "Trip".into()),
                (2, "Trip".into()),
                (3, "Trip".into()),
                (3, "Night shift".into()),
                (4, "Night shift".into()),
                (7, "Standup".into()),
            ]
        );
        // The day the second part goes on into says so, and where it ends.
        let occs = store
            .occurrences(monday + 4, monday + 4, Show::All)
            .unwrap();
        assert!(occs[0].goes_on && occs[0].secs == NO_TIME && occs[0].end == 6 * 3600);
        // A window that begins in the middle of a trip still shows it.
        assert_eq!(titles(&store, monday + 3, monday + 3).len(), 2);
    }

    #[test]
    fn birthdays_come_every_year_with_the_age() {
        let mut store = Store::default();
        let mut b = event("Jane", day(1985, 2, 28), NO_TIME, 1, "");
        b.mark = Mark::Birthday;
        b.known = true;
        let mut leaper = event("Leo", day(2000, 2, 29), NO_TIME, 1, "");
        leaper.mark = Mark::Birthday;
        store.entries.push(b);
        store.entries.push(leaper);
        let occs = store
            .occurrences(day(2026, 12, 1), day(2027, 12, 31), Show::Birthdays)
            .unwrap();
        // Feb 28 2027 for both (Leo's 29th falls back), nothing the year 1985.
        assert_eq!(occs.len(), 2);
        assert_eq!(occs[0].age, 42);
        assert_eq!(occs[1].age, 0);
        assert_eq!(occs[0].day, day(2027, 2, 28));
        let before = store
            .occurrences(day(1985, 1, 1), day(1985, 12, 31), Show::All)
            .unwrap();
        assert_eq!(before.len(), 1);
    }

    #[test]
    fn changed_times_replace_the_repeat_they_stand_in_for() {
        let monday = day(2026, 11, 16);
        let mut store = Store::default();
        store
            .entries
            .push(event("Class", monday, 10 * 3600, 60, "FREQ=WEEKLY"));
        let mut moved = event("Class", monday + 7, 14 * 3600, 60, "");
        moved.uid = hash("Class");
        moved.over = Some(monday + 7);
        store.entries.push(moved);
        let mut gone = event("Class", monday + 14, 10 * 3600, 60, "");
        gone.mark = Mark::Cancelled;
        gone.over = Some(monday + 14);
        store.entries.push(gone);
        store.finish_source(1, monday - 60, monday + 60);
        assert_eq!(store.entries.len(), 2);
        let secs: Vec<_> = store
            .occurrences(monday, monday + 21, Show::All)
            .unwrap()
            .iter()
            .map(|occ| (occ.day - monday, occ.secs / 3600))
            .collect();
        assert_eq!(secs, [(0, 10), (7, 14), (21, 10)]);
    }

    #[test]
    fn the_window_keeps_what_matters() {
        let today = day(2026, 11, 18);
        let mut store = Store::default();
        store.entries.push(event("Old", today - 200, 3600, 60, ""));
        store.entries.push(event("Near", today + 3, 3600, 60, ""));
        store
            .entries
            .push(event("Ended", today - 400, 3600, 60, "FREQ=DAILY;COUNT=5"));
        store
            .entries
            .push(event("Always", today - 400, 3600, 60, "FREQ=WEEKLY"));
        store.finish_source(1, today - WINDOW_DAYS, today + WINDOW_DAYS);
        let kept: Vec<_> = store.entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(kept, ["Near", "Always"]);
    }

    #[test]
    fn the_file_reads_back_the_same() {
        let mut store = Store::default();
        let id = store
            .add_source(Kind::Caldav, "Work", "https://cloud.example/dav/", "anna")
            .unwrap();
        store.source_mut(id).unwrap().synced = 1_795_000_000;
        let mut e = event(
            "Team\tmeeting",
            day(2026, 11, 16),
            9 * 3600,
            30,
            "FREQ=WEEKLY;BYDAY=MO",
        );
        e.place = "Room 4".into();
        e.cal = hash("/dav/calendars/anna/work/");
        e.skip = alloc::vec![day(2026, 11, 23)];
        store.entries.push(Entry {
            title: clean(&e.title, 56).unwrap(),
            ..e
        });
        let mut text = String::new();
        store
            .write(&mut |piece| {
                text.push_str(piece);
                Ok(())
            })
            .unwrap();
        let mut back = Store::default();
        let mut last = None;
        for (n, line) in text.lines().enumerate() {
            if n == 0 {
                assert_eq!(line, HEADER);
                continue;
            }
            last = back.read_line(line).unwrap();
        }
        assert_eq!(last, Some(true));
        assert_eq!(back.sources, store.sources);
        assert_eq!(back.entries, store.entries);
        // A line of a file written before the calendars were told apart.
        let mut old = Store::default();
        old.read_line("E\t1\tE\t20800\t-\t1\t0\t7\t-\t-\tDay off\t")
            .unwrap();
        assert_eq!(old.entries[0].cal, WHOLE_SOURCE);
        assert_eq!(old.entries[0].title, "Day off");
    }
}
