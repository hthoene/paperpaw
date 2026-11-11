//! A linked plugin's settings are taken from the table its build script made,
//! not copied onto the heap: a table that differs from what the plugin
//! declares costs the memory it should save, and the debug build says so.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
#![cfg(feature = "native-linked")]

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
    InputCaps::new(true, false, false, &["back", "page-forward", "page-back"]),
    MemoryClass::Small,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

#[test]
fn every_linked_plugin_takes_its_settings_from_its_table() {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage);
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    let _ = kernel.start(Instant(0));
    // The debug build says when a table does not equal what a plugin declared.
    #[cfg(debug_assertions)]
    {
        let unmatched: Vec<_> = kernel
            .take_warnings()
            .into_iter()
            .filter(|warning| format!("{warning:?}").contains("settings table does not match"))
            .collect();
        assert!(unmatched.is_empty(), "{unmatched:?}");
    }
    // A plugin with settings has its table (one that is not listed costs
    // memory): the ones that declare settings today.
    for id in ["keyboard", "lock", "help", "opds", "power", "bluetooth"] {
        assert!(
            !paperpaw_app::fixed_settings(id).is_empty(),
            "{id} has no table"
        );
    }
}
