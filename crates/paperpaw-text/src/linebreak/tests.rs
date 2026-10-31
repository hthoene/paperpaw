//! Tests of the class table. `LineBreak.txt` and the other Unicode files are
//! read where `PAPERPAW_UCD_TESTS` names a folder with them (CI sets it and
//! fails without it, see `ucd`).

extern crate std;

use std::path::PathBuf;
use std::prelude::rust_2024::*;
use std::{fs, println};

use super::table::{self, lookup};
use super::table_data::{Class, RUNS};
use super::*;

/// The folder with the Unicode files, or `None` locally; in CI (`CI` set)
/// a missing folder is a failure, never a skip.
pub(super) fn ucd() -> Option<PathBuf> {
    match std::env::var_os("PAPERPAW_UCD_TESTS") {
        Some(dir) => Some(PathBuf::from(dir)),
        None => {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI must run the Unicode tests: set PAPERPAW_UCD_TESTS (design, section 4.3)"
            );
            println!("skipped: no PAPERPAW_UCD_TESTS");
            None
        }
    }
}

#[test]
fn the_header_says_what_the_table_is() {
    let (format, version, classes, _index, _stream) = table::header();
    assert_eq!(format, 1);
    assert_eq!(version, UNICODE_VERSION);
    assert_eq!(usize::from(classes), super::table_data::CLASSES.len());
    assert!(RUNS > 1000);
}

#[test]
fn the_unicode_version_is_the_one_of_the_bidi_tables() {
    let tables = include_str!("../bidi/tables.rs");
    let first = tables.lines().nth(1).unwrap();
    let (major, minor, update) = UNICODE_VERSION;
    let want = std::format!("{major}.{minor}.{update}");
    assert!(first.contains(&want), "{first} is not Unicode {want}");
}

#[test]
fn some_known_characters() {
    let class = |c: char| lookup(c).class;
    assert_eq!(class('a'), Class::AL);
    assert_eq!(class(' '), Class::SP);
    assert_eq!(class('-'), Class::HY);
    assert_eq!(class('\u{200B}'), Class::ZW);
    assert_eq!(class('\u{2060}'), Class::WJ);
    assert_eq!(class('\u{00A0}'), Class::GL);
    assert_eq!(class('\u{00AD}'), Class::BA);
    assert_eq!(class('漢'), Class::ID);
    assert_eq!(class('。'), Class::CL);
    assert_eq!(class('「'), Class::OP);
    assert_eq!(class('가'), Class::H2);
    assert_eq!(class('각'), Class::H3);
    assert_eq!(class('\u{1100}'), Class::JL);
    assert_eq!(class('“'), Class::QUPI);
    assert_eq!(class('”'), Class::QUPF);
    assert_eq!(class('"'), Class::QU);
    assert_eq!(class('ゃ'), Class::CJ);
    assert_eq!(class('\u{0E01}'), Class::AL); // Thai letter: SA, resolved
    assert_eq!(class('\u{0E31}'), Class::CM); // Thai vowel sign: SA with Mn
    assert!(lookup('漢').uax && lookup('漢').wide);
    assert!(!lookup('a').uax && !lookup('a').wide);
    assert!(lookup('\u{200B}').uax);
    assert!(
        lookup('\u{3000}').uax,
        "the ideographic space is in a flagged block"
    );
    assert!(
        lookup('Ａ').wide && lookup('Ａ').uax,
        "fullwidth Latin is flagged"
    );
}

#[test]
fn the_first_and_last_code_points_decode() {
    assert_eq!(lookup('\u{0}').class, Class::CM);
    assert_eq!(lookup('\u{10FFFF}').class, Class::AL);
    assert_eq!(lookup('\u{E0001}').class, Class::CM);
}

/// Every code point decodes to the class `LineBreak.txt` gives, resolved as
/// the generator does (the table is the generator's, so this checks the
/// decoder and the encoding).
#[test]
fn every_code_point_decodes_to_its_class() {
    let Some(dir) = ucd() else { return };
    let data = fs::read_to_string(dir.join("LineBreak.txt")).unwrap();
    let mut gc_cm = std::collections::BTreeSet::new();
    let unicode = fs::read_to_string(dir.join("UnicodeData.txt")).unwrap();
    for line in unicode.lines() {
        let f: Vec<&str> = line.split(';').collect();
        if f[2] == "Mn" || f[2] == "Mc" {
            gc_cm.insert(u32::from_str_radix(f[0], 16).unwrap());
        }
    }
    let mut checked = 0;
    for line in data.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let (range, class) = line.split_once(';').unwrap();
        let range = range.trim();
        let (a, b) = range.split_once("..").unwrap_or((range, range));
        let (a, b) = (
            u32::from_str_radix(a, 16).unwrap(),
            u32::from_str_radix(b, 16).unwrap(),
        );
        for cp in a..=b {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let got = lookup(c).class;
            let ok = match class.trim() {
                "AI" | "SG" => got == Class::AL,
                "SA" => {
                    got == if gc_cm.contains(&cp) {
                        Class::CM
                    } else {
                        Class::AL
                    }
                }
                "QU" => matches!(got, Class::QU | Class::QUPI | Class::QUPF),
                "ID" => matches!(got, Class::ID | Class::IDEP),
                "XX" => matches!(got, Class::AL | Class::ALEP),
                other => std::format!("{got:?}") == other,
            };
            assert!(
                ok,
                "U+{cp:04X}: LineBreak.txt says {class}, the table {got:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 100_000, "{checked}");
}

mod conformance {
    use super::*;
    use crate::linebreak::breaker::{LineBreaker, Opportunity};

    /// `LineBreakTest.txt`: `× 0023 × 0020 ÷ 0041 ÷ # comment`. A `×` before
    /// a character means no break, a `÷` a break; the first mark is the start
    /// of text and the last the end of text (always a break, LB3).
    #[test]
    fn line_break_test_txt() {
        let Some(dir) = ucd() else { return };
        let data = fs::read_to_string(dir.join("LineBreakTest.txt")).unwrap();
        let (mut cases, mut failed) = (0, Vec::new());
        for line in data.lines() {
            let body = line.split('#').next().unwrap().trim();
            if body.is_empty() {
                continue;
            }
            let mut marks = Vec::new();
            let mut text = String::new();
            for token in body.split_whitespace() {
                match token {
                    "×" => marks.push(false),
                    "÷" => marks.push(true),
                    hex => {
                        text.push(char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap())
                    }
                }
            }
            assert_eq!(marks.len(), text.chars().count() + 1, "{line}");
            cases += 1;
            let mut breaker = LineBreaker::full();
            let mut got = Vec::new();
            for (at, c) in text.char_indices() {
                let opportunity = breaker.feed(c, &text[at + c.len_utf8()..]);
                got.push(opportunity != Opportunity::No);
            }
            // The start of text never breaks (LB2), the end always (LB3).
            let want: Vec<bool> = marks[..marks.len() - 1].to_vec();
            if got[1..] != want[1..] || got[0] {
                failed.push(std::format!("{line}\n    got {got:?}"));
            }
        }
        for f in failed.iter().take(12) {
            println!("{f}");
        }
        assert!(failed.is_empty(), "{} of {cases} cases fail", failed.len());
        assert!(cases > 19_000, "{cases}");
    }
}
