//! Settings: values parsed from and written to configuration text, kinds
//! and choices, specs kept in the program and changed later, scopes,
//! previews and placements, presets, and the sorted map they are kept in.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::borrow::Cow;
use std::collections::BTreeMap;

use paperpaw_sdk::builtin::{groups, settings::SCOPE_ONLY};
use paperpaw_sdk::collections::VecMap;
use paperpaw_sdk::presets::{self, Preset, Presets};
use paperpaw_sdk::{
    Choices, Command, Id, Level, RegisterError, Setting, SettingError, SettingExtra,
    SettingGroupSpec, SettingKind, SettingSpec, SettingValue, Settings, Text,
};
use proptest::prelude::*;

const SIZE: Setting<i64> = Setting::new("a.size");
const ON: Setting<bool> = Setting::new("a.on");
const MODE: Setting<Text> = Setting::new("a.mode");
const NAME: Setting<Text> = Setting::new("a.name");

fn text(value: &'static str) -> SettingValue {
    SettingValue::Text(Cow::Borrowed(value))
}

fn kind() -> impl Strategy<Value = SettingKind> {
    prop_oneof![
        Just(SettingKind::Bool),
        (any::<i64>(), any::<i64>()).prop_map(|(a, b)| SettingKind::Int {
            min: a.min(b),
            max: a.max(b)
        }),
        Just(SettingKind::Choice(Choices::Fixed(&["x.a", "x.b", "x c"]))),
        Just(SettingKind::Text),
        Just(SettingKind::Language),
        Just(SettingKind::List),
    ]
}

fn value() -> impl Strategy<Value = SettingValue> {
    prop_oneof![
        any::<bool>().prop_map(SettingValue::Bool),
        any::<i64>().prop_map(SettingValue::Int),
        "\\PC{0,12}|x\\.a|x c|[a-z]{2}(-[A-Z]{2})?".prop_map(|t| SettingValue::Text(Cow::Owned(t))),
    ]
}

/// Settings with a size, a switch, a choice and a text.
fn some_settings() -> Settings {
    let mut s = Settings::new();
    s.define(
        SettingSpec::number(
            SIZE.static_str(),
            "a.size",
            1,
            9,
            5,
            paperpaw_sdk::builtin::groups::OTHER,
        )
        .scoped(),
    )
    .unwrap();
    s.define(SettingSpec::switch(
        ON.static_str(),
        "a.on",
        true,
        paperpaw_sdk::builtin::groups::OTHER,
    ))
    .unwrap();
    s.define(SettingSpec::choice(
        MODE.static_str(),
        "a.mode",
        &["a.light", "a.dark"],
        "a.light",
        paperpaw_sdk::builtin::groups::OTHER,
    ))
    .unwrap();
    s.define(SettingSpec::text(
        NAME.static_str(),
        "a.name",
        "",
        paperpaw_sdk::builtin::groups::OTHER,
    ))
    .unwrap();
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// A value a kind accepts is written as configuration text and read
    /// back as itself; text a kind reads is a value it accepts.
    #[test]
    fn values_round_trip_through_configuration_text(kind in kind(), value in value(), raw in "\\PC{0,12}") {
        if kind.accepts(&value) {
            prop_assert_eq!(kind.parse(&value.to_config_text()), Some(value));
        }
        if let Some(read) = kind.parse(&raw) {
            prop_assert!(kind.accepts(&read));
        }
    }

    /// Values are kept as set, by their kind; values that do not fit
    /// leave the setting as it was.
    #[test]
    fn set_values_are_read_back(size in any::<i64>(), name in "\\PC{0,20}") {
        let mut s = some_settings();
        let before = s.int(SIZE.as_str());
        match s.set(SIZE.as_str(), SettingValue::Int(size)) {
            Ok(()) => prop_assert_eq!(s.read(&SIZE), size),
            Err(error) => {
                prop_assert_eq!(error, SettingError::InvalidValue);
                prop_assert!(!(1..=9).contains(&size));
                prop_assert_eq!(s.int(SIZE.as_str()), before);
            }
        }
        s.set_from_text(NAME.as_str(), &name).unwrap();
        prop_assert_eq!(s.read(&NAME), name.as_str());
    }

    /// The sorted map behaves as a `BTreeMap` under any changes.
    #[test]
    fn vec_map_behaves_as_a_btree_map(
        ops in proptest::collection::vec((0u8..4, 0u8..20, any::<u16>()), 0..60)
    ) {
        let mut map: VecMap<u8, u16> = VecMap::new();
        let mut model: BTreeMap<u8, u16> = BTreeMap::new();
        for (op, key, value) in ops {
            match op {
                0 => prop_assert_eq!(map.insert(key, value), model.insert(key, value)),
                1 => prop_assert_eq!(map.remove(&key), model.remove(&key)),
                2 => {
                    let v = map.get_or_insert_with(key, || value);
                    *v = v.wrapping_add(1);
                    let v = model.entry(key).or_insert(value);
                    *v = v.wrapping_add(1);
                }
                _ => {
                    if let Some(v) = map.get_mut(&key) {
                        *v = v.wrapping_mul(3);
                    }
                    if let Some(v) = model.get_mut(&key) {
                        *v = v.wrapping_mul(3);
                    }
                }
            }
            prop_assert_eq!(map.get(&key), model.get(&key));
            prop_assert_eq!(map.contains_key(&key), model.contains_key(&key));
        }
        prop_assert_eq!(map.len(), model.len());
        prop_assert_eq!(map.is_empty(), model.is_empty());
        prop_assert!(map.iter().map(|(k, v)| (*k, *v)).eq(model.iter().map(|(k, v)| (*k, *v))));
        prop_assert!(map.keys().rev().eq(model.keys().rev()));
        for v in map.values_mut() {
            *v = 0;
        }
        prop_assert!(map.values().all(|v| *v == 0));
    }
}

#[test]
fn switches_read_the_words_configuration_files_use() {
    for (word, value) in [
        ("true", true),
        ("yes", true),
        ("on", true),
        ("1", true),
        ("false", false),
        ("no", false),
        ("off", false),
        ("0", false),
    ] {
        assert_eq!(
            SettingKind::Bool.parse(word),
            Some(SettingValue::Bool(value))
        );
    }
    for wrong in ["", "True", "ON", " on", "2", "y"] {
        assert_eq!(SettingKind::Bool.parse(wrong), None, "{wrong:?}");
    }
    assert_eq!(SettingValue::Bool(true).to_config_text(), "on");
    assert_eq!(SettingValue::Bool(false).to_config_text(), "off");
}

#[test]
fn numbers_hold_their_range_and_width() {
    let kind = SettingKind::Int { min: -5, max: 5 };
    assert_eq!(kind.parse("-5"), Some(SettingValue::Int(-5)));
    assert_eq!(kind.parse("+5"), Some(SettingValue::Int(5)));
    for wrong in ["6", "-6", "5.0", " 5", "five", "", "99999999999999999999"] {
        assert_eq!(kind.parse(wrong), None, "{wrong:?}");
    }
    let all = SettingKind::Int {
        min: i64::MIN,
        max: i64::MAX,
    };
    assert_eq!(
        all.parse(&i64::MIN.to_string()),
        Some(SettingValue::Int(i64::MIN))
    );
    // A range that holds nothing accepts nothing.
    let none = SettingKind::Int { min: 1, max: 0 };
    assert_eq!(none.parse("0"), None);
    assert_eq!(none.parse("1"), None);
}

#[test]
fn languages_are_tags() {
    for tag in ["en", "de", "pt-BR", "zh-Hans-CN", "auto"] {
        assert!(SettingKind::Language.parse(tag).is_some(), "{tag}");
    }
    for wrong in ["", "de_DE", "ä", "en US", "en/../x"] {
        assert_eq!(SettingKind::Language.parse(wrong), None, "{wrong:?}");
    }
    // Lists and texts take anything.
    assert!(SettingKind::List.parse("a.b, c.d").is_some());
    assert!(SettingKind::Text.parse("").is_some());
    // Kinds take only values of their type.
    assert!(!SettingKind::Text.accepts(&SettingValue::Int(1)));
    assert!(!SettingKind::Bool.accepts(&text("on")));
}

#[test]
fn choices_kept_in_the_program_or_made_at_run_time() {
    let fixed = Choices::from(&["a", "b"][..]);
    let owned: Choices = ["a", "b"].into_iter().map(Cow::Borrowed).collect();
    for choices in [&fixed, &owned] {
        assert_eq!(choices.len(), 2);
        assert!(!choices.is_empty());
        assert_eq!(choices.get(1), Some("b"));
        assert_eq!(choices.get(2), None);
        assert_eq!(choices.position("b"), Some(1));
        assert!(choices.contains("a") && !choices.contains("c"));
        assert_eq!(choices.iter().collect::<Vec<_>>(), ["a", "b"]);
    }
    assert!(Choices::from(Vec::<Cow<'static, str>>::new()).is_empty());
    let kind = SettingKind::Choice(Choices::Fixed(&[]));
    assert_eq!(kind.parse(""), None);
}

#[test]
fn extras_are_kept_in_the_program_or_made_from_data() {
    static PERCENT: SettingExtra = SettingExtra::new().unit("u.percent").step(10).slider();
    static SPECS: [SettingSpec; 1] =
        [
            SettingSpec::number("a.zoom", "a.zoom", 50, 200, 100, groups::OTHER)
                .with_extra(&PERCENT),
        ];
    assert_eq!(SPECS[0].extra().step, 10);
    assert!(SPECS[0].extra().slider);
    // One made at run time, for a setting from data, takes its place.
    let mut owned = PERCENT.clone();
    owned.section = Some("a.section".into());
    let changed = SPECS[0].clone().with_owned_extra(owned);
    let extra = changed.extra();
    assert_eq!((extra.unit.as_deref(), extra.step), (Some("u.percent"), 10));
    assert_eq!(extra.section.as_deref(), Some("a.section"));
    assert!(PERCENT.section.is_none());
    // Steps are at least 1.
    assert_eq!(SettingExtra::new().step(0).step, 1);
    assert_eq!(SettingExtra::new().step(-5).step, 1);
    static NO_STEP: SettingExtra = SettingExtra::new().step(0);
    let spec = SettingSpec::number(
        SIZE.static_str(),
        "",
        0,
        9,
        1,
        paperpaw_sdk::builtin::groups::OTHER,
    )
    .with_extra(&NO_STEP);
    assert_eq!(spec.extra().step, 1);
    assert_eq!(SettingExtra::new().step(i64::MIN).step, 1);
    // Without any: the defaults.
    let plain = SettingSpec::switch(
        ON.static_str(),
        "",
        false,
        paperpaw_sdk::builtin::groups::OTHER,
    );
    assert_eq!(plain.extra(), &SettingExtra::default());
    static EDITED: SettingExtra = SettingExtra::new()
        .editor(Id::new_static("a.edit"))
        .unit("u");
    let editor = plain.with_extra(&EDITED).description("d").advanced();
    assert_eq!(
        editor.extra().editor,
        Some(paperpaw_sdk::Editor::Screen(Id::new_static("a.edit")))
    );
    // A slot's screen edits it, whichever plugin fills the slot.
    static FOLDER: SettingExtra =
        SettingExtra::new().editor_slot(paperpaw_sdk::builtin::slots::CHOOSE_FOLDER);
    let folder = SettingSpec::switch(
        ON.static_str(),
        "",
        false,
        paperpaw_sdk::builtin::groups::OTHER,
    )
    .with_extra(&FOLDER);
    let opens = folder.extra().editor.as_ref().unwrap().open(&folder.id);
    assert_eq!(
        opens,
        paperpaw_sdk::Command::OpenSlot(
            paperpaw_sdk::builtin::slots::CHOOSE_FOLDER,
            paperpaw_sdk::Params::new().with("id", folder.id.as_str().to_string()),
        )
    );
    assert_eq!(editor.level, Level::Advanced);
    let fixed = SettingSpec::switch("a.x", "a.x", true, groups::OTHER)
        .description("help")
        .advanced();
    assert_eq!(fixed.description.as_deref(), Some("help"));
    assert_eq!(fixed.level, Level::Advanced);
}

#[test]
fn fixed_specs_are_copied_before_they_change() {
    static SPECS: [SettingSpec; 2] = [
        SettingSpec::choice(
            "b.face",
            "b.face",
            &["b.plain", "b.ticks"],
            "b.ticks",
            groups::OTHER,
        ),
        SettingSpec::text("b.label", "b.label", "x", groups::OTHER),
    ];
    let mut s = Settings::new();
    s.define(SettingSpec::switch(
        ON.static_str(),
        "",
        false,
        paperpaw_sdk::builtin::groups::OTHER,
    ))
    .unwrap();
    for at in 0..SPECS.len() {
        s.define_fixed(&SPECS, at).unwrap();
    }
    s.define(SettingSpec::number(
        SIZE.static_str(),
        "",
        0,
        9,
        1,
        paperpaw_sdk::builtin::groups::OTHER,
    ))
    .unwrap();
    assert!(matches!(
        s.define_fixed(&SPECS, 0),
        Err(RegisterError::Duplicate { .. })
    ));
    // Choices made at run time replace the fixed ones; the order of the
    // settings stays.
    s.set_choices(
        "b.face",
        vec![Cow::Borrowed("b.new"), Cow::Borrowed("b.ticks")],
    );
    assert_eq!(
        s.set_from_text("b.face", "b.plain"),
        Err(SettingError::InvalidValue)
    );
    s.set_from_text("b.face", "b.new").unwrap();
    let ids: Vec<&str> = s.specs().map(|spec| spec.id.as_str()).collect();
    assert_eq!(ids, ["a.on", "b.face", "b.label", "a.size"]);
    // The program's list is unchanged.
    assert_eq!(
        SPECS[0].kind,
        SettingKind::Choice(Choices::Fixed(&["b.plain", "b.ticks"]))
    );
    // Unknown settings have no choices to replace.
    s.set_choices("b.none", &["x"][..]);
    assert!(s.spec("b.none").is_none());
}

#[test]
fn defaults_chosen_by_a_shell_give_way_to_the_readers_values() {
    let mut s = some_settings();
    s.set_default_from_text(SIZE.as_str(), "7").unwrap();
    assert_eq!(s.read(&SIZE), 7);
    assert!(!s.is_set(SIZE.as_str()));
    s.set(SIZE.as_str(), SettingValue::Int(2)).unwrap();
    s.set_default_from_text(SIZE.as_str(), "8").unwrap();
    assert_eq!(s.read(&SIZE), 2);
    s.reset(SIZE.as_str()).unwrap();
    assert_eq!(s.read(&SIZE), 8);
    assert_eq!(
        s.set_default_from_text(SIZE.as_str(), "99"),
        Err(SettingError::InvalidValue)
    );
    assert_eq!(
        s.set_default_from_text("a.none", "1"),
        Err(SettingError::Unknown)
    );
    assert_eq!(s.reset("a.none"), Err(SettingError::Unknown));
    assert_eq!(SettingError::Unknown.to_string(), "unknown setting");
}

#[test]
fn values_read_by_another_type_are_empty() {
    let s = some_settings();
    assert!(!s.read(&Setting::<bool>::new("a.size")));
    assert_eq!(s.read(&Setting::<i64>::new("a.mode")), 0);
    assert_eq!(s.read(&Setting::<Text>::new("a.on")), "");
    assert_eq!(
        (s.bool("a.mode"), s.int("a.on"), s.text("a.size")),
        (false, 0, "")
    );
    assert_eq!(s.get("a.none"), None);
    assert_eq!(s.global("a.none"), None);
}

#[test]
fn scopes_take_only_scoped_values_that_fit() {
    let mut s = some_settings();
    s.define(
        SettingSpec::switch(
            SCOPE_ONLY.static_str(),
            "",
            false,
            paperpaw_sdk::builtin::groups::OTHER,
        )
        .scoped(),
    )
    .unwrap();
    // No scope: nothing goes there.
    assert_eq!(
        s.set_in_scope(SIZE.as_str(), SettingValue::Int(3)),
        Err(SettingError::Unknown)
    );
    let mut values = VecMap::new();
    values.insert(SIZE.into(), SettingValue::Int(99));
    values.insert(ON.into(), SettingValue::Bool(false));
    values.insert(Id::new_static("a.unknown"), SettingValue::Int(1));
    values.insert(SCOPE_ONLY.into(), SettingValue::Bool(true));
    s.set_scope(Some(values));
    // Out of range, not scoped or unknown: left out.
    let kept: Vec<&str> = s.scope().unwrap().keys().map(Id::as_str).collect();
    assert_eq!(kept, ["core.scope-only"]);
    assert_eq!(
        s.set_in_scope(ON.as_str(), SettingValue::Bool(false)),
        Err(SettingError::InvalidValue)
    );
    assert_eq!(
        s.set_in_scope(SIZE.as_str(), SettingValue::Int(0)),
        Err(SettingError::InvalidValue)
    );
    s.set_in_scope(SIZE.as_str(), SettingValue::Int(4)).unwrap();
    assert!(s.is_scoped(SIZE.as_str()) && !s.is_scoped(SCOPE_ONLY.as_str()));
    // Reset in the scope: the global value shows again.
    s.reset(SIZE.as_str()).unwrap();
    assert_eq!(s.read(&SIZE), 5);
    // Resetting the switch empties the scope.
    s.set_in_scope(SIZE.as_str(), SettingValue::Int(4)).unwrap();
    s.reset(SCOPE_ONLY.as_str()).unwrap();
    assert!(s.scope().unwrap().is_empty());
    assert_eq!(s.read(&SIZE), 5);
}

#[test]
fn previews_and_placements_together() {
    static SPECS: [SettingSpec; 1] = [SettingSpec::number(
        "w.clock.size",
        "",
        1,
        9,
        5,
        groups::OTHER,
    )];
    let mut s = Settings::new();
    s.define_fixed(&SPECS, 0).unwrap();
    s.set("w.clock@2.size", SettingValue::Int(2)).unwrap();
    s.set("w.clock@3.size", SettingValue::Int(3)).unwrap();
    s.set_preview(vec![
        (Id::new_static("w.clock@2.size"), Some(SettingValue::Int(8))),
        (Id::new_static("w.clock.size"), Some(SettingValue::Int(7))),
    ]);
    let in_place = |n: &str| s.with_placement(Some(n), || s.int("w.clock.size"));
    assert_eq!(in_place("w.clock@2"), 8);
    assert_eq!(in_place("w.clock@3"), 3);
    assert_eq!(in_place("w.clock@4"), 7);
    // `None`, or a placement without `@`: the setting itself.
    assert_eq!(s.with_placement(None, || s.int("w.clock.size")), 7);
    assert_eq!(
        s.with_placement(Some("w.clock"), || s.int("w.clock.size")),
        7
    );
    // Placements nest and come back.
    let nested = s.with_placement(Some("w.clock@2"), || {
        let inner = s.with_placement(Some("w.clock@3"), || s.int("w.clock.size"));
        (inner, s.int("w.clock.size"))
    });
    assert_eq!(nested, (3, 8));
    // Placements are told apart by whole numbers.
    s.set("w.clock@30.size", SettingValue::Int(9)).unwrap();
    assert_eq!(s.placement_values("w.clock@3").len(), 1);
    // Not ids: refused.
    assert_eq!(
        s.set("w.clock@x.size", SettingValue::Int(1)),
        Err(SettingError::Unknown)
    );
    assert_eq!(
        s.set("w.clock@.size", SettingValue::Int(1)),
        Err(SettingError::Unknown)
    );
}

#[test]
fn groups_are_registered_once_in_their_order() {
    let mut s = Settings::new();
    let group =
        |id: &'static str, order| SettingGroupSpec::new(Id::new_static(id), "").order(order);
    s.add_group(group("b.g", 0)).unwrap();
    s.add_group(group("a.g", 0)).unwrap();
    s.add_group(group("c.g", -10)).unwrap();
    assert!(matches!(
        s.add_group(group("a.g", 5)),
        Err(RegisterError::Duplicate { .. })
    ));
    let ids: Vec<&str> = s.groups().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, ["c.g", "a.g", "b.g"]);
}

#[test]
fn presets_read_their_lines_as_written() {
    let preset = Preset::new(
        Id::new_static("p.one"),
        Id::new_static("p.kind"),
        "Title",
        "",
        "a.size = 3\n  # comment\nno equals sign\n = empty id\na.name = x = y\n\na.mode=a.dark\r\na.size = 4\n",
    );
    let values: Vec<(&str, &str)> = preset.values().collect();
    assert_eq!(
        values,
        [
            ("a.size", "3"),
            ("", "empty id"),
            ("a.name", "x = y"),
            ("a.mode", "a.dark"),
            ("a.size", "4")
        ]
    );
    // The first of a setting counts.
    assert_eq!(preset.value("a.size"), Some("3"));
    let s = some_settings();
    let changes = preset.changes(&s, &[]);
    // The last of a setting is applied.
    assert!(changes.contains(&(SIZE.into(), Some(SettingValue::Int(4)))));
    assert!(changes.contains(&(NAME.into(), Some(text("x = y")))));
    assert!(changes.contains(&(MODE.into(), Some(text("a.dark")))));
    assert_eq!(changes.len(), 3);
}

#[test]
fn presets_set_placements_and_leave_out_what_does_not_fit() {
    static SPECS: [SettingSpec; 1] = [SettingSpec::number(
        "w.clock.size",
        "",
        1,
        9,
        5,
        groups::OTHER,
    )];
    let mut s = Settings::new();
    s.define_fixed(&SPECS, 0).unwrap();
    let preset = Preset::new(
        Id::new_static("p.one"),
        Id::new_static("p.kind"),
        "",
        "",
        "w.clock@2.size = 7\nw.clock@3.size = 70\nw.other.size = 1\nW.CLOCK.SIZE = 2",
    );
    let changes = preset.changes(&s, &[Id::new_static("w.clock.size")]);
    assert_eq!(
        changes,
        [
            (Id::new_static("w.clock.size"), None),
            (Id::new_static("w.clock@2.size"), Some(SettingValue::Int(7))),
        ]
    );
    let commands = presets::commands(&s, &changes);
    assert_eq!(
        commands,
        [Command::SetSetting(
            Id::new_static("w.clock@2.size"),
            SettingValue::Int(7)
        )]
    );
    s.set("w.clock@2.size", SettingValue::Int(7)).unwrap();
    assert!(presets::applied(&s, &changes));
}

#[test]
fn presets_of_plugins_come_and_go() {
    static FIXED: [Preset; 1] = [Preset::fixed("p.fixed", "k.a", "", "", "")];
    let mut presets = Presets::new();
    presets.add_fixed(&FIXED);
    presets.add(Preset::new(
        Id::new_static("p.owned"),
        Id::new_static("k.a"),
        "",
        "",
        "",
    ));
    presets.add(Preset::new(
        Id::new_static("q.owned"),
        Id::new_static("k.b"),
        "",
        "",
        "",
    ));
    assert_eq!(presets.of_kind("k.a").count(), 2);
    presets.remove_plugin("p");
    let ids: Vec<&str> = presets.iter().map(|p| p.id.as_str()).collect();
    // What the program keeps stays.
    assert_eq!(ids, ["p.fixed", "q.owned"]);
    assert!(presets.get("p.owned").is_none() && presets.get("q.owned").is_some());
    assert_eq!(presets.of_kind("k.none").count(), 0);
}

#[test]
fn vec_maps_collect_with_the_last_value_winning() {
    let map: VecMap<&str, u8> = [("b", 1), ("a", 2), ("b", 3)].into_iter().collect();
    assert_eq!(map.iter().collect::<Vec<_>>(), [(&"a", &2), (&"b", &3)]);
    assert_eq!(format!("{map:?}"), "{\"a\": 2, \"b\": 3}");
    let mut map = map;
    map.clear();
    assert!(map.is_empty());
}

/// Specs are found by places of 14 bits, the two above marking those kept
/// in the program and those a plugin describes: the 16,385th setting built
/// at run time is refused instead of taking the place of the first of
/// those, and the ones before it are found as they were.
#[test]
fn more_than_16384_settings_are_refused_not_mixed_up() {
    let mut s = Settings::new();
    let define = |s: &mut Settings, i: u32| {
        let id = Id::parse(&format!("a.s{i}")).unwrap();
        s.define(SettingSpec::new(
            id,
            "",
            SettingKind::Bool,
            SettingValue::Bool(false),
            paperpaw_sdk::builtin::groups::OTHER,
        ))
    };
    for i in 0..16_384u32 {
        define(&mut s, i).unwrap();
    }
    assert!(define(&mut s, 16_384).is_err());
    assert!(s.spec("a.s16384").is_none());
    for i in [0, 1, 16_383] {
        let id = format!("a.s{i}");
        assert_eq!(s.spec(&id).unwrap().id.as_str(), id);
    }
}

/// A setting kept in the program is copied before it is changed; with no
/// place left for the copy, that is said: an error that names it, and
/// `set_choices` telling it did nothing.
#[test]
fn a_full_store_says_so_when_a_fixed_setting_is_changed() {
    static LOOK: SettingSpec = SettingSpec::choice(
        "a.look",
        "",
        &["a.plain", "a.bold"],
        "a.plain",
        groups::DISPLAY,
    );
    let mut s = Settings::new();
    s.define_fixed(core::slice::from_ref(&LOOK), 0).unwrap();
    for i in 0..16_384u32 {
        let id = Id::parse(&format!("a.s{i}")).unwrap();
        s.define(SettingSpec::new(
            id,
            "",
            SettingKind::Bool,
            SettingValue::Bool(false),
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    }
    assert_eq!(
        s.set_default_from_text("a.look", "a.bold"),
        Err(SettingError::Full)
    );
    assert!(!s.set_choices("a.look", &["a.thin"][..]));
    assert!(!s.set_choices("a.none", &["a.thin"][..]));
    assert_eq!(
        SettingError::Full.to_string(),
        "no place left to change this setting"
    );
    // The fixed one is as it was.
    let spec = s.spec("a.look").unwrap();
    assert_eq!(spec.default, SettingValue::Text(Cow::Borrowed("a.plain")));
}

/// A setting shown only while another has a value, and one whose values
/// are checked before they are set.
#[test]
fn settings_show_on_a_condition_and_refuse_what_their_check_refuses() {
    fn small(_: &Settings, value: &SettingValue) -> Result<(), &'static str> {
        match value {
            SettingValue::Int(n) if *n > 5 => Err("a.too-large"),
            _ => Ok(()),
        }
    }
    static WHILE_ON: SettingExtra = SettingExtra::new().shown_if(Id::new_static("a.on"), "true");
    static CHECKED: SettingExtra = SettingExtra::new().check(small);
    let mut s = Settings::new();
    s.define(SettingSpec::switch(
        ON.static_str(),
        "a.on",
        false,
        paperpaw_sdk::builtin::groups::OTHER,
    ))
    .unwrap();
    s.define(
        SettingSpec::text(
            NAME.static_str(),
            "a.name",
            "",
            paperpaw_sdk::builtin::groups::OTHER,
        )
        .with_extra(&WHILE_ON),
    )
    .unwrap();
    s.define(
        SettingSpec::number(
            SIZE.static_str(),
            "a.size",
            1,
            9,
            3,
            paperpaw_sdk::builtin::groups::OTHER,
        )
        .with_extra(&CHECKED),
    )
    .unwrap();
    let name = s.spec("a.name").unwrap().clone();
    assert!(!s.shows(&name));
    // Screens listing settings make themselves anew when `a.on` changes.
    assert!(s.governs("a.on"));
    assert!(!s.governs("a.size"));
    s.set("a.on", SettingValue::Bool(true)).unwrap();
    assert!(s.shows(&name));
    // Settings without a condition always show.
    assert!(s.shows(&s.spec("a.on").unwrap().clone()));
    assert_eq!(
        s.set("a.size", SettingValue::Int(7)),
        Err(SettingError::Refused("a.too-large"))
    );
    assert_eq!(s.int("a.size"), 3);
    s.set("a.size", SettingValue::Int(4)).unwrap();
    assert_eq!(s.int("a.size"), 4);
}

/// The reader's own (an account, a folder) and every secret are left as
/// they are by the resets of the settings pages; the rest is not.

#[test]
fn accounts_folders_and_secrets_are_kept_on_reset() {
    static SECRET: SettingExtra = SettingExtra::new().secret();
    static FIXED: [SettingSpec; 3] = [
        SettingSpec::text("a.server", "a.server", "", groups::OTHER).kept_on_reset(),
        SettingSpec::text("a.key", "a.key", "", groups::OTHER).with_extra(&SECRET),
        SettingSpec::number("a.count", "a.count", 0, 9, 1, groups::OTHER),
    ];
    let kept: Vec<bool> = FIXED.iter().map(SettingSpec::is_kept_on_reset).collect();
    assert_eq!(kept, [true, true, false]);
    let built = SettingSpec::text(
        NAME.static_str(),
        "a.name",
        "",
        paperpaw_sdk::builtin::groups::OTHER,
    )
    .kept_on_reset();
    assert!(built.is_kept_on_reset() && built.kept_on_reset);
    assert!(
        SettingSpec::text(
            NAME.static_str(),
            "a.name",
            "",
            paperpaw_sdk::builtin::groups::OTHER
        )
        .with_extra(&SECRET)
        .is_kept_on_reset()
    );
    assert!(
        !SettingSpec::text(
            NAME.static_str(),
            "a.name",
            "",
            paperpaw_sdk::builtin::groups::OTHER
        )
        .is_kept_on_reset()
    );
}
