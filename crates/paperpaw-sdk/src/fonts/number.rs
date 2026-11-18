//! Numbers whose digits another font draws, set at one size (#131).

use super::{FontHandle, LARGER_STEP};

impl FontHandle<'_> {
    /// `size`, or for a number (a clock's time) with digits another font
    /// draws for this one, a pack with no size near it (Persian digits
    /// beside a scalable font's colon), the largest size that pack has:
    /// the number drawn at one size, not its digits small beside a large
    /// colon (#131). Text of letters keeps `size`, as measured a
    /// character at a time.
    pub(super) fn reached(&self, size: f32, text: &str) -> f32 {
        if self.others[0].is_none() || !is_number(text) {
            return size;
        }
        text.chars()
            .filter(|&ch| !self.font.has_char(ch))
            .map(|ch| self.pick(ch))
            .filter(|&(_, index)| index != self.index)
            .filter_map(|(font, _)| font.largest_size())
            .filter(|&largest| size >= largest + LARGER_STEP / 2.0)
            .fold(size, f32::min)
    }
}

/// Whether `text` is a number, a clock's time or a date in digits: digits
/// of any script, spaces and the signs between them.
fn is_number(text: &str) -> bool {
    text.chars().all(|ch| {
        ch.is_numeric()
            || ch.is_whitespace()
            || ch.is_ascii_punctuation()
            || matches!(ch, '\u{60c}' | '\u{66a}'..='\u{66c}')
    })
}
