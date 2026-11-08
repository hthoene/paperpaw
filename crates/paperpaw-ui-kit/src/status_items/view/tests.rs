//! The view's rows and steps, and refusals of allocation. What needs a drawn
//! frame (taps on rows) is the host's to show; here a gesture with no frame
//! drawn hits nothing.
use super::*;
use crate::fail_alloc;
use alloc::string::ToString;
use paperpaw_sdk::{Point, native::callbacks::Input};

fn texts() -> Texts {
    Texts {
        on: "on".to_string(),
        off: "off".to_string(),
        moving: "move".to_string(),
        hint: "hint".to_string(),
        moving_hint: "moving".to_string(),
    }
}

fn entries() -> Vec<Entry> {
    ["clock", "wifi", "battery", "lock"]
        .iter()
        .map(|id| Entry {
            id: (*id).to_string(),
            title: alloc::format!("Name of {id}"),
            also: None,
        })
        .collect()
}

fn view(setting: &str) -> StatusItemsView {
    StatusItemsView::new(entries(), setting, "Bar".to_string(), texts()).unwrap()
}

fn act(view: &mut StatusItemsView, action: &str) -> Step {
    view.step(&Input::Action { now_ms: 0, action }).unwrap()
}

fn values(view: &StatusItemsView) -> Vec<(String, String)> {
    view.list()
        .items()
        .iter()
        .map(|item| match item {
            Item::Row(row) => (row.title.clone(), row.value.clone()),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

#[test]
fn shown_items_come_first_with_their_places() {
    let v = view("wifi, clock");
    assert_eq!(
        values(&v),
        [
            pair("Name of wifi", "on (1)"),
            pair("Name of clock", "on (2)"),
            pair("Name of battery", "off"),
            pair("Name of lock", "off"),
        ]
    );
    // One shown: no place.
    assert_eq!(values(&view("lock"))[0], pair("Name of lock", "on"));
    assert_eq!(values(&view(""))[0], pair("Name of clock", "off"));
}

#[test]
fn confirm_shows_and_hides_and_the_focus_follows_the_buttons() {
    let mut v = view("clock, wifi");
    assert_eq!(act(&mut v, "core.down"), Step::Redraw);
    assert_eq!(act(&mut v, "core.down"), Step::Redraw);
    assert_eq!(v.list().focus(), 2);
    // Battery is shown: last.
    assert_eq!(
        act(&mut v, "core.confirm"),
        Step::Changed("clock, wifi, battery".to_string())
    );
    assert_eq!(values(&v)[2], pair("Name of battery", "on (3)"));
    // Back is not the view's: the screen goes back.
    assert_eq!(act(&mut v, "core.back"), Step::Ignored);
    // Hide the first.
    act(&mut v, "core.up");
    act(&mut v, "core.up");
    assert_eq!(
        act(&mut v, "core.confirm"),
        Step::Changed("wifi, battery".to_string())
    );
    assert_eq!(values(&v)[2].1, "off");
}

#[test]
fn menu_picks_up_a_shown_item_and_the_buttons_move_it() {
    let mut v = view("clock, wifi, battery");
    // A hidden item cannot be picked up.
    act(&mut v, "core.down");
    act(&mut v, "core.down");
    act(&mut v, "core.down");
    assert_eq!(act(&mut v, "core.menu"), Step::Ignored);
    for _ in 0..3 {
        act(&mut v, "core.up");
    }
    assert_eq!(v.list().focus(), 0);
    assert_eq!(act(&mut v, "core.menu"), Step::Redraw);
    assert_eq!(values(&v)[0], pair("Name of clock", "move (1)"));
    assert_eq!(
        act(&mut v, "core.down"),
        Step::Changed("wifi, clock, battery".to_string())
    );
    assert_eq!(v.list().focus(), 1);
    assert_eq!(values(&v)[1], pair("Name of clock", "move (2)"));
    assert_eq!(act(&mut v, "core.confirm"), Step::Redraw);
    assert!(!v.chooser().moving());
    assert_eq!(values(&v)[1], pair("Name of clock", "on (2)"));
    assert_eq!(v.value().unwrap(), "wifi, clock, battery");
}

#[test]
fn what_is_not_the_views_is_ignored() {
    let mut v = view("clock");
    assert_eq!(
        act(&mut v, "core.menu"),
        Step::Ignored,
        "one shown: nothing to move past"
    );
    assert_eq!(act(&mut v, "core.home"), Step::Ignored);
    // A tap or a hold with no frame drawn hits nothing.
    for kind in [InputGestureKind::Tap, InputGestureKind::LongPress] {
        let input = Input::Gesture {
            now_ms: 0,
            position: Point::new(10, 10),
            kind,
        };
        assert_eq!(v.step(&input).unwrap(), Step::Ignored);
    }
    let key = Input::Key {
        now_ms: 0,
        key: "a",
        state: paperpaw_sdk::native::callbacks::KeyState::Press,
    };
    assert_eq!(v.step(&key).unwrap(), Step::Ignored);
}

#[test]
fn refused_allocations_are_errors_and_leave_a_valid_view() {
    let mut v = view("clock, wifi, gone, battery");
    fail_alloc::sweep(|| {
        v.step(&Input::Action {
            now_ms: 0,
            action: "core.down",
        })?;
        v.step(&Input::Action {
            now_ms: 0,
            action: "core.menu",
        })?;
        v.step(&Input::Action {
            now_ms: 0,
            action: "core.back",
        })?;
        v.step(&Input::Action {
            now_ms: 0,
            action: "core.up",
        })?;
        v.value()?;
        Ok(())
    });
    // The rows still stand for every entry once, and the unknown id is kept.
    v.chooser.put_down();
    assert_eq!(v.list().items().len(), 4);
    assert!(v.value().unwrap().contains("gone"));
}
