//! "Series": the books of a series as covers in a row, in their order, each with
//! its number, those read marked and the one being read underlined. The series
//! is the book being read's, or one the reader names.
//!
//! The members are the library's (one query of the series, ordered by the
//! numbers of the catalog), read for the catalog's generation and the series
//! asked for and kept until either moves; a book whose file is gone from the
//! card is left out. At most one page of the library is read.
use crate::native_books::{buffers, with_library};
use crate::native_shelf::{
    Arrange, Shelved, Shown, label_height, mark_height, render, targets, window,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        formatting::Arg,
        library::{BookRef, Library, Query, Show, Sort},
        ui::WidgetTarget,
    },
};
use paperpaw_ui_kit::{
    Paint,
    covers::Pictures,
    mem, text,
    widget::{Cx, Settings, layout},
};

/// The callback token of the widget as declared.
pub const TOKEN: u32 = 117;
/// The series named by the reader (empty: the book being read's), the longest
/// name taken.
const NAME_LIMIT: usize = 120;
/// Which books of a long series show: from the first, as against those round the
/// book being read.
const FIRST: &str = "widgets.series.first";
/// What is said with no series, and with no catalog.
const NONE: &str = "widgets.series.none";
const NO_CATALOG: &str = "widgets.series.no-catalog";

/// A book of the series as the library lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub key: u32,
    pub generation: u32,
    pub title: String,
    pub number: String,
    pub finished: bool,
}

/// The series found for a name, and whether the book being read is in it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Found {
    /// The series' name as shown.
    pub name: String,
    pub members: Vec<Member>,
    /// The members the library holds, though a page lists fewer.
    pub total: usize,
    /// The key of the book being read, if it is in the series.
    pub current: Option<u32>,
    /// The catalog has no books at all.
    pub no_catalog: bool,
}

/// What was found, and what it was found for.
#[derive(Debug, Default)]
pub struct Cache {
    kept: Option<((String, u32), Found)>,
}

/// The series the widget shows: the name the reader gave, else the book being
/// read's; kept for the name and the catalog's generation.
///
/// Where the library refuses a call the widget shows what it has: nothing,
/// tried again at the next draw. Only a refused allocation is an error.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn found<'c>(
    host: &Host<'_>,
    settings: &Settings<'_, '_>,
    cache: &'c mut Cache,
) -> Result<&'c Found, Error> {
    let named = settings.string("name", NAME_LIMIT)?;
    match look(host, named.trim(), cache) {
        Ok(()) => {}
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        // Nothing to show; asked again at the next draw.
        Err(_) => cache.kept = None,
    }
    Ok(cache.kept.as_ref().map_or(&EMPTY, |(_, found)| found))
}

static EMPTY: Found = Found {
    name: String::new(),
    members: Vec::new(),
    total: 0,
    current: None,
    no_catalog: false,
};

fn look(host: &Host<'_>, named: &str, cache: &mut Cache) -> Result<(), Error> {
    let generation = with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        Ok(library.state(host, &mut input, &mut out)?.generation())
    })?;
    let wanted = text::copy(named)?;
    if cache
        .kept
        .as_ref()
        .is_some_and(|((name, kept), _)| *kept == generation && *name == wanted)
    {
        return Ok(());
    }
    let found = read(host, named)?;
    cache.kept = Some(((wanted, generation), found));
    Ok(())
}

/// The books opened last: the first is the book being read, if it was opened.
fn reading(
    host: &Host<'_>,
    library: &Library,
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<Option<(u32, String)>, Error> {
    let query = Query::new().sort(Sort::RecentlyOpened);
    let page = library.query(host, &query, input, out)?;
    Ok(match page.rows().next() {
        Some(row) => {
            let row = row?;
            if row.opened() {
                Some((row.key(), text::copy(row.series())?))
            } else {
                None
            }
        }
        None => None,
    })
}

fn read(host: &Host<'_>, named: &str) -> Result<Found, Error> {
    with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        let state = library.state(host, &mut input, &mut out)?;
        if state.books() == 0 {
            return Ok(Found {
                name: text::copy(named)?,
                no_catalog: true,
                ..Found::default()
            });
        }
        let current = reading(host, library, (&mut input, &mut out))?;
        let name = if named.is_empty() {
            match &current {
                Some((_, series)) => text::copy(series.trim())?,
                None => String::new(),
            }
        } else {
            text::copy(named)?
        };
        if name.is_empty() {
            return Ok(Found::default());
        }
        let query = Query::new()
            .show(Show::All)
            .sort(Sort::Series)
            .series(&name)?;
        let page = library.query(host, &query, &mut input, &mut out)?;
        let generation = page.end().generation();
        let mut members = Vec::new();
        for row in page.rows() {
            let row = row?;
            if !row.series().trim().eq_ignore_ascii_case(&name) {
                continue;
            }
            mem::push(
                &mut members,
                Member {
                    key: row.key(),
                    generation,
                    title: text::copy(row.title())?,
                    number: text::copy(row.number())?,
                    finished: row.finished(),
                },
            )?;
        }
        let total = members.len();
        let members = present(host, library, members)?;
        let current = current
            .map(|(key, _)| key)
            .filter(|key| members.iter().any(|m| m.key == *key));
        Ok(Found {
            name,
            members,
            total,
            current,
            no_catalog: false,
        })
    })
}

/// `members` whose files are on the card.
fn present(host: &Host<'_>, library: &Library, members: Vec<Member>) -> Result<Vec<Member>, Error> {
    let (mut ask, mut answer) = (mem::buffer(1024)?, mem::buffer(mem::SERVICE_BUFFER)?);
    let mut kept = Vec::new();
    for member in members {
        let reference = BookRef::catalog(member.key, member.generation)?;
        match library.book(host, reference, &mut ask, &mut answer) {
            Ok(book) if book.missing() => continue,
            Err(Error::NotFound) => continue,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            _ => {}
        }
        mem::push(&mut kept, member)?;
    }
    Ok(kept)
}

/// The label of a book: its number and its title as the reader wants them.
fn label(member: &Member, numbers: bool, titles: bool) -> Result<String, Error> {
    let number = numbers && !member.number.is_empty();
    Ok(match (number, titles) {
        (true, true) => text::concat(&[&member.number, " · ", &member.title])?,
        (true, false) => text::copy(&member.number)?,
        (false, true) => text::copy(&member.title)?,
        (false, false) => String::new(),
    })
}

/// How many books the reader wants and where they start.
fn wanted(settings: &Settings<'_, '_>) -> Result<(usize, bool), Error> {
    let count = usize::try_from(settings.int("count", 6)?)
        .unwrap_or(2)
        .clamp(2, 12);
    let around = settings.with_text("start", |value| value != FIRST)?;
    Ok((count, around))
}

/// What the widget shows of `found` as `settings` ask: the books that show,
/// all of them (the layout picks those that fit), labelled, those read and
/// the one being read marked.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn shown(settings: &Settings<'_, '_>, found: &Found) -> Result<Shown, Error> {
    if found.name.is_empty() && !found.no_catalog {
        return Ok(Shown {
            empty: Some(NONE),
            ..Shown::default()
        });
    }
    if found.no_catalog {
        return Ok(Shown {
            name: text::copy(&found.name)?,
            empty: Some(NO_CATALOG),
            ..Shown::default()
        });
    }
    if found.members.is_empty() {
        return Ok(Shown {
            name: text::copy(&found.name)?,
            empty: Some(NONE),
            ..Shown::default()
        });
    }
    let (count, around) = wanted(settings)?;
    let at = found
        .current
        .and_then(|key| found.members.iter().position(|m| m.key == key));
    let range = window(found.members.len(), count, at.filter(|_| around));
    let (numbers, titles) = (settings.shows("numbers")?, settings.shows("titles")?);
    let (read, current) = (settings.shows("read")?, settings.shows("current")?);
    let mut books = Vec::new();
    books
        .try_reserve(range.len())
        .map_err(|_| Error::NoMemory)?;
    let mut finished = 0;
    for member in found.members.iter() {
        finished += usize::from(member.finished);
    }
    for member in found.members.get(range).unwrap_or_default() {
        books.push(Shelved {
            book: (member.key, member.generation),
            title: text::copy(&member.title)?,
            label: label(member, numbers, titles)?,
            read: read && member.finished,
            current: current && found.current == Some(member.key),
        });
    }
    Ok(Shown {
        name: text::copy(&found.name)?,
        books,
        total: found.total,
        read: finished,
        empty: None,
    })
}

/// How the books are arranged: in a row, the room below each cover a label's,
/// or the line of the book being read where there are no labels; those round
/// the book being read where more are listed than fit.
fn arrange(settings: &Settings<'_, '_>, cx: &Cx<'_>, shown: &Shown) -> Result<Arrange, Error> {
    let labelled = settings.shows("numbers")? || settings.shows("titles")?;
    let (_, around) = wanted(settings)?;
    let at = shown.books.iter().position(|b| b.current);
    Ok(Arrange {
        grid: false,
        label: if labelled {
            label_height(cx)?
        } else {
            mark_height(cx)
        },
        around: at.filter(|_| around),
    })
}

/// The caption: the series' name and how many of its books are read.
fn caption(cx: &mut Cx<'_>, shown: &Shown) -> Result<String, Error> {
    let name = Arg::Text {
        name: "series",
        value: &shown.name,
    };
    let count = Arg::Int {
        name: "count",
        value: i64::try_from(shown.total).unwrap_or(i64::MAX),
    };
    if shown.read > 0 {
        let read = Arg::Int {
            name: "read",
            value: i64::try_from(shown.read).unwrap_or(i64::MAX),
        };
        cx.format("widgets.series.caption-read", &[name, read, count])
    } else {
        cx.format("widgets.series.caption", &[name, count])
    }
}

/// What the widget shows, to tell when it changed: the catalog's generation
/// and its books, never 0.
///
/// # Errors
/// The host's refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    crate::native_numbers::shows(host, widget)
}

/// The height: the covers of as many books as are wanted side by side, a label,
/// the caption and the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let count = u16::try_from(cx.settings.int("count", 6)?)
        .unwrap_or(2)
        .clamp(1, 12);
    let style = cx.style();
    let cover = (width / count).saturating_sub(style.gap).saturating_mul(3) / 2;
    let caption = layout::caption_height(&cx.font, style.small, style.gap)?;
    Ok(cover
        .saturating_add(label_height(cx)?)
        .saturating_add(caption)
        .saturating_add(cx.shape.frame_height()))
}

/// Draws the widget in `area` without its covers, the series read afresh.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    draw_with(
        cx,
        out,
        area,
        (&mut Cache::default(), &mut Pictures::default()),
    )
}

/// Draws the widget in `area`; the covers the host draws go into `pictures`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw_with(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    (cache, pictures): (&mut Cache, &mut Pictures),
) -> Result<(), Error> {
    let found = found(cx.host, &cx.settings, cache)?;
    let shown = shown(&cx.settings, found)?;
    let how = arrange(&cx.settings, cx, &shown)?;
    let title = caption(cx, &shown)?;
    render(cx, out, pictures, area, (&shown, how, &title))
}

/// The targets of the widget placed in `area`: one for each book on show.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn targets_with(
    cx: &Cx<'_>,
    area: Rect,
    cache: &mut Cache,
) -> Result<Vec<WidgetTarget>, Error> {
    let found = found(cx.host, &cx.settings, cache)?;
    let shown = shown(&cx.settings, found)?;
    let how = arrange(&cx.settings, cx, &shown)?;
    targets(cx, area, (&shown, how))
}

/// The book the target `index` opens: the `index`th of those that show.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn book_at(
    host: &Host<'_>,
    widget: &str,
    cache: &mut Cache,
    index: usize,
) -> Result<Option<(u32, u32)>, Error> {
    let settings = Settings::new(host, widget);
    let found = found(host, &settings, cache)?;
    Ok(shown(&settings, found)?.books.get(index).map(|b| b.book))
}
