//! The comparison that decides whether a table entry stands for a declared
//! setting.
use super::{find, same_setting};
use paperpaw_sdk::{Id, SettingSpec};

const GROUP: Id = Id::new_static("p.g");
static TABLE: [SettingSpec; 3] = [
    SettingSpec::switch("p.a", "A", true, GROUP),
    SettingSpec::number("p.b", "B", 1, 9, 5, GROUP).advanced(),
    SettingSpec::choice("p.c", "C", &["x", "y"], "x", GROUP).description("C help"),
];

#[test]
fn an_equal_spec_is_the_entry_and_choices_compare_by_their_values() {
    let made = TABLE[2].clone();
    assert!(same_setting(&made, &TABLE[2]));
    // A choice list made on the heap equals the one kept in the program.
    let mut owned = TABLE[2].clone();
    owned.kind = paperpaw_sdk::SettingKind::Choice(paperpaw_sdk::Choices::Owned(
        ["x", "y"].map(alloc::borrow::Cow::Borrowed).to_vec(),
    ));
    assert!(same_setting(&owned, &TABLE[2]));
}

#[test]
fn any_field_that_differs_is_a_different_setting() {
    let mut other = TABLE[1].clone();
    assert!(same_setting(&other, &TABLE[1]));
    other.level = paperpaw_sdk::Level::Basic;
    assert!(!same_setting(&other, &TABLE[1]));
    let mut other = TABLE[0].clone();
    other.scoped = true;
    assert!(!same_setting(&other, &TABLE[0]));
    let mut other = TABLE[2].clone();
    other.description = None;
    assert!(!same_setting(&other, &TABLE[2]));
    let mut other = TABLE[2].clone();
    other.kind = paperpaw_sdk::SettingKind::Choice(paperpaw_sdk::Choices::Owned(
        ["y", "x"].map(alloc::borrow::Cow::Borrowed).to_vec(),
    ));
    assert!(!same_setting(&other, &TABLE[2]));
}

#[test]
fn the_entry_is_found_anywhere_in_the_table() {
    assert_eq!(find(&TABLE, &TABLE[1].clone()), Some(1));
    assert_eq!(find(&TABLE, &TABLE[0].clone()), Some(0));
    assert_eq!(find(&TABLE, &TABLE[2].clone()), Some(2));
    let mut gone = TABLE[0].clone();
    gone.default = paperpaw_sdk::SettingValue::Bool(false);
    assert_eq!(find(&TABLE, &gone), None);
    assert_eq!(find(&[], &gone), None);
}

/// A plugin's id table names every id the host makes of its records, sorted
/// and once each, and leaves out what is not an id.
#[test]
fn the_id_table_is_sorted_and_holds_the_ids_the_host_makes() {
    use crate::native_host::registrations::{
        ActionSpec, Record, ScreenSpec, SettingGroup, SlotOffer, records_encode,
    };
    let records = [
        Record::ScreenSpec(ScreenSpec {
            name: "main",
            title: "p.main",
            token: 1,
            flags: 0,
        }),
        Record::ActionSpec(ActionSpec {
            name: "open",
            title: "p.open",
            token: 2,
            flags: 0,
        }),
        Record::SettingGroup(SettingGroup {
            name: "g",
            title: "p.g",
            order: 0,
            page: "main",
            parent: "core.reader",
        }),
        Record::SlotOffer(SlotOffer {
            slot: "home.slot",
            screen: "p.main",
            priority: 0,
        }),
        // Not an id: left out of the table (the host refuses the record).
        Record::SlotOffer(SlotOffer {
            slot: "Not An Id",
            screen: "p.main",
            priority: 0,
        }),
    ];
    let mut bytes = alloc::vec![0; 1024];
    let len = records_encode(&records, &mut bytes).unwrap();
    let source =
        paperpaw_sdk::native::declare::fixed_ids_source("p", "IDS", &[&bytes[..len]]).unwrap();
    let ids: alloc::vec::Vec<&str> = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("paperpaw_sdk::Id::new_static(\""))
        .filter_map(|line| line.strip_suffix("\"),"))
        .collect();
    assert_eq!(
        ids,
        ["core.reader", "home.slot", "p", "p.g", "p.main", "p.open"]
    );
    assert!(source.starts_with("pub static IDS: [paperpaw_sdk::Id; 6] = ["));
}
