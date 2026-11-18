//! What an action, a tap or a finger does to the lock screen's steps: the cases
//! of the interface-6 `locking.rs` and `lock.rs` that concern input, without a
//! host.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_plugin_lock::native_flow::{Context, Effect, Effects, Flow, Lockstate, Step};
use paperpaw_plugin_lock::native_input::{Act, step_focus};
use paperpaw_plugin_lock::native_model as model;
use paperpaw_plugin_lock::native_pad::{Layout, Room};
use paperpaw_sdk::{Point, Rect, native::callbacks::PointerPhase};

const SALT: &str = "v2.abcdefghijklmnop";

fn layout() -> Layout {
    Layout::new(&Room {
        width: 480,
        height: 800,
        dpi: 220,
        margin: 16,
        gap: 8,
        row: 56,
        header: None,
        clock_lines: (80, 24),
        button: 44,
        touch: true,
    })
}

fn ctx(stored: Option<&str>, now_ms: u64, touch: bool) -> Context<'_> {
    Context {
        stored,
        active: model::kept_kind(stored),
        now_ms,
        touch,
    }
}

fn locked() -> Lockstate {
    Lockstate {
        locked: true,
        ..Lockstate::default()
    }
}

fn drain(fx: &mut Effects) -> Vec<Effect> {
    fx.take().collect()
}

fn centre(rect: Rect) -> Point {
    Point::new(
        i32::from(rect.x) + i32::from(rect.width) / 2,
        i32::from(rect.y) + i32::from(rect.height) / 2,
    )
}

#[test]
fn the_focus_goes_round_by_one_and_stops_at_the_end_by_more() {
    assert_eq!(step_focus(0, -1, 12), 11);
    assert_eq!(step_focus(11, 1, 12), 0);
    assert_eq!(step_focus(10, 3, 12), 11);
    assert_eq!(step_focus(1, -3, 12), 0);
    assert_eq!(step_focus(5, 1, 0), 0);
}

#[test]
fn what_passes_the_lock_and_what_does_not() {
    let stored = model::kept_value("pin", SALT, "1234").unwrap();
    let c = ctx(Some(&stored), 0, true);
    let mut lock = locked();
    let mut flow = Flow::new(true, c.active, false);
    let mut fx = Effects::default();
    for action in [
        "core.sleep",
        "core.refresh-screen",
        "core.toggle-night-mode",
    ] {
        assert_eq!(
            flow.act(action, &mut lock, &c, &mut fx).unwrap(),
            Act::Passed
        );
    }
    // Locked, every other action is the lock's.
    for action in ["core.home", "core.menu", "core.library"] {
        assert_eq!(
            flow.act(action, &mut lock, &c, &mut fx).unwrap(),
            Act::Taken
        );
    }
    // Setting up locks nothing: *Home* and the rest go on.
    let mut setup = Flow::new(false, None, false);
    assert_eq!(
        setup.act("core.home", &mut lock, &c, &mut fx).unwrap(),
        Act::List
    );
    let mut new_pin = Flow::new(false, None, false);
    new_pin.choose_kind("pin", &mut fx);
    drain(&mut fx);
    assert_eq!(
        new_pin.act("core.home", &mut lock, &c, &mut fx).unwrap(),
        Act::Passed
    );
}

#[test]
fn the_buttons_move_the_focus_over_the_keypad_and_confirm_presses_it() {
    let stored = model::kept_value("pin", SALT, "1234").unwrap();
    let c = ctx(Some(&stored), 0, false);
    let mut lock = locked();
    let mut flow = Flow::new(true, c.active, false);
    let mut fx = Effects::default();
    // The first press shows the focus where it is.
    flow.act("core.right", &mut lock, &c, &mut fx).unwrap();
    assert!(flow.shown);
    assert_eq!(flow.focus, 0);
    flow.act("core.right", &mut lock, &c, &mut fx).unwrap();
    flow.act("core.down", &mut lock, &c, &mut fx).unwrap();
    assert_eq!(flow.focus, 4);
    drain(&mut fx);
    flow.act("core.confirm", &mut lock, &c, &mut fx).unwrap();
    assert_eq!(drain(&mut fx), [Effect::Redraw]);
    assert_eq!(flow.pin_len(), 1);
}

#[test]
fn while_the_next_try_waits_nothing_is_taken_but_back() {
    let stored = model::kept_value("pin", SALT, "1234").unwrap();
    let mut lock = locked();
    for _ in 0..model::TRIES {
        lock.attempts.failed(1_000);
    }
    let c = ctx(Some(&stored), 2_000, true);
    let mut flow = Flow::new(true, c.active, false);
    let mut fx = Effects::default();
    let l = layout();
    assert_eq!(
        flow.act("core.confirm", &mut lock, &c, &mut fx).unwrap(),
        Act::Taken
    );
    assert_eq!(
        flow.act("core.right", &mut lock, &c, &mut fx).unwrap(),
        Act::Taken
    );
    let key = centre(l.keys[0]);
    assert_eq!(
        flow.tap(key, (&l, None), &mut lock, &c, &mut fx).unwrap(),
        Act::Taken
    );
    assert!(fx.is_empty());
    assert_eq!(flow.pin_len(), 0, "no digit entered during the wait");
    assert!(
        !flow
            .pointer((PointerPhase::Down, 1, key), &l, &mut lock, &c, &mut fx)
            .unwrap()
    );
    // *Back* works (it takes nothing away here: there is nothing entered).
    assert_eq!(
        flow.act("core.back", &mut lock, &c, &mut fx).unwrap(),
        Act::Taken
    );
    // Over: the next input forgets the tries first, then works.
    let later = ctx(Some(&stored), 1_000 + model::WAIT_MS, true);
    flow.wait_over(&mut lock, &later, &mut fx);
    assert_eq!(drain(&mut fx), [Effect::KeepTries(0), Effect::Redraw]);
    flow.tap(key, (&l, None), &mut lock, &later, &mut fx)
        .unwrap();
    assert_eq!(flow.pin_len(), 1);
}

#[test]
fn a_tap_presses_a_key_asks_for_the_password_or_goes_back() {
    let stored = model::kept_value("pin", SALT, "1234").unwrap();
    let c = ctx(Some(&stored), 0, true);
    let l = layout();
    let mut lock = locked();
    let mut fx = Effects::default();
    let mut flow = Flow::new(true, c.active, true);
    // A key; the focus goes (a tap leaves the buttons out of it).
    flow.tap(centre(l.keys[4]), (&l, None), &mut lock, &c, &mut fx)
        .unwrap();
    assert_eq!(flow.pin_len(), 1);
    assert!(!flow.shown);
    // Off the keys: nothing.
    flow.tap(Point::new(1, 1), (&l, None), &mut lock, &c, &mut fx)
        .unwrap();
    assert_eq!(flow.pin_len(), 1);
    // The back arrow of the header (setting up).
    let mut setup = Flow::new(false, Some("pin"), false);
    let arrow = Rect::new(0, 0, 60, 40);
    setup
        .tap(
            Point::new(10, 10),
            (&l, Some(arrow)),
            &mut lock,
            &c,
            &mut fx,
        )
        .unwrap();
    assert_eq!(drain(&mut fx).last(), Some(&Effect::Back));
    // The password's button.
    let word = model::kept_value("password", SALT, "secret1").unwrap();
    let c = ctx(Some(&word), 0, true);
    let mut flow = Flow::new(true, c.active, false);
    drain(&mut fx);
    flow.tap(centre(l.password), (&l, None), &mut lock, &c, &mut fx)
        .unwrap();
    assert_eq!(drain(&mut fx), [Effect::AskPassword]);
}

#[test]
fn a_finger_draws_the_pattern_and_the_lift_checks_it() {
    let stored = model::kept_value("pattern", SALT, "0124").unwrap();
    let c = ctx(Some(&stored), 0, true);
    let l = layout();
    let mut lock = locked();
    let mut fx = Effects::default();
    let mut flow = Flow::new(true, c.active, false);
    // Not a dot: not the lock's.
    assert!(
        !flow
            .pointer(
                (PointerPhase::Down, 1, Point::new(1, 1)),
                &l,
                &mut lock,
                &c,
                &mut fx
            )
            .unwrap()
    );
    for (i, dot) in [0, 1, 2, 4].into_iter().enumerate() {
        let phase = if i == 0 {
            PointerPhase::Down
        } else {
            PointerPhase::Move
        };
        assert!(
            flow.pointer((phase, 1, l.points[dot]), &l, &mut lock, &c, &mut fx)
                .unwrap()
        );
    }
    drain(&mut fx);
    assert!(
        flow.pointer(
            (PointerPhase::Up, 1, l.points[4]),
            &l,
            &mut lock,
            &c,
            &mut fx
        )
        .unwrap()
    );
    assert_eq!(drain(&mut fx), [Effect::Unlocked]);
    assert!(!lock.locked);
}

#[test]
fn the_kinds_are_listed_and_a_kind_chosen() {
    let c = ctx(None, 0, true);
    let mut lock = Lockstate::default();
    let mut fx = Effects::default();
    let mut flow = Flow::new(false, None, false);
    assert_eq!(flow.step(), Step::Choose);
    assert_eq!(
        flow.act("core.down", &mut lock, &c, &mut fx).unwrap(),
        Act::List
    );
    assert_eq!(
        flow.tap(Point::new(5, 5), (&layout(), None), &mut lock, &c, &mut fx)
            .unwrap(),
        Act::List
    );
}
