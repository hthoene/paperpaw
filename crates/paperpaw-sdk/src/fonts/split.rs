//! Text set as what lies around it ([`FontHandle::draw_in`],
//! [`FontHandle::measure_in`]) whose characters one font does not all
//! draw: a word in quotes or brackets, a Hebrew or Arabic word with a
//! colon or a full stop (the script's pack has its letters only), digits
//! beside letters. It is set in parts of one font and one direction each,
//! shown from the left in the order the text's levels give.

use alloc::vec::Vec;

use paperpaw_lang::Direction;
use paperpaw_text::Around;
use paperpaw_text::bidi::{self, Level, visual_order};

use super::{Font, FontHandle};

/// Most parts a text is set in; past it, the rest goes on in the last
/// part's font and direction (its letters another font draws show as that
/// font shows missing ones). A word has two or three.
const PARTS: usize = 16;

/// Characters whose levels are kept on the stack; longer texts allocate.
const STACK: usize = 64;

impl<'a> FontHandle<'a> {
    /// Calls `each` for the parts of `text` that one font draws in one
    /// direction, from the left: the font, its number in the cache, the
    /// part and what lies around it. `false`, calling nothing, where one
    /// font draws all of it that any of them draws.
    pub(super) fn set_in_parts(
        &self,
        text: &str,
        around: &Around,
        mut each: impl FnMut(&'a Font, u16, &str, &Around),
    ) -> bool {
        let first = self.pick_text(text);
        let drawn_by = |font: &Font, c: char| c.is_whitespace() || font.has_char(c);
        if self.others[0].is_none()
            || text
                .chars()
                .all(|c| drawn_by(first.0, c) || !self.has_char(c))
        {
            return false;
        }
        let count = text.chars().count();
        let mut stack = [Level::LTR; STACK];
        let mut heap = Vec::new();
        let levels = match count <= STACK {
            true => stack.get_mut(..count).unwrap_or_default(),
            false => {
                heap.resize(count, Level::LTR);
                heap.as_mut_slice()
            }
        };
        let paragraph = match around.base {
            Direction::Rtl => Level::RTL,
            Direction::Ltr => Level::LTR,
            _ => bidi::paragraph_level(text).unwrap_or(Level::LTR),
        };
        if paragraph.is_rtl() || bidi::needs_bidi(text) {
            bidi::resolve(text, paragraph, levels);
            bidi::reset_line_end(text, levels, paragraph);
        }
        // The parts in reading order: where each starts, its font, level.
        let mut starts = [0usize; PARTS];
        let mut fonts = [first; PARTS];
        let mut kept = [paragraph; PARTS];
        let mut parts: usize = 0;
        let mut current = first;
        for (i, (at, c)) in text.char_indices().enumerate() {
            let level = levels.get(i).copied().unwrap_or(paragraph);
            if !drawn_by(current.0, c) && self.has_char(c) {
                current = self.pick(c);
            }
            let new = match parts.checked_sub(1) {
                None => true,
                Some(last) => fonts[last].1 != current.1 || kept[last] != level,
            };
            if new && parts < PARTS {
                starts[parts] = at;
                fonts[parts] = current;
                kept[parts] = level;
                parts += 1;
            } else if let Some(last) = parts.checked_sub(1) {
                current = fonts[last];
            }
        }
        let mut order = [0u16; PARTS];
        let shown = visual_order(&kept[..parts], &mut order[..parts]);
        let not_mark = |c: &char| !paperpaw_text::is_mark(*c);
        for &p in &order[..shown] {
            let p = usize::from(p);
            let start = starts[p];
            let end = starts.get(p + 1).filter(|_| p + 1 < parts).copied();
            let end = end.unwrap_or(text.len());
            let (Some(part), Some((font, index))) = (text.get(start..end), fonts.get(p)) else {
                continue;
            };
            let before = text[..start].chars().rev().find(not_mark).or(around.before);
            let after = text[end..].chars().find(not_mark).or(around.after);
            let base = match kept[p].is_rtl() {
                true => Direction::Rtl,
                false => Direction::Ltr,
            };
            let here = Around::new(base, around.lang)
                .with_context(before, after)
                .with_flags(around.flags);
            each(font, *index, part, &here);
        }
        true
    }
}
