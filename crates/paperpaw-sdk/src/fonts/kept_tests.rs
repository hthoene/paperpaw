//! [`Fonts::try_keep_open`]: the fonts widgets are set in (#262).
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]

use super::*;
use crate::{FontFace, MemoryStorage};
use alloc::rc::Rc;
use alloc::vec::Vec;
use paperpaw_platform::Storage;
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

fn pack() -> Vec<u8> {
    let mut pack = PackBuilder::new();
    pack.add_strike(
        10.0,
        LineMetrics::new(8.0, 2.0, 12.0),
        alloc::vec![('a', Glyph::empty(6.0))],
    );
    pack.build()
}

fn card_fonts() -> (Fonts, Rc<dyn Storage>) {
    let mut fonts = Fonts::new(4096);
    for (id, path, family, bold) in [
        ("a.slab", "plugins/a/slab.ppfp", "Slab", false),
        ("a.slab-bold", "plugins/a/slab-bold.ppfp", "Slab", true),
        ("a.hand", "plugins/a/hand.ppfp", "Hand", false),
    ] {
        fonts
            .add_face(FontFace::new(Id::new_static(id), path, family, bold, false))
            .unwrap();
    }
    let card = MemoryStorage::new()
        .with("plugins/a/slab.ppfp", pack())
        .with("plugins/a/slab-bold.ppfp", pack())
        .with("plugins/a/hand.ppfp", pack());
    (fonts, Rc::new(card))
}

#[test]
fn a_font_kept_is_open_until_it_is_kept_no_more() {
    let (mut fonts, card) = card_fonts();
    let slab = fonts.family_regular("Slab").unwrap().clone();
    let hand = fonts.family_regular("Hand").unwrap().clone();
    assert_eq!(slab.as_str(), "a.slab");
    assert!(fonts.family_regular("None").is_none());
    // No role names them: nothing is open.
    assert!(fonts.load(&card).is_empty());
    assert!(fonts.get("a.slab").is_none());

    let twice = [slab.clone(), slab.clone()];
    assert!(fonts.try_keep_open(&twice, &card).unwrap().is_empty());
    assert!(fonts.get("a.slab").is_some());
    // Only the regular face a widget is drawn in, and no role is bound.
    assert!(fonts.get("a.slab-bold").is_none());
    assert!(fonts.get("a.hand").is_none());
    assert_eq!(fonts.user.len(), 1);
    // Another load (a setting changed, a plugin installed) keeps it.
    assert!(fonts.load(&card).is_empty());
    assert!(fonts.get("a.slab").is_some());

    assert!(fonts.try_keep_open(&[hand], &card).unwrap().is_empty());
    assert!(fonts.get("a.slab").is_none());
    assert!(fonts.get("a.hand").is_some());
    assert!(fonts.try_keep_open(&[], &card).unwrap().is_empty());
    assert!(fonts.get("a.hand").is_none());
    assert!(fonts.user.is_empty());
}

#[test]
fn a_role_the_reader_bound_stays_when_the_kept_fonts_change() {
    let (mut fonts, card) = card_fonts();
    let slab = Id::new_static("a.slab");
    let bold = Id::new_static("a.slab-bold");
    fonts.bind_user(crate::builtin::fonts::READING_BOLD, bold.clone());
    fonts.try_keep_open(&[slab], &card).unwrap();
    fonts.try_keep_open(&[], &card).unwrap();
    assert!(fonts.user_bound(crate::builtin::fonts::READING_BOLD.as_str()));
    assert!(fonts.get(bold.as_str()).is_some());
    assert!(fonts.get("a.slab").is_none());
}

#[test]
fn a_file_that_will_not_open_is_told_and_the_others_are_kept() {
    let (mut fonts, _) = card_fonts();
    let card: Rc<dyn Storage> = Rc::new(MemoryStorage::new().with("plugins/a/hand.ppfp", pack()));
    let both = [Id::new_static("a.slab"), Id::new_static("a.hand")];
    let failed = fonts.try_keep_open(&both, &card).unwrap();
    assert_eq!(failed, ["plugins/a/slab.ppfp"]);
    assert!(fonts.get("a.hand").is_some());
    assert!(fonts.get("a.slab").is_none());
}
