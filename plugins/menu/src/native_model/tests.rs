use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use alloc::vec;

fn row(item: &str, kind: Kind, flags: u32) -> Row {
    Row {
        item: item.to_string(),
        kind,
        flags,
        title: item.to_string(),
        icon: String::new(),
        badge: String::new(),
        describe: String::new(),
    }
}

fn action(item: &str) -> Row {
    row(item, Kind::Action, flag::ENABLED)
}

fn section(title: &str) -> Row {
    row("", Kind::Section, flag::ENABLED).with_title(title)
}

impl Row {
    fn with_title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }
}

fn items(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|r| r.item.as_str()).collect()
}

fn titles(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|r| r.title.as_str()).collect()
}

#[test]
fn hidden_items_come_from_the_comma_list() {
    assert!(hidden("a, b ,c", "b"));
    assert!(hidden("a,b", "a"));
    assert!(!hidden("ab,c", "b"));
    assert!(!hidden("", "b"));
}

#[test]
fn depth_is_digits_else_zero() {
    assert_eq!(depth_of(Some("3")), 3);
    assert_eq!(depth_of(Some("x")), 0);
    assert_eq!(depth_of(Some("-1")), 0);
    assert_eq!(depth_of(None), 0);
}

#[test]
fn a_heading_with_nothing_below_it_goes() {
    let rows = vec![
        section("A"),
        action("one"),
        section("B"),
        section("C"),
        action("two"),
        section("D"),
    ];
    let shown = shown(rows, "", |_| false).unwrap();
    assert_eq!(titles(&shown), ["A", "one", "C", "two"]);
}

#[test]
fn rows_named_in_hide_or_dropped_are_left_out_before_headings_are_judged() {
    let rows = vec![
        section("A"),
        action("one"),
        action("two"),
        section("B"),
        action("three"),
    ];
    let shown = shown(rows, "one,two", |r| r.item == "three").unwrap();
    assert!(shown.is_empty());
}

#[test]
fn tiles_need_the_style_a_menu_of_its_own_and_an_icon_on_every_entry() {
    let mut rows = vec![action("a"), section("S"), action("b")];
    assert!(!tiles(STYLE_TILES, false, &rows));
    for r in &mut rows {
        r.icon = "x".to_string();
    }
    assert!(tiles(STYLE_TILES, false, &rows));
    assert!(!tiles(STYLE_LIST, false, &rows));
    assert!(!tiles(STYLE_TILES, true, &rows));
    assert!(!tiles(STYLE_TILES, false, &[]));
    assert!(!tiles(STYLE_TILES, false, &[section("S")]));
}

#[test]
fn the_focus_passes_faint_rows_and_headings() {
    let rows = vec![
        section("S"),
        row("faint", Kind::Action, 0),
        action("a"),
        row("faint2", Kind::Action, 0),
        action("b"),
    ];
    assert_eq!(first_choosable(&rows), 2);
    assert_eq!(step_focus(&rows, 2, true), Some(4));
    assert_eq!(step_focus(&rows, 4, true), None);
    assert_eq!(step_focus(&rows, 4, false), Some(2));
    assert_eq!(step_focus(&rows, 2, false), None);
    assert_eq!(first_choosable(&[row("f", Kind::Action, 0)]), 0);
}

#[test]
fn choosing_runs_opens_or_changes() {
    let go = |r: &Row, depth| choose(r, depth).unwrap();
    assert_eq!(
        go(&action("a.b"), 1),
        Choice::Run {
            action: "a.b".to_string(),
            back: 2
        }
    );
    assert_eq!(
        go(&row("sub", Kind::Submenu, flag::ENABLED), 0),
        Choice::OpenMenu {
            menu: "sub".to_string(),
            depth: 1
        }
    );
    assert_eq!(
        go(&row("s", Kind::Setting, flag::ENABLED), 0),
        Choice::Setting("s".to_string())
    );
    assert_eq!(go(&row("a", Kind::Action, 0), 0), Choice::Nothing);
    assert_eq!(go(&section("S"), 0), Choice::Nothing);
    // A loop of submenus ends.
    assert_eq!(
        go(&row("sub", Kind::Submenu, flag::ENABLED), MAX_DEPTH - 1),
        Choice::Nothing
    );
}

fn info(kind: SettingKind, value: Option<Value>) -> SettingInfo {
    SettingInfo {
        key: "x.y".to_string(),
        kind,
        flags: SETTING_SHOWN,
        value,
        min: 0,
        max: 10,
        step: 2,
        unit: "{count} min".to_string(),
        editor_kind: 0,
        editor: String::new(),
        choices: 0,
    }
}

#[test]
fn numbers_step_by_their_step_and_stop_at_their_ends() {
    let n = |v| info(SettingKind::Int, Some(Value::Int(v)));
    assert_eq!(stepped(&n(4), 1), Some(Value::Int(6)));
    assert_eq!(stepped(&n(4), -1), Some(Value::Int(2)));
    assert_eq!(stepped(&n(10), 1), None);
    assert_eq!(stepped(&n(0), -1), None);
    assert_eq!(stepped(&n(9), 1), Some(Value::Int(10)));
    // A value out of range (kept in a file) comes back in.
    assert_eq!(stepped(&n(i64::MAX), 1), Some(Value::Int(10)));
    let b = info(SettingKind::Bool, Some(Value::Bool(false)));
    assert_eq!(stepped(&b, 1), Some(Value::Bool(true)));
    assert_eq!(stepped(&info(SettingKind::Choice, None), 1), None);
}

#[test]
fn units_take_the_number_in_or_follow_it() {
    assert_eq!(with_unit(30, "{count} min").unwrap(), "30 min");
    assert_eq!(with_unit(30, "min").unwrap(), "30 min");
    assert_eq!(with_unit(-3, "").unwrap(), "-3");
    assert_eq!(with_unit(i64::MIN, "").unwrap(), "-9223372036854775808");
}

#[test]
fn choosing_a_setting_row_depends_on_its_kind_and_the_device() {
    let mut i = info(SettingKind::Int, Some(Value::Int(1)));
    assert_eq!(act(&i, false, false).unwrap(), Act::StartNumber);
    assert_eq!(act(&i, true, false).unwrap(), Act::Nothing);
    assert_eq!(act(&i, true, true).unwrap(), Act::StartNumber);
    i.kind = SettingKind::Bool;
    assert_eq!(act(&i, true, false).unwrap(), Act::Step);
    i.kind = SettingKind::Choice;
    i.choices = 2;
    assert_eq!(act(&i, true, false).unwrap(), Act::Step);
    i.choices = 5;
    assert_eq!(act(&i, true, false).unwrap(), Act::Values);
    i.kind = SettingKind::Text;
    assert_eq!(act(&i, true, false).unwrap(), Act::Page);
    i.editor_kind = 1;
    i.editor = "x.screen".to_string();
    assert_eq!(
        act(&i, true, false).unwrap(),
        Act::Screen("x.screen".to_string())
    );
    i.editor_kind = 2;
    assert_eq!(
        act(&i, false, false).unwrap(),
        Act::Slot("x.screen".to_string())
    );
}

#[test]
fn a_setting_is_listed_when_shown_and_a_choice_has_two() {
    let mut i = info(SettingKind::Choice, None);
    i.choices = 1;
    assert!(!i.listed());
    i.choices = 2;
    assert!(i.listed());
    i.flags = 0;
    assert!(!i.listed());
    let mut t = info(SettingKind::Text, None);
    assert!(t.listed());
    t.flags = SETTING_HIDDEN;
    assert!(!t.listed());
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    let base = vec![section("A"), action("one"), section("B"), action("two")];
    sweep(|| {
        let rows = shown(copy_rows(&base)?, "two", |_| false)?;
        let _ = choose(&rows[1], 0)?;
        Ok(rows.len())
    });
    sweep(|| copy_rows(&base).map(|r| r.len()));
    sweep(|| choose(&base[1], 2).map(|_| ()));
    let i = info(SettingKind::Text, Some(Value::Text("a".repeat(300))));
    sweep(|| i.try_clone().map(|c| c == i));
    sweep(|| act(&i, false, false).map(|_| ()));
    sweep(|| with_unit(30, "{count} min").map(|t| t.len()));
}

#[test]
fn rows_keep_their_order() {
    let rows = vec![action("z"), action("a"), action("m")];
    let kept = shown(rows, "", |_| false).unwrap();
    assert_eq!(items(&kept), ["z", "a", "m"]);
}

/// What a row says of the value of a setting, in the language of `en`/`de` files.
fn said(catalog: &paperpaw_i18n::Catalog, info: &SettingInfo) -> String {
    crate::native_plugin::value_text(info, "On", "Off", &mut |key| {
        Ok(catalog.get(key).unwrap_or(key).to_string())
    })
    .unwrap()
}

#[test]
fn a_choice_is_said_by_its_translated_title_not_by_its_key() {
    use paperpaw_i18n::Catalog;
    let choice = |value: &str| info(SettingKind::Choice, Some(Value::Text(value.to_string())));
    let en = Catalog::parse(concat!(
        include_str!("../../../power/i18n/en.ini"),
        "\n",
        include_str!("../../../files/i18n/en.ini"),
    ))
    .unwrap();
    let de = Catalog::parse(concat!(
        include_str!("../../../power/i18n/de.ini"),
        "\n",
        include_str!("../../../files/i18n/de.ini"),
    ))
    .unwrap();
    assert_eq!(said(&en, &choice("power.mode.auto")), "Auto");
    assert_eq!(said(&de, &choice("power.mode.auto")), "Auto");
    assert_eq!(said(&en, &choice("files.sort.recent")), "Recently read");
    assert_eq!(said(&de, &choice("files.sort.recent")), "Zuletzt gelesen");
    assert_eq!(said(&de, &choice("files.view.list")), "Liste");
    // A choice with no value says nothing; a value that is no key is itself.
    assert_eq!(said(&en, &info(SettingKind::Choice, None)), "");
    assert_eq!(said(&en, &choice("")), "");
    assert_eq!(said(&en, &choice("plain")), "plain");
}

#[test]
fn a_switch_a_number_and_a_text_are_said_as_before() {
    // Nothing is translated here: asked to, the closure fails the row.
    let say = |kind, value| {
        crate::native_plugin::value_text(&info(kind, Some(value)), "On", "Off", &mut |_| {
            Err(paperpaw_sdk::native::Error::State)
        })
        .unwrap()
    };
    assert_eq!(say(SettingKind::Bool, Value::Bool(true)), "On");
    assert_eq!(say(SettingKind::Bool, Value::Bool(false)), "Off");
    assert_eq!(say(SettingKind::Int, Value::Int(6)), "6 min");
    // A text (not a choice) is shown as it is, even where it looks like a key.
    assert_eq!(say(SettingKind::Text, Value::Text("a.b".into())), "a.b");
}

#[test]
fn a_choice_with_no_text_in_the_catalogs_is_said_as_it_is_and_a_real_failure_fails() {
    use paperpaw_sdk::native::Error;
    let choice = info(
        SettingKind::Choice,
        Some(Value::Text("Literal".to_string())),
    );
    for refused in [Error::NotFound, Error::Invalid] {
        let said = crate::native_plugin::value_text(&choice, "On", "Off", &mut |_| Err(refused));
        assert_eq!(said.unwrap(), "Literal", "{refused:?}");
    }
    let failed =
        crate::native_plugin::value_text(&choice, "On", "Off", &mut |_| Err(Error::NoMemory));
    assert_eq!(failed, Err(Error::NoMemory));
}
