#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The network policy the services read: off, offline and held, in the order
//! `network.md` 3.2 gives, and closed when it cannot be made.
use super::*;
use crate::native_host::net_service::fail_nth;

fn names<'a>(of: &'a [&'a str]) -> impl Iterator<Item = &'a str> {
    of.iter().copied()
}

fn made(version: u32, offline: bool, mode: bool, off: &[&str], held: &[&str]) -> NetPolicy {
    let policy = NetPolicy::new();
    policy.update(version, offline, mode, names(off), names(held));
    policy
}

fn why(policy: &NetPolicy, plugin: &str, background: bool) -> Option<(u32, bool)> {
    policy
        .end_for(plugin, background)
        .map(|e| (e.reason, e.later))
}

#[test]
fn an_open_policy_ends_nothing() {
    let policy = NetPolicy::new();
    assert_eq!(why(&policy, "mail", true), None);
    assert_eq!(why(&policy, "mail", false), None);
}

#[test]
fn a_plugin_that_is_off_ends_with_network_off_and_no_retry() {
    let policy = made(1, false, false, &["mail"], &[]);
    assert_eq!(
        why(&policy, "mail", false),
        Some((reason::NETWORK_OFF, false))
    );
    assert_eq!(
        why(&policy, "mail", true),
        Some((reason::NETWORK_OFF, false))
    );
    assert_eq!(why(&policy, "news", true), None, "only that plugin");
}

#[test]
fn a_plugin_held_in_saving_is_refused_background_use_only_and_may_try_later() {
    let policy = made(1, false, false, &[], &["mail"]);
    assert_eq!(why(&policy, "mail", true), Some((reason::SAVING, true)));
    assert_eq!(why(&policy, "mail", false), None, "the reader asking goes");
    assert_eq!(why(&policy, "news", true), None);
    // Where the mode is offline itself, the hold is no further hold.
    let offline_mode = made(1, false, true, &[], &["mail"]);
    assert_eq!(why(&offline_mode, "mail", true), None);
}

#[test]
fn the_offline_mode_ends_everyone_and_off_comes_before_it() {
    let policy = made(1, true, true, &["mail"], &["news"]);
    assert_eq!(
        why(&policy, "mail", false),
        Some((reason::NETWORK_OFF, false))
    );
    assert_eq!(why(&policy, "news", true), Some((reason::OFFLINE, true)));
    assert_eq!(why(&policy, "other", false), Some((reason::OFFLINE, true)));
}

#[test]
fn the_snapshot_is_made_again_only_when_something_changed() {
    let policy = made(3, false, false, &["mail"], &[]);
    assert!(!policy.needs(3, false, false));
    assert!(
        policy.needs(4, false, false),
        "the settings were read again"
    );
    assert!(policy.needs(3, true, false), "the mode changed");
    assert!(policy.needs(3, false, true));
}

#[test]
fn a_snapshot_that_cannot_be_made_closes_the_policy_and_is_tried_again() {
    let policy = made(1, false, false, &[], &[]);
    for nth in 1..=3_u32 {
        fail_nth(nth);
        policy.update(2, false, false, names(&["mail", "news"]), names(&["a"]));
        fail_nth(0);
        // Not guessed open: nothing goes out until it can be made.
        assert_eq!(
            why(&policy, "other", false),
            Some((reason::NETWORK_OFF, false)),
            "gate {nth}"
        );
        assert!(policy.needs(2, false, false), "gate {nth}: tried again");
    }
    policy.update(2, false, false, names(&["mail"]), names(&[]));
    assert!(!policy.needs(2, false, false));
    assert_eq!(why(&policy, "other", false), None);
    assert_eq!(
        why(&policy, "mail", false),
        Some((reason::NETWORK_OFF, false))
    );
}
