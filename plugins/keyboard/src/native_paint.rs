//! How the keyboard is drawn: the text field, the keys and the variants of a
//! letter above them, laid out as owned [`Paint`](paperpaw_ui_kit::paint::Paint) operations for the draw
//! callback to replay.
use crate::keys::{Key, Layer};
use crate::native_model::{Board, FIELD_ROW, FieldButton, Popup, Shift, title_height};
use crate::{SHAPE_ROUND, SHAPE_WIDE};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error,
        canvas::PartRole,
        fonts::{Font, TextOrigin},
    },
};
use paperpaw_ui_kit::{
    Paint, Style,
    mem::push as grow,
    paint::{describe, push},
    text::{cap_baseline, copy, fit, wrap_with},
};

/// The words the keyboard shows, found by the plugin from its catalog.
#[derive(Debug, Default)]
pub struct Words {
    pub letters_key: String,
    pub symbols_key: String,
    pub done: String,
    pub phone: String,
    pub paste: String,
    pub hold_back: String,
    /// "Hold: ä" for the focused letter, when it holds others.
    pub hold_for: Option<String>,
}

/// Lays the whole keyboard out.
///
/// # Errors
/// Propagates font and allocation errors.
pub fn paint(
    board: &Board,
    style: &Style,
    shape: &str,
    font: &Font<'_>,
    words: &Words,
    out: &mut Vec<Paint>,
) -> Result<(), Error> {
    let r = board.region;
    push(out, Paint::Fill(r, style.paper))?;
    let rule = (u32::from(style.line) * 2).max(2) as u16;
    push(
        out,
        Paint::Fill(Rect::new(r.x, r.y, r.width, rule), style.ink),
    )?;
    field(board, style, font, words, out)?;
    for row in 0..board.rows.len() {
        for index in 0..board.rows[row].len() {
            key(board, style, shape, font, words, row, index, out)?;
        }
    }
    if let Some(popup) = &board.popup {
        variants(board, style, shape, font, popup, out)?;
    }
    Ok(())
}

/// A stroked or filled box with rounded corners.
fn boxed(
    out: &mut Vec<Paint>,
    area: Rect,
    radius: u32,
    thickness: Option<u32>,
    color: Color,
) -> Result<(), Error> {
    push(out, Paint::Focus(area, radius, thickness, color))
}

/// `text` at `x`, centred in `area` from above to below.
fn text_at(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    text: &str,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
) -> Result<(), Error> {
    let baseline = cap_baseline(font, size, area)?;
    push(
        out,
        Paint::Text {
            text: copy(text)?,
            size,
            origin: TextOrigin::new(x, baseline),
            color,
            clip: Some(area),
        },
    )
}

/// `text` in the middle of `area`.
fn centred(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    text: &str,
    size: f32,
    area: Rect,
    color: Color,
) -> Result<(), Error> {
    let width = font.measure_text(size, text)?;
    let x = f32::from(area.x) + (f32::from(area.width) - width) / 2.0;
    text_at(out, font, text, size, x, area, color)
}

fn field(
    board: &Board,
    style: &Style,
    font: &Font<'_>,
    words: &Words,
    out: &mut Vec<Paint>,
) -> Result<(), Error> {
    let margin = f32::from(style.margin);
    let f = board.field;
    let title_area = Rect::new(f.x, f.y, f.width, title_height(&board.look));
    // With buttons, on a letter that holds others: that it does, at the right
    // end; else how typing ends (held Back).
    let held = words
        .hold_for
        .as_deref()
        .or_else(|| (!board.look.touch).then_some(words.hold_back.as_str()));
    let mut held_width = 0.0;
    if let Some(held) = held {
        let width = font.measure_text(style.small, held)?;
        held_width = width + margin;
        let x = f32::from(f.x + f.width) - margin - width;
        text_at(out, font, held, style.small, x, title_area, style.ink)?;
    }
    let title = fit(
        font,
        style.small,
        &board.title,
        f32::from(f.width) - 2.0 * margin - held_width,
    )?;
    text_at(
        out,
        font,
        &title,
        style.small,
        margin,
        title_area,
        style.muted,
    )?;
    let box_area = board.text_box();
    let thickness = u32::from(style.line).max(2);
    // Where text is typed: the text drawn inside it is what it holds.
    describe(
        out,
        style.describing,
        PartRole::Field,
        box_area,
        false,
        false,
    )?;
    boxed(out, box_area, style.corner, Some(thickness), style.ink)?;
    let buttons_width = board.buttons.first().map_or(0.0, |(_, area)| {
        f32::from(box_area.x + box_area.width - area.x) + margin
    });
    let text = shown_text(board)?;
    if board.asked.multiline {
        lines(board, style, font, &text, box_area, out)?;
    } else {
        let room = f32::from(box_area.width) - margin * 1.5 - buttons_width;
        // The longest end of it that fits, found by halving.
        let starts: Vec<usize> = {
            let mut v = Vec::new();
            v.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
            v.extend(text.char_indices().map(|(i, _)| i));
            v
        };
        let (mut low, mut high) = (0, starts.len());
        while low < high {
            let mid = (low + high) / 2;
            if font.measure_text(style.text, &text[starts[mid]..])? <= room {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        let start = starts.get(low).copied().unwrap_or(text.len());
        let shown = &text[start..];
        let width = font.measure_text(style.text, shown)?;
        let x = f32::from(box_area.x) + margin / 2.0;
        text_at(out, font, shown, style.text, x, box_area, style.ink)?;
        cursor(
            out,
            style,
            x + width,
            f32::from(box_area.y),
            box_area.height,
        )?;
    }
    for (index, &(kind, area)) in board.buttons.iter().enumerate() {
        let focused = board.focus == Some((FIELD_ROW, index));
        let label = match kind {
            FieldButton::Phone => &words.phone,
            FieldButton::Paste => &words.paste,
        };
        button(out, style, font, area, label, focused)?;
    }
    Ok(())
}

/// What the field shows: the text, or dots for a password.
fn shown_text(board: &Board) -> Result<String, Error> {
    if !board.asked.secret {
        return copy(&board.text);
    }
    let mut dots = String::new();
    dots.try_reserve(board.text.chars().count() * 3)
        .map_err(|_| Error::NoMemory)?;
    for _ in board.text.chars() {
        dots.push('•');
    }
    Ok(dots)
}

fn cursor(out: &mut Vec<Paint>, style: &Style, x: f32, top: f32, height: u16) -> Result<(), Error> {
    let cursor = style.text as u32;
    let y = top as i32 + (i32::from(height) - cursor as i32) / 2;
    push(
        out,
        Paint::Fill(
            Rect::new(
                (x as i32 + 2).clamp(0, 65_000) as u16,
                y.clamp(0, 65_000) as u16,
                style.line.max(2),
                cursor.min(65_000) as u16,
            ),
            style.ink,
        ),
    )
}

/// A button of the field: outlined, filled while it has the focus.
fn button(
    out: &mut Vec<Paint>,
    style: &Style,
    font: &Font<'_>,
    area: Rect,
    label: &str,
    focused: bool,
) -> Result<(), Error> {
    // A line of text is a little taller than this button: the part reaches the
    // text's box too, or the field around it would be told the label.
    let around = area.grow(style.line.max(2));
    describe(
        out,
        style.describing,
        PartRole::Button,
        around,
        focused,
        false,
    )?;
    let radius = style.corner.max(u32::from(area.height) / 4);
    let thickness = u32::from(style.line).max(1);
    if focused {
        boxed(out, area, radius, None, style.ink)?;
        centred(out, font, label, style.small, area, style.paper)
    } else {
        boxed(out, area, radius, Some(thickness), style.ink)?;
        centred(out, font, label, style.small, area, style.ink)
    }
}

/// The text of several lines, wrapped to the box: the last lines that fit in
/// it, so the cursor at the end is always in view.
fn lines(
    board: &Board,
    style: &Style,
    font: &Font<'_>,
    text: &str,
    box_area: Rect,
    out: &mut Vec<Paint>,
) -> Result<(), Error> {
    let pad = f32::from(style.margin) / 2.0;
    let size = style.text;
    let line = f32::from(board.look.text_line.max(1));
    let width = (f32::from(box_area.width) - 2.0 * pad).max(1.0);
    let fits = ((f32::from(box_area.height) - pad) / line).max(1.0) as usize;
    let mut wrapped: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            grow(&mut wrapped, String::new())?;
            continue;
        }
        let parts = wrap_with(|s| font.measure_text(size, s), paragraph, width, 1_000)?;
        for part in parts {
            grow(&mut wrapped, part)?;
        }
    }
    let first = wrapped.len().saturating_sub(fits);
    let top = f32::from(box_area.y) + pad / 2.0;
    let mut end = (0.0, top);
    for (n, text) in wrapped[first..].iter().enumerate() {
        let y = top + line * n as f32;
        let row = Rect::new(box_area.x, y as u16, box_area.width, line as u16);
        let w = font.measure_text(size, text)?;
        let x = f32::from(box_area.x) + pad;
        text_at(out, font, text, size, x, row, style.ink)?;
        end = (x + w, y);
    }
    cursor(out, style, end.0, end.1, line as u16)
}

/// The corner radius of a key `height` high, by the key shape.
fn radius(board: &Board, style: &Style, shape: &str, height: u16) -> u32 {
    if !board.look.touch {
        return (style.text / 4.0) as u32;
    }
    match shape {
        SHAPE_ROUND => u32::from(height) / 2,
        SHAPE_WIDE => u32::from(height) / 4,
        _ => style.corner,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one key and what it is drawn with"
)]
fn key(
    board: &Board,
    style: &Style,
    shape: &str,
    font: &Font<'_>,
    words: &Words,
    row: usize,
    index: usize,
    out: &mut Vec<Paint>,
) -> Result<(), Error> {
    let Some(key) = board.key(row, index) else {
        return Ok(());
    };
    let area = board.key_area(row, index);
    let focused = board.focus == Some((row, index));
    // Tapped, keys fill their place but for a seam; the focused one (after a
    // button was used) smaller by its frame. With buttons they fill it but for
    // a pixel on each side, and the focused one is filled.
    let frame = style.line.max(1) * 3;
    let pad = if focused && board.look.touch {
        frame
    } else {
        board.key_inset() as u16
    };
    let h = area.height.saturating_sub(2 * pad);
    let r = Rect::new(
        area.x + pad,
        area.y + (area.height - h) / 2,
        area.width.saturating_sub(2 * pad),
        h,
    );
    // Shift while capitals are locked, and *OK* always: the key that ends
    // typing stands out.
    let active = (key == Key::Shift && board.shift == Shift::Locked) || key == Key::Done;
    let filled = active || (focused && !board.look.touch);
    let (ink, paper) = if filled {
        (style.paper, style.ink)
    } else {
        (style.ink, style.paper)
    };
    describe(out, style.describing, PartRole::Button, r, focused, false)?;
    let radius = radius(board, style, shape, r.height);
    let thickness = u32::from(style.line).max(1) * if focused && board.look.touch { 3 } else { 1 };
    if filled {
        boxed(out, r, radius, None, paper)?;
    } else {
        boxed(out, r, radius, Some(thickness), ink)?;
    }
    let size = if board.look.touch {
        style.text
    } else {
        style.text * board.letters as f32 / 100.0
    };
    let (cx, cy) = (
        i32::from(r.x) + i32::from(r.width) / 2,
        i32::from(r.y) + i32::from(r.height) / 2,
    );
    let glyph = (style.text * 0.6) as i32;
    match key {
        Key::Shift => arrow_up(out, cx, cy, glyph, ink),
        Key::Delete => arrow_left(out, cx, cy, glyph, ink),
        Key::Enter => enter_arrow(out, cx, cy, glyph, ink),
        Key::Space => space(out, style, r, cx, cy, glyph, ink),
        _ => {
            let mut buffer = [0u8; 4];
            let label: &str = match key {
                Key::Char(c) => board.cased(c).encode_utf8(&mut buffer),
                Key::Layer(Layer::Letters) => &words.letters_key,
                Key::Layer(_) => &words.symbols_key,
                Key::NextLayout => board
                    .layouts
                    .get(board.current)
                    .map_or("", |l| l.0.as_str()),
                _ => &words.done,
            };
            centred(out, font, label, size, r, ink)
        }
    }
}

/// An arrow pointing up (*Shift*), centred at `(x, y)`.
fn arrow_up(out: &mut Vec<Paint>, x: i32, y: i32, size: i32, color: Color) -> Result<(), Error> {
    let half = size / 2;
    for dy in 0..=half {
        bar(out, x - dy, y - half + dy, (dy * 2 + 1) as u32, 1, color)?;
    }
    let stem = (size / 4).max(2);
    bar(out, x - stem / 2, y, stem as u32, half as u32, color)
}

/// An arrow pointing left (*Delete*), centred at `(x, y)`.
fn arrow_left(out: &mut Vec<Paint>, x: i32, y: i32, size: i32, color: Color) -> Result<(), Error> {
    let half = size / 2;
    for dx in 0..=half {
        bar(out, x - half + dx, y - dx, 1, (dx * 2 + 1) as u32, color)?;
    }
    let stem = (size / 4).max(2);
    bar(out, x, y - stem / 2, half as u32, stem as u32, color)
}

/// The arrow of a line break: a stem down, then an arrow back along the line.
fn enter_arrow(out: &mut Vec<Paint>, x: i32, y: i32, size: i32, color: Color) -> Result<(), Error> {
    let half = size / 2;
    let t = (size / 4).max(2);
    let base = y + size / 4;
    for dx in 0..=half {
        bar(out, x - half + dx, base - dx, 1, (dx * 2 + 1) as u32, color)?;
    }
    bar(
        out,
        x - half,
        base - t / 2,
        (half * 2) as u32,
        t as u32,
        color,
    )?;
    bar(
        out,
        x + half - t,
        y - half,
        t as u32,
        (base + t / 2 - (y - half)) as u32,
        color,
    )
}

/// The space bar's glyph: a bracket open at the top.
fn space(
    out: &mut Vec<Paint>,
    style: &Style,
    r: Rect,
    cx: i32,
    cy: i32,
    size: i32,
    color: Color,
) -> Result<(), Error> {
    let w = i32::from(r.width) / 3;
    let t = i32::from(style.line).max(2);
    bar(out, cx - w / 2, cy + size / 3, w as u32, t as u32, color)?;
    bar(out, cx - w / 2, cy, t as u32, (size / 3 + t) as u32, color)?;
    bar(
        out,
        cx + w / 2 - t,
        cy,
        t as u32,
        (size / 3 + t) as u32,
        color,
    )
}

/// A filled bar of the glyphs.
fn bar(out: &mut Vec<Paint>, x: i32, y: i32, w: u32, h: u32, color: Color) -> Result<(), Error> {
    if w == 0 || h == 0 {
        return Ok(());
    }
    let clamp = |v: i32| v.clamp(0, 65_000) as u16;
    push(
        out,
        Paint::Fill(
            Rect::new(
                clamp(x),
                clamp(y),
                w.min(65_000) as u16,
                h.min(65_000) as u16,
            ),
            color,
        ),
    )
}

fn variants(
    board: &Board,
    style: &Style,
    shape: &str,
    font: &Font<'_>,
    popup: &Popup,
    out: &mut Vec<Paint>,
) -> Result<(), Error> {
    let a = popup.area;
    let border = (u32::from(style.line) * 2).max(2);
    let outer = border as u16 * 2;
    let frame = Rect::new(
        a.x.saturating_sub(outer),
        a.y.saturating_sub(outer),
        a.width + 2 * outer,
        a.height + 2 * outer,
    );
    push(out, Paint::Fill(frame, style.paper))?;
    boxed(out, frame, 0, Some(border), style.ink)?;
    for (index, c) in popup.chars.iter().enumerate() {
        let key = popup.key(index);
        let pad = style.line.max(1) * 3;
        let r = Rect::new(
            key.x + pad,
            key.y + pad,
            key.width.saturating_sub(2 * pad),
            key.height.saturating_sub(2 * pad),
        );
        let mut buffer = [0u8; 4];
        let label = c.encode_utf8(&mut buffer);
        let radius = radius(board, style, shape, r.height);
        describe(
            out,
            style.describing,
            PartRole::Button,
            r,
            popup.focus == Some(index),
            false,
        )?;
        if popup.focus == Some(index) {
            boxed(out, r, radius, None, style.ink)?;
            centred(out, font, label, style.text, r, style.paper)?;
        } else {
            boxed(
                out,
                r,
                radius,
                Some(u32::from(style.line).max(1)),
                style.ink,
            )?;
            centred(out, font, label, style.text, r, style.ink)?;
        }
    }
    Ok(())
}
