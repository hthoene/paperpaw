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
fn a_broken_word_is_said_once_in_the_text_of_chosen_words() {
    let mut page = words("one two three");
    // A word of text has a place of a length.
    (page[1].start, page[1].end) = (Locator::new(0, 4), Locator::new(0, 7));
    // `two` broken by the layout: its second part has its text and place.
    page[2].text = page[1].text.clone();
    (page[2].start, page[2].end) = (page[1].start, page[1].end);
    assert_eq!(text_of(&page, 0, 2).unwrap(), "one two");
    assert_eq!(text_of(&page, 1, 2).unwrap(), "two");
    assert_eq!(text_of(&page, 2, 2).unwrap(), "two");
}

#[test]
fn the_bar_has_extend_on_buttons_and_cancel_on_touch() {
    let last = |touch| *bar::items(touch, None, true).as_slice().last().unwrap();
    assert_eq!(last(false), Item::Extend);
    assert_eq!(last(true), Item::Cancel);
}

#[test]
fn the_bar_names_highlight_and_note_by_what_is_there() {
    let names = |highlight| {
        let items = bar::items(true, highlight, true);
        (items.as_slice()[1], items.as_slice()[2])
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
    let of =
        |lo: usize, hi: usize| act::span_of(((0, page[lo].start.offset), (0, page[hi].end.offset)));
    // "one" is clear of it, "two" is in it (and it has a note), "six" is clear.
    assert_eq!(act::standing(&ctx, &of(0, 0)), None);
    assert_eq!(act::standing(&ctx, &of(1, 1)), Some(true));
    assert_eq!(act::standing(&ctx, &of(0, 1)), Some(true));
    assert_eq!(act::standing(&ctx, &of(2, 2)), None);
    // Without the book's highlights nothing overlaps.
    let bare = Ctx::new(Locator::START, &page, "", "", "", BOX);
    assert_eq!(act::standing(&bare, &of(1, 1)), None);
}

#[test]
fn the_bar_of_a_stretch_over_pages_has_no_look_up_and_copy() {
    let items = |one_page| bar::items(false, None, one_page);
    let all = items(true);
    assert_eq!(all.as_slice().len(), 5);
    assert_eq!(all.highlight_at(), 1);
    let over = items(false);
    assert_eq!(over.as_slice(), [Item::Highlight, Item::Note, Item::Extend]);
    assert_eq!(over.highlight_at(), 0);
}

#[test]
fn the_selecting_keys_are_the_actions_the_marking_knows() {
    let names = [
        super::WIDEN,
        super::HOLD_LEFT,
        super::HOLD_RIGHT,
        super::HOLD_UP,
        super::HOLD_DOWN,
    ];
    let declared: alloc::vec::Vec<&str> = crate::declarations::SELECTING_KEYS
        .iter()
        .map(|(_, action)| *action)
        .collect();
    assert_eq!(declared, names);
    assert_eq!(crate::declarations::SELECTING_LAYER, super::LAYER);
    for name in names {
        let short = name.strip_prefix("reader.").unwrap();
        assert!(
            crate::declarations::ACTIONS
                .iter()
                .any(|(n, _)| *n == short)
        );
    }
}

#[test]
fn a_lookup_is_anchored_at_the_box_of_its_words_in_their_first_line() {
    let at = |x, y| WordBox::new(Rect::new(x, y, 30, 20), "w", Locator::START, Locator::START);
    let page = [at(10, 100), at(50, 100), at(90, 100), at(10, 124)];
    let anchor = act::anchor(&page, 1, 3).unwrap();
    assert_eq!(anchor, Rect::new(50, 100, 70, 20));
    let mut out = [0; 24];
    assert_eq!(act::anchor_text(anchor, &mut out), "50,100,70,20");
    assert_eq!(
        act::anchor_text(Rect::new(0, 65535, 0, 7), &mut out),
        "0,65535,0,7"
    );
    assert!(act::anchor(&page, 9, 9).is_none());
}
