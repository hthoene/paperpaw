//! The action `vocabulary.scan-card` of Beta 3, which key maps and rules may
//! name, opens the screen that reads the cards' files, all of them (#277).
#![allow(clippy::unwrap_used, reason = "test assertions")]

mod native_activation;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::{Id, MemoryStorage};

#[test]
#[cfg_attr(
    not(all(feature = "native-linked-reading", feature = "native-linked-store")),
    ignore = "needs the native flash cards linked"
)]
fn the_scan_action_opens_the_update_screen_of_the_card_files() {
    let device = DeviceInfo::new(
        "test-x4",
        "Test X4",
        DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg90, None),
        InputCaps::new(true, false, false, &[]),
        MemoryClass::Small,
    );
    let card = native_activation::SharedCard::new(
        MemoryStorage::new().with(".paperpaw/plugins/setup/done", ""),
    );
    let builder = Kernel::builder(device)
        .storage(card.clone())
        .user_config("[help]\nshow=false\n[reader]\nopen-last=false\n")
        .frame_rows(192)
        .row_align(8);
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    let fonts = paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    let fonts = paperpaw_app::Fonts::Outline;
    let builder = paperpaw_app::install(builder, fonts);
    let mut kernel = native_activation::build_on(builder, card.handle());
    kernel.start(Instant(0));
    let mut now = 100;
    let mut effects = kernel.run_action(Instant(now), Id::new_static("vocabulary.scan-card"));
    for _ in 0..8 {
        let Some(at) = effects.wake_at() else { break };
        now = now.max(at.0);
        effects = kernel.tick(Instant(now));
    }
    assert_eq!(
        kernel.active_screen().map(ToString::to_string).as_deref(),
        Some("vocabulary.sd-card")
    );
}
