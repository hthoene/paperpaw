use super::*;

/// Every character is 10 wide.
fn ten(text: &str) -> Result<f32, Error> {
    Ok(text.chars().count() as f32 * 10.0)
}

fn lines(page: &Page) -> Vec<&str> {
    page.page(100).collect()
}

fn action(action: &str) -> Input<'_> {
    Input::Action { now_ms: 0, action }
}

#[test]
fn text_wraps_by_words_and_keeps_paragraphs_and_blank_lines() {
    let page = Page::new("one two three\nfour", ten, 80.0).unwrap();
    assert_eq!(lines(&page), ["one two", "three", "four"]);
    let page = Page::new("x\n\ny", ten, 80.0).unwrap();
    assert_eq!(lines(&page), ["x", "", "y"]);
}

#[test]
fn a_word_wider_than_a_line_is_cut() {
    let page = Page::new("abcdefghij k", ten, 40.0).unwrap();
    assert_eq!(lines(&page), ["abcd", "efgh", "ij k"]);
}

#[test]
fn scrolling_stays_inside_the_text_and_keys_at_the_ends_are_taken() {
    let region = Rect::new(0, 100, 200, 100);
    let mut page = Page::new("a\nb\nc\nd\ne", ten, 200.0).unwrap();
    assert_eq!(
        page.decide(2, region, &action("core.next-page")),
        Step::Scroll(2)
    );
    assert_eq!(
        page.decide(2, region, &action("core.prev-page")),
        Step::Edge
    );
    page.set_top(4);
    assert_eq!(page.decide(2, region, &action("core.down")), Step::Edge);
    assert_eq!(page.decide(2, region, &action("core.up")), Step::Scroll(2));
    assert_eq!(page.decide(2, region, &action("core.confirm")), Step::Close);
    assert_eq!(page.decide(2, region, &action("core.menu")), Step::Ignored);
    page.set_top(99);
    assert_eq!(lines(&page), ["e"]);
}

#[test]
fn a_tap_scrolls_on_to_the_right_goes_back_to_the_left_and_closes_outside() {
    let region = Rect::new(0, 100, 200, 100);
    let mut page = Page::new("a\nb\nc\nd", ten, 200.0).unwrap();
    let tap = |x, y| Input::Gesture {
        now_ms: 0,
        position: Point::new(x, y),
        kind: InputGestureKind::Tap,
    };
    assert_eq!(page.decide(2, region, &tap(150, 150)), Step::Scroll(2));
    // On the first page the left part closes: there is nothing to go back to.
    assert_eq!(page.decide(2, region, &tap(50, 150)), Step::Close);
    page.set_top(2);
    assert_eq!(page.decide(2, region, &tap(50, 150)), Step::Scroll(0));
    assert_eq!(page.decide(2, region, &tap(150, 150)), Step::Edge);
    assert_eq!(page.decide(2, region, &tap(150, 50)), Step::Close);
}

#[test]
fn a_huge_text_is_kept_to_the_line_limit() {
    let long = "word ".repeat(5_000);
    let page = Page::new(&long, ten, 100.0).unwrap();
    assert!(page.page(10_000).count() <= MAX_LINES);
}
