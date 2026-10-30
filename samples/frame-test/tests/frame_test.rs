//! The sample in a kernel, as on an X4: it runs at its pace, full screen,
//! takes raw keys, says what it measured, and *Back* leaves it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use core::time::Duration;

use paperpaw_core::Kernel;
use paperpaw_core::sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, Params, PixelFormat,
};
use paperpaw_platform::{ButtonState, InputEvent, RefreshMode, Rotation};
use paperpaw_sample_frame_test::{FRAME, FrameTest, SCREEN};

/// An X4: black and white, buttons only.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

fn button(kernel: &mut Kernel, at: u64, name: &'static str, state: ButtonState) {
    kernel.handle(Instant(at), InputEvent::Button { name, state });
}

/// What the screen says, its parts' labels together.
fn said(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .iter()
        .map(|p| p.label.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn runs_at_its_pace_and_says_what_it_measured() {
    // With the standard app's fonts, as on a device.
    let builder = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new().with(".paperpaw/plugins/setup/done", ""))
        .user_config("[core]\nlanguage = en\n[help]\nshow = false\n")
        .plugin(FrameTest);
    let mut kernel = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let effects = kernel.open_screen(Instant(10), SCREEN, Params::new());
    assert_eq!(effects.pace(), Some(FRAME));
    let mut now = 10;
    let mut modes = Vec::new();
    for _ in 0..5 {
        let effects = kernel.tick(Instant(now));
        if let Some((area, mode)) = effects.refresh() {
            modes.push(mode);
            kernel.render(&mut |_, _| {});
            kernel.frame_shown(
                area,
                mode,
                Duration::from_millis(5),
                Duration::from_millis(300),
                Instant(now + 300),
            );
        }
        now = effects.wake_at().unwrap().0;
    }
    assert!(modes.len() >= 4, "{modes:?}");
    assert!(modes.iter().all(|m| *m == RefreshMode::Mono), "{modes:?}");
    let text = said(&mut kernel);
    assert!(text.contains("fps (refresh 300 ms)"), "{text}");
    assert!(text.contains("hot ") && text.contains("us"), "{text}");
    // Up doubles the work; confirm switches to ordinary code.
    button(&mut kernel, now, "up", ButtonState::Pressed);
    button(&mut kernel, now + 50, "up", ButtonState::Released);
    button(&mut kernel, now + 60, "confirm", ButtonState::Pressed);
    button(&mut kernel, now + 90, "confirm", ButtonState::Released);
    kernel.tick(Instant(now + 100));
    let text = said(&mut kernel);
    assert!(text.contains("work 40000 steps"), "{text}");
    assert!(text.contains("(ordinary)"), "{text}");
    // Back leaves.
    button(&mut kernel, now + 200, "back", ButtonState::Pressed);
    button(&mut kernel, now + 250, "back", ButtonState::Released);
    assert_ne!(kernel.active_screen(), Some(&SCREEN));
}
