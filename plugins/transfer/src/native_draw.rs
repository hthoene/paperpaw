//! The address on the transfer screen: the QR code that carries it, the address
//! as text to be typed, what to do with it, and what happened last. Drawn under
//! the list's header (`ItemList`), so the header and its back arrow are the
//! stock ones.
use alloc::{string::String, vec::Vec};
use paperpaw_qr::QrCode;
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        fonts::{Font, FontRole},
        snapshot,
    },
};
use paperpaw_ui_kit::{
    Messages, Paint, Style,
    geometry::{clamp_u16, line_px},
    paint::{self, frame_rect},
    text,
};

/// The no-network screen: the status icon and explanation above a bottom button.
///
/// # Errors
/// Host, font and allocation failures.
pub fn draw_no_network(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    managed: bool,
) -> Result<Option<Rect>, Error> {
    let display = snapshot::capture_snapshot(host)?;
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let style = Style::read(host, &display, &font)?;
    let width = u16::try_from(frame.rect.width).unwrap_or(u16::MAX);
    let height = u16::try_from(frame.rect.height).unwrap_or(u16::MAX);
    let button_height =
        paperpaw_ui_kit::Chips::height(&style, paperpaw_ui_kit::Chips::finger(&display));
    let button = managed.then(|| {
        Rect::new(
            style.margin,
            height.saturating_sub(style.margin + button_height),
            width.saturating_sub(2 * style.margin),
            button_height,
        )
    });
    let mut messages = Messages::new()?;
    let title = messages.copy(host, "transfer.no-network.title")?;
    let note = messages.copy(
        host,
        if managed {
            "transfer.no-network"
        } else {
            "transfer.no-network.system"
        },
    )?;
    let inner = width.saturating_sub(2 * style.margin);
    let room = f32::from(width.saturating_sub(4 * style.margin)).max(1.0);
    let lines = text::wrap_with(|line| font.measure_text(style.small, line), &note, room, 6)?;
    let head = line_px(&font, style.title)?;
    let line = line_px(&font, style.small)?;
    let group = style.row + 2 * style.gap + head + line * lines.len() as u16;
    let bottom = button.map_or(height, |button| button.y);
    let top = style.header + bottom.saturating_sub(style.header + group) / 2;
    let mut ops = Vec::new();
    let icon_font = paperpaw_ui_kit::icons::font(host)?;
    if let (Some(icons), Some(glyph)) = (
        icon_font.as_ref(),
        paperpaw_ui_kit::icons::glyph(host, "wifi-off")?,
    ) {
        paperpaw_ui_kit::icons::draw_centred(
            &mut ops,
            icons,
            &glyph,
            style.title * 2.0,
            Rect::new(0, top, width, style.row),
            style.ink,
        )?;
    }
    let mut run = Run {
        font: &font,
        style: &style,
        inner: f32::from(inner),
        width,
        y: top + style.row + style.gap,
    };
    run.put(&mut ops, &[title], style.title, head, style.ink)?;
    run.y += style.gap;
    run.put(&mut ops, &lines, style.small, line, style.muted)?;
    if let Some(rect) = button {
        draw_router_button(host, &font, &style, rect, !display.has_touch(), &mut ops)?;
    }
    let drawn = paint::replay_icons(frame, &font, icon_font.as_ref(), &ops);
    if let Some(icons) = icon_font {
        icons.release()?;
    }
    font.release()?;
    drawn?;
    Ok(button)
}

fn draw_router_button(
    host: &Host<'_>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    focused: bool,
    ops: &mut Vec<Paint>,
) -> Result<(), Error> {
    let label = Messages::new()?.copy(host, "transfer.without-router")?;
    paint::push(
        ops,
        Paint::Focus(
            rect,
            u32::from(rect.height / 2),
            Some(u32::from(style.line)),
            style.ink,
        ),
    )?;
    if focused {
        paint::push(
            ops,
            Paint::Focus(
                rect.inset(4, 4),
                u32::from(rect.height / 2),
                Some(u32::from(style.line)),
                style.ink,
            ),
        )?;
    }
    paint::describe_named(
        ops,
        style.describing,
        paperpaw_sdk::native::canvas::PartRole::Button,
        rect,
        &label,
        focused,
        false,
    )?;
    let wide = font.measure_text(style.small, &label)?;
    text::label(
        ops,
        font,
        &label,
        style.small,
        f32::from(rect.x) + (f32::from(rect.width) - wide) / 2.0,
        rect,
        style.ink,
        None,
    )
}

/// The quiet zone round a code, in modules (what scanners expect).
const QUIET: u16 = 4;
/// Lines of the address, the hint and the last thing said, at most.
const ADDRESS_LINES: usize = 4;
const HINT_LINES: usize = 5;
const LAST_LINES: usize = 2;

/// Where the code goes in `area`: the white square with its quiet zone, the
/// size of a module in pixels, and where the first module is; `None` where not
/// even a pixel a module fits.
#[must_use]
pub fn qr_place(modules: usize, area: Rect) -> Option<(Rect, u16, u16, u16)> {
    let wide = u16::try_from(modules).ok()?.checked_add(2 * QUIET)?;
    let scale = area.width.min(area.height) / wide;
    if scale == 0 {
        return None;
    }
    let side = scale * wide;
    let x = area.x + (area.width - side) / 2;
    let y = area.y + (area.height - side) / 2;
    Some((
        Rect::new(x, y, side, side),
        scale,
        x + QUIET * scale,
        y + QUIET * scale,
    ))
}

/// Calls `fill` for the white square and then for each run of dark modules in
/// a row (one rectangle a run, so a code takes tens of calls, not a thousand).
///
/// # Errors
/// What `fill` says.
pub fn qr_rects(
    qr: &QrCode,
    area: Rect,
    mut fill: impl FnMut(Rect, Color) -> Result<(), Error>,
) -> Result<(), Error> {
    let size = qr.size();
    let Some((white, scale, x0, y0)) = qr_place(size, area) else {
        return Ok(());
    };
    fill(white, Color::WHITE)?;
    for row in 0..size {
        let mut column = 0;
        while column < size {
            if !qr.dark(column, row) {
                column += 1;
                continue;
            }
            let start = column;
            while column < size && qr.dark(column, row) {
                column += 1;
            }
            // Sizes below came from `u16`s that fit.
            let (start, run, row) = (start as u16, (column - start) as u16, row as u16);
            fill(
                Rect::new(x0 + start * scale, y0 + row * scale, run * scale, scale),
                Color::BLACK,
            )?;
        }
    }
    Ok(())
}

/// The reader's own network, joined first: its code, and its name and
/// password to type where the code cannot be scanned.
#[derive(Debug, Clone, Copy)]
pub struct Join<'a> {
    pub qr: Option<&'a QrCode>,
    pub title: &'a str,
    pub note: &'a str,
}

/// What is under the header: the code on top, then the address, the hint and
/// the last thing that happened, centred. With `join` the code to join the
/// reader's own network takes the upper half.
///
/// # Errors
/// What the host's fonts, canvas and texts refuse; `NoMemory`.
pub fn draw_address(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    hint: &str,
    url: &str,
    qr: Option<&QrCode>,
    last: Option<&str>,
    join: Option<&Join<'_>>,
) -> Result<(), Error> {
    if !matches!(frame.purpose, DrawPurpose::Screen) {
        return Ok(());
    }
    let display = snapshot::capture_snapshot(host)?;
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let style = Style::read(host, &display, &font)?;
    let (width, height) = (display.display_width(), display.display_height());
    let top = style.header.saturating_add(style.gap);
    let rest = height.saturating_sub(top);
    let half = if join.is_some() { rest / 2 } else { 0 };
    let upper = Rect::new(0, top, width, half);
    let lower = Rect::new(0, top.saturating_add(half), width, rest - half);
    let drawn = lay_out(host, &font, &style, lower, (hint, url), qr.is_some(), last).and_then(
        |(mut ops, code)| {
            let joined = match join {
                Some(join) => lay_out_join(&font, &style, upper, join, &mut ops)?,
                None => None,
            };
            paint::replay(frame, &font, ops.iter()).map(|()| (code, joined))
        },
    );
    let released = font.release();
    let (code, joined) = drawn?;
    released?;
    let canvas = frame.draw_canvas()?;
    for (qr, area) in [(qr, code), (join.and_then(|join| join.qr), joined)] {
        if let (Some(qr), Some(area)) = (qr, area) {
            qr_rects(qr, area, |rect, color| {
                canvas.fill_rectangle(frame_rect(rect), color)
            })?;
        }
    }
    Ok(())
}

/// Centred lines of text, one under the other.
struct Run<'a> {
    font: &'a Font<'a>,
    style: &'a Style,
    inner: f32,
    width: u16,
    /// Where the next line goes.
    y: u16,
}

impl Run<'_> {
    fn put(
        &mut self,
        ops: &mut Vec<Paint>,
        lines: &[String],
        size: f32,
        rows: u16,
        color: Color,
    ) -> Result<(), Error> {
        let margin = self.style.margin;
        for line in lines {
            let wide = self.font.measure_text(size, line)?;
            let x = f32::from(margin) + ((self.inner - wide) / 2.0).max(0.0);
            let row = Rect::new(margin, self.y, self.width.saturating_sub(2 * margin), rows);
            text::label(ops, self.font, line, size, x, row, color, None)?;
            self.y = self.y.saturating_add(rows);
        }
        Ok(())
    }
}

/// The text runs, and the room for the code (none without one).
fn lay_out(
    host: &Host<'_>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    (hint, url): (&str, &str),
    has_code: bool,
    last: Option<&str>,
) -> Result<(Vec<Paint>, Option<Rect>), Error> {
    let (width, height) = (area.width, area.y.saturating_add(area.height));
    let inner = f32::from(width.saturating_sub(2 * style.margin)).max(1.0);
    let hint = Messages::new()?.copy(host, hint)?;
    let wrap = |size: f32, words: &str, most: usize| {
        text::wrap_with(|line| font.measure_text(size, line), words, inner, most)
    };
    let address = wrap(style.text, url, ADDRESS_LINES)?;
    let hints = wrap(style.small, &hint, HINT_LINES)?;
    let lasts = match last {
        Some(words) => wrap(style.small, words, LAST_LINES)?,
        None => Vec::new(),
    };
    let (big, small) = (line_px(font, style.text)?, line_px(font, style.small)?);
    let gap = i64::from(style.gap);
    let mut below = i64::from(big) * address.len() as i64 + gap;
    below += i64::from(small) * hints.len() as i64;
    if !lasts.is_empty() {
        below += gap + i64::from(small) * lasts.len() as i64;
    }
    let top = area.y;
    let room = i64::from(height) - i64::from(top) - below - gap;
    let side = clamp_u16(
        room.min(i64::from(width.saturating_sub(2 * style.margin)))
            .max(0),
    );
    let code_area = Rect::new(0, top, width, side);
    let y = top
        .saturating_add(if has_code { side } else { 0 })
        .saturating_add(style.gap);
    let mut ops = Vec::new();
    let mut run = Run {
        font,
        style,
        inner,
        width,
        y,
    };
    run.put(&mut ops, &address, style.text, big, style.ink)?;
    run.y = run.y.saturating_add(style.gap);
    run.put(&mut ops, &hints, style.small, small, style.note)?;
    if !lasts.is_empty() {
        run.y = run.y.saturating_add(style.gap);
        run.put(&mut ops, &lasts, style.small, small, style.ink)?;
    }
    Ok((ops, (has_code && side > 0).then_some(code_area)))
}

/// The join code's title and note, and the room for the code between them
/// (none where there is not a pixel for a module).
fn lay_out_join(
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    join: &Join<'_>,
    ops: &mut Vec<Paint>,
) -> Result<Option<Rect>, Error> {
    let inner = f32::from(area.width.saturating_sub(2 * style.margin)).max(1.0);
    let (big, small) = (line_px(font, style.text)?, line_px(font, style.small)?);
    let title = text::wrap_with(
        |line| font.measure_text(style.text, line),
        join.title,
        inner,
        2,
    )?;
    let note = text::wrap_with(
        |line| font.measure_text(style.small, line),
        join.note,
        inner,
        3,
    )?;
    let title_rows = u16::try_from(title.len()).unwrap_or(2);
    let note_rows = u16::try_from(note.len()).unwrap_or(3);
    let (title_high, note_high) = (
        big.saturating_mul(title_rows),
        small.saturating_mul(note_rows),
    );
    let mut run = Run {
        font,
        style,
        inner,
        width: area.width,
        y: area.y,
    };
    run.put(ops, &title, style.text, big, style.ink)?;
    run.y = (area.y.saturating_add(area.height)).saturating_sub(note_high);
    run.put(ops, &note, style.small, small, style.note)?;
    let room = area
        .height
        .saturating_sub(title_high.saturating_add(note_high));
    Ok((join.qr.is_some() && room > 0)
        .then(|| Rect::new(0, area.y.saturating_add(title_high), area.width, room)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qr() -> QrCode {
        QrCode::new(b"http://192.168.1.20:8080/?k=acdefhkmnprtwxy3").unwrap()
    }

    #[test]
    fn the_code_is_centred_with_its_quiet_zone_and_whole_pixels() {
        let (white, scale, x0, y0) = qr_place(33, Rect::new(10, 100, 400, 300)).unwrap();
        // 33 + 8 modules in 300 pixels: 7 pixels each.
        assert_eq!(scale, 7);
        assert_eq!((white.width, white.height), (41 * 7, 41 * 7));
        assert_eq!(white.x, 10 + (400 - 41 * 7) / 2);
        assert_eq!(white.y, 100 + (300 - 41 * 7) / 2);
        assert_eq!((x0, y0), (white.x + 28, white.y + 28));
        assert!(qr_place(33, Rect::new(0, 0, 40, 40)).is_none());
        assert!(qr_place(usize::MAX, Rect::new(0, 0, 40, 40)).is_none());
    }

    #[test]
    fn every_dark_module_is_covered_once_inside_the_white() {
        let (qr, area) = (qr(), Rect::new(0, 50, 480, 480));
        let mut rects = Vec::new();
        qr_rects(&qr, area, |rect, color| {
            rects.push((rect, color));
            Ok(())
        })
        .unwrap();
        let (white, scale, x0, y0) = qr_place(qr.size(), area).unwrap();
        assert_eq!(rects[0], (white, Color::WHITE));
        let dark = rects.len() - 1;
        // Runs, not modules.
        let modules = (0..qr.size())
            .flat_map(|y| (0..qr.size()).map(move |x| (x, y)))
            .filter(|(x, y)| qr.dark(*x, *y))
            .count();
        assert!(dark < modules);
        for y in 0..qr.size() {
            for x in 0..qr.size() {
                let (px, py) = (x0 + x as u16 * scale, y0 + y as u16 * scale);
                let covered = rects[1..].iter().filter(|(r, color)| {
                    *color == Color::BLACK
                        && r.x <= px
                        && px < r.x + r.width
                        && r.y == py
                        && r.height == scale
                });
                assert_eq!(covered.count(), usize::from(qr.dark(x, y)), "{x},{y}");
            }
        }
        for (rect, _) in &rects {
            assert!(rect.x >= white.x && rect.x + rect.width <= white.x + white.width);
            assert!(rect.y >= white.y && rect.y + rect.height <= white.y + white.height);
        }
    }

    #[test]
    fn a_failing_canvas_stops_the_drawing() {
        let mut calls = 0;
        let result = qr_rects(&qr(), Rect::new(0, 0, 400, 400), |_, _| {
            calls += 1;
            Err(Error::State)
        });
        assert_eq!((result, calls), (Err(Error::State), 1));
    }
}
