//! "Favourites": the favourite books as covers, the one added last first, in a
//! row or in rows.
//!
//! The favourites are the library's (the catalog marks them), read for the
//! catalog's generation (it moves when a book is added, opened, finished or
//! made a favourite) and kept until it moves; a book whose file is gone from
//! the card is left out. At most one page of the library is read, so at most
//! the books of one page show where more were asked for.
use crate::native_books::{buffers, with_library};
use crate::native_shelf::{Arrange, Shelved, Shown, label_height, render, targets};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
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
pub const TOKEN: u32 = 116;
/// The most books asked for (the setting's largest count).
const MOST: usize = 24;
/// The widget's name, its caption, and what it says with no books.
const CAPTION: &str = "widgets.favourites";
const NONE: &str = "widgets.favourites.none";
/// The layout that fills the place with rows.
const GRID: &str = "widgets.books.layout.grid";

/// A favourite as the library lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: u32,
    pub generation: u32,
    pub title: String,
    pub finished: bool,
    /// The book opened last: the one being read.
    pub current: bool,
}

/// The favourites, and the catalog generation they were read for.
#[derive(Debug, Default)]
pub struct Cache {
    kept: Option<(u32, Vec<Row>)>,
}

/// The favourites of the library, as the cache holds them for the catalog's
/// generation.
///
/// Where the library refuses a call the widget shows what it has: no books
/// (tried again at the next draw). Only a refused allocation is an error.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn rows<'c>(host: &Host<'_>, cache: &'c mut Cache) -> Result<&'c [Row], Error> {
    let generation = with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        Ok(library.state(host, &mut input, &mut out)?.generation())
    });
    let generation = match generation {
        Ok(generation) => generation,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(&[]),
    };
    if cache
        .kept
        .as_ref()
        .is_none_or(|(kept, _)| *kept != generation)
    {
        match read(host) {
            Ok(rows) => cache.kept = Some((generation, rows)),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // Tried again at the next draw.
            Err(_) => return Ok(&[]),
        }
    }
    Ok(cache
        .kept
        .as_ref()
        .map_or(&[][..], |(_, rows)| rows.as_slice()))
}

/// The favourites whose files are on the card, the newest added first, and which
/// book was opened last.
fn read(host: &Host<'_>) -> Result<Vec<Row>, Error> {
    with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        let current = {
            let query = Query::new().sort(Sort::RecentlyOpened);
            let page = library.query(host, &query, &mut input, &mut out)?;
            match page.rows().next() {
                Some(row) => {
                    let row = row?;
                    row.opened().then_some(row.key())
                }
                None => None,
            }
        };
        let query = Query::new()
            .show(Show::Favourites)
            .sort(Sort::Added)
            .descending(true);
        let page = library.query(host, &query, &mut input, &mut out)?;
        let generation = page.end().generation();
        let mut found = Vec::new();
        for row in page.rows().take(MOST) {
            let row = row?;
            mem::push(
                &mut found,
                Row {
                    key: row.key(),
                    generation,
                    title: text::copy(row.title())?,
                    finished: row.finished(),
                    current: current == Some(row.key()),
                },
            )?;
        }
        // The files: a book gone from the card is left out.
        let mut kept = Vec::new();
        let (mut ask, mut answer) = (mem::buffer(1024)?, buffers()?.1);
        for row in found {
            if gone(host, library, &row, (&mut ask, &mut answer))? {
                continue;
            }
            mem::push(&mut kept, row)?;
        }
        Ok(kept)
    })
}

/// Whether the file of `row`'s book is not on the card now.
fn gone(
    host: &Host<'_>,
    library: &Library,
    row: &Row,
    (ask, answer): (&mut [u8], &mut [u8]),
) -> Result<bool, Error> {
    let reference = BookRef::catalog(row.key, row.generation)?;
    match library.book(host, reference, ask, answer) {
        Ok(book) => Ok(book.missing()),
        Err(Error::NotFound) => Ok(true),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(false),
    }
}

/// What the widget shows of `rows` as `settings` ask: the first `count` of
/// them, labelled with their titles if the reader wants, those read and the
/// one being read marked.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn shown(settings: &Settings<'_, '_>, rows: &[Row]) -> Result<Shown, Error> {
    let count = usize::try_from(settings.int("count", 8)?)
        .unwrap_or(1)
        .clamp(1, MOST);
    let (titles, read, current) = (
        settings.shows("titles")?,
        settings.shows("read")?,
        settings.shows("current")?,
    );
    let mut books = Vec::new();
    books
        .try_reserve(count.min(rows.len()))
        .map_err(|_| Error::NoMemory)?;
    for row in rows.iter().take(count) {
        books.push(Shelved {
            book: (row.key, row.generation),
            title: text::copy(&row.title)?,
            label: if titles {
                text::copy(&row.title)?
            } else {
                String::new()
            },
            read: read && row.finished,
            current: current && row.current,
        });
    }
    let empty = books.is_empty().then_some(NONE);
    Ok(Shown {
        name: String::new(),
        books,
        total: rows.len(),
        read: 0,
        empty,
    })
}

/// How the books are arranged: in a row or in rows, a label's room below.
fn arrange(cx: &Cx<'_>) -> Result<Arrange, Error> {
    let grid = cx.settings.with_text("layout", |value| value == GRID)?;
    Ok(Arrange {
        grid,
        label: if cx.settings.shows("titles")? {
            label_height(cx)?
        } else {
            0
        },
        around: None,
    })
}

/// What the widget shows, to tell when it changed: the catalog's generation
/// and its books, never 0.
///
/// # Errors
/// The host's refusals.
pub fn shows(host: &Host<'_>, _widget: &str) -> Result<u64, Error> {
    crate::native_numbers::shows(host, _widget)
}

/// The height: the covers of the books in their rows, a label, the caption
/// and the frame.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn height_with(cx: &mut Cx<'_>, width: u16, cache: &mut Cache) -> Result<u16, Error> {
    let books = rows(cx.host, cache)?.len().min(
        usize::try_from(cx.settings.int("count", 8)?)
            .unwrap_or(1)
            .clamp(1, MOST),
    );
    let style = cx.style();
    let columns = u16::try_from(books).unwrap_or(u16::MAX).clamp(3, 6);
    let cover = (width / columns)
        .saturating_sub(style.gap)
        .saturating_mul(3)
        / 2;
    let rows = if arrange(cx)?.grid {
        u16::try_from(books)
            .unwrap_or(u16::MAX)
            .div_ceil(columns)
            .max(1)
    } else {
        1
    };
    let label = label_height(cx)?;
    let caption = layout::caption_height(&cx.font, style.small, style.gap)?;
    Ok(cover
        .saturating_add(label)
        .saturating_mul(rows)
        .saturating_add(caption)
        .saturating_add(cx.shape.frame_height()))
}

/// The height with the favourites read afresh.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    height_with(cx, width, &mut Cache::default())
}

/// Draws the widget in `area` without its covers, the favourites read afresh.
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
    let rows = rows(cx.host, cache)?;
    let shown = shown(&cx.settings, rows)?;
    let how = arrange(cx)?;
    let caption = cx.tr(CAPTION)?;
    render(cx, out, pictures, area, (&shown, how, &caption))
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
    let rows = rows(cx.host, cache)?;
    let shown = shown(&cx.settings, rows)?;
    targets(cx, area, (&shown, arrange(cx)?))
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
    let rows = rows(host, cache)?;
    Ok(shown(&settings, rows)?.books.get(index).map(|b| b.book))
}
