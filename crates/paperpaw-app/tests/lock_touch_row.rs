//! The *Lock touch* row is on the lock's settings page of a touch device and
//! not on one without touch, where it would do nothing (#277).
#![allow(clippy::unwrap_used, reason = "test assertions")]

mod native_activation;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat, Point,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Id, MemoryStorage, Params, SettingValue};

/// A started kernel of a device with or without a touch screen.
fn started(touch: bool) -> Kernel {
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
    kernel
}

/// The lock's page of settings on a device with or without a touch screen.
fn lock_page(touch: bool) -> Vec<String> {
    let mut kernel = started(touch);
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

/// *Lock touch* in the quick panel locks touch and closes the panel, so that
/// the page is back and the buttons turn pages (#234): the entry is an action,
/// not the setting's switch, which would leave the panel open on a screen that
/// can no longer be touched. Unlocking is an action too, which the held power
/// button and the home key run.
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs native Settings and Lock"
)]
fn lock_touch_in_the_panel_locks_and_closes_it() {
    let mut kernel = started(true);
    let mut now = 300;
    let settle = |kernel: &mut Kernel, now: &mut u64| {
        for _ in 0..50 {
            kernel.render(&mut |_, _| {});
            *now += 100;
            kernel.tick(Instant(*now));
        }
    };
    kernel.run_action(Instant(now), actions::QUICK_PANEL);
    settle(&mut kernel, &mut now);
    let rows = |kernel: &mut Kernel| -> Vec<(String, paperpaw_platform::Rect)> {
        kernel
            .describe()
            .iter()
            .map(|part| (part.label.to_string(), part.area))
            .collect()
    };
    let panel = rows(&mut kernel);
    let (_, area) = panel
        .iter()
        .find(|(label, _)| label.contains("Lock touch"))
        .unwrap_or_else(|| panic!("Lock touch in the quick panel: {panel:?}"));
    let at = Point::new(
        i32::from(area.x) + i32::from(area.width) / 2,
        i32::from(area.y) + i32::from(area.height) / 2,
    );
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        now += 60;
        let event = InputEvent::Pointer(PointerEvent::new(0, phase, at, PointerTool::Finger, 1000));
        kernel.handle(Instant(now), event);
    }
    settle(&mut kernel, &mut now);
    assert_eq!(
        kernel.settings().global("lock.touch"),
        Some(&SettingValue::Bool(true))
    );
    let after = rows(&mut kernel);
    assert!(
        !after.iter().any(|(label, _)| label.contains("Lock touch")),
        "the panel is closed: {after:?}"
    );
    // The way out that needs no touch: the toggle action.
    kernel.run_action(Instant(now + 100), Id::new_static("lock.touch-toggle"));
    settle(&mut kernel, &mut now);
    assert_eq!(
        kernel.settings().global("lock.touch"),
        Some(&SettingValue::Bool(false))
    );
}
