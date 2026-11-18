//! The table of `command.set_hint` labels: checks, replacement, removal, the
//! limit of 8 actions per instance and the labels going with their instance.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use paperpaw_native::host_glue::ERR_INVALID;

const SCREEN: u32 = 7;
const OWNER: &str = "probe";
/// A budget no test but the budget tests comes near.
const BIG: usize = 1 << 20;

fn set(action: &str, kind: u32, label: &str) -> HintSet {
    make(OWNER, SCREEN, action, kind, label).unwrap()
}

fn set_for(owner: &str, instance: u32, action: &str, label: &str) -> HintSet {
    make(owner, instance, action, 1, label).unwrap()
}

/// What `admits` says of one more command (kind 0 is a removal).
fn admits(
    labels: &HintLabels,
    instance: u32,
    staged: &[HintSet],
    action: &str,
    removes: bool,
) -> Result<(), i32> {
    let (kind, label) = if removes { (0, "") } else { (1, "x") };
    let one = make(OWNER, instance, action, kind, label).unwrap();
    labels.admits(&one, staged, BIG)
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
    assert_eq!(check("probe.go", 2, "arrow-left"), Ok(()));
    assert_eq!(check("probe.go", 2, "no-such-icon"), Err(ERR_NOT_FOUND));
    // Nor is a code point a name, though `icons::char` takes one.
    assert_eq!(check("probe.go", 2, "U+F8000"), Err(ERR_NOT_FOUND));
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
    // An icon is shown by its character, as a text the row draws as it.
    let icon = paperpaw_sdk::icons::char("arrow-left").and_then(paperpaw_sdk::icons::as_text);
    assert!(icon.is_some());
    let mut labels = applied(alloc::vec![
        set("probe.go", 1, "Go"),
        set("probe.more", 2, "arrow-left"),
        set("probe.skip", 3, ""),
    ]);
    assert_eq!(
        labels.hint(SCREEN, "probe.go"),
        Some(Hint::Text("Go".into()))
    );
    assert_eq!(labels.hint(SCREEN, "probe.more"), icon.map(Hint::Text));
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
        admits(&labels, SCREEN, &[], "probe.new", false),
        Err(ERR_NO_SPACE)
    );
    // One it has already, or a removal, fits; so does another instance.
    assert_eq!(admits(&labels, SCREEN, &[], "probe.a3", false), Ok(()));
    assert_eq!(admits(&labels, SCREEN, &[], "probe.a3", true), Ok(()));
    assert_eq!(admits(&labels, SCREEN, &[], "probe.nothing", true), Ok(()));
    assert_eq!(admits(&labels, SCREEN + 1, &[], "probe.new", false), Ok(()));
    // The staged ones count: a removal staged first makes room for one new.
    let staged = [set("probe.a0", 0, "")];
    assert_eq!(admits(&labels, SCREEN, &staged, "probe.new", false), Ok(()));
    let staged = [set("probe.a0", 0, ""), set("probe.new", 1, "x")];
    assert_eq!(
        admits(&labels, SCREEN, &staged, "probe.newer", false),
        Err(ERR_NO_SPACE)
    );
}

#[test]
fn room_is_taken_for_every_new_action_of_one_instance() {
    let sets: Vec<HintSet> = (0..3)
        .map(|n| set(&alloc::format!("probe.a{n}"), 1, "x"))
        .collect();
    let mut labels = HintLabels::default();
    labels.reserve(&sets).unwrap();
    // Up to the cap, so applying three new actions cannot allocate.
    assert!(labels.room(SCREEN) >= MAX_ACTIONS);
    let room = labels.room(SCREEN);
    labels.apply(sets);
    assert_eq!(labels.count(SCREEN), 3);
    assert_eq!(labels.room(SCREEN), room);
}

#[test]
fn labels_go_with_their_instance() {
    let mut labels = applied(alloc::vec![
        set("probe.go", 1, "Go"),
        set_for(OWNER, SCREEN + 1, "probe.go", "Other"),
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

/// "probe.go" with the words "Go": 8 + 2 + 4 bytes charged.
const GO: usize = 14;

#[test]
fn the_budgets_are_the_per_instance_bound_times_three_and_twelve() {
    assert_eq!(label_budget(true), 3 * 8 * (128 + 64 + 4));
    assert_eq!(label_budget(true), 4_704);
    assert_eq!(label_budget(false), 18_816);
}

#[test]
fn a_label_over_the_budget_is_no_memory_and_the_earlier_ones_stay() {
    let first = set("probe.go", 1, "Go");
    let mut labels = HintLabels::default();
    assert_eq!(labels.admits(&first, &[], GO), Ok(()));
    assert_eq!(labels.admits(&first, &[], GO - 1), Err(ERR_NO_MEMORY));
    labels.reserve(core::slice::from_ref(&first)).unwrap();
    labels.apply(alloc::vec![first]);
    assert_eq!(labels.charged(OWNER), GO);
    // "probe.go2" + "Go" + 4 = 15 more bytes.
    let second = set("probe.go2", 1, "Go");
    assert_eq!(labels.admits(&second, &[], GO + 15), Ok(()));
    assert_eq!(labels.admits(&second, &[], GO + 14), Err(ERR_NO_MEMORY));
    // Nothing changed: the first label still shows, the charge is the same.
    assert_eq!(
        labels.hint(SCREEN, "probe.go"),
        Some(Hint::Text("Go".into()))
    );
    assert_eq!(labels.charged(OWNER), GO);
    // Longer words for the same action are charged as the difference.
    let longer = set("probe.go", 1, "Go on");
    assert_eq!(labels.admits(&longer, &[], 8 + 5 + 4), Ok(()));
    assert_eq!(labels.admits(&longer, &[], 8 + 5 + 3), Err(ERR_NO_MEMORY));
    // The staged ones of this callback count too.
    let staged = [set("probe.go2", 1, "Go")];
    let third = set("probe.go3", 1, "Go");
    assert_eq!(labels.admits(&third, &staged, GO + 30), Ok(()));
    assert_eq!(labels.admits(&third, &staged, GO + 29), Err(ERR_NO_MEMORY));
}

#[test]
fn the_budget_is_the_plugins_over_all_its_instances_and_not_another_plugins() {
    let labels = applied(alloc::vec![
        set_for("probe", 1, "probe.go", "Go"),
        set_for("other", 2, "probe.go", "Go"),
    ]);
    let one = |owner| labels.charged(owner);
    assert_eq!(one("probe"), one("other"));
    let more = set_for("probe", 3, "probe.go", "Go");
    // The plugin's other instance counts against its third; another plugin's
    // instance does not.
    assert_eq!(labels.admits(&more, &[], 2 * one("probe")), Ok(()));
    assert_eq!(
        labels.admits(&more, &[], 2 * one("probe") - 1),
        Err(ERR_NO_MEMORY)
    );
}

#[test]
fn every_release_path_brings_the_charge_back() {
    let mut labels = applied(alloc::vec![
        set("probe.go", 1, "Go"),
        set("probe.back", 1, "Back"),
    ]);
    let both = labels.charged(OWNER);
    assert_eq!(both, (8 + 2 + 4) + (10 + 4 + 4));
    // Replaced by shorter words: the difference is released.
    let mut shorter = HintLabels::default();
    core::mem::swap(&mut shorter, &mut labels);
    let mut labels = shorter;
    let replace = alloc::vec![set("probe.back", 1, "B")];
    labels.reserve(&replace).unwrap();
    labels.apply(replace);
    assert_eq!(labels.charged(OWNER), both - 3);
    // A hidden label is charged the action and the overhead only.
    let icons = alloc::vec![set("probe.go", 3, "")];
    labels.reserve(&icons).unwrap();
    labels.apply(icons);
    assert_eq!(labels.charged(OWNER), (8 + 4) + (10 + 1 + 4));
    // Cleared by kind 0: nothing left for it.
    let clear = alloc::vec![set("probe.go", 0, ""), set("probe.back", 0, "")];
    labels.reserve(&clear).unwrap();
    labels.apply(clear);
    assert_eq!(labels.charged(OWNER), 0);
    assert!(labels.is_empty());
    // The instance closing, and the plugin stopping (all its instances).
    let mut labels = applied(alloc::vec![
        set("probe.go", 1, "Go"),
        set_for(OWNER, SCREEN + 1, "probe.go", "Go"),
    ]);
    labels.retain_open(|instance| instance == SCREEN + 1);
    assert_eq!(labels.charged(OWNER), GO);
    labels.retain_open(|_| false);
    assert_eq!(labels.charged(OWNER), 0);
}
