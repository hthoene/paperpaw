//! `menu.entry` (13), `icon.spec` (14), `menu.need` (15): syntax, and refusal until wired.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use crate::native_host::registrations::{records_decode, records_encode};

const PLUGIN: &str = "battery";

fn entry<'a>(
    menu: &'a str,
    item: &'a str,
    order: i32,
    place: u32,
    anchor: &'a str,
) -> MenuEntry<'a> {
    MenuEntry {
        menu,
        item,
        order,
        place,
        anchor,
    }
}

#[test]
fn a_battery_tools_entry_and_icon_are_well_formed() {
    assert_eq!(
        menu_entry_check(PLUGIN, &entry("core.tools", "open", 37, 0, "")),
        Ok(())
    );
    let icon = IconSpec {
        item: "open",
        icon: "battery",
    };
    assert_eq!(icon_check(PLUGIN, &icon), Ok(()));
}

#[test]
fn places_name_an_anchor_exactly_when_they_need_one() {
    for place in [MENU_PLACE_AFTER, MENU_PLACE_BEFORE, MENU_PLACE_IN_SECTION] {
        let named = entry("core.main", "open", 0, place, "stats.open");
        assert_eq!(menu_entry_check(PLUGIN, &named), Ok(()));
        let missing = entry("core.main", "open", 0, place, "");
        assert_eq!(menu_entry_check(PLUGIN, &missing), Err(ERR_INVALID));
    }
    for place in [MENU_PLACE_ORDER, MENU_PLACE_FIRST, MENU_PLACE_LAST] {
        let stray = entry("core.main", "open", 0, place, "stats.open");
        assert_eq!(menu_entry_check(PLUGIN, &stray), Err(ERR_INVALID));
    }
    let unknown = entry("core.main", "open", 0, MENU_PLACE_LAST + 1, "");
    assert_eq!(menu_entry_check(PLUGIN, &unknown), Err(ERR_UNSUPPORTED));
}

#[test]
fn bad_ids_names_and_orders_are_invalid() {
    let cases = [
        entry("", "open", 0, 0, ""),
        entry("Core.Tools", "open", 0, 0, ""),
        entry("core..tools", "open", 0, 0, ""),
        entry("core.tools", "", 0, 0, ""),
        entry("core.tools", "open now", 0, 0, ""),
        entry("core.tools", "open", i32::from(i16::MAX) + 1, 0, ""),
        entry("core.tools", "open", i32::from(i16::MIN) - 1, 0, ""),
    ];
    for case in &cases {
        assert_eq!(menu_entry_check(PLUGIN, case), Err(ERR_INVALID), "{case:?}");
    }
    let edges = [i32::from(i16::MIN), i32::from(i16::MAX)];
    for order in edges {
        assert_eq!(
            menu_entry_check(PLUGIN, &entry("core.tools", "open", order, 0, "")),
            Ok(())
        );
    }
    for name in ["", "Battery", "arrow.left", "arrow left"] {
        let icon = IconSpec {
            item: "open",
            icon: name,
        };
        assert_eq!(icon_check(PLUGIN, &icon), Err(ERR_INVALID), "{name:?}");
    }
    let long = "a".repeat(ICON_SPEC_ICON_MAX_BYTES + 1);
    let icon = IconSpec {
        item: "open",
        icon: &long,
    };
    assert_eq!(icon_check(PLUGIN, &icon), Err(ERR_INVALID));
}

#[test]
fn well_formed_entries_are_refused_until_the_host_takes_them() {
    let records = [
        Record::MenuEntry(entry("core.tools", "open", 37, 0, "")),
        Record::IconSpec(IconSpec {
            item: "open",
            icon: "battery",
        }),
        Record::MenuNeed(MenuNeed {
            item: "open",
            need: 4,
            capability: 0,
        }),
    ];
    let mut out = [0u8; 128];
    let len = records_encode(&records, &mut out).unwrap();
    let batch = records_decode(&out[..len]).unwrap();
    for record in batch.iter() {
        assert_eq!(validate_record(PLUGIN, &record), Err(ERR_UNSUPPORTED));
    }
    assert_eq!(validate_batch(PLUGIN, &batch), Err(ERR_UNSUPPORTED));
    // A malformed one says why: invalid comes before unsupported.
    let bad = Record::MenuEntry(entry("core.tools", "", 37, 0, ""));
    assert_eq!(validate_record(PLUGIN, &bad), Err(ERR_INVALID));
}

#[test]
fn every_listed_need_is_well_formed_and_the_next_one_is_unsupported() {
    let need = |need, capability| MenuNeed {
        item: "open",
        need,
        capability,
    };
    for value in 0..MENU_NEED_HAS {
        assert_eq!(need_check(PLUGIN, &need(value, 0)), Ok(()));
        assert_eq!(need_check(PLUGIN, &need(value, 1)), Err(ERR_INVALID));
    }
    // `has`: any nonzero bit set, also bits this host does not know yet.
    assert_eq!(need_check(PLUGIN, &need(MENU_NEED_HAS, 1 << 11)), Ok(()));
    assert_eq!(need_check(PLUGIN, &need(MENU_NEED_HAS, 1 << 63)), Ok(()));
    assert_eq!(
        need_check(PLUGIN, &need(MENU_NEED_HAS, 0)),
        Err(ERR_INVALID)
    );
    let past = need(MENU_NEEDS.len() as u32, 0);
    assert_eq!(need_check(PLUGIN, &past), Err(ERR_UNSUPPORTED));
    let unnamed = MenuNeed {
        item: "",
        need: 0,
        capability: 0,
    };
    assert_eq!(need_check(PLUGIN, &unnamed), Err(ERR_INVALID));
}
