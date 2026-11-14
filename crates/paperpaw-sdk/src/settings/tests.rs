use super::*;

#[test]
fn a_cause_of_a_later_build_says_its_code() {
    assert_eq!(
        alloc::format!("{}", SettingError::Other(7)),
        "setting error 7"
    );
    // A data file's: refused as not as the format says.
    assert_eq!(
        crate::StorageError::from(crate::data::Error::Other(7)),
        crate::StorageError::InvalidData
    );
}

#[test]
fn a_setting_follows_another_until_set_itself() {
    static FOLLOWS: SettingExtra = SettingExtra::new().follows(Id::new_static("a.sides"));
    let mut s = Settings::new();
    let int = |id: &'static str| {
        SettingSpec::new(
            Id::new_static(id),
            id,
            SettingKind::Int { min: 0, max: 99 },
            SettingValue::Int(4),
            crate::builtin::groups::OTHER,
        )
        .scoped()
    };
    s.define(int("a.sides")).unwrap();
    s.define(int("a.ends").with_extra(&FOLLOWS)).unwrap();
    s.define(
        SettingSpec::new(
            Id::from(crate::builtin::settings::SCOPE_ONLY),
            "x",
            SettingKind::Bool,
            SettingValue::Bool(false),
            crate::builtin::groups::OTHER,
        )
        .scoped(),
    )
    .unwrap();
    s.set("a.sides", SettingValue::Int(8)).unwrap();
    assert_eq!(
        (s.int("a.ends"), s.global("a.ends")),
        (8, Some(&SettingValue::Int(8)))
    );
    // A book's own sides, and so its ends.
    let mut values = VecMap::new();
    values.insert(Id::new_static("a.sides"), SettingValue::Int(2));
    values.insert(
        crate::builtin::settings::SCOPE_ONLY.into(),
        SettingValue::Bool(true),
    );
    s.set_scope(Some(values));
    assert_eq!(s.int("a.ends"), 2);
    s.set_scope(None);
    // Set itself, it keeps its own.
    s.set("a.ends", SettingValue::Int(4)).unwrap();
    assert_eq!(s.int("a.ends"), 4);
    s.set("a.sides", SettingValue::Int(12)).unwrap();
    assert_eq!(s.int("a.ends"), 4);
}

#[test]
fn a_power_mode_wears_values_and_limits_and_takes_them_off() {
    let mut s = Settings::new();
    for id in ["a.sleep", "a.light"] {
        let spec = SettingSpec::new(
            Id::new_static(id),
            id,
            SettingKind::Int { min: 0, max: 100 },
            SettingValue::Int(10),
            crate::builtin::groups::OTHER,
        );
        s.define(spec).unwrap();
    }
    s.set("a.light", SettingValue::Int(80)).unwrap();
    let touched = s.wear(
        alloc::vec![
            (Id::new_static("a.sleep"), SettingValue::Int(5)),
            (Id::new_static("a.unknown"), SettingValue::Int(1)),
        ],
        alloc::vec![(Id::new_static("a.light"), 50)],
    );
    assert_eq!(
        touched,
        [Id::new_static("a.light"), Id::new_static("a.sleep")]
    );
    assert_eq!(s.int("a.sleep"), 5);
    assert_eq!(s.int("a.light"), 50, "kept below the limit");
    // The reader's change wins over a mode's limit, too.
    s.set("a.light", SettingValue::Int(90)).unwrap();
    assert_eq!(s.int("a.light"), 90, "the reader's own");
    // The reader's change wins over the mode's value.
    s.set("a.sleep", SettingValue::Int(20)).unwrap();
    assert_eq!(s.int("a.sleep"), 20);
    assert!(!s.is_worn("a.light") && !s.is_worn("a.sleep"));
    assert_eq!(
        s.released(),
        [Id::new_static("a.light"), Id::new_static("a.sleep")]
    );
    // Taken off: the reader's own values, nothing written by the mode.
    s.wear(alloc::vec![], alloc::vec![]);
    assert_eq!((s.int("a.sleep"), s.int("a.light")), (20, 90));
    assert!(s.released().is_empty());
}

#[test]
fn scopes_override_scoped_settings_while_switched_on() {
    use crate::builtin::settings::SCOPE_ONLY;
    let mut s = Settings::new();
    let int = |id: &'static str, scoped: bool| {
        let spec = SettingSpec::new(
            Id::new_static(id),
            id,
            SettingKind::Int { min: 0, max: 99 },
            SettingValue::Int(10),
            crate::builtin::groups::OTHER,
        );
        if scoped { spec.scoped() } else { spec }
    };
    s.define(int("a.size", true)).unwrap();
    s.define(int("a.other", false)).unwrap();
    s.define(
        SettingSpec::new(
            Id::from(SCOPE_ONLY),
            "x",
            SettingKind::Bool,
            SettingValue::Bool(false),
            crate::builtin::groups::OTHER,
        )
        .scoped(),
    )
    .unwrap();
    s.set("a.size", SettingValue::Int(12)).unwrap();
    assert!(!s.goes_to_scope("a.size"));

    // A book with its own size, switched on.
    let mut values = VecMap::new();
    values.insert(Id::new_static("a.size"), SettingValue::Int(20));
    values.insert(Id::new_static("a.other"), SettingValue::Int(30));
    values.insert(SCOPE_ONLY.into(), SettingValue::Bool(true));
    s.set_scope(Some(values));
    assert_eq!(s.int("a.size"), 20);
    assert!(s.is_scoped("a.size"));
    // Not scoped: the book cannot change it.
    assert_eq!(s.int("a.other"), 10);
    assert!(s.goes_to_scope("a.size") && !s.goes_to_scope("a.other"));
    s.set_in_scope("a.size", SettingValue::Int(25)).unwrap();
    assert_eq!(
        (s.int("a.size"), s.global("a.size")),
        (25, Some(&SettingValue::Int(12)))
    );

    // Switched off: the general value again, the book's dropped.
    s.set_in_scope(SCOPE_ONLY.as_str(), SettingValue::Bool(false))
        .unwrap();
    assert_eq!(s.int("a.size"), 12);
    assert!(s.scope().unwrap().is_empty());
    // Only the switch goes to the scope now.
    assert!(!s.goes_to_scope("a.size") && s.goes_to_scope(SCOPE_ONLY.as_str()));
    s.set_scope(None);
    assert_eq!(s.int("a.size"), 12);
    assert!(!s.goes_to_scope(SCOPE_ONLY.as_str()));
}

fn settings() -> Settings {
    let mut settings = Settings::new();
    settings
        .define(SettingSpec::new(
            Id::new_static("test.count"),
            "test.count",
            SettingKind::Int { min: 1, max: 10 },
            SettingValue::Int(3),
            crate::builtin::groups::OTHER,
        ))
        .unwrap();
    settings
        .define(SettingSpec::new(
            Id::new_static("test.mode"),
            "test.mode",
            SettingKind::Choice(crate::settings::Choices::Fixed(&["a", "b"])),
            SettingValue::Text(Cow::Borrowed("a")),
            crate::builtin::groups::OTHER,
        ))
        .unwrap();
    settings
}

#[test]
fn defaults_and_updates() {
    let mut s = settings();
    assert_eq!(s.int("test.count"), 3);
    s.set_from_text("test.count", "7").unwrap();
    assert_eq!(s.int("test.count"), 7);
    assert_eq!(s.text("test.mode"), "a");
    s.set_from_text("test.mode", "b").unwrap();
    assert_eq!(s.text("test.mode"), "b");
}

#[test]
fn rejects_invalid_values() {
    let mut s = settings();
    assert_eq!(
        s.set_from_text("test.count", "11"),
        Err(SettingError::InvalidValue)
    );
    assert_eq!(
        s.set_from_text("test.mode", "c"),
        Err(SettingError::InvalidValue)
    );
    assert_eq!(
        s.set("test.count", SettingValue::Bool(true)),
        Err(SettingError::InvalidValue)
    );
    assert_eq!(
        s.set_from_text("test.nope", "1"),
        Err(SettingError::Unknown)
    );
}

#[test]
fn groups_are_ordered_and_unknown_groups_fall_back() {
    let mut s = settings();
    for (id, order) in [("b.late", 20), ("a.early", 10), ("c.same", 10)] {
        s.add_group(SettingGroupSpec {
            id: Id::parse(id).unwrap(),
            title: Cow::Borrowed(""),
            order,
            page: None,
            parent: None,
        })
        .unwrap();
    }
    let ids: Vec<&str> = s.groups().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, ["a.early", "c.same", "b.late"]);
    let spec = SettingSpec::new(
        Id::new_static("x.y"),
        "",
        SettingKind::Bool,
        SettingValue::Bool(false),
        Id::new_static("missing.group"),
    );
    assert_eq!(s.group_of(&spec), crate::builtin::groups::OTHER);
    assert_eq!(
        s.group_of(&{
            let mut late = spec.clone();
            late.group = Id::new_static("b.late");
            late
        })
        .as_str(),
        "b.late"
    );
}

#[test]
fn rejects_duplicates_and_bad_defaults() {
    let mut s = settings();
    let spec = SettingSpec::new(
        Id::new_static("test.count"),
        "",
        SettingKind::Bool,
        SettingValue::Bool(false),
        crate::builtin::groups::OTHER,
    );
    assert!(matches!(
        s.define(spec.clone()),
        Err(RegisterError::Duplicate { .. })
    ));
    let mut bad = spec;
    bad.id = Id::new_static("test.bad");
    bad.default = SettingValue::Int(1);
    assert!(matches!(s.define(bad), Err(RegisterError::Invalid { .. })));
}

#[test]
fn placements_have_values_of_their_own() {
    let mut s = Settings::new();
    static SPECS: [SettingSpec; 1] = [SettingSpec::switch(
        "widgets.clock.date",
        "widgets.clock.date",
        true,
        Id::new_static("widgets.clock"),
    )];
    s.define_fixed(&SPECS, 0).unwrap();
    // Unset, a placement has the setting's value.
    assert!(s.bool("widgets.clock@2.date"));
    s.set("widgets.clock@2.date", SettingValue::Bool(false))
        .unwrap();
    assert!(!s.bool("widgets.clock@2.date"));
    assert!(s.bool("widgets.clock.date"));
    // Its spec is the setting's: its kind holds.
    assert_eq!(
        s.set("widgets.clock@2.date", SettingValue::Int(1)),
        Err(SettingError::InvalidValue)
    );
    assert_eq!(
        s.spec("widgets.clock@2.date").unwrap().id.as_str(),
        "widgets.clock.date"
    );
    // While the placement is shown, it is the setting's value.
    let date = s.with_placement(Some("widgets.clock@2"), || s.bool("widgets.clock.date"));
    assert!(!date);
    assert!(s.with_placement(Some("widgets.clock@3"), || s.bool("widgets.clock.date")));
    assert!(s.bool("widgets.clock.date"));
    // Read from configuration text, and forgotten again.
    s.set_from_text("widgets.clock@3.date", "off").unwrap();
    assert_eq!(s.placement_values("widgets.clock@3").len(), 1);
    s.reset("widgets.clock@3.date").unwrap();
    assert!(s.bool("widgets.clock@3.date"));
    assert!(s.placement_values("widgets.clock@3").is_empty());
    // Another widget's setting is not the placement's.
    assert_eq!(
        s.set("widgets.other@2.date", SettingValue::Bool(true)),
        Err(SettingError::Unknown)
    );
}

#[test]
fn previews_show_values_without_keeping_them() {
    let mut s = Settings::new();
    static SPECS: [SettingSpec; 2] = [
        SettingSpec::switch(
            "widgets.clock.date",
            "widgets.clock.date",
            true,
            Id::new_static("widgets.clock"),
        ),
        SettingSpec::number(
            "widgets.clock.size",
            "",
            1,
            9,
            5,
            Id::new_static("widgets.clock"),
        ),
    ];
    s.define_fixed(&SPECS, 0).unwrap();
    s.define_fixed(&SPECS, 1).unwrap();
    s.set("widgets.clock.size", SettingValue::Int(7)).unwrap();
    s.set("widgets.clock@2.date", SettingValue::Bool(false))
        .unwrap();
    s.set_preview(alloc::vec![
        (
            Id::new_static("widgets.clock.date"),
            Some(SettingValue::Bool(false))
        ),
        (Id::new_static("widgets.clock.size"), None),
        (Id::new_static("widgets.clock@2.date"), None),
        (
            Id::new_static("widgets.clock@3.date"),
            Some(SettingValue::Bool(true))
        ),
        // Unknown, or not fitting: left out.
        (Id::new_static("widgets.none"), None),
        (
            Id::new_static("widgets.clock@4.date"),
            Some(SettingValue::Int(1))
        ),
    ]);
    assert!(s.previewing());
    // In the preview set, also a value that stays as kept; not those left out.
    assert!(s.is_previewed("widgets.clock.date") && s.is_previewed("widgets.clock.size"));
    assert!(!s.is_previewed("widgets.none") && !s.is_previewed("widgets.clock@4.date"));
    assert!(!s.bool("widgets.clock.date"));
    // Not set, while shown: the default, not the value kept.
    assert_eq!(s.int("widgets.clock.size"), 5);
    assert_eq!(s.global("widgets.clock.size"), Some(&SettingValue::Int(7)));
    // A placement not set shows the setting's (shown) value.
    assert!(!s.bool("widgets.clock@2.date"));
    assert!(s.with_placement(Some("widgets.clock@3"), || s.bool("widgets.clock.date")));
    assert!(!s.with_placement(Some("widgets.clock@4"), || s.bool("widgets.clock.date")));
    s.set_preview(Vec::new());
    assert!(!s.previewing() && !s.is_previewed("widgets.clock.date"));
    assert_eq!(s.int("widgets.clock.size"), 7);
    assert!(s.bool("widgets.clock.date"));
    assert!(s.is_set("widgets.clock@2.date"));
    assert!(!s.is_set("widgets.clock.date"));
}

#[test]
fn groups_within_groups_know_their_parents() {
    let mut s = settings();
    let (reading, text, other) = (
        Id::new_static("core.group.reading"),
        Id::new_static("core.group.reading-text"),
        Id::new_static("core.group.other"),
    );
    s.add_group(SettingGroupSpec::new(reading.clone(), ""))
        .unwrap();
    s.add_group(SettingGroupSpec::new(text.clone(), "").within(reading.clone()))
        .unwrap();
    assert_eq!(s.parent_of(&text), Some(&reading));
    assert_eq!(s.parent_of(&reading), None);
    assert!(s.within(&text, &reading));
    assert!(s.within(&reading, &reading));
    assert!(!s.within(&reading, &text));
    assert!(!s.within(&text, &other));
}

/// A kind of a later build, in a `static` list: its value kept as text.
#[test]
fn a_setting_of_a_later_kind_keeps_its_value_as_text() {
    static SLIDER: SettingKind = SettingKind::Extended {
        key: "core.slider",
        params: &[("step", "5")],
    };
    let value = SLIDER.parse("35").unwrap();
    assert_eq!(value, SettingValue::Text(Cow::Borrowed("35")));
    assert!(SLIDER.accepts(&value));
    assert!(!SLIDER.accepts(&SettingValue::Int(35)));
}

#[test]
fn a_plugins_settings_and_groups_go_and_the_others_keep_their_places() {
    static LIST: [SettingSpec; 2] = [
        SettingSpec::new(
            Id::new_static("keep.fixed"),
            "keep.fixed",
            SettingKind::Bool,
            SettingValue::Bool(true),
            crate::builtin::groups::OTHER,
        ),
        SettingSpec::new(
            Id::new_static("gone.fixed"),
            "gone.fixed",
            SettingKind::Bool,
            SettingValue::Bool(true),
            crate::builtin::groups::OTHER,
        ),
    ];
    let flag = |id: &'static str| {
        SettingSpec::new(
            Id::new_static(id),
            id,
            SettingKind::Bool,
            SettingValue::Bool(false),
            crate::builtin::groups::OTHER,
        )
    };
    let mut s = Settings::new();
    s.define(flag("keep.first")).unwrap();
    s.define(flag("gone.one")).unwrap();
    s.define_fixed(&LIST, 0).unwrap();
    s.define(flag("keep.second")).unwrap();
    s.define_fixed(&LIST, 1).unwrap();
    s.define(flag("gone.two")).unwrap();
    s.add_group(SettingGroupSpec::new(Id::new_static("gone.group"), "gone"))
        .unwrap();
    s.add_group(SettingGroupSpec::new(Id::new_static("keep.group"), "keep"))
        .unwrap();
    s.remove_plugin("gone");
    for id in ["keep.first", "keep.fixed", "keep.second"] {
        assert!(s.spec(id).is_some(), "{id}");
    }
    for id in ["gone.one", "gone.fixed", "gone.two"] {
        assert!(s.spec(id).is_none(), "{id}");
    }
    let order: Vec<&str> = s.specs.iter().map(|spec| spec.id.as_str()).collect();
    assert_eq!(order, ["keep.first", "keep.fixed", "keep.second"]);
    let groups: Vec<&str> = s.groups().map(|g| g.id.as_str()).collect();
    assert_eq!(groups, ["keep.group"]);
    // The room that is left takes new settings again.
    s.define(flag("gone.three")).unwrap();
    assert!(s.spec("gone.three").is_some());
}

#[test]
fn a_plugin_that_goes_gives_back_the_defaults_it_changed() {
    let flag = |id: &'static str| {
        SettingSpec::new(
            Id::new_static(id),
            id,
            SettingKind::Bool,
            SettingValue::Bool(false),
            crate::builtin::groups::OTHER,
        )
    };
    let mut s = Settings::new();
    s.define(flag("base.flag")).unwrap();
    s.define(flag("base.other")).unwrap();
    s.define(flag("mod.own")).unwrap();
    s.set_default_by("base.flag", "true", Id::new_static("mod"))
        .unwrap();
    s.set_default_by("base.other", "true", Id::new_static("mod"))
        .unwrap();
    s.set_default_by("mod.own", "true", Id::new_static("mod"))
        .unwrap();
    assert_eq!(s.get("base.flag"), Some(&SettingValue::Bool(true)));
    assert_eq!(s.default_from("base.flag").map(Id::as_str), Some("mod"));
    // A second plugin over the first: the default before both comes back.
    s.set_default_by("base.other", "false", Id::new_static("late"))
        .unwrap();
    s.remove_plugin("mod");
    assert_eq!(s.get("base.flag"), Some(&SettingValue::Bool(false)));
    assert!(s.default_from("base.flag").is_none());
    assert_eq!(s.default_from("base.other").map(Id::as_str), Some("late"));
    s.remove_plugin("late");
    assert_eq!(s.get("base.other"), Some(&SettingValue::Bool(false)));
    assert!(s.default_from("base.other").is_none());
    assert!(s.spec("mod.own").is_none());
}

/// `base.n` (an Int, default 0) with `b` (1), then `c` (2) over it.
fn stacked() -> Settings {
    let mut s = Settings::new();
    s.define(SettingSpec::new(
        Id::new_static("base.n"),
        "base.n",
        SettingKind::Int { min: 0, max: 9 },
        SettingValue::Int(0),
        crate::builtin::groups::OTHER,
    ))
    .unwrap();
    s.set_default_by("base.n", "1", Id::new_static("b"))
        .unwrap();
    s.set_default_by("base.n", "2", Id::new_static("c"))
        .unwrap();
    s
}

#[test]
fn a_default_over_a_default_comes_back_when_the_upper_plugin_goes() {
    let mut s = stacked();
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(2)));
    assert_eq!(s.default_from("base.n").map(Id::as_str), Some("c"));
    s.remove_plugin("c");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(1)));
    assert_eq!(s.default_from("base.n").map(Id::as_str), Some("b"));
    s.remove_plugin("b");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(0)));
    assert!(s.default_from("base.n").is_none());
}

#[test]
fn the_lower_plugin_going_first_leaves_the_upper_default_in_force() {
    let mut s = stacked();
    s.remove_plugin("b");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(2)));
    assert_eq!(s.default_from("base.n").map(Id::as_str), Some("c"));
    s.remove_plugin("c");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(0)));
    assert!(s.default_from("base.n").is_none());
}

#[test]
fn three_plugins_going_in_any_order_end_at_the_original_default() {
    let orders: [[&str; 3]; 6] = [
        ["a", "b", "c"],
        ["a", "c", "b"],
        ["b", "a", "c"],
        ["b", "c", "a"],
        ["c", "a", "b"],
        ["c", "b", "a"],
    ];
    for order in orders {
        let mut s = stacked();
        s.set_default_by("base.n", "3", Id::new_static("a"))
            .unwrap();
        let mut gone: Vec<&str> = Vec::new();
        for plugin in order {
            s.remove_plugin(plugin);
            gone.push(plugin);
            // The newest of those left is in force: a=3, b=1, c=2, in the
            // order b, c, a they came.
            let left = ["b", "c", "a"]
                .into_iter()
                .rev()
                .find(|p| !gone.contains(p));
            let want = match left {
                Some("a") => 3,
                Some("b") => 1,
                Some("c") => 2,
                _ => 0,
            };
            assert_eq!(s.get("base.n"), Some(&SettingValue::Int(want)), "{order:?}");
            assert_eq!(s.default_from("base.n").map(Id::as_str), left, "{order:?}");
        }
    }
}

#[test]
fn a_plugin_setting_its_default_again_moves_to_the_top() {
    let mut s = stacked();
    s.set_default_by("base.n", "5", Id::new_static("b"))
        .unwrap();
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(5)));
    // One layer per plugin: c going leaves b's newest value.
    s.remove_plugin("c");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(5)));
    s.remove_plugin("b");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(0)));
}

#[test]
fn a_layer_going_below_the_top_leaves_the_default_in_force() {
    let mut s = stacked();
    // The reader's own value is not touched by either removal.
    s.set("base.n", SettingValue::Int(7)).unwrap();
    s.remove_plugin("b");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(7)));
    assert_eq!(
        s.spec("base.n").map(|spec| &spec.default),
        Some(&SettingValue::Int(2))
    );
    s.remove_plugin("c");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(7)));
    assert_eq!(
        s.spec("base.n").map(|spec| &spec.default),
        Some(&SettingValue::Int(0))
    );
}

#[test]
fn removing_a_plugin_twice_changes_nothing_the_second_time() {
    let mut s = stacked();
    s.remove_plugin("c");
    s.remove_plugin("c");
    assert_eq!(s.get("base.n"), Some(&SettingValue::Int(1)));
    assert_eq!(s.default_from("base.n").map(Id::as_str), Some("b"));
}

#[test]
fn a_layer_value_the_setting_no_longer_accepts_is_skipped() {
    let mut s = stacked();
    // The range narrows to 3..=9, past b's value (1) and the original (0).
    assert!(s.set_range("base.n", 3, 9));
    s.remove_plugin("c");
    let default = s.spec("base.n").map(|spec| spec.default.clone());
    // b's 1 and the original 0 are both out of range: the default stays as it was.
    assert_eq!(default, Some(SettingValue::Int(2)));
}

#[test]
fn room_for_made_settings_ends_at_the_places_there_are() {
    let mut s = Settings::new();
    assert!(s.has_room_for(16_384));
    assert!(!s.has_room_for(16_385));
    assert!(!s.has_room_for(usize::MAX));
    s.define(SettingSpec::new(
        Id::new_static("a.b"),
        "a.b",
        SettingKind::Bool,
        SettingValue::Bool(false),
        crate::builtin::groups::OTHER,
    ))
    .unwrap();
    assert!(s.has_room_for(16_383));
    assert!(!s.has_room_for(16_384));
}
