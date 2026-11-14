//! Which revocations of a library's list the store keeps and hands to the
//! host (the interface-6 `tests/code.rs` revocation cases, on `Revocations`).
#![allow(clippy::unwrap_used, reason = "fixtures")]
use paperpaw_plugin_store::native_model::{MAX_READ, Revocations};
use paperpaw_sdk::native::keys::MAX_REVOKED;

const AUTHOR: [u8; 32] = [1; 32];
const SIGNATURE: &str = "02";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One entry of the list: `key` and `signature` as the library wrote them.
fn entry(
    list: &mut Revocations,
    plugin: &str,
    key: &str,
    signature: Option<&str>,
    here: &[&str],
    stopped: &[&str],
) {
    list.field("plugin", plugin).unwrap();
    list.field("key", key).unwrap();
    if let Some(signature) = signature {
        list.field("signature", signature).unwrap();
    }
    let here = |id: &str| here.contains(&id);
    // Every plugin has the same author key here: only `here` tells which are.
    let author = |_: &str| Some(AUTHOR);
    let stopped = || stopped.iter().map(|s| (*s).to_string()).collect();
    list.entry_done(&here, (&author, &stopped, false)).unwrap();
}

fn good_key() -> String {
    hex(&AUTHOR)
}

fn good_signature() -> String {
    SIGNATURE.repeat(64)
}

fn plugins(list: &Revocations) -> Vec<&str> {
    list.pending().iter().map(|r| r.plugin.as_str()).collect()
}

#[test]
fn an_entry_that_holds_is_kept() {
    let mut list = Revocations::default();
    entry(
        &mut list,
        "clock",
        &good_key(),
        Some(&good_signature()),
        &["clock"],
        &[],
    );
    assert_eq!(plugins(&list), ["clock"]);
    assert_eq!(list.pending()[0].key, AUTHOR);
}

#[test]
fn revocations_that_do_not_hold_stop_nothing() {
    let mut list = Revocations::default();
    let sig = good_signature();
    let here = ["clock"];
    // A key that is not the pinned author's.
    entry(&mut list, "clock", &hex(&[9; 32]), Some(&sig), &here, &[]);
    // A plugin that is not here.
    entry(&mut list, "weather", &good_key(), Some(&sig), &here, &[]);
    // Damaged: a key too short, no signature, no plugin, hex that is none.
    entry(&mut list, "clock", "abcd", Some("00"), &here, &[]);
    entry(&mut list, "clock", &good_key(), None, &here, &[]);
    entry(&mut list, "", &good_key(), Some(&sig), &here, &[]);
    entry(&mut list, "clock", "zz", Some("zz"), &here, &[]);
    assert!(list.pending().is_empty());
}

#[test]
fn a_bad_entry_does_not_cost_the_good_ones_beside_it() {
    let mut list = Revocations::default();
    let here = ["clock"];
    entry(&mut list, "clock", "zz", Some("zz"), &here, &[]);
    // Nothing of the bad entry is left to spoil the next one.
    entry(
        &mut list,
        "clock",
        &good_key(),
        Some(&good_signature()),
        &here,
        &[],
    );
    assert_eq!(plugins(&list), ["clock"]);
}

#[test]
fn entries_for_plugins_not_here_take_no_place_from_those_that_are() {
    let mut list = Revocations::default();
    let (key, sig) = (good_key(), good_signature());
    let here = ["clock"];
    for n in 0..300 {
        entry(&mut list, &format!("p{n}"), &key, Some(&sig), &here, &[]);
    }
    entry(&mut list, "clock", &good_key(), Some(&sig), &here, &[]);
    assert_eq!(plugins(&list), ["clock"]);
}

#[test]
fn no_more_than_the_kernel_takes_in_one_go_are_kept() {
    let mut list = Revocations::default();
    let (key, sig) = (hex(&[7; 32]), good_signature());
    let names: Vec<String> = (0..MAX_REVOKED + 4).map(|n| format!("p{n}")).collect();
    let here: Vec<&str> = names.iter().map(String::as_str).collect();
    let host_checked = |list: &mut Revocations, id: &str| {
        list.field("plugin", id).unwrap();
        list.field("key", &key).unwrap();
        list.field("signature", &sig).unwrap();
        let (here, author, stopped) = (
            |id: &str| here.contains(&id),
            |_: &str| None,
            || Vec::<String>::new(),
        );
        // Where the host keeps the pins, the entry goes to it whatever the
        // store knows of the key.
        list.entry_done(&here, (&author, &stopped, true)).unwrap();
    };
    for name in &names {
        host_checked(&mut list, name);
    }
    assert_eq!(list.pending().len(), MAX_REVOKED);
    assert_eq!(plugins(&list)[0], "p0");
}

#[test]
fn a_list_is_read_only_so_far() {
    let mut list = Revocations::default();
    let (key, sig) = (hex(&[7; 32]), good_signature());
    for n in 0..MAX_READ {
        entry(
            &mut list,
            &format!("p{n}"),
            &key,
            Some(&sig),
            &["clock"],
            &[],
        );
    }
    // The entry that comes after the bound is not looked at.
    entry(&mut list, "clock", &good_key(), Some(&sig), &["clock"], &[]);
    assert!(list.pending().is_empty());
}

#[test]
fn a_revocation_listed_twice_or_already_stopped_is_not_handed_over_again() {
    let mut list = Revocations::default();
    let sig = good_signature();
    for _ in 0..2 {
        entry(&mut list, "clock", &good_key(), Some(&sig), &["clock"], &[]);
    }
    assert_eq!(plugins(&list), ["clock"]);
    let mut stopped = Revocations::default();
    entry(
        &mut stopped,
        "clock",
        &good_key(),
        Some(&sig),
        &["clock"],
        &["clock"],
    );
    assert!(stopped.pending().is_empty());
}

#[test]
fn a_value_longer_than_a_valid_one_is_cut_and_holds_for_nothing() {
    let mut list = Revocations::default();
    let long = "a".repeat(500);
    entry(&mut list, "clock", &long, Some(&long), &["clock"], &[]);
    entry(
        &mut list,
        &"c".repeat(500),
        &good_key(),
        Some(&good_signature()),
        &["clock"],
        &[],
    );
    assert!(list.pending().is_empty());
}
