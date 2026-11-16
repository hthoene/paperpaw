//! The *Lock touch* row is on the lock's settings page of a touch device and
//! not on one without touch, where it would do nothing (#277).
#![allow(clippy::unwrap_used, reason = "test assertions")]

mod native_activation;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::{Id, MemoryStorage, Params};

/// The lock's page of settings on a device with or without a touch screen.
fn lock_page(touch: bool) -> Vec<String> {
    let device = DeviceInfo::new(
        "test",
        "Test",
        DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg90, None),
        InputCaps::new(true, touch, false, &[]),
        MemoryClass::Small,
    );
    let card = native_activation::SharedCard::new(
        MemoryStorage::new().with(".paperpaw/plugins/setup/done", ""),
    );
    let builder = Kernel::builder(device)
        .storage(card.clone())
        .user_config("[help]\nshow=false\n[core]\nlanguage=en\n")
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
        Params::new().with("id", "lock.group"),
    );
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .iter()
        .map(|part| part.label.to_string())
        .collect()
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs native Settings and Lock"
)]
fn lock_touch_is_a_row_on_touch_devices_only() {
    let has = |rows: &[String]| rows.iter().any(|row| row.contains("Lock touch"));
    let buttons = lock_page(false);
    assert!(
        buttons.iter().any(|row| row.contains("Screen lock")),
        "the page is there: {buttons:?}"
    );
    assert!(!has(&buttons), "{buttons:?}");
    assert!(has(&lock_page(true)), "{:?}", lock_page(true));
}
