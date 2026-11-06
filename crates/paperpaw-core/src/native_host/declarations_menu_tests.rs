//! Menu records 13-18 (`menu.entry`, `icon.spec`, `menu.need`, `menu.section`,
//! `menu.hide`, `menu.describe`): syntax, forward compatibility, and refusal
//! until wired.
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

fn need(item: &str, need: u32, capability: u64) -> MenuNeed<'_> {
    MenuNeed {
        item,
        need,
        capability,
    }
}

#[test]
fn a_battery_tools_entry_and_icon_are_well_formed() {
    assert_eq!(
        menu_entry_check(&entry("core.tools", "battery.open", 37, 0, "")),
        Ok(())
    );
    let icon = IconSpec {
        item: "battery.open",
        icon: "battery",
    };
    assert_eq!(icon_check(&icon), Ok(()));
}

#[test]
fn items_may_be_another_plugins_or_a_core_setting() {
    // The reader puts core settings into its own menu (interface 6 parity).
    for item in ["core.night-mode", "core.reading-font", "stats.open"] {
        assert_eq!(
            menu_entry_check(&entry("reader.menu", item, 0, 0, "")),
            Ok(())
        );
        let icon = IconSpec { item, icon: "moon" };
        assert_eq!(icon_check(&icon), Ok(()), "an icon theme may name {item}");
        assert_eq!(need_check(&need(item, 4, 0)), Ok(()));
        let hide = MenuHide {
            menu: "core.main",
            item,
        };
        assert_eq!(hide_check(&hide), Ok(()));
    }
    // An item is a full id: a bare name names no plugin.
    assert_eq!(
        menu_entry_check(&entry("core.tools", "open", 0, 0, "")),
        Err(ERR_INVALID)
    );
}

#[test]
fn places_name_an_anchor_exactly_when_they_need_one() {
    for place in [MENU_PLACE_AFTER, MENU_PLACE_BEFORE, MENU_PLACE_IN_SECTION] {
        let named = entry("core.main", "battery.open", 0, place, "stats.open");
        assert_eq!(menu_entry_check(&named), Ok(()));
        let missing = entry("core.main", "battery.open", 0, place, "");
        assert_eq!(menu_entry_check(&missing), Err(ERR_INVALID));
    }
    for place in [MENU_PLACE_ORDER, MENU_PLACE_FIRST, MENU_PLACE_LAST] {
        let stray = entry("core.main", "battery.open", 0, place, "stats.open");
        assert_eq!(menu_entry_check(&stray), Err(ERR_INVALID));
    }
}

#[test]
fn an_unknown_place_is_kept_so_a_later_plugin_still_loads() {
    for anchor in ["", "stats.open"] {
        let later = entry("core.main", "battery.open", 0, MENU_PLACE_LAST + 1, anchor);
        assert_eq!(menu_entry_check(&later), Ok(()), "{anchor:?}");
    }
    let bad_anchor = entry("core.main", "battery.open", 0, u32::MAX, "Stats");
    assert_eq!(menu_entry_check(&bad_anchor), Err(ERR_INVALID));
}

#[test]
fn bad_ids_names_and_orders_are_invalid() {
    let cases = [
        entry("", "battery.open", 0, 0, ""),
        entry("core", "battery.open", 0, 0, ""),
        entry("Core.Tools", "battery.open", 0, 0, ""),
        entry("core..tools", "battery.open", 0, 0, ""),
        entry("core.tools", "", 0, 0, ""),
        entry("core.tools", "battery.open now", 0, 0, ""),
        entry("core.tools", "battery.open", i32::from(i16::MAX) + 1, 0, ""),
        entry("core.tools", "battery.open", i32::from(i16::MIN) - 1, 0, ""),
    ];
    for case in &cases {
        assert_eq!(menu_entry_check(case), Err(ERR_INVALID), "{case:?}");
    }
    for order in [i32::from(i16::MIN), i32::from(i16::MAX)] {
        let edge = entry("core.tools", "battery.open", order, 0, "");
        assert_eq!(menu_entry_check(&edge), Ok(()));
    }
    for name in ["", "Battery", "arrow.left", "arrow left"] {
        let icon = IconSpec {
            item: "battery.open",
            icon: name,
        };
        assert_eq!(icon_check(&icon), Err(ERR_INVALID), "{name:?}");
    }
    let long = "a".repeat(ICON_SPEC_ICON_MAX_BYTES + 1);
    let icon = IconSpec {
        item: "battery.open",
        icon: &long,
    };
    assert_eq!(icon_check(&icon), Err(ERR_INVALID));
}

#[test]
fn every_listed_need_is_checked_and_an_unknown_one_is_kept() {
    for value in 0..MENU_NEED_HAS {
        assert_eq!(need_check(&need("battery.open", value, 0)), Ok(()));
        assert_eq!(
            need_check(&need("battery.open", value, 1)),
            Err(ERR_INVALID)
        );
    }
    // `has`: any nonzero bit set, also bits this host does not know yet.
    for bits in [1 << 11, 1 << 63] {
        assert_eq!(
            need_check(&need("battery.open", MENU_NEED_HAS, bits)),
            Ok(())
        );
    }
    assert_eq!(
        need_check(&need("battery.open", MENU_NEED_HAS, 0)),
        Err(ERR_INVALID)
    );
    // A need of a later host: accepted (not met), whatever its capability.
    for capability in [0, 1, u64::MAX] {
        let later = need("battery.open", MENU_NEEDS.len() as u32, capability);
        assert_eq!(need_check(&later), Ok(()));
    }
    assert_eq!(need_check(&need("", 0, 0)), Err(ERR_INVALID));
}

#[test]
fn sections_and_descriptions_name_the_plugins_own() {
    let section = |name, title, menu, order| MenuSection {
        name,
        title,
        menu,
        order,
    };
    assert_eq!(
        section_check(
            PLUGIN,
            &section("tools", "battery.section.tools", "core.main", 3)
        ),
        Ok(())
    );
    for bad in [
        section("", "battery.title", "core.main", 0),
        section("Tools", "battery.title", "core.main", 0),
        section("tools", "", "core.main", 0),
        section("tools", "battery.title", "core", 0),
        section(
            "tools",
            "battery.title",
            "core.main",
            i32::from(i16::MAX) + 1,
        ),
    ] {
        assert_eq!(section_check(PLUGIN, &bad), Err(ERR_INVALID), "{bad:?}");
    }
    let describe = |item, key| MenuDescribe { item, key };
    assert_eq!(
        describe_check(PLUGIN, &describe("open", "battery.open.line")),
        Ok(())
    );
    for bad in [
        describe("", "k"),
        describe("open", ""),
        describe("Open", "k"),
    ] {
        assert_eq!(describe_check(PLUGIN, &bad), Err(ERR_INVALID), "{bad:?}");
    }
    let hide = MenuHide {
        menu: "core.main",
        item: "",
    };
    assert_eq!(hide_check(&hide), Err(ERR_INVALID));
}

#[test]
fn well_formed_records_are_refused_until_the_host_takes_them() {
    let records = [
        Record::MenuEntry(entry("core.tools", "battery.open", 37, 0, "")),
        Record::IconSpec(IconSpec {
            item: "battery.open",
            icon: "battery",
        }),
        Record::MenuNeed(need("battery.open", 4, 0)),
        Record::MenuSection(MenuSection {
            name: "tools",
            title: "battery.section",
            menu: "core.main",
            order: 0,
        }),
        Record::MenuHide(MenuHide {
            menu: "core.main",
            item: "stats.open",
        }),
        Record::MenuDescribe(MenuDescribe {
            item: "open",
            key: "battery.open.line",
        }),
    ];
    let mut out = [0u8; 256];
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
