//! The language pack the firmware carries compressed (German, #251): its
//! compiled catalog as one raw deflate stream, inflated piece by piece
//! as it is written to the card (`paperpaw_core::LanguageSeed::catalog`),
//! so that no more than the history window and the decoder are ever on the
//! heap, never the catalog.

use alloc::vec::Vec;
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress};

/// The history window: the largest deflate has (a power of two, so that
/// the position wraps with a mask).
const WINDOW: usize = 32 * 1024;

/// Inflates the raw deflate stream `packed`, handing what comes out to
/// `sink` in pieces of at most 32 KiB (`WINDOW`), in order; `false` where
/// the data is corrupt or short, `sink` said stop (`false`), or the
/// room for the window is not there.
pub fn inflate_to(packed: &[u8], sink: &mut dyn FnMut(&[u8]) -> bool) -> bool {
    let mut window: Vec<u8> = Vec::new();
    if window.try_reserve_exact(WINDOW).is_err() {
        return false;
    }
    window.resize(WINDOW, 0);
    // Made in place on the heap: the decoder is 11 KB, no frame may hold it.
    let mut decoder: Vec<DecompressorOxide> = Vec::new();
    if decoder.try_reserve_exact(1).is_err() {
        return false;
    }
    decoder.resize_with(1, DecompressorOxide::default);
    let Some(state) = decoder.first_mut() else {
        return false;
    };
    let (mut input, mut at) = (packed, 0_usize);
    loop {
        let (status, used, made) = decompress(state, input, &mut window, at, 0);
        input = input.get(used..).unwrap_or_default();
        if made > 0 && !window.get(at..at + made).is_some_and(&mut *sink) {
            return false;
        }
        at = (at + made) & (WINDOW - 1);
        match status {
            TINFLStatus::Done => return true,
            TINFLStatus::HasMoreOutput => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(data: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec(data, 9)
    }

    fn unpacked(packed: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        inflate_to(packed, &mut |piece| {
            out.extend_from_slice(piece);
            true
        })
        .then_some(out)
    }

    #[test]
    fn a_stream_larger_than_the_window_comes_out_whole_and_in_order() {
        let data: Vec<u8> = (0..200_000_u32)
            .map(|n| u8::try_from((n * 7 + n / 251) % 253).unwrap_or(0))
            .collect();
        assert_eq!(unpacked(&pack(&data)), Some(data));
    }

    #[test]
    fn short_or_broken_data_and_a_sink_that_stops_fail() {
        let data = alloc::vec![b'x'; 100_000];
        let packed = pack(&data);
        assert_eq!(unpacked(&packed[..packed.len() / 2]), None);
        assert_eq!(unpacked(&[0xff; 32]), None);
        let mut pieces = 0;
        let stopped = inflate_to(&packed, &mut |_| {
            pieces += 1;
            false
        });
        assert!(!stopped && pieces == 1);
    }
}
