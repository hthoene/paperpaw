//! Decorations of a book's words (ADR 0021): a role's look read from its
//! settings, whatever they hold; marks combined, the earlier winning;
//! lines, frames and backgrounds drawn round the words' boxes and not
//! onto the lines next to them; decorators asked in their order, those
//! switched off left out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::decorations::{
    self, AS_IS, Decorated, Decoration, DecoratorSpec, Decorators, LineKind, NONE, PageText, Tone,
};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{Id, Rect};
use proptest::prelude::*;

use support::{Device, Harness};

fn tone_strategy() -> impl Strategy<Value = Option<Tone>> {
    proptest::option::of(proptest::sample::select(Tone::ALL.to_vec()))
}

fn decoration_strategy() -> impl Strategy<Value = Decoration> {
    (
        tone_strategy(),
        tone_strategy(),
        proptest::option::of(proptest::sample::select(LineKind::ALL.to_vec())),
        tone_strategy(),
        any::<u8>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(background, ink, line, line_tone, thickness, frame, struck)| {
                let mut d = Decoration::NONE.thickness(thickness);
                if let Some(t) = background {
                    d = d.background(t);
                }
                if let Some(t) = ink {
                    d = d.ink(t);
                }
                if let Some(l) = line {
                    d = d.line(l);
                }
                if let Some(t) = line_tone {
                    d = d.line_tone(t);
                }
                if frame {
                    d = d.frame();
                }
                if struck {
                    d = d.struck();
                }
                d
            },
        )
}

// ---- A role's look from its settings -----------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]


}

#[test]
fn the_none_values_are_no_tones_or_lines() {
    for value in [
        NONE,
        AS_IS,
        "",
        "core.tone.",
        "core.tone.yellow ",
        "CORE.TONE.YELLOW",
    ] {
        assert_eq!(Tone::from_id(value), None, "{value:?}");
        assert_eq!(LineKind::from_id(value), None, "{value:?}");
    }
}

// ---- Combining ---------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// No mark adds nothing, on either side; combining is the same
    /// whichever pairs are combined first; each aspect is the earliest
    /// that sets it.
    #[test]
    fn marks_combine_the_earliest_winning(
        a in decoration_strategy(),
        b in decoration_strategy(),
        c in decoration_strategy(),
    ) {
        prop_assert_eq!(a.combined(Decoration::NONE), a);
        prop_assert_eq!(Decoration::NONE.combined(a).background, a.background);
        prop_assert_eq!(a.combined(b).combined(c), a.combined(b.combined(c)));
        let all = decorations::combine([a, b, c]);
        prop_assert_eq!(all.background, a.background.or(b.background).or(c.background));
        prop_assert_eq!(all.ink, a.ink.or(b.ink).or(c.ink));
        prop_assert_eq!(all.line, a.line.or(b.line).or(c.line));
        prop_assert_eq!(all.frame, a.frame || b.frame || c.frame);
        prop_assert_eq!(all.struck, a.struck || b.struck || c.struck);
        prop_assert_eq!(all.is_none(), a.is_none() && b.is_none() && c.is_none());
    }
}

// ---- Drawing -----------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]



}

#[test]
fn words_of_a_right_to_left_line_join_into_one_box() {
    // In reading order, each left of the one before.
    let boxes = decorations::join_boxes([
        (0, Rect::new(60, 0, 10, 5)),
        (1, Rect::new(40, 0, 10, 5)),
        (2, Rect::new(20, 0, 10, 5)),
    ]);
    assert_eq!(boxes, [Rect::new(20, 0, 50, 5)]);
}

#[test]
fn words_join_into_a_box_per_line_only_when_next_to_each_other() {
    // Unordered or repeated indexes and an empty list: never a panic.
    assert!(decorations::join_boxes([]).is_empty());
    let boxes = decorations::join_boxes([
        (usize::MAX, Rect::new(0, 0, 10, 5)),
        (0, Rect::new(12, 0, 10, 5)),
        (0, Rect::new(30, 0, 10, 5)),
        (1, Rect::new(0, 9, 10, 5)),
        (2, Rect::new(14, 9, 10, 5)),
    ]);
    assert_eq!(
        boxes,
        [
            // index 0 follows usize::MAX (wrapping): joined on its line.
            Rect::new(0, 0, 22, 5),
            Rect::new(30, 0, 10, 5),
            Rect::new(0, 9, 24, 5),
        ]
    );
}

// ---- Decorators --------------------------------------------------------

fn at(n: u32) -> Locator {
    Locator::new(0, n)
}

/// A decorator marking every word with `role`, and one empty range.
fn marking(id: &'static str) -> DecoratorSpec {
    let decorate: paperpaw_sdk::decorations::DecorateFn = match id {
        "t.a" => |_, page| every_word(page, "t.a"),
        "t.b" => |_, page| every_word(page, "t.b"),
        "t.late" => |_, page| every_word(page, "t.late"),
        "t.second" => |_, page| every_word(page, "t.second"),
        _ => |_, page| every_word(page, "t.first"),
    };
    DecoratorSpec::new(Id::new_static(id), id, decorate)
}

/// Every word of `page` marked with `role`, and one empty range and one
/// backwards.
fn every_word(page: &PageText<'_>, role: &'static str) -> Vec<Decorated> {
    let mut out: Vec<Decorated> = page
        .words
        .iter()
        .filter_map(|w| Decorated::words(&[*w], &Id::new_static(role)))
        .collect();
    // Empty and backwards ranges are left out.
    out.push(Decorated::new(at(5), at(5), Id::new_static(role)));
    out.push(Decorated::new(at(9), at(2), Id::new_static(role)));
    out
}

/// The decorators' signature is FNV-1a over each one's id, whether it is
/// on and, if so, its own signature, in their order: worked out piece by
/// piece, it is what `fnv1a64` gives for those bytes in one piece.
#[test]
fn the_signature_is_fnv1a_over_ids_switches_and_signatures() {
    let mut h = Harness::new(Device::TOUCH);
    h.define_bool("t.off", false);
    let off = paperpaw_sdk::Setting::<bool>::new("t.off");
    let mut list = Decorators::new();
    list.add(marking("t.a").order(1).signature(|_| 7));
    list.add(marking("t.b").order(2).switch(&off).signature(|_| 9));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"t.a");
    bytes.push(1);
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.extend_from_slice(b"t.b");
    bytes.push(0);
    assert_eq!(list.signature(&h.env()), paperpaw_digest::fnv1a64(&bytes));
    assert_eq!(
        Decorators::new().signature(&h.env()),
        paperpaw_digest::fnv1a64([])
    );
}
