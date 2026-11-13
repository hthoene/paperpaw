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
    let mut source = checked;
    let value = from_record(
        "battery",
        wire::Record::SettingSpec(spec(&default)),
        &mut source,
    )
    .unwrap();
    let Value::Setting(value) = value else {
        panic!("expected a setting")
    };
    assert_eq!(value.id.as_str(), "battery.enabled");
    assert_eq!(checked_id("invalid/id").unwrap_err(), ERR_INVALID);
}

/// A linked plugin's read-only data, as the image `register_linked` lends.
static IMAGE: [u8; 36] = *b"battery.enabledbattery.enabled.helpX";
/// Its id table, sorted by text, as `fixed_ids_source` makes it.
static IDS: [Id; 3] = [
    Id::new_static("battery"),
    Id::new_static("battery.enabled"),
    Id::new_static("power.group"),
];

fn image_text(range: core::ops::Range<usize>) -> &'static str {
    core::str::from_utf8(&IMAGE[range]).unwrap()
}

/// Whether `text` is the bytes of the image, not a copy of them.
fn in_image(text: &str) -> bool {
    IMAGE.as_ptr_range().contains(&text.as_ptr())
}

#[test]
fn kept_ids_and_texts_in_the_image_are_not_copied() {
    let default = encoded(SettingValueRef::Bool(true));
    let mut record = spec(&default);
    record.title = image_text(0..15);
    record.help = image_text(15..35);
    let mut kept = Kept {
        ids: &IDS,
        image: &IMAGE,
        titles: &[],
    };
    let Value::Setting(value) =
        from_record("battery", wire::Record::SettingSpec(record), &mut kept).unwrap()
    else {
        panic!("expected a setting")
    };
    // The table's ids: kept in the program (an owned id has no static text).
    assert_eq!(value.id.clone().static_str(), "battery.enabled");
    assert_eq!(value.group.clone().static_str(), "power.group");
    // The texts are the image's own bytes.
    assert!(in_image(value.title.as_str()));
    assert_eq!(value.title.as_str(), "battery.enabled");
    let help = value.description.unwrap();
    assert!(in_image(help.as_str()));
    assert_eq!(help.as_str(), "battery.enabled.help");
}

#[test]
fn ids_and_texts_kept_nowhere_are_copied() {
    let default = encoded(SettingValueRef::Text("on"));
    let mut record = spec(&default);
    record.kind = 2;
    record.choices = "on\noff";
    record.name = "other";
    let mut kept = Kept {
        ids: &IDS,
        image: &IMAGE,
        titles: &[],
    };
    let Value::Setting(value) =
        from_record("battery", wire::Record::SettingSpec(record), &mut kept).unwrap()
    else {
        panic!("expected a setting")
    };
    assert_eq!(value.id.as_str(), "battery.other");
    assert_eq!(
        value.id.clone().static_str(),
        "",
        "not in the table: a copy"
    );
    assert!(!in_image(value.title.as_str()));
    assert_eq!(value.title.as_str(), "battery.enabled");
    let SettingKind::Choice(Choices::Owned(choices)) = &value.kind else {
        panic!("expected owned choices")
    };
    assert!(choices.iter().all(|choice| matches!(choice, Cow::Owned(_))));
    // The image's last bytes are still its own; the empty text is no copy.
    assert!(matches!(
        kept.text(image_text(35..36)).unwrap(),
        Cow::Borrowed("X")
    ));
    assert!(matches!(kept.text("").unwrap(), Cow::Borrowed("")));
    // An id the table does not hold is still checked.
    assert_eq!(kept.id("Not/An/Id").unwrap_err(), ERR_INVALID);
}

/// The titles table of a linked plugin: one action of it.
static TITLES: [paperpaw_sdk::TitleEntry; 1] = [paperpaw_sdk::TitleEntry::new(
    Id::new_static("battery.open"),
    "battery.open-title",
)];

fn action(title: &str) -> wire::ActionSpec<'_> {
    wire::ActionSpec {
        name: "open",
        title,
        token: 4,
        flags: 0,
    }
}

#[test]
fn an_action_named_as_in_the_titles_table_keeps_its_entry() {
    let mut kept = Kept {
        ids: &IDS,
        image: &IMAGE,
        titles: &TITLES,
    };
    let Value::Action(same) = from_record(
        "battery",
        wire::Record::ActionSpec(action("battery.open-title")),
        &mut kept,
    )
    .unwrap() else {
        panic!("expected an action")
    };
    // The very entry: no row of its own (#248, B).
    assert!(core::ptr::eq(same.kept.unwrap(), &TITLES[0]));
    // Its title is the entry's too, not a copy made and dropped (#1372).
    assert!(matches!(&same.title, Cow::Borrowed(title) if core::ptr::eq(*title, TITLES[0].title)));
    assert_eq!((same.id.as_str(), same.token), ("battery.open", 4));
    // Another title: a copy, whatever the id.
    let Value::Action(other) = from_record(
        "battery",
        wire::Record::ActionSpec(action("battery.other-title")),
        &mut kept,
    )
    .unwrap() else {
        panic!("expected an action")
    };
    assert!(other.kept.is_none());
    assert_eq!(other.title, "battery.other-title");
}

/// A copy of its own the heap refuses: `ERR_NO_MEMORY`, no panic; a kept
/// entry needs no heap at all (#248, B; Code Reviewer, #1372).
#[test]
fn a_title_the_heap_has_no_room_to_copy_is_refused() {
    use crate::native_host::shell::tests::FAIL_NTH;
    FAIL_NTH.with(|n| n.set(1));
    let copy = titled(
        None,
        Id::new_static("battery.open"),
        Cow::Borrowed("battery.open-title"),
    );
    let kept = titled(
        Some(&TITLES[0]),
        Id::new_static("battery.open"),
        Cow::Borrowed("battery.open-title"),
    );
    let left = FAIL_NTH.with(|n| n.replace(0));
    assert_eq!(copy.unwrap_err(), ERR_NO_MEMORY);
    assert!(kept.unwrap().is_kept());
    // The refused allocation was the copy's; the kept entry asked for none.
    assert_eq!(left, 0);
}
