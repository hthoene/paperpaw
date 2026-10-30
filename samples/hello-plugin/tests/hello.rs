//! The sample plugin in a kernel, as on a device: its action opens its
//! screen, a button closes it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_core::sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
};
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_sample_hello::{Hello, SCREEN};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(200, 300, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

fn press(kernel: &mut Kernel, at: u64, name: &'static str) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        kernel.handle(Instant(at), InputEvent::Button { name, state });
    }
}

#[test]
fn opens_and_closes_by_buttons() {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .user_config("[keymap.global]\nbutton.hello = hello.open\n")
        .plugin(Hello)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(kernel.take_warnings().is_empty());
    press(&mut kernel, 10, "hello");
    assert_eq!(kernel.active_screen(), Some(&SCREEN));
    press(&mut kernel, 20, "confirm");
    assert_ne!(kernel.active_screen(), Some(&SCREEN));
    // Its setting is there, with its default.
    assert_eq!(kernel.settings().text("hello.name"), "");
}

#[test]
fn translations_are_complete() {
    use paperpaw_i18n::{Catalog, check};
    let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}
