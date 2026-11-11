//! Covers in a row or a grid, each with a label below (its number in a series,
//! its title), the books read marked: what the favourites and the series
//! widgets share.
//!
//! The layout is pure geometry (`places`, `pack`); `draw` puts the paint of
//! the labels, marks and placeholders into a list and the covers the host draws
//! into [`Pictures`].
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{Error, ui::WidgetTarget},
};
use paperpaw_ui_kit::{
    Paint,
    covers::{Covered, Pictures, fitted},
    geometry, mem, paint,
    widget::{Align, Cx, Look, layout, shape, text::Wrapped},
};

/// The target of a book: this and its index among those that show.
pub const BOOK_TARGET: u32 = 0x4000_0000;
/// The most places a layout makes.
const MOST_PLACES: u32 = 64;
/// The most targets a widget declares (a stage reply holds 32).
const MOST_TARGETS: usize = 24;

/// The most covers a draw asks the host for: it keeps 8 picture ids for a
/// plugin and ends the oldest to make room, so a shelf of more shows the books
/// after these as placeholders.
pub const MOST_COVERS: usize = 8;

/// A book on the shelf, as shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Shelved {
    /// Its key in the catalog, and the catalog generation it is valid in.
    pub book: (u32, u32),
    /// Its title, for the placeholder of a book without a cover.
    pub title: String,
    /// Below its cover (its number, its title), if anything.
    pub label: String,
    /// Marked as read.
    pub read: bool,
    /// Marked as the one being read.
    pub current: bool,
}

/// Where a cover goes and its label below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub cover: Rect,
    pub label: Rect,
}

/// The sizes of a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    /// Between covers.
    pub gap: u16,
    /// A label below a cover (0: none).
    pub label: u16,
    /// The least width of a cover that can be told from another.
    pub least: u16,
}

impl Sizes {
    /// The sizes of the interface for labels `label` high.
    #[must_use]
    pub fn of(cx: &Cx<'_>, label: u16) -> Self {
        let dpi = u32::from(cx.snapshot.display_dpi());
        let least = u16::try_from((dpi * 50).div_ceil(254)).unwrap_or(u16::MAX);
        Self {
            gap: cx.style().gap,
            label,
            least: least.max(24),
        }
    }
}

/// The height of a label below a cover.
///
/// # Errors
/// Font refusals.
pub fn label_height(cx: &Cx<'_>) -> Result<u16, Error> {
    let style = cx.style();
    Ok(geometry::line_px(&cx.font, style.small)?.saturating_add(style.gap / 4))
}

/// The height of the line below the cover of the book being read, where it
/// has no label, and of the room above it.
#[must_use]
pub fn mark_height(cx: &Cx<'_>) -> u16 {
    let style = cx.style();
    style
        .line
        .saturating_mul(3)
        .max(3)
        .saturating_add((style.gap / 4).max(1))
}

/// The left edge of `width` pixels in `area`, placed as `align` says.
fn place_x(area: Rect, width: u16, align: Align) -> u16 {
    geometry::clamp_u16(align.x(area, f32::from(width)) as i64)
}

/// The places of up to `count` covers in `area`, in a row (`grid` false) or in
/// rows: as large as the area allows them, a gap between them, each with
/// `sizes.label` pixels below its cover, together in the middle (or at the
/// side `align` says). Fewer places than books where they would be too small;
/// where even one cover would be, the labels alone in a row (the covers'
/// places empty).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn places(
    area: Rect,
    count: usize,
    grid: bool,
    sizes: Sizes,
    align: Align,
) -> Result<Vec<Place>, Error> {
    let Sizes { gap, label, least } = sizes;
    if count == 0 || area.width == 0 || area.height <= label {
        return Ok(Vec::new());
    }
    let (w, h) = (u32::from(area.width), u32::from(area.height));
    let size = |columns: u32, rows: u32| -> u32 {
        let by_width = w.saturating_sub(u32::from(gap) * (columns - 1)) / columns;
        let rows_h = h.saturating_sub(u32::from(gap / 2) * (rows - 1));
        let cell_h = (rows_h / rows).saturating_sub(u32::from(label));
        by_width.min(cell_h * 2 / 3)
    };
    // A library page lists at most 32 books: never more places than that.
    let n = u32::try_from(count).unwrap_or(u32::MAX).min(MOST_PLACES);
    let count = n as usize;
    let least = u32::from(least);
    // The columns and rows that make the covers largest, all of them shown
    // where they are not too small.
    let mut best = (1, 1, 0);
    for columns in 1..=n.min(u32::from(area.width)) {
        let rows = if grid { n.div_ceil(columns) } else { 1 };
        let width = size(columns, rows);
        if width > best.2 && (grid || columns == n) {
            best = (columns, rows, width);
        }
    }
    let (mut columns, mut rows, mut width) = best;
    if width < least {
        // Fewer, each as small as allowed.
        let step = least + u32::from(gap);
        columns = ((w + u32::from(gap)) / step).max(1).min(n);
        rows = if grid {
            let row = least * 3 / 2 + u32::from(label) + u32::from(gap / 2);
            ((h + u32::from(gap / 2)) / row).max(1).min(n)
        } else {
            1
        };
        width = size(columns, rows);
    }
    if width < least && label > 0 {
        // No cover would be told from another: the labels alone.
        let cell = u16::try_from(least).unwrap_or(u16::MAX).saturating_add(gap);
        let shown = (area.width / cell.max(1))
            .max(1)
            .min(count.min(0xffff) as u16);
        let left = place_x(area, cell.saturating_mul(shown), align);
        let top = area.y.saturating_add((area.height - label) / 2);
        let mut places = Vec::new();
        places
            .try_reserve(usize::from(shown))
            .map_err(|_| Error::NoMemory)?;
        for i in 0..shown {
            let x = left.saturating_add(i.saturating_mul(cell));
            places.push(Place {
                cover: Rect::new(x.saturating_add(cell / 2), top, 0, 0),
                label: Rect::new(x, top, cell, label),
            });
        }
        return Ok(places);
    }
    let shown = columns.saturating_mul(rows).min(n);
    let cover_w = u16::try_from(width).unwrap_or(u16::MAX);
    let cover_h = u16::try_from(width.saturating_mul(3) / 2).unwrap_or(u16::MAX);
    let step = cover_w.saturating_add(gap);
    let used_columns = u16::try_from(shown.min(columns)).unwrap_or(u16::MAX);
    let row_w = step.saturating_mul(used_columns).saturating_sub(gap);
    let left = place_x(area, row_w, align);
    let row_h = cover_h.saturating_add(label);
    let used_rows = u16::try_from(shown.div_ceil(columns)).unwrap_or(u16::MAX);
    let block = row_h
        .saturating_mul(used_rows)
        .saturating_add((gap / 2).saturating_mul(used_rows.saturating_sub(1)))
        .min(area.height);
    let top = area.y.saturating_add((area.height - block) / 2);
    let right = area.x.saturating_add(area.width);
    let mut places = Vec::new();
    places
        .try_reserve(usize::try_from(shown).unwrap_or(0))
        .map_err(|_| Error::NoMemory)?;
    for i in 0..shown {
        let (col, row) = (
            u16::try_from(i % columns).unwrap_or(u16::MAX),
            u16::try_from(i / columns).unwrap_or(u16::MAX),
        );
        let x = left.saturating_add(col.saturating_mul(step));
        let y = top.saturating_add(row.saturating_mul(row_h.saturating_add(gap / 2)));
        // The label as wide as the cover and half the gap each side.
        let label_x = x.saturating_sub(gap / 2).max(area.x);
        let label_w = x
            .saturating_add(cover_w)
            .saturating_add(gap / 2)
            .min(right)
            .saturating_sub(label_x);
        places.push(Place {
            cover: Rect::new(x, y, cover_w, cover_h),
            label: Rect::new(label_x, y.saturating_add(cover_h), label_w, label),
        });
    }
    Ok(places)
}

/// The size of a cover fitted into `width` × `height`: as its picture is
/// (`shape`), or 2:3 for a book without one.
#[must_use]
pub fn cover_size(shape: Option<(u32, u32)>, width: u16, height: u16) -> (u16, u16) {
    match shape {
        Some(shape) => {
            let at = fitted(shape, Rect::new(0, 0, width, height), (0, 0));
            (at.width, at.height)
        }
        None => {
            let w = width.min(height.saturating_mul(2) / 3);
            (w, (w.saturating_mul(3) / 2).min(height))
        }
    }
}

/// The picture's shape of a book's cover, if it has one.
fn shape_of(cover: Covered) -> Option<(u32, u32)> {
    match cover {
        Covered::Ready(cover) => Some((cover.width, cover.height)),
        _ => None,
    }
}

/// Each row of `places` packed by the widths its books' covers really take (a
/// cover of another shape is narrower than its place), a gap between them, the
/// row in `area` as `align` says: covers of any shape stand evenly. Each label
/// stays under its cover.
pub fn pack(places: &mut [Place], covers: &[Covered], area: Rect, gap: u16, align: Align) {
    let right = area.x.saturating_add(area.width);
    let count = places.len().min(covers.len());
    let mut start = 0;
    while start < count {
        let Some(y) = places.get(start).map(|p| p.cover.y) else {
            break;
        };
        let end = start
            + places
                .get(start..count)
                .map_or(0, |rest| rest.iter().take_while(|p| p.cover.y == y).count());
        let (Some(row), Some(shapes)) = (places.get_mut(start..end), covers.get(start..end)) else {
            break;
        };
        start = end.max(start + 1);
        if row.iter().any(|p| p.cover.width == 0) {
            // Labels alone: as placed.
            continue;
        }
        let mut total = gap.saturating_mul(u16::try_from(row.len().saturating_sub(1)).unwrap_or(0));
        for (place, cover) in row.iter().zip(shapes) {
            total = total.saturating_add(
                cover_size(shape_of(*cover), place.cover.width, place.cover.height).0,
            );
        }
        let mut x = place_x(area, total.min(area.width), align);
        for (place, cover) in row.iter_mut().zip(shapes) {
            let w = cover_size(shape_of(*cover), place.cover.width, place.cover.height).0;
            place.cover = Rect::new(x, place.cover.y, w, place.cover.height);
            let label_x = x.saturating_sub(gap / 2).max(area.x);
            let label_w = x
                .saturating_add(w)
                .saturating_add(gap / 2)
                .min(right)
                .saturating_sub(label_x);
            place.label = Rect::new(label_x, place.label.y, label_w, place.label.height);
            x = x.saturating_add(w).saturating_add(gap);
        }
    }
}

/// Where the cover of a book goes in `place`: at its bottom (covers of other
/// shapes stand on the same line), in the middle from side to side.
#[must_use]
pub fn cover_at(place: &Place, cover: Covered) -> Rect {
    let slot = place.cover;
    let (w, h) = cover_size(shape_of(cover), slot.width, slot.height);
    Rect::new(
        slot.x.saturating_add(slot.width.saturating_sub(w) / 2),
        slot.y.saturating_add(slot.height.saturating_sub(h)),
        w,
        h,
    )
}

/// Draws `books` in their `places`: each cover at the bottom of its place, its
/// label below, a mark on those read and a line below the one being read. The
/// covers the host draws are put into `pictures`; a book without one is a
/// frame with its title inside.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    pictures: &mut Pictures,
    look: &Look,
    (places, books, covers): (&[Place], &[Shelved], &[Covered]),
) -> Result<(), Error> {
    let style = cx.style();
    for ((place, shelved), cover) in places.iter().zip(books).zip(covers) {
        let at = cover_at(place, *cover);
        draw_cover(cx, out, pictures, look, at, (&shelved.title, *cover))?;
        if shelved.read && at.width > 1 {
            draw_read_mark(out, style.small, style.line, look, at)?;
        }
        let label = place.label;
        let labelled = !shelved.label.is_empty() && label.height > 0;
        if shelved.current && !labelled {
            // A line below the cover: here one is.
            let line = style.line.saturating_mul(3).max(3);
            let gap = (style.gap / 4).max(1);
            let below = at.y.saturating_add(at.height).saturating_add(gap);
            shape::fill(out, Rect::new(at.x, below, at.width, line), look.ink)?;
        }
        if labelled {
            draw_label(cx, out, look, label, shelved)?;
        }
    }
    Ok(())
}

/// The label of `shelved` in the bottom row of `label`: the book being read has
/// its label light on dark.
fn draw_label(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    label: Rect,
    shelved: &Shelved,
) -> Result<(), Error> {
    let style = cx.style();
    let row_h = geometry::line_px(&cx.font, style.small)?.min(label.height);
    let row = Rect::new(
        label.x,
        label.y.saturating_add(label.height - row_h),
        label.width,
        row_h,
    );
    if !shelved.current {
        return layout::label(
            out,
            &cx.font,
            (style.small, Align::Center),
            row,
            &shelved.label,
            look.ink,
        );
    }
    let text =
        paperpaw_ui_kit::text::fit(&cx.font, style.small, &shelved.label, f32::from(row.width))?;
    let pad = style.gap / 2;
    let measured = cx.font.measure_text(style.small, &text)?;
    let w = geometry::clamp_u16(measured as i64)
        .saturating_add(pad.saturating_mul(2))
        .min(row.width);
    let pill = Rect::new(
        row.x.saturating_add(row.width.saturating_sub(w) / 2),
        row.y,
        w,
        row.height,
    );
    paint::push(
        out,
        Paint::Focus(pill, u32::from(row.height / 2), None, look.ink),
    )?;
    layout::label(
        out,
        &cx.font,
        (style.small, Align::Center),
        row,
        &text,
        look.paper,
    )
}

/// The cover of a book at `at`: the host's picture, or a frame with the title
/// for a book without one.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw_cover(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    pictures: &mut Pictures,
    look: &Look,
    at: Rect,
    (title, cover): (&str, Covered),
) -> Result<(), Error> {
    if at.width < 2 || at.height < 2 {
        return Ok(());
    }
    match cover {
        Covered::Ready(ready) => pictures.place(ready.image, at),
        Covered::Busy => {
            pictures.busy = true;
            draw_placeholder(cx, out, look, at, title)
        }
        _ => draw_placeholder(cx, out, look, at, title),
    }
}

/// A book without a cover: a frame with its title inside.
fn draw_placeholder(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    at: Rect,
    title: &str,
) -> Result<(), Error> {
    let style = cx.style();
    let line = u32::from(style.line.max(1));
    paint::push(
        out,
        Paint::Focus(at, style.corner.min(6), Some(line), look.ink),
    )?;
    let pad = (style.gap / 2).max(2);
    let inner = Rect::new(
        at.x.saturating_add(pad),
        at.y.saturating_add(pad),
        at.width.saturating_sub(pad.saturating_mul(2)),
        at.height.saturating_sub(pad.saturating_mul(2)),
    );
    if inner.width < 8 || inner.height < 8 {
        return Ok(());
    }
    let wrapped = Wrapped::fitting(
        &cx.font,
        title,
        (style.small, (style.small * 0.6).max(6.0)),
        (inner.width, inner.height),
        false,
    )?;
    wrapped.draw(out, &cx.font, inner, Align::Center, look.ink)?;
    Ok(())
}

/// A round mark with a tick at the top right corner of a cover: read.
fn draw_read_mark(
    out: &mut Vec<Paint>,
    small: f32,
    line: u16,
    look: &Look,
    cover: Rect,
) -> Result<(), Error> {
    let radius = u32::from(cover.width / 6)
        .min((small * 2.0 / 3.0) as u32)
        .max(5);
    let inset = (radius / 3) as i32;
    let centre = Point::new(
        i32::from(cover.x) + i32::from(cover.width) - radius as i32 - inset,
        i32::from(cover.y) + radius as i32 + inset,
    );
    // A ring of paper round it, so it stands out on any cover.
    shape::fill_circle(out, centre, radius + (radius / 4).max(1), look.paper)?;
    shape::fill_circle(out, centre, radius, look.ink)?;
    let r = radius as i32;
    let width = (radius / 4).max(1).max(u32::from(line) / 2);
    let a = Point::new(centre.x - r / 2, centre.y);
    let b = Point::new(centre.x - r / 8, centre.y + r * 3 / 8);
    let c = Point::new(centre.x + r / 2, centre.y - r * 3 / 8);
    shape::line(out, a, b, width, look.paper)?;
    shape::line(out, b, c, width, look.paper)
}

/// What a shelf widget has to show.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Shown {
    /// The name in the caption (the series), if any.
    pub name: String,
    pub books: Vec<Shelved>,
    /// How many books the whole has, and how many of them are read.
    pub total: usize,
    pub read: usize,
    /// The translation key of what is said instead of books, if there are none.
    pub empty: Option<&'static str>,
}

/// How a shelf widget lays its books out.
#[derive(Debug, Clone, Copy)]
pub struct Arrange {
    /// In rows filling the place, else in a row.
    pub grid: bool,
    /// Room below each cover: a label's, or the line of the book being read.
    pub label: u16,
    /// Which of more books than fit show: those round the book being read
    /// (its index in the books), else the first.
    pub around: Option<usize>,
}

/// The books that show out of `len` where `count` fit: the first, or those round
/// `at` (one before it, so that it is not the first on show).
#[must_use]
pub fn window(len: usize, count: usize, at: Option<usize>) -> core::ops::Range<usize> {
    let count = count.min(len);
    let start = match at {
        Some(at) => at.saturating_sub(1).min(len - count),
        None => 0,
    };
    start..start + count
}

/// The area the widget's books are in: where a caption leaves room for them.
fn content(cx: &Cx<'_>, look: &Look) -> Result<Rect, Error> {
    let style = cx.style();
    let room = layout::caption_height(&cx.font, style.small, style.gap)?;
    if cx.settings.shows("caption")? && look.area.height >= room.saturating_mul(3) {
        let by = room.min(look.area.height);
        return Ok(Rect::new(
            look.area.x,
            look.area.y.saturating_add(by),
            look.area.width,
            look.area.height - by,
        ));
    }
    Ok(look.area)
}

/// The places of the books that show and which of the books they are.
fn arrange(
    cx: &Cx<'_>,
    look: &Look,
    shown: &Shown,
    how: Arrange,
) -> Result<(Rect, Vec<Place>, core::ops::Range<usize>), Error> {
    let rest = content(cx, look)?;
    let sizes = Sizes::of(cx, how.label);
    let align = cx.settings.align()?;
    let all = places(rest, shown.books.len(), how.grid, sizes, align)?;
    let range = window(shown.books.len(), all.len(), how.around);
    Ok((rest, all, range))
}

/// Draws the widget in `area`: its frame, the caption just above the covers,
/// and the books that show (the host draws the covers: `pictures`).
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn render(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    pictures: &mut Pictures,
    area: Rect,
    (shown, how, caption): (&Shown, Arrange, &str),
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    if let Some(key) = shown.empty {
        let message = cx.tr(key)?;
        return crate::native_draw::message_in(cx, out, &look, &message);
    }
    let (rest, mut laid, range) = arrange(cx, &look, shown, how)?;
    let books = shown.books.get(range).unwrap_or_default();
    laid.truncate(books.len());
    let covers = covers_of(cx.host, books)?;
    pictures.busy = covers.contains(&Covered::Busy);
    let align = cx.settings.align()?;
    pack(&mut laid, &covers, rest, cx.style().gap, align);
    if rest != look.area {
        // The caption stays with the covers, not at the top of a tall place.
        let free = laid.first().map_or(0, |p| p.cover.y.saturating_sub(rest.y));
        let above = Rect::new(
            look.area.x,
            look.area.y.saturating_add(free),
            look.area.width,
            look.area.height.saturating_sub(free),
        );
        let style = cx.style();
        layout::caption(
            out,
            &cx.font,
            (style.small, style.gap),
            (above, align, look.faint),
            caption,
        )?;
    }
    draw(cx, out, pictures, &look, (&laid, books, &covers))
}

/// The covers of the first [`MOST_COVERS`] of `books`, as the host has them.
fn covers_of(
    host: &paperpaw_sdk::native::Host<'_>,
    books: &[Shelved],
) -> Result<Vec<Covered>, Error> {
    let mut covers = Vec::new();
    covers
        .try_reserve(books.len())
        .map_err(|_| Error::NoMemory)?;
    let library = paperpaw_sdk::native::library::Library::open(host).ok();
    let made = (|| {
        for (i, book) in books.iter().enumerate() {
            let cover = match (&library, i < MOST_COVERS) {
                (Some(library), true) => paperpaw_ui_kit::covers::cover(host, library, book.book)?,
                _ => Covered::None,
            };
            covers.push(cover);
        }
        Ok(())
    })();
    let released = library.map_or(Ok(()), |library| library.release(host));
    made?;
    released?;
    Ok(covers)
}

/// One target for each book that shows: its cover and its label together. The
/// target is [`BOOK_TARGET`] and the book's index among those that show.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn targets(
    cx: &Cx<'_>,
    area: Rect,
    (shown, how): (&Shown, Arrange),
) -> Result<Vec<WidgetTarget>, Error> {
    let look = cx.shape.look(area);
    let (_, laid, range) = arrange(cx, &look, shown, how)?;
    let mut targets = Vec::new();
    for (i, place) in laid.iter().take(range.len()).take(MOST_TARGETS).enumerate() {
        let at = place.cover.union(&place.label);
        let id = BOOK_TARGET | u32::try_from(i).unwrap_or(0);
        mem::push(&mut targets, WidgetTarget::new(id, paint::frame_rect(at)))?;
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes() -> Sizes {
        Sizes {
            gap: 8,
            label: 20,
            least: 24,
        }
    }

    #[test]
    fn a_row_of_books_stands_in_the_middle_as_large_as_it_can() {
        let area = Rect::new(0, 0, 400, 200);
        let places = places(area, 4, false, sizes(), Align::Center).unwrap();
        assert_eq!(places.len(), 4);
        let first = places.first().unwrap().cover;
        assert!(first.width > 0 && first.height == first.width * 3 / 2);
        // Four covers and three gaps, centred.
        let used = places.last().unwrap().cover.x + first.width - first.x;
        assert!(first.x + 1 >= area.width.saturating_sub(used) / 2);
        for pair in places.windows(2) {
            assert_eq!(pair[1].cover.x - (pair[0].cover.x + pair[0].cover.width), 8);
        }
    }

    #[test]
    fn a_grid_takes_rows_and_shows_fewer_where_covers_would_be_too_small() {
        let area = Rect::new(0, 0, 200, 120);
        let all = places(area, 6, true, sizes(), Align::Center).unwrap();
        assert!(!all.is_empty() && all.len() <= 6);
        let crowded = places(area, 60, true, sizes(), Align::Center).unwrap();
        assert!(crowded.len() < 60);
        assert!(crowded.iter().all(|p| p.cover.width >= 24));
    }

    #[test]
    fn where_no_cover_fits_the_labels_stand_alone() {
        let area = Rect::new(0, 0, 100, 30);
        let places = places(area, 5, false, sizes(), Align::Center).unwrap();
        assert!(!places.is_empty());
        assert!(
            places
                .iter()
                .all(|p| p.cover.width == 0 && p.label.height == 20)
        );
    }

    #[test]
    fn nothing_is_placed_in_no_room_or_for_no_books() {
        let some = Rect::new(0, 0, 100, 100);
        assert!(
            places(some, 0, false, sizes(), Align::Left)
                .unwrap()
                .is_empty()
        );
        assert!(
            places(Rect::new(0, 0, 0, 50), 3, true, sizes(), Align::Left)
                .unwrap()
                .is_empty()
        );
        assert!(
            places(Rect::new(0, 0, 50, 20), 3, true, sizes(), Align::Left)
                .unwrap()
                .is_empty()
        );
        // Extreme sizes do not panic.
        let huge = Rect::new(u16::MAX, u16::MAX, u16::MAX, u16::MAX);
        let _ = places(huge, usize::MAX, true, sizes(), Align::Right);
    }

    #[test]
    fn covers_of_other_shapes_are_packed_by_their_widths_with_labels_under_them() {
        use paperpaw_ui_kit::covers::Cover;
        let area = Rect::new(0, 0, 400, 200);
        let mut laid = places(area, 3, false, sizes(), Align::Left).unwrap();
        let slot = laid.first().unwrap().cover;
        let wide = Cover {
            image: paperpaw_sdk::native::image::ImageId::new(1).unwrap(),
            width: 300,
            height: 100,
        };
        let covers = [Covered::Ready(wide), Covered::None, Covered::Busy];
        pack(&mut laid, &covers, area, 8, Align::Left);
        // A wide cover is as wide as its place and lower than it.
        assert_eq!(laid[0].cover.width, slot.width);
        assert!(laid[0].cover.height == slot.height);
        // The next stands a gap after it and each label is under its cover.
        assert_eq!(laid[1].cover.x, laid[0].cover.x + laid[0].cover.width + 8);
        assert!(laid[1].label.x <= laid[1].cover.x);
    }

    #[test]
    fn a_cover_is_as_its_picture_is_or_two_by_three() {
        assert_eq!(cover_size(None, 100, 150), (100, 150));
        assert_eq!(cover_size(None, 100, 90), (60, 90));
        assert_eq!(cover_size(Some((200, 100)), 100, 150), (100, 50));
        assert_eq!(cover_size(None, 0, 0), (0, 0));
        let _ = cover_size(Some((u32::MAX, 0)), u16::MAX, u16::MAX);
    }
}
