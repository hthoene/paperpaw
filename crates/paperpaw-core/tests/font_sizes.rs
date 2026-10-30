//! The sizes of book text offered: those the reading font has. A font
//! pack's largest bounds them, a scalable font (or none) has every size.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::DEVICE;
use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::settings;
use paperpaw_core::sdk::{MemoryStorage, SettingKind, SettingValue};
use paperpaw_platform::Instant;
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

/// A data plugin `font-small`, family Small, a pack of 10 and 20 pixels.
fn small_font() -> MemoryStorage {
    let mut builder = PackBuilder::new();
    for size in [10.0, 20.0] {
        let metrics = LineMetrics::new(size * 0.8, size * 0.2, size * 1.2);
        builder.add_strike(size, metrics, vec![('a', Glyph::empty(size / 2.0))]);
    }
    MemoryStorage::new()
        .with(
            "plugins/font-small/plugin.ini",
            "[plugin]\nname = Small\n\n[family]\nname = Small\nregular = small.ppfp\n",
        )
        .with("plugins/font-small/fonts/small.ppfp", builder.build())
}

/// Started with `config`, the small font on its storage.
fn started(config: &str) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(small_font())
        .user_config(config)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

const SIZE: &str = settings::FONT_SIZE.static_str();

fn range(kernel: &Kernel) -> (i64, i64) {
    match kernel.settings().spec(SIZE).map(|s| &s.kind) {
        Some(SettingKind::Int { min, max }) => (*min, *max),
        other => panic!("{other:?}"),
    }
}

fn help(kernel: &Kernel) -> &str {
    let spec = kernel.settings().spec(SIZE).unwrap();
    spec.description.as_deref().unwrap()
}

/// 20 pixels at 100 dpi: 14 points at most. A size chosen before past it
/// gives way to the largest; at the largest, the help says where larger
/// ones are found.
#[test]
fn the_font_size_goes_as_far_as_the_reading_font() {
    let mut kernel = started("[core]\nreading-font = Small\nfont-size = 18\n");
    assert!(kernel.take_warnings().is_empty());
    assert_eq!(range(&kernel), (6, 14));
    assert_eq!(kernel.settings().int(SIZE), 14);
    assert_eq!(kernel.settings().global(SIZE), Some(&SettingValue::Int(14)));
    assert_eq!(help(&kernel), "core.setting.font-size.largest");

    // Smaller: the plain help; past the largest: refused.
    kernel.set_setting(Instant(10), SIZE, "10").unwrap();
    assert_eq!(kernel.settings().int(SIZE), 10);
    assert_eq!(help(&kernel), "core.setting.font-size.help");
    assert!(kernel.set_setting(Instant(20), SIZE, "15").is_err());
    assert_eq!(kernel.settings().int(SIZE), 10);
    kernel.set_setting(Instant(30), SIZE, "14").unwrap();
    assert_eq!(help(&kernel), "core.setting.font-size.largest");
}

/// Without a pack every size; a pack chosen while running takes the size
/// down to its largest.
#[test]
fn every_size_without_a_pack() {
    let mut kernel = started("[core]\nfont-size = 18\n");
    assert_eq!(range(&kernel), (6, 40));
    assert_eq!(kernel.settings().int(SIZE), 18);
    assert_eq!(help(&kernel), "core.setting.font-size.help");

    let font = settings::READING_FONT.static_str();
    kernel.set_setting(Instant(10), font, "Small").unwrap();
    assert_eq!(range(&kernel), (6, 14));
    assert_eq!(kernel.settings().int(SIZE), 14);
    assert_eq!(help(&kernel), "core.setting.font-size.largest");
}
