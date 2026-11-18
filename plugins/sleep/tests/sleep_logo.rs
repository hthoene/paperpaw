//! The logo above the message on interface 7, its decision without the host:
//! the plugin's own where `core.logo` is empty, the card's picture it names
//! (asked for on the library root, trimmed as Beta 3 read it) where it is
//! set, and the plugin's own again where that picture could not be had.

use paperpaw_plugin_sleep::native_sleep::{
    LOGO_PATH_MAX, LOGO_SETTING, LogoFile, OWN_LOGO, logo_file,
};
use paperpaw_sdk::native::image::PrepareRoot;

#[test]
fn the_setting_is_the_core_logo() {
    assert_eq!(LOGO_SETTING, "core.logo");
    assert_eq!(OWN_LOGO, "logo.png");
}

#[test]
fn an_empty_setting_asks_for_the_plugins_own_logo() {
    for setting in ["", "   ", "/", " // "] {
        assert_eq!(logo_file(setting, false), LogoFile::Own, "{setting:?}");
        assert_eq!(
            logo_file(setting, false).request(),
            (PrepareRoot::Data, "logo.png")
        );
    }
}

#[test]
fn a_set_logo_asks_for_the_card_picture_on_the_library_root() {
    assert_eq!(
        logo_file("plugins/mine/logo.png", false).request(),
        (PrepareRoot::Library, "plugins/mine/logo.png")
    );
    // Spaces and slashes at its ends go, as Beta 3 read it.
    assert_eq!(
        logo_file("  /pictures/logo.jpg/ ", false),
        LogoFile::Card("pictures/logo.jpg")
    );
    assert_eq!(
        logo_file(" /logo.gif", false).request(),
        (PrepareRoot::Library, "logo.gif")
    );
}

#[test]
fn a_card_picture_that_failed_falls_back_to_the_plugins_own() {
    assert_eq!(logo_file("plugins/mine/logo.png", true), LogoFile::Own);
    assert_eq!(
        logo_file("plugins/mine/logo.png", true).request(),
        (PrepareRoot::Data, OWN_LOGO)
    );
    assert_eq!(logo_file("", true), LogoFile::Own);
}

#[test]
fn a_path_too_long_for_a_request_is_the_plugins_own() {
    let longest = "a".repeat(LOGO_PATH_MAX);
    assert_eq!(logo_file(&longest, false), LogoFile::Card(longest.as_str()));
    let longer = "a".repeat(LOGO_PATH_MAX + 1);
    assert_eq!(logo_file(&longer, false), LogoFile::Own);
}
