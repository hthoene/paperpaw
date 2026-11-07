//! Normalization owns its data and propagates checked admission failures.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::abi7::settings_wire::settings_encode;
use core::mem::MaybeUninit;

fn checked(text: &str) -> Result<Id, i32> {
    checked_id(text)
}

fn encoded(value: SettingValueRef<'_>) -> Vec<u8> {
    let mut bytes = [MaybeUninit::new(0); 64];
    let length = settings_encode(value, &mut bytes).unwrap();
    bytes[..length]
        .iter()
        .map(|byte| {
            // SAFETY: the encoder initialized every byte in the reported prefix.
            unsafe { byte.assume_init() }
        })
        .collect()
}

fn spec(default: &[u8]) -> wire::SettingSpec<'_> {
    wire::SettingSpec {
        name: "enabled",
        kind: 0,
        flags: 0,
        group: "power.group",
        title: "battery.enabled",
        help: "battery.enabled.help",
        default,
        min: 0,
        max: 0,
        choices: "",
    }
}

#[test]
fn battery_booleans_keep_foreign_groups_title_help_and_default() {
    let default = encoded(SettingValueRef::Bool(true));
    for (name, group) in [("enabled", "power.group"), ("by-plugin", "core.developer")] {
        let value = normalize(
            "battery",
            wire::Record::SettingSpec(wire::SettingSpec {
                name,
                group,
                ..spec(&default)
            }),
            &mut checked,
        )
        .unwrap();
        let Value::Setting(value) = value else {
            panic!("setting expected")
        };
        assert_eq!(value.id.as_str(), alloc::format!("battery.{name}"));
        assert_eq!(value.group.as_str(), group);
        assert_eq!(value.title.as_str(), "battery.enabled");
        assert_eq!(
            value.description.as_ref().unwrap().as_str(),
            "battery.enabled.help"
        );
        assert_eq!(value.default, SettingValue::Bool(true));
        assert_eq!(value.kind, SettingKind::Bool);
    }
}

#[test]
fn supported_setting_flags_ranges_and_choice_values_are_not_lost() {
    let default = encoded(SettingValueRef::Int(-7));
    let Value::Setting(number) = normalize(
        "fixture",
        wire::Record::SettingSpec(wire::SettingSpec {
            kind: 1,
            flags: SCOPED | GUARDED | KEPT | ADVANCED,
            min: -10,
            max: 10,
            ..spec(&default)
        }),
        &mut checked,
    )
    .unwrap() else {
        panic!("setting expected")
    };
    assert_eq!(number.kind, SettingKind::Int { min: -10, max: 10 });
    assert_eq!(number.default, SettingValue::Int(-7));
    assert!(number.scoped && number.guarded && number.kept_on_reset);
    assert_eq!(number.level, Level::Advanced);

    let default = encoded(SettingValueRef::Text("fixture.two"));
    let Value::Setting(choice) = normalize(
        "fixture",
        wire::Record::SettingSpec(wire::SettingSpec {
            kind: 2,
            choices: "fixture.one\nfixture.two",
            ..spec(&default)
        }),
        &mut checked,
    )
    .unwrap() else {
        panic!("setting expected")
    };
    let SettingKind::Choice(values) = choice.kind else {
        panic!("choice expected")
    };
    assert_eq!(
        values.iter().collect::<Vec<_>>(),
        ["fixture.one", "fixture.two"]
    );
    assert_eq!(
        choice.default,
        SettingValue::Text(Cow::Borrowed("fixture.two"))
    );
}

#[test]
fn group_pages_are_relative_and_parent_ids_are_full() {
    let Value::Group(group) = normalize(
        "fixture",
        wire::Record::SettingGroup(wire::SettingGroup {
            name: "group",
            title: "fixture.group",
            order: -4,
            page: "screen",
            parent: "core.settings",
        }),
        &mut checked,
    )
    .unwrap() else {
        panic!("group expected")
    };
    assert_eq!(group.id.as_str(), "fixture.group");
    assert_eq!(group.title, "fixture.group");
    assert_eq!(group.order, -4);
    assert_eq!(group.page.unwrap().as_str(), "fixture.screen");
    assert_eq!(group.parent.unwrap().as_str(), "core.settings");
}

#[test]
fn screen_and_action_tokens_flags_and_titles_survive_normalization() {
    let screen = normalize(
        "battery",
        wire::Record::ScreenSpec(wire::ScreenSpec {
            name: "screen",
            title: "battery.title",
            token: 10,
            flags: 1,
        }),
        &mut checked,
    )
    .unwrap();
    let action = normalize(
        "battery",
        wire::Record::ActionSpec(wire::ActionSpec {
            name: "open",
            title: "battery.title",
            token: 11,
            flags: 1,
        }),
        &mut checked,
    )
    .unwrap();
    for (value, id, token) in [(screen, "battery.screen", 10), (action, "battery.open", 11)] {
        let value = match value {
            Value::Screen(value) | Value::Action(value) => value,
            _ => panic!("callback expected"),
        };
        assert_eq!(value.id.as_str(), id);
        assert_eq!(value.title, "battery.title");
        assert_eq!(value.token, token);
        assert_eq!(value.flags, 1);
    }
}

#[test]
fn invalid_records_are_checked_before_the_id_factory_and_oom_propagates() {
    let default = encoded(SettingValueRef::Bool(true));
    let mut calls = 0;
    let mut refuse = |_: &str| {
        calls += 1;
        Err(ERR_NO_MEMORY)
    };
    assert!(matches!(
        normalize(
            "battery",
            wire::Record::SettingSpec(wire::SettingSpec {
                name: "../escape",
                ..spec(&default)
            }),
            &mut refuse
        ),
        Err(ERR_INVALID)
    ));
    assert!(matches!(
        normalize(
            "battery",
            wire::Record::SettingSpec(spec(&default)),
            &mut refuse
        ),
        Err(ERR_NO_MEMORY)
    ));
    assert_eq!(calls, 1);
}

#[test]
fn secret_settings_resources_and_callback_registrations_are_unsupported() {
    let default = encoded(SettingValueRef::Bool(true));
    for record in [
        wire::Record::SettingSpec(wire::SettingSpec {
            flags: SECRET,
            ..spec(&default)
        }),
        wire::Record::ResourceHyphenation(wire::ResourceHyphenation {
            language: "en",
            path: "data/en.pat",
        }),
        wire::Record::Registration(wire::Registration::Draw { token: 1 }),
    ] {
        assert!(matches!(
            normalize("fixture", record, &mut checked),
            Err(ERR_UNSUPPORTED)
        ));
    }
}

#[test]
fn production_mapping_uses_checked_id_creation() {
    let default = encoded(SettingValueRef::Bool(true));
    let value = from_record("battery", wire::Record::SettingSpec(spec(&default))).unwrap();
    let Value::Setting(value) = value else {
        panic!("expected a setting")
    };
    assert_eq!(value.id.as_str(), "battery.enabled");
    assert_eq!(checked_id("invalid/id").unwrap_err(), ERR_INVALID);
}
