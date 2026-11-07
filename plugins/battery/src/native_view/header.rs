use super::{Messages, Page, Paint, Style, text};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        fonts::{Font, TextOrigin},
        formatting::Arg,
    },
};
#[allow(
    clippy::too_many_arguments,
    reason = "header geometry, localization and paging inputs"
)]
pub(super) fn prepare(
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
    page.paint.push(Paint::Fill(area, paper));
    let lined = !s.dark && !s.plain && s.separators != "core.separators.none";
    if lined {
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
    let extent = half * 4 / 5 + i32::from(s.line * 3).max(3);
    let left = if touch {
        s.margin + ((s.text * 1.15) as u16 + s.gap).max(extent as u16 + s.gap)
    } else {
        s.margin
    };
    if touch {
        page.back = Some(Rect::new(0, 0, left, s.header));
        arrow(
            &mut page.paint,
            s,
            i32::from(s.margin) + reach,
            i32::from(band.height) / 2,
            true,
            ink,
        );
    }
    let right = pager(page, host, font, s, m, area, band, touch, index, count, ink)?;
    let room = f32::from(right.saturating_sub(left).saturating_sub(s.margin));
    let mut size = s.head;
    while size > s.small && font.measure_text(size, title)? > room + 1.0 {
        size = (size - 0.25).max(s.small);
    }
    let title = text::fit(font, size, title, room + 1.0)?;
    page.paint.push(Paint::Text {
        text: title,
        size,
        origin: TextOrigin::new(f32::from(left), text::cap_baseline(font, size, band)?),
        color: ink,
        clip: Some(Rect::new(left, 0, right.saturating_sub(left), s.header)),
    });
    Ok(())
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
        let label = m.format(
            host,
            "core.page-of",
            &[
                Arg::Int {
                    name: "page",
                    value: (index + 1) as i64,
                },
                Arg::Int {
                    name: "pages",
                    value: count as i64,
                },
            ],
        )?;
        let label_width = font.measure_text(s.small, &label)? as u16;
        let next_x = i32::from(width) - i32::from(s.margin) - reach - 1;
        let arrow_width = if touch { s.header * 4 / 5 } else { 0 };
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
        page.paint.push(Paint::Text {
            text: label,
            size: s.small,
            origin: TextOrigin::new(
                f32::from(label_rect.x),
                text::cap_baseline(font, s.small, band)?,
            ),
            color: ink,
            clip: Some(area),
        });
        if touch {
            page.pager = Some((prev, label_rect, next));
            arrow(
                &mut page.paint,
                s,
                i32::from(prev.x) + i32::from(prev.width) / 2,
                i32::from(band.height) / 2,
                true,
                if index == 0 { s.disabled } else { ink },
            );
            arrow(
                &mut page.paint,
                s,
                next_x,
                i32::from(band.height) / 2,
                false,
                if index + 1 == count { s.disabled } else { ink },
            );
        }
    }
    Ok(right)
}
fn arrow(paint: &mut Vec<Paint>, s: &Style, x: i32, y: i32, left: bool, color: Color) {
    let half = (s.title * 0.38) as i32;
    let width = half * 4 / 5;
    let thickness = (s.line * 3).max(3);
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
            paint.push(Paint::Fill(Rect::new(x, y, thickness, 1), color));
        }
    }
}
