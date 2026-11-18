use super::*;

const SCREEN: (i32, i32) = (480, 800);

#[test]
fn holding_select_is_the_views_action_and_the_keys_zoom_pan_or_close() {
    assert_eq!(HOLD, "reader.hold-select");
    assert_eq!(key("core.back"), Key::Back);
    assert_eq!(key("core.menu"), Key::Close);
    assert_eq!(key("core.confirm"), Key::Cycle);
    assert_eq!(key("core.left"), Key::Left);
    assert_eq!(key("core.next-page"), Key::Next);
    // Home and the panels stay the host's.
    assert_eq!(key("core.home"), Key::Other);
}

#[test]
fn the_pictures_of_a_page_are_stepped_through_and_stop_at_the_ends() {
    // Pictures 0 and 2 can be drawn; 1 has no id.
    let places = || [0, 2].into_iter();
    assert_eq!(stepped(places(), 0, true), Some(2));
    assert_eq!(stepped(places(), 2, false), Some(0));
    assert_eq!(stepped(places(), 2, true), None);
    assert_eq!(stepped(places(), 0, false), None);
}

#[test]
fn a_touch_hits_the_picture_it_is_in() {
    let b = (40, 100, 300, 200);
    assert!(inside(b, (40, 100)));
    assert!(inside(b, (339, 299)));
    assert!(!inside(b, (340, 150)));
    assert!(!inside(b, (100, 300)));
    assert!(!inside(b, (39, 150)));
}

#[test]
fn select_cycles_fit_2x_3x_and_back() {
    assert_eq!(next_level(0, 2), 1);
    assert_eq!(next_level(1, 2), 2);
    assert_eq!(next_level(2, 2), 0);
}

#[test]
fn a_picture_too_big_for_a_level_stops_the_cycle_below_it() {
    // 3x was refused at 2x: the cycle is fit, 2x, fit.
    assert_eq!(next_level(1, 1), 0);
    // 2x was refused at fit: it stays at fit.
    assert_eq!(next_level(0, 0), 0);
}

#[test]
fn a_zoom_is_decoded_at_the_size_it_is_drawn_at_and_no_larger_than_the_host_gives() {
    let fit = fit_size((300, 200), SCREEN);
    assert_eq!(fit, (480, 320));
    assert_eq!(zoomed_size(fit, 1), (960, 640));
    assert_eq!(side_for(zoomed_size(fit, 1)), 960);
    assert_eq!(side_for(zoomed_size(fit, 2)), 1_440);
    // A tall picture at 3x would be 2,400 high: the host's most.
    let tall = fit_size((400, 1_000), SCREEN);
    assert_eq!(tall, (320, 800));
    assert_eq!(side_for(zoomed_size(tall, 2)), SIDE_MAX);
}

#[test]
fn a_zoomed_picture_is_kept_in_the_frame() {
    let size = (960, 640);
    // Wider than the screen: pinned at both edges; shorter: centred.
    assert_eq!(clamp_pan((100, 0), size, SCREEN), (0, 80));
    assert_eq!(clamp_pan((-900, 0), size, SCREEN), (-480, 80));
    assert_eq!(clamp_pan((-100, 0), size, SCREEN), (-100, 80));
    // A picture that fits an axis is centred there.
    assert_eq!(clamp_pan((7, 7), (480, 320), SCREEN), (0, 240));
}

#[test]
fn the_arrows_pan_by_four_fifths_and_stop_at_the_edges() {
    let size = (1_440, 2_400);
    let step = (384, 640);
    let start = recentred((0, 0), (480, 800), size, SCREEN);
    assert_eq!(start, (-480, -800));
    // Right goes on to the right edge (the picture moves left) and stops there.
    let right = panned(start, (-step.0, 0), size, SCREEN);
    assert_eq!(right, (-864, -800));
    let edge = panned(right, (-step.0, 0), size, SCREEN);
    assert_eq!(edge, (-960, -800));
    assert_eq!(panned(edge, (-step.0, 0), size, SCREEN), edge);
    // Down the same way, up to the top.
    assert_eq!(panned(start, (0, step.1), size, SCREEN), (-480, -160));
    assert_eq!(panned((0, 0), (step.0, step.1), size, SCREEN), (0, 0));
}

#[test]
fn the_page_buttons_go_on_in_reading_order() {
    let size = (960, 1_600);
    let step = (384, 640);
    let mut at = (0, 0);
    // Across the row, then the start of the next row.
    at = scanned(at, step, size, SCREEN, true);
    assert_eq!(at, (-384, 0));
    at = scanned(at, step, size, SCREEN, true);
    assert_eq!(at, (-480, 0));
    at = scanned(at, step, size, SCREEN, true);
    assert_eq!(at, (0, -640));
    // The last row's end is the end.
    let last = (-480, -800);
    assert_eq!(scanned(last, step, size, SCREEN, true), last);
    // And the way back from the row's start is the end of the row above.
    assert_eq!(scanned((0, -640), step, size, SCREEN, false), (-480, 0));
    assert_eq!(scanned((0, 0), step, size, SCREEN, false), (0, 0));
}

#[test]
fn zooming_keeps_the_middle_of_the_view() {
    // Fit 480 x 320 centred at (0, 240); at 2x the same middle stays in the middle.
    let pan = recentred((0, 240), (480, 320), (960, 640), SCREEN);
    assert_eq!(pan, (-240, 80));
}

#[test]
fn two_taps_close_together_are_a_double_tap() {
    let mut last = None;
    assert!(!double_tap(&mut last, 1_000, Point::new(100, 100)));
    assert!(double_tap(&mut last, 1_300, Point::new(120, 90)));
    // The pair is used up: the next tap starts another.
    assert!(!double_tap(&mut last, 1_400, Point::new(120, 90)));
    // Too late, or too far.
    assert!(!double_tap(&mut last, 2_000, Point::new(120, 90)));
    assert!(!double_tap(&mut last, 2_100, Point::new(400, 700)));
}
