//! The interlinear sample: its toy translation, and the plugin on the
//! shipped reader in both ways, a translation above the lines and one
//! after each paragraph, opened by a tap.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sample_interlinear::{Interlinear, translate, translate_word};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::{MemoryStorage, Params};

#[test]
fn the_toy_translation_keeps_punctuation_and_unknown_words() {
    assert_eq!(translate_word("The"), "die");
    assert_eq!(translate_word("rabbit,"), "Kaninchen,");
    assert_eq!(translate_word("“Alice"), "“Alice");
    assert_eq!(translate("she said so."), "sie sagte so.");
}

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-both",
    "Test both",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(
        true,
        true,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const BOOK: &str = "books/alice-in-wonderland.txt";

/// The book open on its fifth page with `config`, drawn: the screen.
fn page(config: &str) -> Vec<u8> {
    let storage = MemoryStorage::new()
        .with(
            BOOK,
            include_str!("../../library/books/alice-in-wonderland.txt"),
        )
        .with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(format!("[help]\nshow = false\n{config}"));
    let mut kernel = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs)
        .plugin(Interlinear)
        .build()
        .unwrap();
    let mut now = 0;
    let settle = |kernel: &mut Kernel, effects: Effects, now: &mut u64| {
        let mut effects = effects;
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else { return };
            if at.0 > *now + 1000 {
                return;
            }
            *now = (*now).max(at.0);
            effects = kernel.tick(Instant(*now));
        }
    };
    let effects = kernel.start(Instant(0));
    settle(&mut kernel, effects, &mut now);
    now += 100;
    let open = Params::new().with(params::PATH, BOOK);
    let effects = kernel.open_slot(Instant(now), slots::READER, open);
    settle(&mut kernel, effects, &mut now);
    for _ in 0..4 {
        now += 100;
        let effects = kernel.run_action(Instant(now), paperpaw_sdk::builtin::actions::NEXT_PAGE);
        settle(&mut kernel, effects, &mut now);
    }
    let mut screen = vec![255u8; 480 * 800];
    kernel.render(&mut |y, view| {
        for row in 0..view.height {
            for x in 0..view.width {
                screen[usize::from(y + row) * 480 + usize::from(x)] = view.pixel(x, row).luma();
            }
        }
    });
    screen
}

#[test]
fn both_ways_draw_and_change_the_page() {
    let plain = page("[interlinear]\nabove = off\nparallel = off\n");
    let above = page("[interlinear]\nabove = on\nparallel = off\n");
    let parallel = page("[interlinear]\nabove = off\nparallel = on\n");
    assert_ne!(plain, above);
    assert_ne!(plain, parallel);
    assert_ne!(above, parallel);
}
