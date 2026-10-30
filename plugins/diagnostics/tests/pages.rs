//! The diagnostics screen with the real kernel: its page stays within the
//! pages set when they become fewer, and it is reset by a held finger as
//! by a held *Select*.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::{
    ButtonState, InputEvent, Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_diagnostics::Diagnostics;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
};

const fn device(touch: bool) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, touch, false, &[]),
        MemoryClass::Large,
    )
}

struct Test {
    kernel: Kernel,
    now: u64,
}

impl Test {
    fn new(touch: bool) -> Self {
        // Its screen as the home screen, chosen as a user would.
        let mut kernel = Kernel::builder(device(touch))
            .storage(MemoryStorage::new())
            .user_config("[slots]\ncore.home = diagnostics.screen\n")
            .plugin(paperpaw_plugin_fonts_default::DefaultFonts)
            .plugin(Diagnostics)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self { kernel, now: 0 }
    }

    fn press(&mut self, name: &'static str) {
        self.now += 200;
        let at = Instant(self.now);
        for (later, state) in [(0, ButtonState::Pressed), (50, ButtonState::Released)] {
            self.kernel
                .handle(at.add_millis(later), InputEvent::Button { name, state });
        }
        self.kernel.render(&mut |_, _| {});
    }

    /// A finger held on the screen for a second.
    fn hold(&mut self, x: i32, y: i32) {
        self.now += 200;
        let event = |phase| {
            InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                Point::new(x, y),
                PointerTool::Finger,
                1000,
            ))
        };
        self.kernel
            .handle(Instant(self.now), event(PointerPhase::Down));
        self.now += 1000;
        self.kernel.tick(Instant(self.now));
        self.kernel
            .handle(Instant(self.now), event(PointerPhase::Up));
        self.kernel.render(&mut |_, _| {});
    }

    fn page(&mut self) -> String {
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .find(|l| l.starts_with("Page "))
            .unwrap_or_default()
    }
}

#[test]
fn the_page_stays_within_fewer_pages() {
    let mut test = Test::new(false);
    for _ in 0..10 {
        test.press("page-forward");
    }
    assert_eq!(test.page(), "Page 11 of 20");
    test.now += 200;
    test.kernel
        .set_setting(Instant(test.now), "diagnostics.pages", "3")
        .unwrap();
    test.kernel.render(&mut |_, _| {});
    assert_eq!(test.page(), "Page 3 of 3");
    test.press("page-back");
    assert_eq!(test.page(), "Page 2 of 3");
}

#[test]
fn a_held_finger_resets_it() {
    let mut test = Test::new(true);
    for _ in 0..4 {
        test.press("page-forward");
    }
    assert_eq!(test.page(), "Page 5 of 20");
    test.hold(240, 400);
    assert_eq!(test.page(), "Page 1 of 20");
}
