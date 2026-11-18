use super::*;

#[test]
fn the_size_steps_by_points_within_the_range() {
    assert_eq!(stepped(12, 1), 13);
    assert_eq!(stepped(12, -1), 11);
    assert_eq!(stepped(6, -1), 6);
    assert_eq!(stepped(72, 1), 40);
    assert_eq!(stepped(40, 1), 40);
    assert_eq!(stepped(i64::MAX, 1), 40);
    assert_eq!(stepped(i64::MIN, -1), 6);
}

#[test]
fn the_next_chapter_starts_at_or_after_the_page_end() {
    let starts = [(0, 0), (1, 0), (1, 400), (3, 0)];
    assert_eq!(choose(starts.into_iter(), (1, 0), true), Some((1, 0)));
    assert_eq!(choose(starts.into_iter(), (1, 1), true), Some((1, 400)));
    assert_eq!(choose(starts.into_iter(), (3, 1), true), None);
}

#[test]
fn the_previous_chapter_starts_before_the_page() {
    let starts = [(0, 0), (1, 0), (1, 400), (3, 0)];
    assert_eq!(choose(starts.into_iter(), (1, 400), false), Some((1, 0)));
    assert_eq!(choose(starts.into_iter(), (3, 0), false), Some((1, 400)));
    assert_eq!(choose(starts.into_iter(), (0, 0), false), None);
    // No table: no chapter either way.
    assert_eq!(choose(core::iter::empty(), (1, 0), true), None);
}
