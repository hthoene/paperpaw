//! JPEG decoding against images built here: flat levels come out exact,
//! progressive and baseline encodings of the same blocks give the same
//! pixels, restart markers change nothing, and damaged, truncated or
//! oddly ordered files fail cleanly (or show what they have, where that
//! is meant).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Checked, Dribble, Failing, Jpeg, decoded, find_marker, run};
use paperpaw_image::{Fit, Format, ImageError, decode, dimensions};
use proptest::prelude::*;

const SAMPLINGS: [&[(u8, u8)]; 6] = [
    &[(1, 1)],
    &[(1, 1), (1, 1), (1, 1)],
    &[(2, 2), (1, 1), (1, 1)],
    &[(2, 1), (1, 1), (1, 1)],
    &[(1, 2), (1, 1), (1, 1)],
    // Luma at less than the largest resolution: stretched.
    &[(1, 1), (2, 2), (2, 2)],
];

fn jpeg_result(data: &[u8], fit: Fit, color: bool) -> (Result<(), ImageError>, Checked) {
    run(Format::Jpeg, data, fit, color)
}

#[test]
fn flat_blocks_come_out_as_one_level_at_every_scale() {
    // DC d (times its quantizer 8) is d levels from the middle gray.
    for sampling in SAMPLINGS {
        for (w, h) in [(1, 1), (8, 8), (13, 9), (40, 17)] {
            let jpeg = Jpeg::flat(w, h, sampling, [-50, 0, 0]);
            for side in [1, 2, 3, 5, 8, 20, 100] {
                for data in [jpeg.baseline(), jpeg.progressive()] {
                    let image = decoded(&data, Fit::side(side), false);
                    assert!(
                        image.pixels.iter().all(|&p| p.abs_diff(78) <= 1),
                        "{sampling:?} {w}x{h} at {side}: {:?}",
                        &image.pixels[..image.pixels.len().min(16)]
                    );
                }
            }
        }
    }
}

#[test]
fn neutral_chroma_gives_gray_colour_rows() {
    for sampling in &SAMPLINGS[1..] {
        let jpeg = Jpeg::flat(19, 11, sampling, [30, 0, 0]);
        for data in [jpeg.baseline(), jpeg.progressive()] {
            let image = decoded(&data, Fit::side(100), true);
            assert_eq!((image.width, image.height), (19, 11));
            for p in image.pixels.chunks(3) {
                assert!(
                    p.iter().all(|&c| c.abs_diff(158) <= 1),
                    "{sampling:?}: {p:?}"
                );
            }
        }
    }
}

#[test]
fn chroma_turns_into_its_colour() {
    // Cr far above the middle: red; Cb: blue.
    let red = decoded(
        &Jpeg::flat(16, 16, SAMPLINGS[2], [0, 0, 60]).baseline(),
        Fit::side(16),
        true,
    );
    let [r, g, b] = [red.pixels[0], red.pixels[1], red.pixels[2]];
    assert!(r > 200 && g < 100 && b <= 128, "{r} {g} {b}");
    let blue = decoded(
        &Jpeg::flat(16, 16, SAMPLINGS[2], [0, 60, 0]).baseline(),
        Fit::side(16),
        true,
    );
    let [r, g, b] = [blue.pixels[0], blue.pixels[1], blue.pixels[2]];
    assert!(b > 200 && r <= 128 && g < 120, "{r} {g} {b}");
}

#[test]
fn only_luma_counts_for_gray() {
    // Chroma changes colour rows only.
    let plain = Jpeg::flat(24, 16, SAMPLINGS[2], [10, 0, 0]);
    let tinted = Jpeg::flat(24, 16, SAMPLINGS[2], [10, 40, -40]);
    let a = decoded(&plain.baseline(), Fit::side(100), false);
    let b = decoded(&tinted.baseline(), Fit::side(100), false);
    assert_eq!(a.pixels, b.pixels);
    let a = decoded(&plain.baseline(), Fit::side(100), true);
    let b = decoded(&tinted.baseline(), Fit::side(100), true);
    assert_ne!(a.pixels, b.pixels);
}

fn sampling() -> impl Strategy<Value = &'static [(u8, u8)]> {
    (0..SAMPLINGS.len()).prop_map(|i| SAMPLINGS[i])
}

proptest! {
    #![proptest_config(common::config(96))]

    /// The same blocks sent progressively (bits of the DC and AC in
    /// several scans) or at once give the same image, in gray and colour,
    /// at any size.
    #[test]
    fn progressive_and_baseline_agree(
        w in 1u16..60,
        h in 1u16..60,
        sampling in sampling(),
        seed in any::<u64>(),
        side in 1u32..80,
        color in any::<bool>(),
    ) {
        let jpeg = Jpeg::new(w, h, sampling, seed, true);
        let fit = Fit::side(side);
        let baseline = decoded(&jpeg.baseline(), fit, color);
        let progressive = decoded(&jpeg.progressive(), fit, color);
        prop_assert_eq!((baseline.width, baseline.height), (progressive.width, progressive.height));
        prop_assert_eq!(baseline.pixels, progressive.pixels);
    }

    /// Restart markers change nothing in what is decoded.
    #[test]
    fn restart_markers_change_nothing(
        w in 1u16..50,
        h in 1u16..50,
        sampling in sampling(),
        seed in any::<u64>(),
        restart in 1u16..7,
        color in any::<bool>(),
    ) {
        let plain = Jpeg::new(w, h, sampling, seed, true);
        let mut restarted = plain.clone();
        restarted.restart = restart;
        let fit = Fit::side(64);
        for (a, b) in [
            (plain.baseline(), restarted.baseline()),
            (plain.progressive(), restarted.progressive()),
        ] {
            prop_assert_ne!(&a, &b);
            let a = decoded(&a, fit, color);
            let b = decoded(&b, fit, color);
            prop_assert_eq!(a.pixels, b.pixels);
        }
    }

    /// The size: as `Fit` says, inside the box, as `dimensions` read it.
    #[test]
    fn size_is_what_fit_says(
        w in 1u16..300,
        h in 1u16..300,
        sampling in sampling(),
        short in 1u32..120,
        long in 1u32..200,
        progressive in any::<bool>(),
    ) {
        let jpeg = Jpeg::flat(w, h, sampling, [0, 0, 0]);
        let data = if progressive { jpeg.progressive() } else { jpeg.baseline() };
        prop_assert_eq!(dimensions(&data), Some((u32::from(w), u32::from(h))));
        let fit = Fit { short: short.min(long), long: long.max(short) };
        let image = decoded(&data, fit, false);
        prop_assert_eq!((image.width, image.height), fit.size(w.into(), h.into()));
        let (a, b) = (image.width.min(image.height), image.width.max(image.height));
        prop_assert!(a <= fit.short.max(1) && b <= fit.long.max(1), "{}x{} for {:?}", image.width, image.height, fit);
    }

    /// Bytes handed over a few at a time decode the same.
    #[test]
    fn any_read_size_decodes_the_same(
        seed in any::<u64>(),
        step in 1usize..40,
        progressive in any::<bool>(),
    ) {
        let jpeg = Jpeg::new(21, 13, SAMPLINGS[2], seed, true);
        let data = if progressive { jpeg.progressive() } else { jpeg.baseline() };
        let whole = decoded(&data, Fit::side(100), true);
        let mut source = Dribble { data: &data, step };
        let mut sink = Checked::colour();
        decode(Format::Jpeg, &mut source, Fit::side(100), &mut sink).unwrap();
        prop_assert_eq!(whole.pixels, sink.pixels);
    }
}

/// Every prefix of `data`: an error or a whole image, never a panic or
/// rows beyond the height (the sink checks).
fn every_prefix(data: &[u8], color: bool) -> usize {
    let mut whole = 0;
    for cut in 0..data.len() {
        let (result, sink) = jpeg_result(&data[..cut], Fit::side(24), color);
        if result.is_ok() {
            assert!(sink.complete());
            whole += 1;
        }
    }
    whole
}

#[test]
fn truncated_baseline_at_every_length_is_an_error() {
    for sampling in [SAMPLINGS[0], SAMPLINGS[2]] {
        let mut jpeg = Jpeg::new(23, 17, sampling, 5, false);
        let data = jpeg.baseline();
        // Only the end-of-image marker may go missing: the data is whole
        // before it.
        assert!(every_prefix(&data, false) <= 2);
        jpeg.restart = 2;
        assert!(every_prefix(&jpeg.baseline(), true) <= 2);
    }
}

#[test]
fn truncated_progressive_at_every_length_never_panics() {
    let mut jpeg = Jpeg::new(23, 17, SAMPLINGS[2], 9, true);
    every_prefix(&jpeg.progressive(), false);
    jpeg.restart = 3;
    every_prefix(&jpeg.progressive(), true);
}

#[test]
fn real_files_cut_short_never_panic() {
    for data in [
        &include_bytes!("data/kafka-title.jpg")[..],
        include_bytes!("data/kafka-progressive.jpg"),
    ] {
        for cut in (0..data.len()).step_by(97) {
            let _ = jpeg_result(&data[..cut], Fit::side(40), false);
        }
    }
}

#[test]
fn progressive_cut_after_a_segment_between_scans_shows_what_came() {
    // Files that end without their last scans show what there is: here
    // the DC's high bits and the AC of the first component, then a
    // comment, then nothing.
    let jpeg = Jpeg::new(32, 24, SAMPLINGS[0], 3, true);
    let data = jpeg.progressive();
    let mut cut = data[..scans(&data)[2]].to_vec();
    cut.extend_from_slice(&[0xff, 0xfe, 0, 4, b'h', b'i']);
    let cut = decoded(&cut, Fit::side(100), false);
    assert_eq!((cut.width, cut.height), (32, 24));
    let whole = decoded(&data, Fit::side(100), false);
    // Only the last bits are missing: close, not the same.
    assert_ne!(cut.pixels, whole.pixels);
    let diff: u32 = cut
        .pixels
        .iter()
        .zip(&whole.pixels)
        .map(|(&a, &b)| u32::from(a.abs_diff(b)))
        .max()
        .unwrap();
    assert!(diff <= 8, "{diff}");
    // Without its refining scans altogether: the same.
    let unrefined = decoded(&jpeg.progressive_scans(false), Fit::side(100), false);
    assert_eq!(unrefined.pixels, cut.pixels);
}

#[test]
fn progressive_without_its_end_marker_shows_the_image() {
    // Some files end without EOI: what there is is shown. The end of the
    // data after the last scan must be no error, neither where
    // `Bits::fill` reads ahead nor where `next_marker` looks for the
    // marker after a scan.
    let built = Jpeg::new(32, 24, SAMPLINGS[0], 3, true).progressive();
    for data in [&built[..], include_bytes!("data/kafka-progressive.jpg")] {
        assert_eq!(&data[data.len() - 2..], [0xff, 0xd9]);
        let whole = decoded(data, Fit::side(100), false);
        let (result, cut) = jpeg_result(&data[..data.len() - 2], Fit::side(100), false);
        assert_eq!(result, Ok(()));
        assert_eq!(cut.pixels, whole.pixels);
    }
}

#[test]
fn baseline_without_its_end_marker_shows_the_image() {
    // `Bits::fill` takes bytes until more than 24 bits are at hand; at
    // the end of the data it goes on with zeros, as after a marker: the
    // bits the last block needs are all there.
    let data = Jpeg::new(32, 24, SAMPLINGS[2], 3, false).baseline();
    let whole = decoded(&data, Fit::side(100), true);
    let cut = decoded(&data[..data.len() - 2], Fit::side(100), true);
    assert_eq!(cut.pixels, whole.pixels);
}

#[test]
fn source_errors_come_back_as_io() {
    let jpeg = Jpeg::new(30, 30, SAMPLINGS[2], 1, true);
    for data in [jpeg.baseline(), jpeg.progressive()] {
        for ok in 0..data.len() {
            let mut source = Failing { data: &data, ok };
            let mut sink = Checked::gray();
            let result = decode(Format::Jpeg, &mut source, Fit::side(30), &mut sink);
            assert_eq!(result, Err(ImageError::Io), "after {ok} bytes");
        }
    }
}

#[test]
fn a_sink_that_stops_stops_decoding() {
    for sampling in [SAMPLINGS[0], SAMPLINGS[2]] {
        let jpeg = Jpeg::new(40, 40, sampling, 2, true);
        for data in [jpeg.baseline(), jpeg.progressive()] {
            for color in [false, true] {
                let mut sink = if color {
                    Checked::colour()
                } else {
                    Checked::gray()
                };
                sink.stop_at = Some(3);
                let mut source = &data[..];
                let result = decode(Format::Jpeg, &mut source, Fit::side(40), &mut sink);
                assert_eq!(result, Err(ImageError::Stopped));
                assert_eq!((sink.rows, sink.after_stop), (3, 1));
            }
        }
    }
}

// ---------------------------------------------------------------------
// Damaged headers and segments

/// The segments of a JPEG, each its marker and its bytes (marker and
/// length included); the entropy-coded data stays with the scan before.
fn segments(data: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut out = vec![(0xd8, data[..2].to_vec())];
    let mut at = 2;
    while at < data.len() {
        let marker = data[at + 1];
        if marker == 0xd9 {
            out.push((marker, data[at..].to_vec()));
            break;
        }
        let len = usize::from(u16::from_be_bytes([data[at + 2], data[at + 3]]));
        let mut end = at + 2 + len;
        if marker == 0xda {
            // Up to the next marker that is not a restart.
            while end < data.len() {
                let next = data.get(end + 1).copied().unwrap_or(0);
                if data[end] == 0xff && next != 0 && !(0xd0..=0xd7).contains(&next) {
                    break;
                }
                end += 1;
            }
        }
        out.push((marker, data[at..end].to_vec()));
        at = end;
    }
    out
}

fn join(segments: &[(u8, Vec<u8>)]) -> Vec<u8> {
    segments.iter().flat_map(|(_, s)| s.clone()).collect()
}

fn small() -> Jpeg {
    Jpeg::new(20, 12, SAMPLINGS[2], 11, true)
}

fn error_of(data: &[u8]) -> ImageError {
    let (result, _) = jpeg_result(data, Fit::side(50), false);
    result.expect_err("an error")
}

#[test]
fn the_builder_makes_what_the_decoder_reads() {
    // The tests below damage these segments one at a time.
    let markers: Vec<u8> = segments(&small().baseline()).iter().map(|s| s.0).collect();
    assert_eq!(markers, [0xd8, 0xfe, 0xdb, 0xc0, 0xc4, 0xda, 0xd9]);
    decoded(&join(&segments(&small().baseline())), Fit::side(50), false);
}

#[test]
fn segments_in_the_wrong_order_fail_cleanly() {
    let parts = segments(&small().baseline());
    let take = |order: &[usize]| -> Vec<u8> {
        join(&order.iter().map(|&i| parts[i].clone()).collect::<Vec<_>>())
    };
    // The scan before the frame.
    assert!(matches!(
        error_of(&take(&[0, 1, 2, 5, 3, 4, 6])),
        ImageError::Unsupported(_) | ImageError::Invalid(_)
    ));
    // The scan before its Huffman tables.
    assert_eq!(
        error_of(&take(&[0, 1, 2, 3, 5, 4, 6])),
        ImageError::Invalid("missing Huffman table")
    );
    // No scan at all.
    assert_eq!(
        error_of(&take(&[0, 1, 2, 3, 4, 6])),
        ImageError::Invalid("no image data")
    );
    // Nothing after the start.
    assert_eq!(error_of(&take(&[0])), ImageError::Invalid("unexpected end"));
    // The scan before the quantization tables: decodes (all zero), no
    // panic.
    let (result, _) = jpeg_result(&take(&[0, 3, 4, 5, 2, 6]), Fit::side(50), false);
    assert!(result.is_ok());
}

#[test]
fn the_same_segment_twice_is_read_again() {
    let parts = segments(&small().baseline());
    let mut twice = parts.clone();
    twice.insert(3, parts[3].clone());
    twice.insert(5, parts[4].clone());
    let a = decoded(&join(&parts), Fit::side(50), false);
    let b = decoded(&join(&twice), Fit::side(50), false);
    assert_eq!(a.pixels, b.pixels);
}

#[test]
fn fill_bytes_and_unknown_segments_are_skipped() {
    let parts = segments(&small().baseline());
    let mut padded = parts.clone();
    // Fill bytes before a marker, an APP segment, a stray RST marker.
    padded[2].1.insert(0, 0xff);
    padded.insert(2, (0xe1, vec![0xff, 0xe1, 0, 6, b'E', b'x', b'i', b'f']));
    padded.insert(2, (0xd3, vec![0xff, 0xd3]));
    let a = decoded(&join(&parts), Fit::side(50), false);
    let b = decoded(&join(&padded), Fit::side(50), false);
    assert_eq!(a.pixels, b.pixels);
}

/// `data` with its frame header (SOF) changed by `change` (the bytes
/// after the length: precision, height, width, count, components).
fn with_frame(data: &[u8], change: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
    let mut parts = segments(data);
    let sof = parts
        .iter_mut()
        .find(|s| matches!(s.0, 0xc0 | 0xc2))
        .unwrap();
    let mut payload = sof.1[4..].to_vec();
    change(&mut payload);
    let mut bytes = sof.1[..2].to_vec();
    bytes.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    bytes.extend_from_slice(&payload);
    sof.1 = bytes;
    join(&parts)
}

#[test]
fn bad_frame_headers_fail_cleanly() {
    for data in [small().baseline(), small().progressive()] {
        // 12-bit samples.
        let twelve = with_frame(&data, |f| f[0] = 12);
        assert_eq!(error_of(&twelve), ImageError::Unsupported("12-bit JPEG"));
        // No width, no height.
        for at in [1, 3] {
            let zero = with_frame(&data, |f| {
                f[at] = 0;
                f[at + 1] = 0;
            });
            assert!(matches!(error_of(&zero), ImageError::Unsupported(_)));
            assert_eq!(dimensions(&zero), None);
        }
        // Two or four components (CMYK).
        for count in [0, 2, 4] {
            let other = with_frame(&data, |f| f[5] = count);
            assert!(matches!(
                error_of(&other),
                ImageError::Unsupported(_) | ImageError::Invalid(_)
            ));
        }
        // Sampling of 0 or 5.
        for hv in [0x01, 0x10, 0x51, 0x15] {
            let bad = with_frame(&data, |f| f[7] = hv);
            assert_eq!(error_of(&bad), ImageError::Invalid("bad sampling"));
        }
        // Quantization table 3, never given: zeros, decodes.
        let unset = with_frame(&data, |f| f[8] = 3);
        assert!(jpeg_result(&unset, Fit::side(50), false).0.is_ok());
        // The frame header cut short: what follows is read as its rest.
        let mut parts = segments(&data);
        parts[3].1.truncate(6);
        parts[3].1[3] = 4;
        let _ = jpeg_result(&join(&parts), Fit::side(50), true);
    }
}

#[test]
fn lossless_and_arithmetic_coding_are_unsupported() {
    for marker in [0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf] {
        let mut parts = segments(&small().baseline());
        parts[3].1[1] = marker;
        assert_eq!(
            error_of(&join(&parts)),
            ImageError::Unsupported("lossless or arithmetic-coded JPEG"),
            "{marker:#x}"
        );
    }
}

#[test]
fn segment_lengths_below_two_are_invalid() {
    for len in [0u8, 1] {
        let mut parts = segments(&small().baseline());
        parts[1].1[2] = 0;
        parts[1].1[3] = len;
        assert_eq!(
            error_of(&join(&parts)),
            ImageError::Invalid("bad segment length")
        );
    }
}

#[test]
fn bad_quantization_and_huffman_tables_fail_cleanly() {
    let data = small().baseline();
    // A quantization table numbered 4 or more.
    let mut parts = segments(&data);
    parts[2].1[4] = 0x07;
    assert_eq!(error_of(&join(&parts)), ImageError::Invalid("bad table"));
    // 16-bit quantization values (a longer segment): read, decodes.
    let mut parts = segments(&data);
    let mut dqt = vec![0xff, 0xdb, 0, 2 + 1 + 128, 0x10];
    dqt.extend((0..64).flat_map(|k: u16| (if k == 0 { 8 } else { 1 + k % 3 }).to_be_bytes()));
    parts[2].1 = dqt;
    let a = decoded(&join(&parts), Fit::side(50), false);
    let b = decoded(&data, Fit::side(50), false);
    assert_eq!(a.pixels, b.pixels);
    // Huffman counts adding up to more than 256 values.
    let mut parts = segments(&data);
    parts[4].1[5..21].fill(255);
    assert!(jpeg_result(&join(&parts), Fit::side(50), false).0.is_err());
    // A table whose length is longer than its values: an error.
    let mut parts = segments(&data);
    parts[4].1[8] = 13;
    assert!(jpeg_result(&join(&parts), Fit::side(50), false).0.is_err());
}

#[test]
fn huffman_tables_that_do_not_fit_their_data_fail_cleanly() {
    let data = small().baseline();
    let dht_at = segments(&data)[..4]
        .iter()
        .map(|s| s.1.len())
        .sum::<usize>();
    // Each count of the DC table changed: oversubscribed, empty or short
    // tables; the data no longer matches them.
    for i in 0..16 {
        for count in [0u8, 1, 2, 3, 12, 40] {
            let mut damaged = data.clone();
            damaged[dht_at + 5 + i] = count;
            let _ = jpeg_result(&damaged, Fit::side(50), false);
        }
    }
    // Every value of the tables changed to every byte: any symbol the
    // decoder may meet (runs past the block, categories above 16).
    for at in dht_at + 21..dht_at + 21 + 12 {
        for value in [0x0f, 0x10, 0x1f, 0x3f, 0xf0, 0xf5, 0xff] {
            let mut damaged = data.clone();
            damaged[at] = value;
            let _ = jpeg_result(&damaged, Fit::side(50), true);
        }
    }
}

#[test]
fn scans_that_do_not_match_the_frame_fail_cleanly() {
    let data = small().baseline();
    let mut parts = segments(&data);
    // An unknown component.
    parts[5].1[5] = 9;
    assert_eq!(
        error_of(&join(&parts)),
        ImageError::Invalid("unknown component")
    );
    // Fewer components than the frame (non-interleaved baseline).
    let mut parts = segments(&data);
    parts[5].1[4] = 1;
    assert!(matches!(
        error_of(&join(&parts)),
        ImageError::Unsupported(_)
    ));
    // Huffman table numbers that were never given.
    let mut parts = segments(&data);
    parts[5].1[6] = 0x23;
    assert_eq!(
        error_of(&join(&parts)),
        ImageError::Invalid("missing Huffman table")
    );
}

#[test]
fn restart_markers_missing_or_wrong_fail_cleanly() {
    let mut jpeg = small();
    jpeg.restart = 1;
    let data = jpeg.baseline();
    let first = find_marker(&data, 0xd0, 0).unwrap();
    // A restart marker numbered out of turn: taken as it is.
    let mut renumbered = data.clone();
    renumbered[first + 1] = 0xd5;
    assert_eq!(
        decoded(&renumbered, Fit::side(50), false).pixels,
        decoded(&data, Fit::side(50), false).pixels
    );
    // Another marker where a restart belongs.
    let mut wrong = data.clone();
    wrong[first + 1] = 0xc4;
    assert_eq!(
        error_of(&wrong),
        ImageError::Invalid("missing restart marker")
    );
    // The markers gone: the data runs on, and fails somewhere.
    let mut gone = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0xff && (0xd0..=0xd7).contains(&data[i + 1]) {
            i += 2;
            continue;
        }
        gone.push(data[i]);
        i += 1;
    }
    assert!(jpeg_result(&gone, Fit::side(50), false).0.is_err());
}

// ---------------------------------------------------------------------
// Progressive scans

/// The progressive file's scan headers: where each starts.
fn scans(data: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = find_marker(data, 0xda, at) {
        out.push(i);
        at = i + 2;
    }
    out
}

#[test]
fn bad_spectral_ranges_and_approximations_are_invalid() {
    let data = small().progressive();
    for scan in 0..scans(&data).len() {
        let at = scans(&data)[scan];
        let count = usize::from(data[at + 4]);
        let ss = at + 5 + 2 * count;
        for (start, end, ah_al) in [
            (6u8, 5u8, 0x00u8), // start after end
            (1, 64, 0x00),      // past the block
            (0, 5, 0x00),       // the DC with AC
            (1, 5, 0x0e),       // a shift of 14
        ] {
            let mut bad = data.clone();
            bad[ss] = start;
            bad[ss + 1] = end;
            bad[ss + 2] = ah_al;
            assert_eq!(
                error_of(&bad),
                ImageError::Invalid("bad scan"),
                "scan {scan}: {start} {end} {ah_al:#x}"
            );
        }
        for count in [0u8, 5] {
            let mut bad = data.clone();
            bad[at + 4] = count;
            assert_eq!(error_of(&bad), ImageError::Invalid("bad scan"));
        }
    }
}

#[test]
fn odd_but_valid_looking_scans_never_panic() {
    // Successive approximation in every combination, spectral bands
    // inside the block, a scan for every component and none.
    let data = Jpeg::new(33, 21, SAMPLINGS[2], 4, true).progressive();
    for scan in scans(&data) {
        let count = usize::from(data[scan + 4]);
        let ss = scan + 5 + 2 * count;
        for (start, end) in [(0, 0), (1, 1), (1, 63), (5, 63), (63, 63), (2, 9)] {
            for ah_al in [0x00, 0x01, 0x10, 0x21, 0x32, 0xdd, 0xf0] {
                for color in [false, true] {
                    let mut odd = data.clone();
                    odd[ss] = start;
                    odd[ss + 1] = end;
                    odd[ss + 2] = ah_al;
                    let _ = jpeg_result(&odd, Fit::side(40), color);
                }
            }
        }
        // Another component's tables, another component.
        for id in [1u8, 2, 3, 7] {
            let mut odd = data.clone();
            odd[scan + 5] = id;
            let _ = jpeg_result(&odd, Fit::side(40), true);
        }
    }
}

#[test]
fn the_smallest_sizes_skip_the_ac_scans() {
    // An eighth: the DC alone; damage in the AC scans goes unnoticed.
    let data = Jpeg::new(64, 64, SAMPLINGS[0], 8, true).progressive();
    let scans = scans(&data);
    let mut damaged = data.clone();
    for byte in &mut damaged[scans[1] + 12..scans[2] - 2] {
        *byte = 0x5a;
    }
    let a = decoded(&data, Fit::side(8), false);
    let b = decoded(&damaged, Fit::side(8), false);
    assert_eq!((a.width, a.height), (8, 8));
    assert_eq!(a.pixels, b.pixels);
}

#[test]
fn progressive_without_frame_or_tables_fails_cleanly() {
    let data = small().progressive();
    let parts = segments(&data);
    // Tables after the first scan come too late for it.
    let dht = parts.iter().position(|s| s.0 == 0xc4).unwrap();
    let mut late = parts.clone();
    let table = late.remove(dht);
    let first_scan = late.iter().position(|s| s.0 == 0xda).unwrap();
    late.insert(first_scan + 1, table);
    assert_eq!(
        error_of(&join(&late)),
        ImageError::Invalid("missing Huffman table")
    );
}

#[test]
fn huge_progressive_frames_are_too_large_for_good() {
    // 65535 × 65535: even the DC alone of its blocks takes more than the
    // decoder ever keeps, so it is too large, not short of memory now.
    let data = with_frame(&small().progressive(), |f| {
        f[1..5].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    });
    assert_eq!(dimensions(&data), Some((65535, 65535)));
    for color in [false, true] {
        let (result, sink) = jpeg_result(&data, Fit::screen(480, 800), color);
        assert_eq!(result, Err(ImageError::TooLarge));
        assert!(!sink.started);
    }
}

#[test]
fn huge_baseline_frames_fail_at_the_end_of_the_data() {
    // 65535 × 65535 announced, a few blocks given: the size decoded to
    // fits the screen, then the data ends (no end marker, after which
    // zeros would be read).
    let data = small().baseline();
    let data = with_frame(&data[..data.len() - 2], |f| {
        f[1..5].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    });
    for color in [false, true] {
        let (result, sink) = jpeg_result(&data, Fit::screen(480, 800), color);
        assert!(result.is_err());
        assert_eq!((sink.width, sink.height), (480, 480));
    }
}
