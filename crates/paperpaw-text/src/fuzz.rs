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

/// Any bytes as a font pack's substitution table: applied to a few keys
/// (Devanagari, Thai and plane 15 keys), it must not panic, and the keys
/// must not grow past the limit.
pub fn substitutions(data: &[u8]) {
    let Some(table) = crate::substitutions::Substitutions::new(data) else {
        return;
    };
    let mut keys = vec![0x915, 0x94D, 0x937, 0xE33, 0xF_0000, 0x20];
    for feature in table.features() {
        let _ = (feature.tag, feature.applies(feature.script, "hi"));
    }
    table.apply(0, "", &mut keys, 12);
    table.apply(5, "hi", &mut keys, 12);
    assert!(keys.len() <= 12);
}

/// Any bytes as a substitution table (its length in the first two bytes)
/// and text (the rest): shaping each run of the text with the table must
/// not panic and gives at most `MAX_SHAPED` keys a run; the part drawn as
/// one around any character holds it.
pub fn indic(data: &[u8]) {
    use crate::indic::{self, Glyphs, MAX_SHAPED, Shaper};
    let Some((len, rest)) = data.split_first_chunk::<2>() else {
        return;
    };
    let cut = usize::from(u16::from_le_bytes(*len)).min(rest.len());
    let (table, text) = rest.split_at(cut);
    let text = String::from_utf8_lossy(text);
    let shaper = Shaper::new(crate::substitutions::Substitutions::new(table));
    let mut glyphs = Glyphs::default();
    let mut at = 0;
    while at < text.len() {
        let end = indic::run_end(&text, at);
        assert!(end > at && end <= text.len());
        shaper.shape(&text[at..end], at == 0, &mut glyphs);
        assert!(glyphs.keys().count() <= MAX_SHAPED);
        at = end;
    }
    for (i, _) in text.char_indices() {
        let part = indic::shaped_at(&text, i);
        assert!(part.start <= i && i < part.end && part.end <= text.len());
    }
}
