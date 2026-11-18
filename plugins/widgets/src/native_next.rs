//! "Next up": a book to read next, as the reader chooses: the next one in the
//! series of the book being read, the newest unread one, or an unread one by
//! chance (another each day); with its cover, title, author and series.
//!
//! The books are the library's. Which books are new the catalog says itself
//! (`Show::New`: added lately and never opened), so the widget keeps no files.
//! The choice is kept for the catalog's generation, the pick and the day.
use crate::native_block::{self as block, Block};
use crate::native_books::{buffers, with_library};
use crate::native_highlight::mix;
use crate::native_shelf::{BOOK_TARGET, cover_size, draw_cover};
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
    covers::{Covered, Pictures, cover},
    geometry, mem, paint, text,
    widget::Cx,
};

/// The callback token of the widget as declared.
pub const TOKEN: u32 = 118;
/// The pick that takes the newest unread book, and the one that takes any by chance.
const NEWEST: &str = "widgets.next.newest";
const RANDOM: &str = "widgets.next.random";
/// Unread books looked at, at most, to find one whose file is on the card.
const CANDIDATES: usize = 6;
/// What is said with no book.
const NONE: &str = "widgets.next.none";
/// The target that opens the library, where there is no book.
pub const LIBRARY_TARGET: u32 = 1;

/// How the book is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Series,
    Newest,
    Random,
}

/// The book chosen, as the library lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: u32,
    pub generation: u32,
    pub title: String,
    pub author: String,
    pub series: String,
    pub number: String,
}

/// The book chosen, and what it was chosen for: the pick, the day and the
/// catalog's generation.
#[derive(Debug, Default)]
pub struct Cache {
    kept: Option<((u8, u64, u32), Option<Row>)>,
}

fn pick_of(cx_pick: &str) -> Pick {
    match cx_pick {
        NEWEST => Pick::Newest,
        RANDOM => Pick::Random,
        _ => Pick::Series,
    }
}

/// How the book is chosen, as the reader set it.
fn pick(settings: &paperpaw_ui_kit::widget::Settings<'_, '_>) -> Result<Pick, Error> {
    settings.with_text("pick", pick_of)
}

/// The score of a book on a day: the lowest is taken.
fn chance(key: u32, day: u64) -> u64 {
    mix(u64::from(key) ^ mix(day))
}

/// The book chosen for this pick and day, kept for the catalog's generation.
///
/// Where the library refuses a call the widget shows no book (tried again at
/// the next draw). Only a refused allocation is an error.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn chosen<'c>(
    host: &Host<'_>,
    pick: (&paperpaw_ui_kit::widget::Settings<'_, '_>, u64),
    cache: &'c mut Cache,
) -> Result<Option<&'c Row>, Error> {
    let (settings, day) = pick;
    let how = self::pick(settings)?;
    let generation = with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        Ok(library.state(host, &mut input, &mut out)?.generation())
    });
    let generation = match generation {
        Ok(generation) => generation,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(None),
    };
    let key = (how as u8, day, generation);
    if cache.kept.as_ref().is_none_or(|(kept, _)| *kept != key) {
        match read(host, how, day) {
            Ok(row) => cache.kept = Some((key, row)),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(None),
        }
    }
    Ok(cache.kept.as_ref().and_then(|(_, row)| row.as_ref()))
}

fn read(host: &Host<'_>, how: Pick, day: u64) -> Result<Option<Row>, Error> {
    with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        let first = match how {
            Pick::Series => in_series(host, library, (&mut input, &mut out))?,
            Pick::Newest => newest(host, library, (&mut input, &mut out))?,
            Pick::Random => None,
        };
        match first {
            Some(row) => Ok(Some(row)),
            // Else by chance.
            None => by_chance(host, library, day, (&mut input, &mut out)),
        }
    })
}

/// The first row of `query`'s page that is a book never opened whose file is
/// on the card.
fn first_unread(
    host: &Host<'_>,
    library: &Library,
    query: &Query<'_>,
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<Option<Row>, Error> {
    let page = library.query(host, query, input, out)?;
    let generation = page.end().generation();
    let mut found = Vec::new();
    for row in page.rows() {
        let row = row?;
        if !row.opened() {
            mem::push(&mut found, row_of(row.key(), generation, &row)?)?;
        }
        if found.len() >= CANDIDATES {
            break;
        }
    }
    present(host, library, found)
}

fn row_of(
    key: u32,
    generation: u32,
    row: &paperpaw_sdk::native::library::Entry<'_>,
) -> Result<Row, Error> {
    Ok(Row {
        key,
        generation,
        title: text::copy(row.title())?,
        author: text::copy(row.author())?,
        series: text::copy(row.series())?,
        number: text::copy(row.number())?,
    })
}

/// The first of `rows` whose file is on the card.
fn present(host: &Host<'_>, library: &Library, rows: Vec<Row>) -> Result<Option<Row>, Error> {
    let (mut ask, mut answer) = (mem::buffer(1024)?, mem::buffer(mem::SERVICE_BUFFER)?);
    for row in rows {
        let reference = BookRef::catalog(row.key, row.generation)?;
        match library.book(host, reference, &mut ask, &mut answer) {
            Ok(book) if book.missing() => {}
            Err(Error::NotFound) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            _ => return Ok(Some(row)),
        }
    }
    Ok(None)
}

/// The book being read: the one opened last, with its series.
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
                Some((row.key(), text::copy(row.series().trim())?))
            } else {
                None
            }
        }
        None => None,
    })
}

/// The first book after the one being read in its series that was not begun.
fn in_series(
    host: &Host<'_>,
    library: &Library,
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<Option<Row>, Error> {
    let Some((current, series)) = reading(host, library, (&mut *input, &mut *out))? else {
        return Ok(None);
    };
    if series.is_empty() {
        return Ok(None);
    }
    let query = Query::new()
        .show(Show::All)
        .sort(Sort::Series)
        .series(&series)?;
    let page = library.query(host, &query, input, out)?;
    let generation = page.end().generation();
    let mut after = Vec::new();
    let mut passed = false;
    for row in page.rows() {
        let row = row?;
        if row.key() == current {
            passed = true;
        } else if passed && !row.opened() && row.series().trim().eq_ignore_ascii_case(&series) {
            mem::push(&mut after, row_of(row.key(), generation, &row)?)?;
            if after.len() >= CANDIDATES {
                break;
            }
        }
    }
    present(host, library, after)
}

fn newest(
    host: &Host<'_>,
    library: &Library,
    sides: (&mut [u8], &mut [u8]),
) -> Result<Option<Row>, Error> {
    let query = Query::new()
        .show(Show::New)
        .sort(Sort::Added)
        .descending(true);
    first_unread(host, library, &query, sides)
}

/// An unread book by chance: the day's lowest scores among the unread books of
/// one page, the first whose file is on the card.
fn by_chance(
    host: &Host<'_>,
    library: &Library,
    day: u64,
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<Option<Row>, Error> {
    let query = Query::new().show(Show::Unread);
    let page = library.query(host, &query, input, out)?;
    let generation = page.end().generation();
    let mut kept: Vec<(u64, Row)> = Vec::new();
    for row in page.rows() {
        let row = row?;
        let score = chance(row.key(), day);
        if kept.len() == CANDIDATES && kept.last().is_some_and(|(s, _)| *s <= score) {
            continue;
        }
        mem::push(&mut kept, (score, row_of(row.key(), generation, &row)?))?;
        // The row goes to its place among the sorted ones; a std sort's frame is
        // over the X4's 2,048 B bound.
        let mut at = kept.len() - 1;
        while at > 0 && kept[at - 1].0 > kept[at].0 {
            kept.swap(at - 1, at);
            at -= 1;
        }
        kept.truncate(CANDIDATES);
    }
    present(
        host,
        library,
        kept.into_iter().map(|(_, row)| row).collect(),
    )
}

/// What the widget shows, to tell when it changed: the catalog's generation
/// and the day, never 0.
///
/// # Errors
/// The host's refusals.
pub fn shows(host: &Host<'_>, _widget: &str) -> Result<u64, Error> {
    let day = crate::native_draw::today(host)?;
    let numbers = crate::native_numbers::shows(host, _widget)?;
    Ok(crate::native_draw::fnv(numbers, &day.to_le_bytes()))
}

/// The height: three rows of the interface and the frame.
///
/// # Errors
/// Never.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    Ok(cx
        .style()
        .row
        .saturating_mul(3)
        .saturating_add(cx.shape.frame_height()))
}

/// What `book` says of its series: "Name, no. 2" where it has a number.
fn series_text(cx: &mut Cx<'_>, book: &Row) -> Result<String, Error> {
    match (book.series.is_empty(), book.number.is_empty()) {
        (true, _) => Ok(String::new()),
        (false, true) => text::copy(&book.series),
        (false, false) => cx.format(
            "widgets.next.in-series",
            &[
                Arg::Text {
                    name: "series",
                    value: &book.series,
                },
                Arg::Text {
                    name: "number",
                    value: &book.number,
                },
            ],
        ),
    }
}

/// Where the parts of the widget go.
#[derive(Debug, Default)]
struct Laid {
    cover: Option<Rect>,
    text: Option<Rect>,
}

/// The cover and the text of a book in `area`: the cover on the left where the
/// place is wide, the text beside it; else the cover above the text (what fits
/// in half of the place), left out where too small to tell.
fn lay_out(
    cx: &Cx<'_>,
    area: Rect,
    shape: Option<(u32, u32)>,
    (text_h, least): (u16, u16),
) -> Result<Laid, Error> {
    let style = cx.style();
    let gap = style.gap;
    let align = cx.settings.align()?;
    if !cx.settings.shows("cover")? {
        return Ok(Laid {
            cover: None,
            text: Some(area),
        });
    }
    let (w, h) = cover_size(shape, area.width.saturating_mul(2) / 5, area.height);
    let room = f32::from(area.width.saturating_sub(w.saturating_add(gap)));
    if area.width >= area.height && room >= style.small * 7.0 {
        let shift = w.saturating_add(gap).min(area.width);
        return Ok(Laid {
            cover: Some(Rect::new(
                area.x,
                area.y.saturating_add(area.height.saturating_sub(h) / 2),
                w,
                h,
            )),
            text: Some(Rect::new(
                area.x.saturating_add(shift),
                area.y,
                area.width - shift,
                area.height,
            )),
        });
    }
    // High: the cover above, the text below it.
    let free = area.height.saturating_sub(text_h.saturating_add(gap / 2));
    if free < area.height / 3 || free < least.saturating_mul(3) / 2 {
        return Ok(Laid {
            cover: None,
            text: Some(area),
        });
    }
    let (w, h) = cover_size(shape, area.width, free);
    let total = h.saturating_add(gap / 2).saturating_add(text_h);
    let top = area
        .y
        .saturating_add(area.height.saturating_sub(total.min(area.height)) / 2);
    let left = geometry::clamp_u16(align.x(area, f32::from(w)) as i64);
    Ok(Laid {
        cover: Some(Rect::new(left, top, w, h)),
        text: Some(Rect::new(
            area.x,
            top.saturating_add(h).saturating_add(gap / 2),
            area.width,
            text_h,
        )),
    })
}

/// What the widget says of a book: the caption, the title, the author and the
/// series, as the settings show them.
fn block_of(cx: &mut Cx<'_>, book: &Row) -> Result<Block, Error> {
    let lead = if cx.settings.shows("caption")? {
        Some(cx.tr("widgets.next.caption")?)
    } else {
        None
    };
    let mut notes = Vec::new();
    if cx.settings.shows("author")? && !book.author.is_empty() {
        mem::push(&mut notes, text::copy(&book.author)?)?;
    }
    if cx.settings.shows("series")? {
        let series = series_text(cx, book)?;
        if !series.is_empty() {
            mem::push(&mut notes, series)?;
        }
    }
    Ok(Block {
        lead,
        title: Some(text::copy(&book.title)?),
        notes,
    })
}

/// Draws the widget in `area`; the cover the host draws goes into `pictures`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw_with(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    (cache, pictures): (&mut Cache, &mut Pictures),
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let day = crate::native_draw::today(cx.host)?;
    let (host, settings) = (cx.host, cx.settings);
    let Some(book) = chosen(host, (&settings, day), cache)? else {
        let message = cx.tr(NONE)?;
        return crate::native_draw::message_in(cx, out, &look, &message);
    };
    let covered = if cx.settings.shows("cover")? {
        with_library(host, |library| {
            cover(host, library, (book.key, book.generation))
        })?
    } else {
        Covered::None
    };
    if covered == Covered::Busy {
        pictures.busy = true;
    }
    let shape = match covered {
        Covered::Ready(ready) => Some((ready.width, ready.height)),
        _ => None,
    };
    let block = block_of(cx, book)?;
    let title_size = cx.settings.size()?.text(cx.style());
    let least =
        u16::try_from((u32::from(cx.snapshot.display_dpi()) * 50).div_ceil(254)).unwrap_or(0);
    // The text's height where the cover is above it: what fits in half the place.
    let half = look.area.height / 2;
    let text_h = block::height(cx, &block, (look.area.width, half), title_size)?;
    let laid = lay_out(cx, look.area, shape, (text_h, least.max(24)))?;
    if let Some(at) = laid.cover {
        draw_cover(cx, out, pictures, &look, at, (&book.title, covered))?;
    }
    if let Some(room) = laid.text {
        let align = cx.settings.align()?;
        block::draw(cx, out, &look, (room, align), (&block, title_size))?;
    }
    Ok(())
}

/// Draws the widget in `area` without its cover, the book chosen afresh.
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

/// The widget's one target: a tap opens the book, or the library where there is
/// none.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn targets_with(
    cx: &Cx<'_>,
    area: Rect,
    cache: &mut Cache,
) -> Result<Vec<WidgetTarget>, Error> {
    let day = crate::native_draw::today(cx.host)?;
    let id = if chosen(cx.host, (&cx.settings, day), cache)?.is_some() {
        BOOK_TARGET
    } else {
        LIBRARY_TARGET
    };
    let mut targets = Vec::new();
    mem::push(&mut targets, WidgetTarget::new(id, paint::frame_rect(area)))?;
    Ok(targets)
}

/// The book a tap opens: the one chosen.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn book(host: &Host<'_>, widget: &str, cache: &mut Cache) -> Result<Option<(u32, u32)>, Error> {
    let settings = paperpaw_ui_kit::widget::Settings::new(host, widget);
    let day = crate::native_draw::today(host)?;
    Ok(chosen(host, (&settings, day), cache)?.map(|row| (row.key, row.generation)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_is_read_from_its_choice_and_series_is_the_default() {
        assert_eq!(pick_of(NEWEST), Pick::Newest);
        assert_eq!(pick_of(RANDOM), Pick::Random);
        assert_eq!(pick_of("widgets.next.series"), Pick::Series);
        assert_eq!(pick_of("anything else"), Pick::Series);
    }

    #[test]
    fn the_chance_of_a_book_is_the_same_on_a_day_and_another_on_the_next() {
        assert_eq!(chance(7, 20_261_110), chance(7, 20_261_110));
        let days: alloc::collections::BTreeSet<u64> =
            (0..30).map(|day| chance(7, 20_261_100 + day)).collect();
        assert!(days.len() > 20);
        let books: alloc::collections::BTreeSet<u64> =
            (0..30).map(|key| chance(key, 20_261_110)).collect();
        assert!(books.len() > 20);
    }
}
