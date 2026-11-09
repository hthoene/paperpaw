//! *Reading cat* as declared natively is what the interface-6 registration
//! made: the widget (name, title, kind, cells, live), its group and its
//! settings (ids, kinds, defaults, choices, titles, help), and the host
//! takes the records. The image holds what the builder makes.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::{Record, records_decode},
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;
use paperpaw_sdk::native::declare::{Declarations, Kind};
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar, SettingKind, SettingValue};
use paperpaw_ui_kit::widget::options::row;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the tests read the same file"
)]
mod declarations;

#[allow(dead_code, reason = "the tests read the widgets and the events")]
mod image {
    include!(concat!(env!("OUT_DIR"), "/reading_goal_declarations.rs"));
}

fn widget_batches() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        declarations::widgets(&mut d).unwrap();
    }
    out
}

fn event_batches() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    declarations::events(|batch: &[u8]| {
        out.push(batch.to_vec());
        Ok(())
    })
    .unwrap();
    out
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (name, built, image) in [
        ("widgets", widget_batches(), image::WIDGETS),
        ("events", event_batches(), image::EVENTS),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

/// The host takes what it has wired, and refuses the rest as unsupported
/// until it wires them (`screens-input.md` section 1).
#[test]
fn the_host_admits_the_records_or_refuses_them_as_unsupported() {
    for batch in widget_batches().into_iter().chain(event_batches()) {
        let records = records_decode(&batch).unwrap();
        if let Err(status) = validate_batch("reading-goal", &records) {
            assert_eq!(status, ERR_UNSUPPORTED);
        }
        for record in records.iter() {
            if let Err(status) = validate_record("reading-goal", &record) {
                assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
            }
        }
    }
}

fn interface6() -> Registrar {
    let mut r = Registrar::new(Id::new_static("reading-goal"));
    paperpaw_plugin_reading_goal::ReadingGoal
        .register(&mut r)
        .unwrap();
    r
}

#[test]
fn the_widget_is_the_one_of_interface_6() {
    let old = interface6();
    let widget = old
        .into_contributions()
        .into_iter()
        .find_map(|c| match c {
            Contribution::Widget(widget) => Some(widget),
            _ => None,
        })
        .unwrap();
    let batches = widget_batches();
    let mut seen = None;
    for batch in &batches {
        for record in records_decode(batch).unwrap().iter() {
            if let Record::WidgetSpec(spec) = record {
                seen = Some((
                    spec.name.to_string(),
                    spec.title.to_string(),
                    spec.category.to_string(),
                    spec.cells,
                    spec.token,
                    spec.flags,
                ));
            }
        }
    }
    let (name, title, category, cells, token, flags) = seen.unwrap();
    assert_eq!(format!("reading-goal.{name}"), widget.id.as_str());
    assert_eq!(title, widget.title);
    assert_eq!(category, widget.category);
    let span = widget.cells.unwrap();
    // Columns in the low byte, rows in the next; the least size is left to the host.
    assert_eq!(
        (cells & 0xff, (cells >> 8) & 0xff),
        (u32::from(span.w), u32::from(span.h))
    );
    assert_eq!(token, declarations::WIDGET_TOKEN);
    // Asked each minute: a new day makes the cat sad again.
    assert_eq!(flags & 1, 1, "live");
    assert_ne!(declarations::WIDGET_TOKEN, declarations::TICK_TOKEN);
    assert_ne!(declarations::WIDGET_TOKEN, declarations::EVENTS_TOKEN);
}

/// What a setting is, comparable between the two registrations.
#[derive(Debug, PartialEq)]
struct Shape {
    id: String,
    title: String,
    help: Option<String>,
    kind: String,
}

fn legacy() -> Vec<Shape> {
    let mut found = Vec::new();
    for contribution in interface6().into_contributions() {
        let Contribution::FixedSettings(list) = contribution else {
            continue;
        };
        for spec in list
            .iter()
            .filter(|s| s.id.as_str().starts_with("reading-goal.widget."))
        {
            let kind = match (&spec.kind, &spec.default) {
                (SettingKind::Bool, SettingValue::Bool(on)) => format!("bool {on}"),
                (SettingKind::Choice(choices), SettingValue::Text(default)) => {
                    format!("choice {default} {:?}", choices.iter().collect::<Vec<_>>())
                }
                other => panic!("a kind the widget does not use: {other:?}"),
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

fn native() -> Vec<Shape> {
    declarations::WIDGET_OPTIONS
        .iter()
        .map(|option| {
            let row = row(*option);
            let kind = match row.kind {
                Kind::Bool(on) => format!("bool {on}"),
                Kind::Choice { default, choices } => format!("choice {default} {choices:?}"),
                other => panic!("a kind the widget does not use: {other:?}"),
            };
            Shape {
                id: format!("reading-goal.{}.{}", declarations::WIDGET, row.suffix),
                title: row.title.to_string(),
                help: Some(row.help.to_string()),
                kind,
            }
        })
        .collect()
}

#[test]
fn the_widget_has_the_settings_of_its_interface_6_registration() {
    let (old, new) = (legacy(), native());
    assert_eq!(old.len(), new.len(), "{old:#?}");
    for (old, new) in old.iter().zip(&new) {
        assert_eq!(old, new);
    }
}
