//! The Unicode Bidirectional Algorithm (UAX #9) for text in more than
//! one direction, such as Arabic or Hebrew with Latin words and numbers.
//!
//! It works without allocation and without a buffer for the paragraph.
//! A layout resolves a paragraph's levels ([`resolve`]), gives each
//! line's ends the paragraph's level (rule L1, [`reset_line_end`]) and
//! draws its runs in the order the levels give (rule L2,
//! [`visual_order`]), characters with mirrors mirrored ([`mirror`]) at
//! odd levels.
//!
//! The test `the_example` resolves a paragraph and puts it in visual order.
//!
//! Limits, chosen for small devices and stated by UAX #9 as allowed
//! deviations of the depth: at most [`DEPTH`] embeddings and isolates are
//! open at once (deeper ones count as overflow, as past level 125 in the
//! standard), at most [`ISOLATES`] isolates with a matching PDI nest, and
//! at most [`BRACKETS`] brackets are open in a paragraph (more stop the
//! pairing of brackets, as past 63 in the standard).

#[rustfmt::skip]
mod tables;
mod walk;

#[cfg(test)]
mod conformance;
#[cfg(test)]
mod tests;

pub(crate) use walk::Carry;
#[cfg(test)]
use walk::{BRACKETS, DEPTH, ISOLATES};

/// The bidirectional class of a character (`Bidi_Class`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[allow(
    clippy::upper_case_acronyms,
    reason = "the names UAX #9 gives the classes"
)]
#[non_exhaustive]
pub(crate) enum Class {
    /// Left-to-right.
    L,
    /// Right-to-left (Hebrew and others).
    R,
    /// Right-to-left Arabic.
    AL,
    /// European number.
    EN,
    /// European number separator.
    ES,
    /// European number terminator.
    ET,
    /// Arabic number.
    AN,
    /// Common number separator.
    CS,
    /// Nonspacing mark.
    NSM,
    /// Boundary neutral.
    BN,
    /// Paragraph separator.
    B,
    /// Segment separator.
    S,
    /// Whitespace.
    WS,
    /// Other neutral.
    ON,
    /// Left-to-right embedding.
    LRE,
    /// Left-to-right override.
    LRO,
    /// Right-to-left embedding.
    RLE,
    /// Right-to-left override.
    RLO,
    /// Pop directional format.
    PDF,
    /// Left-to-right isolate.
    LRI,
    /// Right-to-left isolate.
    RLI,
    /// First strong isolate.
    FSI,
    /// Pop directional isolate.
    PDI,
}

/// The classes in the order the generated tables number them.
const CLASSES: [Class; 23] = [
    Class::L,
    Class::R,
    Class::AL,
    Class::EN,
    Class::ES,
    Class::ET,
    Class::AN,
    Class::CS,
    Class::NSM,
    Class::BN,
    Class::B,
    Class::S,
    Class::WS,
    Class::ON,
    Class::LRE,
    Class::LRO,
    Class::RLE,
    Class::RLO,
    Class::PDF,
    Class::LRI,
    Class::RLI,
    Class::FSI,
    Class::PDI,
];

/// The bidirectional class of `ch`, from the Unicode Character Database
/// (unassigned code points take their block's default).
#[must_use]
pub(crate) fn class(ch: char) -> Class {
    let cp = u32::from(ch);
    let index = if let Ok(cp) = u16::try_from(cp) {
        run(&tables::BMP_STARTS, cp).and_then(|i| tables::BMP_CLASSES.get(i))
    } else {
        run(&tables::REST_STARTS, cp).and_then(|i| tables::REST_CLASSES.get(i))
    };
    index
        .and_then(|&i| CLASSES.get(usize::from(i)))
        .copied()
        .unwrap_or(Class::L)
}

/// The index of the run of sorted `starts` that holds `value`.
fn run<T: Ord + Copy>(starts: &[T], value: T) -> Option<usize> {
    match starts.binary_search(&value) {
        Ok(i) => Some(i),
        Err(0) => None,
        Err(i) => Some(i - 1),
    }
}

/// An embedding level: even is left-to-right, odd right-to-left.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Level(u8);

impl Level {
    /// A left-to-right paragraph.
    pub const LTR: Level = Level(0);
    /// A right-to-left paragraph.
    pub const RTL: Level = Level(1);
    /// The highest level UAX #9 knows.
    pub const MAX: u8 = 125;

    /// The level `number`, if it is one (at most [`Level::MAX`] + 1, the
    /// level a number in the deepest right-to-left embedding gets).
    #[must_use]
    pub const fn new(number: u8) -> Option<Level> {
        if number <= Level::MAX + 1 {
            Some(Level(number))
        } else {
            None
        }
    }

    /// The level as a number.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.0
    }

    /// Whether text at this level runs right to left.
    #[must_use]
    pub const fn is_rtl(self) -> bool {
        self.0 & 1 == 1
    }
}

/// Resolves the levels of a paragraph's characters (rules X1 to I2), one
/// per character of `text` into `levels`, `paragraph` its level (from
/// [`paragraph_level`], or the book's direction). Returns how many it
/// wrote: one per character, or fewer where `levels` is shorter. The
/// levels are before rule L1 ([`reset_line_end`]), which each line
/// applies to its own.
pub fn resolve(text: &str, paragraph: Level, levels: &mut [Level]) -> usize {
    let mut carry = Carry::new(paragraph);
    carry.resolve(text.chars(), levels.len(), levels)
}

/// Bytes the algorithm's state at a point of a paragraph takes: what a
/// reader keeps to go on from there, e.g. at the start of each piece of
/// a paragraph split in pieces. The bytes are the algorithm's own: their
/// size and meaning may change with any release, so it is told by a
/// function, and a state is only good for the build that wrote it.
#[must_use]
pub fn state_len() -> usize {
    walk::STATE_LEN
}

/// Writes into `state` (at least [`state_len`] long) the state at the
/// start of a paragraph at `paragraph`'s level. False where `state` is
/// shorter.
pub fn start_state(paragraph: Level, state: &mut [u8]) -> bool {
    Carry::new(paragraph).encode(state)
}

/// The paragraph level `state` holds; none where it is no state.
#[must_use]
pub fn state_paragraph(state: &[u8]) -> Option<Level> {
    Carry::decode(state).map(|c| c.paragraph())
}

/// As [`resolve`], from the point of a paragraph `state` says (see
/// [`start_state`]), and moves the state on past the characters
/// resolved. `rest` runs from that point to as far as the reader reads
/// ahead: the paragraph's end, or the end of its next piece (what lies
/// after decides neutrals, numbers and brackets near the end of the
/// resolved text). Resolves the next `count` characters into `levels`
/// and returns how many it wrote. A `state` that is not one (another
/// build's, or too short) is taken as the paragraph's start at
/// `paragraph`, and written anew.
pub fn resolve_from<I>(
    state: &mut [u8],
    paragraph: Level,
    rest: I,
    count: usize,
    levels: &mut [Level],
) -> usize
where
    I: Iterator<Item = char> + Clone,
{
    let mut carry = Carry::decode(state).unwrap_or_else(|| Carry::new(paragraph));
    let written = carry.resolve(rest, count, levels);
    carry.encode(state);
    written
}

/// Moves `state` on past the next `count` characters of `rest` without
/// their levels (see [`resolve_from`]): to reach a later piece of a
/// paragraph.
pub fn skip_from<I>(state: &mut [u8], paragraph: Level, rest: I, count: usize)
where
    I: Iterator<Item = char> + Clone,
{
    let mut carry = Carry::decode(state).unwrap_or_else(|| Carry::new(paragraph));
    carry.skip(rest, count);
    carry.encode(state);
}

/// The paragraph's level from its first strong character (rules P2 and
/// P3), skipping what lies inside isolates. `None` where it has none:
/// the caller then takes the book's or the interface's direction (rule
/// HL1), or left-to-right.
#[must_use]
pub fn paragraph_level(text: &str) -> Option<Level> {
    walk::first_strong(text.chars(), false)
}

/// The character drawn for `ch` at a right-to-left level (rule L4,
/// `Bidi_Mirroring_Glyph`): `(` for `)`, `»` for `«`. `ch` itself where it
/// has no mirror.
#[must_use]
pub(crate) fn mirror(ch: char) -> char {
    let Ok(cp) = u16::try_from(u32::from(ch)) else {
        return ch;
    };
    match tables::MIRRORS.binary_search_by_key(&cp, |&(c, _)| c) {
        Ok(i) => tables::MIRRORS
            .get(i)
            .and_then(|&(_, m)| char::from_u32(u32::from(m)))
            .unwrap_or(ch),
        Err(_) => ch,
    }
}

/// Whether rule X9 leaves `ch` out of the algorithm: embeddings,
/// overrides, `PDF` and boundary neutrals (ZWJ, ZWNJ, soft hyphen). It
/// gets the level of the character before it; it draws nothing.
#[cfg(test)]
fn is_removed(ch: char) -> bool {
    matches!(
        class(ch),
        Class::RLE | Class::LRE | Class::RLO | Class::LRO | Class::PDF | Class::BN
    )
}

/// Whether text in a left-to-right paragraph needs the algorithm at all:
/// it holds a right-to-left letter, an Arabic number or a directional
/// control. Quotes, dashes, ellipses and ligatures do not count, so
/// Latin text answers `false` at the cost of a scan of its bytes.
#[must_use]
pub fn needs_bidi(text: &str) -> bool {
    text.chars().any(|ch| {
        u32::from(ch) >= 0x0590
            && matches!(
                class(ch),
                Class::R
                    | Class::AL
                    | Class::AN
                    | Class::LRE
                    | Class::LRO
                    | Class::RLE
                    | Class::RLO
                    | Class::PDF
                    | Class::LRI
                    | Class::RLI
                    | Class::FSI
                    | Class::PDI
            )
            || matches!(ch, '\u{200E}' | '\u{200F}')
    })
}

/// Rule L1 for one line: segment and paragraph separators, and the
/// whitespace and isolate controls before them or at the line's end,
/// take the paragraph's level. `levels` has one level per character of
/// `line`, as [`resolve`] gave them.
pub fn reset_line_end(line: &str, levels: &mut [Level], paragraph: Level) {
    let mut run: Option<usize> = None;
    for (i, ch) in line.chars().enumerate() {
        if i >= levels.len() {
            return;
        }
        match class(ch) {
            Class::S | Class::B => {
                let from = run.take().unwrap_or(i);
                fill(levels, from, i + 1, paragraph);
            }
            Class::WS
            | Class::LRI
            | Class::RLI
            | Class::FSI
            | Class::PDI
            | Class::BN
            | Class::LRE
            | Class::LRO
            | Class::RLE
            | Class::RLO
            | Class::PDF => {
                run.get_or_insert(i);
            }
            _ => run = None,
        }
    }
    if let Some(from) = run {
        fill(levels, from, levels.len(), paragraph);
    }
}

fn fill(levels: &mut [Level], from: usize, to: usize, level: Level) {
    if let Some(part) = levels.get_mut(from..to.min(levels.len())) {
        part.fill(level);
    }
}

/// Rule L2: the order in which a line's characters are shown from the
/// left, as indexes into its levels. Writes `min(levels, order)` indexes
/// and returns their count. Characters rule X9 leaves out (embeddings,
/// overrides, `PDF`, boundary neutrals) are ordered too, with their
/// neighbours; they draw nothing.
pub fn visual_order(levels: &[Level], order: &mut [u16]) -> usize {
    let n = levels.len().min(order.len()).min(usize::from(u16::MAX));
    let Some(order) = order.get_mut(..n) else {
        return 0;
    };
    for (i, slot) in order.iter_mut().enumerate() {
        *slot = u16::try_from(i).unwrap_or(u16::MAX);
    }
    let levels = &levels[..n];
    let highest = levels.iter().map(|l| l.0).max().unwrap_or(0);
    let lowest_odd = levels
        .iter()
        .map(|l| l.0)
        .filter(|l| l & 1 == 1)
        .min()
        .unwrap_or(highest + 1);
    let mut level = highest;
    while level >= lowest_odd && level > 0 {
        let mut i = 0;
        while i < n {
            if levels[usize::from(order[i])].0 >= level {
                let start = i;
                while i < n && levels[usize::from(order[i])].0 >= level {
                    i += 1;
                }
                order[start..i].reverse();
            } else {
                i += 1;
            }
        }
        level -= 1;
    }
    n
}

/// The bracket `ch` pairs with and whether it opens
/// (`Bidi_Paired_Bracket`), with U+2329 and U+232A read as U+3008 and
/// U+3009, as canonical equivalence asks.
fn bracket(ch: char) -> Option<(u16, bool)> {
    let cp = u16::try_from(u32::from(ch)).ok()?;
    let i = tables::BRACKETS.binary_search_by_key(&cp, |b| b.0).ok()?;
    let &(_, pair, open) = tables::BRACKETS.get(i)?;
    Some((canonical(pair), open == 1))
}

/// The bracket of the pair a closing bracket is compared by.
fn canonical(cp: u16) -> u16 {
    match cp {
        0x2329 => 0x3008,
        0x232A => 0x3009,
        other => other,
    }
}
