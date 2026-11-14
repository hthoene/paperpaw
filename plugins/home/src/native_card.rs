//! The continue card, as the home screen draws it: the book opened last, its
//! cover on the left where there is one, and beside it the caption, the title as
//! large as fits in up to two lines, and how far it is read, each where there
//! is room for it (the caption is left out first). Where nothing was read yet
//! the card says where the books are, and a tap opens the library.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{Error, fonts::Font, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint, geometry, icons, paint,
    widget::{
        Cx,
        look::Align,
        text::{self, Wrapped},
    },
};

use crate::native_recent::{self, Book, Covered, Known, Pictures};

/// The card's one target: a tap on it opens the book, or the library.
pub const TARGET: u32 = 1;

/// The height the card takes: three rows of the interface (its frame inside).
#[must_use]
pub fn height(cx: &Cx<'_>) -> u16 {
    (f32::from(cx.style().row) * 3.0) as u16
}

/// Where the parts of the card go in its place.
#[derive(Debug, Default)]
struct Parts {
    /// Where the cover goes, if it shows.
    cover: Option<Rect>,
    /// The column of text; none where the cover alone fits.
    text: Option<Rect>,
    /// "Continue reading" above the title.
    caption: bool,
    /// The title, in one line or two.
    title: Wrapped,
    /// The percentage and the bar below.
    progress: bool,
    /// Between the rows: half a gap, or none where that keeps the caption.
    gap: u16,
}

/// The parts of `book`'s card in `room` (inside its frame): the cover on the
/// left where the text keeps room enough beside it (else the text alone, or the
/// cover alone where not even the text has room); the title as large as fits
/// in up to two lines, then the progress and the caption where there is room
/// for them too.
fn parts(
    cx: &mut Cx<'_>,
    room: Rect,
    title: &str,
    cover: Option<(u32, u32)>,
) -> Result<Parts, Error> {
    let (text_size, small, title_size, gap) = {
        let style = cx.style();
        (style.text, style.small, style.title, style.gap)
    };
    // A few words of a title at the body's size.
    let least = (text_size * 4.0) as u16;
    let mut parts = Parts::default();
    let mut text = room;
    if let Some(size) = cover.filter(|_| cx.settings.shows("cover").unwrap_or(true)) {
        let place = Rect::new(room.x, room.y, room.width / 3, room.height);
        let at = native_recent::fitted(size, place, 0.0, 0.5);
        let beside = room.width.saturating_sub(at.width.saturating_add(gap));
        if beside >= least {
            parts.cover = Some(at);
            text = Rect::new(
                room.x.saturating_add(at.width).saturating_add(gap),
                room.y,
                beside,
                room.height,
            );
        } else if room.width < least {
            // No room for text: the cover alone, in the middle.
            parts.cover = Some(native_recent::fitted(size, room, 0.5, 0.5));
            return Ok(parts);
        }
    }
    let small_line = geometry::line_px(&cx.font, small)?;
    let line = geometry::line_px(&cx.font, text_size)?;
    let caption = cx.tr("home.continue")?;
    let caption_fits = cx.font.measure_text(small, &caption)? <= f32::from(text.width);
    // Half a gap between the rows; none where only that keeps all three (the
    // small rows have room above their letters anyway).
    let all = |gap: u16| {
        caption_fits
            && line.saturating_add(2u16.saturating_mul(gap.saturating_add(small_line)))
                <= text.height
    };
    let half = if all(gap / 2) || !all(0) { gap / 2 } else { 0 };
    parts.gap = half;
    let mut left = text.height;
    parts.progress = cx.settings.shows("progress").unwrap_or(true)
        && left >= line.saturating_add(half).saturating_add(small_line);
    if parts.progress {
        left = left.saturating_sub(half.saturating_add(small_line));
    }
    parts.caption = caption_fits && left >= line.saturating_add(half).saturating_add(small_line);
    if parts.caption {
        left = left.saturating_sub(half.saturating_add(small_line));
    }
    // Two lines at most; in a flat card, down to the small size.
    let title_line = geometry::line_px(&cx.font, title_size)?;
    let heading_step = geometry::clamp_u16((f32::from(title_line) * 0.9) as i64);
    let two = left.min(title_line.saturating_add(heading_step));
    let down_to = if left >= line { text_size } else { small };
    parts.title = Wrapped::fitting(
        &cx.font,
        title,
        (title_size, down_to),
        (text.width, two),
        true,
    )?;
    parts.text = Some(text);
    Ok(parts)
}

/// The height the column of text takes.
fn column(cx: &Cx<'_>, parts: &Parts) -> Result<u16, Error> {
    let small = geometry::line_px(&cx.font, cx.style().small)?.saturating_add(parts.gap);
    Ok(parts
        .title
        .height(&cx.font)?
        .saturating_add(if parts.caption { small } else { 0 })
        .saturating_add(if parts.progress { small } else { 0 }))
}

/// Draws the card in `area`; where its cover goes is put in `pictures`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    icons: Option<&Font<'_>>,
    (pictures, known): (&mut Pictures, &Known),
    area: Rect,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let host = cx.host;
    let Some(book) = native_recent::read(host, known, 0, 1)?.into_iter().next() else {
        return empty(cx, out, icons, &look);
    };
    let covered = if cx.settings.shows("cover").unwrap_or(true) {
        native_recent::with_library(host, |library| {
            native_recent::cover_known(host, known, library, &book)
        })?
    } else {
        Covered::None
    };
    let size = match covered {
        Covered::Ready(cover) => Some((cover.width, cover.height)),
        Covered::Busy => {
            pictures.busy = true;
            None
        }
        Covered::None => None,
    };
    let parts = parts(cx, look.area, &book.title, size)?;
    if let (Some(at), Covered::Ready(cover)) = (parts.cover, covered) {
        pictures.place(cover.image, at)?;
    }
    let Some(room) = parts.text else {
        return Ok(());
    };
    text_column(cx, out, &look, &parts, room, &book)
}

/// The caption, the title and the progress together in the middle of `room`.
fn text_column(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::look::Look,
    parts: &Parts,
    room: Rect,
    book: &Book,
) -> Result<(), Error> {
    let (small, gap, line_px) = {
        let style = cx.style();
        (style.small, parts.gap, style.line.max(1))
    };
    let label = geometry::line_px(&cx.font, small)?;
    let total = column(cx, parts)?;
    let mut y = room.y.saturating_add(room.height.saturating_sub(total) / 2);
    let align = cx.settings.align()?;
    let row = |y: u16, height: u16| Rect::new(room.x, y, room.width, height);
    if parts.caption {
        let caption = cx.tr("home.continue")?;
        text::line(
            out,
            &cx.font,
            row(y, label),
            align,
            (small, small),
            &caption,
            look.faint,
        )?;
        y = y.saturating_add(label.saturating_add(gap));
    }
    let title = parts.title.height(&cx.font)?;
    parts
        .title
        .draw(out, &cx.font, row(y, title), align, look.ink)?;
    y = y.saturating_add(title.saturating_add(gap));
    if !parts.progress {
        return Ok(());
    }
    let percent = cx.format(
        "home.percent",
        &[Arg::Int {
            name: "percent",
            value: i64::from(book.progress),
        }],
    )?;
    let at = row(y, label);
    text::line(
        out,
        &cx.font,
        at,
        Align::Left,
        (small, small),
        &percent,
        look.faint,
    )?;
    // The bar beside it: an outline, filled as far as the book is read.
    let bar = u16::try_from(u32::from(line_px) * 4)
        .unwrap_or(u16::MAX)
        .max(3);
    let taken = cx.font.measure_text(small, &percent)? + f32::from(cx.style().gap);
    let x = f32::from(room.x) + taken;
    let width = (f32::from(room.width) - taken).max(1.0);
    let (left, width) = (
        geometry::clamp_u16(x as i64),
        geometry::clamp_u16(width as i64),
    );
    let top = geometry::clamp_u16(i64::from(y) + i64::from(label) / 2 - i64::from(bar) / 2);
    let outline = Rect::new(left, top, width, bar);
    paint::push(
        out,
        Paint::Focus(outline, 0, Some(u32::from(line_px)), look.ink),
    )?;
    let filled = geometry::clamp_u16(i64::from(width) * i64::from(book.progress.min(100)) / 100);
    paint::push(
        out,
        Paint::Fill(Rect::new(left, top, filled, bar), look.ink),
    )
}

/// What the card says where nothing was read yet: an icon, a title and a text,
/// together a little above the middle, in the widget's own ink.
fn empty(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    icons: Option<&Font<'_>>,
    look: &paperpaw_ui_kit::widget::look::Look,
) -> Result<(), Error> {
    let (text_size, small, title_size, gap) = {
        let style = cx.style();
        (style.text, style.small, style.title, style.gap)
    };
    let area = look.area;
    let title = cx.tr("home.start.title")?;
    let message = cx.tr("home.start.text")?;
    let words = Wrapped::fitting(
        &cx.font,
        &title,
        (title_size, small),
        (area.width, area.height),
        true,
    )?;
    let title_h = words.height(&cx.font)?;
    let below = area.height.saturating_sub(title_h.saturating_add(gap));
    let text = Wrapped::fitting(
        &cx.font,
        &message,
        (text_size, small),
        (area.width, below),
        false,
    )?;
    let text_h = text.height(&cx.font)?;
    let icon_size = title_size * 2.4;
    let icon_h = icon_size as u16;
    let lines = title_h.saturating_add(if text_h > 0 {
        gap.saturating_add(text_h)
    } else {
        0
    });
    let glyph = icons::glyph(cx.host, "books")?.filter(|_| icons.is_some());
    // The icon where there is room for it too.
    let with_icon =
        glyph.is_some() && lines.saturating_add(gap).saturating_add(icon_h) <= area.height;
    let total = lines.saturating_add(if with_icon {
        gap.saturating_add(icon_h)
    } else {
        0
    });
    let mut y = area
        .y
        .saturating_add(area.height.saturating_sub(total).saturating_mul(2) / 5);
    if let (true, Some(glyph), Some(font)) = (with_icon, glyph.as_ref(), icons) {
        let row = Rect::new(area.x, y, area.width, icon_h);
        icons::draw_centred(out, font, glyph, icon_size, row, look.faint)?;
        y = y.saturating_add(icon_h).saturating_add(gap);
    }
    let row = |y: u16, height: u16| Rect::new(area.x, y, area.width, height);
    words.draw(out, &cx.font, row(y, title_h), Align::Center, look.ink)?;
    y = y.saturating_add(title_h).saturating_add(gap);
    if text_h > 0 {
        text.draw(out, &cx.font, row(y, text_h), Align::Center, look.faint)?;
    }
    Ok(())
}

/// What the card shows, to tell when it changed.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn shows(host: &paperpaw_sdk::native::Host<'_>) -> Result<u64, Error> {
    Ok(native_recent::signature(&native_recent::read_listed(
        host, 0, 1,
    )?))
}
