#![allow(clippy::unwrap_used, reason = "test assertions")]

use super::*;

#[test]
fn a_focus_step_draws_again_all_the_ring_covers() {
    // Air 12 / 2 = 6 around the target, then a line of at least 2.
    assert_eq!(ring_room_of(12, 1), 8);
    assert_eq!(ring_room_of(12, 3), 9);
    let target = Rect::new(100, 200, 50, 60);
    let area = focus_area(target, ring_room_of(12, 1), (0, 40));
    // The area reaches past the air and the line, measured from the region.
    let (x, y) = (100 - 6 - 2, 200 - 6 - 2 - 40);
    assert_eq!((area.x, area.y), (x, y));
    assert_eq!((area.width, area.height), (50 + 16, 60 + 16));
}

#[test]
fn a_point_is_inside_a_rectangle_by_its_left_and_top_edges() {
    let r = Rect::new(10, 20, 30, 40);
    assert!(inside(r, Point::new(10, 20)));
    assert!(inside(r, Point::new(39, 59)));
    assert!(!inside(r, Point::new(40, 59)));
    assert!(!inside(r, Point::new(39, 60)));
    assert!(!inside(r, Point::new(9, 30)));
}

fn footer() -> Footer {
    Footer {
        pencil: Rect::new(300, 400, 60, 60),
        plus: Rect::new(10, 400, 120, 60),
        actions: [Rect::new(200, 400, 60, 60), Rect::new(240, 400, 60, 60)].into(),
    }
}

fn owners(footer: &Footer, buttons: Buttons) -> Vec<Owner> {
    let mut targets = Vec::new();
    footer_targets(&mut targets, footer, buttons).unwrap();
    targets.iter().map(|t| t.owner).collect()
}

#[test]
fn the_footer_buttons_are_reached_add_first_and_the_pencil_last() {
    let f = footer();
    let both = Buttons {
        editing: true,
        plus: true,
        pencil: true,
    };
    assert_eq!(
        owners(&f, both),
        [
            Owner::Button(Button::Plus),
            Owner::Button(Button::Action(0)),
            Owner::Button(Button::Action(1)),
            Owner::Button(Button::Pencil)
        ]
    );
    let none = Buttons {
        editing: false,
        plus: false,
        pencil: false,
    };
    assert_eq!(owners(&f, none).len(), 2);
    let pencil = Buttons {
        editing: false,
        plus: false,
        pencil: true,
    };
    assert_eq!(
        owners(&f, pencil).last(),
        Some(&Owner::Button(Button::Pencil))
    );
}

#[test]
fn the_pencil_toggles_editing_and_add_opens_its_screen() {
    let pencil = Owner::Button(Button::Pencil);
    assert_eq!(decide(pencil, false), Some(Next::Edit(true)));
    assert_eq!(decide(pencil, true), Some(Next::Edit(false)));
    assert_eq!(decide(Owner::Button(Button::Plus), true), Some(Next::Add));
}

#[test]
fn while_editing_an_item_offers_its_options_and_the_rest_runs_nothing() {
    assert_eq!(decide(Owner::Tool(2), true), Some(Next::Options(2)));
    assert_eq!(
        decide(Owner::Widget { item: 1, target: 3 }, true),
        Some(Next::Options(1))
    );
    assert_eq!(
        decide(Owner::Button(Button::Action(0)), true),
        Some(Next::Nothing)
    );
    // Not editing: it runs (the host decides what).
    for owner in [
        Owner::Tool(0),
        Owner::Widget { item: 0, target: 1 },
        Owner::Button(Button::Action(0)),
    ] {
        assert_eq!(decide(owner, false), None);
    }
}

#[test]
fn a_dashboard_opened_again_keeps_its_page_and_focus() {
    let mut nav = Nav::new(3);
    assert!(nav.turn(true));
    assert!(nav.turn(true));
    let mut dashboard = Dashboard {
        nav: Some(nav),
        ..Dashboard::new()
    };
    dashboard.focus.show(true);
    dashboard.stale = false;
    dashboard.reopened(false);
    assert_eq!(dashboard.nav.map(|n| n.page), Some(2));
    assert!(dashboard.focus.shown());
    assert!(!dashboard.editing());
    // Not laid out again for nothing.
    assert!(!dashboard.stale);
    // Asked to edit: editing starts, the page stays.
    dashboard.reopened(true);
    assert!(dashboard.editing());
    assert!(dashboard.stale);
    assert_eq!(dashboard.nav.map(|n| n.page), Some(2));
}

#[test]
fn the_focus_shows_at_once_only_after_buttons() {
    for (last, shown) in [
        (Some(InputMode::Buttons), true),
        (Some(InputMode::Touch), false),
        (Some(InputMode::Pen), false),
        (None, false),
    ] {
        assert_eq!(focus_at_show(true, last), shown, "{last:?}");
        // The setting off: never at once.
        assert!(!focus_at_show(false, last), "{last:?}");
    }
    let mut dashboard = Dashboard::new();
    dashboard.shown(Some(InputMode::Touch));
    assert!(dashboard.reshow && dashboard.stale);
    assert_eq!(dashboard.last_input, Some(InputMode::Touch));
}

#[test]
fn a_long_press_is_taken_and_runs_nothing() {
    assert_eq!(touch_of(InputGestureKind::LongPress), Touch::Held);
    assert_eq!(touch_of(InputGestureKind::Tap), Touch::Tap);
}

#[test]
fn the_middle_of_a_target_is_where_a_button_press_taps() {
    assert_eq!(middle(Rect::new(10, 20, 30, 40)), Point::new(25, 40));
}
