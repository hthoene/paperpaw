//! The guard and lock rules of `docs/abi/ui.md` sections 1.2, 1.3, 3.1, 4.3,
//! each with the refusal and the case just inside it.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate alloc;
use super::*;
use alloc::vec::Vec;

const CORE: u64 = 0;
const READER: u64 = 1;
const LOCK: u64 = 2;
const KEYBOARD: u64 = 3;
const MENU: u64 = 4;
const MAIL: u64 = 5;
const HELP: u64 = 6;
const A: u64 = 7;
const B: u64 = 8;
const LOCK_A: u64 = 9;
const LOCK_B: u64 = 10;

fn row(instance: u32, owner: u64) -> Row {
    Row {
        instance,
        owner,
        overlay: false,
        guards: false,
        secret: false,
        host: false,
        guard_dialog: false,
    }
}

/// page(1, reader) < lock(2, lock, guards) < keyboard(3, keyboard, dialog)
fn locked() -> Vec<Row> {
    let mut lock = row(2, LOCK);
    lock.guards = true;
    let mut keyboard = row(3, KEYBOARD);
    keyboard.guard_dialog = true;
    alloc::vec![row(1, READER), lock, keyboard]
}

fn plain() -> Vec<Row> {
    alloc::vec![row(1, READER), row(2, MENU)]
}

impl UiStackView for Vec<Row> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
    fn row(&self, index: usize) -> Option<Row> {
        self.get(index).copied()
    }
}

fn view(rows: &Vec<Row>) -> &dyn UiStackView {
    rows
}

#[test]
fn a_slice_of_rows_is_a_stack_view() {
    let rows = locked();
    let slice = Rows(&rows);
    assert_eq!(UiStackView::len(&slice), 3);
    assert_eq!(slice.row(1).map(|r| r.owner), Some(LOCK));
    assert!(slice.row(3).is_none());
    assert_eq!(guard_owner(&slice), Some(LOCK));
}

#[test]
fn the_topmost_guard_and_secrecy_follow_the_flags_and_the_position() {
    let rows = locked();
    let v = view(&rows);
    assert_eq!(topmost_guard(v), Some(1));
    assert_eq!(guard_owner(v), Some(LOCK));
    // Not secret below the guard; the guard and what lies above it are.
    assert!(!is_secret(v, 0));
    assert!(is_secret(v, 1));
    assert!(is_secret(v, 2));
    assert!(top_is_secret(v));
    assert!(is_secret(v, 3), "missing rows fail closed");
    let rows = plain();
    assert_eq!(topmost_guard(view(&rows)), None);
    assert!(!top_is_secret(view(&rows)));
    let mut rows = plain();
    rows[1].secret = true;
    assert!(top_is_secret(view(&rows)));
    let empty: Vec<Row> = Vec::new();
    assert_eq!(topmost_guard(view(&empty)), None);
    assert!(!top_is_secret(view(&empty)));
}

#[test]
fn only_the_top_instance_gets_input_and_the_lock_owns_the_path() {
    let rows = locked();
    let v = view(&rows);
    assert_eq!(input_target(v), Some(3));
    for instance in [1, 2, 4, 0] {
        assert!(!may_receive_input(v, instance), "instance {instance}");
    }
    assert!(may_receive_input(v, 3));
    // An open by another plugin cannot take the top: it waits below the guard.
    assert_eq!(open_index(v, MAIL, false, false), 1);
    // The guard's plugin, its authorized dialog and the host's sleep screen go on top.
    assert_eq!(open_index(v, LOCK, false, false), 3);
    assert_eq!(open_index(v, KEYBOARD, true, false), 3);
    assert_eq!(open_index(v, CORE, false, true), 3);
    // Without a guard every open goes on top.
    let rows = plain();
    assert_eq!(open_index(view(&rows), MAIL, false, false), 2);
    let empty: Vec<Row> = Vec::new();
    assert_eq!(open_index(view(&empty), MAIL, false, false), 0);
    assert_eq!(input_target(view(&empty)), None);
}

#[test]
fn a_guard_turns_on_only_for_the_own_top_instance_not_counting_the_hosts_screen() {
    let mut rows = plain();
    // Own top: allowed.
    assert_eq!(guard_on_allowed(view(&rows), MENU, 2), Ok(()));
    // Own but not top, another plugin's instance, unknown instance.
    assert_eq!(guard_on_allowed(view(&rows), READER, 1), Err(ERR_STATE));
    assert_eq!(guard_on_allowed(view(&rows), READER, 2), Err(ERR_DENIED));
    assert_eq!(guard_on_allowed(view(&rows), MENU, 9), Err(ERR_NOT_FOUND));
    // The host's sleep screen above does not count: a lock locking while the
    // sleep screen shows guards at once.
    let mut sleep = row(3, CORE);
    sleep.host = true;
    rows.push(sleep);
    assert_eq!(guard_on_allowed(view(&rows), MENU, 2), Ok(()));
    // A plugin's own screen above does.
    rows.push(row(4, MAIL));
    assert_eq!(guard_on_allowed(view(&rows), MENU, 2), Err(ERR_STATE));
    let empty: Vec<Row> = Vec::new();
    assert_eq!(guard_on_allowed(view(&empty), MENU, 1), Err(ERR_NOT_FOUND));
}

#[test]
fn only_the_guards_plugin_raises_closes_or_goes_back_past_the_lock() {
    let rows = locked();
    let v = view(&rows);
    assert_eq!(raise_allowed(v, MAIL), Err(ERR_DENIED));
    assert_eq!(raise_allowed(v, LOCK), Ok(()));
    assert_eq!(raise_allowed(view(&plain()), MAIL), Ok(()));
    // Close: the guard itself and what is above it.
    assert_eq!(close_allowed(v, MAIL, 2), Err(ERR_DENIED));
    assert_eq!(close_allowed(v, MAIL, 3), Err(ERR_DENIED));
    assert_eq!(close_allowed(v, LOCK, 2), Ok(Close::Instance(1)));
    assert_eq!(close_allowed(v, LOCK, 3), Err(ERR_DENIED), "not its own");
    // A guard dialog closes itself (0 = own top), nobody else's.
    assert_eq!(close_allowed(v, KEYBOARD, 0), Ok(Close::Instance(2)));
    assert_eq!(close_allowed(v, KEYBOARD, 3), Ok(Close::Instance(2)));
    assert_eq!(close_allowed(v, KEYBOARD, 2), Err(ERR_DENIED));
    // The page below the guard is another plugin's: not closed by a stranger.
    assert_eq!(close_allowed(v, MAIL, 1), Err(ERR_DENIED));
    // Back to: nothing to close is fine, closing anything past the lock is not.
    assert_eq!(back_to_allowed(v, MAIL, 2), Ok(()));
    assert_eq!(back_to_allowed(v, MAIL, 0), Err(ERR_DENIED));
    assert_eq!(back_to_allowed(v, MAIL, 1), Err(ERR_DENIED));
    assert_eq!(back_to_allowed(v, LOCK, 0), Ok(()));
    assert_eq!(back_to_allowed(view(&plain()), MAIL, 0), Ok(()));
    // A target off the stack is refused before any arithmetic, also usize::MAX.
    for target in [3, 4, usize::MAX - 1, usize::MAX] {
        assert_eq!(back_to_allowed(v, LOCK, target), Err(ERR_NOT_FOUND));
        assert_eq!(back_to_allowed(v, MAIL, target), Err(ERR_NOT_FOUND));
    }
    let empty: Vec<Row> = Vec::new();
    assert_eq!(back_to_allowed(view(&empty), LOCK, 0), Err(ERR_NOT_FOUND));
}

#[test]
fn close_follows_the_navigation_rules_without_a_guard() {
    let mut rows = plain();
    rows[0].overlay = false;
    let v = view(&rows);
    // 0 is the own top instance; outside it ERR_STATE.
    assert_eq!(close_allowed(v, MENU, 0), Ok(Close::Instance(1)));
    assert_eq!(close_allowed(v, READER, 0), Err(ERR_STATE));
    // A non-zero instance: own, wherever it is; unknown is not found.
    assert_eq!(close_allowed(v, READER, 1), Ok(Close::NoEffect), "root");
    assert_eq!(close_allowed(v, MENU, 7), Err(ERR_NOT_FOUND));
    // Another plugin's instance only when it is an overlay.
    assert_eq!(close_allowed(v, HELP, 2), Err(ERR_DENIED));
    rows[1].overlay = true;
    assert_eq!(close_allowed(view(&rows), HELP, 2), Ok(Close::Instance(1)));
    let empty: Vec<Row> = Vec::new();
    assert_eq!(close_allowed(view(&empty), MENU, 0), Err(ERR_STATE));
}

#[test]
fn input_filters_and_key_bindings_stay_inside_the_lock() {
    let rows = locked();
    let v = view(&rows);
    // The top is the guard dialog (secret or not): its plugin's filter and
    // the guard's run, no others.
    assert!(filter_runs(v, LOCK));
    assert!(filter_runs(v, KEYBOARD), "non-secret guard question");
    assert!(!filter_runs(v, MAIL));
    assert!(!filter_runs(v, READER));
    let mut secret = locked();
    secret[2].secret = true;
    assert!(
        filter_runs(view(&secret), KEYBOARD),
        "secret guard question"
    );
    assert!(filter_runs(view(&secret), LOCK));
    assert!(!filter_runs(view(&secret), MAIL));
    // A row of the keyboard's plugin above the guard that is not the guard's
    // dialog gets no filter call.
    let mut other = locked();
    other[2].guard_dialog = false;
    assert!(!filter_runs(view(&other), KEYBOARD));
    assert!(filter_runs(view(&other), LOCK));
    // No guard and nothing secret: every filter runs.
    assert!(filter_runs(view(&plain()), MAIL));
    // A secret screen without a guard (a password field): own plugin's only.
    let mut rows = plain();
    rows[1].secret = true;
    assert!(filter_runs(view(&rows), MENU));
    assert!(!filter_runs(view(&rows), MAIL));
    // Key maps: another plugin's action or input does nothing under the lock.
    assert!(binding_runs(v, None), "host action");
    assert!(binding_runs(v, Some(LOCK)));
    assert!(!binding_runs(v, Some(MAIL)));
    assert!(binding_runs(view(&plain()), Some(MAIL)));
}

#[test]
fn the_stack_other_plugins_see_is_the_guard_alone() {
    let rows = locked();
    let v = view(&rows);
    assert_eq!(
        visible_rows(v, MAIL),
        Visible {
            start: 1,
            end: 2,
            left_out: true
        }
    );
    assert_eq!(
        visible_rows(v, LOCK),
        Visible {
            start: 1,
            end: 3,
            left_out: true
        }
    );
    assert_eq!(
        visible_rows(view(&plain()), MAIL),
        Visible {
            start: 0,
            end: 2,
            left_out: false
        }
    );
    // A guard at the root: the guard's plugin leaves nothing out.
    let mut root = alloc::vec![row(1, LOCK)];
    root[0].guards = true;
    assert_eq!(
        visible_rows(view(&root), LOCK),
        Visible {
            start: 0,
            end: 1,
            left_out: false
        }
    );
    assert_eq!(
        visible_rows(view(&root), MAIL),
        Visible {
            start: 0,
            end: 1,
            left_out: false
        }
    );
}

#[test]
fn an_overlay_of_another_plugin_never_draws_the_secret_or_the_locked() {
    // page < lock(guards) ; sleep overlay of `core` over them.
    let rows = locked();
    let v = view(&rows);
    assert!(!drawn_under_overlay(v, 2, CORE), "above the guard: secret");
    assert!(
        !drawn_under_overlay(v, 0, CORE),
        "the page, below the guard"
    );
    assert!(!drawn_under_overlay(v, 1, CORE));
    // Its own plugin's overlay shows it.
    assert!(drawn_under_overlay(v, 1, LOCK));
    // Without a guard the page is drawn under the sleep screen, unless secret.
    let mut rows = plain();
    assert!(drawn_under_overlay(view(&rows), 0, CORE));
    rows[0].secret = true;
    assert!(!drawn_under_overlay(view(&rows), 0, CORE));
    assert!(!drawn_under_overlay(view(&rows), 9, CORE));
}

fn ask<'a>(asker: u64, kind: u32, slot: &'a str) -> Ask<'a> {
    Ask {
        asker,
        kind,
        slot,
        secret: false,
        parent: None,
    }
}

fn nested(secret: bool) -> Option<Parent> {
    Some(Parent {
        secret,
        nested: false,
        has_nested: false,
    })
}

#[test]
fn questions_have_a_valid_kind_and_slot() {
    let rows = plain();
    let v = view(&rows);
    let open = OpenRequests::default();
    for kind in 0..=3 {
        assert!(ask_admission(v, &ask(MAIL, kind, ""), open, false).is_ok());
    }
    for kind in [4, 5, u32::MAX] {
        assert_eq!(
            ask_admission(v, &ask(MAIL, kind, ""), open, false),
            Err(ERR_INVALID)
        );
    }
    // The phone keyboard slot is for text questions only; any other slot is invalid.
    assert!(ask_admission(v, &ask(MAIL, 0, REMOTE_KEYBOARD), open, false).is_ok());
    for kind in 1..=3 {
        assert_eq!(
            ask_admission(v, &ask(MAIL, kind, REMOTE_KEYBOARD), open, false),
            Err(ERR_INVALID)
        );
    }
    for slot in ["core.keyboard", "x", "core.remote-keyboard "] {
        assert_eq!(
            ask_admission(v, &ask(MAIL, 0, slot), open, false),
            Err(ERR_INVALID)
        );
    }
}

#[test]
fn requests_are_capped_at_four_and_one_on_the_x4_asking_again_replaces() {
    let rows = plain();
    let v = view(&rows);
    let open = |total, asker_has_one| OpenRequests {
        total,
        asker_has_one,
    };
    // Limit and limit + 1.
    assert!(ask_admission(v, &ask(A, 0, ""), open(3, false), false).is_ok());
    assert_eq!(
        ask_admission(v, &ask(A, 0, ""), open(4, false), false),
        Err(ERR_FULL)
    );
    assert_eq!(
        ask_admission(v, &ask(A, 0, ""), open(200, false), false),
        Err(ERR_FULL)
    );
    // X4: one.
    assert!(ask_admission(v, &ask(A, 0, ""), open(0, false), true).is_ok());
    assert_eq!(
        ask_admission(v, &ask(A, 0, ""), open(1, false), true),
        Err(ERR_FULL)
    );
    // The asker's own open request is cancelled by asking again, so it
    // makes room even on a full device.
    let again = ask_admission(v, &ask(A, 0, ""), open(1, true), true).unwrap();
    assert!(again.cancels_asker_request);
    let again = ask_admission(v, &ask(A, 0, ""), open(4, true), false).unwrap();
    assert!(again.cancels_asker_request);
    // Another asker on the same full device is refused.
    assert_eq!(
        ask_admission(v, &ask(B, 0, ""), open(4, false), false),
        Err(ERR_FULL)
    );
}

#[test]
fn the_nested_phone_question_is_one_per_dialog_and_one_plus_one_on_the_x4() {
    let rows = plain();
    let v = view(&rows);
    let mut phone = ask(KEYBOARD, 0, REMOTE_KEYBOARD);
    phone.parent = nested(false);
    // Allowed beside the full request limit, also on the X4 (1 + 1).
    for (total, x4) in [(1, true), (4, false)] {
        let open = OpenRequests {
            total,
            asker_has_one: false,
        };
        let got = ask_admission(v, &phone, open, x4).unwrap();
        assert!(!got.cancels_asker_request && !got.below_guard);
    }
    // A second nested question of the same parent, or one asked from a nested one.
    let mut second = phone;
    second.parent = Some(Parent {
        secret: false,
        nested: false,
        has_nested: true,
    });
    assert_eq!(
        ask_admission(v, &second, OpenRequests::default(), true),
        Err(ERR_FULL)
    );
    second.parent = Some(Parent {
        secret: false,
        nested: true,
        has_nested: false,
    });
    assert_eq!(
        ask_admission(v, &second, OpenRequests::default(), false),
        Err(ERR_FULL)
    );
    // Only kind 0 on the phone slot may be nested.
    for (kind, slot) in [(0, ""), (1, ""), (2, ""), (3, "")] {
        let mut other = ask(KEYBOARD, kind, slot);
        other.parent = nested(false);
        assert_eq!(
            ask_admission(v, &other, OpenRequests::default(), false),
            Err(ERR_DENIED),
            "kind {kind}"
        );
    }
    // It inherits the parent's secret.
    let mut secret = phone;
    secret.parent = nested(true);
    assert!(
        ask_admission(v, &secret, OpenRequests::default(), false)
            .unwrap()
            .secret
    );
    let mut flagged = phone;
    flagged.secret = true;
    assert!(
        ask_admission(v, &flagged, OpenRequests::default(), false)
            .unwrap()
            .secret
    );
    assert!(
        !ask_admission(v, &phone, OpenRequests::default(), false)
            .unwrap()
            .secret
    );
}

#[test]
fn phone_typing_is_refused_while_anything_guards() {
    let rows = locked();
    let v = view(&rows);
    // The lock's own PIN dialog asks to type on the phone: refused for 1.0.
    let mut phone = ask(KEYBOARD, 0, REMOTE_KEYBOARD);
    phone.parent = nested(true);
    for x4 in [false, true] {
        assert_eq!(
            ask_admission(v, &phone, OpenRequests::default(), x4),
            Err(ERR_DENIED)
        );
    }
    // The same question with the guard gone is fine.
    let rows = plain();
    assert!(ask_admission(view(&rows), &phone, OpenRequests::default(), false).is_ok());
    // A plain question under the lock: the lock's own opens above, another
    // plugin's waits below the guard.
    let rows = locked();
    let v = view(&rows);
    let own = ask_admission(v, &ask(LOCK, 0, ""), OpenRequests::default(), false).unwrap();
    assert!(!own.below_guard);
    let other = ask_admission(v, &ask(MAIL, 0, ""), OpenRequests::default(), false).unwrap();
    assert!(other.below_guard);
}

#[test]
fn the_hosts_sleep_screen_above_the_guard_gets_no_input() {
    // page < lock(guards) < core.sleep (host): the lock keeps the input.
    let mut sleep = row(9, CORE);
    sleep.host = true;
    sleep.overlay = true;
    let mut rows = locked();
    rows.push(sleep);
    let v = view(&rows);
    assert_eq!(
        input_target(v),
        Some(3),
        "the keyboard, not the sleep screen"
    );
    assert!(may_receive_input(v, 3));
    assert!(!may_receive_input(v, 9));
    // The lock alone with the sleep screen over it: the lock gets the input.
    let mut lock = row(2, LOCK);
    lock.guards = true;
    let mut rows = alloc::vec![row(1, READER), lock];
    let mut sleep = row(9, CORE);
    sleep.host = true;
    rows.push(sleep);
    assert_eq!(input_target(view(&rows)), Some(2));
    assert!(top_is_secret(view(&rows)), "the lock is what input reaches");
    assert!(filter_runs(view(&rows), LOCK));
    assert!(!filter_runs(view(&rows), CORE));
    assert!(!filter_runs(view(&rows), MAIL));
    // Without a guard the sleep screen is the top and gets input.
    let mut sleep = row(9, CORE);
    sleep.host = true;
    let rows = alloc::vec![row(1, READER), sleep];
    assert_eq!(input_target(view(&rows)), Some(9));
}

#[test]
fn a_lower_guarding_overlay_is_not_closable_by_an_unrelated_plugin() {
    // page < lockA(guards, overlay) < menu(overlay) < lockB(guards): two guards.
    let mut a = row(2, LOCK_A);
    a.guards = true;
    a.overlay = true;
    let mut menu = row(3, MENU);
    menu.overlay = true;
    let mut b = row(4, LOCK_B);
    b.guards = true;
    let rows = alloc::vec![row(1, READER), a, menu, b];
    let v = view(&rows);
    assert_eq!(topmost_guard(v), Some(3));
    // An unrelated plugin closes neither guard, though the lower one is an
    // overlay below the topmost guard.
    for instance in [2, 4] {
        assert_eq!(
            close_allowed(v, MAIL, instance),
            Err(ERR_DENIED),
            "{instance}"
        );
    }
    // Nor does the other guard's plugin close the lower guard through the overlay rule,
    // but the topmost guard's plugin may, and a guard its own.
    assert_eq!(close_allowed(v, LOCK_B, 2), Ok(Close::Instance(1)));
    assert_eq!(close_allowed(v, LOCK_A, 2), Ok(Close::Instance(1)));
    assert_eq!(close_allowed(v, LOCK_B, 4), Ok(Close::Instance(3)));
    // A non-guarding overlay below the topmost guard follows the ordinary rule.
    assert_eq!(close_allowed(v, HELP, 3), Ok(Close::Instance(2)));
}

#[test]
fn a_guard_dialog_opens_nothing_but_its_nested_question() {
    let rows = locked();
    let v = view(&rows);
    // The keyboard dialog (instance 3) cannot open a screen; the lock (2) and
    // the page (1), an action or event (None) and an unknown instance can.
    assert_eq!(open_allowed(v, Some(3)), Err(ERR_DENIED));
    for from in [Some(1), Some(2), None, Some(99)] {
        assert_eq!(open_allowed(v, from), Ok(()), "{from:?}");
    }
    // Its one nested phone question is the op 8 path, which the guard denies too.
    let mut phone = ask(KEYBOARD, 0, REMOTE_KEYBOARD);
    phone.parent = nested(true);
    assert_eq!(
        ask_admission(v, &phone, OpenRequests::default(), false),
        Err(ERR_DENIED)
    );
}

#[test]
fn full_width_owner_keys_never_alias_their_low_bits() {
    let owner = u64::MAX;
    let narrowed = u64::from(u16::MAX);
    let mut guard = row(1, owner);
    guard.guards = true;
    let rows = alloc::vec![guard];
    let v = view(&rows);
    assert_eq!(guard_owner(v), Some(owner));
    assert!(filter_runs(v, owner));
    assert!(!filter_runs(v, narrowed));
    assert_eq!(raise_allowed(v, owner), Ok(()));
    assert_eq!(raise_allowed(v, narrowed), Err(ERR_DENIED));
    assert_eq!(open_index(v, owner, false, false), 1);
    assert_eq!(open_index(v, narrowed, false, false), 0);
}

#[test]
fn a_plugin_row_with_owner_zero_fails_closed() {
    let rows = alloc::vec![row(1, READER), row(2, 0)];
    let v = view(&rows);
    assert_eq!(topmost_guard(v), Some(1));
    assert_eq!(guard_owner(v), Some(0));
    assert!(is_secret(v, 0));
    assert!(top_is_secret(v));
    assert_eq!(input_target(v), None);
    assert!(!filter_runs(v, READER));
    assert!(!filter_runs(v, 0));
    assert!(!binding_runs(v, None));
    assert_eq!(raise_allowed(v, READER), Err(ERR_STATE));
    assert_eq!(open_allowed(v, None), Err(ERR_STATE));
    assert_eq!(guard_on_allowed(v, READER, 1), Err(ERR_STATE));
    assert_eq!(close_allowed(v, READER, 1), Err(ERR_STATE));
    assert_eq!(back_to_allowed(v, READER, 0), Err(ERR_STATE));
    assert_eq!(
        visible_rows(v, READER),
        Visible {
            start: 0,
            end: 0,
            left_out: true
        }
    );
    assert!(!drawn_under_overlay(v, 0, READER));
    assert_eq!(
        ask_admission(v, &ask(READER, 0, ""), OpenRequests::default(), false),
        Err(ERR_STATE)
    );
}

#[test]
fn owner_zero_is_valid_only_for_host_screens_and_never_a_plugin_caller() {
    let mut sleep = row(1, 0);
    sleep.host = true;
    let rows = alloc::vec![sleep];
    let v = view(&rows);
    assert_eq!(input_target(v), Some(1));
    assert!(!is_secret(v, 0));
    assert!(!filter_runs(v, 0));
    assert!(!binding_runs(v, Some(0)));
    assert_eq!(raise_allowed(v, 0), Err(ERR_STATE));
    assert_eq!(guard_on_allowed(v, 0, 1), Err(ERR_STATE));
    assert_eq!(close_allowed(v, 0, 1), Err(ERR_STATE));
    assert_eq!(back_to_allowed(v, 0, 0), Err(ERR_STATE));
    assert_eq!(
        ask_admission(v, &ask(0, 0, ""), OpenRequests::default(), false),
        Err(ERR_STATE)
    );
}
