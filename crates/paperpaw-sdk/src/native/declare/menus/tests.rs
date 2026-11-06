#![allow(
    clippy::unwrap_used,
    reason = "fixture encoding failures must fail tests"
)]
use super::*;
use crate::native::declare::{BATCH_MAX_RECORDS, EventKind};
use alloc::string::ToString;
use alloc::{vec, vec::Vec};
use wire::{Record, records_decode, records_encode};

#[test]
fn every_placement_and_order_edge_matches_generated_bytes() {
    for order in [i16::MIN, 37, i16::MAX] {
        for (place, code, anchor) in [
            (EntryPlacement::Order, 0, ""),
            (EntryPlacement::After("foreign.item"), 1, "foreign.item"),
            (EntryPlacement::Before("foreign.item"), 2, "foreign.item"),
            (
                EntryPlacement::InSection("foreign.section"),
                3,
                "foreign.section",
            ),
            (EntryPlacement::First, 4, ""),
            (EntryPlacement::Last, 5, ""),
        ] {
            let mut batches = Vec::new();
            let mut builder = Declarations::new(|batch: &[u8]| {
                batches.push(batch.to_vec());
                Ok(())
            });
            let mut entry = MenuEntryDecl::new("foreign.tools", "battery.open");
            entry.order = order;
            entry.place = place;
            builder
                .menu_entry(entry)
                .unwrap()
                .icon("battery.open", "battery")
                .unwrap();
            builder.submit().unwrap();
            drop(builder);
            let records = [
                Record::MenuEntry(wire::MenuEntry {
                    menu: "foreign.tools",
                    item: "battery.open",
                    order: i32::from(order),
                    place: code,
                    anchor,
                }),
                Record::IconSpec(wire::IconSpec {
                    item: "battery.open",
                    icon: "battery",
                }),
            ];
            let mut out = [0; 256];
            let length = records_encode(&records, &mut out).unwrap();
            assert_eq!(batches, [out[..length].to_vec()]);
        }
    }
}

#[test]
fn every_need_uses_the_contract_value_and_preserves_declaration_order() {
    let kinds = [
        (crate::MenuNeed::Speech, 0, 0),
        (crate::MenuNeed::Sleep, 1, 0),
        (crate::MenuNeed::Rotation, 2, 0),
        (crate::MenuNeed::NoTouch, 3, 0),
        (crate::MenuNeed::Touch, 4, 0),
        (crate::MenuNeed::ButtonRow, 5, 0),
        (crate::MenuNeed::Colors, 6, 0),
        (crate::MenuNeed::Wifi, 7, 0),
        (crate::MenuNeed::Firmware, 8, 0),
        (crate::MenuNeed::BlackAndWhite, 9, 0),
        (crate::MenuNeed::Frontlight, 10, 0),
        (crate::MenuNeed::Bluetooth, 11, 0),
        (crate::MenuNeed::Has(crate::Capability::AUDIO), 12, 1),
        (
            crate::MenuNeed::Has(crate::Capability::PEN_HOVER),
            12,
            1 << 16,
        ),
    ];
    let mut batches = Vec::new();
    let mut builder = Declarations::new(|batch: &[u8]| {
        batches.push(batch.to_vec());
        Ok(())
    });
    let mut expected = vec![Record::Registration(wire::Registration::Event {
        event_tag: wire::EVENT_STARTED_TAG,
        token: 2,
    })];
    builder.subscribe_event(EventKind::Started, 2).unwrap();
    for (need, code, capability) in kinds {
        builder.menu_need("battery.open", need).unwrap();
        expected.push(Record::MenuNeed(wire::MenuNeed {
            item: "battery.open",
            need: code,
            capability,
        }));
    }
    builder.submit().unwrap();
    assert_eq!(builder.len(), expected.len());
    drop(builder);
    for (batch, records) in batches.iter().zip(expected.chunks(BATCH_MAX_RECORDS)) {
        let mut out = [0; 512];
        let length = records_encode(records, &mut out).unwrap();
        assert_eq!(batch, &out[..length]);
    }
    assert_eq!(batches.len(), 2);
}

#[test]
fn maximum_field_lengths_fit_and_unknown_icon_names_are_kept() {
    let menu = "a.".to_string() + &"a".repeat(wire::MENU_ENTRY_MENU_MAX_BYTES - 2);
    let item = "b.".to_string() + &"b".repeat(wire::MENU_ENTRY_ITEM_MAX_BYTES - 2);
    let anchor = "c.".to_string() + &"c".repeat(wire::MENU_ENTRY_ANCHOR_MAX_BYTES - 2);
    let icon = "d".repeat(wire::ICON_SPEC_ICON_MAX_BYTES);
    let mut batches = Vec::new();
    let mut builder = Declarations::new(|batch: &[u8]| {
        batches.push(batch.to_vec());
        Ok(())
    });
    let mut entry = MenuEntryDecl::new(&menu, &item);
    entry.place = EntryPlacement::After(&anchor);
    builder
        .menu_entry(entry)
        .unwrap()
        .icon(&item, &icon)
        .unwrap();
    builder.menu_need(&item, crate::MenuNeed::Touch).unwrap();
    builder.submit().unwrap();
    drop(builder);
    let decoded = records_decode(&batches[0]).unwrap();
    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded.iter().nth(1), Some(Record::IconSpec(value)) if value.icon == icon));
}

#[test]
fn invalid_menu_fields_latch_and_abort_pending_callbacks() {
    let long_menu = "a".repeat(wire::MENU_ENTRY_MENU_MAX_BYTES + 1);
    let long_item = "a".repeat(wire::MENU_ENTRY_ITEM_MAX_BYTES + 1);
    let long_anchor = "a".repeat(wire::MENU_ENTRY_ANCHOR_MAX_BYTES + 1);
    for (menu, item, place) in [
        ("", "battery.open", EntryPlacement::Order),
        ("tools", "battery.open", EntryPlacement::Order),
        ("core.tools", "open", EntryPlacement::Order),
        (
            "core.tools",
            "battery.open",
            EntryPlacement::After("anchor"),
        ),
        ("Core.tools", "battery.open", EntryPlacement::Order),
        ("core..tools", "battery.open", EntryPlacement::Order),
        ("core.tools", "", EntryPlacement::Order),
        ("core.tools", "bad name", EntryPlacement::Order),
        ("core.tools", "battery.open", EntryPlacement::After("")),
        ("core.tools", "battery.open", EntryPlacement::Before("BAD")),
        (
            "core.tools",
            "battery.open",
            EntryPlacement::InSection("bad..section"),
        ),
        (&long_menu, "battery.open", EntryPlacement::Order),
        ("core.tools", &long_item, EntryPlacement::Order),
        (
            "core.tools",
            "battery.open",
            EntryPlacement::After(&long_anchor),
        ),
    ] {
        let mut batches = Vec::new();
        let mut builder = Declarations::new(|batch: &[u8]| {
            batches.push(batch.to_vec());
            Ok(())
        });
        builder.subscribe_event(EventKind::Stopping, 7).unwrap();
        let mut entry = MenuEntryDecl::new(menu, item);
        entry.place = place;
        let error = builder.menu_entry(entry).unwrap_err();
        assert_eq!(builder.icon("battery.open", "battery").unwrap_err(), error);
        assert_eq!(
            builder
                .menu_need("battery.open", crate::MenuNeed::Touch)
                .unwrap_err(),
            error
        );
        assert_eq!(builder.submit(), Err(error));
        assert_eq!(builder.submit(), Err(error));
        drop(builder);
        assert_eq!(batches, [Vec::<u8>::new()]);
    }
}

#[test]
fn invalid_icon_fields_and_need_items_are_refused() {
    let long_item = "a".repeat(wire::ICON_SPEC_ITEM_MAX_BYTES + 1);
    let long_icon = "a".repeat(wire::ICON_SPEC_ICON_MAX_BYTES + 1);
    for (item, icon) in [
        ("", "battery"),
        ("bad..item", "battery"),
        ("battery.open", ""),
        ("battery.open", "Battery"),
        ("battery.open", "arrow.left"),
        ("battery.open", "é"),
        (&long_item, "battery"),
        ("battery.open", &long_icon),
    ] {
        let mut builder = Declarations::new(|_: &[u8]| Ok(()));
        assert!(builder.icon(item, icon).is_err());
        assert!(builder.submit().is_err());
    }
    for item in ["", "Bad", "bad..item", &long_item] {
        let mut builder = Declarations::new(|_: &[u8]| Ok(()));
        assert!(builder.menu_need(item, crate::MenuNeed::Wifi).is_err());
        assert!(builder.submit().is_err());
    }
}

#[test]
fn sink_refusal_is_preserved_without_retry_or_further_encoding() {
    let mut calls = 0;
    let mut builder = Declarations::new(|_: &[u8]| {
        calls += 1;
        Err(-77)
    });
    for _ in 0..BATCH_MAX_RECORDS {
        builder.icon("battery.open", "battery").unwrap();
    }
    let error = DeclareError::Sink(-77);
    assert_eq!(
        builder
            .menu_entry(MenuEntryDecl::new("core.tools", "battery.open"))
            .unwrap_err(),
        error
    );
    assert_eq!(
        builder
            .menu_need("battery.open", crate::MenuNeed::Touch)
            .unwrap_err(),
        error
    );
    assert_eq!(builder.icon("battery.open", "battery").unwrap_err(), error);
    assert_eq!(
        builder
            .menu_section(MenuSectionDecl::new("heading", "title", "core.tools"))
            .unwrap_err(),
        error
    );
    assert_eq!(
        builder
            .hide_menu_entry("core.tools", "other.open")
            .unwrap_err(),
        error
    );
    assert_eq!(
        builder.describe_item("open", "battery.help").unwrap_err(),
        error
    );
    assert_eq!(builder.submit(), Err(error));
    drop(builder);
    assert_eq!(calls, 1);
}
