//! Tests of Unicode line breaking in the wrapping functions (design,
//! section 9): text without a flagged character is laid out as before (the
//! oracle is a verbatim copy of the old functions, `legacy`), and text with
//! one breaks as UAX #14 says, in lines as full as they can be, never at a
//! place that moves nothing.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;
use std::prelude::rust_2024::*;
use std::vec;

use super::legacy::{legacy_wrap_balanced_in, legacy_wrap_kerned_in};
use super::{Line, wrap_lines_balanced_in, wrap_lines_kerned_in};

// The bits of `ScriptOptions` (positions fixed by D3).
const KEEP_ALL: u32 = 1 << 2;
const LOOSE: u32 = 1 << 3;
const STRICT: u32 = 2 << 3;
const ANYWHERE: u32 = 3 << 3;
const BREAK_ALL: u32 = 1 << 5;

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Characters of text that has no flagged character (design, section 6):
/// letters of several scripts, digits, and every character the review lists
/// as unchanged.
fn unflagged() -> Vec<char> {
    let mut chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        .chars()
        .collect();
    chars.extend("абвгдежзийклмнопрстуфхцчшщэюя".chars());
    chars.extend("αβγδεζηθικλμνξοπρστυφχψω".chars());
    chars.extend("ابتثجحخدذرزسشصضطظعغفقكلمنهوي\u{064B}\u{064E}".chars());
    chars.extend("אבגדהוזחטיכלמנסעפצקרשת\u{05B0}".chars());
    // Punctuation and the characters that must not change.
    chars.extend(".,;:!?()[]\"'/\\\u{201C}\u{201D}\u{2018}\u{2019}".chars());
    chars.extend(['-', '\u{2010}', '\u{2013}', '\u{2014}']);
    chars.extend([
        '\u{00A0}', '\u{202F}', '\u{00AD}', '\u{2002}', '\u{2009}', '\u{200A}', '\t', '\u{200E}',
        '\u{200F}', '\u{061C}', '\u{2066}', '\u{2069}', '\u{060C}', '\u{061F}', '\u{05BE}',
        '\u{200C}', '\u{200D}',
    ]);
    // Spaces count double so that words are short.
    chars.extend([' ', ' ', ' ', ' ', ' ', ' ']);
    chars
}

fn advance(_: usize, c: char) -> f32 {
    match c {
        ' ' => 4.0,
        c if c.is_ascii_digit() => 7.0,
        c if (c as u32) < 0x300 => 6.0 + (c as u32 % 5) as f32,
        '\u{00AD}' | '\u{200C}' | '\u{200D}' | '\u{200E}' | '\u{200F}' | '\u{061C}' => 0.0,
        _ => 8.0,
    }
}

fn kern(_: usize, left: char, right: char) -> f32 {
    if left == 'A' || right == 'V' {
        -1.0
    } else {
        0.0
    }
}

/// A hyphenation callback: after every fourth letter.
fn hyphenate(word: &str, found: &mut dyn FnMut(usize)) {
    for (n, (at, _)) in word.char_indices().enumerate() {
        if n > 0 && n % 4 == 0 {
            found(at);
        }
    }
}

/// The options a text without flagged characters is unchanged under: every
/// combination of bits 2 to 5 except those that lift the filter on purpose.
fn plain_options() -> Vec<u32> {
    let mut all = vec![
        0,
        KEEP_ALL,
        LOOSE,
        STRICT,
        KEEP_ALL | LOOSE,
        KEEP_ALL | STRICT,
    ];
    all.push(1 << 6 | 1 << 12); // bits the text crate does not know are ignored
    all
}

fn random_text(rng: &mut Rng, alphabet: &[char]) -> String {
    let len = 1 + rng.below(160);
    let mut text = String::new();
    for _ in 0..len {
        if rng.below(40) == 0 {
            text.push('\n');
        } else {
            text.push(alphabet[rng.below(alphabet.len())]);
        }
    }
    text
}

#[test]
fn text_without_a_flagged_character_is_laid_out_as_before() {
    let alphabet = unflagged();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut checked = 0;
    for round in 0..4000 {
        let text = random_text(&mut rng, &alphabet);
        let first = 30.0 + rng.below(120) as f32;
        let rest = 30.0 + rng.below(120) as f32;
        let width = |n: usize| if n == 0 { first } else { rest };
        let hyphen: Option<super::Hyphenate<'_>> = if round % 2 == 0 {
            Some(&hyphenate)
        } else {
            None
        };
        for options in plain_options() {
            let old = legacy_wrap_kerned_in(&text, &width, &advance, &kern, hyphen);
            let new = wrap_lines_kerned_in(&text, &width, &advance, &kern, hyphen, options);
            assert_eq!(new, old, "greedy, options {options:#x}, {text:?}");
            let width = |_: usize| rest;
            let cost = |left: f32, spaces: usize| if spaces > 2 { left * 0.01 } else { 0.0 };
            let old = legacy_wrap_balanced_in(&text, &width, &advance, &kern, hyphen, &cost);
            let new =
                wrap_lines_balanced_in(&text, &width, &advance, &kern, hyphen, &cost, options);
            assert_eq!(new, old, "balanced, options {options:#x}, {text:?}");
            checked += 1;
        }
    }
    assert!(checked > 20_000);
}

/// A text of one character per unit; `width` units per line.
fn lines_of(text: &str, width: f32, options: u32) -> Vec<Line> {
    wrap_lines_kerned_in(
        text,
        &|_| width,
        &|_, _| 10.0,
        &|_, _, _| 0.0,
        None,
        options,
    )
}

fn texts<'a>(text: &'a str, lines: &[Line]) -> Vec<&'a str> {
    lines.iter().map(|l| &text[l.range.clone()]).collect()
}

#[test]
fn chinese_breaks_between_characters_and_keeps_closing_marks_off_a_line_start() {
    let text = "我们在这里读书。他们「在」那里写字，很好。";
    let lines = lines_of(text, 50.0, 0);
    for line in texts(text, &lines) {
        let first = line.chars().next().unwrap();
        let last = line.chars().last().unwrap();
        assert!(!"。，」".contains(first), "a closing mark starts {line:?}");
        assert!(last != '「', "an opening mark ends {line:?}");
        assert!(line.chars().count() <= 5);
    }
    assert!(lines.len() >= 4, "{lines:?}");
}

#[test]
fn lines_are_as_full_as_they_can_be() {
    // Hangul and Chinese, several widths: adding the next allowed segment
    // would overflow (the greedy rule), so no line ends a character early.
    let texts_in = [
        "한국어는 매우 아름다운 언어입니다 그리고 우리는 그것을 사랑합니다",
        "한국어는매우아름다운언어입니다그리고우리는그것을사랑합니다",
        "我们在这里读书他们在那里写字很好大家都很高兴我们明天再见",
    ];
    for text in texts_in {
        for units in 3..14 {
            let width = units as f32 * 10.0;
            let lines = lines_of(text, width, 0);
            for pair in lines.windows(2) {
                let first = &text[pair[0].range.clone()];
                let next = text[pair[1].range.start..].chars().next().unwrap();
                // A line may be short only where the next character would
                // not fit, or where it ends at a space (a word moved down).
                let used = first.chars().count() as f32 * 10.0;
                let next_is_word = text[pair[0].range.end..pair[1].range.start].contains(' ');
                if !next_is_word {
                    assert!(
                        used + 10.0 > width,
                        "{first:?} is short of {units} before {next:?} in {text:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_line_never_ends_where_nothing_moves() {
    // Overflow that leaves an ideograph, or an opening mark, at the start of
    // the new line, under a guard on the calls made (an endless loop would
    // trip it).
    let calls = Cell::new(0usize);
    let advance = |_: usize, c: char| {
        calls.set(calls.get() + 1);
        assert!(calls.get() < 2_000_000, "the wrapper does not move forward");
        if c == ' ' { 4.0 } else { 10.0 }
    };
    for text in [
        "漢漢漢漢漢漢漢漢漢漢漢漢",
        "「「「「「「「「「「",
        "a漢b漢c漢d漢e",
        "Hello「世界」Hello「世界」",
        "漢字 漢字 漢字 漢字",
    ] {
        for width in [5.0, 9.0, 10.0, 11.0, 25.0] {
            for options in [0, KEEP_ALL, LOOSE, ANYWHERE, BREAK_ALL] {
                let lines =
                    wrap_lines_kerned_in(text, &|_| width, &advance, &|_, _, _| 0.0, None, options);
                let mut at = 0;
                for line in &lines {
                    assert!(line.range.start >= at, "{text:?} {lines:?}");
                    at = line.range.end;
                }
                let cost = |_: f32, _: usize| 0.0;
                let _ = wrap_lines_balanced_in(
                    text,
                    &|_| width,
                    &advance,
                    &|_, _, _| 0.0,
                    None,
                    &cost,
                    options,
                );
            }
        }
    }
}

#[test]
fn korean_breaks_between_syllables_unless_keep_all() {
    let text = "한국어는 아름다운 언어입니다";
    let free = lines_of(text, 35.0, 0);
    assert!(free.iter().any(|l| {
        // a word split between two lines
        let end = text[l.range.end..].chars().next();
        end.is_some_and(|c| c != ' ' && c != '\n') && l.range.end < text.len()
    }));
    let kept = lines_of(text, 60.0, KEEP_ALL);
    for line in &kept {
        let after = text[line.range.end..].chars().next();
        assert!(
            after.is_none_or(|c| c == ' '),
            "keep-all splits a word: {:?}",
            texts(text, &kept)
        );
    }
}

#[test]
fn keep_all_keeps_hanja_words_together_too() {
    let text = "大韓民國 大韓民國";
    let kept = lines_of(text, 45.0, KEEP_ALL);
    assert_eq!(texts(text, &kept), vec!["大韓民國", "大韓民國"]);
    let free = lines_of(text, 45.0, 0);
    assert_eq!(texts(text, &free)[0], "大韓民國");
}

#[test]
fn a_zero_width_space_is_a_break_and_takes_no_room() {
    let text = "สวัสดี\u{200B}ครับ\u{200B}ขอบคุณ";
    let lines = lines_of(text, 60.0, 0);
    let shown = texts(text, &lines);
    assert!(shown.len() >= 2, "{shown:?}");
    for line in &lines {
        let inside = &text[line.range.clone()];
        assert!(!inside.ends_with('\u{200B}') || line.width <= 60.0);
    }
    // Without it a Thai word is not broken (no break inside a word).
    let word = "สวัสดีครับขอบคุณ";
    let one = wrap_lines_kerned_in(word, &|_| 1000.0, &|_, _| 10.0, &|_, _, _| 0.0, None, 0);
    assert_eq!(one.len(), 1);
}

#[test]
fn break_all_and_anywhere_break_inside_latin_words() {
    let text = "internationalization";
    assert_eq!(lines_of(text, 55.0, 0).len(), 4, "cut at overflow");
    for options in [BREAK_ALL, ANYWHERE] {
        let lines = lines_of(text, 55.0, options);
        assert!(lines.len() >= 3);
        for line in &lines {
            assert!(line.width <= 55.0 + 0.01);
        }
    }
    // STRICT and LOOSE do not lift the filter: nothing else changes.
    let lines = lines_of("hello world again", 70.0, STRICT);
    assert_eq!(
        texts("hello world again", &lines),
        ["hello", "world", "again"]
    );
}

/// A long paragraph that has more places to break than the balanced wrapper
/// weighs (and one longer than 64 KB): greedy lines come out, valid.
#[test]
fn a_paragraph_with_too_many_places_is_broken_greedily_and_validly() {
    let syllables: String = "가나다라마바사아자차카타파하"
        .chars()
        .cycle()
        .take(1500)
        .collect();
    let cost = |_: f32, _: usize| 0.0;
    let lines = wrap_lines_balanced_in(
        &syllables,
        &|_| 100.0,
        &|_, _| 10.0,
        &|_, _, _| 0.0,
        None,
        &cost,
        0,
    );
    let mut at = 0;
    for line in &lines {
        assert_eq!(line.range.start, at);
        at = line.range.end;
        assert!(line.width <= 100.0 + 0.01, "{line:?}");
    }
    assert_eq!(at, syllables.len());
    let huge: String = "가".repeat(30_000);
    let lines = wrap_lines_balanced_in(
        &huge,
        &|_| 100.0,
        &|_, _| 10.0,
        &|_, _, _| 0.0,
        None,
        &cost,
        0,
    );
    assert_eq!(lines.last().unwrap().range.end, huge.len());
    assert!(lines.len() >= 2900);
}

/// The state at a new line start must be that of one pass over the
/// paragraph, for each of the three ways a line ends.
#[test]
fn the_state_at_a_line_start_is_that_of_one_pass() {
    use crate::linebreak::{LineBreaker, Opportunity};
    /// The places one pass allows (compat profile).
    fn places(text: &str, options: u32) -> Vec<usize> {
        let mut breaker = LineBreaker::new(options);
        let mut out = Vec::new();
        for (at, c) in text.char_indices() {
            if breaker.feed(c, &text[at + c.len_utf8()..]) != Opportunity::No {
                out.push(at);
            }
        }
        out
    }
    let cases = [
        // (a) a line ends at a space, then a flagged character follows
        "aaa bbb 漢字漢字 ccc",
        // (b) an overlong word is cut at overflow before an ideograph
        "abcdefghijklmnopqrstuvwxyz漢字漢字漢字",
        // (c) a hyphenation cut before a CJK quotation mark
        "international「世界」international「世界」",
        // flags (regional indicators) and numbers across a cut
        "🇺🇸🇩🇪🇫🇷🇯🇵🇬🇧 100% 200%",
    ];
    for text in cases {
        for width in [23.0, 37.0, 55.0, 70.0] {
            let hyphen: Option<super::Hyphenate<'_>> = Some(&hyphenate);
            let lines = wrap_lines_kerned_in(
                text,
                &|_| width,
                &|_, c| if c == ' ' { 4.0 } else { 10.0 },
                &|_, _, _| 0.0,
                hyphen,
                0,
            );
            let allowed = places(text, 0);
            // The places the next line's own breaks use are allowed ones:
            // every line end that is not a cut is in `allowed` (or at a space).
            for line in &lines[..lines.len().saturating_sub(1)] {
                let end = line.range.end;
                let after = lines
                    .iter()
                    .find(|l| l.range.start >= end)
                    .map(|l| l.range.start)
                    .unwrap_or(text.len());
                let ok = allowed.contains(&after)
                    || line.hyphen
                    || text[end..after].contains(' ')
                    || after == end;
                // a cut at overflow (no place on the line) is allowed too
                let cut_at_overflow = !allowed.iter().any(|&p| p > line.range.start && p <= after);
                assert!(
                    ok || cut_at_overflow,
                    "{text:?} width {width}: {:?}",
                    texts(text, &lines)
                );
            }
        }
    }
}
