//! The native declarations are what the image holds, name what the settings
//! plugin has always named (screens and their titles including the passwords
//! and keys, the advanced switch, the slots it fills, and the action, menu
//! entry and icon of the passwords and keys), and the host takes them as far
//! as it has wired them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::records_decode,
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script also uses the event declarations"
)]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/settings_declarations.rs"));
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

/// Whether `text` is in a batch as its bytes.
fn names(batches: &[Vec<u8>], text: &str) -> bool {
    batches
        .iter()
        .any(|batch| batch.windows(text.len()).any(|w| w == text.as_bytes()))
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (name, built, image) in [
        ("screens", batches!(declarations::screens), image::SCREENS),
        (
            "settings",
            batches!(declarations::settings),
            image::SETTINGS,
        ),
        (
            "navigation",
            batches!(declarations::navigation),
            image::NAVIGATION,
        ),
        ("events", batches!(declarations::events), image::EVENTS),
    ] {
        assert_eq!(built.len(), 1, "{name}: one batch");
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

#[test]
fn the_seven_screens_are_declared_with_their_titles() {
    let screens = batches!(declarations::screens);
    for (name, title) in [
        ("list", "settings.title"),
        ("group", "settings.title"),
        ("themes", "settings.theme"),
        ("choose", "settings.title"),
        ("search", "settings.search"),
        ("secrets", "settings.secrets"),
        ("secret", "settings.secrets"),
    ] {
        assert!(names(&screens, name), "{name}");
        assert!(names(&screens, title), "{name}: {title}");
    }
}

#[test]
fn the_advanced_switch_and_the_slots_are_declared() {
    let settings = batches!(declarations::settings);
    for text in [
        "advanced",
        "core.group.system",
        "settings.advanced",
        "settings.advanced.help",
    ] {
        assert!(names(&settings, text), "{text}");
    }
    let navigation = batches!(declarations::navigation);
    for text in [
        "core.settings",
        "settings.list",
        "core.choose-value",
        "settings.choose",
    ] {
        assert!(names(&navigation, text), "{text}");
    }
}

#[test]
fn the_action_the_menu_entry_and_the_icon_of_the_passwords_and_keys_are_declared() {
    let navigation = batches!(declarations::navigation);
    for text in [
        "secrets.open",
        "settings.secrets",
        "core.settings-pages",
        "settings.secrets.open",
        "key",
    ] {
        assert!(names(&navigation, text), "{text}");
    }
    assert_eq!(declarations::SECRETS_ORDER, 95);
    assert_eq!(
        paperpaw_plugin_settings::native_secrets::OPEN,
        "settings.secrets.open"
    );
}

#[test]
fn the_plugin_and_the_declarations_share_their_tokens() {
    use paperpaw_plugin_settings::native_plugin as plugin;
    assert_eq!(
        [
            plugin::LIST,
            plugin::GROUP,
            plugin::CHOOSE,
            plugin::THEMES,
            plugin::SEARCH,
            plugin::SECRETS,
            plugin::SECRET,
            plugin::EVENTS,
            plugin::OPEN_SECRETS
        ],
        [
            declarations::LIST,
            declarations::GROUP,
            declarations::CHOOSE,
            declarations::THEMES,
            declarations::SEARCH,
            declarations::SECRETS,
            declarations::SECRET,
            declarations::EVENTS,
            declarations::OPEN_SECRETS
        ]
    );
}

/// The host takes the screens; the other families it refuses as unsupported
/// until it wires them, so `make` is refused until then (pending #944).
#[test]
fn the_host_admits_the_screens_and_refuses_the_rest_as_unsupported() {
    for batch in batches!(declarations::screens) {
        let records = records_decode(&batch).unwrap();
        validate_batch("settings", &records).unwrap();
    }
    for batch in batches!(declarations::settings)
        .into_iter()
        .chain(batches!(declarations::navigation))
        .chain(batches!(declarations::events))
    {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            if let Err(status) = validate_record("settings", &record) {
                assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
            }
        }
    }
}
