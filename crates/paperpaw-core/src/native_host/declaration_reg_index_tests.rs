//! REG unique keys stay distinct even when their hashes collide.
#![allow(clippy::unwrap_used, reason = "fixture failure must fail the test")]
use super::{declaration_index::*, raw_declarations::RawDeclarations, registrations::*};
use alloc::vec;
use paperpaw_native::host_glue::ERR_INVALID;

fn rows() -> [Record<'static>; 8] {
    [
        Record::SetupStep(SetupStep {
            name: "setup",
            title: "Setup",
            help: "",
            order: 0,
            page_kind: 0,
            page: "fixture.page",
            applies_key: "",
            applies_value: "",
            need: 0,
            capability: 0,
            flags: 1,
        }),
        Record::SlotSpec(SlotSpec {
            name: "slot",
            title: "Slot",
        }),
        Record::SlotOffer(SlotOffer {
            slot: "core.settings",
            screen: "fixture.page",
            priority: 0,
        }),
        Record::KeyBind(KeyBind {
            layer: "core.reader",
            input: "next",
            action: "fixture.next",
            when_key: "fixture.mode",
            when_value: "one",
        }),
        Record::SettingDefault(SettingDefault {
            key: "other.setting",
            value: &[],
        }),
        Record::SettingChoice(SettingChoice {
            key: "other.setting",
            value: "one",
            title: "One",
        }),
        Record::PresetSpec(PresetSpec {
            name: "preset",
            kind: "core.reading",
            title: "Preset",
            help: "",
        }),
        Record::PresetValue(PresetValue {
            preset: "fixture.preset",
            key: "other.setting",
            value: &[],
            flags: 0,
        }),
    ]
}

fn check(records: &[Record<'_>], owned: bool, collision: bool) -> Result<(), i32> {
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    let image = (!owned).then_some(bytes.as_slice());
    let mut raw = RawDeclarations::default();
    if collision {
        raw.force_hash = Some(7);
    }
    let pending = raw
        .prepare(image, Some(&bytes), &records_decode(&bytes).unwrap())
        .unwrap();
    raw.commit(pending);
    raw.sort_index();
    raw.check_duplicates(image)
}

#[test]
fn reg_kinds_are_unique_and_duplicates_refuse_on_both_storage_paths() {
    for (index, record) in rows().into_iter().enumerate() {
        assert_eq!(usize::from(key(record).unwrap().0), index + 8);
        for owned in [false, true] {
            for collision in [false, true] {
                assert_eq!(check(&[record, record], owned, collision), Err(ERR_INVALID));
            }
        }
    }
    assert_eq!(core::mem::size_of::<NameEntry>() * rows().len(), 64);
}

#[test]
fn every_composite_field_participates_in_equality_and_lookup() {
    let base = KeyBind {
        layer: "core.reader",
        input: "next",
        action: "fixture.next",
        when_key: "fixture.mode",
        when_value: "one",
    };
    let variants = [
        KeyBind {
            layer: "core.menu",
            ..base
        },
        KeyBind {
            input: "prev",
            ..base
        },
        KeyBind {
            when_key: "fixture.other",
            ..base
        },
        KeyBind {
            when_value: "two",
            ..base
        },
    ];
    let first = Record::KeyBind(base);
    for variant in variants {
        let second = Record::KeyBind(variant);
        assert!(!same_record_name(first, second));
        assert_ne!(record_hash(first), record_hash(second));
        for owned in [false, true] {
            assert_eq!(check(&[first, second], owned, true), Ok(()));
        }
    }
    let changed_action = Record::KeyBind(KeyBind {
        action: "fixture.other",
        ..base
    });
    assert!(same_record_name(first, changed_action));
    assert_eq!(
        check(&[first, changed_action], true, true),
        Err(ERR_INVALID)
    );

    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&[first], &mut bytes).unwrap();
    bytes.truncate(len);
    let mut raw = RawDeclarations::default();
    raw.force_hash = Some(1);
    let pending = raw
        .prepare(None, Some(&bytes), &records_decode(&bytes).unwrap())
        .unwrap();
    raw.commit(pending);
    raw.sort_index();
    let kind = KIND_TABLE[11].1;
    assert!(
        raw.find_parts(
            None,
            kind,
            &[base.layer, base.input, base.when_key, base.when_value]
        )
        .unwrap()
        .is_some()
    );
    assert!(
        raw.find_parts(None, kind, &[base.layer, base.input, base.when_key, "two"])
            .unwrap()
            .is_none()
    );
    assert!(raw.find(None, kind, base.layer).unwrap().is_none());
}

#[test]
fn same_slot_can_offer_different_screens_and_two_part_keys_compare_the_tail() {
    let pairs = [
        (
            Record::SlotOffer(SlotOffer {
                slot: "core.settings",
                screen: "fixture.one",
                priority: 1,
            }),
            Record::SlotOffer(SlotOffer {
                slot: "core.settings",
                screen: "fixture.two",
                priority: 2,
            }),
        ),
        (
            Record::SettingChoice(SettingChoice {
                key: "other.setting",
                value: "one",
                title: "One",
            }),
            Record::SettingChoice(SettingChoice {
                key: "other.setting",
                value: "two",
                title: "Two",
            }),
        ),
        (
            Record::PresetValue(PresetValue {
                preset: "fixture.preset",
                key: "other.one",
                value: &[],
                flags: 0,
            }),
            Record::PresetValue(PresetValue {
                preset: "fixture.preset",
                key: "other.two",
                value: &[],
                flags: 0,
            }),
        ),
    ];
    for (a, b) in pairs {
        assert!(!same_record_name(a, b));
        assert_ne!(record_hash(a), record_hash(b));
        for owned in [false, true] {
            assert_eq!(check(&[a, b], owned, true), Ok(()));
        }
    }
}
