//! Moving a selection over the words of a page with the buttons, and the
//! boxes drawn around selected words, for any page: the cursor stays on
//! the page, up and down reach another line where there is one, and the
//! boxes cover exactly the words picked, one per run on a line.

extern crate std;

use alloc::vec::Vec;

use paperpaw_sdk::Rect;
use paperpaw_sdk::document::Locator;
use proptest::prelude::*;

use super::*;
use paperpaw_sdk::reading::{LineBidi, WordBox};
/// The navigation actions, as the host names them.
mod actions {
    pub const LEFT: &str = "core.left";
    pub const RIGHT: &str = "core.right";
    pub const UP: &str = "core.up";
    pub const DOWN: &str = "core.down";
    pub const NEXT: &str = "core.next";
    pub const PREVIOUS: &str = "core.previous";
    pub const NEXT_PAGE: &str = "core.next-page";
    pub const PREV_PAGE: &str = "core.prev-page";
    pub const CONFIRM: &str = "core.confirm";
}

/// A page of `lines`, each with that many words of `width` pixels.
fn page(lines: &[(u8, u8)]) -> Vec<WordBox> {
    let mut words = Vec::new();
    let mut offset = 0;
    for (line, &(count, width)) in lines.iter().enumerate() {
        let mut x = 10u16;
        for _ in 0..count {
            let width = u16::from(width.max(1));
            words.push(WordBox::new(
                Rect::new(x, 20 + line as u16 * 30, width, 24),
                "word",
                Locator::new(0, offset),
                Locator::new(0, offset + 4),
            ));
            x += width + 8;
            offset += 5;
        }
    }
    words
}

fn moves() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just(actions::LEFT),
        Just(actions::RIGHT),
        Just(actions::UP),
        Just(actions::DOWN),
        Just(actions::NEXT_PAGE),
        Just(actions::PREV_PAGE),
        Just(actions::NEXT),
        Just(actions::PREVIOUS),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn the_cursor_stays_on_the_page(
        lines in proptest::collection::vec((0u8..6, 1u8..90), 0..8),
        moves in proptest::collection::vec(moves(), 0..40),
    ) {
        let words = page(&lines);
        let mut selection = Selection::choosing(0);
        for action in &moves {
            match selection.moved(action, &words) {
                Some(cursor) => {
                    prop_assert!(cursor < words.len());
                    selection.cursor = cursor;
                }
                // Only on an empty page.
                None => prop_assert!(words.is_empty()),
            }
        }
        let (lo, hi) = selection.range();
        prop_assert!(lo <= hi);
    }

    #[test]
    fn up_and_down_reach_the_next_line_where_there_is_one(
        lines in proptest::collection::vec((1u8..6, 1u8..90), 1..8),
        at in any::<proptest::sample::Index>(),
    ) {
        let words = page(&lines);
        let from = at.index(words.len());
        let y = words[from].area.y;
        let down = Selection::choosing(from).moved(actions::DOWN, &words).unwrap();
        let up = Selection::choosing(from).moved(actions::UP, &words).unwrap();
        let below = words.iter().map(|w| w.area.y).filter(|&l| l > y).min();
        let above = words.iter().map(|w| w.area.y).filter(|&l| l < y).max();
        prop_assert_eq!(below.unwrap_or(y), words[down].area.y);
        prop_assert_eq!(above.unwrap_or(y), words[up].area.y);
        // The page buttons go by line as up and down.
        prop_assert_eq!(Selection::choosing(from).moved(actions::NEXT_PAGE, &words), Some(down));
        prop_assert_eq!(Selection::choosing(from).moved(actions::PREV_PAGE, &words), Some(up));
    }

    #[test]
    fn boxes_cover_the_words_picked(
        lines in proptest::collection::vec((0u8..6, 1u8..90), 0..8),
        picked in proptest::collection::vec(any::<bool>(), 48),
    ) {
        let words = page(&lines);
        let boxes = line_boxes(&words, |i| picked[i % picked.len()]);
        for (i, word) in words.iter().enumerate() {
            let inside = boxes
                .iter()
                .any(|b| b.intersection(&word.area) == Some(word.area));
            prop_assert_eq!(inside, picked[i % picked.len()], "word {}", i);
        }
        // One box per run of words picked on a line.
        let runs = (0..words.len())
            .filter(|&i| {
                picked[i % picked.len()]
                    && (i == 0
                        || !picked[(i - 1) % picked.len()]
                        || words[i - 1].area.y != words[i].area.y)
            })
            .count();
        prop_assert_eq!(boxes.len(), runs);
    }
}

#[test]
fn nothing_moves_on_an_empty_page() {
    assert_eq!(Selection::choosing(0).moved(actions::RIGHT, &[]), None);
    assert!(line_boxes(&[], |_| true).is_empty());
}

#[test]
fn other_actions_do_not_move() {
    let words = page(&[(3, 20)]);
    assert_eq!(Selection::choosing(1).moved(actions::CONFIRM, &words), None);
}

#[test]
fn the_range_is_ordered_either_way() {
    let selection = Selection {
        anchor: 7,
        cursor: 2,
        mode: Mode::Extending,
        focus: None,
    };
    assert_eq!(selection.range(), (2, 7));
    assert_eq!(Selection::touched(4).mode, Mode::Bar);
}

#[test]
fn words_go_by_one_and_wrap_round_the_page() {
    let words = page(&[(3, 20), (2, 20)]);
    let moved = |at, action| Selection::choosing(at).moved(action, &words);
    assert_eq!(moved(1, actions::RIGHT), Some(2));
    assert_eq!(moved(1, actions::LEFT), Some(0));
    // Left from the first word reaches the last, right from the last the first.
    assert_eq!(moved(0, actions::LEFT), Some(4));
    assert_eq!(moved(4, actions::RIGHT), Some(0));
}

#[test]
fn lines_go_by_the_page_buttons_and_stop_at_the_ends() {
    // Three lines of four words: the page buttons are a line, not a word.
    let words = page(&[(4, 20), (4, 20), (4, 20)]);
    let moved = |at, action| Selection::choosing(at).moved(action, &words);
    assert_eq!(moved(1, actions::NEXT_PAGE), Some(5));
    assert_eq!(moved(5, actions::PREV_PAGE), Some(1));
    // Not past the last line, nor before the first.
    assert_eq!(moved(10, actions::NEXT_PAGE), Some(10));
    assert_eq!(moved(2, actions::PREV_PAGE), Some(2));
}

#[test]
fn a_cursor_past_the_words_moves_without_panicking() {
    let word = |x: u16, y: u16| {
        WordBox::new(
            Rect::new(x, y, 10, 10),
            "word",
            Locator::START,
            Locator::START,
        )
    };
    let words = [word(0, 0), word(20, 0), word(0, 20)];
    // Chosen on a page with more words than this one.
    let selection = Selection::choosing(7);
    for action in [actions::UP, actions::DOWN] {
        assert_eq!(selection.moved(action, &words), Some(7));
    }
    assert_eq!(Selection::choosing(1).moved(actions::DOWN, &words), Some(2));
}

/// A word `x` from the left on the first line, running right to left
/// where `rtl`.
fn at(x: u16, rtl: bool) -> WordBox {
    WordBox::new(Rect::new(x, 0, 10, 10), "w", Locator::START, Locator::START).rtl(rtl)
}

/// The bidi of a line of a paragraph running right to left where `rtl`.
fn para(rtl: bool) -> LineBidi {
    LineBidi::new(u8::from(rtl), 0)
}

/// A word on the line below the first, in a paragraph of `rtl`'s way.
fn below(rtl: bool) -> WordBox {
    WordBox::new(
        Rect::new(60, 20, 10, 10),
        "w",
        Locator::START,
        Locator::START,
    )
    .bidi(para(rtl))
}

#[test]
fn selected_words_of_a_right_to_left_line_are_one_box() {
    // In reading order from the right.
    let words = [at(60, true), at(40, true), at(20, true)];
    assert_eq!(line_boxes(&words, |_| true), [Rect::new(20, 0, 50, 10)]);
}

#[test]
fn a_selection_in_a_line_both_ways_leaves_out_words_drawn_between() {
    // `ab`, then Hebrew `x y` drawn as `y x`, then `cd`: in reading
    // order ab x y cd, drawn ab y x cd.
    let words = [at(0, false), at(40, true), at(20, true), at(60, false)];
    // ab and x: not y, drawn between them.
    assert_eq!(
        line_boxes(&words, |i| i <= 1),
        [Rect::new(0, 0, 10, 10), Rect::new(40, 0, 10, 10)]
    );
    // x and y: one box; ab to cd: all.
    assert_eq!(
        line_boxes(&words, |i| (1..=2).contains(&i)),
        [Rect::new(20, 0, 30, 10)]
    );
    assert_eq!(line_boxes(&words, |_| true), [Rect::new(0, 0, 70, 10)]);
}

#[test]
fn left_and_right_move_as_the_words_are_drawn() {
    let words = [at(0, false), at(40, true), at(20, true), at(60, false)];
    let on = |word: usize, action: &str| Selection::choosing(word).moved(action, &words);
    // To the word drawn beside: from `ab` right to `y`, from `x`
    // left to `y` and right to `cd`.
    assert_eq!(on(0, actions::RIGHT), Some(2));
    assert_eq!(on(1, actions::LEFT), Some(2));
    assert_eq!(on(1, actions::RIGHT), Some(3));
    assert_eq!(on(2, actions::LEFT), Some(0));
    // Next and previous keep to reading order.
    assert_eq!(on(1, actions::NEXT), Some(2));
    assert_eq!(on(2, actions::PREVIOUS), Some(1));
    // At the line's edge on in reading order: past `cd` (left to
    // right) to the next word; left of a right-to-left line's last
    // word, the next too.
    let rtl_line = [
        at(60, true).bidi(para(true)),
        at(40, true).bidi(para(true)),
        at(20, true).bidi(para(true)),
        below(true),
    ];
    let on = |word: usize, action: &str| Selection::choosing(word).moved(action, &rtl_line);
    assert_eq!(on(2, actions::LEFT), Some(3));
    // Right of the first word, in a right-to-left word, wraps to the last
    // one, as left of the first word does in a left-to-right one (#55).
    assert_eq!(on(0, actions::RIGHT), Some(3));
}

/// A line of words `(x, rtl)` in reading order, of a paragraph running
/// right to left where `paragraph_rtl`, then one word on the line
/// below.
fn page_of(paragraph_rtl: bool, line: &[(u16, bool)]) -> Vec<WordBox> {
    let mut words: Vec<WordBox> = line
        .iter()
        .map(|&(x, rtl)| at(x, rtl).bidi(para(paragraph_rtl)))
        .collect();
    words.push(below(paragraph_rtl));
    words
}

#[test]
fn at_the_edge_of_an_arabic_line_a_latin_word_or_number_goes_on_as_the_line_does() {
    // Reading order from the right: two Arabic words, then a number
    // at the left end (it runs left to right); then the next line.
    let words = page_of(true, &[(60, true), (40, true), (20, false)]);
    let on = |word: usize, action: &str| Selection::choosing(word).moved(action, &words);
    // Left of the number: the line's end, on to the next line.
    assert_eq!(on(2, actions::LEFT), Some(3));
    // Right of it: the word drawn there.
    assert_eq!(on(2, actions::RIGHT), Some(1));
    // A Latin word first, at the right end: right of it wraps back
    // (first word of the page: the last), left of it is the word beside.
    let words = page_of(true, &[(60, false), (40, true), (20, true)]);
    let on = |word: usize, action: &str| Selection::choosing(word).moved(action, &words);
    assert_eq!(on(0, actions::RIGHT), Some(3));
    assert_eq!(on(0, actions::LEFT), Some(1));
    // The last word of the page, left of the line's end: wraps to the first.
    let line = [(60, true), (40, true), (20, true)];
    let words: Vec<WordBox> = line
        .iter()
        .map(|&(x, rtl)| at(x, rtl).bidi(para(true)))
        .collect();
    assert_eq!(Selection::choosing(2).moved(actions::LEFT, &words), Some(0));
}

#[test]
fn a_line_of_only_an_english_quote_in_an_arabic_book_goes_on_as_the_book_does() {
    // Three Latin words drawn left to right (reading order from the
    // left), in an Arabic paragraph, then the next line. Every word
    // runs left to right, and no drawn position says which way the
    // paragraph goes: the line's bidi does.
    let words = page_of(true, &[(20, false), (40, false), (60, false)]);
    let on = |word: usize, action: &str| Selection::choosing(word).moved(action, &words);
    // Left at the line's left edge: on in reading order, to the next line.
    assert_eq!(on(0, actions::LEFT), Some(3));
    // Right at its right edge: back, past the page's start round to the last.
    assert_eq!(on(2, actions::RIGHT), Some(3));
    // The same quote in a left-to-right book: left wraps, right goes on.
    let words = page_of(false, &[(20, false), (40, false), (60, false)]);
    let on = |word: usize, action: &str| Selection::choosing(word).moved(action, &words);
    assert_eq!(on(0, actions::LEFT), Some(3));
    assert_eq!(on(2, actions::RIGHT), Some(3));
}

#[test]
fn a_line_that_runs_left_to_right_keeps_its_way_with_a_hebrew_word_at_its_edge() {
    // `ab cd`, then Hebrew `x y` (drawn `y x`) last, at the right end.
    let words = page_of(false, &[(0, false), (20, false), (60, true), (40, true)]);
    let on = |word: usize, action: &str| Selection::choosing(word).moved(action, &words);
    // Right of the rightmost word of the line: on to the next line.
    assert_eq!(on(2, actions::RIGHT), Some(4));
    // Left of the first word of the line (and of the page): the last.
    assert_eq!(on(0, actions::LEFT), Some(4));
}
