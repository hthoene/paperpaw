//! Host-free parts of the marking. The host paths (choose, bar, look up, copy,
//! follow, *Back*) need a book on a document host: they are the ignored cases
//! `reader-selection-host-flow` and `reader-link-follow-back` in
//! `tests/acceptance_pending.rs`.
use super::*;
use paperpaw_sdk::Rect;

fn words(text: &str) -> Vec<WordBox> {
    text.split(' ')
        .map(|word| WordBox::new(Rect::new(0, 0, 1, 1), word, Locator::START, Locator::START))
        .collect()
}

#[test]
fn the_sentence_around_a_word() {
    let page = words("It was late. \"Oh dear!\" said the Rabbit. Alice ran.");
    // "Rabbit." is word 7: its sentence begins after "dear!\"".
    assert_eq!(act::sentence(&page, 7, 7).unwrap(), "said the Rabbit.");
    assert_eq!(act::sentence(&page, 0, 0).unwrap(), "It was late.");
    assert_eq!(act::sentence(&page, 9, 9).unwrap(), "Alice ran.");
    // Several words: from the first one's sentence to the last one's.
    assert_eq!(
        act::sentence(&page, 2, 3).unwrap(),
        "It was late. \"Oh dear!\""
    );
}

#[test]
fn the_text_of_chosen_words_is_one_line() {
    let page = words("one two three");
    assert_eq!(text_of(&page, 0, 1).unwrap(), "one two");
    assert_eq!(text_of(&page, 2, 2).unwrap(), "three");
    // Past the page: nothing.
    assert_eq!(text_of(&page, 5, 6).unwrap(), "");
}

#[test]
fn the_bar_has_extend_on_buttons_and_cancel_on_touch() {
    assert_eq!(bar::items(false), [Item::LookUp, Item::Copy, Item::Extend]);
    assert_eq!(bar::items(true), [Item::LookUp, Item::Copy, Item::Cancel]);
}
