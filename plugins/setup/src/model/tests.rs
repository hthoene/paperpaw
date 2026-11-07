use super::*;
use alloc::string::ToString;
use alloc::vec;

fn step(id: &str) -> Step {
    Step {
        id: id.to_string(),
        kind: PageKind::Setting,
        page: id.to_string(),
        title: id.to_string(),
    }
}

fn number(value: i64, min: i64, max: i64, step: i64) -> Setting {
    Setting {
        key: "core.size".to_string(),
        kind: Kind::Int,
        title: String::new(),
        help: String::new(),
        value: Some(Val::Int(value)),
        min,
        max,
        step,
        unit: "core.unit.pt".to_string(),
        choices: Vec::new(),
        editor: false,
    }
}

fn choice(value: &str, options: &[&str]) -> Setting {
    Setting {
        kind: Kind::Choice,
        value: Some(Val::Text(value.to_string())),
        choices: options
            .iter()
            .map(|o| ((*o).to_string(), o.to_uppercase()))
            .collect(),
        ..number(0, 0, 0, 1)
    }
}

#[test]
fn turning_stays_before_the_first_page_and_finishes_past_the_end() {
    assert_eq!(turn(-1, 3), Turn::Stay);
    assert_eq!(turn(0, 3), Turn::To(0));
    // The end page is one past the last step.
    assert_eq!(turn(3, 3), Turn::To(3));
    assert_eq!(turn(4, 3), Turn::Finish);
    assert_eq!(turn(0, 0), Turn::To(0));
    assert_eq!(turn(1, 0), Turn::Finish);
}

#[test]
fn the_focus_goes_around_the_card_and_the_buttons() {
    let mut guide = Guide::new(vec![step("a")], false);
    // The first use shows the focus where it is: on Next.
    guide.move_focus(1);
    assert!(guide.shown);
    assert_eq!(guide.focus, Target::Next);
    guide.move_focus(1);
    assert_eq!(guide.focus, Target::Other);
    guide.move_focus(1);
    assert_eq!(guide.focus, Target::Card);
    guide.move_focus(-1);
    assert_eq!(guide.focus, Target::Other);
}

#[test]
fn the_end_page_has_no_card() {
    let mut guide = Guide::new(vec![step("a")], true);
    guide.go(1);
    assert!(guide.at_end());
    assert!(!guide.has_card());
    assert_eq!(targets(false), [Target::Next, Target::Other]);
    guide.move_focus(1);
    assert_eq!(guide.focus, Target::Other);
    guide.move_focus(1);
    assert_eq!(guide.focus, Target::Next);
}

#[test]
fn going_to_a_page_puts_the_focus_on_next_and_drops_the_setting() {
    let mut guide = Guide::new(vec![step("a"), step("b")], true);
    guide.focus = Target::Card;
    guide.editing = true;
    guide.setting = Some(number(1, 0, 2, 1));
    guide.go(1);
    assert_eq!(guide.focus, Target::Next);
    assert!(!guide.editing);
    assert!(guide.setting.is_none());
}

#[test]
fn a_step_that_comes_or_goes_leaves_the_page_where_its_step_is() {
    let mut guide = Guide::new(vec![step("a"), step("b"), step("c")], true);
    guide.go(1);
    guide.replace_steps(vec![step("x"), step("a"), step("b")]);
    assert_eq!(guide.page, 2);
    // The step shown went: the page number stays, within the list.
    guide.replace_steps(vec![step("a")]);
    assert_eq!(guide.page, 1);
    assert!(guide.at_end());
}

#[test]
fn numbers_move_by_their_step_within_their_range() {
    let size = number(10, 6, 14, 2);
    assert_eq!(size.moved(1), Some(12));
    assert_eq!(size.moved(-1), Some(8));
    assert_eq!(size.moved(100), Some(14));
    assert_eq!(number(14, 6, 14, 2).moved(1), None);
    assert_eq!(number(6, 6, 14, 2).moved(-1), None);
    // A step of 0 moves by 1.
    assert_eq!(number(10, 0, 20, 0).moved(1), Some(11));
    // Not a number.
    assert_eq!(choice("a", &["a", "b"]).moved(1), None);
    // No overflow at the ends of the range of numbers.
    assert_eq!(number(i64::MAX, i64::MIN, i64::MAX, i64::MAX).moved(1), None);
}

#[test]
fn two_options_sit_side_by_side_more_do_not() {
    assert!(choice("a", &["a", "b"]).two().is_some());
    assert!(choice("a", &["a", "b", "c"]).two().is_none());
    assert!(choice("a", &["a"]).two().is_none());
    let mut with_editor = choice("a", &["a", "b"]);
    with_editor.editor = true;
    assert!(with_editor.two().is_none());
}

#[test]
fn the_next_option_goes_around_the_end() {
    assert_eq!(choice("a", &["a", "b", "c"]).next_choice(), Some("b"));
    assert_eq!(choice("c", &["a", "b", "c"]).next_choice(), Some("a"));
    // A value that is not an option: the first.
    assert_eq!(choice("zz", &["a", "b"]).next_choice(), Some("a"));
    assert_eq!(choice("a", &[]).next_choice(), None);
}

#[test]
fn the_name_of_the_value_is_the_options_else_the_value() {
    assert_eq!(choice("b", &["a", "b"]).value_name().as_deref(), Some("B"));
    assert_eq!(choice("q", &["a", "b"]).value_name().as_deref(), Some("q"));
}

#[test]
fn a_size_in_points_has_a_sample_others_do_not() {
    assert_eq!(number(11, 6, 20, 1).sample_points(), Some(11));
    let mut other = number(11, 6, 20, 1);
    other.unit = "core.unit.s".to_string();
    assert_eq!(other.sample_points(), None);
}
