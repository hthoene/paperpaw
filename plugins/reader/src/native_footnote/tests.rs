use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;

#[test]
fn note_marks() {
    for mark in ["1", "12", "[3]", "(14)", "*", "†", "[123]", " 7 "] {
        assert!(note_like(mark), "{mark}");
    }
    for text in ["CHAPTER I.", "Notes", "", "   ", "the end", "[1234567]"] {
        assert!(!note_like(text), "{text}");
    }
}

#[test]
fn a_link_opens_as_a_note_only_where_it_is_one() {
    assert!(opens_as_note(true, false, "[2]"));
    assert!(!opens_as_note(false, false, "[2]"));
    assert!(!opens_as_note(true, true, "[2]"));
    assert!(!opens_as_note(true, false, "Chapter II"));
}

#[test]
fn the_note_has_room_for_the_lines_of_its_half_and_at_least_one() {
    // 800 high: 400 for the note, less a row of 60 and three gaps of 10.
    assert_eq!(lines_room(800, 60, 10, 30), (400 - 60 - 30) / 30);
    assert_eq!(lines_room(100, 60, 10, 30), 1);
    assert_eq!(lines_room(800, 60, 10, 0), 310);
    assert_eq!(lines_room(0, u16::MAX, u16::MAX, 1), 1);
}

#[test]
fn a_text_that_fits_is_its_lines_and_one_that_does_not_ends_in_what_fit_makes_of_the_rest() {
    let text = "one two three four five six";
    let ranges = [0..8, 8..19, 19..27];
    let all = lines(text, &ranges, 5, |_| panic!("nothing is cut")).unwrap();
    assert_eq!(all, ["one two", "three four", "five six"]);
    let mut seen = None;
    let cut = lines(text, &ranges, 2, |rest| {
        seen = Some(rest.to_string());
        Ok("three four…".to_string())
    })
    .unwrap();
    assert_eq!(cut, ["one two", "three four…"]);
    // The rest, from where the last line kept begins.
    assert_eq!(seen.as_deref(), Some("three four five six"));
    // Nothing to show is no lines; room for none is room for one.
    assert!(lines("", &[], 3, |_| Ok(String::new())).unwrap().is_empty());
    let one = lines(text, &ranges, 0, |rest| Ok(rest.chars().take(3).collect())).unwrap();
    assert_eq!(one, ["one"]);
}

fn metrics() -> Metrics {
    let (width, height, margin, gap) = (480, 800, 20, 10);
    Metrics {
        width,
        height,
        margin,
        gap,
        row: 40,
        line: 30,
        button_row: 60,
        room: 12,
    }
}

#[test]
fn the_buttons_lie_side_by_side_in_the_overlay_at_the_bottom() {
    let m = metrics();
    assert!(side_by_side([60, 55], &m));
    assert!(!side_by_side([60, 90], &m));
    let at = layout(&m, 3, true, [0, 0]);
    // 3 gaps less half a gap, three lines, the buttons and the edge room.
    let height = (30 - 5) + 90 + 60 + 12;
    assert_eq!(at.region, Rect::new(0, 800 - height, 480, height));
    let y = at.region.y + height - 60 - 12;
    assert_eq!(at.buttons[0], Rect::new(20, y, 240 - 25, 60));
    assert_eq!(at.buttons[1], Rect::new(245, y, 215, 60));
    assert_eq!(m.half_width(), 215);
    assert_eq!(m.inner_width(), 440);
    // Both inside the overlay, apart, inside the display.
    assert!(at.buttons[0].right() <= at.buttons[1].x as u32);
    assert!(at.buttons[1].right() <= 480);
}

#[test]
fn the_buttons_lie_one_below_the_other_where_a_label_is_long() {
    let m = metrics();
    let at = layout(&m, 2, false, [60, 100]);
    let height = (30 - 5) + 60 + (60 + 10 + 100) + 12;
    assert_eq!(at.region.height, height);
    let y = at.region.y + height - (60 + 10 + 100) - 12;
    assert_eq!(at.buttons[0], Rect::new(20, y, 440, 60));
    assert_eq!(at.buttons[1], Rect::new(20, y + 70, 440, 100));
    assert_eq!(at.region.bottom(), 800);
}

#[test]
fn a_layout_that_is_too_big_stays_inside_the_numbers() {
    let m = Metrics {
        width: u16::MAX,
        height: 10,
        margin: u16::MAX,
        gap: u16::MAX,
        row: u16::MAX,
        line: u16::MAX,
        button_row: u16::MAX,
        room: u16::MAX,
    };
    let at = layout(&m, usize::MAX, false, [u16::MAX; 2]);
    assert_eq!(at.region.y, 0);
    assert_eq!(at.region.height, u16::MAX);
    assert_eq!(layout(&m, 0, true, [0; 2]).region.width, u16::MAX);
}

#[test]
fn the_keys_move_the_focus_press_and_close() {
    assert_eq!(on_key(None, Key::Previous), Decision::Focus(0));
    assert_eq!(on_key(Some(0), Key::Next), Decision::Focus(1));
    assert_eq!(
        on_key(Some(1), Key::Confirm),
        Decision::Press(Press::Follow)
    );
    assert_eq!(on_key(Some(0), Key::Confirm), Decision::Press(Press::Close));
    // After a touch no button is focused: confirm closes.
    assert_eq!(on_key(None, Key::Confirm), Decision::Press(Press::Close));
    assert_eq!(on_key(Some(1), Key::Back), Decision::Close);
}

#[test]
fn a_tap_presses_the_button_it_is_on_and_elsewhere_closes() {
    let at = layout(&metrics(), 3, true, [0, 0]);
    let inside = |r: Rect| Point::new(i32::from(r.x) + 2, i32::from(r.y) + 2);
    assert_eq!(on_tap(&at.buttons, inside(at.buttons[0])), Press::Close);
    assert_eq!(on_tap(&at.buttons, inside(at.buttons[1])), Press::Follow);
    assert_eq!(on_tap(&at.buttons, Point::new(0, 0)), Press::Close);
    assert_eq!(on_tap(&at.buttons, Point::new(-5, -5)), Press::Close);
}

#[test]
fn the_buttons_drawn_again_are_the_two_the_focus_moved_between() {
    assert_eq!(redraw(Some(0), Some(1)), Some((0, 1)));
    assert_eq!(redraw(Some(1), Some(1)), Some((1, 1)));
    // From no focus (a touch): the one it came to, twice.
    assert_eq!(redraw(None, Some(0)), Some((0, 0)));
    assert_eq!(redraw(Some(0), None), None);
}

#[test]
fn every_allocation_may_be_refused() {
    let text = "one two three four five six";
    let ranges = [0..8, 8..19, 19..27];
    sweep(|| {
        lines(text, &ranges, 5, |_| copy(""))?;
        lines(text, &ranges, 2, copy)?;
        Ok(())
    });
}
