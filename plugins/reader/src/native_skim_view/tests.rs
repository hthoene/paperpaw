use super::*;
use crate::declarations;

const SIZES: (u16, u16) = (2, 8);

#[test]
fn the_track_keeps_the_knob_inside_the_bar() {
    let bar = Rect::new(16, 100, 768, 40);
    let track = track_of(SIZES, bar);
    let knob = knob_of(bar);
    assert!(track.x >= bar.x + knob / 2);
    assert!(track.x + track.width + knob / 2 <= bar.x + bar.width);
    assert!(track.y >= bar.y && track.y + track.height <= bar.y + bar.height);
    // A bar too narrow for its insets still has a track.
    let tiny = Rect::new(0, 0, 10, 20);
    assert!(track_of(SIZES, tiny).width <= tiny.width);
}

#[test]
fn the_screen_is_the_one_declared_for_the_slot() {
    assert_eq!(declarations::SKIM, 13);
    assert_eq!(SCREEN_NAME, "reader.skim-screen");
}

#[test]
fn a_view_without_a_layout_ignores_a_tap() {
    let mut view = SkimView {
        skim: Skim::new(false),
        layout: None,
    };
    assert_eq!(view.tap(Point::new(1, 1)), Step::Ignored);
    assert!(view.region().is_none());
}
