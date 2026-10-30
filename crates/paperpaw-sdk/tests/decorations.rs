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
    self, AS_IS, Decorated, Decoration, DecoratorSpec, Decorators, LineKind, NONE, PageText,
    PageWord, THICKNESS, Tone,
};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{Contribution, Id, Rect, Registrar};
use proptest::prelude::*;

use support::{Device, Harness, drawn_in, drawn_outside, frame};

const ROLE: Id = Id::new_static("t.learning");

/// A device with role `t.learning` added, its settings set to `values`
/// (aspect, value as text) where they take them.
fn with_role(default: Decoration) -> Harness {
    let mut h = Harness::new(Device::TOUCH);
    let mut r = Registrar::new(Id::new_static("t"));
    decorations::add_role(&mut r, &ROLE, "t.role", default).unwrap();
    for c in r.into_contributions() {
        if let Contribution::Setting(spec) = c {
            h.settings.define(*spec).unwrap();
        }
    }
    h
}

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

    /// A role added with any default reads back as that default (the
    /// thickness as the settings keep it: 0 is the standard one).
    #[test]
    fn a_role_reads_back_its_default(default in decoration_strategy()) {
        let default = default.thickness(default.thickness % 21);
        let h = with_role(default);
        let read = Decoration::of_role(&h.env(), &ROLE);
        let thickness = if default.thickness == 0 { THICKNESS } else { default.thickness };
        prop_assert_eq!(read, default.thickness(thickness));
    }

    /// Whatever text a role's settings hold (as a theme or a hand-edited
    /// file may give them), the look is read without a panic and
    /// unknown values mark nothing.
    #[test]
    fn any_text_in_a_roles_settings_is_read_without_harm(
        background in ".{0,30}",
        line in ".{0,30}",
        thickness in any::<i64>(),
    ) {
        let mut h = Harness::new(Device::TOUCH);
        h.define_text("t.learning.background", &background);
        h.define_text("t.learning.line", &line);
        h.define_int("t.learning.thickness", thickness, i64::MIN, i64::MAX);
        let read = Decoration::of_role(&h.env(), &ROLE);
        prop_assert_eq!(read.background, Tone::from_id(&background));
        prop_assert_eq!(read.line, LineKind::from_id(&line));
        prop_assert_eq!(i64::from(read.thickness), thickness.clamp(0, 255));
    }
}

#[test]
fn a_role_nobody_added_marks_nothing() {
    let h = Harness::new(Device::TOUCH);
    let read = Decoration::of_role(&h.env(), &Id::new_static("t.nobody"));
    assert!(read.is_none());
    assert_eq!(read, Decoration::NONE);
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

#[test]
fn a_role_set_by_hand_reads_what_is_set() {
    let mut h = with_role(Decoration::NONE);
    let set = |h: &mut Harness, aspect: &str, value: &str| {
        h.settings
            .set_from_text(&format!("t.learning.{aspect}"), value)
            .unwrap();
    };
    set(&mut h, "background", Tone::Yellow.id());
    set(&mut h, "line", LineKind::Wavy.id());
    set(&mut h, "line-tone", Tone::Red.id());
    set(&mut h, "thickness", "7");
    set(&mut h, "struck", "true");
    let read = Decoration::of_role(&h.env(), &ROLE);
    assert_eq!(
        read,
        Decoration::NONE
            .background(Tone::Yellow)
            .line(LineKind::Wavy)
            .line_tone(Tone::Red)
            .thickness(7)
            .struck()
    );
    // Out of the setting's range: refused, the look as it was.
    assert!(
        h.settings
            .set_from_text("t.learning.thickness", "300")
            .is_err()
    );
    assert_eq!(Decoration::of_role(&h.env(), &ROLE).thickness, 7);
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

/// Boxes of words on lines 40 high, 4 apart, as a page has them.
fn boxes_strategy() -> impl Strategy<Value = Vec<Rect>> {
    // Off the left edge (see the test of a background there).
    proptest::collection::vec((0u16..5, 10u16..380, 1u16..200), 1..6).prop_map(|words| {
        words
            .into_iter()
            .map(|(line, x, width)| Rect::new(x, 20 + line * 44, width, 40))
            .collect()
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// Lines, frames and strikes of the usual thicknesses (up to 0.8 mm)
    /// stay on the lines of their words: nothing below a box (on the line
    /// under it), nothing above it, nothing further to the side than the
    /// frame's padding.
    #[test]
    fn marks_are_drawn_on_the_lines_of_their_words(
        boxes in boxes_strategy(),
        decoration in decoration_strategy(),
    ) {
        let decoration = decoration.thickness(decoration.thickness % 9);
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let pad = env.metrics().tenth_mm(5).max(1) as u16;
        for b in &boxes {
            let mut frame = frame(640, 260);
            decorations::draw_behind(&mut frame.canvas(), &env, &[*b], &decoration);
            decorations::draw_over(&mut frame.canvas(), &env, &[*b], &decoration);
            let x = b.x.saturating_sub(pad);
            let bound = Rect::new(x, b.y, (b.right() as u16 + pad) - x, b.height);
            prop_assert_eq!(drawn_outside(&mut frame, bound), None, "{:?} {:?}", b, decoration);
        }
    }

    /// Of any thickness settings offer (up to 2 mm), marks never reach
    /// the line below their words, nor further to the side than the
    /// padding. (Thick double and wavy lines rise above short words: the
    /// user chose them so.)
    #[test]
    fn marks_never_reach_the_line_below(
        boxes in boxes_strategy(),
        decoration in decoration_strategy(),
    ) {
        let decoration = decoration.thickness(decoration.thickness % 21);
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let pad = env.metrics().tenth_mm(5).max(1) as u16;
        for b in &boxes {
            let mut frame = frame(640, 260);
            decorations::draw_behind(&mut frame.canvas(), &env, &[*b], &decoration);
            decorations::draw_over(&mut frame.canvas(), &env, &[*b], &decoration);
            let x = b.x.saturating_sub(pad);
            let bound = Rect::new(x, 0, (b.right() as u16 + pad) - x, b.bottom() as u16);
            prop_assert_eq!(drawn_outside(&mut frame, bound), None, "{:?} {:?}", b, decoration);
        }
    }

    /// Whatever it marks, a decoration draws: a background behind the
    /// words, a line at their foot.
    #[test]
    fn what_a_decoration_marks_shows(
        b in (0u16..300, 20u16..200).prop_map(|(x, w)| Rect::new(x, 30, w, 40)),
        tone in proptest::sample::select(Tone::ALL.to_vec()),
        kind in proptest::sample::select(LineKind::ALL.to_vec()),
    ) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let mut frame = frame(640, 120);
        let marked = Decoration::NONE.background(tone);
        decorations::draw_behind(&mut frame.canvas(), &env, &[b], &marked);
        prop_assert!(drawn_in(&frame, b), "a background in {tone:?}");
        let mut frame = support::frame(640, 120);
        let marked = Decoration::NONE.line(kind);
        decorations::draw_over(&mut frame.canvas(), &env, &[b], &marked);
        let foot = Rect::new(b.x, b.y + b.height / 2, b.width, b.height / 2);
        prop_assert!(drawn_in(&frame, foot), "a {kind:?} line");
    }
}

/// A word at the very left of the page: the background's padding is cut
/// there, and it is to keep its padding on the right as anywhere else.
#[test]
fn a_background_at_the_left_edge_keeps_its_padding_on_the_right() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let pad = env.metrics().tenth_mm(5).max(1) as u16;
    let word = Rect::new(0, 20, 50, 30);
    let mut frame = frame(200, 80);
    let marked = Decoration::NONE.background(Tone::Black);
    decorations::draw_behind(&mut frame.canvas(), &env, &[word], &marked);
    let bound = Rect::new(0, 20, 50 + pad, 30);
    assert_eq!(drawn_outside(&mut frame, bound), None);
}

#[test]
fn no_mark_and_no_boxes_draw_nothing() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let mut frame = frame(200, 80);
    let word = [Rect::new(10, 10, 50, 30)];
    decorations::draw_behind(&mut frame.canvas(), &env, &word, &Decoration::NONE);
    decorations::draw_over(&mut frame.canvas(), &env, &word, &Decoration::NONE);
    // Ink alone is the text's colour: nothing drawn around it.
    let ink = Decoration::NONE.ink(Tone::Red);
    decorations::draw_behind(&mut frame.canvas(), &env, &word, &ink);
    decorations::draw_over(&mut frame.canvas(), &env, &word, &ink);
    let full = Decoration::NONE.background(Tone::Gray).frame().struck();
    decorations::draw_behind(&mut frame.canvas(), &env, &[], &full);
    decorations::draw_over(&mut frame.canvas(), &env, &[], &full);
    assert_eq!(frame.take_dirty(), None);
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
fn marking(id: &'static str, role: &'static str) -> DecoratorSpec {
    DecoratorSpec::new(Id::new_static(id), id, move |_, page: &PageText<'_>| {
        let mut out: Vec<Decorated> = page
            .words
            .iter()
            .filter_map(|w| Decorated::words(&[*w], &Id::new_static(role)))
            .collect();
        // Empty and backwards ranges are left out.
        out.push(Decorated::new(at(5), at(5), Id::new_static(role)));
        out.push(Decorated::new(at(9), at(2), Id::new_static(role)));
        out
    })
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
    list.add(marking("t.a", "t.a").order(1).signature(|_| 7));
    list.add(marking("t.b", "t.b").order(2).switch(&off).signature(|_| 9));
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

#[test]
fn decorators_mark_in_their_order_and_only_while_switched_on() {
    let mut h = Harness::new(Device::TOUCH);
    h.define_bool("t.second-on", true);
    let switch = paperpaw_sdk::Setting::<bool>::new("t.second-on");
    let mut list = Decorators::new();
    list.add(marking("t.late", "t.late").order(5));
    list.add(marking("t.second", "t.second").switch(&switch).taps());
    list.add(marking("t.first", "t.first").order(-3));
    let words = [
        PageWord::new("one", at(0), at(3)),
        PageWord::new("two", at(4), at(7)),
    ];
    let page = PageText::new("books/a.epub", "en", &words);
    let marks = list.decorate(&h.env(), &page);
    let order: Vec<&str> = marks.iter().map(|m| m.decorator.as_str()).collect();
    assert_eq!(
        order,
        [
            "t.first", "t.first", "t.second", "t.second", "t.late", "t.late"
        ]
    );
    assert!(marks.iter().all(|m| m.start < m.end), "no empty ranges");
    assert!(
        marks
            .iter()
            .all(|m| m.taps == (m.decorator.as_str() == "t.second"))
    );
    assert!(marks[0].overlaps(at(2), at(10)));
    assert!(!marks[0].overlaps(at(3), at(10)), "the end is not in it");
    let on = list.signature(&h.env());
    // Switched off: left out, and the signature says so.
    h.settings
        .set("t.second-on", paperpaw_sdk::SettingValue::Bool(false))
        .unwrap();
    let marks = list.decorate(&h.env(), &page);
    assert!(marks.iter().all(|m| m.decorator.as_str() != "t.second"));
    assert_ne!(list.signature(&h.env()), on);
    assert!(list.get(&Id::new_static("t.late")).is_some());
    assert!(Decorators::new().decorate(&h.env(), &page).is_empty());
    assert!(Decorated::words(&[], &ROLE).is_none());
}
