//! The panel's state and its wrapping, without a host.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::string::ToString;
use alloc::vec;

fn panel() -> Panel {
    let mut panel = Panel::new("Hello\n  world,   this is  text".to_string()).unwrap();
    // As a drawing leaves it: where it is, and how many lines it showed.
    panel.region = Rect::new(0, 100, 300, 200);
    panel.buttons = [Rect::new(10, 150, 280, 30), Rect::new(10, 190, 280, 30)];
    panel.rows = 2;
    panel
}

fn lines(panel: &mut Panel, n: usize) {
    panel.lines = Some((0..n).map(|i| i.to_string()).collect());
}

#[test]
fn the_heading_is_the_original_in_one_line_from_its_first_bytes() {
    assert_eq!(panel().heading, "Hello world, this is text");
    let long = "ж ".repeat(1000);
    let heading = heading_of(&long).unwrap();
    assert!(heading.len() <= HEADING_BYTES);
    assert!(heading.starts_with("ж ж"));
    assert_eq!(heading_of("").unwrap(), "");
}

#[test]
fn it_starts_loading_and_asks_once() {
    let mut p = panel();
    assert_eq!(p.body(), &Body::Note(Note::Loading));
    assert!(p.first_time());
    assert!(!p.first_time());
}

#[test]
fn the_notes_are_the_interface_6_texts() {
    for (note, key) in [
        (Note::Loading, "translate.loading"),
        (Note::NeedsKey, "translate.needs-key"),
        (Note::BadKey, "translate.bad-key"),
        (Note::Quota, "translate.quota"),
        (Note::NoNetwork, "translate.no-network"),
        (Note::Unreachable, "translate.unreachable"),
        (Note::Failed, "translate.failed"),
    ] {
        assert_eq!(note.key(), key);
    }
}

#[test]
fn select_closes_or_asks_for_the_key_and_the_arrows_scroll() {
    let mut p = panel();
    lines(&mut p, 5);
    assert_eq!(p.action("core.confirm"), Act::Close);
    assert_eq!(p.action("core.next-page"), Act::Moved);
    assert_eq!(p.top, 2);
    assert_eq!(p.action("core.down"), Act::Moved);
    assert_eq!(p.top, 4);
    // At the end: taken, nothing moves.
    assert_eq!(p.action("core.next"), Act::Held);
    assert_eq!(p.action("core.prev-page"), Act::Moved);
    assert_eq!(p.top, 2);
    assert_eq!(p.action("core.left"), Act::Moved);
    assert_eq!(p.action("core.up"), Act::Held);
    assert_eq!(p.action("core.menu"), Act::Ignored);
    // No key, or one refused: Select asks for it.
    for note in [Note::NeedsKey, Note::BadKey] {
        p.say(note);
        assert!(p.needs_key());
        assert_eq!(p.action("core.confirm"), Act::AskKey);
    }
    p.show("Hallo".into());
    assert!(!p.needs_key());
    assert_eq!(p.top, 0);
    assert!(p.lines.is_none(), "wrapped again for the new text");
}

#[test]
fn a_tap_scrolls_forward_back_asks_for_the_key_or_closes_at_the_end() {
    let mut p = panel();
    lines(&mut p, 3);
    let right = Point::new(250, 150);
    let left = Point::new(20, 150);
    assert_eq!(p.tap(Point::new(250, 50)), Act::Ignored, "above the panel");
    assert_eq!(p.tap(right), Act::Moved);
    // The last page: a tap forward closes, one back goes back.
    assert_eq!(p.tap(right), Act::Close);
    assert_eq!(p.tap(left), Act::Moved);
    assert_eq!(p.tap(left), Act::Held);
    p.say(Note::NeedsKey);
    assert_eq!(p.tap(left), Act::AskKey);
    assert_eq!(p.tap(right), Act::AskKey);
}

#[test]
fn the_offer_is_made_once_and_chosen_by_keys_or_taps() {
    let mut p = panel();
    assert!(p.offer());
    assert_eq!(p.body(), &Body::Offer);
    assert!(p.offered());
    // Go online is focused first.
    assert_eq!(p.action("core.confirm"), Act::GoOnline);
    assert_eq!(p.action("core.down"), Act::Focus);
    assert_eq!(p.action("core.down"), Act::Held);
    assert_eq!(p.action("core.confirm"), Act::NotNow);
    assert_eq!(p.action("core.up"), Act::Focus);
    assert_eq!(p.action("core.menu"), Act::Ignored);
    assert_eq!(p.tap(Point::new(100, 160)), Act::GoOnline);
    assert_eq!(p.tap(Point::new(100, 200)), Act::NotNow);
    assert_eq!(
        p.tap(Point::new(100, 130)),
        Act::Held,
        "between the buttons"
    );
    assert_eq!(p.tap(Point::new(100, 50)), Act::Ignored);
    // Not again on this panel.
    p.say(Note::Loading);
    assert!(!p.offer());
    assert_ne!(p.body(), &Body::Offer);
}

/// Each character is 10 wide.
fn wide(text: &str) -> Result<f32, Error> {
    Ok(text.chars().count() as f32 * 10.0)
}

#[test]
fn paragraphs_wrap_apart_and_an_empty_one_is_a_line() {
    let wrapped = wrap_with(wide, "aaa bbb ccc\n\nddd", 70.0).unwrap();
    assert_eq!(wrapped, vec!["aaa bbb", "ccc", "", "ddd"]);
    assert_eq!(wrap_with(wide, "", 70.0).unwrap(), vec![String::new()]);
}

#[test]
fn a_text_of_too_many_lines_ends_in_an_ellipsis() {
    let long = "x\n".repeat(MAX_LINES + 50);
    let wrapped = wrap_with(wide, &long, 70.0).unwrap();
    assert_eq!(wrapped.len(), MAX_LINES);
    assert!(wrapped.last().unwrap().ends_with('…'));
    // Whole when it fits.
    let fits = "x\n".repeat(MAX_LINES - 1) + "x";
    let wrapped = wrap_with(wide, &fits, 70.0).unwrap();
    assert_eq!(wrapped.len(), MAX_LINES);
    assert!(!wrapped.last().unwrap().ends_with('…'));
}
