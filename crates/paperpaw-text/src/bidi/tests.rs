use alloc::vec;
use alloc::vec::Vec;

use super::*;

fn levels(text: &str, paragraph: Level) -> Vec<u8> {
    let mut carry = Carry::new(paragraph);
    let mut out = vec![Level::LTR; text.chars().count()];
    let n = carry.resolve(text.chars(), out.len(), &mut out);
    assert_eq!(n, out.len());
    reset_line_end(text, &mut out, paragraph);
    out.iter().map(|l| l.number()).collect()
}

#[test]
fn numbers_stay_left_to_right_in_arabic() {
    // Arabic word, space, 2026: the number is at level 2.
    assert_eq!(
        levels("\u{627}\u{644} 2026", Level::RTL),
        [1, 1, 1, 2, 2, 2, 2]
    );
}

/// Levels with L1 for lines ending at `breaks`, the carry kept.
fn by_lines(text: &str, paragraph: Level, breaks: &[usize]) -> Vec<u8> {
    let chars: Vec<char> = text.chars().collect();
    let mut carry = Carry::new(paragraph);
    let mut out = Vec::new();
    let mut from = 0;
    for &to in breaks.iter().chain([chars.len()].iter()) {
        let mut line = vec![Level::LTR; to - from];
        carry.resolve(chars[from..].iter().copied(), to - from, &mut line);
        let s: alloc::string::String = chars[from..to].iter().collect();
        reset_line_end(&s, &mut line, paragraph);
        out.extend(line.iter().map(|l| l.number()));
        from = to;
    }
    out
}

/// The paragraph's levels before L1, then L1 per line.
fn whole_then_lines(text: &str, paragraph: Level, breaks: &[usize]) -> Vec<u8> {
    let chars: Vec<char> = text.chars().collect();
    let mut all = vec![Level::LTR; chars.len()];
    Carry::new(paragraph).resolve(text.chars(), chars.len(), &mut all);
    let mut from = 0;
    for &to in breaks.iter().chain([chars.len()].iter()) {
        let s: alloc::string::String = chars[from..to].iter().collect();
        reset_line_end(&s, &mut all[from..to], paragraph);
        from = to;
    }
    all.iter().map(|l| l.number()).collect()
}

fn same_by_lines(text: &str, paragraph: Level) {
    let n = text.chars().count();
    for b in 1..n {
        assert_eq!(
            by_lines(text, paragraph, &[b]),
            whole_then_lines(text, paragraph, &[b]),
            "{text:?} at {b}"
        );
    }
}

// The review's cases (A-H1) where resolving each line alone goes wrong.

#[test]
fn an_isolate_that_wraps_keeps_its_direction_on_the_next_line() {
    // RTL paragraph: Arabic, an LTR isolate "abc def" broken inside.
    let text = "\u{627} \u{2066}abc def\u{2069} \u{628}";
    same_by_lines(text, Level::RTL);
    let levels = by_lines(text, Level::RTL, &[7]);
    assert_eq!(
        levels[7], 2,
        "the 'd' after the break stays inside the isolate"
    );
}

#[test]
fn a_number_after_arabic_on_the_line_before_is_arabic() {
    // W2 across a break: AL ends line 1, "50%" starts line 2.
    let text = "\u{627}\u{644} 50% \u{628}";
    same_by_lines(text, Level::RTL);
}

#[test]
fn a_neutral_between_latin_words_across_a_break_is_ltr() {
    // N1 across a break: "...abc" ends line 1, "! def" starts line 2.
    let text = "\u{627} abc ! def \u{628}";
    same_by_lines(text, Level::RTL);
    assert_eq!(whole_then_lines(text, Level::RTL, &[])[6], 2);
}

#[test]
fn brackets_that_span_lines_pair() {
    let text = "\u{627} (abc def) \u{628}";
    same_by_lines(text, Level::RTL);
}

#[test]
fn a_split_block_continues_with_the_carry() {
    // A paragraph cut into two pieces: the second starts with Latin but
    // keeps the first piece's right-to-left paragraph and state.
    let first = "\u{627}\u{644}\u{639} (abc ";
    let second = "def) 2026 \u{628}";
    let paragraph = paragraph_level(first).unwrap_or(Level::LTR);
    let mut carry = Carry::new(paragraph);
    let n1 = first.chars().count();
    let mut a = vec![Level::LTR; n1];
    carry.resolve(first.chars().chain(second.chars()), n1, &mut a);
    let n2 = second.chars().count();
    let mut b = vec![Level::LTR; n2];
    carry.resolve(second.chars(), n2, &mut b);
    let whole: alloc::string::String = [first, second].concat();
    let mut all = vec![Level::LTR; n1 + n2];
    Carry::new(paragraph).resolve(whole.chars(), n1 + n2, &mut all);
    a.extend(b);
    assert_eq!(a, all);
    assert_eq!(carry.paragraph(), Level::RTL);
}

#[test]
fn brackets_mirror_at_odd_levels() {
    assert_eq!(mirror('('), ')');
    assert_eq!(mirror('\u{AB}'), '\u{BB}');
    assert_eq!(mirror('['), ']');
    assert_eq!(mirror('a'), 'a');
}

#[test]
fn latin_typography_does_not_need_bidi() {
    assert!(!needs_bidi(
        "\u{2019}Quotes\u{201C} \u{2014} dashes\u{2026} \u{FB01}ne \u{A0}"
    ));
    assert!(needs_bidi("PaperPaw \u{627}"));
    assert!(needs_bidi("abc \u{2067}x\u{2069}"));
    assert!(needs_bidi("\u{5D0}"));
}

#[test]
fn persian_digits_are_european_numbers_and_arabic_ones_arabic() {
    assert_eq!(class('\u{6F1}'), Class::EN);
    assert_eq!(class('\u{661}'), Class::AN);
    assert_eq!(class('\u{66B}'), Class::AN);
    assert_eq!(class('\u{66C}'), Class::AN);
    assert_eq!(class('\u{5D0}'), Class::R);
    assert_eq!(class('\u{10FFFF}'), Class::BN);
}

#[test]
fn the_carry_is_small() {
    assert!(
        core::mem::size_of::<Carry>() <= 256,
        "{}",
        core::mem::size_of::<Carry>()
    );
}

proptest::proptest! {
    #[test]
    fn any_text_resolves_alike_by_lines_and_never_panics(
        picks in proptest::collection::vec(0usize..40, 0..60),
        rtl in proptest::bool::ANY,
        cut in 0usize..60,
    ) {
        const POOL: [char; 40] = [
            'a', 'b', ' ', '1', '2', '+', '-', '$', '%', ',', '.', ':', '(', ')', '[', ']', '!', '"',
            '\u{5D0}', '\u{627}', '\u{644}', '\u{660}', '\u{6F1}', '\u{300}', '\u{64E}', '\u{AD}',
            '\u{200D}', '\t', '\u{2029}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
            '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{AB}', '\u{BB}',
        ];
        let text: alloc::string::String = picks.iter().map(|&i| POOL[i]).collect();
        let paragraph = if rtl { Level::RTL } else { Level::LTR };
        let n = text.chars().count();
        let breaks: Vec<usize> = if cut > 0 && cut < n { vec![cut] } else { Vec::new() };
        proptest::prop_assert_eq!(by_lines(&text, paragraph, &breaks), whole_then_lines(&text, paragraph, &breaks));
        let mut levels = vec![Level::LTR; n];
        Carry::new(paragraph).resolve(text.chars(), n, &mut levels);
        let mut order = vec![0u16; n];
        let shown = visual_order(&levels, &mut order);
        let mut sorted = order[..shown].to_vec();
        sorted.sort_unstable();
        proptest::prop_assert!(sorted.iter().enumerate().all(|(i, &o)| usize::from(o) == i));
    }
}

/// The example of the module's documentation.
#[test]
fn the_example() {
    let text = "\u{627}\u{644}\u{639}\u{627}\u{645} 2026 PaperPaw.";
    let paragraph = paragraph_level(text).unwrap_or(Level::LTR);
    let mut carry = Carry::new(paragraph);
    let mut levels = [Level::LTR; 64];
    let count = text.chars().count();
    let resolved = carry.resolve(text.chars(), count, &mut levels);
    reset_line_end(text, &mut levels[..resolved], paragraph);
    let mut order = [0u16; 64];
    let shown = visual_order(&levels[..resolved], &mut order);
    // The Arabic word comes last from the left, after "2026 PaperPaw".
    assert_eq!(order[shown - 1], 0);
}

/// Text made to be slow (thousands of isolates, terminators, brackets)
/// costs time linear in its length: four times the text, about four
/// times the time, not sixteen. Timing, so not run by default:
/// `cargo test --release -p paperpaw-text --lib hostile -- --ignored`.
#[test]
#[ignore = "measures time; run in release"]
fn hostile_text_costs_linear_time() {
    extern crate std;
    let time = |unit: &str, n: usize| {
        let text = unit.repeat(n);
        let starts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let count = starts.len();
        let mut levels = vec![Level::LTR; count];
        let mut carry = Carry::new(Level::RTL);
        let at = std::time::Instant::now();
        let mut done = 0;
        while done < count {
            let k = 60.min(count - done);
            carry.resolve(text[starts[done]..].chars(), k, &mut levels[done..done + k]);
            done += k;
        }
        at.elapsed().as_secs_f64()
    };
    for unit in [
        "\u{2067}",
        "\u{2068}",
        "\u{2066}a",
        "$",
        "\u{627}$",
        "(\u{2067}",
        "\u{2067} ! ",
        "\u{2068}1 \u{2069} !",
        "\u{202A}\u{2067}",
    ] {
        let (small, large) = (time(unit, 4_000), time(unit, 16_000));
        assert!(large < small * 8.0, "{unit:?}: {small} s, then {large} s");
    }
}
