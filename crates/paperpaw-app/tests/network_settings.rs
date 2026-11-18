//! No plugin of the program itself (interface 6) uses the network any more: the
//! network plugins are native ones, whose settings the host makes when it admits
//! them (tested in the core).

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
use paperpaw_sdk::{MemoryStorage, Uses};

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

/// The standard app, started with `config`.
fn app(config: &str) -> Kernel {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage).user_config(config);
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    let _ = kernel.start(Instant(0));
    kernel
}

#[test]
fn fixed_network_settings_cover_the_plugins() {
    let kernel = app("");
    let networked: Vec<&str> = kernel
        .plugins()
        .iter()
        .filter(|p| p.manifest.uses.contains(Uses::NETWORK) && p.manifest.id.as_str() != "core")
        .map(|p| p.manifest.id.as_str())
        .collect();
    // Every plugin with network settings is a native one (their settings are
    // made when the host admits them, tested in the core): none is left in the
    // program's interface-6 plugins (#244).
    assert!(networked.is_empty(), "networked plugins: {networked:?}");
}
