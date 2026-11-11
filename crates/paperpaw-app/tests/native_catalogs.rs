//! A linked native plugin has no `register` to add its translations, so the
//! shell's registration does: the texts of the keyboard, the menu and the
//! settings (and of every plugin of `NATIVE_PLUGINS`) resolve, in the
//! language chosen and in English behind it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::MemoryStorage;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &["back", "confirm", "left", "right"]),
    MemoryClass::Small,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

fn started(language: &str) -> Kernel {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(&format!("[core]\nlanguage = {language}\n"));
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    let _ = kernel.start(Instant(0));
    kernel
}

/// Without the native-linked feature the legacy plugins add their own: the
/// same keys resolve there, so the test holds in both builds.
#[test]
fn the_texts_of_the_linked_plugins_resolve_in_english() {
    let kernel = started("en");
    let translator = kernel.translator();
    assert_eq!(translator.get("keyboard.title"), Some("Keyboard"));
    assert_eq!(translator.get("menu.title"), Some("Menu"));
    assert_eq!(translator.get("settings.title"), Some("Settings"));
    // A letter row of the keyboard, read by its own screen when it opens.
    assert!(translator.get("keyboard.letters").is_some());
}

#[test]
fn and_in_the_language_chosen_with_english_behind_it() {
    let kernel = started("de");
    let translator = kernel.translator();
    assert_eq!(translator.get("keyboard.title"), Some("Tastatur"));
    assert_eq!(
        translator.get("keyboard.layout.de"),
        Some("Deutsch (QWERTZ)")
    );
    assert_eq!(translator.get("settings.title"), Some("Einstellungen"));
}
