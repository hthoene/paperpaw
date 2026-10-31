//! Text set as what lies around it ([`FontHandle::draw_in`],
//! [`FontHandle::measure_in`]) whose characters one font does not all
//! draw: a word in quotes or brackets, a Hebrew or Arabic word with a
//! colon or a full stop (the script's pack has its letters only), digits
//! beside letters. It is set in parts of one font and one direction each,
//! shown from the left in the order the text's levels give. Each part is
//! measured and drawn alone: no kerning across the edge of two parts (two
//! fonts kern nothing between them anyway).

use alloc::vec::Vec;

use paperpaw_lang::Direction;
use paperpaw_text::Around;
use paperpaw_text::bidi::{self, Level, visual_order};

use super::{Font, FontHandle};

/// Parts kept on the stack; texts of more allocate. A word has two or
/// three; a row of letters and full stops (`א.ב.ג`) one per character.
const PARTS: usize = 16;

/// Characters whose levels are kept on the stack; longer texts allocate.
const STACK: usize = 64;

/// One part: where it starts in the text, its font and the font's number
/// in the cache, its level.
#[derive(Clone, Copy)]
struct Part<'a> {
    start: usize,
    font: &'a Font,
    index: u16,
    level: Level,
}

/// Whether `c` is a mark drawn onto the character before it: the
/// combining blocks ([`paperpaw_text::is_mark`]) and the points and
/// vowel marks of Hebrew and Arabic (bidi class NSM there). It stays in
/// the part of its letter and is passed over as what lies around a part.
fn is_mark(c: char) -> bool {
    paperpaw_text::is_mark(c)
        || matches!(
            c,
            '\u{0591}'..='\u{05BD}'
                | '\u{05BF}'
                | '\u{05C1}'..='\u{05C2}'
                | '\u{05C4}'..='\u{05C5}'
                | '\u{05C7}'
                | '\u{0610}'..='\u{061A}'
                | '\u{064B}'..='\u{065F}'
                | '\u{0670}'
                | '\u{06D6}'..='\u{06DC}'
                | '\u{06DF}'..='\u{06E4}'
                | '\u{06E7}'..='\u{06E8}'
                | '\u{06EA}'..='\u{06ED}'
                | '\u{0816}'..='\u{082D}'
                | '\u{0898}'..='\u{089F}'
                | '\u{08CA}'..='\u{08E1}'
                | '\u{08E3}'..='\u{08FF}'
        )
}

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
        if self.others[0].is_none()
            || text
                .chars()
                .all(|c| self.drawn_by(first.0, c) || !self.has_char(c))
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
        let levels = &*levels;
        // The parts in reading order, counted first, then kept.
        let mut parts = 0usize;
        self.parts(text, levels, first, paragraph, |_| parts += 1);
        let start = Part {
            start: 0,
            font: first.0,
            index: first.1,
            level: paragraph,
        };
        let mut stack = ([start; PARTS], [paragraph; PARTS], [0u16; PARTS]);
        let mut heap = (Vec::new(), Vec::new(), Vec::new());
        let (kept, kept_levels, order) = match parts <= PARTS {
            true => (
                stack.0.get_mut(..parts).unwrap_or_default(),
                stack.1.get_mut(..parts).unwrap_or_default(),
                stack.2.get_mut(..parts).unwrap_or_default(),
            ),
            false => {
                heap.0.resize(parts, start);
                heap.1.resize(parts, paragraph);
                heap.2.resize(parts, 0);
                (
                    heap.0.as_mut_slice(),
                    heap.1.as_mut_slice(),
                    heap.2.as_mut_slice(),
                )
            }
        };
        let mut at = 0;
        self.parts(text, levels, first, paragraph, |part| {
            if let (Some(k), Some(l)) = (kept.get_mut(at), kept_levels.get_mut(at)) {
                (*k, *l) = (part, part.level);
            }
            at += 1;
        });
        let shown = visual_order(kept_levels, order);
        for &p in order.get(..shown).unwrap_or_default() {
            let p = usize::from(p);
            let Some(part) = kept.get(p) else {
                continue;
            };
            let end = kept.get(p + 1).map_or(text.len(), |next| next.start);
            let (Some(drawn), Some(ahead), Some(behind)) = (
                text.get(part.start..end),
                text.get(..part.start),
                text.get(end..),
            ) else {
                continue;
            };
            let before = ahead.chars().rev().find(|c| !is_mark(*c));
            let after = behind.chars().find(|c| !is_mark(*c));
            let base = match part.level.is_rtl() {
                true => Direction::Rtl,
                false => Direction::Ltr,
            };
            let here = Around::new(base, around.lang)
                .with_context(before.or(around.before), after.or(around.after))
                .with_flags(around.flags);
            each(part.font, part.index, drawn, &here);
        }
        true
    }

    /// Calls `put` for each part of `text` in reading order, `levels` one
    /// per character: a new one where the font or the level changes. A
    /// character goes on in the font of the part before it where that
    /// font draws it, a mark always (it sits on its letter); a character
    /// no font draws too, shown as that font shows missing ones. At most
    /// as many parts as the order of their levels can count (`u16`).
    fn parts(
        &self,
        text: &str,
        levels: &[Level],
        first: (&'a Font, u16),
        paragraph: Level,
        mut put: impl FnMut(Part<'a>),
    ) {
        let mut current: Option<Part<'a>> = None;
        let mut count = 0usize;
        for (i, (at, c)) in text.char_indices().enumerate() {
            let level = levels.get(i).copied().unwrap_or(paragraph);
            let (font, index) = match current {
                None => first,
                Some(part) if is_mark(c) || self.drawn_by(part.font, c) || !self.has_char(c) => {
                    (part.font, part.index)
                }
                Some(_) => self.pick(c),
            };
            let new = current.is_none_or(|part| part.index != index || part.level != level);
            if new && count < usize::from(u16::MAX) {
                if let Some(part) = current {
                    put(part);
                }
                current = Some(Part {
                    start: at,
                    font,
                    index,
                    level,
                });
                count += 1;
            }
        }
        if let Some(part) = current {
            put(part);
        }
    }

    /// Whether `font` draws `c` in a part: a character it has; a space
    /// only where no font has one (the packs of a script have none: it is
    /// drawn by the font that has it, as wide as it is there).
    fn drawn_by(&self, font: &Font, c: char) -> bool {
        font.has_char(c) || (c.is_whitespace() && !self.has_char(c))
    }
}
