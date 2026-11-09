//! The table of `command.set_hint` labels: checks, replacement, removal, the
//! limit of 8 actions per instance and the labels going with their instance.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use paperpaw_native::host_glue::ERR_INVALID;

const SCREEN: u32 = 7;

fn set(action: &str, kind: u32, label: &str) -> HintSet {
    make(SCREEN, action, kind, label).unwrap()
}

fn applied(sets: Vec<HintSet>) -> HintLabels {
    let mut labels = HintLabels::default();
    labels.reserve(&sets).unwrap();
    labels.apply(sets);
    labels
}

#[test]
fn the_fields_are_checked_in_the_contracts_words() {
    // kind 0 and 3 carry no label, 1 and 2 need one, other kinds are invalid.
    assert_eq!(check("probe.go", 0, "x"), Err(ERR_INVALID));
    assert_eq!(check("probe.go", 3, "x"), Err(ERR_INVALID));
    assert_eq!(check("probe.go", 1, ""), Err(ERR_INVALID));
    assert_eq!(check("probe.go", 2, ""), Err(ERR_INVALID));
    assert_eq!(check("probe.go", 4, ""), Err(ERR_INVALID));
    assert_eq!(check("probe.go", u32::MAX, "x"), Err(ERR_INVALID));
    assert_eq!(check("probe.go", 0, ""), Ok(()));
    assert_eq!(check("probe.go", 1, "Go"), Ok(()));
    assert_eq!(check("probe.go", 3, ""), Ok(()));
    // The action is a full id of at most 128 bytes; the label at most 64.
    assert_eq!(check("", 0, ""), Err(ERR_INVALID));
    assert_eq!(check("Probe.Go", 0, ""), Err(ERR_INVALID));
    assert_eq!(check("probe..go", 0, ""), Err(ERR_INVALID));
    let long = alloc::format!("probe.{}", "a".repeat(123));
    assert_eq!(long.len(), 129);
    assert_eq!(check(&long, 0, ""), Err(ERR_INVALID));
    assert_eq!(check("probe.go", 1, &"é".repeat(33)), Err(ERR_INVALID));
    assert_eq!(check("probe.go", 1, &"a".repeat(64)), Ok(()));
    assert_eq!(check("probe.go", 1, &"a".repeat(65)), Err(ERR_INVALID));
    // An icon is a host icon; an unknown one is not found.
    let icon = paperpaw_sdk::icons::names().next().unwrap();
    assert_eq!(check("probe.go", 2, icon), Ok(()));
    assert_eq!(check("probe.go", 2, "no-such-icon"), Err(ERR_NOT_FOUND));
}

#[test]
fn the_ways_out_are_never_hidden_but_may_be_relabelled() {
    for action in [
        "core.back",
        "core.menu",
        "core.settings",
        "core.sleep",
        "core.power-off",
        "core.restart",
    ] {
        assert_eq!(check(action, 3, ""), Err(ERR_DENIED), "{action}");
        assert_eq!(check(action, 1, "Close"), Ok(()), "{action}");
        assert_eq!(check(action, 0, ""), Ok(()), "{action}");
    }
    assert_eq!(check("core.home", 3, ""), Ok(()));
    assert_eq!(check("probe.back", 3, ""), Ok(()));
}

#[test]
fn a_label_shows_replaces_and_is_removed_by_kind_0() {
    let icon = paperpaw_sdk::icons::names().next().unwrap();
    let mut labels = applied(alloc::vec![
        set("probe.go", 1, "Go"),
        set("probe.more", 2, icon),
        set("probe.skip", 3, ""),
    ]);
    assert_eq!(
        labels.hint(SCREEN, "probe.go"),
        Some(Hint::Text("Go".into()))
    );
    assert_eq!(labels.hint(SCREEN, "probe.more"), Some(Hint::Icon(icon)));
    assert_eq!(labels.hint(SCREEN, "probe.skip"), Some(Hint::Hidden));
    assert_eq!(labels.hint(SCREEN, "probe.other"), None);
    assert_eq!(labels.hint(SCREEN + 1, "probe.go"), None);
    // A second set for one action replaces the first, kind 0 removes it.
    let sets = alloc::vec![set("probe.go", 1, "Run"), set("probe.skip", 0, "")];
    labels.reserve(&sets).unwrap();
    labels.apply(sets);
    assert_eq!(
        labels.hint(SCREEN, "probe.go"),
        Some(Hint::Text("Run".into()))
    );
    assert_eq!(labels.hint(SCREEN, "probe.skip"), None);
    assert_eq!(labels.count(SCREEN), 2);
    // A removal of one never set changes nothing, and the last one empties
    // the instance's entry.
    let sets = alloc::vec![set("probe.nothing", 0, ""), set("probe.go", 0, "")];
    labels.reserve(&sets).unwrap();
    labels.apply(sets);
    let sets = alloc::vec![set("probe.more", 0, "")];
    labels.reserve(&sets).unwrap();
    labels.apply(sets);
    assert!(labels.is_empty());
}

#[test]
fn a_ninth_action_is_refused_and_a_replacement_or_removal_is_not() {
    let eight: Vec<HintSet> = (0..8)
        .map(|n| set(&alloc::format!("probe.a{n}"), 1, "x"))
        .collect();
    let labels = applied(eight);
    assert_eq!(labels.count(SCREEN), 8);
    assert_eq!(
        labels.admits(SCREEN, &[], "probe.new", false),
        Err(ERR_NO_SPACE)
    );
    // One it has already, or a removal, fits; so does another instance.
    assert_eq!(labels.admits(SCREEN, &[], "probe.a3", false), Ok(()));
    assert_eq!(labels.admits(SCREEN, &[], "probe.a3", true), Ok(()));
    assert_eq!(labels.admits(SCREEN, &[], "probe.nothing", true), Ok(()));
    assert_eq!(labels.admits(SCREEN + 1, &[], "probe.new", false), Ok(()));
    // The staged ones count: a removal staged first makes room for one new.
    let staged = [set("probe.a0", 0, "")];
    assert_eq!(labels.admits(SCREEN, &staged, "probe.new", false), Ok(()));
    let staged = [set("probe.a0", 0, ""), set("probe.new", 1, "x")];
    assert_eq!(
        labels.admits(SCREEN, &staged, "probe.newer", false),
        Err(ERR_NO_SPACE)
    );
}

#[test]
fn labels_go_with_their_instance() {
    let mut labels = applied(alloc::vec![
        set("probe.go", 1, "Go"),
        make(SCREEN + 1, "probe.go", 1, "Other").unwrap(),
    ]);
    labels.retain_open(|instance| instance == SCREEN + 1);
    assert_eq!(labels.hint(SCREEN, "probe.go"), None);
    assert_eq!(
        labels.hint(SCREEN + 1, "probe.go"),
        Some(Hint::Text("Other".into()))
    );
    labels.retain_open(|_| false);
    assert!(labels.is_empty());
}

#[test]
fn applying_to_an_instance_that_left_does_nothing() {
    let sets = alloc::vec![set("probe.go", 1, "Go")];
    let mut labels = HintLabels::default();
    labels.reserve(&sets).unwrap();
    labels.retain_open(|_| false);
    labels.apply(sets);
    assert!(labels.is_empty());
}
