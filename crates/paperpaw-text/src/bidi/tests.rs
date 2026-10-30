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

#[test]
fn a_paragraph_resolves_at_once() {
    // Hebrew in a left-to-right paragraph, a number in a right-to-left one.
    let mut levels = [Level::LTR; 16];
    let text = "ab \u{5d0}\u{5d1}";
    assert_eq!(resolve(text, Level::LTR, &mut levels), 5);
    let numbers: Vec<u8> = levels[..5].iter().map(|l| l.number()).collect();
    assert_eq!(numbers, [0, 0, 0, 1, 1]);
    let text = "\u{5d0} 12";
    assert_eq!(resolve(text, Level::RTL, &mut levels), 4);
    let numbers: Vec<u8> = levels[..4].iter().map(|l| l.number()).collect();
    assert_eq!(numbers, [1, 1, 2, 2]);
    // Fewer levels than characters: as many as fit.
    assert_eq!(resolve(text, Level::RTL, &mut levels[..2]), 2);
}

/// Text of every kind the walk keeps state for: letters both ways,
/// numbers and their separators, brackets, embeddings and isolates.
const MIXED: &str = "abc \u{5d0}\u{5d1} (12.5%) \u{627}[\u{2067}x\u{202b}y\u{202c}\u{2069}] \u{661}\u{662}, $3 \u{2068}\u{5d2}\u{2069}!";

#[test]
fn a_state_as_bytes_is_the_state() {
    assert_eq!(state_len(), walk::STATE_LEN);
    assert!(state_len() <= 170, "{}", state_len());
    let chars: Vec<char> = MIXED.chars().collect();
    for cut in 0..=chars.len() {
        let mut carry = Carry::new(Level::RTL);
        carry.skip(chars.iter().copied(), cut);
        let mut bytes = vec![0u8; state_len()];
        assert!(carry.encode(&mut bytes));
        assert_eq!(Carry::decode(&bytes), Some(carry), "cut at {cut}");
    }
    assert!(!Carry::new(Level::LTR).encode(&mut [0u8; 10]));
}

#[test]
fn a_paragraph_resolved_piece_by_piece_is_resolved_as_one() {
    let text = MIXED.repeat(3);
    let chars: Vec<char> = text.chars().collect();
    for paragraph in [Level::LTR, Level::RTL] {
        let mut whole = vec![Level::LTR; chars.len()];
        resolve(&text, paragraph, &mut whole);
        for cut in [1, 17, chars.len() / 2, chars.len() - 1] {
            let mut state = vec![0u8; state_len()];
            assert!(start_state(paragraph, &mut state));
            let mut pieces = vec![Level::LTR; chars.len()];
            let (first, second) = pieces.split_at_mut(cut);
            resolve_from(&mut state, paragraph, chars.iter().copied(), cut, first);
            // The state kept, the second piece on its own.
            let kept = state.clone();
            let mut state = kept;
            let n = second.len();
            resolve_from(
                &mut state,
                paragraph,
                chars[cut..].iter().copied(),
                n,
                second,
            );
            assert_eq!(pieces, whole, "cut at {cut}");
            // Skipping the first piece instead gives the same state.
            let mut skipped = vec![0u8; state_len()];
            start_state(paragraph, &mut skipped);
            skip_from(&mut skipped, paragraph, chars.iter().copied(), cut);
            let mut again = vec![0u8; state_len()];
            start_state(paragraph, &mut again);
            resolve_from(
                &mut again,
                paragraph,
                chars.iter().copied(),
                cut,
                &mut vec![Level::LTR; cut],
            );
            assert_eq!(skipped, again);
        }
    }
}

#[test]
fn bytes_that_are_no_state_start_the_paragraph_anew_and_never_panic() {
    let chars: Vec<char> = MIXED.chars().collect();
    let mut fresh = vec![Level::LTR; chars.len()];
    resolve(MIXED, Level::RTL, &mut fresh);
    let mut bytes = vec![0u8; state_len()];
    let mut out = vec![Level::LTR; chars.len()];
    resolve_from(
        &mut bytes,
        Level::RTL,
        chars.iter().copied(),
        chars.len(),
        &mut out,
    );
    assert_eq!(out, fresh, "all zero is no state");
    // Random bytes, and real states with one byte changed: levels for
    // every character, whatever they come to.
    let mut seed = 0x2545_f491_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut real = vec![0u8; state_len()];
    start_state(Level::RTL, &mut real);
    skip_from(&mut real, Level::RTL, chars.iter().copied(), 20);
    for round in 0..4000 {
        let mut state = match round % 2 {
            0 => (0..state_len()).map(|_| next() as u8).collect::<Vec<u8>>(),
            _ => {
                let mut s = real.clone();
                let at = next() as usize % s.len();
                s[at] = next() as u8;
                s
            }
        };
        if round % 4 == 1 {
            state[0] = 1;
        }
        let n = resolve_from(
            &mut state,
            Level::RTL,
            chars.iter().copied(),
            chars.len(),
            &mut out,
        );
        assert_eq!(n, chars.len());
        assert!(out.iter().all(|l| l.number() <= Level::MAX + 1));
    }
    // Too short: taken as the start, nothing written.
    let mut short = [1u8; 4];
    resolve_from(
        &mut short,
        Level::LTR,
        chars.iter().copied(),
        chars.len(),
        &mut out,
    );
    assert_eq!(short, [1u8; 4]);
}
