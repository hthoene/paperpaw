use super::*;
use paperpaw_sdk::Rect;

fn word(text: &str, start: u32, end: u32) -> WordBox {
    WordBox::new(
        Rect::new(0, 0, 1, 1),
        text,
        Locator::new(0, start),
        Locator::new(0, end),
    )
}

fn link(start: u32, end: u32, href: &str) -> PageLink {
    PageLink {
        section: 0,
        start,
        end,
        kind: LinkKind::Internal,
        href: href.into(),
    }
}

#[test]
fn a_word_belongs_to_the_link_it_overlaps() {
    let links = [link(10, 14, "a.html#n1"), link(20, 25, "b.html")];
    assert_eq!(link_of(&links, &word("see", 5, 8)), None);
    assert_eq!(
        link_of(&links, &word("see", 9, 12)).unwrap().href,
        "a.html#n1"
    );
    assert_eq!(
        link_of(&links, &word("more", 20, 24)).unwrap().href,
        "b.html"
    );
    // Right after the link's end: not part of it; another section: not either.
    assert_eq!(link_of(&links, &word("x", 14, 16)), None);
    let mut other = word("see", 9, 12);
    other.start = Locator::new(1, 9);
    other.end = Locator::new(1, 12);
    assert_eq!(link_of(&links, &other), None);
}

#[test]
fn the_label_is_the_part_of_the_words_inside_the_link() {
    // `source¹`: the link is only the mark, the last two bytes of the word.
    let words = [word("source", 0, 6), word("¹", 6, 8)];
    let l = link(6, 8, "n");
    assert_eq!(label_of(&words, &l).unwrap(), "¹");
    // A link over two words keeps their spacing as one space.
    let words = [word("the", 0, 3), word("end", 4, 7)];
    assert_eq!(label_of(&words, &link(0, 7, "x")).unwrap(), "the end");
}

#[test]
fn note_marks_are_told_from_words() {
    for mark in ["1", "12", "[3]", "(14)", "*", "†", "[123]"] {
        assert!(note_like(mark), "{mark}");
    }
    for text in ["CHAPTER I.", "Notes", "", "the end"] {
        assert!(!note_like(text), "{text}");
    }
}

#[test]
fn the_way_back_is_offered_where_the_last_link_landed() {
    let mut returns = Returns::new();
    let (here, there) = (Locator::new(0, 100), Locator::new(2, 0));
    assert!(returns.is_empty());
    returns.followed(here).unwrap();
    returns.landed_on(there);
    assert!(returns.offered_on(there));
    assert!(!returns.offered_on(here));
    assert_eq!(returns.back(), Some(here));
    assert_eq!(returns.back(), None);
    assert!(!returns.offered_on(there));
}

#[test]
fn only_the_newest_places_are_kept() {
    let mut returns = Returns::new();
    for n in 0..(RETURNS as u32 + 5) {
        returns.followed(Locator::new(0, n)).unwrap();
    }
    assert_eq!(returns.back(), Some(Locator::new(0, RETURNS as u32 + 4)));
    let mut last = None;
    while let Some(place) = returns.back() {
        last = Some(place);
    }
    assert_eq!(last, Some(Locator::new(0, 5)));
}

#[test]
fn the_spans_of_a_page_are_one_for_each_section() {
    let a = word("a", 5, 6);
    let b = word("b", 20, 25);
    let mut c = word("c", 1, 3);
    c.start = Locator::new(1, 1);
    c.end = Locator::new(1, 3);
    let spans = spans(&[a, b, c]).unwrap();
    assert_eq!(spans, [(0, 5, 25), (1, 1, 3)]);
}
