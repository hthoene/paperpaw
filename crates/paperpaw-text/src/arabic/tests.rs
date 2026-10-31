use alloc::string::String;
use alloc::vec::Vec;

use super::*;

fn keys_of(text: &str) -> Vec<char> {
    keys(text, Context::NONE).collect()
}

#[test]
fn one_key_per_character() {
    for line in include_str!("../../tests/data/arabic-golden.txt").lines() {
        if let Some((text, _)) = line.split_once('\t') {
            assert_eq!(keys_of(text).len(), text.chars().count(), "{text}");
        }
    }
}

/// The keys as the independent reshaper writes them: no key for a
/// consumed alef or a control, and a letter without presentation forms
/// as itself.
fn comparable(text: &str) -> Vec<char> {
    keys_of(text)
        .into_iter()
        .filter(|&k| k != NO_GLYPH)
        .map(|k| {
            if u32::from(k) >= PRIVATE {
                letter_of(k).0
            } else {
                k
            }
        })
        .collect()
}

#[test]
fn keys_match_an_independent_reshaper() {
    let mut checked = 0;
    for line in include_str!("../../tests/data/arabic-golden.txt").lines() {
        let Some((text, want)) = line.split_once('\t') else {
            continue;
        };
        let want: Vec<char> = want
            .split_whitespace()
            .filter_map(|h| u32::from_str_radix(h, 16).ok().and_then(char::from_u32))
            .filter(|&c| !matches!(c, '\u{200C}' | '\u{200D}'))
            .collect();
        let got = comparable(text);
        assert_eq!(
            got.iter().map(|c| u32::from(*c)).collect::<Vec<_>>(),
            want.iter().map(|c| u32::from(*c)).collect::<Vec<_>>(),
            "{text}"
        );
        checked += 1;
    }
    assert_eq!(checked, 20);
}

#[test]
fn lam_alef_takes_a_mark_between_and_the_alef_draws_nothing() {
    // lam, fatha, alef: the ligature, the mark, nothing.
    assert_eq!(
        keys_of("\u{644}\u{64E}\u{627}"),
        ['\u{FEFB}', '\u{64E}', NO_GLYPH]
    );
    // beh, lam, alef: lam joins beh, so the final ligature.
    assert_eq!(
        keys_of("\u{628}\u{644}\u{627}"),
        ['\u{FE91}', '\u{FEFC}', NO_GLYPH]
    );
}

#[test]
fn zwnj_breaks_and_zwj_and_tatweel_join() {
    // beh ZWNJ beh: both isolated.
    assert_eq!(
        keys_of("\u{628}\u{200C}\u{628}"),
        ['\u{628}', NO_GLYPH, '\u{628}']
    );
    // beh ZWJ: beh initial.
    assert_eq!(keys_of("\u{628}\u{200D}"), ['\u{FE91}', NO_GLYPH]);
    // beh tatweel beh: initial, tatweel, final.
    assert_eq!(
        keys_of("\u{628}\u{640}\u{628}"),
        ['\u{FE91}', '\u{640}', '\u{FE90}']
    );
}

/// A zero width space and a word joiner (a break place and a no-break mark)
/// are invisible but join nothing across them, as before they were
/// invisible: beh, the mark, beh gives two isolated letters (design of
/// line breaking, M1). Unicode's data says Transparent; this is deliberate.
#[test]
fn zero_width_space_and_word_joiner_join_nothing_across_them() {
    for mark in ['\u{200B}', '\u{2060}', '\u{2061}', '\u{2064}'] {
        let text = alloc::format!("\u{628}{mark}\u{628}");
        assert_eq!(keys_of(&text), ['\u{628}', NO_GLYPH, '\u{628}'], "{mark:?}");
    }
    // The byte order mark stays Transparent (Unicode's data says so, and it
    // did before): the letters around it join.
    assert_eq!(
        keys_of("\u{628}\u{FEFF}\u{628}"),
        ['\u{FE91}', NO_GLYPH, '\u{FE90}']
    );
}

#[test]
fn a_run_keeps_its_neighbours_joining() {
    // The middle letter of beh beh beh, drawn as a run of its own.
    let context = Context::new(Some('\u{628}'), Some('\u{628}'));
    assert_eq!(keys("\u{628}", context).collect::<Vec<_>>(), ['\u{FE92}']);
    // A lam ends one run, its alef starts the next.
    assert_eq!(
        keys("\u{644}", Context::new(None, Some('\u{627}'))).collect::<Vec<_>>(),
        ['\u{FEFB}']
    );
    assert_eq!(
        keys("\u{627}", Context::new(Some('\u{644}'), None)).collect::<Vec<_>>(),
        [NO_GLYPH]
    );
}

#[test]
fn letters_without_presentation_forms_take_private_keys() {
    // Urdu heh goal with hamza (U+06C2) between two behs: medial.
    let k = keys_of("\u{628}\u{6C2}\u{628}");
    assert_eq!(k[1], char::from_u32(PRIVATE + 4 * 0xC2 + 3).unwrap_or('?'));
    assert_eq!(letter_of(k[1]), ('\u{6C2}', Form::Medial));
    assert!(u32::from(k[1]) > 0xF_0000 && u32::from(k[1]) < 0xF_1000);
}

#[test]
fn letter_of_undoes_form_key() {
    for cp in 0x600..=0x8FF_u32 {
        let Some(letter) = char::from_u32(cp) else {
            continue;
        };
        for form in [Form::Isolated, Form::Final, Form::Initial, Form::Medial] {
            let key = form_key(letter, form);
            if key != letter {
                assert_eq!(letter_of(key), (letter, form), "{cp:04X}");
            }
        }
    }
}

#[test]
fn marks_stack_as_naskh_does() {
    let mut places = [Place {
        side: Side::Above,
        step: 0,
    }; 3];
    // shadda then fatha: shadda on the letter, fatha on top.
    assert_eq!(place_marks(&['\u{651}', '\u{64E}'], &mut places), 2);
    assert_eq!(
        places[..2],
        [
            Place {
                side: Side::Above,
                step: 0
            },
            Place {
                side: Side::Above,
                step: 1
            }
        ]
    );
    // fatha then shadda (canonical order): the same places.
    place_marks(&['\u{64E}', '\u{651}'], &mut places);
    assert_eq!(
        places[..2],
        [
            Place {
                side: Side::Above,
                step: 1
            },
            Place {
                side: Side::Above,
                step: 0
            }
        ]
    );
    // kasra with shadda: over the letter, under the shadda.
    place_marks(&['\u{651}', '\u{650}'], &mut places);
    assert_eq!(
        places[..2],
        [
            Place {
                side: Side::Above,
                step: 1
            },
            Place {
                side: Side::Above,
                step: 0
            }
        ]
    );
    // kasra alone: below.
    place_marks(&['\u{650}'], &mut places);
    assert_eq!(
        places[0],
        Place {
            side: Side::Below,
            step: 0
        }
    );
}

#[test]
fn search_folds_marks_and_letter_variants() {
    let folded: String = "\u{623}\u{64E}\u{644}\u{652}\u{640}\u{649} \u{663}"
        .chars()
        .filter_map(search_fold)
        .collect();
    assert_eq!(folded, "\u{627}\u{644}\u{64A} 3");
}

/// The example of the module's documentation.
#[test]
fn the_example() {
    // "salam": seen, lam, alef, meem.
    let keys: Vec<char> = keys("\u{633}\u{644}\u{627}\u{645}", Context::NONE).collect();
    // Meem after alef stands alone: the letter itself.
    assert_eq!(keys, ['\u{FEB3}', '\u{FEFC}', NO_GLYPH, '\u{645}']);
}
