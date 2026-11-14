//! The interface-7 rules: the last book read from the state file as version 1
//! wrote it, the state written and read back, sentences read aloud and the
//! timer's minutes, and every allocation refused in turn. (They were compared
//! with the interface-6 code on the same inputs while it existed, #244.)
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::MemoryStorage;
use proptest::prelude::*;

use crate::fail_alloc::sweep;
use crate::{native_aloud, native_state};

const STATES: [&str; 9] = [
    include_str!("../testdata/state.ini.v1"),
    "",
    "[reader]\nlast = books/a.epub\n",
    "[reader]\nlast = a\nlast = b\n",
    "[other]\nlast = x\n[reader]\nlast = \"q \\\"uoted\\\" \\u00e4\\n\"\n",
    "last = top\n",
    "[reader]\nlast = \"unterminated\n",
    "[reader]\n=\nlast\nlast = \n",
    "[reader]\nlast = \"\"\n",
];

#[test]
fn the_last_book_is_read_as_before() {
    // As the interface-6 reader read them (pinned from it, #244).
    let expected: [Option<&str>; 3] = [
        Some("books/\"Quoted\" title.epub"),
        None,
        Some("books/a.epub"),
    ];
    for (state, expected) in STATES.iter().zip(expected) {
        assert_eq!(
            native_state::last_in(state).unwrap().as_deref(),
            expected,
            "{state:?}"
        );
    }
    // The last of two lines wins; a key outside the section is not the book.
    assert_eq!(
        native_state::last_in(STATES[3]).unwrap().as_deref(),
        Some("b")
    );
    assert_eq!(native_state::last_in(STATES[5]).unwrap(), None);
}

#[test]
fn the_state_is_written_as_before() {
    for path in [
        "books/a.epub",
        "",
        " lead",
        "trail ",
        "\"quote",
        "a\"b\\c\td\ne\rf",
        "ünï/cödé.epub",
    ] {
        let old = format!("[reader]\nlast = {}\n", paperpaw_sdk::config::quote(path));
        assert_eq!(native_state::state_text(path).unwrap(), old, "{path:?}");
        // And it reads back as the path (a carriage return is dropped).
        let back = native_state::last_in(&old).unwrap().unwrap();
        assert_eq!(back, path.replace('\r', ""), "{path:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn any_state_text_is_read_without_panic(state in ".{0,200}") {
        std::hint::black_box(native_state::last_in(&state).ok());
    }

    #[test]
    fn any_path_is_written_and_read_back(path in ".{0,120}") {
        let text = native_state::state_text(&path).unwrap();
        prop_assert_eq!(
            text.clone(),
            format!("[reader]\nlast = {}\n", paperpaw_sdk::config::quote(&path))
        );
        let back = native_state::last_in(&text).unwrap().unwrap_or_default();
        prop_assert_eq!(back, path.replace('\r', ""));
    }

    #[test]
    fn a_sentence_ends_at_its_first_end(
        words in proptest::collection::vec("[a-zA-Z]{1,6}[.!?…,;:\"”’)\\]-]{0,2}", 1..200),
        from in 0usize..220,
    ) {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let from = from % words.len();
        let end = native_aloud::sentence_end(&words, from);
        prop_assert!(end > from && end <= words.len());
        prop_assert!(end - from <= native_aloud::MAX_WORDS);
        // No end of a sentence before its last word.
        prop_assert!(words[from..end - 1].iter().all(|w| !native_aloud::ends_sentence(w)));
        // It ends with one, or at the most words, or at the last word.
        prop_assert!(
            native_aloud::ends_sentence(words[end - 1])
                || end - from == native_aloud::MAX_WORDS
                || end == words.len()
        );
    }

    #[test]
    fn the_text_spoken_is_the_words_in_order(
        words in proptest::collection::vec("[a-zA-Z-]{1,6}", 0..60),
        lines in proptest::collection::vec(any::<bool>(), 60),
    ) {
        let parts: Vec<(&str, bool)> =
            words.iter().zip(&lines).map(|(w, &l)| (w.as_str(), l)).collect();
        let text = native_aloud::spoken(parts.iter().copied()).unwrap();
        // Joined again where a hyphen split a word, else with spaces: the
        // same letters in the same order.
        let letters = |s: &str| s.chars().filter(|c| !matches!(c, ' ' | '-')).collect::<String>();
        let all: String = words.iter().map(|w| letters(w)).collect();
        prop_assert_eq!(letters(&text), all);
        prop_assert!(!text.starts_with(' ') && !text.ends_with(' '));
        prop_assert!(!text.contains("  "));
    }
}

#[test]
fn a_sentence_without_an_end_stops_at_the_most_words() {
    let many: Vec<&str> = core::iter::repeat_n("word", 200).collect();
    assert_eq!(
        native_aloud::sentence_end(&many, 0),
        native_aloud::MAX_WORDS
    );
    assert_eq!(native_aloud::sentence_end(&many, 150), 200);
}

#[test]
fn nothing_is_spoken_of_nothing() {
    assert_eq!(native_aloud::spoken(core::iter::empty()).unwrap(), "");
    assert_eq!(
        native_aloud::spoken([("-", true), ("a", true)].into_iter()).unwrap(),
        "- a"
    );
}

#[test]
fn the_timer_choices_give_their_minutes() {
    let choices = [
        "reader.aloud-timer.off",
        "reader.aloud-timer.15",
        "reader.aloud-timer.30",
        "reader.aloud-timer.60",
        "reader.aloud-timer.90",
    ];
    let minutes: Vec<_> = choices
        .iter()
        .map(|c| native_aloud::timer_minutes(c))
        .collect();
    assert_eq!(minutes, [None, Some(15), Some(30), Some(60), Some(90)]);
    assert_eq!(native_aloud::timer_minutes(""), None);
    assert_eq!(native_aloud::timer_minutes("0"), None);
    assert_eq!(native_aloud::timer_minutes("reader.aloud-timer.x"), None);
}

#[test]
fn the_state_file_of_version_1_is_kept_in_the_data_folder() {
    // The file keeps its name and form: written by one, read by the other.
    let storage = MemoryStorage::new();
    let text = native_state::state_text("books/\"Quoted\" title.epub").unwrap();
    paperpaw_sdk::Storage::write(&storage, native_state::STATE, text.as_bytes()).unwrap();
    let kept =
        paperpaw_sdk::read_file(&storage, native_state::STATE, native_state::MAX_STATE).unwrap();
    assert_eq!(
        native_state::last_in(core::str::from_utf8(&kept).unwrap())
            .unwrap()
            .as_deref(),
        Some("books/\"Quoted\" title.epub")
    );
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    for state in STATES {
        sweep(|| native_state::last_in(state).map(|path| path.map(|p| p.len())));
    }
    for path in ["books/a.epub", " lead", "a\"b\\c\td\ne"] {
        sweep(|| native_state::state_text(path).map(|t| t.len()));
    }
    let words = [
        ("The", true),
        ("fox", false),
        ("jum-", false),
        ("ped", true),
        ("over", false),
    ];
    sweep(|| native_aloud::spoken(words.into_iter()).map(|t| t.len()));
}
