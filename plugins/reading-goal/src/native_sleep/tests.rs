use super::*;

#[test]
fn the_cat_and_the_minutes_sit_together_in_the_middle() {
    let screen = Rect::new(0, 0, 480, 800);
    let at = layout(screen, 20, 30, 24, Some((400, 400)));
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
    let at = layout(screen, 10, 18, 20, Some((400, 400)));
    // The room is 180 wide and 142 high: a square cat fills the height.
    assert_eq!(at.cat.height, 200 - 20 - 18 - 20);
    assert_eq!(at.cat.width, at.cat.height);
    assert!(at.cat.y + at.cat.height + 18 <= at.minutes.y);
    assert!(at.minutes.y + at.minutes.height <= 200);
}

#[test]
fn without_a_picture_the_words_are_above_the_minutes() {
    let screen = Rect::new(0, 0, 480, 800);
    let at = layout(screen, 20, 30, 24, None);
    assert_eq!(at.cat.height, 0);
    assert_eq!(at.words.y + at.words.height, at.cat.y);
    assert!(at.words.y < at.minutes.y);
}

#[test]
fn a_screen_with_no_room_never_underflows() {
    for (w, h) in [(0, 0), (5, 5), (40, 10)] {
        let at = layout(Rect::new(3, 4, w, h), 20, 30, 24, Some((400, 400)));
        assert_eq!(at.cat.height, 0);
        let _ = at.minutes;
    }
}

#[test]
fn the_minutes_sit_under_the_cat_as_it_is_fitted_not_under_a_box() {
    let screen = Rect::new(0, 0, 480, 800);
    // A wide cat fills the width and is low: the lines come up with it.
    let wide = layout(screen, 20, 30, 24, Some((800, 400)));
    assert_eq!((wide.cat.width, wide.cat.height), (440, 220));
    assert_eq!(wide.minutes.y, wide.cat.y + 220 + 30);
    // A tall cat fills the height the room leaves and is centred.
    let tall = layout(screen, 20, 30, 24, Some((200, 1600)));
    assert_eq!(tall.cat.height, 800 - 40 - 30 - 24);
    assert_eq!(tall.cat.width, tall.cat.height / 8);
    assert_eq!(tall.cat.x, 20 + (440 - tall.cat.width) / 2);
    // The whole is in the middle either way.
    for at in [wide, tall] {
        let above = at.cat.y;
        let below = 800 - (at.minutes.y + at.minutes.height);
        assert!(above.abs_diff(below) <= 1, "{above} {below}");
    }
}

#[test]
fn a_picture_of_no_size_is_none_at_all() {
    let at = layout(Rect::new(0, 0, 480, 800), 20, 30, 24, Some((0, 100)));
    assert_eq!((at.cat.width, at.cat.height), (0, 0));
}
