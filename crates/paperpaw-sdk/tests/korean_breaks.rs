//! The reader's "Korean line breaks" setting reaches the line breaker only
//! for books in Korean, and only when it says by word.

#![cfg(feature = "unstable")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::Harness;
use paperpaw_sdk::builtin::{fonts, groups, settings};
use paperpaw_sdk::reading::{PageType, StatusRoom};
use paperpaw_sdk::{Font, FontSpec, Id, Priority, SettingKind, SettingSpec, SettingValue};

const LITERATA: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/Literata.ttf");
const FONT_ID: Id = Id::new_static("test.literata");

/// The key of the pages of a book in `language`, with the setting at
/// `choice` (`None`: the setting is not defined).
fn key(language: &str, choice: Option<&str>) -> String {
    let mut h = Harness::new();
    for (id, value) in [
        (settings::FONT_SIZE.static_str(), 12),
        (settings::MARGIN.static_str(), 4),
        (settings::MARGIN_ENDS.static_str(), 4),
        (settings::WIDOWS.static_str(), 1),
        (settings::ORPHANS.static_str(), 1),
    ] {
        h.settings
            .define(SettingSpec::number(id, "x", 0, 300, value, groups::OTHER))
            .unwrap();
    }
    if let Some(choice) = choice {
        h.settings
            .define(SettingSpec::new(
                Id::new_static(settings::KOREAN_BREAKS.static_str()),
                "x",
                SettingKind::Text,
                SettingValue::Text(choice.to_string().into()),
                groups::OTHER,
            ))
            .unwrap();
    }
    h.fonts
        .add(FontSpec::new(
            FONT_ID,
            Font::from_data(LITERATA, &[("wght", 400.0)]).unwrap(),
        ))
        .unwrap();
    h.fonts.propose(fonts::READING, FONT_ID, Priority::DEFAULT);
    let env = h.env();
    let font = env.fonts().role(fonts::READING.as_str()).unwrap();
    PageType::new(&env, &font, language, false, StatusRoom::NONE).key
}

#[test]
fn by_word_changes_the_pages_of_korean_books_only() {
    let syllable = settings::KOREAN_BREAKS_SYLLABLE;
    let word = settings::KOREAN_BREAKS_WORD;
    for language in ["ko", "ko-KR", "KO", "ko_KR"] {
        assert_ne!(
            key(language, Some(syllable)),
            key(language, Some(word)),
            "{language}"
        );
    }
    for language in ["en", "zh", "ja", "de-AT", "kok", "", "k"] {
        assert_eq!(
            key(language, Some(syllable)),
            key(language, Some(word)),
            "{language}: the setting is for Korean books"
        );
    }
}

#[test]
fn by_syllable_is_the_default_and_leaves_the_key_alone() {
    let syllable = settings::KOREAN_BREAKS_SYLLABLE;
    // Not defined (a build without the setting) reads as by syllable.
    assert_eq!(key("ko", None), key("ko", Some(syllable)));
}
