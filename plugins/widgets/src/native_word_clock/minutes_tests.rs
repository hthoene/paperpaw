//! The word clock for every minute of the day in each language it
//! speaks: the five minutes it tells (rounded, or the last five with dots
//! for the rest), the sentence checked against an English reckoning of
//! its own and against known phrases in both languages (noon, the
//! quarters, midnight, a minute before it), every sentence complete and
//! lit in the grid of letters in reading order.

extern crate std;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_i18n::Catalog;
use paperpaw_sdk::Translator;

use super::{Fake, grid, lit, sentence, told};

const LANGUAGES: [&str; 2] = ["en", "de"];

fn translator(language: &str) -> Translator {
    let mut t = Translator::new();
    t.add(Catalog::parse(include_str!("../../i18n/en.ini")).unwrap());
    t.add(Catalog::parse(include_str!("../../i18n/de.ini")).unwrap());
    t.set_language(language);
    t
}

/// What the clock says at `hour`:`minute` (rounded to five minutes).
fn says(t: &Translator, hour: u8, minute: u8, next: bool) -> String {
    let (h, m, _) = told(hour, minute, false);
    let (lead, rest) = sentence(&mut Fake(t), h, m, next).unwrap();
    format!("{lead} {rest}")
}

/// The time in English words, reckoned here independently of the
/// translations.
fn english(hour: u8, minute: u8) -> String {
    const HOURS: [&str; 12] = [
        "twelve", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven",
    ];
    let total = (u32::from(hour) * 60 + u32::from(minute) + 2) / 5 * 5;
    let (h, m) = ((total / 60) % 24, total % 60);
    let this = HOURS[(h % 12) as usize];
    let next = HOURS[((h + 1) % 12) as usize];
    let rest = match m {
        0 => format!("{this} o'clock"),
        5 => format!("five past {this}"),
        10 => format!("ten past {this}"),
        15 => format!("quarter past {this}"),
        20 => format!("twenty past {this}"),
        25 => format!("twenty-five past {this}"),
        30 => format!("half past {this}"),
        35 => format!("twenty-five to {next}"),
        40 => format!("twenty to {next}"),
        45 => format!("quarter to {next}"),
        50 => format!("ten to {next}"),
        _ => format!("five to {next}"),
    };
    format!("It is {rest}")
}

#[test]
fn every_minute_is_told_to_the_nearest_five() {
    for hour in 0..24u8 {
        for minute in 0..60u8 {
            let (h, m, rest) = told(hour, minute, false);
            assert_eq!(rest, 0);
            assert!(h < 24 && m < 60 && m % 5 == 0, "{hour}:{minute}");
            // At most two and a half minutes off, round the clock.
            let now = i32::from(hour) * 60 + i32::from(minute);
            let said = i32::from(h) * 60 + i32::from(m);
            let off = (said - now).rem_euclid(1440);
            let off = off.min(1440 - off);
            assert!(off <= 2, "{hour}:{minute} told as {h}:{m}");
            // Two and a half rounds up: :03 is the next five.
            assert_eq!(said.rem_euclid(1440) == now, minute % 5 == 0);
        }
    }
}

#[test]
fn with_dots_every_minute_is_its_five_and_the_minutes_past() {
    for hour in 0..24u8 {
        for minute in 0..60u8 {
            let (h, m, past) = told(hour, minute, true);
            assert_eq!((h, m + past), (hour, minute), "{hour}:{minute}");
            assert!(m % 5 == 0 && past < 5);
        }
    }
}

#[test]
fn every_english_minute_says_what_an_english_clock_would() {
    let en = translator("en");
    for hour in 0..24u8 {
        for minute in 0..60u8 {
            assert_eq!(
                says(&en, hour, minute, false),
                english(hour, minute),
                "{hour}:{minute}"
            );
        }
    }
}

#[test]
fn known_phrases_round_noon_and_midnight() {
    let en = translator("en");
    for (hour, minute, text) in [
        (12, 0, "It is twelve o'clock"),
        (12, 15, "It is quarter past twelve"),
        (12, 30, "It is half past twelve"),
        (12, 45, "It is quarter to one"),
        (0, 0, "It is twelve o'clock"),
        (23, 59, "It is twelve o'clock"),
        (23, 57, "It is five to twelve"),
        (23, 58, "It is twelve o'clock"),
        (23, 55, "It is five to twelve"),
        (11, 58, "It is twelve o'clock"),
        (12, 58, "It is one o'clock"),
        (0, 33, "It is twenty-five to one"),
    ] {
        assert_eq!(says(&en, hour, minute, false), text, "en {hour}:{minute}");
    }
    let de = translator("de");
    for (hour, minute, next, text) in [
        (12, 0, false, "Es ist zwölf Uhr"),
        (12, 15, false, "Es ist Viertel nach zwölf"),
        (12, 30, false, "Es ist halb eins"),
        (12, 45, false, "Es ist Viertel vor eins"),
        (0, 0, false, "Es ist zwölf Uhr"),
        (23, 59, false, "Es ist zwölf Uhr"),
        (23, 55, false, "Es ist fünf vor zwölf"),
        // One o'clock is "ein Uhr", but "eins" in every other phrase.
        (1, 0, false, "Es ist ein Uhr"),
        (13, 0, false, "Es ist ein Uhr"),
        (12, 58, false, "Es ist ein Uhr"),
        (1, 5, false, "Es ist fünf nach eins"),
        (0, 55, false, "Es ist fünf vor eins"),
        (0, 30, false, "Es ist halb eins"),
        // Quarters towards the next hour.
        (12, 15, true, "Es ist viertel eins"),
        (12, 45, true, "Es ist dreiviertel eins"),
        (23, 45, true, "Es ist dreiviertel zwölf"),
        (0, 15, true, "Es ist viertel eins"),
        (23, 59, true, "Es ist zwölf Uhr"),
    ] {
        assert_eq!(
            says(&de, hour, minute, next),
            text,
            "de {hour}:{minute} {next}"
        );
    }
}

#[test]
fn every_sentence_of_the_day_is_complete() {
    for language in LANGUAGES {
        let t = translator(language);
        let lead = if language == "en" { "It is" } else { "Es ist" };
        for next in [false, true] {
            for hour in 0..24u8 {
                for minute in 0..60u8 {
                    let text = says(&t, hour, minute, next);
                    assert!(text.starts_with(lead), "{language} {hour}:{minute}: {text}");
                    // Every placeholder filled, no key shown for want of
                    // a translation.
                    assert!(
                        !text.contains(['{', '}']) && !text.contains("widgets."),
                        "{language} {hour}:{minute}: {text}"
                    );
                    assert!(text.split_whitespace().count() >= 3, "{text}");
                }
            }
        }
    }
}

#[test]
fn the_same_five_minutes_say_the_same_and_the_next_say_otherwise() {
    for language in LANGUAGES {
        let t = translator(language);
        let mut before = says(&t, 23, 58, false);
        let mut changes = 0;
        for minute in 0..24 * 60u16 {
            let (hour, minute) = ((minute / 60) as u8, (minute % 60) as u8);
            let text = says(&t, hour, minute, false);
            if text != before {
                changes += 1;
                // It changes as the next five minutes are nearer.
                assert_eq!(minute % 5, 3, "{language} {hour}:{minute}");
            }
            before = text;
        }
        // Twelve fives an hour, every one said differently.
        assert_eq!(changes, 24 * 12, "{language}");
    }
}

/// The letters `words` (in capitals) spell, without anything else.
fn letters(words: &str) -> String {
    words
        .chars()
        .filter(|c| c.is_alphabetic())
        .flat_map(char::to_uppercase)
        .collect()
}

#[test]
fn the_grid_lights_every_sentence_in_reading_order() {
    for language in LANGUAGES {
        let t = translator(language);
        let rows = grid(&mut Fake(&t)).unwrap();
        let columns = rows.iter().map(Vec::len).max().unwrap();
        for next in [false, true] {
            for hour in 0..24u8 {
                for minute in (0..60u8).step_by(5) {
                    let (lead, rest) = sentence(&mut Fake(&t), hour, minute, next).unwrap();
                    let words = format!("{lead} {rest}");
                    let mut cells = lit(&rows, &words).unwrap();
                    // Each letter once, all in the grid.
                    cells.sort_by_key(|&(r, c)| r * columns + c);
                    let before = cells.len();
                    cells.dedup();
                    assert_eq!(cells.len(), before, "{language}: {words}");
                    assert!(cells.iter().all(|&(r, c)| c < rows[r].len()));
                    // Read row by row, the lit letters spell the sentence.
                    let read: String = cells.iter().map(|&(r, c)| rows[r][c]).collect();
                    assert_eq!(read, letters(&words), "{language}");
                }
            }
        }
    }
}

#[test]
fn words_not_in_the_grid_light_nothing() {
    let rows = grid(&mut Fake(&translator("en"))).unwrap();
    assert_eq!(lit(&rows, "It is quarter past quarter past ten"), None);
    assert_eq!(lit(&rows, "Zwölf"), None);
    // Nothing to light: no letters lit, not none found.
    assert_eq!(lit(&rows, ""), Some(Vec::new()));
    assert_eq!(lit(&rows, "- , !"), Some(Vec::new()));
    // An empty grid finds no word.
    assert_eq!(lit(&[], "It is"), None);
}
