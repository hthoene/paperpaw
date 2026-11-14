//! A linked native plugin has its translation catalogs: its texts are found
//! (a native plugin built into the firmware brings them built in).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
#![cfg(feature = "native-linked")]

#[test]
fn every_linked_plugin_with_catalogs_has_its_files() {
    // The plugins that have texts of their own are the ones with an `i18n`
    // folder: a linked one found by its id, in every language it has.
    for id in ["keyboard", "menu", "settings", "help", "lock"] {
        let catalogs = paperpaw_app::native_catalogs(id);
        assert!(catalogs.len() >= 2, "{id}: {} catalogs", catalogs.len());
        assert!(catalogs.iter().all(|text| !text.is_empty()), "{id}");
    }
    assert!(paperpaw_app::native_catalogs("no-such-plugin").is_empty());
}

/// The store's plugins are linked only where the build has them (the X4's
/// firmware has none): their texts are not carried for nobody.
#[test]
fn a_plugin_the_build_leaves_out_brings_no_catalogs() {
    let built: Vec<&str> = paperpaw_app::CRATES.split(',').collect();
    for id in [
        "format-docx",
        "format-images",
        "format-mobi",
        "news",
        "vocabulary",
    ] {
        let name = format!("paperpaw-plugin-{id}");
        if !built.contains(&name.as_str()) {
            let catalogs = paperpaw_app::native_catalogs(id);
            assert!(catalogs.is_empty(), "{id}: {} catalogs", catalogs.len());
        }
    }
}

#[test]
fn the_kernel_finds_the_texts_of_a_linked_plugin() {
    use paperpaw_core::Kernel;
    use paperpaw_platform::{
        DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation,
    };
    let device = DeviceInfo::new(
        "test-x4",
        "Test X4",
        DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
        InputCaps::new(true, false, false, &["back"]),
        MemoryClass::Small,
    );
    let mut kernel = Kernel::builder(device).build().unwrap();
    assert_eq!(kernel.translator().tr("keyboard.done"), "keyboard.done");
    paperpaw_app::add_native_catalogs(&mut kernel);
    assert_ne!(kernel.translator().tr("keyboard.done"), "keyboard.done");
    // A second call adds nothing (a shell that calls it after
    // `register_native_plugins` did).
    let before = kernel.translator().tr("keyboard.done").to_string();
    paperpaw_app::add_native_catalogs(&mut kernel);
    assert_eq!(kernel.translator().tr("keyboard.done"), before);
}
