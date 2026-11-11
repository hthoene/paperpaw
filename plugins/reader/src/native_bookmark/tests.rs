use super::*;

fn range(start: (u32, u32), end: (u32, u32)) -> Range {
    Range { start, end }
}

fn line<'a>(text: &'a str, baseline: f32, words: &'a [(u32, u32)]) -> LineView<'a> {
    LineView {
        text,
        baseline,
        section: 0,
        first: words.first().copied(),
        last: words.last().copied(),
        inserted: false,
    }
}

fn text(lines: &[LineView<'_>]) -> String {
    text_of(lines.len(), |i| lines.get(i).copied()).unwrap()
}

#[test]
fn a_page_holds_what_starts_between_its_start_and_the_next_pages() {
    let page = range((1, 100), (1, 200));
    assert!(page.holds((1, 100)));
    assert!(page.holds((1, 199)));
    assert!(!page.holds((1, 200)));
    assert!(!page.holds((1, 99)));
    assert!(!page.holds((0, 500)));
    // The page across a section's end.
    let across = range((1, 900), (2, 40));
    assert!(across.holds((1, 950)) && across.holds((2, 39)) && !across.holds((2, 40)));
}

#[test]
fn a_page_past_the_end_shows_nothing() {
    assert!(range((3, 10), (3, 10)).is_empty());
    assert!(!range((3, 10), (3, 11)).is_empty());
}

#[test]
fn only_a_bookmark_that_starts_on_the_page_in_the_same_conversion_is_its_bookmark() {
    let page = range((0, 0), (0, 50));
    let here = |kind, at, conversion| is_here(&page, kind, at, conversion, 7);
    assert!(here(AnnotationKind::Bookmark, (0, 10), Some(7)));
    // No conversion: taken as this one (a mark written by another program).
    assert!(here(AnnotationKind::Bookmark, (0, 10), None));
    assert!(!here(AnnotationKind::Bookmark, (0, 10), Some(8)));
    assert!(!here(AnnotationKind::Highlight, (0, 10), Some(7)));
    assert!(!here(AnnotationKind::Bookmark, (0, 50), Some(7)));
}

#[test]
fn the_progress_is_kept_in_whole_percent_rounded() {
    assert_eq!(percent_of(0), 0);
    assert_eq!(percent_of(4_999), 0);
    assert_eq!(percent_of(5_000), 1);
    assert_eq!(percent_of(504_999), 50);
    assert_eq!(percent_of(1_000_000), 100);
    assert_eq!(percent_of(u32::MAX), 100);
}

#[test]
fn the_lines_of_a_page_join_with_spaces() {
    let lines = [
        line("It was a dark", 10.0, &[(0, 2), (3, 6), (7, 8), (9, 13)]),
        line("and stormy night.", 30.0, &[(14, 17), (18, 24), (25, 31)]),
    ];
    assert_eq!(text(&lines), "It was a dark and stormy night.");
}

#[test]
fn a_word_broken_at_the_end_of_a_line_is_whole_without_the_hyphen_the_layout_put() {
    // "wonderful" broken as "won-" / "derful": the next line's first word
    // starts where this line's last word ends.
    let lines = [
        line("A won-", 10.0, &[(0, 1), (2, 5)]),
        line("derful day", 30.0, &[(5, 10), (11, 14)]),
    ];
    assert_eq!(text(&lines), "A wonderful day");
    // A hyphen of the book's own (the words are apart) stays.
    let own = [
        line("a well-", 10.0, &[(0, 1), (2, 7)]),
        line("known fact", 30.0, &[(8, 13), (14, 18)]),
    ];
    assert_eq!(text(&own), "a well- known fact");
}

#[test]
fn what_a_filter_inserted_is_not_the_books() {
    let mut inserted = line("(gloss)", 20.0, &[]);
    inserted.inserted = true;
    let lines = [
        line("one", 10.0, &[(0, 3)]),
        inserted,
        line("two", 30.0, &[(4, 7)]),
    ];
    assert_eq!(text(&lines), "one two");
}

#[test]
fn a_drop_capital_goes_on_beside_the_word_it_starts() {
    // The capital line's baseline is not above the next line's.
    let lines = [
        line("O", 40.0, &[(0, 1)]),
        line("nce upon", 30.0, &[(1, 4), (5, 9)]),
    ];
    assert_eq!(text(&lines), "Once upon");
}

#[test]
fn a_snippet_is_one_line_cut_at_a_character() {
    assert_eq!(snippet("a\n b\t\tc", 100).unwrap(), "a  b  c");
    let long = "é".repeat(400);
    let cut = snippet(&long, 512).unwrap();
    assert!(cut.len() <= 512 && cut.chars().all(|c| c == 'é'));
    assert_eq!(snippet("abc", 2).unwrap(), "ab");
    assert_eq!(snippet("", 10).unwrap(), "");
}

#[test]
fn building_the_text_survives_every_refused_allocation() {
    let lines = [
        line("A won-", 10.0, &[(0, 1), (2, 5)]),
        line("derful day", 30.0, &[(5, 10), (11, 14)]),
    ];
    crate::fail_alloc::sweep(|| text_of(lines.len(), |i| lines.get(i).copied()).map(|t| t.len()));
    crate::fail_alloc::sweep(|| snippet("a snippet to cut", 8).map(|t| t.len()));
}
