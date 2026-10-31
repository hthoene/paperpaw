//! Selecting text with the real kernel, by touch (a long press, taps on
//! more words, the bar and its *More*) and with the buttons of a device
//! without touch (*Select text*, moving word by word and line by line,
//! *Extend*, the bar's focus): what is looked up (without punctuation,
//! with its sentence), highlighted and noted (the annotations file),
//! copied, and that *Back* or a tap beside the words ends it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_platform::PointerPhase;
use paperpaw_plugin_reader::actions as reader;
use paperpaw_sdk::annotations::{self, Kind};
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::{Point, Storage};

const BOOK: &str = "books/numbered.txt";

/// The words of the book in order: `p0 word0x1 … word0x11. p1 …`.
fn word(index: usize) -> String {
    let (paragraph, i) = (index / 12, index % 12);
    match i {
        0 => format!("p{paragraph}"),
        11 => format!("word{paragraph}x11."),
        i => format!("word{paragraph}x{i}"),
    }
}

/// Words `from..=to` as selected text has them.
fn text_of(from: usize, to: usize) -> String {
    (from..=to).map(word).collect::<Vec<_>>().join(" ")
}

fn numbered(device: paperpaw_sdk::DeviceInfo) -> Reading {
    let text = numbered_book(60, 12);
    let mut reading = Reading::new(device, Shared::default(), BOOK, text.as_bytes(), "");
    reading.open();
    reading
}

/// The word looked up last, as the dictionary was asked for it.
fn looked_up(reading: &Reading) -> String {
    let asked = reading.last_slot("lookup").expect("nothing was looked up");
    asked.get(params::WORD).unwrap().to_string()
}

fn highlights(reading: &Reading) -> Vec<annotations::Annotation> {
    annotations::load(&reading.storage, BOOK)
        .into_iter()
        .filter(|a| a.mark.kind == Kind::Highlight)
        .collect()
}

/// Looks up what is selected with the bar, and closes the dictionary:
/// the text stays selected (to highlight it after), *Back* ends that.
fn look_up(reading: &mut Reading) -> String {
    reading.tap_on("Look up");
    assert_eq!(reading.screen(), "recorder.lookup");
    reading.press(actions::BACK);
    assert!(reading.part("Look up").is_some(), "still selected");
    reading.press(actions::BACK);
    assert!(reading.part("Look up").is_none());
    looked_up(reading)
}

/// The words below the first line of the page: the first line is in the
/// strip at the top where a finger swipes in from the edge (see
/// `a_long_press_on_the_first_line_selects_its_word`).
fn below_the_top(reading: &mut Reading) -> Vec<(usize, Point)> {
    let words = reading.words();
    let first = words[0].y;
    words
        .into_iter()
        .enumerate()
        .filter(|(_, w)| w.y > first)
        .collect()
}

#[test]
fn a_long_press_selects_the_word_held() {
    // The pixels the tests find words by match the book's words: a long
    // press on each looks that one up.
    let mut reading = numbered(TOUCH);
    let words = below_the_top(&mut reading);
    assert!(words.len() > 20, "{} words", words.len());
    for &(i, at) in words.iter().take(12) {
        reading.long_press(at);
        assert_eq!(
            look_up(&mut reading),
            word(i).trim_end_matches('.'),
            "word {i}"
        );
    }
    assert!(highlights(&reading).is_empty());
}

/// The first line of a page lies in the top strip (10 mm) where the
/// kernel follows a finger as a swipe in from the edge: held there, it is
/// never a long press, and lifted it is taken as a tap at the top (a
/// bookmark in the corner, or what `tap.top` does), so its words cannot
/// be selected by touch.
#[test]
fn a_long_press_on_the_first_line_selects_its_word() {
    let mut reading = numbered(TOUCH);
    let words = reading.words();
    for (i, &at) in words.iter().enumerate().take(3) {
        reading.long_press(at);
        assert_eq!(reading.screen(), "reader.screen", "word {i}");
        assert!(reading.part("Look up").is_some(), "word {i}");
        reading.press(actions::BACK);
    }
    assert!(annotations::load(&reading.storage, BOOK).is_empty());
}

#[test]
fn a_long_press_looks_up_the_word_without_punctuation_and_with_its_sentence() {
    let mut reading = numbered(TOUCH);
    let words = reading.words();
    // The last word of the first paragraph, with its full stop.
    reading.long_press(words[11]);
    reading.tap_on("Look up");
    assert_eq!(looked_up(&reading), "word0x11");
    let asked = reading.last_slot("lookup").unwrap();
    assert_eq!(asked.get(params::LANGUAGE), Some("en"));
    // Plugins hear the word with its sentence, book and place.
    let heard = reading.last_event("looked-up").unwrap();
    let heard = <paperpaw_sdk::builtin::events::WordLookedUp as paperpaw_sdk::event::EventData>::from_params(&heard).unwrap();
    assert_eq!(heard.word, "word0x11");
    assert_eq!(heard.path, BOOK);
    assert_eq!(heard.text, text_of(0, 11));
    assert!(heard.offset > 0);
}

#[test]
fn taps_extend_a_selection_and_highlight_writes_it() {
    let mut reading = numbered(TOUCH);
    let words = reading.words();
    reading.long_press(words[4]);
    // A tap on a word further on: the selection reaches it.
    reading.tap(words[8]);
    // *Highlight* is in the selection menu (the bar has no room for it
    // here), which runs it for the selected text.
    reading.press(reader::HIGHLIGHT);
    let found = highlights(&reading);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].text, text_of(4, 8));
    assert!(found[0].mark.start < found[0].mark.end);
    assert!(reading.storage.exists(&annotations::file(BOOK)));
    assert!(reading.events().contains(&"annotations"));
    // The bar is gone.
    assert!(reading.part("Look up").is_none());
    // A tap on a word before the one held: from there up to it.
    reading.long_press(words[12]);
    reading.tap(words[10]);
    reading.press(reader::HIGHLIGHT);
    let texts: Vec<String> = highlights(&reading).into_iter().map(|a| a.text).collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts.contains(&text_of(10, 12)), "{texts:?}");
}

#[test]
fn more_offers_the_whole_selection_menu() {
    let mut reading = numbered(TOUCH);
    let words = reading.words();
    reading.long_press(words[5]);
    // The bar keeps *Cancel* last and *More* before it.
    let shown = reading.described();
    assert_eq!(shown.lines().last(), Some("Cancel"), "{shown}");
    reading.tap_on("More");
    let asked = reading.last_slot("menu").unwrap();
    assert_eq!(
        asked.get(params::MENU),
        Some(paperpaw_plugin_reader::SELECTION_MENU.as_str())
    );
    // On touch, *Extend* is left out (taps extend), and *Translate*
    // without a translator.
    let hidden = asked.get(params::HIDE).unwrap();
    assert!(hidden.contains(reader::EXTEND.as_str()), "{hidden}");
    assert!(hidden.contains(reader::TRANSLATE.as_str()), "{hidden}");
    // The menu closes and runs the action chosen: the text is still
    // selected for it.
    reading.press(actions::BACK);
    reading.press(reader::COPY);
    assert_eq!(reading.copied(), [word(5)]);
}

#[test]
fn highlighted_text_offers_to_remove_the_highlight() {
    let mut reading = numbered(BUTTONS);
    reading.press(reader::SELECT);
    reading.press(actions::RIGHT);
    reading.press(actions::CONFIRM);
    assert!(reading.part("Highlight").is_some());
    reading.press(reader::HIGHLIGHT);
    assert_eq!(highlights(&reading).len(), 1);
    // Again on it: the bar says so, and removes it.
    reading.press(reader::SELECT);
    reading.press(actions::RIGHT);
    reading.press(actions::CONFIRM);
    assert!(
        reading.part("Remove highlight").is_some(),
        "{}",
        reading.described()
    );
    reading.press(reader::HIGHLIGHT);
    assert!(highlights(&reading).is_empty());
}

#[test]
fn a_tap_beside_the_words_ends_the_selection() {
    let mut reading = numbered(TOUCH);
    let words = reading.words();
    reading.long_press(words[5]);
    assert!(reading.part("Look up").is_some());
    // Right of the last word of a paragraph, alone on its line.
    let beside = Point::new(330, words[11].y);
    assert!(words.iter().filter(|w| w.y == beside.y).count() == 1);
    reading.tap(beside);
    assert!(reading.part("Look up").is_none());
    // A long press beside the words selects nothing.
    reading.long_press(beside);
    assert!(reading.part("Look up").is_none());
    assert!(highlights(&reading).is_empty());
}

#[test]
fn a_long_press_then_a_drag_keeps_the_word_pressed() {
    // Dragging is not how a selection is extended (taps are): the word
    // held stays selected, whatever the finger does after.
    let mut reading = numbered(TOUCH);
    let words = reading.words();
    reading.now += 1000;
    reading.finger(PointerPhase::Down, words[4]);
    reading.now += 800;
    reading.kernel.tick(paperpaw_sdk::Instant(reading.now));
    reading.finger(PointerPhase::Move, words[6]);
    reading.finger(PointerPhase::Move, words[9]);
    reading.finger(PointerPhase::Up, words[9]);
    assert_eq!(look_up(&mut reading), word(4));
}

#[test]
fn copy_puts_the_words_on_the_clipboard() {
    let mut reading = numbered(TOUCH);
    let words = reading.words();
    reading.long_press(words[4]);
    reading.tap(words[5]);
    reading.copied();
    reading.press(reader::COPY);
    assert_eq!(reading.copied(), [text_of(4, 5)]);
    assert!(reading.part("Look up").is_none());
}

#[test]
fn buttons_choose_a_word_and_the_bar_looks_it_up() {
    let mut reading = numbered(BUTTONS);
    reading.press(reader::SELECT);
    reading.press(actions::RIGHT);
    reading.press(actions::RIGHT);
    // Confirm shows the bar, its first item focused: *Look up*.
    reading.press(actions::CONFIRM);
    let parts = reading.kernel.describe();
    let focused = paperpaw_sdk::semantics::focused(&parts).map(|p| p.label.clone());
    assert_eq!(focused.as_deref(), Some("Look up"));
    // Without touch, *Cancel* is not offered (back is).
    let shown = reading.described();
    assert!(!shown.contains("Cancel"), "{shown}");
    // The focus moves along the bar and wraps around.
    let labels: Vec<String> = shown.lines().map(String::from).collect();
    for step in 1..=labels.len() {
        reading.press(actions::RIGHT);
        let parts = reading.kernel.describe();
        let focused = paperpaw_sdk::semantics::focused(&parts).map(|p| p.label.clone());
        assert_eq!(focused, Some(labels[step % labels.len()].clone()));
    }
    reading.press(actions::CONFIRM);
    assert_eq!(looked_up(&reading), word(2));
}

/// Looks up the word chosen with the buttons: it is `expected`. The
/// dictionary closes, then the selection ends.
fn check_chosen(reading: &mut Reading, expected: usize) {
    reading.press(actions::CONFIRM);
    reading.press(reader::LOOK_UP);
    assert_eq!(looked_up(reading), word(expected).trim_end_matches('.'));
    reading.press(actions::BACK);
    reading.press(actions::BACK);
}

#[test]
fn buttons_move_by_words_and_wrap_round_the_page() {
    let mut reading = numbered(BUTTONS);
    let words = reading.words();
    // Left from the first word reaches the last word of the page.
    reading.press(reader::SELECT);
    reading.press(actions::LEFT);
    check_chosen(&mut reading, words.len() - 1);
    // Right from the last word reaches the first.
    reading.press(reader::SELECT);
    for _ in 0..words.len() {
        reading.press(actions::RIGHT);
    }
    check_chosen(&mut reading, 0);
    assert_eq!(reading.turned(), 0);
}

/// While a word is chosen, Up and Down move to the line above or below
/// (the arrows turn pages only while nothing is chosen).
#[test]
fn buttons_move_by_lines_and_stop_at_the_ends() {
    let mut reading = numbered(BUTTONS);
    let words = reading.words();
    // Down: the word below, nearest to it.
    let below = (0..words.len())
        .filter(|&i| words[i].y > words[0].y)
        .min_by_key(|&i| ((words[i].y - words[0].y) * 10_000) + (words[i].x - words[0].x).abs())
        .unwrap();
    reading.press(reader::SELECT);
    reading.press(actions::DOWN);
    check_chosen(&mut reading, below);
    // Up from the first line stays on it.
    reading.press(reader::SELECT);
    reading.press(actions::RIGHT);
    reading.press(actions::UP);
    check_chosen(&mut reading, 1);
    // Down from the last line stays on it.
    reading.press(reader::SELECT);
    reading.press(actions::LEFT);
    reading.press(actions::DOWN);
    check_chosen(&mut reading, words.len() - 1);
    assert_eq!(reading.turned(), 0);
}

/// A device with two page buttons and no up and down: they go by line
/// while a word is chosen, as up and down do.
#[test]
fn the_page_buttons_move_by_lines() {
    let mut reading = numbered(BUTTONS);
    let words = reading.words();
    let below = (0..words.len())
        .filter(|&i| words[i].y > words[0].y)
        .min_by_key(|&i| ((words[i].y - words[0].y) * 10_000) + (words[i].x - words[0].x).abs())
        .unwrap();
    reading.press(reader::SELECT);
    reading.press(actions::NEXT_PAGE);
    check_chosen(&mut reading, below);
    reading.press(reader::SELECT);
    reading.press(actions::RIGHT);
    reading.press(actions::PREV_PAGE);
    check_chosen(&mut reading, 1);
    assert_eq!(reading.turned(), 0);
}

#[test]
fn buttons_extend_a_selection_and_highlight_it() {
    let mut reading = numbered(BUTTONS);
    reading.press(reader::SELECT);
    reading.press(actions::RIGHT);
    reading.press(actions::CONFIRM);
    reading.press(reader::EXTEND);
    for _ in 0..3 {
        reading.press(actions::RIGHT);
    }
    reading.press(actions::CONFIRM);
    reading.press(reader::HIGHLIGHT);
    let found = highlights(&reading);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, text_of(1, 4));
    // Extending back past the start: from there up to the start.
    reading.press(reader::SELECT);
    for _ in 0..8 {
        reading.press(actions::RIGHT);
    }
    reading.press(actions::CONFIRM);
    reading.press(reader::EXTEND);
    reading.press(actions::LEFT);
    reading.press(actions::LEFT);
    reading.press(actions::CONFIRM);
    reading.press(reader::HIGHLIGHT);
    let texts: Vec<String> = highlights(&reading).into_iter().map(|a| a.text).collect();
    assert!(texts.contains(&text_of(6, 8)), "{texts:?}");
}

#[test]
fn back_ends_choosing_and_the_page_turns_again() {
    let mut reading = numbered(BUTTONS);
    reading.press(reader::SELECT);
    reading.press(actions::NEXT_PAGE);
    // While choosing, the page button moves the cursor.
    assert_eq!(reading.turned(), 0);
    reading.press(actions::BACK);
    reading.press(actions::NEXT_PAGE);
    assert_eq!(reading.turned(), 1);
    assert!(highlights(&reading).is_empty());
}

#[test]
fn a_note_is_typed_kept_and_offered_again() {
    let mut reading = numbered(BUTTONS);
    reading.press(reader::SELECT);
    reading.press(actions::RIGHT);
    reading.press(actions::CONFIRM);
    reading.press(reader::NOTE);
    // The keyboard asks, with no note yet; the word is highlighted.
    let asked = reading.last_slot("keyboard").unwrap();
    assert_eq!(asked.get(params::TEXT).unwrap_or_default(), "");
    assert_eq!(highlights(&reading).len(), 1);
    reading.type_text("A note\twith a tab\nand a line");
    assert_eq!(reading.screen(), "reader.screen");
    let found = highlights(&reading);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, word(1));
    assert!(found[0].mark.noted);
    assert!(found[0].note.starts_with("A note"), "{:?}", found[0].note);
    // On the noted word again: the note to start from.
    reading.press(reader::SELECT);
    reading.press(actions::RIGHT);
    reading.press(actions::CONFIRM);
    reading.press(reader::NOTE);
    let asked = reading.last_slot("keyboard").unwrap();
    assert_eq!(asked.get(params::TEXT), Some(found[0].note.as_str()));
    // Still one highlight, not a second.
    reading.type_text("Changed");
    let found = highlights(&reading);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].note, "Changed");
}

#[test]
fn select_on_an_empty_page_chooses_nothing() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), BOOK, b"", "");
    reading.open();
    reading.press(reader::SELECT);
    reading.press(actions::CONFIRM);
    reading.press(reader::HIGHLIGHT);
    assert!(highlights(&reading).is_empty());
    assert!(reading.last_slot("lookup").is_none());
}

#[test]
fn a_tap_in_the_top_right_corner_sets_and_removes_a_bookmark() {
    let mut reading = numbered(TOUCH);
    let bookmarks = |reading: &Reading| {
        annotations::marks(&reading.storage, BOOK)
            .into_iter()
            .filter(|m| m.kind == Kind::Bookmark)
            .count()
    };
    reading.tap(Point::new(470, 8));
    assert_eq!(bookmarks(&reading), 1);
    reading.tap(Point::new(470, 8));
    assert_eq!(bookmarks(&reading), 0);
}

/// The bar's buttons keep the side margin, their labels whole; a tap
/// beside the first or last of them, out to the screen's edge, is theirs.
#[test]
fn the_bars_ends_belong_to_its_first_and_last_items() {
    let mut reading = numbered(TOUCH);
    let words = reading.words();
    reading.long_press(words[5]);
    let shown = reading.described();
    // Only *More …* has dots: no label is shortened.
    assert_eq!(shown.matches('…').count(), 1, "a label shortened:\n{shown}");
    let first = reading.part("Look up").unwrap();
    let y = i32::from(first.area.y) + i32::from(first.area.height) / 2;
    reading.tap(Point::new(1, y));
    assert_eq!(
        reading.screen(),
        "recorder.lookup",
        "{}",
        reading.described()
    );
    reading.press(actions::BACK);
    assert!(reading.part("Cancel").is_some(), "still selected");
    let width = i32::from(TOUCH.display.width);
    reading.tap(Point::new(width - 2, y));
    assert!(reading.part("Look up").is_none(), "{}", reading.described());
}
