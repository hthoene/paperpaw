use super::*;

#[test]
fn the_cat_and_the_minutes_sit_together_in_the_middle() {
    let screen = Rect::new(0, 0, 480, 800);
    let at = layout(screen, 20, 30, 24, true);
    // As wide as the room leaves, a square.
    assert_eq!((at.cat.x, at.cat.width, at.cat.height), (20, 440, 440));
    // The minutes below it, the whole centred: equal room above and below.
    assert_eq!(at.minutes.y, at.cat.y + at.cat.height + 30);
    let above = at.cat.y;
    let below = 800 - (at.minutes.y + at.minutes.height);
    assert!(above.abs_diff(below) <= 1, "{above} {below}");
}

#[test]
fn a_low_screen_makes_the_cat_smaller_and_the_text_never_overlaps() {
    let screen = Rect::new(0, 0, 200, 200);
    let at = layout(screen, 10, 18, 20, true);
    assert_eq!(at.cat.width, 180);
    assert_eq!(at.cat.height, 200 - 20 - 18 - 20);
    assert!(at.cat.y + at.cat.height + 18 <= at.minutes.y);
    assert!(at.minutes.y + at.minutes.height <= 200);
}

#[test]
fn without_a_picture_the_words_are_above_the_minutes() {
    let screen = Rect::new(0, 0, 480, 800);
    let at = layout(screen, 20, 30, 24, false);
    assert_eq!(at.cat.height, 0);
    assert_eq!(at.words.y + at.words.height, at.cat.y);
    assert!(at.words.y < at.minutes.y);
}

#[test]
fn a_screen_with_no_room_never_underflows() {
    for (w, h) in [(0, 0), (5, 5), (40, 10)] {
        let at = layout(Rect::new(3, 4, w, h), 20, 30, 24, true);
        assert_eq!(at.cat.height, 0);
        let _ = at.minutes;
    }
}
