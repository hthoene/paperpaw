//! The model against the interface-6 items screen: the same order, the same
//! keeps, on every small setting; and allocation refusals.
use super::*;
use crate::fail_alloc;
use alloc::{format, string::ToString, vec};

const ENTRIES: [(&str, Option<&str>); 4] = [
    ("clock", None),
    ("wifi", Some("net")),
    ("battery", None),
    ("lock", None),
];

fn entries() -> Vec<Entry> {
    ENTRIES
        .iter()
        .map(|(id, also)| Entry {
            id: (*id).to_string(),
            title: format!("title.{id}"),
            also: also.map(ToString::to_string),
        })
        .collect()
}

// ---- The interface-6 algorithms, as the SDK's `ItemsScreen` has them. ----

fn canonical(id: &str) -> &str {
    ENTRIES
        .iter()
        .find(|(e, also)| *e == id || *also == Some(id))
        .map_or(id, |(e, _)| e)
}
fn known(id: &str) -> bool {
    let id = canonical(id);
    ENTRIES.iter().any(|(e, _)| *e == id)
}
fn ref_chosen(setting: &str) -> Vec<&str> {
    let mut chosen: Vec<&str> = Vec::new();
    for c in setting.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        let c = canonical(c);
        if known(c) && !chosen.contains(&c) {
            chosen.push(c);
        }
    }
    chosen
}
fn ref_order(setting: &str) -> Vec<&'static str> {
    let chosen = ref_chosen(setting);
    let mut order: Vec<&'static str> = chosen
        .iter()
        .map(|c| ENTRIES.iter().find(|(e, _)| e == c).unwrap().0)
        .collect();
    for (e, _) in ENTRIES {
        if !order.contains(&e) {
            order.push(e);
        }
    }
    order
}
fn ref_keep(setting: &str, chosen: &[&str]) -> String {
    let mut left = chosen.iter();
    let mut all: Vec<&str> = setting
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .filter_map(|c| {
            if known(c) {
                left.next().copied()
            } else {
                Some(c)
            }
        })
        .collect();
    all.extend(left);
    all.join(", ")
}
fn ref_toggle(setting: &str, row: usize) -> Option<String> {
    let id = *ref_order(setting).get(row)?;
    let mut chosen = ref_chosen(setting);
    match chosen.iter().position(|c| *c == id) {
        Some(at) => {
            chosen.remove(at);
        }
        None => chosen.push(id),
    }
    Some(ref_keep(setting, &chosen))
}
fn ref_move(setting: &str, from: usize, to: usize) -> Option<String> {
    let mut chosen = ref_chosen(setting);
    if from >= chosen.len() || to >= chosen.len() || from == to {
        return None;
    }
    let item = chosen.remove(from);
    chosen.insert(to, item);
    Some(ref_keep(setting, &chosen))
}

/// Settings of every size over the items, an old id, an unknown plugin's
/// items and a repeated id.
fn settings() -> Vec<String> {
    let words = ["clock", "wifi", "net", "battery", "lock", "gone", "other"];
    let mut found = vec![String::new()];
    let mut level: Vec<Vec<usize>> = vec![vec![]];
    for _ in 0..4 {
        let mut next = Vec::new();
        for base in &level {
            for w in 0..words.len() {
                let mut v = base.clone();
                v.push(w);
                found.push(v.iter().map(|i| words[*i]).collect::<Vec<_>>().join(", "));
                next.push(v);
            }
        }
        level = next;
    }
    found.push(" clock ,, wifi ,".to_string());
    found
}

fn names(chooser: &Chooser, rows: &[usize]) -> Vec<String> {
    rows.iter()
        .map(|r| chooser.entries()[*r].id.clone())
        .collect()
}

#[test]
fn rows_are_the_shown_items_then_the_others() {
    for setting in settings() {
        let chooser = Chooser::new(entries(), &setting).unwrap();
        let rows = chooser.rows().unwrap();
        assert_eq!(names(&chooser, &rows), ref_order(&setting), "{setting:?}");
        let chosen = chooser.chosen().unwrap();
        assert_eq!(
            names(&chooser, &chosen),
            ref_chosen(&setting),
            "{setting:?}"
        );
    }
}

/// The setting with its ids trimmed and joined, as nothing changed.
fn plain(setting: &str) -> String {
    setting
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn toggling_a_row_keeps_what_the_old_screen_keeps() {
    for setting in settings() {
        for row in 0..=ENTRIES.len() {
            let mut chooser = Chooser::new(entries(), &setting).unwrap();
            let changed = chooser.toggle(row).unwrap();
            let want = ref_toggle(&setting, row);
            assert_eq!(changed, want.is_some(), "{setting:?} row {row}");
            assert_eq!(
                chooser.value().unwrap(),
                want.unwrap_or_else(|| plain(&setting)),
                "{setting:?} row {row}"
            );
        }
    }
}

#[test]
fn moving_a_shown_item_keeps_what_the_old_screen_keeps() {
    for setting in settings() {
        for from in 0..=ENTRIES.len() {
            for to in 0..=ENTRIES.len() {
                let mut chooser = Chooser::new(entries(), &setting).unwrap();
                let changed = chooser.move_to(from, to).unwrap();
                let want = ref_move(&setting, from, to);
                assert_eq!(changed, want.is_some(), "{setting:?} {from}->{to}");
                assert_eq!(
                    chooser.value().unwrap(),
                    want.unwrap_or_else(|| plain(&setting)),
                    "{setting:?} {from}->{to}"
                );
            }
        }
    }
}

#[test]
fn items_of_absent_plugins_stay_where_they_were() {
    let mut chooser = Chooser::new(entries(), "gone, clock, other, wifi").unwrap();
    assert!(chooser.move_to(0, 1).unwrap());
    assert_eq!(chooser.value().unwrap(), "gone, wifi, other, clock");
    assert!(chooser.toggle(0).unwrap());
    assert_eq!(chooser.value().unwrap(), "gone, clock, other");
}

#[test]
fn the_buttons_pick_up_move_and_put_down() {
    let mut c = Chooser::new(entries(), "clock, wifi, battery").unwrap();
    // The focus is not shown yet: Confirm is left to the caller.
    assert_eq!(
        c.action("core.confirm", 0, false).unwrap(),
        Outcome::Ignored
    );
    // Not on a shown item, or too few shown: Menu is not taken.
    assert_eq!(c.action("core.menu", 3, true).unwrap(), Outcome::Ignored);
    assert_eq!(c.action("core.down", 0, true).unwrap(), Outcome::Ignored);
    // Pick up the first, move it down twice, the third press stops at the end.
    assert_eq!(c.action("core.menu", 0, true).unwrap(), Outcome::Redraw);
    assert!(c.moving());
    assert_eq!(c.action("core.down", 0, true).unwrap(), Outcome::Changed(1));
    assert_eq!(
        c.action("core.next-page", 1, true).unwrap(),
        Outcome::Changed(2)
    );
    assert_eq!(c.action("core.down", 2, true).unwrap(), Outcome::Redraw);
    assert_eq!(c.value().unwrap(), "wifi, battery, clock");
    assert_eq!(c.action("core.up", 2, true).unwrap(), Outcome::Changed(1));
    assert_eq!(c.action("core.home", 1, true).unwrap(), Outcome::Ignored);
    assert_eq!(c.action("core.back", 1, true).unwrap(), Outcome::Redraw);
    assert!(!c.moving());
    assert_eq!(c.value().unwrap(), "wifi, clock, battery");
    // Not moving: Confirm shows or hides the row.
    assert_eq!(
        c.action("core.confirm", 3, true).unwrap(),
        Outcome::Changed(3)
    );
    assert_eq!(c.value().unwrap(), "wifi, clock, battery, lock");
    assert_eq!(
        c.action("core.confirm", 0, true).unwrap(),
        Outcome::Changed(0)
    );
    assert_eq!(c.value().unwrap(), "clock, battery, lock");
    // A finger takes over.
    c.action("core.menu", 0, true).unwrap();
    c.put_down();
    assert!(!c.moving());
}

#[test]
fn refused_allocations_are_errors_and_leave_a_valid_model() {
    // Built outside the sweep: only the model's own allocations are refused.
    let mut c = Chooser::new(entries(), "clock, wifi, gone, battery").unwrap();
    fail_alloc::sweep(|| {
        c.rows()?;
        c.chosen()?;
        c.value()?;
        c.can_move(0, true)?;
        // Moves that undo each other.
        c.move_to(0, 1)?;
        c.move_to(1, 0)?;
        c.action("core.menu", 0, true)?;
        c.action("core.down", 0, true)?;
        c.action("core.up", 1, true)?;
        c.action("core.back", 0, true)?;
        c.toggle(3)?;
        c.toggle(3)?;
        Ok(())
    });
    // Whatever was refused, every id of the setting is still there once.
    c.put_down();
    let value = c.value().unwrap();
    assert!(value.contains("gone"), "{value}");
    assert_eq!(c.rows().unwrap().len(), 4);
    for setting in ["clock, wifi, gone, battery", ""] {
        fail_alloc::sweep(|| Chooser::new(Vec::new(), setting).map(|c| c.ids.len()));
    }
}
