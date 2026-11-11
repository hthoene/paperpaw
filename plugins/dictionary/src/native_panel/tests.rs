use super::*;
use alloc::{format, string::ToString};

/// Every character is 10 wide.
fn ten(text: &str) -> Result<f32, Error> {
    Ok(text.chars().count() as f32 * 10.0)
}

fn entry(source: &str, text: &str) -> Entry {
    Entry {
        source: source.to_string(),
        text: text.to_string(),
    }
}

fn lines(panel: &Panel) -> Vec<String> {
    panel.page(100).map(|(t, _)| t.to_string()).collect()
}

#[test]
fn text_wraps_by_words_and_keeps_paragraphs_and_blank_lines() {
    let panel = Panel::new(&[entry("A", "one two three\nfour")], "", ten, 80.0).unwrap();
    assert_eq!(lines(&panel), ["one two", "three", "four"]);
    let panel = Panel::new(&[entry("A", "x"), entry("A", "y")], "", ten, 80.0).unwrap();
    assert_eq!(lines(&panel), ["x", "", "y"]);
}

#[test]
fn a_word_wider_than_a_line_is_cut() {
    let panel = Panel::new(&[entry("A", "abcdefghij k")], "", ten, 40.0).unwrap();
    assert_eq!(lines(&panel), ["abcd", "efgh", "ij k"]);
}

#[test]
fn sources_are_named_only_where_there_are_several() {
    let one = Panel::new(&[entry("A", "x"), entry("A", "y")], "", ten, 200.0).unwrap();
    assert!(lines(&one).iter().all(|l| !l.starts_with('(')));
    let two = Panel::new(&[entry("A", "x"), entry("B", "y")], "", ten, 200.0).unwrap();
    assert_eq!(lines(&two), ["(A)", "x", "", "(B)", "y"]);
    let faint: Vec<bool> = two.page(100).map(|(_, f)| f).collect();
    assert_eq!(faint, [true, false, false, true, false]);
}

#[test]
fn nothing_found_shows_the_message_and_empty_entries_are_left_out() {
    let panel = Panel::new(&[entry("A", "  ")], "No entry", ten, 200.0).unwrap();
    assert_eq!(lines(&panel), ["No entry"]);
}

#[test]
fn pages_and_scrolling_stay_inside_the_text() {
    let region = Rect::new(0, 100, 200, 100);
    let mut panel = Panel::new(&[entry("A", "a\nb\nc\nd\ne")], "", ten, 200.0).unwrap();
    assert_eq!(panel.pages(2), (1, 3));
    let action = |action| Input::Action { now_ms: 0, action };
    assert_eq!(
        panel.decide(2, region, &action("core.next-page")),
        Step::Scroll(2)
    );
    assert_eq!(
        panel.decide(2, region, &action("core.prev-page")),
        Step::Edge
    );
    panel.set_top(4);
    assert_eq!(panel.pages(2), (3, 3));
    assert_eq!(panel.decide(2, region, &action("core.down")), Step::Edge);
    assert_eq!(panel.decide(2, region, &action("core.up")), Step::Scroll(2));
    assert_eq!(
        panel.decide(2, region, &action("core.confirm")),
        Step::Close
    );
    panel.set_top(99);
    assert_eq!(panel.top(), 4);
}

#[test]
fn a_tap_scrolls_on_to_the_right_goes_back_to_the_left_and_closes_outside() {
    let region = Rect::new(0, 100, 200, 100);
    let mut panel = Panel::new(&[entry("A", "a\nb\nc\nd")], "", ten, 200.0).unwrap();
    let tap = |x, y| Input::Gesture {
        now_ms: 0,
        position: Point::new(x, y),
        kind: InputGestureKind::Tap,
    };
    assert_eq!(panel.decide(2, region, &tap(150, 150)), Step::Scroll(2));
    // On the first page the left part closes: there is nothing to go back to.
    assert_eq!(panel.decide(2, region, &tap(50, 150)), Step::Close);
    panel.set_top(2);
    assert_eq!(panel.decide(2, region, &tap(50, 150)), Step::Scroll(0));
    assert_eq!(panel.decide(2, region, &tap(150, 150)), Step::Edge);
    assert_eq!(panel.decide(2, region, &tap(150, 50)), Step::Close);
}

#[test]
fn a_huge_text_is_kept_to_the_line_limit() {
    let long = "word ".repeat(5_000);
    let panel = Panel::new(&[entry("A", &long)], "", ten, 100.0).unwrap();
    assert!(panel.pages(1).1 <= MAX_LINES);
}

#[test]
fn keys_at_the_ends_are_taken_and_other_keys_are_not() {
    let region = Rect::new(0, 100, 200, 100);
    let panel = Panel::new(&[entry("A", "a\nb")], "", ten, 200.0).unwrap();
    let action = |action| Input::Action { now_ms: 0, action };
    assert_eq!(
        panel.decide(5, region, &action("core.next-page")),
        Step::Edge
    );
    assert_eq!(panel.decide(5, region, &action("core.up")), Step::Edge);
    assert_eq!(panel.decide(5, region, &action("core.menu")), Step::Ignored);
}

#[test]
fn a_definition_over_the_entry_limit_is_wrapped_whole_to_its_last_word() {
    let long = (0..1_200)
        .map(|n| format!("w{n}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(long.len() > 2_048 * 2);
    let panel = Panel::new(&[entry("A", &long)], "", ten, 200.0).unwrap();
    let shown = panel
        .page(10_000)
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(shown.ends_with("w1199"), "{shown}");
    assert_eq!(shown.split(' ').count(), 1_200);
}
