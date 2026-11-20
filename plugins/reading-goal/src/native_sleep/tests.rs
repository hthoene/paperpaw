use super::*;

#[test]
fn a_wide_picture_fills_the_width_a_tall_one_the_height() {
    assert_eq!(fitted((800, 400), (440, 420)), (440, 220));
    assert_eq!(fitted((400, 400), (440, 420)), (420, 420));
    assert_eq!(fitted((200, 1600), (440, 420)), (52, 420));
}

#[test]
fn a_picture_of_no_size_or_a_room_of_none_is_none_at_all() {
    assert_eq!(fitted((0, 100), (440, 420)), (0, 0));
    assert_eq!(fitted((100, 100), (0, 420)), (0, 0));
}

#[test]
fn the_share_is_the_minutes_of_the_goal_at_most_all() {
    let today = |goal, minutes| Today {
        goal,
        minutes,
        mood: Mood::Sad,
        statistics: Statistics::Counting,
    };
    assert_eq!(share(&today(Some(20), 14)), Some(70));
    assert_eq!(share(&today(Some(20), 42)), Some(100));
    assert_eq!(share(&today(None, 42)), None);
    assert_eq!(share(&today(Some(0), 42)), None);
}
