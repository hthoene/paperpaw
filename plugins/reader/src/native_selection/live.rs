//! The text being chosen, by place in the book (#235, owner Decision 221).
//!
//! A selection is two words, each as the place it has in the book (section and
//! offset), so it holds across page turns, a new layout and a font change: the
//! page's own word indexes are only found again from the places when they are
//! needed (the cache). Host-free: the movement over the words of one page is
//! [`Selection::moved`](super::Selection::moved); this adds what the page edge,
//! the paragraph and the *Widen* step need, and the pages a selection may span.
use core::cmp::Ordering;

use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{layout::LineBidi, reading::WordBox};

use super::moves::{Mode, Selection, continues};

/// A place in the book: section and offset.
pub type Pos = (u32, u32);

/// Most pages one selection may take in (the page it began on included).
pub const MAX_PAGES: i8 = 5;

/// What a word is by place: where it starts and ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct At {
    pub start: Pos,
    pub end: Pos,
}

const fn pos(at: Locator) -> Pos {
    (at.section, at.offset)
}

impl At {
    /// The place of `word`.
    #[must_use]
    pub const fn of(word: &WordBox) -> Self {
        Self {
            start: pos(word.start),
            end: pos(word.end),
        }
    }

    /// The first box of the page that starts where this word does: the cache of
    /// the place, none where the page does not have the word.
    #[must_use]
    pub fn index_in(&self, words: &[WordBox]) -> Option<usize> {
        words.iter().position(|w| pos(w.start) == self.start)
    }

    /// Where the word is to the page: before it, on it, after it.
    #[must_use]
    pub fn relation(&self, words: &[WordBox]) -> Ordering {
        match (words.first(), words.last()) {
            (Some(first), _) if self.start < pos(first.start) => Ordering::Less,
            (_, Some(last)) if self.start > pos(last.start) => Ordering::Greater,
            _ => Ordering::Equal,
        }
    }
}

/// How far *Widen* has taken the selection around its word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// The word.
    Word,
    /// The sentence it is in.
    Sentence,
    /// Its paragraph.
    Paragraph,
}

impl Unit {
    const fn next(self) -> Self {
        match self {
            Self::Word => Self::Sentence,
            Self::Sentence => Self::Paragraph,
            Self::Paragraph => Self::Word,
        }
    }
}

/// Which end of the selection a key moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Both: one word is chosen and moves.
    Both,
    /// The start.
    Start,
    /// The end.
    End,
}

/// The pages a selection has been on, from the one it began on (`here` is 0
/// there): at most [`MAX_PAGES`] from the lowest to the highest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pages {
    here: i8,
    low: i8,
    high: i8,
}

impl Pages {
    /// Whether a turn on is still within the cap.
    #[must_use]
    pub fn allows(&self, forward: bool) -> bool {
        let to = self.here + if forward { 1 } else { -1 };
        to.max(self.high) - to.min(self.low) < MAX_PAGES
    }

    /// The selection went a page on (or back).
    pub fn turned(&mut self, forward: bool) {
        self.here += if forward { 1 } else { -1 };
        self.low = self.low.min(self.here);
        self.high = self.high.max(self.here);
    }
}

/// What one key does to the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// A navigation action (word or line), as the host names it.
    Nav(&'static str),
    /// To the first word of the next (`true`) or the previous paragraph.
    Paragraph(bool),
    /// To the last (`true`) or the first word of the page.
    Page(bool),
}

/// Where a move went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Went {
    /// To this word of the page.
    To(usize),
    /// Past the page's edge, on (`true`) or back: the page turns.
    Edge(bool),
    /// Nowhere.
    Stays,
}

/// The selection: its first and last word by place, and what the buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Live {
    /// The first chosen word.
    pub from: At,
    /// The last chosen word.
    pub to: At,
    pub mode: Mode,
    /// The bar item the buttons have focused (none after a touch).
    pub focus: Option<usize>,
    /// How far *Widen* is.
    pub unit: Unit,
    /// The word *Widen* goes out from.
    pub base: At,
    pub pages: Pages,
}

impl Live {
    /// One word, to be moved with the buttons.
    #[must_use]
    pub const fn choosing(word: At) -> Self {
        Self {
            from: word,
            to: word,
            mode: Mode::Choosing,
            focus: None,
            unit: Unit::Word,
            base: word,
            pages: Pages {
                here: 0,
                low: 0,
                high: 0,
            },
        }
    }

    /// One word, chosen by touch: the bar is shown at once.
    #[must_use]
    pub const fn touched(word: At) -> Self {
        let mut live = Self::choosing(word);
        live.mode = Mode::Bar;
        live
    }

    /// The stretch of the book chosen, by place.
    #[must_use]
    pub const fn span(&self) -> (Pos, Pos) {
        (self.from.start, self.to.end)
    }

    /// Whether `word` is one of the chosen: it starts between the first chosen
    /// word's start and the last one's (both parts of a broken word do).
    #[must_use]
    pub fn covers(&self, word: &WordBox) -> bool {
        let at = pos(word.start);
        self.from.start <= at && at <= self.to.start
    }

    /// The first and last box of the page that are chosen, where both ends are
    /// on it (a broken word's parts all included).
    #[must_use]
    pub fn on_page(&self, words: &[WordBox]) -> Option<(usize, usize)> {
        let lo = self.from.index_in(words)?;
        let mut hi = self.to.index_in(words)?;
        while continues(words, hi + 1) {
            hi += 1;
        }
        Some((lo, hi))
    }

    /// The side the arrows move: the end once the word is chosen.
    #[must_use]
    pub fn arrows(&self) -> Side {
        if self.mode == Mode::Choosing {
            Side::Both
        } else {
            Side::End
        }
    }

    /// Where `mv` takes the `side` of the selection on this page.
    #[must_use]
    pub fn step(&self, words: &[WordBox], mv: Move, side: Side) -> Went {
        let at = if side == Side::Start {
            self.from
        } else {
            self.to
        };
        let Some(mut cur) = at.index_in(words) else {
            // Not on this page (it was turned by hand): the nearest word of it.
            return match at.relation(words) {
                Ordering::Less => first_word(words).map_or(Went::Stays, Went::To),
                _ => last_word(words).map_or(Went::Stays, Went::To),
            };
        };
        if side != Side::Start {
            while continues(words, cur + 1) {
                cur += 1;
            }
        }
        match mv {
            Move::Nav(action) => {
                let here = Selection::choosing(cur);
                match here.edge(action, words) {
                    Some(forward) => Went::Edge(forward),
                    None => here.moved(action, words).map_or(Went::Stays, Went::To),
                }
            }
            Move::Paragraph(down) => paragraph_move(words, cur, down),
            Move::Page(last) => (if last {
                last_word(words)
            } else {
                first_word(words)
            })
            .map_or(Went::Stays, Went::To),
        }
    }

    /// Puts the `side` on word `index` of the page: an end does not go past the
    /// other (a selection shrinks to one word, not to nothing).
    pub fn put(&mut self, words: &[WordBox], side: Side, index: usize) {
        let Some(word) = words.get(index) else {
            return;
        };
        let at = At::of(word);
        match side {
            Side::Both => {
                self.from = at;
                self.to = at;
            }
            Side::Start => {
                self.from = if at.start > self.to.start {
                    self.to
                } else {
                    at
                };
            }
            Side::End => {
                self.to = if at.start < self.from.start {
                    self.from
                } else {
                    at
                };
            }
        }
    }

    /// After a turn: puts the `side` on the first word of the new page after the
    /// word it was on (`forward`), or the last one before it. Whether it did.
    pub fn land(&mut self, words: &[WordBox], side: Side, forward: bool) -> bool {
        let edge = if side == Side::Start {
            self.from
        } else {
            self.to
        };
        let found = if forward {
            words
                .iter()
                .position(|w| pos(w.start) >= edge.end && !w.lookup_text().is_empty())
        } else {
            words
                .iter()
                .rposition(|w| pos(w.end) <= edge.start && !w.lookup_text().is_empty())
        };
        found.is_some_and(|index| {
            self.put(words, side, index);
            true
        })
    }

    /// The end moved to `word` of the page by a touch: a word before the first
    /// moves the start instead.
    pub fn touch_to(&mut self, words: &[WordBox], index: usize) {
        let Some(word) = words.get(index) else {
            return;
        };
        let side = if pos(word.start) < self.from.start {
            Side::Start
        } else {
            Side::End
        };
        self.put(words, side, index);
    }

    /// One step wider: the word, its sentence, its paragraph, the word again
    /// (cut at the page's edges). Whether the page has the word to go out from.
    pub fn widen(&mut self, words: &[WordBox]) -> bool {
        if self.unit == Unit::Word {
            self.base = self.from;
        }
        let Some(at) = self.base.index_in(words) else {
            return false;
        };
        let unit = self.unit.next();
        let (first, last) = match unit {
            Unit::Word => (at, at),
            Unit::Sentence => sentence_range(words, at),
            Unit::Paragraph => paragraph_range(words, at),
        };
        let (Some(a), Some(b)) = (words.get(first), words.get(last)) else {
            return false;
        };
        self.from = At::of(a);
        self.to = At::of(b);
        self.unit = unit;
        self.mode = Mode::Extending;
        true
    }
}

/// The first word that has letters, or any first one.
fn first_word(words: &[WordBox]) -> Option<usize> {
    words
        .iter()
        .position(|w| !w.lookup_text().is_empty())
        .or_else(|| (!words.is_empty()).then_some(0))
}

/// The last word that has letters, or any last one.
fn last_word(words: &[WordBox]) -> Option<usize> {
    words
        .iter()
        .rposition(|w| !w.lookup_text().is_empty())
        .or_else(|| words.len().checked_sub(1))
}

/// Whether word `i` is the first of a paragraph (on the first line of one).
fn starts_paragraph(words: &[WordBox], i: usize) -> bool {
    words.get(i).is_some_and(|w| {
        w.bidi.has(LineBidi::PARAGRAPH_START)
            && (i == 0 || words.get(i - 1).is_none_or(|b| b.area.y != w.area.y))
    })
}

/// The first word of the paragraph after (`down`) or before word `cur`; before:
/// the start of its own paragraph first, as an editor goes. The page's ends
/// are the paragraph's where the layout did not begin one there.
fn paragraph_move(words: &[WordBox], cur: usize, down: bool) -> Went {
    if down {
        return (cur + 1..words.len())
            .find(|&i| starts_paragraph(words, i) && !continues(words, i))
            .map_or(Went::Edge(true), Went::To);
    }
    if cur == 0 {
        return Went::Edge(false);
    }
    let own = (0..=cur)
        .rev()
        .find(|&i| starts_paragraph(words, i))
        .unwrap_or(0);
    if own < cur {
        return Went::To(own);
    }
    Went::To(
        (0..cur)
            .rev()
            .find(|&i| starts_paragraph(words, i))
            .unwrap_or(0),
    )
}

/// The words of the sentence word `at` is in, cut at the page's ends.
fn sentence_range(words: &[WordBox], at: usize) -> (usize, usize) {
    let ends = |i: usize| {
        words
            .get(i)
            .is_some_and(|w| crate::native_marking::act::ends_sentence(&w.text))
    };
    let mut first = at;
    while first > 0 && !ends(first - 1) {
        first -= 1;
    }
    let mut last = at;
    while last + 1 < words.len() && !ends(last) {
        last += 1;
    }
    (first, last)
}

/// The words of the paragraph word `at` is in, cut at the page's ends.
fn paragraph_range(words: &[WordBox], at: usize) -> (usize, usize) {
    let first = (0..=at)
        .rev()
        .find(|&i| starts_paragraph(words, i))
        .unwrap_or(0);
    let last = (at + 1..words.len())
        .find(|&i| starts_paragraph(words, i))
        .map_or(words.len().saturating_sub(1), |i| i - 1);
    (first, last.max(at))
}

/// Most bytes of the words from `first` on that are kept as the start of a
/// highlight's text where its end is on another page (`TEXT_MAX`).
pub const LEAD_BYTES: usize = 512;

/// The text of the words from `first` to the page's end, a space between
/// them, in at most [`LEAD_BYTES`]: the start of a highlight that goes over
/// the page (nothing more of the chosen text is kept).
///
/// # Errors
/// `NoMemory`.
pub fn lead_of(
    words: &[WordBox],
    first: usize,
) -> Result<alloc::string::String, paperpaw_sdk::native::Error> {
    let mut text = alloc::string::String::new();
    text.try_reserve(LEAD_BYTES)
        .map_err(|_| paperpaw_sdk::native::Error::NoMemory)?;
    for (at, word) in words.iter().enumerate().skip(first) {
        if at > first && continues(words, at) {
            continue;
        }
        if text.len() + word.text.len() + 1 > LEAD_BYTES {
            break;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&word.text);
    }
    Ok(text)
}
