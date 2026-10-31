//! Moving a selection over the words of a page with the buttons, and the
//! boxes drawn around selected words, for any page: the cursor stays on
//! the page, up and down reach another line where there is one, and the
//! boxes cover exactly the words picked, one per run on a line.

extern crate std;

use alloc::vec::Vec;

use paperpaw_sdk::Rect;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::document::Locator;
use proptest::prelude::*;

use super::*;

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

fn moves() -> impl Strategy<Value = Id> {
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
        let down = Selection::choosing(from).moved(&actions::DOWN, &words).unwrap();
        let up = Selection::choosing(from).moved(&actions::UP, &words).unwrap();
        let below = words.iter().map(|w| w.area.y).filter(|&l| l > y).min();
        let above = words.iter().map(|w| w.area.y).filter(|&l| l < y).max();
        prop_assert_eq!(below.unwrap_or(y), words[down].area.y);
        prop_assert_eq!(above.unwrap_or(y), words[up].area.y);
        // The page buttons go by line as up and down.
        prop_assert_eq!(Selection::choosing(from).moved(&actions::NEXT_PAGE, &words), Some(down));
        prop_assert_eq!(Selection::choosing(from).moved(&actions::PREV_PAGE, &words), Some(up));
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
    assert_eq!(Selection::choosing(0).moved(&actions::RIGHT, &[]), None);
    assert!(line_boxes(&[], |_| true).is_empty());
}

#[test]
fn other_actions_do_not_move() {
    let words = page(&[(3, 20)]);
    assert_eq!(
        Selection::choosing(1).moved(&actions::CONFIRM, &words),
        None
    );
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
    let moved = |at, action| Selection::choosing(at).moved(&action, &words);
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
    let moved = |at, action| Selection::choosing(at).moved(&action, &words);
    assert_eq!(moved(1, actions::NEXT_PAGE), Some(5));
    assert_eq!(moved(5, actions::PREV_PAGE), Some(1));
    // Not past the last line, nor before the first.
    assert_eq!(moved(10, actions::NEXT_PAGE), Some(10));
    assert_eq!(moved(2, actions::PREV_PAGE), Some(2));
}
