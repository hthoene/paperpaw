//! The lock screen's steps without a host: the cases of the interface-6
//! `tests/locking.rs` that concern what a code, a key or a dot does.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_plugin_lock::native_flow::{
    Context, Effect, Effects, Flow, Lockstate, Message, PATTERN_OK, Step,
};
use paperpaw_plugin_lock::native_model as model;
use paperpaw_sdk::Point;

const SALT: &str = "v2.abcdefghijklmnop";

fn kept(kind: &str, code: &str) -> String {
    model::kept_value(kind, SALT, code).unwrap()
}

fn room<'a>(stored: Option<&'a str>, now_ms: u64) -> Context<'a> {
    Context {
        stored,
        active: model::kept_kind(stored),
        now_ms,
        touch: true,
    }
}

fn effects(fx: &mut Effects) -> Vec<Effect> {
    fx.take().collect()
}

fn type_pin(
    flow: &mut Flow,
    lock: &mut Lockstate,
    room: &Context<'_>,
    digits: &str,
) -> Vec<Effect> {
    let mut fx = Effects::default();
    for digit in digits.chars() {
        let key = model::KEYS
            .iter()
            .position(|k| k.starts_with(digit))
            .unwrap();
        flow.press(key, lock, room, &mut fx).unwrap();
    }
    // Each digit draws the dots again; the verdict is what *OK* does.
    fx.take().for_each(drop);
    let ok = model::KEYS.iter().position(|k| *k == "OK").unwrap();
    flow.press(ok, lock, room, &mut fx).unwrap();
    effects(&mut fx)
}

#[test]
fn the_right_code_unlocks_and_forgets_the_tries() {
    let stored = kept("pin", "1234");
    let ctx = room(Some(&stored), 0);
    let mut lock = Lockstate {
        locked: true,
        ..Lockstate::default()
    };
    let mut flow = Flow::new(true, ctx.active, false);
    let wrong = type_pin(&mut flow, &mut lock, &ctx, "1111");
    assert_eq!(wrong, [Effect::KeepTries(1), Effect::Redraw]);
    assert_eq!(flow.message(), Message::Wrong);
    let right = type_pin(&mut flow, &mut lock, &ctx, "1234");
    assert_eq!(right, [Effect::KeepTries(0), Effect::Unlocked]);
    assert!(!lock.locked);
    assert_eq!(lock.attempts.failures(), 0);
}

#[test]
fn five_wrong_codes_make_the_next_try_wait_thirty_seconds() {
    let stored = kept("pin", "1234");
    let mut lock = Lockstate {
        locked: true,
        ..Lockstate::default()
    };
    let mut flow = Flow::new(true, Some("pin"), false);
    let mut last = Vec::new();
    for _ in 0..model::TRIES {
        last = type_pin(&mut flow, &mut lock, &room(Some(&stored), 1_000), "9999");
    }
    assert_eq!(
        last,
        [
            Effect::WakeAt(31_000),
            Effect::KeepTries(model::TRIES),
            Effect::Redraw
        ]
    );
    assert!(Flow::waiting(&lock, 30_999));
    assert!(!Flow::waiting(&lock, 31_000));
    let mut fx = Effects::default();
    flow.wait_over(&mut lock, &room(Some(&stored), 31_000), &mut fx);
    assert_eq!(effects(&mut fx), [Effect::KeepTries(0), Effect::Redraw]);
    assert_eq!(flow.message(), Message::None);
    assert_eq!(lock.attempts.failures(), 0);
}

#[test]
fn a_code_removed_elsewhere_releases_the_device() {
    let ctx = room(None, 0);
    let mut lock = Lockstate {
        locked: true,
        ..Lockstate::default()
    };
    let mut flow = Flow::new(true, Some("pin"), false);
    let mut fx = Effects::default();
    assert!(flow.released(&mut lock, &ctx, &mut fx));
    assert_eq!(effects(&mut fx), [Effect::Unlocked]);
    assert!(!lock.locked);
    // Not while setting up, and not while a code is kept.
    let mut flow = Flow::new(false, None, false);
    assert!(!flow.released(&mut lock, &ctx, &mut fx));
}

#[test]
fn setting_up_checks_the_code_in_use_then_lists_the_kinds() {
    let stored = kept("pin", "1234");
    let ctx = room(Some(&stored), 0);
    let mut lock = Lockstate::default();
    let mut flow = Flow::new(false, ctx.active, false);
    assert_eq!(flow.step(), Step::Verify);
    assert_eq!(flow.title_key(), "lock.current-pin");
    let done = type_pin(&mut flow, &mut lock, &ctx, "1234");
    assert_eq!(done, [Effect::Redraw]);
    assert_eq!(flow.step(), Step::Choose);
    assert_eq!(Flow::new(false, None, false).step(), Step::Choose);
}

#[test]
fn a_code_kept_with_an_old_salt_is_kept_anew_when_it_opens_the_lock() {
    let stored = model::kept_value("pin", "legacy", "1234").unwrap();
    let ctx = room(Some(&stored), 0);
    let mut lock = Lockstate {
        locked: true,
        ..Lockstate::default()
    };
    let mut flow = Flow::new(true, ctx.active, false);
    let fx = type_pin(&mut flow, &mut lock, &ctx, "1234");
    assert_eq!(fx, [Effect::Renew("pin", "1234".into()), Effect::Unlocked]);
}

#[test]
fn off_removes_the_code_at_once() {
    let mut flow = Flow::new(false, None, false);
    let mut fx = Effects::default();
    flow.choose_kind("off", &mut fx);
    assert_eq!(
        effects(&mut fx),
        [
            Effect::Forget,
            Effect::SetKind("lock.kind.off"),
            Effect::Notify("lock.removed"),
            Effect::Back
        ]
    );
}

#[test]
fn a_new_pin_is_asked_twice_and_kept_when_equal() {
    let ctx = room(None, 0);
    let mut lock = Lockstate::default();
    let mut flow = Flow::new(false, None, false);
    let mut fx = Effects::default();
    flow.choose_kind("pin", &mut fx);
    assert_eq!(effects(&mut fx), [Effect::Redraw]);
    assert_eq!(flow.title_key(), "lock.new-pin");
    // Too short.
    assert_eq!(type_pin(&mut flow, &mut lock, &ctx, "12"), [Effect::Redraw]);
    assert_eq!(flow.message(), Message::TooShort);
    assert_eq!(flow.step(), Step::New);
    // The first, then a different second one.
    type_pin(&mut flow, &mut lock, &ctx, "1234");
    assert_eq!(
        (flow.step(), flow.title_key()),
        (Step::Confirm, "lock.again")
    );
    type_pin(&mut flow, &mut lock, &ctx, "1235");
    assert_eq!(
        (flow.step(), flow.message()),
        (Step::New, Message::Mismatch)
    );
    // Twice the same.
    type_pin(&mut flow, &mut lock, &ctx, "1234");
    let done = type_pin(&mut flow, &mut lock, &ctx, "1234");
    assert_eq!(
        done,
        [
            Effect::Keep("pin", "1234".into()),
            Effect::SetKind("lock.kind.pin"),
            Effect::Notify("lock.done"),
            Effect::Back
        ]
    );
}

#[test]
fn a_password_is_asked_for_on_the_keyboard_and_typed_twice() {
    let ctx = room(None, 0);
    let mut lock = Lockstate::default();
    let mut flow = Flow::new(false, None, false);
    let mut fx = Effects::default();
    flow.choose_kind("password", &mut fx);
    assert_eq!(effects(&mut fx), [Effect::AskPassword, Effect::Redraw]);
    flow.typed("secret1".into(), &mut lock, &ctx, &mut fx)
        .unwrap();
    assert_eq!(effects(&mut fx), [Effect::AskPassword, Effect::Redraw]);
    assert_eq!(flow.step(), Step::Confirm);
    flow.typed("secret1".into(), &mut lock, &ctx, &mut fx)
        .unwrap();
    assert_eq!(
        effects(&mut fx).first(),
        Some(&Effect::Keep("password", "secret1".into()))
    );
}

#[test]
fn a_pattern_is_drawn_from_dot_to_dot_and_checked_when_the_finger_lifts() {
    let stored = kept("pattern", "0124");
    let ctx = room(Some(&stored), 0);
    let mut lock = Lockstate {
        locked: true,
        ..Lockstate::default()
    };
    let mut flow = Flow::new(true, ctx.active, false);
    let mut fx = Effects::default();
    assert!(
        !flow
            .finger_down(1, Point::new(0, 0), None, &mut fx)
            .unwrap()
    );
    assert!(
        flow.finger_down(1, Point::new(10, 10), Some(0), &mut fx)
            .unwrap()
    );
    for dot in [1, 1, 2, 4] {
        flow.finger_moved(1, Point::new(20, 20), Some(dot), &mut fx)
            .unwrap();
    }
    assert_eq!(flow.pattern().collect::<Vec<_>>(), [0, 1, 2, 4]);
    effects(&mut fx);
    flow.finger_up(&mut lock, &ctx, &mut fx).unwrap();
    assert_eq!(effects(&mut fx), [Effect::Unlocked]);
    assert!(!lock.locked);
}

#[test]
fn a_cancelled_contact_drops_the_pattern() {
    let mut flow = Flow::new(true, Some("pattern"), false);
    let mut fx = Effects::default();
    flow.finger_down(1, Point::new(1, 1), Some(3), &mut fx)
        .unwrap();
    effects(&mut fx);
    flow.finger_cancelled(&mut fx);
    assert_eq!(effects(&mut fx), [Effect::Redraw]);
    assert_eq!(flow.pattern().count(), 0);
    assert!(flow.finger().is_none());
}

#[test]
fn without_touch_the_pattern_is_chosen_dot_by_dot_with_the_buttons() {
    let flow = Flow::new(true, Some("pattern"), true);
    assert_eq!(flow.targets(true), 9);
    assert_eq!(flow.targets(false), 10);
    assert_eq!(PATTERN_OK, 9);
    let kinds = |touch| Flow::kinds(touch).collect::<Vec<_>>();
    assert_eq!(kinds(true), ["off", "pin", "pattern", "password"]);
    assert_eq!(kinds(false), ["off", "pin", "password"]);
}

#[test]
fn back_closes_the_setup_and_takes_a_digit_away_while_unlocking() {
    let ctx = room(None, 0);
    let mut lock = Lockstate::default();
    let mut fx = Effects::default();
    let mut setup = Flow::new(false, None, false);
    setup.back(&mut fx);
    assert_eq!(effects(&mut fx), [Effect::Back]);
    let mut flow = Flow::new(true, Some("pin"), false);
    flow.back(&mut fx);
    assert!(fx.is_empty(), "nothing to take away");
    flow.press(0, &mut lock, &ctx, &mut fx).unwrap();
    effects(&mut fx);
    assert_eq!(flow.pin_len(), 1);
    flow.back(&mut fx);
    assert_eq!(effects(&mut fx), [Effect::Redraw]);
    assert_eq!(flow.pin_len(), 0);
}
