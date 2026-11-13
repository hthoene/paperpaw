//! The sleeping cat built into the firmware is stored as small as it can be
//! (4-bit grays, #258), and decodes to the very pixels it always had: the
//! 760 × 501 drawing in 16 grays, row by row as the screen gets them.

use paperpaw_plugin_sleep::cat::PICTURE;
use paperpaw_sdk::image::{Fit, Format, ImageError, ImageSink, decode};

/// FNV-1a (64 bits) of the drawing's gray pixels, row after row, as the
/// 8-bit PNG Beta 3 shipped decodes them.
const PIXELS: u64 = 0xdca7_2b88_57f6_d116;

/// Hashes what the decoder hands on.
struct Hashed {
    size: Option<(u32, u32)>,
    rows: u32,
    hash: u64,
}

impl ImageSink for Hashed {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        self.size = Some((width, height));
        Ok(())
    }

    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        self.rows += 1;
        for byte in gray {
            self.hash = (self.hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(())
    }
}

#[test]
fn the_cat_decodes_to_the_pixels_it_always_had() {
    let mut sink = Hashed {
        size: None,
        rows: 0,
        hash: 0xcbf2_9ce4_8422_2325,
    };
    let mut source = PICTURE;
    decode(Format::Png, &mut source, Fit::side(4096), &mut sink).expect("decodes");
    assert_eq!(sink.size, Some((760, 501)));
    assert_eq!(sink.rows, 501);
    assert_eq!(sink.hash, PIXELS, "the cat's pixels changed");
}

#[test]
fn the_cat_is_stored_in_four_bit_grays() {
    // The header chunk's body: bit depth (byte 24), colour type (byte 25,
    // 0 = gray), interlace (byte 28, none).
    assert_eq!(PICTURE.get(24..26), Some(&[4, 0][..]));
    assert_eq!(PICTURE.get(28), Some(&0));
}
