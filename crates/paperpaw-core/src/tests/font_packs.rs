//! What font packs read from the storage may take of memory follows the
//! device's profile, not the processor the kernel runs on: an emulator of
//! the X4 refuses the packs the X4 refuses.

use super::*;
use paperpaw_platform::MemoryClass;
use paperpaw_text::{
    FontError, Glyph, LineMetrics, PackBuilder, PackLimits, PackSource, StoredPack,
};

struct Bytes(Vec<u8>);

impl PackSource for Bytes {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let at = at as usize;
        match self.0.get(at..at + buf.len()) {
            Some(bytes) => {
                buf.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

/// A character every 200 code points: an index of a segment each, 16 KB
/// for 1,000 of them.
fn scattered() -> Rc<Bytes> {
    let glyphs = (0..1000u32)
        .filter_map(|i| Some((char::from_u32(0x1_0000 + 200 * i)?, Glyph::empty(1.0))))
        .collect();
    let mut builder = PackBuilder::new();
    builder.add_strike(10.0, LineMetrics::new(8.0, 2.0, 12.0), glyphs);
    Rc::new(Bytes(builder.build()))
}

fn device(memory: MemoryClass) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(90, 90, 100, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, true, false, &[]),
        memory,
    )
}

#[test]
fn packs_are_opened_within_the_device_s_limits() {
    let small = Kernel::builder(device(MemoryClass::Small)).build().unwrap();
    let limits = small.shared.fonts.pack_limits();
    assert_eq!(limits, PackLimits::SMALL);
    assert_eq!(
        StoredPack::open_within(scattered(), limits).err(),
        Some(FontError::TooLarge)
    );
    let large = Kernel::builder(device(MemoryClass::Large)).build().unwrap();
    let limits = large.shared.fonts.pack_limits();
    assert_eq!(limits, PackLimits::LARGE);
    assert!(StoredPack::open_within(scattered(), limits).is_ok());
}
