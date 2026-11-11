//! WOFF fonts damaged at random: unpacking never panics and never makes a
//! font larger than allowed.

extern crate std;

use alloc::vec::Vec;

use proptest::prelude::*;

use super::*;

const WOFF: &[u8] = include_bytes!("../../tests/data/atkinson-subset.woff");

/// `cases` cases, or as many as `PROPTEST_CASES` says, for longer runs.
fn cases(cases: u32) -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(cases);
    ProptestConfig::with_cases(cases)
}

proptest! {
    #![proptest_config(cases(256))]

    #[test]
    fn arbitrary_bytes_never_panic(data in prop::collection::vec(any::<u8>(), 0..512), max in 0usize..1 << 16) {
        let mut woff = SIGNATURE.to_vec();
        woff.extend_from_slice(&data);
        if let Some(font) = to_font(&woff, max) {
            prop_assert!(font.len() <= max + 3);
        }
    }

    #[test]
    fn damaged_woffs_never_panic_nor_grow_past_the_limit(
        damage in prop::collection::vec((0usize..400, any::<u8>()), 1..12),
        max in prop::sample::select(&[100usize, 4096, 1 << 20][..]),
    ) {
        // Damage aimed at the header and table directory, where the
        // counts, offsets and lengths are.
        let mut woff: Vec<u8> = WOFF.to_vec();
        for (at, byte) in damage {
            let at = at.min(woff.len() - 1);
            woff[at] = byte;
        }
        if let Some(font) = to_font(&woff, max) {
            prop_assert!(font.len() <= max + 3);
            prop_assert_eq!(&font[..4], &woff[4..8]);
        }
    }
}

#[test]
fn table_counts_and_sizes_at_their_limits_are_refused() {
    let mut woff = WOFF.to_vec();
    // As many tables as the field holds, none of them there.
    woff[12..14].copy_from_slice(&u16::MAX.to_be_bytes());
    assert_eq!(to_font(&woff, 1 << 20), None);
    let mut woff = WOFF.to_vec();
    woff[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(to_font(&woff, 16 << 20), None);
}
