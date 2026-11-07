//! The settings the native declarations make for a widget are the settings
//! the interface-6 registration made: ids, kinds, defaults, choices, titles
//! and help texts, in the same order.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_widgets::Widgets;
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

fn legacy(widget: &str) -> Vec<Shape> {
    let mut r = Registrar::new(Id::new_static("widgets"));
    Widgets.register(&mut r).unwrap();
    let prefix = format!("widgets.{widget}.");
    let mut found = Vec::new();
    for contribution in r.into_contributions() {
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
                other => panic!("a kind the widgets do not use: {other:?}"),
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

fn native(def: &declarations::Def) -> Vec<Shape> {
    def.all()
        .map(|option| {
            let row = row(option);
            let kind = match row.kind {
                Kind::Bool(on) => format!("bool {on}"),
                Kind::Choice { default, choices } => format!("choice {default} {choices:?}"),
                Kind::Text(default) => format!("text {default:?}"),
                other => panic!("a kind the widgets do not use: {other:?}"),
            };
            Shape {
                id: format!("widgets.{}.{}", def.name, row.suffix),
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
        let (old, new) = (legacy(def.name), native(def));
        assert_eq!(old.len(), new.len(), "{}: {old:#?}", def.name);
        for (old, new) in old.iter().zip(&new) {
            assert_eq!(old, new);
        }
    }
}
