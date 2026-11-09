//! The widgets the native declarations make are the widgets the interface-6
//! registration made: the daily goal, and for each widget its group, its place
//! in the catalogue and every setting (ids, kinds, defaults, choices, titles
//! and help texts, in the same order).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the plugin read the same file"
)]
mod declarations;
use paperpaw_plugin_stats::Stats;
use paperpaw_sdk::native::declare::Kind;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar, SettingKind, SettingValue};
use paperpaw_ui_kit::widget::options::row;

/// What a setting is, comparable between the two registrations.
#[derive(Debug, PartialEq)]
struct Shape {
    id: String,
    title: String,
    help: Option<String>,
    kind: String,
}

fn shape_of(spec: &paperpaw_sdk::SettingSpec) -> Shape {
    let kind = match (&spec.kind, &spec.default) {
        (SettingKind::Bool, SettingValue::Bool(on)) => format!("bool {on}"),
        (SettingKind::Choice(choices), SettingValue::Text(default)) => {
            format!("choice {default} {:?}", choices.iter().collect::<Vec<_>>())
        }
        (SettingKind::Text, SettingValue::Text(default)) => format!("text {default:?}"),
        (SettingKind::Int { min, max }, SettingValue::Int(default)) => {
            format!("int {default} {min}..={max}")
        }
        other => panic!("a kind the widgets do not use: {other:?}"),
    };
    Shape {
        id: spec.id.as_str().to_string(),
        title: spec.title.to_string(),
        help: spec.description.as_deref().map(str::to_string),
        kind,
    }
}

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("stats"));
    Stats.register(&mut r).unwrap();
    r.into_contributions()
}

/// The interface-6 settings whose id starts with `prefix`, in order.
fn old(prefix: &str) -> Vec<Shape> {
    let mut found = Vec::new();
    for contribution in legacy() {
        let Contribution::FixedSettings(list) = contribution else {
            continue;
        };
        found.extend(
            list.iter()
                .filter(|s| s.id.as_str().starts_with(prefix))
                .map(shape_of),
        );
    }
    found
}

fn native(def: &declarations::Def) -> Vec<Shape> {
    def.all()
        .map(|option| {
            let row = row(option);
            let kind = match row.kind {
                Kind::Bool(on) => format!("bool {on}"),
                Kind::Choice { default, choices } => format!("choice {default} {choices:?}"),
                Kind::Text(default) => format!("text {default:?}"),
                Kind::Int { default, min, max } => format!("int {default} {min}..={max}"),
                other => panic!("a kind the widgets do not use: {other:?}"),
            };
            Shape {
                id: format!("stats.{}.{}", def.name, row.suffix),
                title: row.title.to_string(),
                help: Some(row.help.to_string()),
                kind,
            }
        })
        .collect()
}

#[test]
fn each_native_widget_has_the_settings_of_its_interface_6_registration() {
    for def in declarations::WIDGETS {
        let (old, new) = (old(&format!("stats.{}.", def.name)), native(&def));
        assert_eq!(old.len(), new.len(), "{}: {old:#?}", def.name);
        for (old, new) in old.iter().zip(&new) {
            assert_eq!(old.id, new.id);
            assert_eq!(old.kind, new.kind, "{}", old.id);
            assert_eq!(old.title, new.title, "{}", old.id);
            // A widget's setting without help says none; the shared rows' help
            // is the kit's own text.
            if let Some(help) = &old.help {
                assert_eq!(Some(help), new.help.as_ref(), "{}", old.id);
            }
        }
    }
}

#[test]
fn the_daily_goal_is_the_interface_6_setting() {
    let old = old("stats.goal");
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].id, "stats.goal");
    assert_eq!(old[0].title, "stats.setting.goal");
    assert_eq!(old[0].help.as_deref(), Some("stats.setting.goal.help"));
    assert_eq!(
        old[0].kind,
        format!("choice stats.goal.off {:?}", declarations::GOALS)
    );
}
