//! The settings the native declarations make for the two widgets are the
//! settings the interface-6 registration made: ids, kinds, defaults, choices,
//! titles and help texts. The order differs (the shared look comes first), so
//! they are compared by id.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the build script and the plugin read the same file")]
mod declarations;
use paperpaw_plugin_weather::Weather;
use paperpaw_sdk::native::declare::Kind;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar, SettingKind, SettingValue};
use paperpaw_ui_kit::widget::options::{row, shared_look};

/// What a setting is, comparable between the two registrations.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Shape {
    id: String,
    title: String,
    help: Option<String>,
    kind: String,
}

fn legacy(widget: &str) -> Vec<Shape> {
    let mut r = Registrar::new(Id::new_static("weather"));
    Weather.register(&mut r).unwrap();
    let prefix = format!("weather.{widget}.");
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
                (SettingKind::Int { min, max }, SettingValue::Int(default)) => {
                    format!("int {default} {min}..={max}")
                }
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
    found.sort();
    found
}

fn native(widget: &str, options: &[declarations::Opt]) -> Vec<Shape> {
    let look = shared_look("core.align.center");
    let mut found: Vec<Shape> = look
        .into_iter()
        .chain(options.iter().copied())
        .map(|option| {
            let row = row(option);
            let kind = match row.kind {
                Kind::Bool(on) => format!("bool {on}"),
                Kind::Choice { default, choices } => format!("choice {default} {choices:?}"),
                Kind::Int { default, min, max } => format!("int {default} {min}..={max}"),
                other => panic!("a kind the widgets do not use: {other:?}"),
            };
            Shape {
                id: format!("weather.{widget}.{}", row.suffix),
                title: row.title.to_string(),
                help: Some(row.help.to_string()),
                kind,
            }
        })
        .collect();
    found.sort();
    found
}

#[test]
fn each_widget_has_the_settings_of_its_interface_6_registration() {
    for (name, options) in [
        ("now", declarations::NOW_OPTIONS),
        ("forecast", declarations::FORECAST_OPTIONS),
    ] {
        let (old, new) = (legacy(name), native(name, options));
        assert_eq!(old.len(), new.len(), "{name}: {old:#?}");
        for (old, new) in old.iter().zip(&new) {
            assert_eq!(old, new, "{name}");
        }
    }
}
