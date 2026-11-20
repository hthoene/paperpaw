//! The selection by place: it holds over pages, its keys go to the page's
//! edge and turn it, *Widen* goes word, sentence, paragraph, and it takes in
//! at most five pages (#235).
use crate::native_word::Word;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Rect, document::Locator, layout::LineBidi};

use super::live::*;
use super::moves::Mode;

/// A page of `lines` of words; `first` is the offset of its first word and a
/// line that starts a paragraph is marked as the layout does.
fn page(first: u32, lines: &[(&str, bool)]) -> Vec<Word> {
    let mut words = Vec::new();
    let mut offset = first;
    for (y, (line, starts)) in lines.iter().enumerate() {
        let mut x = 10u16;
        for text in line.split(' ') {
            let end = offset + text.len() as u32;
            let flags = if *starts {
                LineBidi::PARAGRAPH_START
            } else {
                0
            };
            words.push(
                Word::new(
                    Rect::new(x, 20 + y as u16 * 30, 40, 24),
                    text,
                    Locator::new(0, offset),
                    Locator::new(0, end),
                )
                .bidi(LineBidi::new(0, flags)),
            );
            x += 48;
            offset = end + 1;
        }
    }
    words
}

fn text(words: &[Word], (lo, hi): (usize, usize)) -> String {
    words[lo..=hi]
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn on(words: &[Word], index: usize) -> Live {
    Live::choosing(At::of(&words[index]))
}

const ONE: &[(&str, bool)] = &[
    ("Alice was tired.", true),
    ("She sat by the bank and", false),
    ("looked. Then a Rabbit ran by.", false),
    ("The end of it", true),
];

#[test]
fn a_selection_is_places_and_holds_over_pages() {
    let one = page(0, ONE);
    let two = page(200, ONE);
    let mut live = on(&one, 2);
    live.mode = Mode::Extending;
    live.to = At::of(&two[1]);
    // It is not on one page; each page has the words that are in it.
    assert_eq!(live.on_page(&one), None);
    assert_eq!(live.on_page(&two), None);
    assert!(one[2..].iter().all(|w| live.covers(w)));
    assert!(two[..=1].iter().all(|w| live.covers(w)));
    assert!(!live.covers(&two[2]));
    assert!(!one[..2].iter().any(|w| live.covers(w)));
    assert_eq!(
        live.span(),
        (
            (one[2].start.section, one[2].start.offset),
            (two[1].end.section, two[1].end.offset)
        )
    );
}

#[test]
fn the_keys_go_to_the_edge_and_the_page_turns() {
    let one = page(0, ONE);
    let two = page(200, ONE);
    let last = one.len() - 1;
    let mut live = on(&one, last - 1);
    // Right on the last word: past the page.
    assert_eq!(
        live.step(&one, Move::Nav("core.right"), Side::Both),
        Went::To(last)
    );
    live.put(&one, Side::Both, last);
    assert_eq!(
        live.step(&one, Move::Nav("core.right"), Side::Both),
        Went::Edge(true)
    );
    assert_eq!(
        live.step(&one, Move::Nav("core.down"), Side::Both),
        Went::Edge(true)
    );
    // On the new page it goes on at the first word after the one it was on.
    assert!(live.land(&two, Side::Both, true));
    assert_eq!((live.from, live.to), (At::of(&two[0]), At::of(&two[0])));
    // Left on the first word: back; it lands on the last word before.
    assert_eq!(
        live.step(&two, Move::Nav("core.left"), Side::Both),
        Went::Edge(false)
    );
    assert!(live.land(&one, Side::Both, false));
    assert_eq!(live.to, At::of(&one[last]));
}

#[test]
fn an_end_keeps_the_start_and_does_not_pass_it() {
    let one = page(0, ONE);
    let mut live = on(&one, 3);
    live.mode = Mode::Extending;
    live.put(&one, Side::End, 6);
    assert_eq!(live.from, At::of(&one[3]));
    assert_eq!(live.to, At::of(&one[6]));
    // The end goes back past the start: it stays on the start's word.
    live.put(&one, Side::End, 1);
    assert_eq!((live.from, live.to), (At::of(&one[3]), At::of(&one[3])));
    // The start goes on past the end: it stays on the end's word.
    live.put(&one, Side::Start, 8);
    assert_eq!((live.from, live.to), (At::of(&one[3]), At::of(&one[3])));
    // The start can go earlier; a touch before the start moves the start.
    live.put(&one, Side::Start, 1);
    assert_eq!(live.from, At::of(&one[1]));
    live.touch_to(&one, 0);
    assert_eq!(live.from, At::of(&one[0]));
}

#[test]
fn holding_left_and_right_go_to_the_ends_of_the_page() {
    let one = page(0, ONE);
    let live = on(&one, 4);
    let first = live.step(&one, Move::Page(false), Side::Both);
    let last = live.step(&one, Move::Page(true), Side::Both);
    assert_eq!((first, last), (Went::To(0), Went::To(one.len() - 1)));
}

#[test]
fn widen_goes_word_sentence_paragraph_and_back() {
    let one = page(0, ONE);
    // "Rabbit" is in the middle of "Then a Rabbit ran by." (the sentence
    // before ends at "looked."; the paragraph is the first three lines).
    let at = one.iter().position(|w| w.text == "Rabbit").unwrap();
    let mut live = on(&one, at);
    assert!(live.widen(&one));
    assert_eq!(live.mode, Mode::Extending);
    let range = live.on_page(&one).unwrap();
    assert_eq!(text(&one, range), "Then a Rabbit ran by.");
    assert!(live.widen(&one));
    let range = live.on_page(&one).unwrap();
    assert_eq!(
        text(&one, range),
        "Alice was tired. She sat by the bank and looked. Then a Rabbit ran by."
    );
    assert!(live.widen(&one));
    assert_eq!(live.on_page(&one).unwrap(), (at, at));
    // It goes out from the same word again.
    assert!(live.widen(&one));
    assert_eq!(
        text(&one, live.on_page(&one).unwrap()),
        "Then a Rabbit ran by."
    );
    // Not from a word that is on another page.
    assert!(!live.widen(&page(500, ONE)));
}

#[test]
fn a_sentence_is_cut_at_the_start_of_the_page() {
    let one = page(0, &[("and then", false), ("it ended. Next.", false)]);
    let mut live = on(&one, 1);
    assert!(live.widen(&one));
    assert_eq!(
        text(&one, live.on_page(&one).unwrap()),
        "and then it ended."
    );
}

#[test]
fn the_paragraph_keys_go_to_the_first_word_of_a_paragraph() {
    let one = page(0, ONE);
    let para = one.iter().position(|w| w.text == "The").unwrap();
    // Down from the first paragraph: the next one's first word.
    assert_eq!(
        on(&one, 0).step(&one, Move::Paragraph(true), Side::Both),
        Went::To(para)
    );
    // Down from the last paragraph: past the page.
    assert_eq!(
        on(&one, para).step(&one, Move::Paragraph(true), Side::Both),
        Went::Edge(true)
    );
    // Up from the middle of one: its own start; from its start: the one before.
    let middle = para + 2;
    assert_eq!(
        on(&one, middle).step(&one, Move::Paragraph(false), Side::Both),
        Went::To(para)
    );
    assert_eq!(
        on(&one, para).step(&one, Move::Paragraph(false), Side::Both),
        Went::To(0)
    );
    assert_eq!(
        on(&one, 0).step(&one, Move::Paragraph(false), Side::Both),
        Went::Edge(false)
    );
}

#[test]
fn a_selection_takes_in_at_most_five_pages() {
    let mut pages = Pages::default();
    for _ in 0..4 {
        assert!(pages.allows(true));
        pages.turned(true);
    }
    // The fifth page is the last: no sixth.
    assert!(!pages.allows(true));
    // Back is fine, and forward again to the same extent.
    assert!(pages.allows(false));
    pages.turned(false);
    assert!(pages.allows(true));
    // Going back past the start counts as a page too.
    let mut back = Pages::default();
    for _ in 0..4 {
        back.turned(false);
    }
    assert!(!back.allows(false));
}

#[test]
fn the_start_of_the_text_is_kept_in_512_bytes() {
    let long: Vec<Word> = (0..200)
        .map(|i| {
            Word::new(
                Rect::new(0, 0, 1, 1),
                "wordwordword",
                Locator::new(0, i * 20),
                Locator::new(0, i * 20 + 12),
            )
        })
        .collect();
    let lead = lead_of(&long, 0).unwrap();
    assert!(lead.len() <= LEAD_BYTES && lead.len() > LEAD_BYTES - 14);
    assert!(lead.starts_with("wordwordword wordwordword"));
    // A word the layout broke is said once.
    let mut broken = page(0, &[("a bro-", false), ("ken word", false)]);
    broken[2] = Word::new(broken[2].area, "ken", broken[1].start, broken[1].end);
    assert_eq!(lead_of(&broken, 0).unwrap(), "a bro- word");
}
