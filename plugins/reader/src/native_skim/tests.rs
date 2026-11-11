use super::*;
use crate::fail_alloc::sweep;

fn go(step: Step) -> Option<Move> {
    match step {
        Step::Do { go, .. } => go,
        _ => None,
    }
}

#[test]
fn page_and_direction_keys_move_as_beta_3_did() {
    let mut skim = Skim::new(true);
    assert_eq!(go(skim.action("core.prev-page")), Some(Move::Pages(-10)));
    assert_eq!(go(skim.action("core.previous")), Some(Move::Pages(-10)));
    assert_eq!(go(skim.action("core.next-page")), Some(Move::Pages(10)));
    assert_eq!(go(skim.action("core.next")), Some(Move::Pages(10)));
    assert_eq!(go(skim.action("core.left")), Some(Move::Chapters(-1)));
    assert_eq!(go(skim.action("core.right")), Some(Move::Chapters(1)));
    assert_eq!(skim.action("core.home"), Step::Ignored);
    // None of these closes the screen.
    assert!(matches!(
        skim.action("core.right"),
        Step::Do { close: false, .. }
    ));
}

#[test]
fn up_and_down_go_between_the_bar_and_the_buttons_and_round() {
    let mut skim = Skim::new(true);
    assert_eq!(skim.focus(), (Focus::Bar, true));
    for i in 0..4 {
        assert_eq!(skim.action("core.down"), Step::Redraw);
        assert_eq!(skim.button(), Some(i));
    }
    assert_eq!(skim.action("core.down"), Step::Redraw);
    assert_eq!(skim.button(), None);
    assert_eq!(skim.action("core.up"), Step::Redraw);
    assert_eq!(skim.button(), Some(3));
    assert_eq!(skim.action("core.up"), Step::Redraw);
    assert_eq!(skim.button(), Some(2));
    skim.action("core.up");
    skim.action("core.up");
    skim.action("core.up");
    assert_eq!(skim.button(), None);
}

#[test]
fn confirm_presses_the_focused_button_or_stays() {
    let mut skim = Skim::new(true);
    // On the bar it stays where it is.
    assert_eq!(
        skim.action("core.confirm"),
        Step::Do {
            go: None,
            close: true
        }
    );
    skim.action("core.down");
    assert_eq!(
        skim.action("core.confirm"),
        Step::Do {
            go: Some(Move::Chapters(-1)),
            close: false
        }
    );
    skim.action("core.down");
    assert_eq!(
        skim.action("core.confirm"),
        Step::Do {
            go: Some(Move::Back),
            close: true
        }
    );
    skim.action("core.down");
    assert_eq!(
        skim.action("core.confirm"),
        Step::Do {
            go: Some(Move::Chapters(1)),
            close: false
        }
    );
    skim.action("core.down");
    assert_eq!(
        skim.action("core.confirm"),
        Step::Do {
            go: Some(Move::Number),
            close: true
        }
    );
}

#[test]
fn back_returns_and_menu_stays() {
    let mut skim = Skim::new(false);
    assert_eq!(
        skim.action("core.back"),
        Step::Do {
            go: Some(Move::Back),
            close: true
        }
    );
    assert_eq!(
        skim.action("core.menu"),
        Step::Do {
            go: None,
            close: true
        }
    );
}

#[test]
fn a_direction_after_touching_shows_the_focus_but_a_page_key_alone_does_not() {
    let mut skim = Skim::new(false);
    assert_eq!(skim.focus(), (Focus::Bar, false));
    skim.action("core.prev-page");
    assert!(!skim.focus().1);
    skim.action("core.left");
    assert!(skim.focus().1);
    // A tap on a button hides it again and leaves the buttons.
    skim.action("core.down");
    assert_eq!(
        skim.tap_button(1),
        Step::Do {
            go: Some(Move::Back),
            close: true
        }
    );
    assert_eq!(skim.focus(), (Focus::Bar, false));
}

#[test]
fn a_tap_on_the_bar_is_a_place_of_the_book() {
    assert_eq!(permille_at(0, 500), 0);
    assert_eq!(permille_at(250, 500), 500);
    assert_eq!(permille_at(500, 500), 1000);
    assert_eq!(permille_at(900, 500), 1000);
    assert_eq!(permille_at(-30, 500), 0);
    assert_eq!(permille_at(5, 0), 0);
    assert_eq!(go(Skim::tap_bar(125, 500)), Some(Move::Permille(250)));
    assert_eq!(
        Skim::tap_outside(),
        Step::Do {
            go: None,
            close: true
        }
    );
    assert_eq!(done_width(500, 250), 125);
    assert_eq!(done_width(500, 5000), 500);
}

#[test]
fn marks_are_read_and_bounded() {
    assert_eq!(
        parse_marks("10, 250,x,1000, 4000,").unwrap(),
        [10, 250, 1000, 1000]
    );
    assert!(parse_marks("").unwrap().is_empty());
    let many = "5,".repeat(MAX_MARKS + 10);
    assert_eq!(parse_marks(&many).unwrap().len(), MAX_MARKS);
}

#[test]
fn the_place_is_taken_over_clamped() {
    let mut skim = Skim::new(true);
    skim.update("3 / 120", "One", "2500").unwrap();
    assert_eq!(
        (skim.label(), skim.chapter(), skim.permille()),
        ("3 / 120", "One", 1000)
    );
    skim.update("4", "", "x").unwrap();
    assert_eq!(skim.permille(), 0);
    skim.set_marks(parse_marks("100,200").unwrap());
    assert_eq!(skim.marks(), [100, 200]);
}

#[test]
fn allocation_refused_in_turn_is_no_memory() {
    sweep(|| {
        let mut skim = Skim::new(true);
        skim.update("3 / 120", "A chapter", "400")?;
        parse_marks("1,2,3").map(|_| ())
    });
}

#[test]
fn every_button_and_title_has_both_languages() {
    for catalog in [
        include_str!("../../i18n/en.ini"),
        include_str!("../../i18n/de.ini"),
    ] {
        for key in BUTTONS.iter().chain(&["reader.skim"]) {
            assert!(
                catalog
                    .lines()
                    .any(|l| l.starts_with(key) && l[key.len()..].trim_start().starts_with('=')),
                "{key}"
            );
        }
    }
}
