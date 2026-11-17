//! Tests of `widgets.rs`.
use super::*;

#[test]
fn alignments_place_text_in_the_area() {
    let area = Rect::new(10, 0, 100, 20);
    assert_eq!(Align::Left.x(area, 40.0), 10.0);
    assert_eq!(Align::Center.x(area, 40.0), 40.0);
    assert_eq!(Align::Right.x(area, 40.0), 70.0);
    // Too wide: from the left edge.
    assert_eq!(Align::Right.x(area, 140.0), 10.0);
}
