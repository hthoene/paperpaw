//! What the screens show, as rows of the kit's list: the calendar (agenda,
//! day, week, birthdays, an event whole) and the sources. The Month view is
//! drawn by `native_month`. The rows are built from the events in memory;
//! nothing here reads the card.
use alloc::{string::String, vec::Vec};
use paperpaw_caldav::{
    date::{civil_from_days, days_from_civil},
    rrule::{Freq, Rule},
};
use paperpaw_sdk::native::{Error, Host, formatting::Arg};
use paperpaw_ui_kit::{
    Item, ItemList, Messages,
    mem::push,
    text::{concat, copy},
};

use crate::model::{Kind, Mark, NO_TIME, Occ, Show, Store, state};
use crate::native_month::Grid;
use crate::native_text::{aged, day_text, long_date, month_name, short_date, time_text, when_text};

/// What the calendar screen shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Today and the next days.
    #[default]
    Agenda,
    Day(i32),
    /// The week that begins on this day.
    Week(i32),
    /// The month of this day, the day chosen.
    Month(i32),
    /// The birthdays and anniversaries of the next months.
    Birthdays,
    /// One event whole.
    Detail,
}

/// One event at one time, as a row stands for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    pub entry: u16,
    pub day: i32,
    pub age: u16,
}

/// What a row of a screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Which view is shown: choosing it offers the others.
    View,
    /// The step to the day or week before, and after.
    Prev,
    Next,
    /// The title of a day.
    Heading,
    Event(Pick),
    /// Back from an event.
    Back,
    /// A line of an event, not to be chosen.
    Info,
    /// The source with this id, *Add a calendar* and *Sync now*.
    Source(u8),
    Add,
    Sync,
}

/// A screen that is open.
#[derive(Debug, Default)]
pub struct View {
    pub list: ItemList,
    pub rows: Vec<Row>,
    /// What an empty list says, and what is said below the rows.
    pub empty: (String, String),
    pub hint: String,
    /// The Month view: where it drew its days.
    pub grid: Grid,
}

impl View {
    /// Takes the rows built anew, the focus where it was.
    pub fn set(&mut self, built: Built) {
        let focus = self.list.focus();
        self.list = ItemList::new(built.items);
        self.list
            .set_focus(focus.min(built.rows.len().saturating_sub(1)));
        self.rows = built.rows;
        self.empty = built.empty;
        self.hint = built.hint;
    }
}

/// The rows of a screen.
#[derive(Debug, Default)]
pub struct Built {
    pub items: Vec<Item>,
    pub rows: Vec<Row>,
    pub empty: (String, String),
    pub hint: String,
}

impl Built {
    fn add(&mut self, item: Item, row: Row) -> Result<(), Error> {
        push(&mut self.items, item)?;
        push(&mut self.rows, row)
    }
}

/// What the rows are built with: the day, the time, how the clock is shown.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub today: i32,
    /// The second of the day now.
    pub secs: u32,
    pub twelve: bool,
    /// How many days the agenda shows.
    pub days: i32,
    pub week_start: u8,
    pub syncing: bool,
    /// How far the reader's clock is east of UTC, for the time a source was
    /// read.
    pub zone: i32,
}

/// The first day of the week of `day`.
#[must_use]
pub fn week_of(day: i32, week_start: u8) -> i32 {
    let lead =
        (i32::from(paperpaw_caldav::date::weekday(i64::from(day))) - i32::from(week_start) + 7) % 7;
    day - lead
}

/// The days of month and year before or after the one of `day`, keeping the
/// day of the month where it fits.
#[must_use]
pub fn month_step(day: i32, by: i32) -> i32 {
    let (y, m, d) = civil_from_days(i64::from(day));
    let index = i64::from(y) * 12 + i64::from(m) - 1 + i64::from(by);
    let (y, m) = (
        i32::try_from(index.div_euclid(12)).unwrap_or(y),
        u8::try_from(index.rem_euclid(12) + 1).unwrap_or(1),
    );
    let d = d.min(paperpaw_caldav::date::days_in_month(y, m));
    i32::try_from(days_from_civil(y, m, d)).unwrap_or(day)
}

/// The label of a view and what it spans.
fn view_row(host: &Host<'_>, say: &mut Messages, mode: Mode, ctx: &Ctx) -> Result<Item, Error> {
    let (key, span) = match mode {
        Mode::Day(day) => ("calendar.view.day", long_date(host, say, day)?),
        Mode::Week(start) => {
            let (first, last) = (
                short_date(host, say, start)?,
                short_date(host, say, start + 6)?,
            );
            ("calendar.view.week", concat(&[&first, " – ", &last])?)
        }
        Mode::Month(day) => {
            let (year, month, _) = civil_from_days(i64::from(day));
            let name = month_name(host, say, month, false)?;
            let mut digits = [0u8; 20];
            let year =
                paperpaw_ui_kit::text::decimal(usize::try_from(year).unwrap_or(0), &mut digits);
            ("calendar.view.month", concat(&[&name, " ", year])?)
        }
        Mode::Birthdays => ("calendar.view.birthdays", String::new()),
        _ => (
            "calendar.view.agenda",
            say.format(
                host,
                "calendar.agenda.days",
                &[Arg::Count(i64::from(ctx.days))],
            )?,
        ),
    };
    Item::row(&say.copy(host, key)?, &span)?.with_icon(host, "calendar")
}

/// What an occurrence's row says at its end.
fn occ_value(
    host: &Host<'_>,
    say: &mut Messages,
    store: &Store,
    occ: &Occ,
    ctx: &Ctx,
) -> Result<String, Error> {
    match store.entries.get(usize::from(occ.entry)).map(|e| e.mark) {
        Some(Mark::Birthday) => say.copy(host, "calendar.birthday"),
        Some(Mark::Anniversary) => say.copy(host, "calendar.anniversary"),
        _ => when_text(host, say, ctx.twelve, occ, true),
    }
}

/// Adds the events `occs` as rows, a heading over those of each day (`days`)
/// or none.
fn add_events(
    host: &Host<'_>,
    say: &mut Messages,
    store: &Store,
    (built, occs): (&mut Built, &[Occ]),
    (ctx, headings): (&Ctx, bool),
) -> Result<(), Error> {
    let untitled = say.copy(host, "calendar.untitled")?;
    let mut last = None;
    for occ in occs {
        let Some(entry) = store.entries.get(usize::from(occ.entry)) else {
            continue;
        };
        if headings && last != Some(occ.day) {
            last = Some(occ.day);
            let text = day_text(host, say, occ.day, ctx.today)?;
            built.add(Item::heading(&text)?, Row::Heading)?;
        }
        let title = match entry.title.as_str() {
            "" => untitled.clone(),
            title if entry.mark == Mark::Event => copy(title)?,
            name => aged(host, say, name, occ.age)?,
        };
        let value = occ_value(host, say, store, occ, ctx)?;
        let pick = Pick {
            entry: occ.entry,
            day: occ.day,
            age: occ.age,
        };
        built.add(Item::row(&title, &value)?, Row::Event(pick))?;
    }
    Ok(())
}

/// Whether an event of today is over (and an agenda leaves it out).
fn over(occ: &Occ, ctx: &Ctx) -> bool {
    if occ.day != ctx.today || occ.whole_day() {
        return false;
    }
    let ends = if occ.end == NO_TIME {
        occ.secs
    } else {
        occ.end
    };
    ends < ctx.secs
}

/// The rows of the calendar screen showing `mode`.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn calendar_rows(
    host: &Host<'_>,
    store: &Store,
    mode: Mode,
    ctx: &Ctx,
) -> Result<Built, Error> {
    let mut say = Messages::new()?;
    let mut built = Built::default();
    built.add(view_row(host, &mut say, mode, ctx)?, Row::View)?;
    let (from, to, steps) = match mode {
        Mode::Day(day) => (day, day, Some(("day", mode))),
        Mode::Week(start) => (start, start + 6, Some(("week", mode))),
        Mode::Birthdays => (ctx.today, ctx.today + 90, None),
        _ => (ctx.today, ctx.today + ctx.days - 1, None),
    };
    if let Some((unit, _)) = steps {
        for (row, key, mark) in [
            (Row::Prev, "calendar.step.prev.", "‹"),
            (Row::Next, "calendar.step.next.", "›"),
        ] {
            let title = say.copy(host, &concat(&[key, unit])?)?;
            built.add(Item::row(&title, mark)?, row)?;
        }
    }
    let show = if mode == Mode::Birthdays {
        Show::Birthdays
    } else {
        Show::All
    };
    let mut occs = store.occurrences(from, to, show)?;
    if matches!(mode, Mode::Agenda) {
        occs.retain(|occ| !over(occ, ctx));
    }
    let headings = !matches!(mode, Mode::Day(_));
    add_events(host, &mut say, store, (&mut built, &occs), (ctx, headings))?;
    let none = match mode {
        Mode::Day(_) => "calendar.empty.day",
        Mode::Week(_) => "calendar.empty.week",
        Mode::Birthdays => "calendar.empty.birthdays",
        _ => "calendar.empty.agenda",
    };
    built.empty = (
        say.copy(host, "calendar.empty.title")?,
        if store.sources.is_empty() {
            say.copy(host, "calendar.empty.sources")?
        } else {
            say.copy(host, none)?
        },
    );
    built.hint = say.copy(host, "calendar.hint")?;
    Ok(built)
}

/// The words for how an event repeats.
fn repeat_key(rule: &str) -> Option<&'static str> {
    Some(match Rule::parse(rule)?.freq {
        Freq::Daily => "calendar.repeat.daily",
        Freq::Weekly => "calendar.repeat.weekly",
        Freq::Monthly => "calendar.repeat.monthly",
        Freq::Yearly => "calendar.repeat.yearly",
    })
}

/// The rows of one event: back, its title with its day and time, where it is,
/// which calendar and how it repeats.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn detail_rows(host: &Host<'_>, store: &Store, pick: Pick, ctx: &Ctx) -> Result<Built, Error> {
    let mut say = Messages::new()?;
    let mut built = Built::default();
    built.add(
        Item::row(&say.copy(host, "calendar.back")?, "‹")?,
        Row::Back,
    )?;
    let Some(entry) = store.entries.get(usize::from(pick.entry)) else {
        return Ok(built);
    };
    let occs = store.occurrences(pick.day, pick.day, Show::All)?;
    let occ = occs.iter().find(|occ| occ.entry == pick.entry);
    let day = long_date(host, &mut say, pick.day)?;
    let when = match occ {
        Some(occ) if entry.mark == Mark::Event => {
            let time = when_text(host, &mut say, ctx.twelve, occ, true)?;
            concat(&[&day, ", ", &time])?
        }
        _ => day,
    };
    let title = match entry.mark {
        Mark::Event => copy(&entry.title)?,
        _ => aged(host, &mut say, &entry.title, pick.age)?,
    };
    built.add(Item::whole(&title, "", &when)?, Row::Info)?;
    if !entry.place.is_empty() {
        let label = say.copy(host, "calendar.detail.place")?;
        built.add(
            Item::noted(&label, "", &entry.place)?.into_plain(),
            Row::Info,
        )?;
    }
    if let Some(source) = store.source(entry.src) {
        let label = say.copy(host, "calendar.detail.calendar")?;
        built.add(
            Item::noted(&label, "", &source.name)?.into_plain(),
            Row::Info,
        )?;
    }
    if let Some(key) = repeat_key(&entry.rule) {
        let label = say.copy(host, "calendar.detail.repeats")?;
        let how = say.copy(host, key)?;
        built.add(Item::noted(&label, "", &how)?.into_plain(), Row::Info)?;
    }
    built.hint = String::new();
    Ok(built)
}

trait Plain {
    fn into_plain(self) -> Self;
}

impl Plain for Item {
    /// A row that is only read: the focus skips it.
    fn into_plain(mut self) -> Self {
        if let Item::Row(row) = &mut self {
            row.plain = true;
        }
        self
    }
}

/// What a source is called in the list of kinds.
#[must_use]
pub const fn kind_key(kind: Kind) -> &'static str {
    match kind {
        Kind::Caldav => "calendar.kind.caldav",
        Kind::Ics => "calendar.kind.ics",
        Kind::File => "calendar.kind.file",
        Kind::Contacts => "calendar.kind.contacts",
    }
}

/// How a source's last read went, in words.
fn source_note(
    host: &Host<'_>,
    say: &mut Messages,
    source: &crate::model::Source,
    ctx: &Ctx,
) -> Result<String, Error> {
    let key = match source.state {
        state::DENIED => return say.copy(host, "calendar.source.denied"),
        state::FAILED => return say.copy(host, "calendar.source.failed"),
        state::NEVER => return say.copy(host, "calendar.source.never"),
        state::PARTLY => "calendar.source.partly",
        _ => "calendar.source.read",
    };
    let local = source.synced + i64::from(ctx.zone);
    let day = i32::try_from(local.div_euclid(86_400)).unwrap_or(0);
    let secs = u32::try_from(local.rem_euclid(86_400)).unwrap_or(0);
    let when = concat(&[
        &day_text(host, say, day, ctx.today)?,
        " ",
        &time_text(host, say, ctx.twelve, secs)?,
    ])?;
    say.format(
        host,
        key,
        &[Arg::Text {
            name: "when",
            value: &when,
        }],
    )
}

/// The rows of the sources' screen. `message` is how the last run went.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn sources_rows(
    host: &Host<'_>,
    store: &Store,
    ctx: &Ctx,
    message: &str,
) -> Result<Built, Error> {
    let mut say = Messages::new()?;
    let mut built = Built::default();
    for source in &store.sources {
        let kind = say.copy(host, kind_key(source.kind))?;
        let note = source_note(host, &mut say, source, ctx)?;
        let name = if source.name.is_empty() {
            copy(&source.url)?
        } else {
            copy(&source.name)?
        };
        built.add(Item::noted(&name, &kind, &note)?, Row::Source(source.id))?;
    }
    built.add(
        Item::row(&say.copy(host, "calendar.source.add")?, "+")?,
        Row::Add,
    )?;
    let note = if ctx.syncing {
        say.copy(host, "calendar.syncing")?
    } else if message.is_empty() {
        say.copy(host, "calendar.sources.idle")?
    } else {
        copy(message)?
    };
    let title = say.copy(host, "calendar.action.sync")?;
    built.add(Item::noted(&title, "›", &note)?, Row::Sync)?;
    built.hint = say.copy(host, "calendar.sources.hint")?;
    Ok(built)
}
