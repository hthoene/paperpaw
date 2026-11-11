use crate::{Messages, Page, Paint, Style, text};
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
        (s.ink, s.paper)
    } else {
        (s.paper, s.ink)
    };
    page.paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
    page.paint.push(Paint::Fill(area, paper));
    let lined = !s.dark && !s.plain && s.separators != "core.separators.none";
    if lined {
        page.paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
        page.paint.push(Paint::Fill(
            Rect::new(0, s.header.saturating_sub(s.line), width, s.line),
            s.rule,
        ));
    }
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
        name_arrow(
            page,
            (host, m),
            (s, area),
            "core.hint.core.back",
            Rect::new(0, 0, left, s.header),
            false,
        )?;
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
    let mut size = s.head;
    while size > s.small && font.measure_text(size, title)? > room + 1.0 {
        size = (size - 0.25).max(s.small);
    }
    let title = text::fit(font, size, title, room + 1.0)?;
    // Only a mirrored title needs its width, to put its right edge where the
    // left edge was.
    let wide = if s.rtl {
        font.measure_text(size, &title)?
    } else {
        0.0
    };
    // The title is the heading: it takes the text drawn next as its label.
    crate::paint::describe(
        &mut page.paint,
        s.describing,
        PartRole::Heading,
        band,
        false,
        false,
    )?;
    page.paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
    page.paint.push(Paint::Text {
        text: title,
        size,
        origin: TextOrigin::new(
            s.flip_x(area, f32::from(left), wide),
            text::cap_baseline(font, size, band)?,
        ),
        color: ink,
        clip: Some(s.flip(
            area,
            Rect::new(left, 0, right.saturating_sub(left), s.header),
        )),
    });
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
            for (key, rect, faint) in [
                ("core.pager.previous", prev, index == 0),
                ("core.pager.next", next, index.saturating_add(1) == count),
            ] {
                name_arrow(page, (host, m), (s, area), key, rect, faint)?;
            }
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
/// Tells the part of an arrow's tap region (it has no text to name it) where
/// somebody asks: a button named by the catalog message `key`, faint where
/// `faint`.
fn name_arrow(
    page: &mut Page,
    (host, m): (&Host<'_>, &mut Messages),
    (s, area): (&Style, Rect),
    key: &str,
    rect: Rect,
    faint: bool,
) -> Result<(), Error> {
    if !s.describing {
        return Ok(());
    }
    let name = m.copy(host, key)?;
    crate::paint::describe_labelled(
        &mut page.paint,
        true,
        PartRole::Button,
        s.flip(area, rect),
        false,
        faint,
        &name,
    )
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
