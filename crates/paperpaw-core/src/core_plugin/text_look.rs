//! How book text is inked: smooth or black and white, and how dark. Part
//! of the standard type of book pages; each book may have its own.

use paperpaw_sdk::SettingSpec;
use paperpaw_sdk::builtin::{groups, settings};

pub(super) static SETTINGS: [SettingSpec; 3] = [
    // Off unless the panel shows grays itself (see
    // `smooth_text_default`): on a panel of black and white every page
    // with it is a refresh of the whole screen in grays.
    SettingSpec::switch(
        settings::SMOOTH_TEXT.static_str(),
        "core.setting.smooth-text",
        false,
        groups::READING_DISPLAY,
    )
    .scoped()
    .description("core.setting.smooth-text.help"),
    SettingSpec::choice(
        settings::TEXT_CONTRAST.static_str(),
        "core.setting.text-contrast",
        settings::TEXT_CONTRAST_LEVELS,
        settings::TEXT_CONTRAST_LEVELS[1],
        groups::READING_DISPLAY,
    )
    .scoped()
    .description("core.setting.text-contrast.help"),
    // A curve of one's own, for themes and for people who want more: in
    // place of the contrast when set.
    SettingSpec::text(
        settings::TEXT_CURVE.static_str(),
        "core.setting.text-curve",
        "",
        groups::READING_DISPLAY,
    )
    .scoped()
    .advanced()
    .description("core.setting.text-curve.help"),
];

/// The default of [`settings::SMOOTH_TEXT`] on a panel of `gray_levels`
/// grays (`None`: many, or colour), where it is not the declared one
/// (off): on where the panel shows grays itself. A panel of black and
/// white shows smooth letters only with a refresh of the whole screen in
/// grays at every page, so there it stays off unless the reader chooses
/// it, and a device whose default was not set stays as quick.
pub(crate) fn smooth_text_default(gray_levels: Option<u16>) -> Option<&'static str> {
    (gray_levels != Some(2)).then_some("true")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_text_is_off_at_first_only_in_black_and_white() {
        assert_eq!(smooth_text_default(Some(2)), None);
        assert_eq!(smooth_text_default(Some(4)), Some("true"));
        assert_eq!(smooth_text_default(Some(16)), Some("true"));
        assert_eq!(smooth_text_default(None), Some("true"));
    }
}
