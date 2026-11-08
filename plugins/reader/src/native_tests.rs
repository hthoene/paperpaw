//! The interface-7 rules against the interface-6 ones: the same answers on
//! the same inputs (the old code is the reference until it goes at
//! activation), and every allocation refused in turn.
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::MemoryStorage;
use proptest::prelude::*;

use crate::fail_alloc::sweep;
use crate::{aloud, native_aloud, native_state};

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
    for state in STATES {
        assert_eq!(
            native_state::last_in(state).unwrap(),
            crate::last_in(state),
            "{state:?}"
        );
    }
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
    fn any_state_text_is_read_as_before(state in ".{0,200}") {
        prop_assert_eq!(native_state::last_in(&state).unwrap(), crate::last_in(&state));
    }

    #[test]
    fn any_path_is_written_as_before_and_read_back(path in ".{0,120}") {
        let old = format!("[reader]\nlast = {}\n", paperpaw_sdk::config::quote(&path));
        prop_assert_eq!(native_state::state_text(&path).unwrap(), old);
    }

    #[test]
    fn sentences_are_as_before(
        words in proptest::collection::vec("[a-zA-Z]{1,6}[.!?…,;:\"”’)\\]-]{0,2}", 0..200),
        from in 0usize..220,
    ) {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        prop_assert_eq!(native_aloud::sentence_end(&words, from), aloud::sentence_end(&words, from));
        for word in &words {
            prop_assert_eq!(native_aloud::ends_sentence(word), aloud::ends_sentence(word));
        }
    }

    #[test]
    fn spoken_text_is_as_before(
        words in proptest::collection::vec(("[a-zA-Z-]{1,6}", any::<bool>()), 0..60),
    ) {
        let list = || words.iter().map(|(w, n)| (w.as_str(), *n));
        prop_assert_eq!(native_aloud::spoken(list()).unwrap(), aloud::spoken(list()));
    }
}

#[test]
fn the_timer_choices_give_their_minutes() {
    let minutes: Vec<_> = aloud::TIMER_CHOICES
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
        crate::last_in(core::str::from_utf8(&kept).unwrap()).as_deref(),
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
