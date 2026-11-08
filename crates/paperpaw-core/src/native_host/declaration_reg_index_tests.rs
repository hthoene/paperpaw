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

fn cross_batch_check(records: &[Record<'_>], owned_mask: u8, collision: bool) {
    use super::{HostOwner, Registration, Teardown};
    use alloc::vec::Vec;
    use core::{cell::Cell, pin::pin};

    // One live image holds all batches, including those copied as runtime inputs.
    let mut image = Vec::new();
    let mut ranges = Vec::new();
    for record in records {
        let mut wire = vec![0; REGISTRATION_MAX_BYTES];
        let len = records_encode(&[*record], &mut wire).unwrap();
        let start = image.len();
        image.extend_from_slice(&wire[..len]);
        ranges.push(start..image.len());
    }
    let owner = pin!(HostOwner::new((), ()));
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    construction.set_image(&image).unwrap();
    if collision {
        construction.raw.force_hash = Some(7);
    }
    for (index, range) in ranges.into_iter().enumerate() {
        let wire = &image[range];
        let batch = records_decode(wire).unwrap();
        let source = (owned_mask & (1 << index) == 0).then_some(image.as_slice());
        let pending = construction
            .raw
            .prepare(source, Some(wire), &batch)
            .unwrap();
        construction.raw.commit(pending);
    }
    construction.raw.sort_index();
    assert_eq!(
        construction.raw.check_duplicates(Some(&image)),
        Err(ERR_INVALID)
    );
    let mapped = Cell::new(false);
    let teardown = Teardown {
        report: |_, _, _| panic!("refused construction cannot invoke teardown reporting"),
        cancel: |_| panic!("refused construction cannot arm cancellation"),
        release: |_| panic!("refused construction cannot arm release"),
        destroy: |_| panic!("refused construction cannot transfer destruction"),
    };
    let result = construction.commit_native(teardown, |_| {
        mapped.set(true);
        Ok(())
    });
    assert!(matches!(result, Err(ERR_INVALID)));
    assert!(!mapped.get(), "whole-plugin refusal must precede mapping");
    assert!(!owner.is_live(), "none of the plugin may become active");
}

#[test]
fn duplicates_across_batches_refuse_the_whole_construction() {
    let unrelated = Record::SlotSpec(SlotSpec {
        name: "unrelated",
        title: "Unrelated",
    });
    for record in rows() {
        for mask in 0..8 {
            for collision in [false, true] {
                cross_batch_check(&[record, unrelated, record], mask, collision);
            }
        }
    }
}

#[test]
fn differing_payloads_do_not_disambiguate_unique_keys_across_batches() {
    let mut changed = rows();
    changed[2] = Record::SlotOffer(SlotOffer {
        slot: "core.settings",
        screen: "fixture.page",
        priority: 42,
    });
    changed[3] = Record::KeyBind(KeyBind {
        layer: "core.reader",
        input: "next",
        action: "fixture.other",
        when_key: "fixture.mode",
        when_value: "one",
    });
    changed[4] = Record::SettingDefault(SettingDefault {
        key: "other.setting",
        value: &[1],
    });
    for index in [2, 3, 4] {
        for mask in 0..4 {
            cross_batch_check(&[rows()[index], changed[index]], mask, true);
        }
    }
}
