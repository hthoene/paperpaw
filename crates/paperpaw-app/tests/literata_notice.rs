//! Literata's upgrade hint waits for real startup flows and links to its pack.
#![cfg(all(
    feature = "native-linked",
    feature = "native-linked-reading",
    feature = "packs"
))]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
mod native_activation;
use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::{MemoryStorage, builtin::actions};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "notice-test",
    "Notice test",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down"],
    ),
    MemoryClass::Small,
);
const CONFIG: &str = "[core]\nlanguage = en\nreading-font = Literata\n";
const NOTICE: &str = "core.literata-notice";
const BOOK: &str = "books/alice-in-wonderland.txt";

fn settle(kernel: &mut Kernel, mut effects: Effects, mut now: u64) {
    for _ in 0..10_000 {
        let Some(at) = effects.wake_at() else { break };
        if at.0 > now + 1000 {
            break;
        }
        now = now.max(at.0);
        effects = kernel.tick(Instant(now));
    }
    kernel.render(&mut |_, _| {});
}

fn start(storage: MemoryStorage) -> Kernel {
    let storage = storage.with("paperpaw.ini", CONFIG);
    let (builder, card) = native_activation::quiet_on(DEVICE, storage, CONFIG);
    let builder = paperpaw_app::install(builder, build_fonts());
    let mut kernel = native_activation::build_on(builder, card);
    let effects = kernel.start(Instant(0));
    settle(&mut kernel, effects, 0);
    kernel
}

fn seen(kernel: &Kernel) -> bool {
    String::from_utf8(paperpaw_sdk::read_file(kernel.storage(), "paperpaw.ini", 65536).unwrap())
        .unwrap()
        .contains("literata-pack-notice-seen = on")
}

#[test]
fn reopening_the_last_book_defers_the_hint_until_home() {
    let storage = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            BOOK,
            include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
        )
        .with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = {BOOK}\n"),
        );
    let mut kernel = start(storage);
    assert_eq!(kernel.active_screen().unwrap().as_str(), "reader.screen");
    assert!(!kernel.screens().any(|s| s.as_str() == NOTICE));
    assert!(!seen(&kernel));
    kernel.run_action(Instant(100), actions::BACK);
    let effects = kernel.tick(Instant(110));
    settle(&mut kernel, effects, 110);
    assert_eq!(kernel.active_screen().unwrap().as_str(), NOTICE);
}

#[test]
fn first_run_setup_is_not_covered_by_the_hint() {
    let mut kernel = start(MemoryStorage::new());
    assert_eq!(kernel.active_screen().unwrap().as_str(), "setup.guide");
    kernel.tick(Instant(100));
    assert!(!kernel.screens().any(|s| s.as_str() == NOTICE));
    assert!(!seen(&kernel));
}

/// The fonts this build has: the packs, or the scalable fonts where a
/// package built with it switches `outline` on and `packs` is off.
fn build_fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(not(any(feature = "packs", not(feature = "outline"))))]
    return paperpaw_app::Fonts::Outline;
}
