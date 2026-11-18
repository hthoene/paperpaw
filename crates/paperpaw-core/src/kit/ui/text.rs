//! Labels, icons, headings, messages and empty screens: text drawn in
//! its room, shortened where it does not fit.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::look::TextPart;
use super::*;

/// Lines of a row's note, e.g. a setting's description.
pub(crate) const NOTE_LINES: usize = 2;

/// Draws icon `name` ([`icons`](paperpaw_sdk::icons)) `size` pixels high,
/// vertically centred in `area`, starting at `x`; returns its width, 0 if
/// there is no icon font or no such icon.
pub fn draw_icon(
    canvas: &mut Canvas<'_>,
    env: &Place<'_>,
    name: &str,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
) -> f32 {
    let Some((font, ch)) = icon(env, name) else {
        return 0.0;
    };
    let mut text = [0u8; 4];
    let text = ch.encode_utf8(&mut text);
    draw_text(canvas, &font, None, size, x, area, text, color)
}

/// The width of icon `name` at `size` pixels, 0 if it cannot be drawn.
pub fn icon_width(env: &Place<'_>, name: &str, size: f32) -> f32 {
    icon(env, name).map_or(0.0, |(font, ch)| font.advance(size, ch))
}

/// The icon font and the character of icon `name`, where it has a drawing
/// (its own, or an icon pack's). A semantic icon
/// ([`icons::BACK`](paperpaw_sdk::icons::BACK) and the others) is the icon a
/// plugin named for it, else its default.
pub(crate) fn icon<'f>(env: &Place<'f>, name: &str) -> Option<(FontHandle<'f>, char)> {
    let name = if name.starts_with("core.icon.") {
        env.menus()
            .icon(name)
            .or_else(|| paperpaw_sdk::icons::default_of(name))?
    } else {
        name
    };
    let font = env
        .fonts()
        .bound(paperpaw_sdk::builtin::fonts::ICONS.as_str())?;
    let ch = paperpaw_sdk::icons::char(name)?;
    font.has_char(ch).then_some((font, ch))
}

/// Draws icon `name` `size` pixels high from `x` with its ink centred
/// on the line at `twice_middle` / 2 (twice the y, so that a centre
/// between two pixels is said exactly): to the pixel where the ink's
/// height and the line allow, else half a pixel lower. Its width, 0
/// where it has no drawing.
pub(crate) fn draw_icon_centred(
    canvas: &mut Canvas<'_>,
    env: &Place<'_>,
    name: &str,
    size: f32,
    x: f32,
    twice_middle: i32,
    color: Color,
) -> f32 {
    let Some((font, ch)) = icon(env, name) else {
        return 0.0;
    };
    let mut text = [0u8; 4];
    let text = ch.encode_utf8(&mut text);
    let Some((top, bottom)) = font.ink(size, text) else {
        return font.advance(size, ch);
    };
    let baseline = (twice_middle - top - bottom + 1).div_euclid(2);
    font.draw(canvas, size, x, baseline, text, color)
}

/// [`draw_label`] for text that follows the look: `role` says what the
/// text is ([`TextRole::BUTTON`], [`TextRole::HINT`], [`TextRole::TITLE`]
/// …) and the look's text painter, if it has one and draws it, paints it
/// (its width stays the stock one). Same placing, shortening and result as
/// [`draw_label`].
#[allow(clippy::too_many_arguments, reason = "draw_label's, and the look")]
pub(crate) fn draw_label_in(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    style: &UiStyle,
    role: TextRole,
    size: f32,
    x: f32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let at = Rect::new(x.max(0.0) as u16, area.y, room.max(0.0) as u16, area.height);
    canvas.describe(Role::Text, text, at);
    draw_text(
        canvas,
        font,
        Some((style, role)),
        size,
        x,
        area,
        text,
        color,
    )
}

/// [`draw_label`] without telling what it draws (icons say nothing), and
/// by the look's text painter where it has `look` (style and role).
#[allow(clippy::too_many_arguments, reason = "one run of text")]
pub(crate) fn draw_text(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    look: Option<(&UiStyle, TextRole)>,
    size: f32,
    x: f32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    let metrics = font.line_metrics(size);
    let baseline = i32::from(area.y)
        + ((f32::from(area.height) + metrics.ascent - metrics.descent) / 2.0) as i32;
    let room = f32::from(area.x) + f32::from(area.width) - x;
    // A pixel of slack for rounding: callers often measured to fit.
    if room > 0.0 && font.measure(size, text) > room + 1.0 {
        let text = fit(font, size, text, room);
        return put_run(canvas, font, look, size, x, baseline, area, &text, color);
    }
    put_run(canvas, font, look, size, x, baseline, area, text, color)
}

/// One run of text: by the look's text painter, if there is a `look`, it
/// has one and draws it, else as usual; its width.
#[allow(clippy::too_many_arguments, reason = "one run of text")]
pub(crate) fn put_run(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    look: Option<(&UiStyle, TextRole)>,
    size: f32,
    x: f32,
    baseline: i32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    if let Some((style, role)) = look
        && style.look().painters().is_some()
    {
        let part = TextPart::new(text, x, baseline, size, color, area, font, role);
        if style.look().text(canvas, style, &part) {
            // Looks, not metrics: the width is the stock one.
            return font.measure(size, text);
        }
    }
    font.draw(canvas, size, x, baseline, text, color)
}

/// `text` shortened with an ellipsis to fit `width` pixels, its pairs
/// kerned as [`FontHandle::measure`] sets them (a pair kerned apart, `f\`
/// in the UI font, takes more than its advances).
pub(crate) fn fit(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> String {
    if font.measure(size, text) <= width {
        return text.to_string();
    }
    let ellipsis = font.measure(size, "…");
    let mut result = String::new();
    let mut used = 0.0;
    let mut before = None;
    for c in text.chars() {
        let w = before.map_or(0.0, |b| font.kern(size, b, c)) + font.advance(size, c);
        if used + w + font.kern(size, c, '…') + ellipsis > width {
            break;
        }
        used += w;
        result.push(c);
        before = Some(c);
    }
    result.push('…');
    // Text of right-to-left letters is measured as drawn (in its order on
    // the line, Arabic letters joined), which the letters' advances above
    // only estimate: shortened on until that fits.
    if paperpaw_text::bidi::needs_bidi(text) {
        while font.measure(size, &result) > width {
            let cut = result.len() - '…'.len_utf8();
            let Some(last) = result[..cut].chars().next_back() else {
                break;
            };
            result.remove(cut - last.len_utf8());
        }
    }
    result
}
