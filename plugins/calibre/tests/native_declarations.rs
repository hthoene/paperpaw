//! The plugin's native declarations are what the image holds, the host admits
//! them, and the settings are pinned.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};
use paperpaw_sdk::native::declare::Declarations;

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/calibre_declarations.rs"));
}

macro_rules! batches {
    ($build:path) => {{
        let mut out: Vec<Vec<u8>> = Vec::new();
        {
            let mut d = Declarations::new(|batch: &[u8]| {
                out.push(batch.to_vec());
                Ok(())
            });
            $build(&mut d).unwrap();
        }
        out
    }};
}

/// A declaration's name, the batches it sends and the ones it expects.
type Case = (&'static str, Vec<Vec<u8>>, &'static [&'static [u8]]);

fn all() -> [Case; 4] {
    [
        (
            "settings",
            batches!(declarations::settings),
            image::SETTINGS,
        ),
        ("screens", batches!(declarations::screens), image::SCREENS),
        (
            "navigation",
            batches!(declarations::navigation),
            image::NAVIGATION,
        ),
        ("events", batches!(declarations::events), image::EVENTS),
    ]
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (name, built, image) in all() {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

#[test]
fn the_host_admits_every_record() {
    for (name, built, _) in all() {
        for batch in built {
            let records = records_decode(&batch).unwrap();
            assert!(records.iter().count() > 0, "{name}");
            validate_batch("calibre", &records).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }
}

/// The setting, its part, the screen, the action, the tools entry (order 47),
/// the icon and the two subscriptions are what interface 6 registered (compared
/// field by field before it was removed).
#[test]
fn what_is_declared_is_what_interface_6_registered() {
    use paperpaw_core::native_host::registrations::Record;
    let mut seen = Vec::new();
    for (_, built, _) in all() {
        for batch in built {
            for record in records_decode(&batch).unwrap().iter() {
                seen.push(match record {
                    Record::SettingSpec(s) => format!(
                        "setting {} {} {} {} kind={} flags={}",
                        s.name, s.group, s.title, s.help, s.kind, s.flags
                    ),
                    Record::SettingExtra(e) => format!("extra {} section={}", e.name, e.section),
                    Record::ScreenSpec(s) => format!("screen {} {}", s.name, s.title),
                    Record::ActionSpec(a) => format!("action {} {}", a.name, a.title),
                    Record::MenuEntry(e) => format!("menu {} {} {}", e.menu, e.item, e.order),
                    Record::IconSpec(i) => format!("icon {} {}", i.item, i.icon),
                    other => format!("{other:?}"),
                });
            }
        }
    }
    let expected = [
        "setting address core.group.library calibre.setting.address calibre.setting.address.help kind=3 flags=16",
        "extra address section=calibre.plugin.name",
        "screen screen calibre.title",
        "action open calibre.title",
        "menu core.tools calibre.open 47",
        "icon calibre.open device-desktop",
    ];
    assert_eq!(
        &seen[..expected.len().min(seen.len())],
        &expected[..expected.len().min(seen.len())],
        "{seen:#?}"
    );
    // The two subscriptions follow.
    assert_eq!(seen.len(), expected.len() + 2, "{seen:#?}");
}
