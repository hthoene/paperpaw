//! Every setting of the settings pages says what it does: a help text
//! below its name, in every language shipped;
//! sliders show their value with its unit.

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
    let builder = Kernel::builder(DEVICE).storage(storage).user_config("");
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
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

/// The row of the full refresh on the display's settings page, as the native
/// settings show it in `language`.
#[cfg(feature = "native-linked")]
fn full_refresh_row(language: &str, title: &str) -> String {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let (builder, card) =
        native_activation::quiet_on(DEVICE, storage, &format!("[core]\nlanguage = {language}\n"));
    let mut kernel = native_activation::build_on(paperpaw_app::install(builder, fonts()), card);
    let _ = kernel.start(Instant(0));
    let _ = kernel.open_screen(
        Instant(100),
        paperpaw_sdk::Id::new_static("settings.group"),
        paperpaw_sdk::Params::new().with("id", "core.group.reading-display"),
    );
    kernel.render(&mut |_, _| {});
    let labels: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    labels
        .into_iter()
        .find(|label| label.starts_with(title))
        .unwrap_or_else(|| panic!("no {title} row on {:?}", kernel.active_screen()))
}

/// A number's unit is said as Beta 3 said it: the unit's plural form for the
/// number, in the language chosen (`core.unit.pages`), never its key.
#[cfg(feature = "native-linked")]
#[test]
fn a_numbers_unit_is_said_in_the_language_chosen() {
    let row = full_refresh_row("en", "Full refresh");
    assert!(row.starts_with("Full refresh, 6 pages,"), "{row}");
    let row = full_refresh_row("de", "Volle Aktualisierung");
    assert!(row.starts_with("Volle Aktualisierung, 6 Seiten,"), "{row}");
}
