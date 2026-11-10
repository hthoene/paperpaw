//! "Now reading": the book being read, its cover, title and author, the page it
//! is at, a bar of how far, the chapter and the time left to read it at one's
//! speed.
//!
//! The book is the library's (the one opened last that is still on the card,
//! see [`native_current`](crate::native_current)); the page, the chapter and the
//! speed are what [`native_recorder`](crate::native_recorder) wrote down of
//! the reading events. Where there is too little room the parts give way in
//! the order [`DROPPED`] says; the title never.
use crate::native_current::{Cache, Current, covered, percent_text, read, shape_of};
use crate::native_recorder::{Position, Recorder};
use crate::native_shelf::{BOOK_TARGET, cover_size, draw_cover};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{Error, Host, formatting::Arg, ui::WidgetTarget},
};
use paperpaw_ui_kit::{
    Paint,
    covers::Pictures,
    geometry, mem, paint,
    text::copy,
    widget::{
        Cx, Look,
        text::{Wrapped, line as one_line},
    },
};

/// The callback token of the widget as declared.
pub const TOKEN: u32 = 120;
/// The target that opens the library, where no book is read.
pub const LIBRARY_TARGET: u32 = 1;
const CAPTION: &str = "widgets.reading.caption";
const NOTHING: &str = "widgets.books.nothing-read";

/// The parts of the widget beside the cover, in the order they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Caption,
    Author,
    Bar,
    Pages,
    Left,
    Chapter,
}

/// The parts in the order they give way: the time left in the book first, the
/// heading and the author (known anyway) next, then the bar (the percentage
/// says it too), the chapter, the page last.
const DROPPED: [Part; 6] = [
    Part::Left,
    Part::Caption,
    Part::Author,
    Part::Bar,
    Part::Chapter,
    Part::Pages,
];

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

/// What the widget shows moves with the catalog and with the position
/// recorded.
///
/// # Errors
/// The host's refusals.
pub fn shows_with(host: &Host<'_>, widget: &str, recorder: &Recorder) -> Result<u64, Error> {
    let books = crate::native_current::shows(host, widget)?;
    Ok(books.wrapping_add(u64::from(recorder.revision()).wrapping_mul(0x9E37_79B9_7F4A_7C15)))
}

/// The sizes the layout goes by.
struct Metrics {
    /// A small line, and the room it takes with the gap after it.
    row: u16,
    step: u16,
    /// The bar, and the room it takes with the gap round it.
    bar: u16,
    bar_room: u16,
    /// What sets the chapter apart from the page and the time.
    apart: u16,
    /// The smallest line the title is set in.
    title_least: f32,
    title_line: u16,
}

fn metrics(cx: &Cx<'_>) -> Result<Metrics, Error> {
    let style = cx.style();
    let row = geometry::line_px(&cx.font, style.small)?;
    let bar = style.line.saturating_mul(3).max(4);
    let title_least = style.small * 0.8;
    Ok(Metrics {
        row,
        step: row.saturating_add(style.gap / 4),
        bar,
        bar_room: bar.saturating_add(style.gap),
        apart: style.gap / 2,
        title_least,
        title_line: geometry::line_px(&cx.font, title_least)?,
    })
}

/// How high `parts` are together, the title left out.
fn parts_height(m: &Metrics, parts: &[Part]) -> u16 {
    let mut total = 0_u16;
    for part in parts {
        let own = if *part == Part::Bar {
            m.bar_room
        } else {
            m.step
        };
        total = total.saturating_add(own);
    }
    let book = parts.iter().any(|p| matches!(p, Part::Pages | Part::Left));
    if book && parts.contains(&Part::Chapter) {
        total = total.saturating_add(m.apart);
    }
    total
}

/// What the text says besides the title.
struct Texts {
    caption: String,
    author: String,
    /// The page and percentage, the way that fits first.
    pages: String,
    left: Option<String>,
}

/// "About 2 h 5 min left", "About 40 min left".
fn left_text(cx: &mut Cx<'_>, seconds: u64) -> Result<String, Error> {
    let minutes = i64::try_from(seconds.div_ceil(60).max(1)).unwrap_or(i64::MAX);
    let (hours, minutes) = (minutes / 60, minutes % 60);
    if hours == 0 {
        cx.format(
            "widgets.reading.left-minutes",
            &[Arg::Int {
                name: "minutes",
                value: minutes,
            }],
        )
    } else {
        cx.format(
            "widgets.reading.left-hours",
            &[
                Arg::Int {
                    name: "hours",
                    value: hours,
                },
                Arg::Int {
                    name: "minutes",
                    value: minutes,
                },
            ],
        )
    }
}

/// "Page 34 of 210 · 16 %", "34/210 · 16 %" or "34/210" - the longest that
/// fits `width` - or the percentage alone while the pages are not counted.
fn pages_text(
    cx: &mut Cx<'_>,
    book: &Current,
    position: &Position,
    width: u16,
) -> Result<String, Error> {
    let percent = percent_text(cx, book.percent)?;
    let (Some(page), Some(pages)) = (position.page, position.pages) else {
        return cx.format(
            "widgets.reading.percent",
            &[Arg::Text {
                name: "percent",
                value: &percent,
            }],
        );
    };
    let (shown, count) = (cx.number(i64::from(page))?, cx.number(i64::from(pages))?);
    let long = cx.format(
        "widgets.reading.page",
        &[
            Arg::Int {
                name: "page",
                value: i64::from(page),
            },
            Arg::Int {
                name: "pages",
                value: i64::from(pages),
            },
        ],
    )?;
    let long = mem::join(&[long, copy(&percent)?], " · ")?;
    let small = cx.style().small;
    let fits = |text: &str| Ok::<_, Error>(cx.font.measure_text(small, text)? <= f32::from(width));
    if fits(&long)? {
        return Ok(long);
    }
    let short = mem::join(&[shown, count], "/")?;
    let both = mem::join(&[copy(&short)?, percent], " · ")?;
    Ok(if fits(&both)? || !fits(&short)? {
        both
    } else {
        short
    })
}

/// The parts that are there and wanted, in the order they are drawn.
fn wanted(
    cx: &Cx<'_>,
    book: &Current,
    position: &Position,
    left: bool,
) -> Result<Vec<Part>, Error> {
    let mut parts = Vec::new();
    for (part, name, there) in [
        (Part::Caption, "caption", true),
        (Part::Author, "author", !book.author.is_empty()),
        (Part::Bar, "bar", true),
        (Part::Pages, "pages", true),
        (Part::Left, "left", left),
        (Part::Chapter, "chapter", !position.chapter.is_empty()),
    ] {
        if there && cx.settings.shows(name)? {
            mem::push(&mut parts, part)?;
        }
    }
    Ok(parts)
}

/// The place beside the cover, if it is shown, and the cover's.
fn places(
    cx: &Cx<'_>,
    area: Rect,
    covered: Option<(u32, u32)>,
) -> Result<(Option<Rect>, Rect), Error> {
    let style = cx.style();
    let (w, h) = cover_size(covered, area.width / 3, area.height);
    let room = area.width.saturating_sub(w.saturating_add(style.gap));
    if !cx.settings.shows("cover")? || f32::from(room) < style.small * 7.0 {
        return Ok((None, area));
    }
    let at = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(h) / 2),
        w,
        h,
    );
    let rest = Rect::new(
        area.x.saturating_add(area.width - room),
        area.y,
        room,
        area.height,
    );
    Ok((Some(at), rest))
}

/// The parts that stay in `rest`, and the title set in what they leave.
fn fit(
    cx: &Cx<'_>,
    m: &Metrics,
    mut parts: Vec<Part>,
    (rest, title): (Rect, &str),
    title_size: f32,
) -> Result<(Vec<Part>, Wrapped), Error> {
    for drop in DROPPED {
        if parts_height(m, &parts).saturating_add(m.title_line) <= rest.height {
            break;
        }
        parts.retain(|part| *part != drop);
    }
    let room = rest.height.saturating_sub(parts_height(m, &parts));
    let largest = title_size.max(m.title_least);
    let wrapped = Wrapped::fitting(
        &cx.font,
        title,
        (largest, m.title_least),
        (rest.width, room),
        true,
    )?;
    Ok((parts, wrapped))
}

/// A line of small text in `at`.
fn small_line(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    (at, align): (Rect, paperpaw_ui_kit::widget::Align),
    text: &str,
    color: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let small = cx.style().small;
    one_line(out, &cx.font, at, align, (small, small), text, color)?;
    Ok(())
}

/// A bar `area` across, filled as far as `percent`: a fine line with a round
/// fill over it.
fn draw_bar(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    percent: u8,
) -> Result<(), Error> {
    let h = area.height.max(2);
    let fine = cx.style().line.max(1).min(h);
    let middle = area.y.saturating_add((h - fine) / 2);
    paint::push(
        out,
        Paint::Fill(Rect::new(area.x, middle, area.width, fine), look.ink),
    )?;
    let done =
        u16::try_from(u32::from(area.width) * u32::from(percent.min(100)) / 100).unwrap_or(0);
    if done > 0 {
        let done = done.max(h).min(area.width);
        paint::push(
            out,
            Paint::Focus(
                Rect::new(area.x, area.y, done, h),
                u32::from(h / 2),
                None,
                look.ink,
            ),
        )?;
    }
    Ok(())
}

/// What a drawing of the text beside the cover needs.
struct Scene<'a> {
    book: &'a Current,
    look: &'a Look,
    texts: &'a Texts,
    parts: &'a [Part],
    title: &'a Wrapped,
}

/// The text in `rest`, in the middle of it and placed as the widget aligns.
fn draw_text(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    m: &Metrics,
    scene: &Scene<'_>,
    (rest, chapter): (Rect, &str),
) -> Result<(), Error> {
    let (look, parts) = (scene.look, scene.parts);
    let align = cx.settings.align()?;
    let title_h = scene.title.height(&cx.font)?;
    let total = title_h
        .saturating_add(parts_height(m, parts))
        .min(rest.height);
    let mut y = rest.y.saturating_add(rest.height.saturating_sub(total) / 2);
    let row_at = |y: u16| Rect::new(rest.x, y, rest.width, m.row);
    let has = |part: Part| parts.contains(&part);
    if has(Part::Caption) {
        small_line(
            cx,
            out,
            (row_at(y), align),
            &scene.texts.caption,
            look.faint,
        )?;
        y = y.saturating_add(m.step);
    }
    let at = Rect::new(rest.x, y, rest.width, title_h);
    scene.title.draw(out, &cx.font, at, align, look.ink)?;
    y = y.saturating_add(title_h);
    if has(Part::Author) {
        small_line(cx, out, (row_at(y), align), &scene.texts.author, look.faint)?;
        y = y.saturating_add(m.step);
    }
    if has(Part::Bar) {
        let bar = Rect::new(rest.x, y.saturating_add(m.bar_room / 3), rest.width, m.bar);
        draw_bar(cx, out, look, bar, scene.book.percent)?;
        y = y.saturating_add(m.bar_room);
    }
    if has(Part::Pages) {
        small_line(cx, out, (row_at(y), align), &scene.texts.pages, look.ink)?;
        y = y.saturating_add(m.step);
    }
    if let (true, Some(left)) = (has(Part::Left), &scene.texts.left) {
        small_line(cx, out, (row_at(y), align), left, look.faint)?;
        y = y.saturating_add(m.step);
    }
    if has(Part::Chapter) {
        if has(Part::Pages) || has(Part::Left) {
            y = y.saturating_add(m.apart);
        }
        small_line(cx, out, (row_at(y), align), chapter, look.ink)?;
    }
    Ok(())
}

/// Draws the widget in `area`; the cover the host draws goes into `pictures`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw_with(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    (cache, pictures, recorder): (&mut Cache, &mut Pictures, &mut Recorder),
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let host = cx.host;
    let Some(book) = read(host, cache)? else {
        let message = cx.tr(NOTHING)?;
        return crate::native_draw::message_in(cx, out, &look, &message);
    };
    let position = recorder.of(host, &book.path)?;
    let found = covered(cx, book, pictures)?;
    let (cover, rest) = places(cx, look.area, shape_of(found))?;
    if let Some(at) = cover {
        draw_cover(cx, out, pictures, &look, at, (&book.title, found))?;
    }
    let left = match position.left() {
        Some(seconds) if cx.settings.shows("left")? => Some(left_text(cx, seconds)?),
        _ => None,
    };
    let m = metrics(cx)?;
    let parts = wanted(cx, book, &position, left.is_some())?;
    let title_size = cx.settings.size()?.text(cx.style());
    let (parts, title) = fit(cx, &m, parts, (rest, &book.title), title_size)?;
    let texts = Texts {
        caption: cx.tr(CAPTION)?,
        author: copy(&book.author)?,
        pages: pages_text(cx, book, &position, rest.width)?,
        left,
    };
    let scene = Scene {
        book,
        look: &look,
        texts: &texts,
        parts: &parts,
        title: &title,
    };
    draw_text(cx, out, &m, &scene, (rest, &position.chapter))
}

/// Draws the widget in `area` without its cover, the book read afresh.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    draw_with(
        cx,
        out,
        area,
        (
            &mut Cache::default(),
            &mut Pictures::default(),
            &mut Recorder::default(),
        ),
    )
}

/// The widget's one target: a tap opens the book, or the library where none was
/// read.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn targets_with(
    cx: &Cx<'_>,
    area: Rect,
    cache: &mut Cache,
) -> Result<Vec<WidgetTarget>, Error> {
    let id = if read(cx.host, cache)?.is_some() {
        BOOK_TARGET
    } else {
        LIBRARY_TARGET
    };
    let mut targets = Vec::new();
    mem::push(&mut targets, WidgetTarget::new(id, paint::frame_rect(area)))?;
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes() -> Metrics {
        Metrics {
            row: 14,
            step: 16,
            bar: 6,
            bar_room: 14,
            apart: 4,
            title_least: 12.0,
            title_line: 14,
        }
    }

    #[test]
    fn the_parts_take_a_row_each_the_bar_its_room_and_the_chapter_a_gap() {
        let m = sizes();
        assert_eq!(parts_height(&m, &[]), 0);
        assert_eq!(parts_height(&m, &[Part::Caption, Part::Author]), 32);
        assert_eq!(parts_height(&m, &[Part::Bar]), 14);
        assert_eq!(parts_height(&m, &[Part::Chapter]), 16);
        assert_eq!(parts_height(&m, &[Part::Pages, Part::Chapter]), 36);
    }

    #[test]
    fn every_part_but_the_title_can_give_way_and_the_page_goes_last() {
        for part in [
            Part::Caption,
            Part::Author,
            Part::Bar,
            Part::Pages,
            Part::Left,
            Part::Chapter,
        ] {
            assert!(DROPPED.contains(&part), "{part:?}");
        }
        assert_eq!(DROPPED.first(), Some(&Part::Left));
        assert_eq!(DROPPED.last(), Some(&Part::Pages));
    }
}
