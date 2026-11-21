//! Another plugin's screen chosen in `sleep.show` is what the device shows
//! when it sleeps (#354): the sleep plugin binds the slot `core.sleep` to it.
#![cfg(all(
    feature = "native-linked",
    feature = "native-linked-reading",
    feature = "native-linked-store",
    feature = "packs"
))]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
mod native_activation;
use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::{MemoryStorage, Params, builtin::slots};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "sleep-foreign",
    "Sleep foreign",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);
const CARD: &str = "vocabulary.sleep-card";

fn settle(kernel: &mut Kernel, mut effects: Effects, mut now: u64) -> u64 {
    for _ in 0..10_000 {
        let Some(at) = effects.wake_at() else { break };
        if at.0 > now + 1000 {
            break;
        }
        now = now.max(at.0);
        effects = kernel.tick(Instant(now));
    }
    kernel.render(&mut |_, _| {});
    now
}

fn start(config: &str) -> (Kernel, u64) {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let (builder, card) = native_activation::quiet_on(DEVICE, storage, config);
    let builder = paperpaw_app::install(builder, build_fonts());
    let mut kernel = native_activation::build_on(builder, card);
    let effects = kernel.start(Instant(0));
    let now = settle(&mut kernel, effects, 0);
    (kernel, now)
}

fn asleep(kernel: &mut Kernel, now: u64) -> String {
    drop(native_activation::take_failed_calls());
    let effects = kernel.open_slot(Instant(now + 100), slots::SLEEP, Params::new());
    settle(kernel, effects, now + 100);
    let failed = native_activation::take_failed_calls();
    assert!(failed.is_empty(), "{failed:?}");
    kernel.active_screen().unwrap().as_str().to_owned()
}

#[test]
fn the_screen_chosen_at_start_is_shown_asleep() {
    let (mut kernel, now) = start("[core]\nlanguage = en\n[sleep]\nshow = vocabulary.sleep-card\n");
    assert_eq!(asleep(&mut kernel, now), CARD);
}

#[test]
fn the_screen_chosen_later_is_shown_asleep() {
    let (mut kernel, now) = start("[core]\nlanguage = en\n");
    let effects = kernel
        .set_setting(Instant(now + 50), "sleep.show", CARD)
        .unwrap();
    let now = settle(&mut kernel, effects, now + 50);
    assert_eq!(asleep(&mut kernel, now), CARD);
}

/// The fonts this build has: the packs, or the scalable fonts where a
/// package built with it switches `outline` on and `packs` is off.
fn build_fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(not(any(feature = "packs", not(feature = "outline"))))]
    return paperpaw_app::Fonts::Outline;
}
