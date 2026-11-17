//! Labels, icons, headings, messages and empty screens: text drawn in
//! its room, shortened where it does not fit.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::look::TextPart;
use super::*;



/// The width of icon `name` at `size` pixels, 0 if it cannot be drawn.
pub fn icon_width(env: &Env<'_>, name: &str, size: f32) -> f32 {
    icon(env, name).map_or(0.0, |(font, ch)| font.advance(size, ch))
}

/// The icon font and the character of icon `name`, where it has a drawing
/// (its own, or an icon pack's). A semantic icon
/// ([`icons::BACK`](crate::icons::BACK) and the others) is the icon a
/// plugin named for it, else its default.
pub(crate) fn icon<'f>(env: &Env<'f>, name: &str) -> Option<(FontHandle<'f>, char)> {
    let name = if name.starts_with("core.icon.") {
        env.menus()
            .icon(name)
            .or_else(|| crate::icons::default_of(name))?
    } else {
        name
    };
    let font = env.fonts().bound(crate::builtin::fonts::ICONS.as_str())?;
    let ch = crate::icons::char(name)?;
    font.has_char(ch).then_some((font, ch))
}

/// Draws icon `name` `size` pixels high from `x` with its ink centred
/// on the line at `twice_middle` / 2 (twice the y, so that a centre
/// between two pixels is said exactly): to the pixel where the ink's
/// height and the line allow, else half a pixel lower. Its width, 0
/// where it has no drawing.
pub(crate) fn draw_icon_centred(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
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
