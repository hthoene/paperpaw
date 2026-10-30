//! The network settings of every networked plugin the app comes with are
//! kept in the program, not made on the heap at each start; and they are
//! the settings they were: their ids, defaults and the values a
//! configuration file gives them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

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
    let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
    let _ = kernel.start(Instant(0));
    kernel
}

#[test]
fn fixed_network_settings_cover_the_plugins() {
    let kernel = app("");
    let mut networked: Vec<&str> = kernel
        .plugins()
        .iter()
        .filter(|p| p.manifest.uses.contains(Uses::NETWORK) && p.manifest.id.as_str() != "core")
        .map(|p| p.manifest.id.as_str())
        .collect();
    networked.sort_unstable();
    let mut fixed: Vec<&str> = paperpaw_app::NETWORK_SETTINGS
        .iter()
        .map(|(p, _)| *p)
        .collect();
    fixed.sort_unstable();
    assert_eq!(
        networked, fixed,
        "networked plugins and those with fixed network settings"
    );
}

#[test]
fn network_settings_keep_their_ids_defaults_and_values() {
    let kernel = app("");
    for (plugin, _) in paperpaw_app::NETWORK_SETTINGS {
        let network = format!("core.network.{plugin}");
        let saving = format!("core.saving.{plugin}");
        let settings = kernel.settings();
        assert!(settings.bool(&network), "{network}");
        assert_eq!(settings.text(&saving), "core.saving.follow", "{saving}");
    }
    let kernel = app("[core]\nnetwork.opds = false\nsaving.news = core.saving.manual\n");
    assert!(!kernel.settings().bool("core.network.opds"));
    assert_eq!(
        kernel.settings().text("core.saving.news"),
        "core.saving.manual"
    );
    assert!(kernel.settings().bool("core.network.news"));
}
