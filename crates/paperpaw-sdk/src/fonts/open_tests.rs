//! [`open_file`]: a font file of the card for a plugin's own use.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]

use super::*;
use crate::MemoryStorage;
use paperpaw_text::{Glyph, LineMetrics, PackBuilder, PackLimits};

fn pack() -> Vec<u8> {
    let mut pack = PackBuilder::new();
    pack.add_strike(
        10.0,
        LineMetrics::new(8.0, 2.0, 12.0),
        alloc::vec![('a', Glyph::empty(6.0))],
    );
    pack.build()
}

fn card() -> Rc<dyn Storage> {
    Rc::new(
        MemoryStorage::new()
            .with("f/a.ppfp", pack())
            .with("f/a.PPFP", pack())
            .with("f/bad.ppfp", alloc::vec![1_u8; 40])
            .with("f/bad.ttf", alloc::vec![1_u8; 40])
            .with("f/empty.ttf", Vec::new()),
    )
}

#[test]
fn a_pack_is_opened_where_it_is_whatever_the_case_of_its_extension() {
    let card = card();
    for path in ["f/a.ppfp", "f/a.PPFP"] {
        let font = open_file(&card, path, PackLimits::SMALL).unwrap();
        assert!(matches!(font, Font::Stored(_)), "{path}");
        assert!((font.advance('a', 10.0) - 6.0).abs() < 0.01);
    }
}

#[test]
fn a_missing_file_a_bad_pack_and_a_bad_font_are_told_apart() {
    let card = card();
    for path in ["f/none.ppfp", "f/none.ttf"] {
        assert_eq!(
            open_file(&card, path, PackLimits::SMALL).unwrap_err(),
            OpenError::NotFound
        );
    }
    for path in ["f/bad.ppfp", "f/bad.ttf", "f/empty.ttf"] {
        assert!(
            matches!(
                open_file(&card, path, PackLimits::SMALL),
                Err(OpenError::Font(_))
            ),
            "{path}"
        );
    }
}

#[cfg(not(feature = "outline"))]
#[test]
fn a_file_that_is_no_pack_is_refused_unread_where_the_build_has_no_outlines() {
    let card = card();
    assert_eq!(
        open_file(&card, "f/bad.ttf", PackLimits::SMALL).unwrap_err(),
        OpenError::Font(FontError::OutlinesNotSupported)
    );
}
