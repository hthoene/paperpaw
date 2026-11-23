#![allow(clippy::unwrap_used, reason = "test assertions")]

use super::*;

/// Every label the choice overlay shows is in both languages.
#[test]
fn the_options_and_texts_the_screen_uses_are_in_both_languages() {
    let en = include_str!("../../i18n/en.ini");
    let de = include_str!("../../i18n/de.ini");
    let mut keys = option_keys();
    keys.extend(
        [
            "file-manager.title",
            "file-manager.device",
            "file-manager.empty",
            "file-manager.new-folder",
            "file-manager.paste",
            "file-manager.cancel",
            "file-manager.rename",
            "file-manager.picked-move",
            "file-manager.picked-copy",
            "file-manager.delete-question",
            "file-manager.delete-folder-question",
            "file-manager.delete-book-question",
            "file-manager.option.delete-yes",
            "file-manager.option.cancel",
            "file-manager.error.not-found",
            "file-manager.error.invalid",
            "file-manager.error.unsupported",
            "file-manager.error.full",
            "file-manager.error.own-data",
            "file-manager.error.io",
            "file-manager.error.memory",
            "file-manager.error.too-long",
        ]
        .map(alloc::string::String::from),
    );
    for key in &keys {
        for (language, text) in [("en", en), ("de", de)] {
            let line = text
                .lines()
                .find(|l| l.starts_with(&alloc::format!("{key} =")));
            assert!(
                line.is_some_and(|l| l.len() > key.len() + 3),
                "{language}: {key} is missing"
            );
        }
    }
}

/// A name as long as the card takes (255 UTF-16 units) is read whole, of
/// three-byte letters too (#486): at 256 bytes such a name failed to read and
/// the rename said nothing.
#[test]
fn a_name_as_long_as_the_card_takes_is_read_whole() {
    let longest = "語".repeat(crate::browser::NAME_UNITS);
    assert!(longest.len() <= NAME_LIMIT, "{}", longest.len());
}
