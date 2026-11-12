#![allow(clippy::unwrap_used, reason = "fixtures")]
use super::*;
use paperpaw_sdk::MemoryStorage;

const WORDS: &str = "Rabbit\ten\t1\t10\tbooks/alice.txt\tAlice\tThe White Rabbit.\n\
                     white rabbit\ten-GB\t1\t10\tbooks/alice.txt\tAlice\t\n\
                     Hase\tde\t1\t10\tbooks/hase.txt\tHase\t\n\
                     Hatter\t\t1\t10\tbooks/alice.txt\tAlice\t\n\
                     Straße\tde\t1\t10\tbooks/hase.txt\tHase\t\n";

fn entries(language: &str) -> Entries {
    let storage = MemoryStorage::new().with(store::file(), WORDS);
    Entries::read(&storage, language).unwrap()
}

/// The marks of one run as (run, from, to).
fn marks(entries: &Entries, run: &str, number: u16) -> Vec<(u16, u16, u16)> {
    let mut out = Vec::new();
    entries.find(run, number, &mut out).unwrap();
    out.chunks(MARK)
        .map(|m| {
            let part = |at: usize| u16::from_le_bytes([m[at], m[at + 1]]);
            (part(0), part(2), part(4))
        })
        .collect()
}

#[test]
fn the_words_of_the_list_are_marked_as_beta_3_marked_them() {
    let run = "The White Rabbit, the rabbit and (Hatter). Alice";
    let found = marks(&entries("en"), run, 3);
    let seen: Vec<&str> = found
        .iter()
        .map(|&(n, from, to)| {
            assert_eq!(n, 3);
            &run[usize::from(from)..usize::from(to)]
        })
        .collect();
    // The phrase is one mark, a word and the punctuation the page shows with it too.
    assert_eq!(seen, ["White Rabbit,", "rabbit", "(Hatter)."]);
}

#[test]
fn only_the_words_of_the_books_language_are_marked_and_unknown_ones_count_for_every_book() {
    let de = entries("de");
    let found = marks(&de, "Der Hase und das Rabbit sind Hatter", 0);
    // Hase (de) and Hatter (any language); Rabbit is English.
    assert_eq!(found.len(), 2);
    assert!(entries("").len() > de.len(), "no language: every word");
    // A region does not matter, nor the case: the list says en-GB and EN.
    assert_eq!(
        marks(&entries(primary("EN-gb")), "white rabbit", 0).len(),
        1
    );
}

#[test]
fn case_sharp_s_accents_and_apostrophes_are_folded_as_the_sdk_folds_them() {
    let de = entries("de");
    assert_eq!(marks(&de, "die STRASSE", 0), [(0, 4, 11)]);
    let mut set = Entries::default();
    set.add("don\u{2019}t").unwrap();
    set.add("  (..)  ").unwrap();
    set.finish();
    assert_eq!(set.len(), 1, "text with no word is no entry");
    assert_eq!(marks(&set, "Don't stop", 0), [(0, 0, 5)]);
    // The same entry twice is one.
    set.add("DON'T").unwrap();
    set.finish();
    assert_eq!(set.len(), 1);
}

#[test]
fn the_longest_entry_wins_and_no_entry_goes_across_punctuation_alone() {
    let mut set = Entries::default();
    for word in ["white", "white rabbit", "rabbit hole"] {
        set.add(word).unwrap();
    }
    set.finish();
    // `white rabbit` beats `white`; `rabbit hole` is not started on, the words went.
    assert_eq!(marks(&set, "a white rabbit hole", 1), [(1, 2, 14)]);
    assert_eq!(marks(&set, "white - rabbit", 1), [(1, 0, 5)]);
}

#[test]
fn a_reply_holds_at_most_what_the_stage_allows_and_says_so() {
    let mut set = Entries::default();
    set.add("go").unwrap();
    set.finish();
    let run = "go ".repeat(700);
    let mut out = Vec::new();
    assert!(!set.find(&run, 0, &mut out).unwrap(), "the reply filled up");
    assert_eq!(out.len(), MAX_RANGES);
    assert_eq!(out.len() / MARK, 600);
}

#[test]
fn an_empty_list_and_an_empty_run_mark_nothing() {
    assert!(marks(&Entries::default(), "any text", 0).is_empty());
    assert!(marks(&entries("en"), "", 0).is_empty());
    assert!(marks(&entries("en"), "   ", 0).is_empty());
}

#[test]
fn the_look_is_the_roles_settings_in_the_stages_codes() {
    // Beta 3's default for the words being learnt: a dotted line, four tenths thick.
    let look = Look::default();
    assert_eq!(
        look,
        Look {
            background: 0,
            ink: 0,
            line: 2,
            line_tone: 0,
            thickness: 4,
            flags: TAPS,
        }
    );
    let set = Settings {
        background: Tone::Yellow.id(),
        ink: Tone::Black.id(),
        line: LineKind::Wavy.id(),
        line_tone: Tone::Red.id(),
        thickness: 99,
        frame: true,
        struck: true,
    };
    let look = Look::from_settings(&set);
    assert_eq!(
        (look.background, look.ink, look.line, look.line_tone),
        (6, 4, 4, 11)
    );
    assert_eq!((look.thickness, look.flags), (20, TAPS | FRAME | STRUCK));
    // A value that is no tone or line is none; a thickness under 1 is 1.
    let odd = Look::from_settings(&Settings {
        background: "x",
        line: "x",
        thickness: -5,
        ..Settings::default()
    });
    assert_eq!((odd.background, odd.line, odd.thickness), (0, 0, 1));
}

#[test]
fn the_codes_are_those_of_the_sdks_tones_and_lines() {
    for (code, tone) in TONES {
        assert_eq!(
            Look::from_settings(&Settings {
                background: tone.id(),
                ..Settings::default()
            })
            .background,
            code
        );
    }
    assert_eq!(
        TONES.len(),
        Tone::ALL.len(),
        "a tone added to the SDK needs a code"
    );
    for (code, kind) in LINES {
        assert_eq!(
            Look::from_settings(&Settings {
                line: kind.id(),
                ..Settings::default()
            })
            .line,
            code
        );
    }
    assert_eq!(LINES.len(), LineKind::ALL.len());
}

#[test]
fn a_tapped_word_loses_its_punctuation_and_keeps_the_language_it_was_kept_in() {
    assert_eq!(tapped_word("(Hatter)."), "Hatter");
    assert_eq!(tapped_word("..."), "");
    let storage = MemoryStorage::new().with(store::file(), WORDS);
    assert_eq!(language_of(&storage, "RABBIT").unwrap(), "en");
    assert_eq!(language_of(&storage, "white  rabbit").unwrap(), "en-GB");
    assert_eq!(language_of(&storage, "nothing").unwrap(), "");
}
