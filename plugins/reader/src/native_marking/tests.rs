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
    let last = |touch| *bar::items(touch, None).last().unwrap();
    assert_eq!(last(false), Item::Extend);
    assert_eq!(last(true), Item::Cancel);
}

#[test]
fn the_bar_names_highlight_and_note_by_what_is_there() {
    let names = |highlight| {
        let items = bar::items(true, highlight);
        (items[1], items[2])
    };
    assert_eq!(names(None), (Item::Highlight, Item::Note));
    assert_eq!(names(Some(false)), (Item::Unhighlight, Item::Note));
    assert_eq!(names(Some(true)), (Item::Unhighlight, Item::EditNote));
}

fn placed(text: &str, start: u32, end: u32) -> WordBox {
    WordBox::new(
        Rect::new(0, 0, 1, 1),
        text,
        Locator::new(0, start),
        Locator::new(0, end),
    )
}

const BOX: native_pages::Box = native_pages::Box {
    x: 0,
    y: 0,
    width: 100,
    height: 100,
};

fn mark(start: u32, end: u32, noted: bool) -> crate::native_marks::Span {
    crate::native_marks::Span {
        start: (0, start),
        end: (0, end),
        noted,
    }
}

#[test]
fn the_bar_is_named_by_the_highlight_the_chosen_words_overlap() {
    let page = [
        placed("one", 0, 3),
        placed("two", 4, 7),
        placed("six", 20, 23),
    ];
    let held = [mark(5, 10, true), mark(30, 40, false)];
    let ctx = Ctx::new(Locator::START, &page, "", "", "", BOX).book(Book {
        highlights: &held,
        library: None,
        conversion: 0,
        progress: None,
        stored_ppm: 0,
    });
    // "one" is clear of it, "two" is in it (and it has a note), "six" is clear.
    assert_eq!(act::standing(&ctx, (0, 0)), None);
    assert_eq!(act::standing(&ctx, (1, 1)), Some(true));
    assert_eq!(act::standing(&ctx, (0, 1)), Some(true));
    assert_eq!(act::standing(&ctx, (2, 2)), None);
    // Without the book's highlights, or past the page, nothing overlaps.
    let bare = Ctx::new(Locator::START, &page, "", "", "", BOX);
    assert_eq!(act::standing(&bare, (1, 1)), None);
    assert_eq!(act::standing(&ctx, (5, 6)), None);
}
