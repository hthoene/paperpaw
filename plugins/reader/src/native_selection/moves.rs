//! Choosing words on a page and moving over them with the buttons: the
//! cursor and the anchor as indexes into the page's words, the boxes drawn
//! round the chosen words, and where the cursor goes for each navigation
//! action. Host-free; the interface-6 reader's `selection.rs` is the reference
//! (this is its movement code and its tests, with action names as text).
use alloc::vec::Vec;

use paperpaw_sdk::Rect;
use paperpaw_sdk::reading::WordBox;

/// What the buttons do with the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Moving the cursor; the selection is one word.
    Choosing,
    /// Moving the end; the start stays.
    Extending,
    /// The bar is shown.
    Bar,
}

/// Selected words, as indexes into the page's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Selection {
    pub anchor: usize,
    pub cursor: usize,
    pub mode: Mode,
    /// The bar item focused by buttons (none after a touch).
    pub focus: Option<usize>,
}

impl Selection {
    /// One word, chosen by touch: the bar is shown at once.
    pub fn touched(word: usize) -> Self {
        Self {
            anchor: word,
            cursor: word,
            mode: Mode::Bar,
            focus: None,
        }
    }

    /// One word, to be moved with the buttons.
    pub fn choosing(word: usize) -> Self {
        Self {
            anchor: word,
            cursor: word,
            mode: Mode::Choosing,
            focus: None,
        }
    }

    /// The first and last selected word.
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }

    /// Moves the cursor for a navigation action; `None` if `action` is
    /// another one. Left and right go a word, up and down a line (the
    /// page buttons too, a device with two page buttons has no others).
    /// Words wrap round the page: left from the first word reaches the
    /// last, right from the last the first; lines stop at the ends.
    pub fn moved(&self, action: &str, words: &[WordBox]) -> Option<usize> {
        let last = words.len().checked_sub(1)?;
        let a = action;
        if a == "core.left" || a == "core.right" {
            // To the word drawn beside the cursor's on its line, also
            // where the line runs both ways.
            if let Some(beside) = beside(words, self.cursor, a == "core.right") {
                return Some(beside);
            }
            // At the line's edge: on to the line that follows in reading
            // order (right at the end of a line that runs left to right,
            // left at the end of one that runs right to left), or back to
            // the one before; past the page's ends round to the other.
            let Some((first, end)) = line_span(words, self.cursor) else {
                return Some(self.cursor.min(last));
            };
            // The line goes the way its paragraph does (the layout says so,
            // also for a line of a quote, a number or a Latin word that
            // runs the other way: it goes on past them as the paragraph).
            let rtl = words.get(self.cursor).is_some_and(|w| w.bidi.is_rtl());
            let forward = (a == "core.right") != rtl;
            return Some(if forward {
                if end >= last { 0 } else { end + 1 }
            } else {
                first.checked_sub(1).unwrap_or(last)
            });
        }
        Some(if a == "core.previous" {
            self.cursor.checked_sub(1).unwrap_or(last)
        } else if a == "core.next" {
            if self.cursor >= last {
                0
            } else {
                self.cursor + 1
            }
        } else if ["core.up", "core.prev-page"].contains(&a) {
            nearest_on_line(words, self.cursor, false)
        } else if ["core.down", "core.next-page"].contains(&a) {
            nearest_on_line(words, self.cursor, true)
        } else {
            return None;
        })
    }
}

/// Boxes around runs of words on one line, e.g. to draw a selection or
/// highlight as one shape per line.
pub fn line_boxes(words: &[WordBox], picked: impl Fn(usize) -> bool) -> Vec<Rect> {
    let mut boxes: Vec<Rect> = Vec::new();
    let mut previous: Option<usize> = None;
    for (index, word) in words.iter().enumerate() {
        if !picked(index) {
            previous = None;
            continue;
        }
        let after = index.checked_sub(1).filter(|p| previous == Some(*p));
        match boxes.last_mut() {
            Some(last) if after.is_some_and(|p| drawn_together(words, p, index, &picked)) => {
                *last = last.union(&word.area);
            }
            _ => {
                // Out of memory leaves the rest of the frames out: the words stay chosen.
                if boxes.try_reserve(1).is_err() {
                    break;
                }
                boxes.push(word.area);
            }
        }
        previous = Some(index);
    }
    boxes
}

/// Whether words `a` and `b` are drawn next to each other: on one line,
/// no word between them that is not `in_box`. Words that follow each
/// other in reading order may stand apart where a line runs both ways
/// (#33): a box around both would take in a word between them.
pub fn drawn_together(
    words: &[WordBox],
    a: usize,
    b: usize,
    in_box: impl Fn(usize) -> bool,
) -> bool {
    let (Some(wa), Some(wb)) = (words.get(a), words.get(b)) else {
        return false;
    };
    if wa.area.y != wb.area.y {
        return false;
    }
    let (left, right) = if wa.area.x <= wb.area.x {
        (wa.area, wb.area)
    } else {
        (wb.area, wa.area)
    };
    !words.iter().enumerate().any(|(i, w)| {
        i != a
            && i != b
            && !in_box(i)
            && w.area.y == left.y
            && u32::from(w.area.x) >= left.right()
            && w.area.right() <= u32::from(right.x)
    })
}

/// The first and the last word, in reading order, of the line of word
/// `index` (the words drawn at its height); none where `index` is past
/// the page's words.
fn line_span(words: &[WordBox], index: usize) -> Option<(usize, usize)> {
    let y = words.get(index)?.area.y;
    let mut line = words.iter().enumerate().filter(|(_, w)| w.area.y == y);
    let first = line.next()?.0;
    Some((first, line.next_back().map_or(first, |(i, _)| i)))
}

/// The word drawn next to word `index` on its line, on its right or its
/// left: none at the line's edge.
fn beside(words: &[WordBox], index: usize, right: bool) -> Option<usize> {
    let current = words.get(index)?.area;
    words
        .iter()
        .enumerate()
        .filter(|(i, w)| *i != index && w.area.y == current.y)
        .filter(|(_, w)| match right {
            true => w.area.x > current.x,
            false => w.area.x < current.x,
        })
        .min_by_key(|(_, w)| w.area.x.abs_diff(current.x))
        .map(|(i, _)| i)
}

/// The word on the line above (or below) word `index` nearest to it.
fn nearest_on_line(words: &[WordBox], index: usize, below: bool) -> usize {
    let Some(current) = words.get(index).map(|w| &w.area) else {
        return index;
    };
    let center = |r: &Rect| i32::from(r.x) + i32::from(r.width) / 2;
    let line = words
        .iter()
        .map(|w| w.area.y)
        .filter(|&y| if below { y > current.y } else { y < current.y })
        .min_by_key(|&y| y.abs_diff(current.y));
    let Some(line) = line else {
        return index;
    };
    words
        .iter()
        .enumerate()
        .filter(|(_, w)| w.area.y == line)
        .min_by_key(|(_, w)| center(&w.area).abs_diff(center(current)))
        .map_or(index, |(i, _)| i)
}
