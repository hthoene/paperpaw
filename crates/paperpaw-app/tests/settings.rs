//! Every setting of the settings pages says what it does: a help text
//! below its name, in every language shipped;
//! sliders show their value with its unit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::MemoryStorage;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-boox",
    "Test Boox",
    DisplayInfo::new(1264, 1680, 300, PixelFormat::Gray8, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &["back", "page-forward", "page-back"]),
    MemoryClass::Large,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// Groups whose settings are a widget's looks, chosen where the widget is
/// edited and named plainly enough there.
fn is_widget_group(kernel: &Kernel, group: &str) -> bool {
    kernel.widgets().has(group)
}

/// The standard app, started.
fn app() -> Kernel {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[help]\nshow = false\n");
    let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
    let _ = kernel.start(Instant(0));
    kernel
}

#[test]
fn every_setting_has_a_help_text() {
    let kernel = app();
    let settings = kernel.settings();
    let missing: Vec<String> = settings
        .specs()
        .filter(|spec| !is_widget_group(&kernel, settings.group_of(spec).as_str()))
        .filter(|spec| spec.description.is_none())
        .map(|spec| spec.id.to_string())
        .collect();
    assert!(
        missing.is_empty(),
        "settings without a help text: {missing:#?}"
    );
}

/// A slider is set by eye, but its value says what was chosen: with its
/// unit ("135 %", "4 mm"), never a bare number.
#[test]
fn sliders_show_their_unit() {
    let kernel = app();
    let bare: Vec<String> = kernel
        .settings()
        .specs()
        .filter(|spec| paperpaw_sdk::ui::slider::marked(spec) && spec.extra().unit.is_none())
        .map(|spec| spec.id.to_string())
        .collect();
    assert!(bare.is_empty(), "sliders without a unit: {bare:#?}");
}
