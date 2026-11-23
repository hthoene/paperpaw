use crate::{Messages, Page, Paint, Style, text, widget};
use alloc::vec::Vec;
use paperpaw_sdk::native::canvas::PartRole;
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        fonts::{Font, TextOrigin},
        formatting::{self, Arg, FormatBuffers},
    },
};
#[allow(
    clippy::too_many_arguments,
    reason = "header geometry, localization and paging inputs"
)]
pub fn prepare(
    page: &mut Page,
    host: &Host<'_>,
    font: &Font<'_>,
    s: &Style,
    m: &mut Messages,
    title: &str,
    width: u16,
    touch: bool,
    index: usize,
    count: usize,
) -> Result<(), Error> {
    let area = Rect::new(0, 0, width, s.header);
    let (paper, ink) = if s.dark {
        let bar = s.header_bar();
        (bar, s.on(bar, s.roles.bar.is_some(), s.paper))
    } else {
        (s.paper, s.ink)
    };
    background(&mut page.paint, s, area, paper)?;
    let lined = !s.dark && !s.plain && s.separators != "core.separators.none";
    let band = Rect::new(
        0,
        0,
        width,
        s.header.saturating_sub(if lined { s.line } else { 0 }),
    );
    let half = (s.title * 0.38) as i32;
    let reach = (half * 4 / 5) / 2;
    let extent = half * 4 / 5 + (i32::from(s.line) * 3).max(3);
    let left = if touch {
        let arrow =
            ((s.text * 1.15) as u32 + u32::from(s.gap)).max(extent as u32 + u32::from(s.gap));
        crate::geometry::clamp_u16(i64::from(s.margin) + i64::from(arrow))
    } else {
        s.margin
    };
    // Laid out left to right; mirrored as it is made where the interface
    // runs right to left.
    if touch {
        page.back = Some(s.flip(area, Rect::new(0, 0, left, s.header)));
        arrow(
            &mut page.paint,
            (s, area),
            i32::from(s.margin) + reach,
            i32::from(band.height) / 2,
            true,
            ink,
        )?;
    }
    let right = pager(page, host, font, s, m, area, band, touch, index, count, ink)?;
    let room = f32::from(right.saturating_sub(left).saturating_sub(s.margin));
    let clip = s.flip(
        area,
        Rect::new(left, 0, right.saturating_sub(left), s.header),
    );
    title_part(page, font, s, (area, band, clip), (left, room), title, ink)
}

/// The size of captions, to the small size: the least a title wraps at.
const CAPTION: f32 = 0.8;

/// How a header's title is shown.
#[derive(Debug, PartialEq)]
enum Title {
    /// On one line at this size, shortened with "…" where it does not fit.
    Line(f32),
    /// On these lines, each whole, at this size.
    Lines(f32, Vec<alloc::string::String>),
}

/// How `title` is shown in `room` pixels, as Beta 3 showed it: on one line, as
/// large as fits from `head` down to `least`; where it does not fit so, on as
/// many lines as the bar holds (`most` tells how many at a size), at `least`,
/// else at `caption`; shortened at `least` only where none of that holds it.
/// A pixel of grace: a title that fits is not cut.
///
/// # Errors
/// What `measure` or `most` say; [`Error::NoMemory`].
fn fit_title(
    measure: &mut dyn FnMut(f32, &str) -> Result<f32, Error>,
    most: &mut dyn FnMut(f32) -> Result<usize, Error>,
    (head, least, caption): (f32, f32, f32),
    title: &str,
    room: f32,
) -> Result<Title, Error> {
    if measure(least, title)? <= room + 1.0 {
        let size =
            widget::text::largest_size(
                head,
                least,
                |size| Ok(measure(size, title)? <= room + 1.0),
            )?;
        return Ok(Title::Line(size.unwrap_or(least)));
    }
    for size in [least, caption.min(least)] {
        // A line beyond the most the bar holds tells that it does not fit.
        let most = most(size)?.max(1);
        let lines = text::wrap_with(
            |line| measure(size, line),
            title,
            room.max(1.0),
            most.saturating_add(1),
        )?;
        if lines.len() < 2 {
            return Ok(Title::Line(size));
        }
        if lines.len() <= most {
            return Ok(Title::Lines(size, lines));
        }
    }
    Ok(Title::Line(least))
}

/// How many lines of a heading at `size` fit `high` pixels (at least one).
fn lines_in(font: &Font<'_>, size: f32, high: u16) -> Result<usize, Error> {
    let line = crate::geometry::line_px(font, size)?.max(1);
    let apart = widget::text::step(font, size, true)?.max(1);
    Ok(1 + usize::from(high.saturating_sub(line) / apart))
}

/// Draws the header's `title` from `left`, `room` wide, in the header `area`
/// (its ink centred in `band`, cut at `clip`), as [`fit_title`] fits it; it is
/// the heading.
fn title_part(
    page: &mut Page,
    font: &Font<'_>,
    s: &Style,
    (area, band, clip): (Rect, Rect, Rect),
    (left, room): (u16, f32),
    title: &str,
    ink: Color,
) -> Result<(), Error> {
    let least = s.small.min(s.head);
    let shown = fit_title(
        &mut |size, text| font.measure_text(size, text),
        &mut |size| lines_in(font, size, area.height),
        (s.head, least, s.small * CAPTION),
        title,
        room,
    )?;
    let (size, lines) = match shown {
        Title::Line(size) => (size, alloc::vec![text::fit(font, size, title, room + 1.0)?]),
        Title::Lines(size, lines) => (size, lines),
    };
    let one = lines.len() == 1;
    // The title is the heading: it takes the texts drawn inside it as its
    // label (on lines, the whole bar holds them).
    crate::paint::describe(
        &mut page.paint,
        s.describing,
        PartRole::Heading,
        if one { band } else { area },
        false,
        false,
    )?;
    // One line by its capitals in the band; lines a heading's apart, the
    // whole of them in the middle of the bar.
    let (top, apart, line) = if one {
        (band.y, 0, band.height)
    } else {
        let tall = widget::text::lines_height(font, size, true, lines.len())?;
        (
            area.y + area.height.saturating_sub(tall) / 2,
            widget::text::step(font, size, true)?,
            crate::geometry::line_px(font, size)?,
        )
    };
    page.paint
        .try_reserve(lines.len())
        .map_err(|_| Error::NoMemory)?;
    for (n, shown) in lines.into_iter().enumerate() {
        let y = top.saturating_add(apart.saturating_mul(n as u16));
        let baseline = text::cap_baseline(font, size, Rect::new(area.x, y, area.width, line))?;
        // Only a mirrored title needs its width, to put its right edge where
        // the left edge was.
        let wide = if s.rtl {
            font.measure_text(size, &shown)?
        } else {
            0.0
        };
        page.paint.push(Paint::Text {
            text: shown,
            size,
            origin: TextOrigin::new(s.flip_x(area, f32::from(left), wide), baseline),
            color: ink,
            clip: Some(clip),
        });
    }
    Ok(())
}

/// The pager's label (`2 / 5`): said through two small stack buffers, so that a
/// frame makes no 4 KiB buffer for it; a label that does not fit them (none of
/// the catalogs' does) is said through `m`.
fn page_label(
    host: &Host<'_>,
    m: &mut Messages,
    index: usize,
    count: usize,
) -> Result<alloc::string::String, Error> {
    let args = [
        Arg::Int {
            name: "page",
            value: index.saturating_add(1) as i64,
        },
        Arg::Int {
            name: "pages",
            value: count as i64,
        },
    ];
    let (mut scratch, mut out) = ([0; 96], [0; 128]);
    let mut buffers = FormatBuffers::new(&mut scratch, &mut out);
    if let Ok(length) = formatting::format_message(host, "core.page-of", &args, &mut buffers)
        && let Some(bytes) = out.get(..length)
        && let Ok(label) = core::str::from_utf8(bytes)
    {
        return text::copy(label);
    }
    m.format(host, "core.page-of", &args)
}

#[allow(
    clippy::too_many_arguments,
    reason = "paging geometry and localized label inputs"
)]
fn pager(
    page: &mut Page,
    host: &Host<'_>,
    font: &Font<'_>,
    s: &Style,
    m: &mut Messages,
    area: Rect,
    band: Rect,
    touch: bool,
    index: usize,
    count: usize,
    ink: Color,
) -> Result<u16, Error> {
    let width = area.width;
    let mut right = width;
    let reach = ((s.title * 0.38) as i32 * 4 / 5) / 2;
    if count > 1 {
        let label = page_label(host, m, index, count)?;
        let label_width = font.measure_text(s.small, &label)? as u16;
        let next_x = i32::from(width) - i32::from(s.margin) - reach - 1;
        let arrow_width = if touch {
            (u32::from(s.header) * 4 / 5) as u16
        } else {
            0
        };
        let next_start = if touch {
            (next_x - i32::from(arrow_width / 2)).max(0) as u16
        } else {
            width.saturating_sub(s.margin)
        };
        let label_rect = Rect::new(
            next_start.saturating_sub(label_width),
            0,
            label_width,
            s.header,
        );
        let prev = Rect::new(
            label_rect.x.saturating_sub(arrow_width),
            0,
            arrow_width,
            s.header,
        );
        let next = Rect::new(next_start, 0, width.saturating_sub(next_start), s.header);
        right = prev.x;
        page.paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
        page.paint.push(Paint::Text {
            text: label,
            size: s.small,
            origin: TextOrigin::new(
                s.flip_x(area, f32::from(label_rect.x), f32::from(label_width)),
                text::cap_baseline(font, s.small, band)?,
            ),
            color: ink,
            clip: Some(area),
        });
        if touch {
            page.pager = Some((
                s.flip(area, prev),
                s.flip(area, label_rect),
                s.flip(area, next),
            ));
            arrow(
                &mut page.paint,
                (s, area),
                i32::from(prev.x) + i32::from(prev.width) / 2,
                i32::from(band.height) / 2,
                true,
                if index == 0 { s.disabled } else { ink },
            )?;
            arrow(
                &mut page.paint,
                (s, area),
                next_x,
                i32::from(band.height) / 2,
                false,
                if index.saturating_add(1) == count {
                    s.disabled
                } else {
                    ink
                },
            )?;
        }
    }
    Ok(right)
}
/// An arrow pointing `left` (as laid out left to right) at `x`, mirrored in
/// `area` where the interface runs right to left.
fn arrow(
    paint: &mut Vec<Paint>,
    (s, area): (&Style, Rect),
    x: i32,
    y: i32,
    left: bool,
    color: Color,
) -> Result<(), Error> {
    let (x, left) = if s.rtl {
        (2 * i32::from(area.x) + i32::from(area.width) - x, !left)
    } else {
        (x, left)
    };
    let half = (s.title * 0.38) as i32;
    let width = half * 4 / 5;
    let thickness = (u32::from(s.line) * 3).max(3).min(u32::from(u16::MAX)) as u16;
    for dy in -half..=half {
        let dx = width * dy.abs() / half.max(1);
        let tip = if left {
            x - width / 2 + dx
        } else {
            x + width / 2 - dx
        };
        let start = if left {
            tip
        } else {
            tip - i32::from(thickness) + 1
        };
        if let (Ok(x), Ok(y)) = (u16::try_from(start), u16::try_from(y + dy)) {
            paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
            paint.push(Paint::Fill(Rect::new(x, y, thickness, 1), color));
        }
    }
    Ok(())
}

/// The header plate and its separator, as Beta 3: full ink for lines,
/// an inset tint for inset separators, none for plain or dark headers.
fn background(paint: &mut Vec<Paint>, s: &Style, area: Rect, paper: Color) -> Result<(), Error> {
    paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
    paint.push(Paint::Fill(area, paper));
    if s.dark || s.plain || s.separators == "core.separators.none" {
        return Ok(());
    }
    let inset = s.separators == "core.separators.inset";
    let margin = if inset {
        s.margin.min(area.width / 2)
    } else {
        0
    };
    let rect = Rect::new(
        area.x.saturating_add(margin),
        area.y.saturating_add(area.height).saturating_sub(s.line),
        area.width.saturating_sub(2 * margin),
        s.line,
    );
    let line = if inset && s.mono {
        Paint::Stipple(rect, s.rule)
    } else {
        Paint::Fill(rect, if inset { s.rule } else { s.ink })
    };
    paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
    paint.push(line);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_made_smaller_in_few_steps_then_wrapped_then_shortened() {
        // Every letter half the size wide; the bar holds two lines of the
        // small size (10) and three of the caption's (8).
        let measures = core::cell::Cell::new(0);
        let fit = |title: &str| {
            measures.set(0);
            fit_title(
                &mut |size, text| {
                    measures.set(measures.get() + 1);
                    Ok(text.chars().count() as f32 * size * 0.5)
                },
                &mut |size| Ok(if size < 10.0 { 3 } else { 2 }),
                (20.0, 10.0, 8.0),
                title,
                100.0,
            )
            .unwrap()
        };
        assert_eq!(fit("Short"), Title::Line(20.0));
        // 15 letters fit at 13.3 (99 px): found in 8 % steps, not in 0.25 px.
        let Title::Line(size) = fit("Fifteen letters") else {
            panic!("on one line");
        };
        assert!((13.0..13.4).contains(&size), "{size}");
        assert!(measures.get() <= 8, "{}", measures.get());
        assert_eq!(
            fit("The quiet word in the valley of the"),
            Title::Lines(
                10.0,
                alloc::vec!["The quiet word in".into(), "the valley of the".into()]
            )
        );
        assert_eq!(
            fit("The quiet word in the valley of the river that runs"),
            Title::Lines(
                8.0,
                alloc::vec![
                    "The quiet word in the".into(),
                    "valley of the river that".into(),
                    "runs".into()
                ]
            )
        );
        let long =
            "The quiet word in the valley of the river that runs to the sea far away and back";
        assert_eq!(fit(long), Title::Line(10.0));
    }

    #[test]
    fn a_light_inset_header_keeps_its_rule_on_monochrome() {
        let mut s = Style::themed(Color::BLACK, Color::WHITE);
        s.mono = true;
        s.rule = Color::gray(150);
        s.separators = alloc::string::String::from("core.separators.inset");
        s.margin = 36;
        s.line = 3;
        let mut paint = Vec::new();
        background(&mut paint, &s, Rect::new(0, 0, 480, 58), s.paper).unwrap();
        assert!(matches!(paint.get(1), Some(Paint::Stipple(rect, ink))
            if *rect == Rect::new(36, 55, 408, 3) && *ink == s.rule));
        // A grayscale panel keeps the tint, while plain headers stay bare.
        s.mono = false;
        paint.clear();
        background(&mut paint, &s, Rect::new(0, 0, 480, 58), s.paper).unwrap();
        assert!(matches!(paint.get(1), Some(Paint::Fill(_, ink)) if *ink == s.rule));
        s.plain = true;
        paint.clear();
        background(&mut paint, &s, Rect::new(0, 0, 480, 58), s.paper).unwrap();
        assert_eq!(paint.len(), 1);
    }
}
