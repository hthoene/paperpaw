#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::super::settings_value::{SettingValueRef, settings_decode};
use super::*;
use alloc::{vec, vec::Vec};

#[test]
fn all_registry_declarations_use_generated_records_in_order() {
    let mut batches = Vec::new();
    let mut declarations = Declarations::new(|bytes: &[u8]| {
        batches.push(bytes.to_vec());
        Ok(())
    });
    let mut step = SetupStepDecl::new("start", "setup.start", SetupPage::Screen("setup.open"));
    step.need = Some(crate::MenuNeed::Wifi);
    declarations.setup_step(step).unwrap();
    declarations
        .slot(SlotDecl::new("place", "slot.place"))
        .unwrap();
    let mut offer = SlotOfferDecl::new("core.home", "setup.open");
    offer.priority = -3;
    declarations.slot_offer(offer).unwrap();
    let mut binding = KeyBindingDecl::new("global", "button.menu", "setup.open");
    binding.when_key = "setup.enabled";
    binding.when_value = "on";
    declarations.key_binding(binding).unwrap();
    declarations
        .setting_default("reader.font-size", SettingValue::Int(14))
        .unwrap();
    declarations
        .setting_choice("power.mode", "goal", "goal.name")
        .unwrap();
    declarations
        .preset(PresetDecl::new("day", "core.power-mode", "preset.day"))
        .unwrap();
    declarations
        .preset_value(PresetValueDecl::new(
            "setup.day",
            "reader.font-size",
            SettingValue::Absent,
        ))
        .unwrap();
    declarations.submit().unwrap();
    drop(declarations);
    let rows: Vec<_> = batches
        .iter()
        .flat_map(|bytes| {
            wire::records_decode(bytes)
                .unwrap()
                .iter()
                .collect::<Vec<_>>()
        })
        .collect();
    assert_registry_declarations(&rows);
}

fn assert_registry_declarations(rows: &[wire::Record<'_>]) {
    assert_eq!(rows.len(), 8);
    let wire::Record::SetupStep(step) = rows[0] else {
        panic!("expected step");
    };
    assert_eq!(
        (
            step.page_kind,
            step.page,
            step.need,
            step.capability,
            step.flags
        ),
        (1, "setup.open", 7, 0, 3)
    );
    assert!(matches!(
        rows[1],
        wire::Record::SlotSpec(wire::SlotSpec {
            name: "place",
            title: "slot.place"
        })
    ));
    assert!(matches!(
        rows[2],
        wire::Record::SlotOffer(wire::SlotOffer { priority: -3, .. })
    ));
    assert!(matches!(
        rows[3],
        wire::Record::KeyBind(wire::KeyBind {
            when_key: "setup.enabled",
            when_value: "on",
            ..
        })
    ));
    let wire::Record::SettingDefault(value) = rows[4] else {
        panic!("expected default");
    };
    assert_eq!(
        settings_decode(value.value).unwrap(),
        SettingValueRef::Int(14)
    );
    assert!(matches!(
        rows[5],
        wire::Record::SettingChoice(wire::SettingChoice { value: "goal", .. })
    ));
    assert!(matches!(
        rows[6],
        wire::Record::PresetSpec(wire::PresetSpec {
            kind: "core.power-mode",
            ..
        })
    ));
    assert!(matches!(
        rows[7],
        wire::Record::PresetValue(wire::PresetValue {
            value: [],
            flags: 1,
            ..
        })
    ));
}

#[test]
fn no_device_requirement_is_distinct_from_speech() {
    let mut batches = Vec::new();
    let mut declarations = Declarations::new(|bytes: &[u8]| {
        batches.push(bytes.to_vec());
        Ok(())
    });
    let mut step = SetupStepDecl::new("first", "first.title", SetupPage::Setting("wifi.enabled"));
    declarations.setup_step(step).unwrap();
    step.name = "second";
    step.need = Some(crate::MenuNeed::Speech);
    step.applies = false;
    declarations.setup_step(step).unwrap();
    declarations.submit().unwrap();
    drop(declarations);
    let rows: Vec<_> = wire::records_decode(&batches[0]).unwrap().iter().collect();
    let wire::Record::SetupStep(first) = rows[0] else {
        panic!();
    };
    let wire::Record::SetupStep(second) = rows[1] else {
        panic!();
    };
    assert_eq!((first.need, first.capability, first.flags), (0, 0, 1));
    assert_eq!((second.need, second.capability, second.flags), (0, 0, 2));
}

#[test]
fn invalid_registry_declaration_latches_and_aborts_without_partial_submission() {
    let mut batches = Vec::new();
    let mut declarations = Declarations::new(|bytes: &[u8]| {
        batches.push(bytes.to_vec());
        Ok(())
    });
    declarations
        .slot(SlotDecl::new("home", "home.title"))
        .unwrap();
    assert_eq!(
        declarations
            .setting_default("foreign.choice", SettingValue::Absent)
            .unwrap_err(),
        DeclareError::Default
    );
    assert_eq!(
        declarations
            .preset(PresetDecl::new("later", "core.power-mode", "later.title"))
            .unwrap_err(),
        DeclareError::Default
    );
    assert_eq!(declarations.submit(), Err(DeclareError::Default));
    drop(declarations);
    assert_eq!(batches, vec![Vec::<u8>::new()]);
}

#[test]
fn encoded_foreign_values_must_fit_the_record_before_borrowing() {
    let maximum =
        wire::SETTING_DEFAULT_VALUE_MAX_BYTES - super::super::settings_value::SETTINGS_HEADER_BYTES;
    let text = "x".repeat(maximum);
    let long = "x".repeat(maximum + 1);
    let mut batches = Vec::new();
    let mut declarations = Declarations::new(|bytes: &[u8]| {
        batches.push(bytes.to_vec());
        Ok(())
    });
    declarations
        .setting_default("foreign.name", SettingValue::Text(&text))
        .unwrap();
    declarations
        .preset_value(PresetValueDecl::new(
            "profiles.day",
            "foreign.name",
            SettingValue::Text(&text),
        ))
        .unwrap();
    declarations.submit().unwrap();
    drop(declarations);
    let rows: Vec<_> = wire::records_decode(&batches[0]).unwrap().iter().collect();
    let wire::Record::SettingDefault(value) = rows[0] else {
        panic!();
    };
    assert_eq!(
        settings_decode(value.value).unwrap(),
        SettingValueRef::Text(&text)
    );
    for preset in [false, true] {
        let mut declarations = Declarations::new(|_: &[u8]| Ok(()));
        let result = if preset {
            declarations.preset_value(PresetValueDecl::new(
                "profiles.day",
                "foreign.name",
                SettingValue::Text(&long),
            ))
        } else {
            declarations.setting_default("foreign.name", SettingValue::Text(&long))
        };
        assert_eq!(
            result.unwrap_err(),
            DeclareError::Encode(paperpaw_native::host_glue::ERR_NO_SPACE)
        );
        assert_eq!(
            declarations.submit(),
            Err(DeclareError::Encode(
                paperpaw_native::host_glue::ERR_NO_SPACE
            ))
        );
    }
}
