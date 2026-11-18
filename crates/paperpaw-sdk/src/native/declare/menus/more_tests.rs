#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::*;
use alloc::{string::ToString, vec::Vec};
use wire::{Record, records_encode};

#[test]
fn sections_hides_and_descriptions_match_generated_fields_and_bytes() {
    for order in [i16::MIN, 37, i16::MAX] {
        let mut batches = Vec::new();
        let mut builder = Declarations::new(|batch: &[u8]| {
            batches.push(batch.to_vec());
            Ok(())
        });
        let mut section = MenuSectionDecl::new("tools.heading", "battery.heading", "foreign.tools");
        section.order = order;
        builder.menu_section(section).unwrap();
        builder
            .hide_menu_entry("foreign.tools", "other.open")
            .unwrap();
        builder
            .describe_item("open", "battery.open.help 🐾")
            .unwrap();
        builder.submit().unwrap();
        drop(builder);
        let records = [
            Record::MenuSection(wire::MenuSection {
                name: "tools.heading",
                title: "battery.heading",
                menu: "foreign.tools",
                order: i32::from(order),
            }),
            Record::MenuHide(wire::MenuHide {
                menu: "foreign.tools",
                item: "other.open",
            }),
            Record::MenuDescribe(wire::MenuDescribe {
                item: "open",
                key: "battery.open.help 🐾",
            }),
        ];
        let mut encoded = [0; 512];
        let length = records_encode(&records, &mut encoded).unwrap();
        assert_eq!(batches, [encoded[..length].to_vec()]);
    }
}

#[test]
fn new_field_caps_accept_exact_bounds_and_reject_one_extra_byte() {
    let name = "s".repeat(wire::MENU_SECTION_NAME_MAX_BYTES);
    let title = "t".repeat(wire::MENU_SECTION_TITLE_MAX_BYTES);
    let menu = "m.".to_string() + &"m".repeat(wire::MENU_SECTION_MENU_MAX_BYTES - 2);
    let item = "i.".to_string() + &"i".repeat(wire::MENU_HIDE_ITEM_MAX_BYTES - 2);
    let own_item = "i".repeat(wire::MENU_DESCRIBE_ITEM_MAX_BYTES);
    let key = "k".repeat(wire::MENU_DESCRIBE_KEY_MAX_BYTES);
    let mut batches = Vec::new();
    let mut builder = Declarations::new(|batch: &[u8]| {
        batches.push(batch.to_vec());
        Ok(())
    });
    builder
        .menu_section(MenuSectionDecl::new(&name, &title, &menu))
        .unwrap();
    builder.hide_menu_entry(&menu, &item).unwrap();
    builder.describe_item(&own_item, &key).unwrap();
    builder.submit().unwrap();
    drop(builder);
    assert_eq!(wire::records_decode(&batches[0]).unwrap().len(), 3);
    for (name, title, menu) in [
        (name.clone() + "x", title.clone(), menu.clone()),
        (name.clone(), title.clone() + "x", menu.clone()),
        (name.clone(), title.clone(), menu.clone() + "x"),
    ] {
        let mut builder = Declarations::new(|_: &[u8]| Ok(()));
        assert!(matches!(
            builder.menu_section(MenuSectionDecl::new(&name, &title, &menu)),
            Err(DeclareError::TooLong(_))
        ));
        assert!(builder.submit().is_err());
    }
    for (menu, item) in [
        (menu.clone() + "x", item.clone()),
        (menu.clone(), item.clone() + "x"),
    ] {
        let mut builder = Declarations::new(|_: &[u8]| Ok(()));
        assert!(matches!(
            builder.hide_menu_entry(&menu, &item),
            Err(DeclareError::TooLong(_))
        ));
        assert!(builder.submit().is_err());
    }
    for (item, key) in [
        (own_item.clone() + "x", key.clone()),
        (own_item.clone(), key.clone() + "x"),
    ] {
        let mut builder = Declarations::new(|_: &[u8]| Ok(()));
        assert!(matches!(
            builder.describe_item(&item, &key),
            Err(DeclareError::TooLong(_))
        ));
        assert!(builder.submit().is_err());
    }
}

#[test]
fn invalid_section_fields_latch_before_any_valid_record_is_submitted() {
    for (name, title, menu) in [
        ("", "title", "core.tools"),
        ("Bad", "title", "core.tools"),
        ("heading", "", "core.tools"),
        ("heading", "title", ""),
        ("heading", "title", "tools"),
        ("heading", "title", "core..tools"),
    ] {
        let mut batches = Vec::new();
        let mut builder = Declarations::new(|batch: &[u8]| {
            batches.push(batch.to_vec());
            Ok(())
        });
        builder.icon("battery.open", "battery").unwrap();
        let error = builder
            .menu_section(MenuSectionDecl::new(name, title, menu))
            .unwrap_err();
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
        assert_eq!(builder.submit(), Err(error));
        drop(builder);
        assert_eq!(batches, [Vec::<u8>::new()]);
    }
}

#[test]
fn malformed_hide_ids_and_empty_description_fields_are_refused() {
    for (menu, item) in [
        ("", "other.open"),
        ("core.tools", ""),
        ("tools", "other.open"),
        ("core.tools", "open"),
        ("core.tools", "bad..item"),
    ] {
        let mut builder = Declarations::new(|_: &[u8]| Ok(()));
        assert!(builder.hide_menu_entry(menu, item).is_err());
        assert!(builder.submit().is_err());
    }
    for (item, key) in [("", "key"), ("bad name", "key"), ("open", "")] {
        let mut builder = Declarations::new(|_: &[u8]| Ok(()));
        assert!(builder.describe_item(item, key).is_err());
        assert!(builder.submit().is_err());
    }
}
