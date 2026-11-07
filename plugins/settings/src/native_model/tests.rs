use super::*;
use alloc::string::ToString;
use alloc::vec;

fn group(id: &str, parent: &str, order: i32, title: &str) -> Group {
    Group {
        id: id.to_string(),
        parent: parent.to_string(),
        page: String::new(),
        order,
        title: title.to_string(),
    }
}

fn setting(key: &str, group: &str, kind: Kind, flags: u32) -> Setting {
    Setting {
        key: key.to_string(),
        group: group.to_string(),
        kind,
        flags,
        is_default: true,
        in_scope: false,
        title: key.to_string(),
        value: None,
    }
}

const SHOWN: u32 = flag::SHOWN;

#[test]
fn a_setting_is_shown_in_its_group_or_falls_into_other() {
    let groups = vec![group("a.one", "", 0, "One"), group("a.two", "", 0, "Two")];
    let in_one = setting("t.x", "a.one", Kind::Bool, SHOWN);
    let nowhere = setting("t.y", "b.nowhere", Kind::Bool, SHOWN);
    assert!(group_is(&groups, &in_one, "a.one"));
    assert!(!group_is(&groups, &in_one, "a.two"));
    assert!(!group_is(&groups, &in_one, OTHER_GROUP));
    // A group that is not registered shows nothing of its own.
    assert!(!group_is(&groups, &nowhere, "b.nowhere"));
    assert!(group_is(&groups, &nowhere, OTHER_GROUP));
    // The other group's own settings are shown there.
    let other = setting("t.z", OTHER_GROUP, Kind::Bool, SHOWN);
    assert!(group_is(&groups, &other, OTHER_GROUP));
}

#[test]
fn a_part_is_within_its_parent_at_any_depth() {
    let groups = vec![
        group("a", "", 0, "A"),
        group("a.b", "a", 0, "B"),
        group("a.b.c", "a.b", 0, "C"),
        group("loop.x", "loop.y", 0, "X"),
        group("loop.y", "loop.x", 0, "Y"),
    ];
    assert!(within(&groups, "a.b.c", "a"));
    assert!(within(&groups, "a", "a"));
    assert!(!within(&groups, "a", "a.b"));
    // A damaged cycle ends instead of looping.
    assert!(!within(&groups, "loop.x", "a"));
}

#[test]
fn offered_and_visible_follow_the_levels() {
    let groups = vec![group("g", "", 0, "G"), group(DEVELOPER_GROUP, "", 0, "Dev")];
    let cx = Context::default();
    let basic = setting("g.a", "g", Kind::Bool, SHOWN);
    let advanced = setting("g.b", "g", Kind::Bool, SHOWN | flag::ADVANCED);
    let hidden = setting("g.c", "g", Kind::Bool, 0);
    let dev = setting("g.d", DEVELOPER_GROUP, Kind::Bool, SHOWN | flag::ADVANCED);
    assert!(visible(&groups, cx, &basic, None));
    assert!(!visible(&groups, cx, &advanced, None));
    assert!(offered(&groups, cx, &advanced, None));
    assert!(visible(
        &groups,
        Context {
            advanced: true,
            ..cx
        },
        &advanced,
        None
    ));
    assert!(
        !offered(&groups, cx, &hidden, None),
        "its shown_if does not hold"
    );
    // Developer settings: once asked for, and then without the advanced switch.
    assert!(!visible(&groups, cx, &dev, None));
    assert!(visible(
        &groups,
        Context {
            developer: true,
            ..cx
        },
        &dev,
        None
    ));
    // Only this book means nothing outside a scope.
    let only = setting(SCOPE_ONLY, "g", Kind::Bool, SHOWN);
    assert!(!offered(&groups, cx, &only, None));
    assert!(offered(
        &groups,
        Context { scoped: true, ..cx },
        &only,
        None
    ));
    // Nothing to choose.
    let choice = setting("g.e", "g", Kind::Choice, SHOWN);
    assert!(!offered(&groups, cx, &choice, Some(1)));
    assert!(offered(&groups, cx, &choice, Some(2)));
    assert!(offered(&groups, cx, &choice, None));
    // A secret is kept elsewhere.
    let secret = setting("g.f", "g", Kind::Text, SHOWN | flag::SECRET);
    assert!(!offered(&groups, cx, &secret, None));
}

#[test]
fn the_main_page_lists_groups_by_order_then_name() {
    let groups = vec![
        group("g.late", "", 0, "Zeta"),
        group("g.first", "", 5, "Beta"),
        group("g.also", "", 0, "Alpha"),
        group("g.part", "g.first", 0, "Part"),
        group("g.empty", "", 1, "Empty"),
        group("w.clock", "", 2, "Clock"),
    ];
    let holding = vec![
        "g.late".to_string(),
        "g.first".to_string(),
        "g.part".to_string(),
        "g.also".to_string(),
        "w.clock".to_string(),
    ];
    let found = main_groups(&groups, &holding, |id| id.starts_with("w.")).unwrap();
    let ids: Vec<&str> = found.iter().map(|i| groups[*i].id.as_str()).collect();
    // Beta (order 5), then those without an order by name; no part, no empty
    // group, no widget.
    assert_eq!(ids, ["g.first", "g.also", "g.late"]);
    // A group holding only through a part is listed.
    let holding = vec!["g.part".to_string()];
    let found = main_groups(&groups, &holding, |_| false).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(groups[found[0]].id, "g.first");
}

#[test]
fn sections_come_standard_first() {
    use paperpaw_sdk::builtin::group_sections as s;
    let order =
        section_order(["z.own", s::PAGE, "", s::FONT, s::PAGE, "a.own"].into_iter()).unwrap();
    assert_eq!(order, ["", s::FONT, s::PAGE, "z.own", "a.own"]);
}

fn int_detail(min: i64, max: i64, step: i64) -> Detail {
    Detail {
        min,
        max,
        step,
        ..Detail::default()
    }
}

#[test]
fn numbers_step_by_their_step_and_stop_at_their_ends() {
    let d = int_detail(1, 10, 2);
    assert_eq!(stepped(Kind::Int, &Val::Int(5), &d, 1), Some(Val::Int(7)));
    assert_eq!(stepped(Kind::Int, &Val::Int(5), &d, -1), Some(Val::Int(3)));
    assert_eq!(stepped(Kind::Int, &Val::Int(9), &d, 1), Some(Val::Int(10)));
    assert_eq!(stepped(Kind::Int, &Val::Int(10), &d, 1), None);
    assert_eq!(stepped(Kind::Int, &Val::Int(1), &d, -1), None);
    // A step of 0 and 1 are one step.
    assert_eq!(
        stepped(Kind::Int, &Val::Int(5), &int_detail(0, 9, 0), 1),
        Some(Val::Int(6))
    );
    // Overflow stops at the end.
    assert_eq!(
        stepped(
            Kind::Int,
            &Val::Int(i64::MAX),
            &int_detail(0, i64::MAX, 5),
            1
        ),
        None
    );
    assert_eq!(
        stepped(Kind::Bool, &Val::Bool(true), &Detail::default(), 1),
        Some(Val::Bool(false))
    );
    assert_eq!(
        stepped(
            Kind::Text,
            &Val::Text("x".to_string()),
            &Detail::default(),
            1
        ),
        None
    );
}

#[test]
fn a_unit_with_a_count_takes_the_number() {
    assert_eq!(with_unit(5, "").unwrap(), "5");
    assert_eq!(with_unit(5, "{count} s").unwrap(), "5 s");
    assert_eq!(with_unit(-12, "{count} mm").unwrap(), "-12 mm");
    assert_eq!(with_unit(30, "min").unwrap(), "30 min");
    assert_eq!(with_unit(i64::MIN, "").unwrap(), "-9223372036854775808");
}

#[test]
fn placements_rename_the_settings_of_their_group() {
    assert_eq!(
        placed("widgets.clock.face", "widgets.clock", "widgets.clock@2").unwrap(),
        "widgets.clock@2.face"
    );
    assert_eq!(
        placed("other.face", "widgets.clock", "widgets.clock@2"),
        None
    );
    assert_eq!(placed("widgets.clockwork", "widgets.clock", "x@1"), None);
}

#[test]
fn every_word_counts() {
    let find = |text: &str, query: &str| matches(text, &words(query).unwrap()).unwrap();
    assert!(find("Font size of the book's text", "SIZE font"));
    assert!(!find("Font size", "font weight"));
    assert!(find("Schriftgröße", "größe"));
    assert!(find("Ränder", "rand"));
    assert!(find("Schriftgrösse", "grosse"));
    assert!(find("Café", "cafe"));
    assert!(words("   ").unwrap().is_empty());
}

#[test]
fn a_theme_is_in_use_when_its_values_are_set_and_the_rest_is_default() {
    let mut a = setting("look.a", "g", Kind::Choice, SHOWN);
    a.value = Some(Val::Text("x".to_string()));
    a.is_default = false;
    let b = setting("look.b", "g", Kind::Bool, SHOWN);
    let outside = setting("other", "g", Kind::Bool, SHOWN);
    let all = vec![a, b, outside];
    let covered = vec!["look.a".to_string(), "look.b".to_string()];
    let theme = vec![("look.a".to_string(), Some(Val::Text("x".to_string())))];
    assert!(applied(&all, &covered, &theme));
    let other = vec![("look.a".to_string(), Some(Val::Text("y".to_string())))];
    assert!(!applied(&all, &covered, &other));
    // A value a theme resets must be the default.
    let resets = vec![("look.a".to_string(), None)];
    assert!(!applied(&all, &covered, &resets));
    // A covered setting a theme leaves out must be the default.
    assert!(!applied(&all, &covered, &[]));
}
