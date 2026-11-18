use super::*;
use crate::fail_alloc::sweep;
use alloc::vec;

const AREA: Rect = Rect::new(0, 0, 480, 800);

#[test]
fn the_list_is_split_at_commas_and_trimmed() {
    let all: Vec<&str> = ids(" home.date , ,home.recent,core.fill ").collect();
    assert_eq!(all, ["home.date", "home.recent", "core.fill"]);
    assert_eq!(ids("").count(), 0);
    assert_eq!(ids(" , ").count(), 0);
}

#[test]
fn widgets_go_in_the_middle_two_gaps_apart() {
    let placed = stack(AREA, 20, 10, &[Entry::Widget(100), Entry::Widget(200)]).unwrap();
    // 100 + 20 + 200 = 320 high: (800 - 320) / 2 = 240 from the top.
    assert_eq!(
        placed,
        [
            (0, Rect::new(20, 240, 440, 100)),
            (1, Rect::new(20, 360, 440, 200)),
        ]
    );
}

#[test]
fn a_widget_with_nothing_to_show_is_left_out() {
    let placed = stack(AREA, 0, 10, &[Entry::Widget(0), Entry::Widget(100)]).unwrap();
    assert_eq!(placed, [(1, Rect::new(0, 350, 480, 100))]);
    assert!(stack(AREA, 0, 10, &[Entry::Widget(0)]).unwrap().is_empty());
    assert!(stack(AREA, 0, 10, &[]).unwrap().is_empty());
    // Free spaces alone show nothing.
    assert!(stack(AREA, 0, 10, &[Entry::Fill]).unwrap().is_empty());
}

#[test]
fn those_that_do_not_fit_below_the_others_are_left_out_with_what_follows() {
    let entries = [
        Entry::Widget(500),
        Entry::Widget(400),
        Entry::Widget(10),
        Entry::Widget(0),
    ];
    // 500 + 20 + 400 > 800: the second ends the list, the third is not tried.
    let placed = stack(AREA, 0, 10, &entries).unwrap();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].0, 0);
    assert_eq!(placed[0].1.y, 150);
}

#[test]
fn free_spaces_push_the_widgets_apart() {
    let entries = [Entry::Widget(100), Entry::Fill, Entry::Widget(100)];
    let placed = stack(AREA, 0, 0, &entries).unwrap();
    // 600 is left, one space takes all of it; nothing is centred.
    assert_eq!(
        placed,
        [
            (0, Rect::new(0, 0, 480, 100)),
            (2, Rect::new(0, 700, 480, 100))
        ]
    );
    let two = [Entry::Fill, Entry::Widget(100), Entry::Fill];
    let placed = stack(AREA, 0, 0, &two).unwrap();
    assert_eq!(placed, [(1, Rect::new(0, 350, 480, 100))]);
}

#[test]
fn the_margins_narrow_the_widgets_and_never_wrap_around() {
    let narrow = Rect::new(5, 7, 30, 200);
    let placed = stack(narrow, 40, 0, &[Entry::Widget(10)]).unwrap();
    assert_eq!(placed[0].1.width, 0);
    let big = Rect::new(u16::MAX, u16::MAX, u16::MAX, u16::MAX);
    let placed = stack(big, u16::MAX, u16::MAX, &[Entry::Widget(u16::MAX)]).unwrap();
    assert_eq!(placed[0].1.x, u16::MAX);
    assert_eq!(placed[0].1.y, u16::MAX);
}

#[test]
fn the_stack_survives_every_refused_allocation() {
    let entries = vec![Entry::Widget(50); 4];
    sweep(|| stack(AREA, 10, 5, &entries));
}

#[test]
fn a_panel_is_inset_all_round() {
    assert_eq!(
        inside(Rect::new(10, 20, 100, 50), 5),
        Rect::new(15, 25, 90, 40)
    );
    assert_eq!(inside(Rect::new(10, 20, 8, 8), 5).width, 0);
}

#[test]
fn the_picture_is_used_as_the_setting_says() {
    assert!(used(ALWAYS, false, false));
    assert!(used(ALWAYS, false, true));
    assert!(used(READING, false, true));
    assert!(!used(READING, false, false));
    assert!(used(ELSEWHERE, false, false));
    assert!(!used(ELSEWHERE, false, true));
    // A value nobody knows is "always".
    assert!(used("", false, false));
    // A page that is guarded is never shown.
    for when in [ALWAYS, READING, ELSEWHERE, ""] {
        assert!(!used(when, true, true));
        assert!(!used(when, true, false));
    }
}
