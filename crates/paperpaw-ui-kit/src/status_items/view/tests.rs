//! The view's rows and steps, and refusals of allocation. What needs a drawn
//! frame (taps on rows) is shown with the geometry a drawn frame leaves
//! behind (`draw`): rows of 40 px under a 40 px header with a back arrow; a
//! gesture with no frame drawn hits nothing.
use super::*;
use crate::fail_alloc;
use alloc::string::ToString;
use paperpaw_sdk::{
    Point, Rect,
    native::callbacks::{Input, PointerPhase},
};

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

#[test]
fn refused_allocations_in_touch_steps_leave_rows_and_model_in_step() {
    let mut v = view("clock, wifi, gone, battery");
    draw(&mut v);
    let gestures = [
        (InputGestureKind::LongPress, row_y(0)),
        (InputGestureKind::Tap, row_y(2)),
        (InputGestureKind::Tap, row_y(3)),
        (InputGestureKind::Tap, row_y(3)),
    ];
    fail_alloc::sweep(|| {
        for (kind, y) in gestures {
            let input = Input::Gesture {
                now_ms: 0,
                position: Point::new(50, y),
                kind,
            };
            v.step(&input)?;
        }
        Ok(())
    });
    // Whatever was refused on the way, the rows are those of the model.
    let rows = v.build().unwrap();
    assert_eq!(v.list().items(), rows.as_slice());
    assert!(v.value().unwrap().contains("gone"));
}

/// What a drawn frame leaves: every row 40 px high under the header, and the
/// back arrow in its corner.
fn draw(view: &mut StatusItemsView) {
    let rows = view.list().items().len();
    view.list.hits = (0..rows)
        .map(|row| (Rect::new(0, 40 + 40 * row as u16, 100, 40), row))
        .collect();
    view.list.header.back = Some(Rect::new(0, 0, 40, 40));
}

fn gesture(view: &mut StatusItemsView, kind: InputGestureKind, x: i32, y: i32) -> Step {
    let input = Input::Gesture {
        now_ms: 0,
        position: Point::new(x, y),
        kind,
    };
    view.step(&input).unwrap()
}

/// The centre of row `row` in the drawn frame.
const fn row_y(row: i32) -> i32 {
    60 + 40 * row
}

#[test]
fn a_tap_on_a_drawn_row_shows_or_hides_it() {
    let mut v = view("clock, wifi");
    draw(&mut v);
    // Row 2 is battery, hidden: shown, last.
    assert_eq!(
        gesture(&mut v, InputGestureKind::Tap, 50, row_y(2)),
        Step::Changed("clock, wifi, battery".to_string())
    );
    assert_eq!(v.list().focus(), 2);
    assert_eq!(values(&v)[2], pair("Name of battery", "on (3)"));
    // Row 0 is clock, shown: hidden.
    assert_eq!(
        gesture(&mut v, InputGestureKind::Tap, 50, row_y(0)),
        Step::Changed("wifi, battery".to_string())
    );
    assert_eq!(values(&v)[2].1, "off");
}

#[test]
fn a_hold_picks_up_and_a_tap_puts_down_where_it_is_tapped() {
    let mut v = view("clock, wifi, battery");
    draw(&mut v);
    // A hidden row is not picked up.
    assert_eq!(
        gesture(&mut v, InputGestureKind::LongPress, 50, row_y(3)),
        Step::Ignored
    );
    assert_eq!(
        gesture(&mut v, InputGestureKind::LongPress, 50, row_y(0)),
        Step::Redraw
    );
    assert!(v.chooser().moving());
    assert_eq!(values(&v)[0], pair("Name of clock", "move (1)"));
    assert_eq!(
        gesture(&mut v, InputGestureKind::Tap, 50, row_y(2)),
        Step::Changed("wifi, battery, clock".to_string())
    );
    assert!(!v.chooser().moving());
    assert_eq!(v.list().focus(), 2);
    assert_eq!(values(&v)[2], pair("Name of clock", "on (3)"));
    // Held and put down on the same row: nothing changes.
    gesture(&mut v, InputGestureKind::LongPress, 50, row_y(1));
    assert_eq!(
        gesture(&mut v, InputGestureKind::Tap, 50, row_y(1)),
        Step::Redraw
    );
    assert_eq!(v.value().unwrap(), "wifi, battery, clock");
}

#[test]
fn a_hold_needs_another_shown_item_to_move_past() {
    let mut v = view("clock");
    draw(&mut v);
    assert_eq!(
        gesture(&mut v, InputGestureKind::LongPress, 50, row_y(0)),
        Step::Ignored
    );
    assert!(!v.chooser().moving());
}

#[test]
fn any_tap_off_a_row_puts_the_item_down() {
    for (x, y, step) in [
        // Below the last row.
        (50, 500, Step::Redraw),
        // On a hidden row: put down, and the row is not shown.
        (50, row_y(3), Step::Redraw),
        // The back arrow: put down, and its action runs.
        (10, 10, Step::Header("core.back")),
    ] {
        let mut v = view("clock, wifi, battery");
        draw(&mut v);
        gesture(&mut v, InputGestureKind::LongPress, 50, row_y(0));
        assert!(v.chooser().moving());
        assert_eq!(gesture(&mut v, InputGestureKind::Tap, x, y), step);
        assert!(!v.chooser().moving(), "tap at ({x}, {y})");
        assert_eq!(v.value().unwrap(), "clock, wifi, battery");
        assert_eq!(values(&v)[0], pair("Name of clock", "on (1)"));
    }
}

#[test]
fn what_the_host_refuses_leaves_the_view_as_it_was() {
    let refused = |_: &Step| Err(Error::NoMemory);
    let tap = |row| Input::Gesture {
        now_ms: 0,
        position: Point::new(50, row_y(row)),
        kind: InputGestureKind::Tap,
    };
    let hold = Input::Gesture {
        now_ms: 0,
        position: Point::new(50, row_y(0)),
        kind: InputGestureKind::LongPress,
    };
    // A change: the setting was not set, so the model and the rows keep the
    // old one.
    let mut v = view("clock, wifi");
    draw(&mut v);
    let rows = v.list().items().to_vec();
    assert_eq!(v.run(&tap(2), refused), Err(Error::NoMemory));
    assert_eq!(v.value().unwrap(), "clock, wifi");
    assert_eq!(v.list().items(), rows.as_slice());
    assert_eq!(v.list().focus(), 0);
    // The same input goes through when the host takes it.
    let ok = |_: &Step| Ok(());
    assert_eq!(
        v.run(&tap(2), ok).unwrap(),
        Some(Step::Changed("clock, wifi, battery".to_string()))
    );
    assert_eq!(v.list().focus(), 2);
    // A move: the item stays picked up where it was.
    let mut v = view("clock, wifi, battery");
    draw(&mut v);
    v.run(&hold, ok).unwrap();
    let rows = v.list().items().to_vec();
    assert_eq!(v.run(&tap(2), refused), Err(Error::NoMemory));
    assert!(v.chooser().moving());
    assert_eq!(v.value().unwrap(), "clock, wifi, battery");
    assert_eq!(v.list().items(), rows.as_slice());
    assert_eq!(v.list().focus(), 0);
    // Buttons: a move by the direction button is undone the same way.
    let down = Input::Action {
        now_ms: 0,
        action: "core.down",
    };
    assert_eq!(v.run(&down, refused), Err(Error::NoMemory));
    assert_eq!(v.value().unwrap(), "clock, wifi, battery");
    assert_eq!(v.list().items(), rows.as_slice());
    assert_eq!(v.list().focus(), 0);
    // A header action the host refuses leaves the item picked up.
    assert_eq!(v.run(&back_tap(), refused), Err(Error::NoMemory));
    assert!(v.chooser().moving());
    assert_eq!(v.list().items(), rows.as_slice());
}

fn back_tap() -> Input<'static> {
    Input::Gesture {
        now_ms: 0,
        position: Point::new(10, 10),
        kind: InputGestureKind::Tap,
    }
}

/// What a drawn frame leaves for the grips: the last 40 px of each of the first
/// `shown` rows (rows of 100 px).
fn draw_grips(view: &mut StatusItemsView, shown: usize) {
    view.list.grip_rects = (0..shown)
        .map(|row| (Rect::new(60, 40 + 40 * row as u16, 40, 40), row))
        .collect();
    view.list.set_grips(if shown > 1 { shown } else { 0 });
}

fn pointer(view: &mut StatusItemsView, phase: PointerPhase, contact: u32, y: i32) -> Step {
    let input = Input::Pointer {
        now_ms: 0,
        position: Point::new(80, y),
        phase,
        contact,
    };
    view.step(&input).unwrap()
}

#[test]
fn a_finger_takes_a_shown_item_along_by_its_grip_and_puts_it_where_it_lifts() {
    let mut v = view("clock, wifi, battery");
    draw(&mut v);
    draw_grips(&mut v, 3);
    assert_eq!(
        pointer(&mut v, PointerPhase::Down, 1, row_y(0)),
        Step::Redraw
    );
    assert_eq!(v.list().focus(), 0);
    // Over the third row: the bar moves (a redraw); within it, nothing to draw.
    assert_eq!(
        pointer(&mut v, PointerPhase::Move, 1, row_y(2)),
        Step::Redraw
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Move, 1, row_y(2) + 5),
        Step::Taken
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Up, 1, row_y(2)),
        Step::Changed("wifi, battery, clock".to_string())
    );
    assert_eq!(v.list().focus(), 2);
    assert_eq!(values(&v)[2], pair("Name of clock", "on (3)"));
    // Lifted below the shown ones: the last of them; above: the first.
    assert_eq!(
        pointer(&mut v, PointerPhase::Down, 1, row_y(2)),
        Step::Redraw
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Up, 1, row_y(3) + 30),
        Step::Redraw
    );
    assert_eq!(v.value().unwrap(), "wifi, battery, clock");
    assert_eq!(
        pointer(&mut v, PointerPhase::Down, 1, row_y(2)),
        Step::Redraw
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Up, 1, 0),
        Step::Changed("clock, wifi, battery".to_string())
    );
}

#[test]
fn only_a_contact_on_the_grip_of_a_shown_item_is_taken() {
    let mut v = view("clock, wifi");
    draw(&mut v);
    draw_grips(&mut v, 2);
    // On the row but not on the grip (a tap's business), on a hidden row's
    // place, with nothing taken: not for the view.
    let off_grip = Input::Pointer {
        now_ms: 0,
        position: Point::new(20, row_y(0)),
        phase: PointerPhase::Down,
        contact: 1,
    };
    assert_eq!(v.step(&off_grip).unwrap(), Step::Ignored);
    assert_eq!(
        pointer(&mut v, PointerPhase::Down, 1, row_y(2)),
        Step::Ignored
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Move, 1, row_y(1)),
        Step::Ignored
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Up, 1, row_y(1)),
        Step::Ignored
    );
    // A lone shown item has no other to move past, so no grip.
    let mut one = view("clock");
    draw(&mut one);
    draw_grips(&mut one, 1);
    assert_eq!(
        pointer(&mut one, PointerPhase::Down, 1, row_y(0)),
        Step::Ignored
    );
}

#[test]
fn another_finger_is_not_taken_and_a_cancel_changes_nothing() {
    let mut v = view("clock, wifi, battery");
    draw(&mut v);
    draw_grips(&mut v, 3);
    pointer(&mut v, PointerPhase::Down, 1, row_y(0));
    // Another finger meanwhile: neither its moves nor its end are taken.
    assert_eq!(
        pointer(&mut v, PointerPhase::Move, 2, row_y(2)),
        Step::Ignored
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Up, 2, row_y(2)),
        Step::Ignored
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Move, 1, row_y(1)),
        Step::Redraw
    );
    assert_eq!(
        pointer(&mut v, PointerPhase::Cancel, 1, row_y(1)),
        Step::Redraw
    );
    assert_eq!(v.value().unwrap(), "clock, wifi, battery");
    // The drag is over: its moves are not taken any more.
    assert_eq!(
        pointer(&mut v, PointerPhase::Move, 1, row_y(2)),
        Step::Ignored
    );
}

#[test]
fn a_finger_takes_over_from_the_buttons() {
    let mut v = view("clock, wifi, battery");
    draw(&mut v);
    draw_grips(&mut v, 3);
    act(&mut v, "core.menu");
    assert!(v.chooser().moving());
    pointer(&mut v, PointerPhase::Down, 1, row_y(1));
    assert!(!v.chooser().moving());
    assert_eq!(v.list().focus(), 1);
}

#[test]
fn drags_survive_refused_allocations() {
    let mut v = view("clock, wifi, battery");
    draw(&mut v);
    draw_grips(&mut v, 3);
    fail_alloc::sweep(|| {
        for (phase, y) in [(PointerPhase::Down, row_y(0)), (PointerPhase::Up, row_y(2))] {
            v.step(&Input::Pointer {
                now_ms: 0,
                position: Point::new(80, y),
                phase,
                contact: 1,
            })?;
        }
        Ok(())
    });
    let rows = v.build().unwrap();
    assert_eq!(v.list().items(), rows.as_slice());
}
