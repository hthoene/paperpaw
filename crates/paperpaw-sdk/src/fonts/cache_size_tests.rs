//! [`Fonts::fit_cache`]: the glyph cache while a pack of thousands of
//! characters is open (#203).
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]

use super::*;
use crate::{FontFace, MemoryStorage};
use alloc::rc::Rc;
use alloc::vec::Vec;
use paperpaw_platform::Storage;
use paperpaw_text::{Glyph, LARGE_SCRIPT, LineMetrics, PackBuilder, PackLimits};

fn pack(chars: usize) -> Vec<u8> {
    let mut pack = PackBuilder::new();
    pack.add_strike(
        10.0,
        LineMetrics::new(8.0, 2.0, 12.0),
        (0..chars)
            .map(|n| {
                (
                    char::from_u32(0x4e00 + n as u32).unwrap(),
                    Glyph::empty(10.0),
                )
            })
            .collect(),
    );
    pack.build()
}

#[test]
fn a_pack_of_thousands_of_characters_gets_the_devices_larger_cache() {
    let mut fonts = Fonts::new(4096);
    for (id, path, family) in [
        ("a.big", "plugins/a/big.ppfp", "Big"),
        ("a.small", "plugins/a/small.ppfp", "Small"),
    ] {
        fonts
            .add_face(FontFace::new(
                Id::new_static(id),
                path,
                family,
                false,
                false,
            ))
            .unwrap();
    }
    let card: Rc<dyn Storage> = Rc::new(
        MemoryStorage::new()
            .with("plugins/a/big.ppfp", pack(LARGE_SCRIPT))
            .with("plugins/a/small.ppfp", pack(LARGE_SCRIPT - 1)),
    );
    let large = PackLimits::SMALL.glyph_cache_bytes();
    assert!(large > 4096);
    let capacity = |fonts: &Fonts| fonts.cache.borrow().capacity();
    let big = [Id::new_static("a.big")];
    let small = [Id::new_static("a.small")];
    assert!(fonts.try_keep_open(&big, &card).unwrap().is_empty());
    assert_eq!(capacity(&fonts), large);
    // A pack of fewer characters: the shell's cache, no memory more.
    assert!(fonts.try_keep_open(&small, &card).unwrap().is_empty());
    assert_eq!(capacity(&fonts), 4096);
    assert!(fonts.try_keep_open(&big, &card).unwrap().is_empty());
    assert!(fonts.try_keep_open(&[], &card).unwrap().is_empty());
    assert_eq!(capacity(&fonts), 4096);
    // A device with a large cache of its own keeps it.
    let mut fonts = Fonts::new(1 << 20).with_pack_limits(PackLimits::LARGE);
    fonts
        .add_face(FontFace::new(
            Id::new_static("a.big"),
            "plugins/a/big.ppfp",
            "Big",
            false,
            false,
        ))
        .unwrap();
    assert!(fonts.try_keep_open(&big, &card).unwrap().is_empty());
    assert_eq!(capacity(&fonts), 1 << 20);
}
