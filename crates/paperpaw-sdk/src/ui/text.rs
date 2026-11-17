//! Labels, icons, headings, messages and empty screens: text drawn in
//! its room, shortened where it does not fit.

use super::*;









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
