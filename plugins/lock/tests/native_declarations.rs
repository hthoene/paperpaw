//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/lock_declarations.rs"));
}

/// The batches a builder makes.
macro_rules! batches {
    ($build:path) => {{
        let mut out: Vec<Vec<u8>> = Vec::new();
        {
            let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
                out.push(batch.to_vec());
                Ok(())
            });
            $build(&mut d).unwrap();
        }
        out
    }};
}

type Group = (&'static str, Vec<Vec<u8>>, &'static [&'static [u8]], usize);

/// Each group of declarations: its name, what the builder makes, what the image
/// holds and how many records each batch has (the settings: the group, three
/// settings, the kind's editor and the touch lock's need; all batches of a group together).
fn groups() -> [Group; 6] {
    [
        (
            "settings",
            batches!(declarations::settings),
            image::SETTINGS,
            7,
        ),
        (
            "screens",
            batches!(declarations::screens),
            image::SCREENS,
            1,
        ),
        (
            "navigation",
            batches!(declarations::navigation),
            image::NAVIGATION,
            2,
        ),
        ("events", batches!(declarations::events), image::EVENTS, 2),
        (
            "touch ui",
            batches!(declarations::touch_ui),
            image::TOUCH_UI,
            11,
        ),
        (
            "touch events",
            batches!(declarations::touch_events),
            image::TOUCH_EVENTS,
            2,
        ),
    ]
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (name, built, image, _) in groups() {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

#[test]
fn the_host_admits_the_declarations() {
    for (name, built, _, count) in groups() {
        let mut seen = 0;
        for batch in built {
            let records = records_decode(&batch).unwrap();
            seen += records.iter().count();
            validate_batch("lock", &records).unwrap();
        }
        assert_eq!(seen, count, "{name}");
    }
}

#[test]
fn lock_icons_keep_beta3_references() {
    use paperpaw_core::native_host::registrations::Record;
    let mut icons = Vec::new();
    for batch in batches!(declarations::settings)
        .into_iter()
        .chain(batches!(declarations::navigation))
    {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::IconSpec(icon) = record {
                icons.push((icon.item.to_owned(), icon.icon.to_owned()));
            }
        }
    }
    assert_eq!(
        icons,
        [
            ("lock.group".into(), "lock".into()),
            ("lock.now".into(), "lock".into())
        ]
    );
}

/// *Lock touch* in the menus is the action, which closes the panel it is chosen
/// in; the setting's switch would stay open on a screen that can no longer be
/// touched (#234). The way out without touch is bound while the switch is on.
#[test]
fn lock_touch_in_the_menus_is_an_action_and_the_home_key_is_a_way_out() {
    use paperpaw_core::native_host::registrations::Record;
    let (mut entries, mut keys, mut actions) = (Vec::new(), Vec::new(), Vec::new());
    for batch in batches!(declarations::touch_ui) {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::MenuEntry(e) => entries.push((e.menu.to_owned(), e.item.to_owned())),
                Record::KeyBind(k) => keys.push((
                    k.input.to_owned(),
                    k.action.to_owned(),
                    k.when_key.to_owned(),
                    k.when_value.to_owned(),
                )),
                Record::ActionSpec(a) => actions.push(a.name.to_owned()),
                _ => {}
            }
        }
    }
    assert_eq!(
        entries,
        [
            ("core.quick".into(), "lock.touch-lock".into()),
            ("core.reader-menu".into(), "lock.touch-lock".into())
        ]
    );
    assert_eq!(
        keys,
        [(
            "long.home".into(),
            "lock.touch-toggle".into(),
            "lock.touch".into(),
            "true".into()
        )]
    );
    assert_eq!(actions, ["touch-toggle", "touch-lock", "touch-hint"]);
}
