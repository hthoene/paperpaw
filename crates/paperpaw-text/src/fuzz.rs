//! What the fuzz targets check of the crate's private parts.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::arabic::{self, Context, Place, Side};
use crate::bidi::{self, Carry, Level};

/// Any text through the bidirectional algorithm and Arabic shaping:
/// resolved whole and in two lines with the carry kept, it must give the
/// same levels, a visual order that is a permutation, and one key per
/// char. The first byte picks the paragraph's level and the break.
pub fn bidi(data: &[u8]) {
    let Some((&head, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let paragraph = match head & 3 {
        0 => Level::LTR,
        1 => Level::RTL,
        _ => bidi::paragraph_level(&text).unwrap_or(Level::LTR),
    };
    let cut = if n > 1 { usize::from(head >> 2) % n } else { 0 };

    let mut whole = vec![Level::LTR; n];
    assert_eq!(
        Carry::new(paragraph).resolve(text.chars(), n, &mut whole),
        n
    );

    let mut carry = Carry::new(paragraph);
    let mut lines = vec![Level::LTR; n];
    carry.resolve(chars.iter().copied(), cut, &mut lines[..cut]);
    carry.resolve(chars[cut..].iter().copied(), n - cut, &mut lines[cut..]);
    assert_eq!(lines, whole);

    let first: String = chars[..cut].iter().collect();
    let second: String = chars[cut..].iter().collect();
    bidi::reset_line_end(&first, &mut lines[..cut], paragraph);
    bidi::reset_line_end(&second, &mut lines[cut..], paragraph);
    let mut order = vec![0u16; n];
    let shown = bidi::visual_order(&lines, &mut order);
    let mut seen = vec![false; shown];
    for &i in &order[..shown] {
        let i = usize::from(i);
        assert!(i < shown && !seen[i]);
        seen[i] = true;
    }

    assert_eq!(arabic::keys(&text, Context::NONE).count(), n);
    let mut places = [Place::new(Side::Above, 0); 8];
    let marks: Vec<char> = chars
        .iter()
        .copied()
        .filter(|&c| arabic::is_mark(c))
        .take(8)
        .collect();
    arabic::place_marks(&marks, &mut places);
}
