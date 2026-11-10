//! The widgets the native declarations make (the date and the clock) are the
//! widgets the interface-6 registration made: their ids, titles and categories,
//! the group of settings of each, and each setting's id, kind, default, choices,
//! title and help, in the same order.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_home::Home;
use paperpaw_sdk::native::declare::Kind;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar, SettingKind, SettingValue};
use paperpaw_ui_kit::widget::options::row;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the plugin read the same file"
)]
mod declarations;

/// What a setting is, comparable between the two registrations.
#[derive(Debug, PartialEq)]
struct Shape {
    id: String,
    title: String,
    help: Option<String>,
    kind: String,
}

fn legacy_contributions() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("home"));
    Home.register(&mut r).unwrap();
    r.into_contributions()
}

fn legacy_settings(widget: &str) -> Vec<Shape> {
    let prefix = format!("home.{widget}.");
    let mut found = Vec::new();
    for contribution in legacy_contributions() {
        let Contribution::FixedSettings(list) = contribution else {
            continue;
        };
        for spec in list.iter().filter(|s| s.id.as_str().starts_with(&prefix)) {
            let kind = match (&spec.kind, &spec.default) {
                (SettingKind::Bool, SettingValue::Bool(on)) => format!("bool {on}"),
                (SettingKind::Choice(choices), SettingValue::Text(default)) => {
                    format!("choice {default} {:?}", choices.iter().collect::<Vec<_>>())
                }
                (SettingKind::Text, SettingValue::Text(default)) => format!("text {default:?}"),
                (SettingKind::List, SettingValue::Text(default)) => format!("list {default:?}"),
                (SettingKind::Int { min, max }, SettingValue::Int(default)) => {
                    format!("int {default} {min}..={max}")
                }
                other => panic!("a kind the home widgets do not use: {other:?}"),
            };
            found.push(Shape {
                id: spec.id.as_str().to_string(),
                title: spec.title.to_string(),
                help: spec.description.as_deref().map(str::to_string),
                kind,
            });
        }
    }
    found
}

fn native_settings(def: &declarations::Def) -> Vec<Shape> {
    def.all()
        .map(|option| {
            let row = row(option);
            let kind = match row.kind {
                Kind::Bool(on) => format!("bool {on}"),
                Kind::Choice { default, choices } => format!("choice {default} {choices:?}"),
                Kind::Text(default) => format!("text {default:?}"),
                Kind::List(default) => format!("list {default:?}"),
                Kind::Int { default, min, max } => format!("int {default} {min}..={max}"),
                other => panic!("a kind the home widgets do not use: {other:?}"),
            };
            Shape {
                id: format!("home.{}.{}", def.name, row.suffix),
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
        let (old, new) = (legacy_settings(def.name), native_settings(def));
        assert!(!old.is_empty(), "{}", def.name);
        assert_eq!(old.len(), new.len(), "{}: {old:#?}", def.name);
        for (old, new) in old.iter().zip(&new) {
            assert_eq!(old, new);
        }
    }
}

#[test]
fn each_native_widget_has_the_id_title_category_and_group_of_its_interface_6_one() {
    let contributions = legacy_contributions();
    let mut widgets = paperpaw_sdk::widgets::Widgets::new();
    for contribution in &contributions {
        if let Contribution::FixedWidgets(list) = contribution {
            widgets.add_fixed(list);
        }
    }
    for def in declarations::WIDGETS {
        let id = format!("home.{}", def.name);
        assert_eq!(widgets.title(&id), Some(def.title), "{id}");
        assert_eq!(widgets.category(&id), Some(def.category), "{id}");
        // Its settings are in a group of the same name and title.
        let group = contributions.iter().find_map(|c| match c {
            Contribution::SettingGroup(g) if g.id.as_str() == id => Some(g),
            _ => None,
        });
        let group = group.unwrap_or_else(|| panic!("the group {id}"));
        assert_eq!(group.title, def.title, "{id}");
    }
}

#[test]
fn the_status_line_has_the_settings_widget_and_place_of_its_interface_6_registration() {
    // The parts (the line, left, centre, right), then the look of the frame.
    let native: Vec<Shape> = declarations::STATUS_PARTS
        .iter()
        .map(|&(name, default, title, help, _)| Shape {
            id: format!("home.status.{name}"),
            title: title.to_string(),
            help: Some(help.to_string()),
            kind: format!("list {default:?}"),
        })
        .chain(declarations::STATUS_LOOK.iter().map(|&option| {
            let row = row(option);
            let kind = match row.kind {
                Kind::Bool(on) => format!("bool {on}"),
                Kind::Choice { default, choices } => format!("choice {default} {choices:?}"),
                Kind::Text(default) => format!("text {default:?}"),
                other => panic!("a kind the status look does not use: {other:?}"),
            };
            Shape {
                id: format!("home.status.{}", row.suffix),
                title: row.title.to_string(),
                help: Some(row.help.to_string()),
                kind,
            }
        }))
        .collect();
    let old = legacy_settings("status");
    assert_eq!(old.len(), native.len(), "{old:#?}");
    for (old, new) in old.iter().zip(&native) {
        assert_eq!(old, new);
    }
    // The widget (id, title, category), its group and the place that names the three
    // settings that list its items.
    let contributions = legacy_contributions();
    let mut widgets = paperpaw_sdk::widgets::Widgets::new();
    for contribution in &contributions {
        if let Contribution::FixedWidgets(list) = contribution {
            widgets.add_fixed(list);
        }
    }
    assert_eq!(widgets.title("home.status"), Some("home.widget.status"));
    assert_eq!(widgets.category("home.status"), Some("core.widgets.device"));
}
