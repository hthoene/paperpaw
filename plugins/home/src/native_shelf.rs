//! The recent shelf, as the home screen draws it: the books read before the
//! current one, as many as the setting says and as fit across the screen, each
//! a cover with its title below and the heading above. Where the covers would be
//! smaller than two lines of text the titles stand alone, shown even when set
//! off, as something must say which books they are. A book without a cover is a
//! frame with its title in it. A tap on a book opens it.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{Error, Host, snapshot::capture_snapshot},
};
use paperpaw_ui_kit::{
    Paint, geometry, paint,
    widget::{
        Cx, Settings,
        look::{Align, Look},
        text::{self, Wrapped},
    },
};

use crate::native_recent::{self, Book, Covered, Known, MOST, Pictures};
use crate::native_source::mm;

/// How wide a cover's place is at least, in millimetres.
const COVER_MM: u32 = 18;
/// The most targets a callback answers; the shelf holds [`MOST`] books.
const _: () = assert!(MOST <= 32);

/// The target of the book at `index`; 0 means no target.
#[must_use]
pub const fn target(index: usize) -> u32 {
    index as u32 + 1
}

/// How many books the shelf shows at most: as many as set, but no more than
/// fit across the screen (looking each up takes time); and how wide each place
/// is at `width`.
///
/// # Errors
/// The host's refusal.
fn columns(settings: &Settings<'_, '_>, dpi: u16, width: u16) -> Result<(usize, u16), Error> {
    let count = settings.int("count", 6)?.clamp(1, MOST as i64) as u32;
    let each = mm(dpi, COVER_MM).max(1);
    let columns = (u32::from(width) / each).clamp(1, count);
    let columns = u16::try_from(columns).unwrap_or(1).max(1);
    Ok((usize::from(columns), width / columns))
}

/// The books the shelf shows: the ones after the current book.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn books(cx: &Cx<'_>, known: &Known) -> Result<Vec<Book>, Error> {
    let (fit, _) = cx_columns(cx, cx.snapshot.display_width())?;
    native_recent::read(cx.host, known, 1, fit)
}

fn cx_columns(cx: &Cx<'_>, width: u16) -> Result<(usize, u16), Error> {
    columns(&cx.settings, cx.snapshot.display_dpi(), width)
}

/// Heights of the heading and of a title below a cover (0 where not shown).
fn labels(cx: &Cx<'_>) -> Result<(u16, u16), Error> {
    let (small, gap) = (cx.style().small, cx.style().gap);
    let label = geometry::line_px(&cx.font, small)?;
    let heading = if cx.settings.shows("heading")? {
        label.saturating_add(gap / 2)
    } else {
        0
    };
    let title = if cx.settings.shows("titles")? {
        label
    } else {
        0
    };
    Ok((heading, title))
}

/// The height of the shelf at `width`: nothing where there is no book.
///
/// # Errors
/// The host's refusals, font refusals and [`Error::NoMemory`].
pub fn height(cx: &Cx<'_>, known: &Known, width: u16) -> Result<(u16, u16), Error> {
    if books(cx, known)?.is_empty() {
        return Ok((0, 0));
    }
    let (_, cell) = cx_columns(cx, width)?;
    let (heading, title) = labels(cx)?;
    let cover = cell.saturating_sub(cx.style().gap).saturating_mul(4) / 3;
    let full = heading
        .saturating_add(cover)
        .saturating_add(title)
        .saturating_add(cx.shape.frame_height());
    // Covers a third as high, where there is less room.
    let covers = full.saturating_sub(heading.saturating_add(title));
    Ok((
        full,
        heading.saturating_add(title).saturating_add(covers / 3),
    ))
}

/// Where the parts of the shelf go: the heading, and each book's place for its
/// cover with the row of its title where titles show.
#[derive(Debug, Default)]
struct Parts {
    heading: Option<Rect>,
    places: Vec<(Rect, Option<Rect>)>,
}

/// The parts of a shelf of `count` books in `area` (inside its frame): covers
/// 3:4 as large as the columns and the height allow, the heading above and the
/// titles below; where the covers would be smaller than two lines of text, the
/// titles alone, with the heading where there is room for it.
fn parts(cx: &mut Cx<'_>, area: Rect, count: usize) -> Result<Parts, Error> {
    if count == 0 {
        return Ok(Parts::default());
    }
    let (small, gap) = (cx.style().small, cx.style().gap);
    let (fit, _) = cx_columns(cx, area.width)?;
    let columns = u16::try_from(count.clamp(1, fit)).unwrap_or(1);
    let label = geometry::line_px(&cx.font, small)?;
    let heading_text = cx.tr("home.recent")?;
    let heading_fits = cx.font.measure_text(small, &heading_text)? <= f32::from(area.width);
    let (mut heading, mut title) = labels(cx)?;
    if !heading_fits {
        heading = 0;
    }
    let covers =
        area.height.saturating_sub(heading.saturating_add(title)) >= label.saturating_mul(2);
    if !covers {
        title = label;
        if area.height < heading.saturating_add(title) {
            heading = 0;
        }
    }
    let room = area.height.saturating_sub(heading.saturating_add(title));
    // Covers 3:4, as wide as the columns or as high as the room allows.
    let column = area.width / columns;
    let (w, h) = if covers {
        let w = column.saturating_sub(gap).min(room.saturating_mul(3) / 4);
        (w, w.saturating_mul(4) / 3)
    } else {
        (column.saturating_sub(gap), 0)
    };
    let total = heading.saturating_add(h).saturating_add(title);
    let top = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    let left = area
        .x
        .saturating_add(area.width.saturating_sub(column.saturating_mul(columns)) / 2);
    let heading = (heading > 0).then(|| Rect::new(area.x, top, area.width, label));
    let top = top.saturating_add(heading.map_or(0, |_| label.saturating_add(gap / 2)));
    let mut places = Vec::new();
    for i in 0..columns {
        let start = left.saturating_add(i.saturating_mul(column));
        let cover = Rect::new(
            start.saturating_add(column.saturating_sub(w) / 2),
            top,
            w,
            h,
        );
        let row = Rect::new(
            start.saturating_add(gap / 2),
            top.saturating_add(h),
            column.saturating_sub(gap),
            title,
        );
        paperpaw_ui_kit::mem::push(&mut places, (cover, (title > 0).then_some(row)))?;
    }
    Ok(Parts { heading, places })
}

/// Where a tap on the shelf in `area` does something: each book's cover and
/// title.
///
/// # Errors
/// The host's refusals, font refusals and [`Error::NoMemory`].
pub fn targets(cx: &mut Cx<'_>, known: &Known, area: Rect) -> Result<Vec<(u32, Rect)>, Error> {
    let count = books(cx, known)?.len();
    let inner = cx.shape.look(area).area;
    let parts = parts(cx, inner, count)?;
    let mut found = Vec::new();
    for (n, (cover, title)) in parts.places.iter().take(count).enumerate() {
        let at = title.map_or(*cover, |row| cover.union(&row));
        paperpaw_ui_kit::mem::push(&mut found, (target(n), at))?;
    }
    Ok(found)
}

/// Draws the shelf in `area`; where its covers go is put in `pictures`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    (pictures, known): (&mut Pictures, &Known),
    area: Rect,
) -> Result<(), Error> {
    let books = books(cx, known)?;
    // Nothing read before the current book: nothing to show.
    if books.is_empty() {
        return Ok(());
    }
    let look = cx.shape.paint(out, area, 256)?;
    let parts = parts(cx, look.area, books.len())?;
    let align = cx.settings.align()?;
    let small = cx.style().small;
    if let Some(row) = parts.heading {
        let heading = cx.tr("home.recent")?;
        text::line(
            out,
            &cx.font,
            row,
            align,
            (small, small),
            &heading,
            look.faint,
        )?;
    }
    let covers = covers(cx, known, &books, &parts, pictures)?;
    for ((place, row), (book, covered)) in parts.places.iter().zip(books.iter().zip(&covers)) {
        match covered {
            // Covers too small to tell: the titles alone.
            _ if place.height == 0 => {}
            // In the middle of its place, at the bottom.
            Covered::Ready(cover) => {
                let at = native_recent::fitted((cover.width, cover.height), *place, 0.5, 1.0);
                pictures.place(cover.image, at)?;
            }
            Covered::Busy | Covered::None => placeholder(cx, out, &look, *place, &book.title)?,
        }
        if let Some(row) = row {
            text::line(
                out,
                &cx.font,
                *row,
                align,
                (small, small),
                &book.title,
                look.ink,
            )?;
        }
    }
    Ok(())
}

/// The covers of the books whose places have room for one.
fn covers(
    cx: &Cx<'_>,
    known: &Known,
    books: &[Book],
    parts: &Parts,
    pictures: &mut Pictures,
) -> Result<Vec<Covered>, Error> {
    let host: &Host<'_> = cx.host;
    let mut found = Vec::new();
    native_recent::with_library(host, |library| {
        for (book, (place, _)) in books.iter().zip(&parts.places) {
            let covered = if place.height > 0 {
                native_recent::cover_known(host, known, library, book)?
            } else {
                Covered::None
            };
            pictures.busy |= covered == Covered::Busy;
            paperpaw_ui_kit::mem::push(&mut found, covered)?;
        }
        Ok(())
    })?;
    Ok(found)
}

/// A book without a cover: a frame with its title in it, broken between words
/// as far as fits; where more is left than fits, the last line ends in "…".
fn placeholder(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    place: Rect,
    title: &str,
) -> Result<(), Error> {
    let (small, gap, line) = {
        let style = cx.style();
        (style.small, style.gap, style.line)
    };
    let thickness = u32::from(line).max(2);
    paint::push(out, Paint::Focus(place, 0, Some(thickness), look.ink))?;
    let pad = gap / 2;
    let (width, height) = (
        place.width.saturating_sub(2 * pad),
        place.height.saturating_sub(2 * pad),
    );
    if f32::from(width) < small * 2.0 || height < geometry::line_px(&cx.font, small)? {
        return Ok(());
    }
    let lines = Wrapped::new(&cx.font, title, small, (width, height), false)?;
    let room = Rect::new(
        place.x.saturating_add(pad),
        place.y.saturating_add(pad),
        width,
        lines.height(&cx.font)?,
    );
    lines.draw(out, &cx.font, room, Align::Left, look.ink)?;
    Ok(())
}

/// What the shelf `widget` shows, to tell when it changed.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let snapshot = capture_snapshot(host)?;
    let settings = Settings::new(host, widget);
    let (fit, _) = columns(&settings, snapshot.display_dpi(), snapshot.display_width())?;
    let listed = native_recent::read_listed(host, 1, fit)?;
    Ok(native_recent::signature(&listed))
}
