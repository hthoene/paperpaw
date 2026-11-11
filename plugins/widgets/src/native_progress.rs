//! "Book progress": the book being read, its cover large and beside it a ring
//! round the share read, the percentage inside; or the ring round a small cover,
//! the ring alone, or a bar below the title.
//!
//! The book is the library's (the one opened last that is still on the card,
//! see [`native_current`](crate::native_current)); its cover is the host's
//! picture. Where the place is too small for a ring the percentage stands alone.
use crate::native_battery_shapes::ring;
use crate::native_block::{self as block, Block};
use crate::native_books::caption;
use crate::native_current::{Cache, Current, covered, percent_text, read, shape_of};
use crate::native_shelf::{BOOK_TARGET, cover_size, draw_cover};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{Error, Host, ui::WidgetTarget},
};
use paperpaw_ui_kit::{
    Paint,
    covers::{Covered, Pictures},
    geometry, mem, paint, text,
    widget::{
        Align, Cx, Look,
        text::{ink_baseline, largest_size, line as one_line},
    },
};

/// The callback token of the widget as declared.
pub const TOKEN: u32 = 119;
/// The target that opens the library, where no book is read.
pub const LIBRARY_TARGET: u32 = 1;
/// The caption, and what is said where no book was read.
const CAPTION: &str = "widgets.progress.caption";
const NOTHING: &str = "widgets.books.nothing-read";
/// The thin and the thick ring or bar.
const THIN: &str = "widgets.books.thin";
const THICK: &str = "widgets.books.thick";

/// How the progress shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// The cover large, a ring beside it.
    Beside,
    /// A ring round a small cover.
    Round,
    /// The ring alone.
    Ring,
    /// A bar below the title.
    Bar,
}

fn style_of(value: &str) -> Style {
    match value {
        "widgets.progress.round" => Style::Round,
        "widgets.progress.ring" => Style::Ring,
        "widgets.progress.bar" => Style::Bar,
        _ => Style::Beside,
    }
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

/// How thick the ring or the bar is in a place `size` pixels across.
fn thickness(cx: &Cx<'_>, size: u16) -> Result<u32, Error> {
    let share = cx.settings.with_text("thickness", |value| match value {
        THIN => 16,
        THICK => 6,
        _ => 10,
    })?;
    Ok((u32::from(size) / share)
        .max(u32::from(cx.style().line) * 2)
        .max(2))
}

/// What of the book the widget says: its title and author as the settings show
/// them, and a lead line of `lead` where there is one.
fn block_of(cx: &Cx<'_>, book: &Current, lead: Option<String>) -> Result<Block, Error> {
    let title = if cx.settings.shows("title")? {
        Some(text::copy(&book.title)?)
    } else {
        None
    };
    let mut notes = Vec::new();
    if cx.settings.shows("author")? && !book.author.is_empty() {
        mem::push(&mut notes, text::copy(&book.author)?)?;
    }
    Ok(Block { lead, title, notes })
}

/// Where the ring goes and the text beside or below it.
#[derive(Debug)]
struct RingLayout {
    ring: Rect,
    /// The text's place, and whether it is beside the ring (else below).
    text: Option<(Rect, bool)>,
}

/// The ring and the text in `area`: the ring left with the text beside it where
/// the place is wide, else the ring above the text (the text what fits in half
/// the place, or its first line), else the ring alone.
fn ring_layout(
    cx: &Cx<'_>,
    area: Rect,
    (block, title_size): (&Block, f32),
) -> Result<RingLayout, Error> {
    let style = cx.style();
    let gap = style.gap;
    let align = cx.settings.align()?;
    let x_of = |width: u16| geometry::clamp_u16(align.x(area, f32::from(width)) as i64);
    if area.width >= area.height.saturating_mul(3) / 2 {
        let d = area.height.min(area.width.saturating_mul(2) / 5);
        let room = area
            .width
            .saturating_sub(d.saturating_add(gap.saturating_mul(3) / 2));
        if f32::from(room) >= style.small * 6.0 && !block.is_empty() {
            return Ok(RingLayout {
                ring: Rect::new(
                    area.x,
                    area.y.saturating_add(area.height.saturating_sub(d) / 2),
                    d,
                    d,
                ),
                text: Some((
                    Rect::new(
                        area.x.saturating_add(area.width - room),
                        area.y,
                        room,
                        area.height,
                    ),
                    true,
                )),
            });
        }
    }
    for budget in [area.height / 2, geometry::line_px(&cx.font, style.text)?] {
        let text_h = block::height(cx, block, (area.width, budget), title_size)?;
        let below = text_h.saturating_add(gap / 2);
        let d = area.width.min(area.height.saturating_sub(below));
        if (d >= area.height / 2 || d >= area.width) && d > 0 && !block.is_empty() {
            let top = area.y.saturating_add(
                area.height
                    .saturating_sub(d.saturating_add(below).min(area.height))
                    / 2,
            );
            return Ok(RingLayout {
                ring: Rect::new(x_of(d), top, d, d),
                text: Some((
                    Rect::new(
                        area.x,
                        top.saturating_add(d).saturating_add(gap / 2),
                        area.width,
                        text_h,
                    ),
                    false,
                )),
            });
        }
    }
    let d = area.width.min(area.height);
    Ok(RingLayout {
        ring: Rect::new(
            x_of(d),
            area.y.saturating_add(area.height.saturating_sub(d) / 2),
            d,
            d,
        ),
        text: None,
    })
}

/// The radius of what a ring round `at` leaves inside it.
fn ring_inner(cx: &Cx<'_>, at: Rect) -> Result<u32, Error> {
    let side = at.width.min(at.height);
    let radius = u32::from(side / 2);
    if radius < 4 {
        return Ok(0);
    }
    Ok(radius.saturating_sub(thickness(cx, side)? + (radius / 8).max(1)))
}

/// The ring round `at` filled as far as `percent`, with the percentage inside
/// where `inside`; returns the radius of what is inside it.
fn draw_ring_in(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    at: Rect,
    (percent, inside): (u8, bool),
) -> Result<u32, Error> {
    let side = at.width.min(at.height);
    let radius = u32::from(side / 2);
    if radius < 4 {
        return Ok(0);
    }
    let centre = Point::new(
        i32::from(at.x) + radius as i32,
        i32::from(at.y) + radius as i32,
    );
    let thick = thickness(cx, side)?;
    ring(
        out,
        look.ink,
        (centre, radius, thick),
        percent,
        cx.style().line,
    )?;
    let inner = ring_inner(cx, at)?;
    if inside && inner >= 8 {
        ring_percent(cx, out, look, (centre, inner), percent)?;
    }
    Ok(inner)
}

/// The percentage in the middle of a ring: the digits as large as fit inside
/// and a smaller sign after them.
fn ring_percent(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    (centre, inner): (Point, u32),
    percent: u8,
) -> Result<(), Error> {
    let number = cx.number(i64::from(percent))?;
    let small = cx.style().small;
    let room = inner as f32 * 1.5;
    let least = small * 0.6;
    let sign = |size: f32| (size * 0.45).max(least * 0.8);
    let font = &cx.font;
    let width = |size: f32| -> Result<f32, Error> {
        Ok(font.measure_text(size, &number)? + font.measure_text(sign(size), "%")?)
    };
    let size = largest_size((inner as f32 * 0.8).max(least), least, |s| {
        if width(s)? > room {
            return Ok(false);
        }
        Ok(match font.ink(s, &number)? {
            Some(ink) => {
                let (top, bottom) = geometry::check_ink(ink.top, ink.bottom)?;
                (bottom - top) as u32 <= inner
            }
            None => true,
        })
    })?
    .unwrap_or(least);
    let total = width(size)?;
    let x = centre.x as f32 - total / 2.0;
    let band = Rect::new(
        0,
        geometry::clamp_u16(i64::from(centre.y) - i64::from(inner)),
        1,
        geometry::clamp_u16(i64::from(inner) * 2),
    );
    let baseline = ink_baseline(font, size, &number, band)?;
    push_text(out, &number, (size, x, baseline), look.ink)?;
    let x = x + font.measure_text(size, &number)?;
    push_text(out, "%", (sign(size), x, baseline), look.faint)
}

fn push_text(
    out: &mut Vec<Paint>,
    shown: &str,
    (size, x, baseline): (f32, f32, i32),
    color: paperpaw_sdk::Color,
) -> Result<(), Error> {
    crate::native_books::push_text(out, shown, (size, x, baseline), color)
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
    let fine = cx.style().line.clamp(1, h);
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

/// The percentage alone, as large as the place allows.
fn percent_alone(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    percent: u8,
) -> Result<(), Error> {
    let shown = percent_text(cx, percent)?;
    let style = cx.style();
    let (max, small) = (style.title * 2.0, style.small);
    let align = cx.settings.align()?;
    one_line(out, &cx.font, area, align, (max, small), &shown, look.ink)?;
    Ok(())
}

/// The percentage large with the title and author beside it, in a place wide and
/// low.
fn percent_beside(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    (book, title_size): (&Current, f32),
) -> Result<(), Error> {
    let shown = percent_text(cx, book.percent)?;
    let (small, title, gap) = {
        let style = cx.style();
        (style.small, style.title, style.gap)
    };
    let size = largest_size(title * 2.0, small, |s| {
        Ok(geometry::line_px(&cx.font, s)? <= area.height)
    })?
    .unwrap_or(small);
    let percent_w = geometry::clamp_u16((cx.font.measure_text(size, &shown)? + 1.0) as i64);
    let rest = area.width.saturating_sub(percent_w.saturating_add(gap));
    let text = block_of(cx, book, None)?;
    let text_w = if text.is_empty() {
        0
    } else {
        text_width(cx, &text, (rest, area.height), title_size)?
    };
    let total = if text.is_empty() {
        percent_w
    } else {
        percent_w.saturating_add(gap).saturating_add(text_w)
    };
    let align = cx.settings.align()?;
    let x = geometry::clamp_u16(align.x(area, f32::from(total.min(area.width))) as i64);
    one_line(
        out,
        &cx.font,
        Rect::new(x, area.y, percent_w, area.height),
        Align::Left,
        (size, size),
        &shown,
        look.ink,
    )?;
    if !text.is_empty() {
        let beside = Rect::new(
            x.saturating_add(percent_w).saturating_add(gap),
            area.y,
            text_w,
            area.height,
        );
        block::draw(cx, out, look, (beside, Align::Left), (&text, title_size))?;
    }
    Ok(())
}

/// The width the text of `block` takes in a room: its widest line.
fn text_width(cx: &Cx<'_>, block: &Block, room: (u16, u16), title_size: f32) -> Result<u16, Error> {
    let small = cx.style().small;
    let mut widest = 0.0_f32;
    if let Some(title) = &block.title {
        let wrapped = paperpaw_ui_kit::widget::text::Wrapped::fitting(
            &cx.font,
            title,
            (title_size, small),
            room,
            true,
        )?;
        for line in wrapped.lines() {
            widest = widest.max(cx.font.measure_text(wrapped.size(), line)?);
        }
    }
    for note in &block.notes {
        widest = widest.max(cx.font.measure_text(small, note)?);
    }
    Ok(geometry::clamp_u16(widest as i64).min(room.0))
}

/// Everything a drawing of the book needs besides the canvas.
struct Scene<'a> {
    book: &'a Current,
    look: &'a Look,
    covered: Covered,
    title_size: f32,
}

/// The widget in `area` below the caption.
fn draw_book(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    pictures: &mut Pictures,
    area: Rect,
    scene: &Scene<'_>,
) -> Result<(), Error> {
    let (book, look) = (scene.book, scene.look);
    let how = cx.settings.with_text("style", style_of)?;
    let small = cx.style().small;
    if how != Style::Bar && f32::from(area.width.min(area.height)) < small * 3.0 {
        if area.width >= area.height.saturating_mul(4) && !block_of(cx, book, None)?.is_empty() {
            return percent_beside(cx, out, look, area, (book, scene.title_size));
        }
        return percent_alone(cx, out, look, area, book.percent);
    }
    match how {
        Style::Ring => draw_ring_style(cx, out, area, scene),
        Style::Round => draw_round(cx, out, pictures, area, scene),
        Style::Bar => draw_bar_style(cx, out, pictures, area, scene),
        Style::Beside => draw_beside(cx, out, pictures, area, scene),
    }
}

/// The ring alone, with the text beside it or below.
fn draw_ring_style(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    scene: &Scene<'_>,
) -> Result<(), Error> {
    let block = block_of(cx, scene.book, None)?;
    let laid = ring_layout(cx, area, (&block, scene.title_size))?;
    draw_laid(cx, out, scene, (&laid, &block), 0)
}

/// A ring and its text: the ring with the percentage inside, the text beside
/// or below it. A `shift` moves the ring left (to stand next to a cover).
fn draw_laid(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    scene: &Scene<'_>,
    (laid, block): (&RingLayout, &Block),
    shift: i32,
) -> Result<(), Error> {
    let mut at = laid.ring;
    at.x = geometry::clamp_u16(i64::from(at.x) - i64::from(shift));
    draw_ring_in(cx, out, scene.look, at, (scene.book.percent, true))?;
    if let Some((room, beside)) = laid.text {
        let align = cx.settings.align()?;
        let align = if beside && align == Align::Center {
            Align::Left
        } else {
            align
        };
        block::draw(
            cx,
            out,
            scene.look,
            (room, align),
            (block, scene.title_size),
        )?;
    }
    Ok(())
}

/// The cover left, the ring right of it (and the text), where the place has room
/// for both; else the ring alone.
fn draw_beside(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    pictures: &mut Pictures,
    area: Rect,
    scene: &Scene<'_>,
) -> Result<(), Error> {
    let gap = cx.style().gap;
    let block = block_of(cx, scene.book, None)?;
    let shape = shape_of(scene.covered);
    let (cw, ch) = cover_size(shape, area.width.saturating_mul(2) / 5, area.height);
    let rest_w = area.width.saturating_sub(cw.saturating_add(gap));
    let wanted = cx.settings.shows("cover")? && rest_w >= area.height.saturating_mul(6) / 10;
    if !wanted {
        let laid = ring_layout(cx, area, (&block, scene.title_size))?;
        return draw_laid(cx, out, scene, (&laid, &block), 0);
    }
    let rest = Rect::new(
        area.x.saturating_add(cw).saturating_add(gap),
        area.y,
        rest_w,
        area.height,
    );
    let laid = ring_layout(cx, rest, (&block, scene.title_size))?;
    // Without text the cover and the ring stand together as the place aligns them.
    let (x, shift) = if laid.text.is_none() {
        let group = cw.saturating_add(gap).saturating_add(laid.ring.width);
        let x = geometry::clamp_u16(cx.settings.align()?.x(area, f32::from(group)) as i64);
        (
            x,
            i32::from(laid.ring.x) - i32::from(x.saturating_add(cw).saturating_add(gap)),
        )
    } else {
        (area.x, 0)
    };
    let at = Rect::new(
        x,
        area.y.saturating_add(area.height.saturating_sub(ch) / 2),
        cw,
        ch,
    );
    draw_cover(
        cx,
        out,
        pictures,
        scene.look,
        at,
        (&scene.book.title, scene.covered),
    )?;
    draw_laid(cx, out, scene, (&laid, &block), shift)
}

/// A ring round a small cover, the percentage and the text beside or below.
fn draw_round(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    pictures: &mut Pictures,
    area: Rect,
    scene: &Scene<'_>,
) -> Result<(), Error> {
    let lead = percent_text(cx, scene.book.percent)?;
    let block = block_of(cx, scene.book, Some(lead))?;
    let laid = ring_layout(cx, area, (&block, scene.title_size))?;
    let inner = draw_ring_in(cx, out, scene.look, laid.ring, (scene.book.percent, false))?;
    let inner = u16::try_from(inner).unwrap_or(0);
    if inner >= 8 && cx.settings.shows("cover")? {
        let (w, h) = cover_size(
            shape_of(scene.covered),
            inner.saturating_mul(2).saturating_mul(56) / 100,
            inner.saturating_mul(2).saturating_mul(83) / 100,
        );
        let ring = laid.ring;
        let at = Rect::new(
            ring.x.saturating_add(ring.width.saturating_sub(w) / 2),
            ring.y.saturating_add(ring.height.saturating_sub(h) / 2),
            w,
            h,
        );
        draw_cover(
            cx,
            out,
            pictures,
            scene.look,
            at,
            (&scene.book.title, scene.covered),
        )?;
    }
    if let Some((room, beside)) = laid.text {
        let align = match cx.settings.align()? {
            Align::Center if beside => Align::Left,
            align => align,
        };
        block::draw(
            cx,
            out,
            scene.look,
            (room, align),
            (&block, scene.title_size),
        )?;
    }
    Ok(())
}

/// The cover left where there is room, the title and author, and below them
/// the percentage with a bar beside it.
fn draw_bar_style(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    pictures: &mut Pictures,
    area: Rect,
    scene: &Scene<'_>,
) -> Result<(), Error> {
    let (small, gap) = {
        let style = cx.style();
        (style.small, style.gap)
    };
    let (cw, ch) = cover_size(
        shape_of(scene.covered),
        area.width.saturating_mul(45) / 100,
        area.height,
    );
    let room = f32::from(area.width.saturating_sub(cw.saturating_add(gap)));
    let mut rest = area;
    if cx.settings.shows("cover")? && room >= small * 7.0 {
        let at = Rect::new(
            area.x,
            area.y.saturating_add(area.height.saturating_sub(ch) / 2),
            cw,
            ch,
        );
        draw_cover(
            cx,
            out,
            pictures,
            scene.look,
            at,
            (&scene.book.title, scene.covered),
        )?;
        let shift = cw.saturating_add(gap).min(area.width);
        rest = Rect::new(
            area.x.saturating_add(shift),
            area.y,
            area.width - shift,
            area.height,
        );
    }
    let row = geometry::line_px(&cx.font, small)?;
    let block = block_of(cx, scene.book, None)?;
    let text_h = block::height(
        cx,
        &block,
        (
            rest.width,
            rest.height.saturating_sub(row.saturating_add(gap)),
        ),
        scene.title_size,
    )?;
    let total = text_h
        .saturating_add(gap)
        .saturating_add(row)
        .min(rest.height);
    let top = rest.y.saturating_add(rest.height.saturating_sub(total) / 2);
    let align = cx.settings.align()?;
    block::draw(
        cx,
        out,
        scene.look,
        (Rect::new(rest.x, top, rest.width, text_h), align),
        (&block, scene.title_size),
    )?;
    let y = top.saturating_add(total).saturating_sub(row);
    let percent = percent_text(cx, scene.book.percent)?;
    let width = geometry::clamp_u16(cx.font.measure_text(small, &percent)? as i64);
    one_line(
        out,
        &cx.font,
        Rect::new(rest.x, y, rest.width, row),
        Align::Left,
        (small, small),
        &percent,
        scene.look.faint,
    )?;
    let bar_h = geometry::clamp_u16(i64::from(thickness(cx, row.saturating_mul(4))?)).min(row);
    let shift = width.saturating_add(gap).min(rest.width);
    let bar = Rect::new(
        rest.x.saturating_add(shift),
        y.saturating_add(row.saturating_sub(bar_h) / 2),
        rest.width - shift,
        bar_h,
    );
    draw_bar(cx, out, scene.look, bar, scene.book.percent)
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
    let host = cx.host;
    let Some(book) = read(host, cache)? else {
        let message = cx.tr(NOTHING)?;
        return crate::native_draw::message_in(cx, out, &look, &message);
    };
    let rest = caption(cx, out, &look, CAPTION)?;
    let found = if cx.settings.with_text("style", style_of)? == Style::Ring {
        Covered::None
    } else {
        covered(cx, book, pictures)?
    };
    let title_size = cx.settings.size()?.text(cx.style());
    let scene = Scene {
        book,
        look: &look,
        covered: found,
        title_size,
    };
    draw_book(cx, out, pictures, rest, &scene)
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
        (&mut Cache::default(), &mut Pictures::default()),
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

/// The book a tap opens: the one being read.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn book(host: &Host<'_>, cache: &mut Cache) -> Result<Option<(u32, u32)>, Error> {
    Ok(read(host, cache)?.map(|book| (book.key, book.generation)))
}

/// Whether the text size setting is the one the title is drawn at: here only
/// used to keep [`TextSize`] in the module's reach for the tests.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_is_read_from_its_choice_and_beside_is_the_default() {
        assert_eq!(style_of("widgets.progress.round"), Style::Round);
        assert_eq!(style_of("widgets.progress.ring"), Style::Ring);
        assert_eq!(style_of("widgets.progress.bar"), Style::Bar);
        assert_eq!(style_of("widgets.progress.beside"), Style::Beside);
        assert_eq!(style_of("anything else"), Style::Beside);
    }
}
