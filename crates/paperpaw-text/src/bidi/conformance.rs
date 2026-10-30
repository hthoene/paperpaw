//! Unicode's conformance tests of the bidirectional algorithm
//! (`BidiTest.txt`, `BidiCharacterTest.txt`), run whole and line by
//! line: every paragraph is also resolved in two lines at every place,
//! and one character a line, with the carry going from line to line, and
//! must give the levels of the whole paragraph, rule L1 applied per line.
//!
//! A part of `BidiCharacterTest.txt` lies in the crate's `tests/data` and
//! always runs. The whole files run where `PAPERPAW_UCD_TESTS` names a folder
//! with them (of the Unicode version of `src/bidi/tables.rs`):
//!
//! ```sh
//! PAPERPAW_UCD_TESTS=<folder> cargo test --release -p paperpaw-text \
//!     --lib bidi::conformance -- --ignored --nocapture
//! ```

extern crate std;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::prelude::rust_2024::*;
use std::{format, println, vec};

use crate::bidi::{
    self, BRACKETS, Carry, Class, DEPTH, ISOLATES, Level, is_removed, reset_line_end, visual_order,
};

/// What a test case asks.
struct Case {
    text: String,
    paragraph: Option<Level>,
    levels: Vec<Option<u8>>,
    order: Vec<usize>,
}

#[derive(Default)]
struct Tally {
    cases: usize,
    passed: usize,
    beyond_limits: usize,
    failed: Vec<String>,
    splits: usize,
    splits_equal: usize,
    split_failed: Vec<String>,
}

impl Tally {
    fn report(&self, name: &str) -> String {
        format!(
            "{name} (carry {} bytes): {} cases, {} pass, {} fail beyond the limits (depth {DEPTH}, isolates {ISOLATES}, brackets {BRACKETS}), {} other failures; line by line: {} splits, {} equal to the whole paragraph",
            core::mem::size_of::<Carry>(),
            self.cases,
            self.passed,
            self.beyond_limits,
            self.failed.len(),
            self.splits,
            self.splits_equal
        )
    }
}

/// Levels before rule L1, for the whole paragraph.
fn whole(text: &str, paragraph: Level) -> Vec<Level> {
    let n = text.chars().count();
    let mut levels = vec![Level::LTR; n];
    let written = Carry::new(paragraph).resolve(text.chars(), n, &mut levels);
    assert_eq!(written, n);
    levels
}

/// Levels with L1 for lines that end at `breaks` (char indexes).
fn by_lines(text: &str, paragraph: Level, breaks: &[usize]) -> Vec<Level> {
    let chars: Vec<char> = text.chars().collect();
    let mut carry = Carry::new(paragraph);
    let mut out = Vec::new();
    let mut from = 0;
    for &to in breaks.iter().chain(core::iter::once(&chars.len())) {
        let mut levels = vec![Level::LTR; to - from];
        let written = carry.resolve(chars[from..].iter().copied(), to - from, &mut levels);
        assert_eq!(written, to - from);
        let line: String = chars[from..to].iter().collect();
        reset_line_end(&line, &mut levels, paragraph);
        out.extend(levels);
        from = to;
    }
    out
}

/// The whole paragraph's levels with L1 per line.
fn whole_by_lines(text: &str, paragraph: Level, breaks: &[usize]) -> Vec<Level> {
    let chars: Vec<char> = text.chars().collect();
    let mut levels = whole(text, paragraph);
    let mut from = 0;
    for &to in breaks.iter().chain(core::iter::once(&chars.len())) {
        let line: String = chars[from..to].iter().collect();
        reset_line_end(&line, &mut levels[from..to], paragraph);
        from = to;
    }
    levels
}

fn shown(text: &str, levels: &[Level]) -> Vec<Option<u8>> {
    text.chars()
        .zip(levels)
        .map(|(ch, l)| (!is_removed(ch)).then_some(l.number()))
        .collect()
}

/// Whether the case nests deeper than the limits allow.
fn beyond_limits(text: &str) -> bool {
    let (mut depth, mut most, mut open, mut most_open) = (0usize, 0usize, 0usize, 0usize);
    for ch in text.chars() {
        match bidi::class(ch) {
            Class::LRE
            | Class::RLE
            | Class::LRO
            | Class::RLO
            | Class::LRI
            | Class::RLI
            | Class::FSI => {
                depth += 1;
                most = most.max(depth);
            }
            Class::PDF | Class::PDI => depth = depth.saturating_sub(1),
            _ => {}
        }
        if matches!(ch, '(' | '[' | '{' | '\u{2329}' | '\u{3008}' | '\u{FF08}') {
            open += 1;
            most_open = most_open.max(open);
        }
        if matches!(ch, ')' | ']' | '}' | '\u{232A}' | '\u{3009}' | '\u{FF09}') {
            open = open.saturating_sub(1);
        }
    }
    most + 1 > DEPTH.min(ISOLATES + 1) || most_open > BRACKETS
}

fn check(case: &Case, tally: &mut Tally, splits: bool) {
    tally.cases += 1;
    let paragraph = case
        .paragraph
        .unwrap_or_else(|| bidi::paragraph_level(&case.text).unwrap_or(Level::LTR));
    let n = case.text.chars().count();
    let mut levels = whole(&case.text, paragraph);
    reset_line_end(&case.text, &mut levels, paragraph);
    let got = shown(&case.text, &levels);
    let mut order = vec![0u16; n];
    visual_order(&levels, &mut order);
    let chars: Vec<char> = case.text.chars().collect();
    let got_order: Vec<usize> = order
        .iter()
        .map(|&i| usize::from(i))
        .filter(|&i| !is_removed(chars[i]))
        .collect();
    if got == case.levels && got_order == case.order {
        tally.passed += 1;
    } else if beyond_limits(&case.text) {
        tally.beyond_limits += 1;
    } else if tally.failed.len() < 1000 {
        let mut line = String::new();
        for ch in case.text.chars() {
            let _ = write!(line, "{:04X} ", u32::from(ch));
        }
        let _ = write!(
            line,
            "para {:?}: want {:?} {:?}, got {:?} {:?}",
            case.paragraph, case.levels, case.order, got, got_order
        );
        tally.failed.push(line);
    } else {
        tally.failed.push(String::new());
    }
    if !splits || n < 2 {
        return;
    }
    let mut plans: Vec<Vec<usize>> = (1..n).map(|b| vec![b]).collect();
    plans.push((1..n).collect());
    for breaks in plans {
        tally.splits += 1;
        let a = shown(&case.text, &by_lines(&case.text, paragraph, &breaks));
        let b = shown(&case.text, &whole_by_lines(&case.text, paragraph, &breaks));
        if a == b {
            tally.splits_equal += 1;
        } else if tally.split_failed.len() < 50 {
            tally
                .split_failed
                .push(format!("{:?} breaks {breaks:?}: {a:?} vs {b:?}", case.text));
        }
    }
}

fn character_cases(text: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split(';').collect();
        if f.len() < 5 {
            continue;
        }
        let text: String = f[0]
            .split_whitespace()
            .filter_map(|h| u32::from_str_radix(h, 16).ok().and_then(char::from_u32))
            .collect();
        let paragraph = match f[1].trim() {
            "0" => Some(Level::LTR),
            "1" => Some(Level::RTL),
            _ => None,
        };
        let levels = f[3].split_whitespace().map(|l| l.parse().ok()).collect();
        let order = f[4]
            .split_whitespace()
            .filter_map(|i| i.parse().ok())
            .collect();
        cases.push(Case {
            text,
            paragraph,
            levels,
            order,
        });
    }
    cases
}

/// A character of each class, for `BidiTest.txt`.
fn sample(class: &str) -> Option<char> {
    Some(match class {
        "L" => 'a',
        "R" => '\u{5D0}',
        "AL" => '\u{627}',
        "EN" => '1',
        "ES" => '+',
        "ET" => '$',
        "AN" => '\u{660}',
        "CS" => ',',
        "NSM" => '\u{300}',
        "BN" => '\u{AD}',
        "B" => '\u{2029}',
        "S" => '\t',
        "WS" => ' ',
        "ON" => '!',
        "LRE" => '\u{202A}',
        "LRO" => '\u{202D}',
        "RLE" => '\u{202B}',
        "RLO" => '\u{202E}',
        "PDF" => '\u{202C}',
        "LRI" => '\u{2066}',
        "RLI" => '\u{2067}',
        "FSI" => '\u{2068}',
        "PDI" => '\u{2069}',
        _ => return None,
    })
}

fn class_cases(text: &str, mut each: impl FnMut(Case)) {
    let mut levels: Vec<Option<u8>> = Vec::new();
    let mut order: Vec<usize> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("@Levels:") {
            levels = rest.split_whitespace().map(|l| l.parse().ok()).collect();
            continue;
        }
        if let Some(rest) = line.strip_prefix("@Reorder:") {
            order = rest
                .split_whitespace()
                .filter_map(|i| i.parse().ok())
                .collect();
            continue;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with('@') {
            continue;
        }
        let Some((input, bits)) = line.split_once(';') else {
            continue;
        };
        let Some(text) = input
            .split_whitespace()
            .map(sample)
            .collect::<Option<String>>()
        else {
            continue;
        };
        let bits = u8::from_str_radix(bits.trim(), 16).unwrap_or(0);
        for (bit, paragraph) in [(1, None), (2, Some(Level::LTR)), (4, Some(Level::RTL))] {
            if bits & bit != 0 {
                each(Case {
                    text: text.clone(),
                    paragraph,
                    levels: levels.clone(),
                    order: order.clone(),
                });
            }
        }
    }
}

fn finish(name: &str, tally: &Tally) {
    println!("{}", tally.report(name));
    for f in tally.failed.iter().take(20) {
        println!("  fail: {f}");
    }
    for f in tally.split_failed.iter().take(20) {
        println!("  split: {f}");
    }
    assert!(tally.failed.is_empty(), "{}", tally.report(name));
    assert_eq!(tally.splits, tally.splits_equal, "{}", tally.report(name));
}

#[test]
fn the_part_of_the_character_tests_kept_here_passes() {
    let text = include_str!("../../tests/data/BidiCharacterTest-part.txt");
    let mut tally = Tally::default();
    for case in character_cases(text) {
        check(&case, &mut tally, true);
    }
    assert!(tally.cases > 500);
    finish("BidiCharacterTest-part", &tally);
}

fn ucd_file(name: &str) -> Option<String> {
    let dir = PathBuf::from(std::env::var_os("PAPERPAW_UCD_TESTS")?);
    std::fs::read_to_string(dir.join(name)).ok()
}

#[test]
#[ignore = "needs the UCD's BidiCharacterTest.txt in PAPERPAW_UCD_TESTS"]
fn the_character_tests_pass() {
    let text = ucd_file("BidiCharacterTest.txt").expect("PAPERPAW_UCD_TESTS/BidiCharacterTest.txt");
    let mut tally = Tally::default();
    for case in character_cases(&text) {
        check(&case, &mut tally, true);
    }
    finish("BidiCharacterTest", &tally);
}

#[test]
#[ignore = "needs the UCD's BidiTest.txt in PAPERPAW_UCD_TESTS"]
fn the_class_tests_pass() {
    let text = ucd_file("BidiTest.txt").expect("PAPERPAW_UCD_TESTS/BidiTest.txt");
    let mut tally = Tally::default();
    class_cases(&text, |case| check(&case, &mut tally, true));
    finish("BidiTest", &tally);
}
