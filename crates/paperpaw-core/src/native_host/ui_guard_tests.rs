//! The guard and lock rules of `docs/abi/ui.md` sections 1.2, 1.3, 3.1, 4.3,
//! each with the refusal and the case just inside it.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use alloc::vec::Vec;

fn row(instance: u32, owner: &str) -> Row<'_> {
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
fn locked<'a>() -> Vec<Row<'a>> {
    let mut lock = row(2, "lock");
    lock.guards = true;
    let mut keyboard = row(3, "keyboard");
    keyboard.guard_dialog = true;
    alloc::vec![row(1, "reader"), lock, keyboard]
}

fn plain<'a>() -> Vec<Row<'a>> {
    alloc::vec![row(1, "reader"), row(2, "menu")]
}

impl UiStackView for Vec<Row<'_>> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
    fn row(&self, index: usize) -> Option<Row<'_>> {
        self.get(index).copied()
    }
}

fn view<'a>(rows: &'a Vec<Row<'a>>) -> &'a dyn UiStackView {
    rows
}

#[test]
fn a_slice_of_rows_is_a_stack_view() {
    let rows = locked();
    let slice = Rows(&rows);
    assert_eq!(UiStackView::len(&slice), 3);
    assert_eq!(slice.row(1).map(|r| r.owner), Some("lock"));
    assert!(slice.row(3).is_none());
    assert_eq!(guard_owner(&slice), Some("lock"));
}

#[test]
fn the_topmost_guard_and_secrecy_follow_the_flags_and_the_position() {
    let rows = locked();
    let v = view(&rows);
    assert_eq!(topmost_guard(v), Some(1));
    assert_eq!(guard_owner(v), Some("lock"));
    // Not secret below the guard; the guard and what lies above it are.
    assert!(!is_secret(v, 0));
    assert!(is_secret(v, 1));
    assert!(is_secret(v, 2));
    assert!(top_is_secret(v));
    assert!(!is_secret(v, 3), "no such row");
    let rows = plain();
    assert_eq!(topmost_guard(view(&rows)), None);
    assert!(!top_is_secret(view(&rows)));
    let mut rows = plain();
    rows[1].secret = true;
    assert!(top_is_secret(view(&rows)));
    let empty: Vec<Row<'_>> = Vec::new();
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
    assert_eq!(open_index(v, "mail", false, false), 1);
    // The guard's plugin, its authorized dialog and the host's sleep screen go on top.
    assert_eq!(open_index(v, "lock", false, false), 3);
    assert_eq!(open_index(v, "keyboard", true, false), 3);
    assert_eq!(open_index(v, "core", false, true), 3);
    // Without a guard every open goes on top.
    let rows = plain();
    assert_eq!(open_index(view(&rows), "mail", false, false), 2);
    let empty: Vec<Row<'_>> = Vec::new();
    assert_eq!(open_index(view(&empty), "mail", false, false), 0);
    assert_eq!(input_target(view(&empty)), None);
}

#[test]
fn a_guard_turns_on_only_for_the_own_top_instance_not_counting_the_hosts_screen() {
    let mut rows = plain();
    // Own top: allowed.
    assert_eq!(guard_on_allowed(view(&rows), "menu", 2), Ok(()));
    // Own but not top, another plugin's instance, unknown instance.
    assert_eq!(guard_on_allowed(view(&rows), "reader", 1), Err(ERR_STATE));
    assert_eq!(guard_on_allowed(view(&rows), "reader", 2), Err(ERR_DENIED));
    assert_eq!(guard_on_allowed(view(&rows), "menu", 9), Err(ERR_NOT_FOUND));
    // The host's sleep screen above does not count: a lock locking while the
    // sleep screen shows guards at once.
    let mut sleep = row(3, "core");
    sleep.host = true;
    rows.push(sleep);
    assert_eq!(guard_on_allowed(view(&rows), "menu", 2), Ok(()));
    // A plugin's own screen above does.
    rows.push(row(4, "mail"));
    assert_eq!(guard_on_allowed(view(&rows), "menu", 2), Err(ERR_STATE));
    let empty: Vec<Row<'_>> = Vec::new();
    assert_eq!(
        guard_on_allowed(view(&empty), "menu", 1),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn only_the_guards_plugin_raises_closes_or_goes_back_past_the_lock() {
    let rows = locked();
    let v = view(&rows);
    assert_eq!(raise_allowed(v, "mail"), Err(ERR_DENIED));
    assert_eq!(raise_allowed(v, "lock"), Ok(()));
    assert_eq!(raise_allowed(view(&plain()), "mail"), Ok(()));
    // Close: the guard itself and what is above it.
    assert_eq!(close_allowed(v, "mail", 2), Err(ERR_DENIED));
    assert_eq!(close_allowed(v, "mail", 3), Err(ERR_DENIED));
    assert_eq!(close_allowed(v, "lock", 2), Ok(Close::Instance(1)));
    assert_eq!(close_allowed(v, "lock", 3), Err(ERR_DENIED), "not its own");
    // A guard dialog closes itself (0 = own top), nobody else's.
    assert_eq!(close_allowed(v, "keyboard", 0), Ok(Close::Instance(2)));
    assert_eq!(close_allowed(v, "keyboard", 3), Ok(Close::Instance(2)));
    assert_eq!(close_allowed(v, "keyboard", 2), Err(ERR_DENIED));
    // The page below the guard is another plugin's: not closed by a stranger.
    assert_eq!(close_allowed(v, "mail", 1), Err(ERR_DENIED));
    // Back to: nothing to close is fine, closing anything past the lock is not.
    assert_eq!(back_to_allowed(v, "mail", 2), Ok(()));
    assert_eq!(back_to_allowed(v, "mail", 0), Err(ERR_DENIED));
    assert_eq!(back_to_allowed(v, "mail", 1), Err(ERR_DENIED));
    assert_eq!(back_to_allowed(v, "lock", 0), Ok(()));
    assert_eq!(back_to_allowed(view(&plain()), "mail", 0), Ok(()));
}

#[test]
fn close_follows_the_navigation_rules_without_a_guard() {
    let mut rows = plain();
    rows[0].overlay = false;
    let v = view(&rows);
    // 0 is the own top instance; outside it ERR_STATE.
    assert_eq!(close_allowed(v, "menu", 0), Ok(Close::Instance(1)));
    assert_eq!(close_allowed(v, "reader", 0), Err(ERR_STATE));
    // A non-zero instance: own, wherever it is; unknown is not found.
    assert_eq!(close_allowed(v, "reader", 1), Ok(Close::NoEffect), "root");
    assert_eq!(close_allowed(v, "menu", 7), Err(ERR_NOT_FOUND));
    // Another plugin's instance only when it is an overlay.
    assert_eq!(close_allowed(v, "help", 2), Err(ERR_DENIED));
    rows[1].overlay = true;
    assert_eq!(
        close_allowed(view(&rows), "help", 2),
        Ok(Close::Instance(1))
    );
    let empty: Vec<Row<'_>> = Vec::new();
    assert_eq!(close_allowed(view(&empty), "menu", 0), Err(ERR_STATE));
}

#[test]
fn input_filters_and_key_bindings_stay_inside_the_lock() {
    let rows = locked();
    let v = view(&rows);
    // The top (a guard dialog) is secret: its plugin's filters and the
    // guard's run, no others.
    assert!(filter_runs(v, "lock"));
    assert!(filter_runs(v, "keyboard"));
    assert!(!filter_runs(v, "mail"));
    assert!(!filter_runs(v, "reader"));
    // No guard and nothing secret: every filter runs.
    assert!(filter_runs(view(&plain()), "mail"));
    // A secret screen without a guard (a password field): own plugin's only.
    let mut rows = plain();
    rows[1].secret = true;
    assert!(filter_runs(view(&rows), "menu"));
    assert!(!filter_runs(view(&rows), "mail"));
    // Key maps: another plugin's action or input does nothing under the lock.
    assert!(binding_runs(v, None), "host action");
    assert!(binding_runs(v, Some("lock")));
    assert!(!binding_runs(v, Some("mail")));
    assert!(binding_runs(view(&plain()), Some("mail")));
}

#[test]
fn the_stack_other_plugins_see_is_the_guard_alone() {
    let rows = locked();
    let v = view(&rows);
    assert_eq!(
        visible_rows(v, "mail"),
        Visible {
            start: 1,
            end: 2,
            left_out: true
        }
    );
    assert_eq!(
        visible_rows(v, "lock"),
        Visible {
            start: 1,
            end: 3,
            left_out: true
        }
    );
    assert_eq!(
        visible_rows(view(&plain()), "mail"),
        Visible {
            start: 0,
            end: 2,
            left_out: false
        }
    );
    // A guard at the root: the guard's plugin leaves nothing out.
    let mut root = alloc::vec![row(1, "lock")];
    root[0].guards = true;
    assert_eq!(
        visible_rows(view(&root), "lock"),
        Visible {
            start: 0,
            end: 1,
            left_out: false
        }
    );
    assert_eq!(
        visible_rows(view(&root), "mail"),
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
    assert!(
        !drawn_under_overlay(v, 2, "core"),
        "above the guard: secret"
    );
    assert!(
        !drawn_under_overlay(v, 0, "core"),
        "the page, below the guard"
    );
    assert!(!drawn_under_overlay(v, 1, "core"));
    // Its own plugin's overlay shows it.
    assert!(drawn_under_overlay(v, 1, "lock"));
    // Without a guard the page is drawn under the sleep screen, unless secret.
    let mut rows = plain();
    assert!(drawn_under_overlay(view(&rows), 0, "core"));
    rows[0].secret = true;
    assert!(!drawn_under_overlay(view(&rows), 0, "core"));
    assert!(!drawn_under_overlay(view(&rows), 9, "core"));
}

fn ask<'a>(asker: &'a str, kind: u32, slot: &'a str) -> Ask<'a> {
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
        assert!(ask_admission(v, &ask("mail", kind, ""), open, false).is_ok());
    }
    for kind in [4, 5, u32::MAX] {
        assert_eq!(
            ask_admission(v, &ask("mail", kind, ""), open, false),
            Err(ERR_INVALID)
        );
    }
    // The phone keyboard slot is for text questions only; any other slot is invalid.
    assert!(ask_admission(v, &ask("mail", 0, REMOTE_KEYBOARD), open, false).is_ok());
    for kind in 1..=3 {
        assert_eq!(
            ask_admission(v, &ask("mail", kind, REMOTE_KEYBOARD), open, false),
            Err(ERR_INVALID)
        );
    }
    for slot in ["core.keyboard", "x", "core.remote-keyboard "] {
        assert_eq!(
            ask_admission(v, &ask("mail", 0, slot), open, false),
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
    assert!(ask_admission(v, &ask("a", 0, ""), open(3, false), false).is_ok());
    assert_eq!(
        ask_admission(v, &ask("a", 0, ""), open(4, false), false),
        Err(ERR_FULL)
    );
    assert_eq!(
        ask_admission(v, &ask("a", 0, ""), open(200, false), false),
        Err(ERR_FULL)
    );
    // X4: one.
    assert!(ask_admission(v, &ask("a", 0, ""), open(0, false), true).is_ok());
    assert_eq!(
        ask_admission(v, &ask("a", 0, ""), open(1, false), true),
        Err(ERR_FULL)
    );
    // The asker's own open request is cancelled by asking again, so it
    // makes room even on a full device.
    let again = ask_admission(v, &ask("a", 0, ""), open(1, true), true).unwrap();
    assert!(again.cancels_asker_request);
    let again = ask_admission(v, &ask("a", 0, ""), open(4, true), false).unwrap();
    assert!(again.cancels_asker_request);
    // Another asker on the same full device is refused.
    assert_eq!(
        ask_admission(v, &ask("b", 0, ""), open(4, false), false),
        Err(ERR_FULL)
    );
}

#[test]
fn the_nested_phone_question_is_one_per_dialog_and_one_plus_one_on_the_x4() {
    let rows = plain();
    let v = view(&rows);
    let mut phone = ask("keyboard", 0, REMOTE_KEYBOARD);
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
        let mut other = ask("keyboard", kind, slot);
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
    let mut phone = ask("keyboard", 0, REMOTE_KEYBOARD);
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
    let own = ask_admission(v, &ask("lock", 0, ""), OpenRequests::default(), false).unwrap();
    assert!(!own.below_guard);
    let other = ask_admission(v, &ask("mail", 0, ""), OpenRequests::default(), false).unwrap();
    assert!(other.below_guard);
}
