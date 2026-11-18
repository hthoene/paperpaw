//! The native declarations (settings, screens, navigation, stages) are what the
//! image holds, and the host takes them as far as it has wired them.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::records_decode,
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/keyboard_declarations.rs"));
}

/// The batches `$declare` submits.
macro_rules! batches {
    ($declare:path) => {{
        let mut out: Vec<Vec<u8>> = Vec::new();
        {
            let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
                out.push(batch.to_vec());
                Ok(())
            });
            $declare(&mut d).unwrap();
        }
        out
    }};
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (name, built, image) in [
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
        ("stages", batches!(declarations::stages), image::STAGE_SPECS),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

/// The declarations name the settings, the group, the screens, the slot, the
/// action with its key binding and the first-run step by their ids and texts.
#[test]
fn the_declarations_name_the_settings_the_screens_and_the_slot() {
    fn holds(batches: &[Vec<u8>], names: &[&str]) {
        for name in names {
            assert!(
                batches
                    .iter()
                    .any(|b| b.windows(name.len()).any(|w| w == name.as_bytes())),
                "{name}"
            );
        }
    }
    holds(
        &batches!(declarations::settings),
        &[
            "keyboard.group",
            "keyboard.title",
            "keyboard.setting.layouts",
            "keyboard.setting.layouts.help",
            "keyboard.setting.umlaut-keys",
            "keyboard.setting.key-shape",
            "keyboard.setting.size",
            "keyboard.setting.auto-capitals",
            "keyboard.layout.auto",
        ],
    );
    holds(
        &batches!(declarations::screens),
        &["keyboard.title", "keyboard.setting.layouts"],
    );
    holds(
        &batches!(declarations::navigation),
        &["keyboard.done-action", "long.back"],
    );
}

/// The host takes the screens; the other families it refuses as unsupported
/// until it wires them (`screens-input.md` section 1), so `make` is refused
/// until then: the same state as the other native plugins, pending #944.
#[test]
fn the_host_admits_the_screens_and_refuses_the_rest_as_unsupported() {
    for batch in batches!(declarations::screens) {
        let records = records_decode(&batch).unwrap();
        validate_batch("keyboard", &records).unwrap();
    }
    for batch in batches!(declarations::settings)
        .into_iter()
        .chain(batches!(declarations::navigation))
    {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            if let Err(status) = validate_record("keyboard", &record) {
                assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
            }
        }
    }
}

#[test]
fn the_plugin_and_the_declarations_share_their_tokens() {
    use paperpaw_plugin_keyboard::native_plugin as plugin;
    assert_eq!(
        [plugin::SCREEN, plugin::CHOOSE, plugin::DONE],
        [
            declarations::SCREEN,
            declarations::CHOOSE,
            declarations::DONE
        ]
    );
}

/// The stage other plugins add layouts through is declared, and its bounds are
/// those of the generated codec (`stages/keyboard.layouts@1.def`).
#[test]
fn the_layouts_stage_is_declared_with_the_codecs_bounds() {
    use paperpaw_core::native_host::{
        declarations::validate_batch,
        registrations::{Record, records_decode},
    };
    let mut stages = 0;
    for batch in batches!(declarations::stages) {
        let records = records_decode(&batch).unwrap();
        validate_batch("keyboard", &records).unwrap();
        for record in records.iter() {
            let Record::StageSpec(spec) = record else {
                panic!("an unexpected record: {record:?}")
            };
            assert_eq!(spec.name, "layouts");
            stages += 1;
        }
    }
    assert_eq!(stages, 1);
    let schema: serde_json_free::Def =
        serde_json_free::read(include_str!("../stages/keyboard.layouts@1.def"));
    assert_eq!(
        (
            schema.max_input,
            schema.max_reply,
            schema.reply_rows,
            schema.major
        ),
        (
            declarations::LAYOUTS_MAX_INPUT,
            declarations::LAYOUTS_MAX_REPLY,
            declarations::LAYOUTS_MAX_RECORDS,
            declarations::LAYOUTS_SCHEMA
        )
    );
}

/// The few numbers of a schema file, read without a JSON library.
mod serde_json_free {
    pub struct Def {
        pub max_input: u32,
        pub max_reply: u32,
        pub reply_rows: u32,
        pub major: u32,
    }

    fn number(text: &str, key: &str) -> u32 {
        let at = text.find(&format!("\"{key}\":")).unwrap() + key.len() + 3;
        text[at..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap()
    }

    pub fn read(text: &str) -> Def {
        Def {
            max_input: number(text, "max_input"),
            max_reply: number(text, "max_reply"),
            reply_rows: number(text, "reply_rows"),
            major: number(text, "major"),
        }
    }
}
