//! Settings list values show catalogue labels, as in Beta 3 (#277).
#![allow(clippy::unwrap_used, reason = "test assertions")]

mod native_activation;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::{Id, MemoryStorage, Params};

fn keyboard_settings(language: &str, value: &str) -> Kernel {
    let device = DeviceInfo::new(
        "test-x4",
        "Test X4",
        DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg90, None),
        InputCaps::new(true, false, false, &[]),
        MemoryClass::Small,
    );
    let card = native_activation::SharedCard::new(
        MemoryStorage::new().with(".paperpaw/plugins/setup/done", ""),
    );
    let config = format!(
        "[help]\nshow=false\n[reader]\nopen-last=false\n[core]\nlanguage={language}\n[keyboard]\nlayouts={value}\n"
    );
    let builder = Kernel::builder(device)
        .storage(card.clone())
        .user_config(&config)
        .frame_rows(192)
        .row_align(8);
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    let fonts = paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    let fonts = paperpaw_app::Fonts::Outline;
    let builder = paperpaw_app::install(builder, fonts);
    let mut kernel = native_activation::build_on(builder, card.handle());
    kernel.start(Instant(0));
    kernel.open_screen(
        Instant(300),
        Id::new_static("settings.group"),
        Params::new().with("id", "keyboard.group"),
    );
    kernel.render(&mut |_, _| {});
    kernel
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs native Settings and keyboard"
)]
fn keyboard_layout_values_have_names_in_both_ui_languages() {
    for (language, title, name, none) in [
        ("en", "Keyboard layouts", "Automatic", "None"),
        ("de", "Tastatur-Layouts", "Automatisch", "Keine"),
    ] {
        for (value, expected) in [("keyboard.layout.auto", name), ("", none)] {
            let mut kernel = keyboard_settings(language, value);
            let parts = kernel.describe();
            let row = parts
                .iter()
                .find(|part| part.label.contains(title))
                .unwrap();
            assert!(
                row.label.contains(expected),
                "{language}/{value}: {}",
                row.label
            );
            assert!(!row.label.contains("keyboard.layout."), "{}", row.label);
        }
    }
}
