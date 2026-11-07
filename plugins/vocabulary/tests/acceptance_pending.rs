//! What the interface-6 integration tests (`collecting.rs`, `cards.rs`, `widgets.rs`) prove
//! with a whole kernel and the plugin in it. The words' and the cards' data layers are
//! tested in `src/store.rs`, `src/store/format_tests.rs`, `src/lines.rs`, `src/files.rs` and
//! `src/alloc_refusal_tests.rs`; these need host services or ported parts that are not there
//! yet and stay ignored. Each names the missing piece; none passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

/// The word looked up is kept and listed, newest first.
const COLLECT: Case = Case {
    name: "vocabulary-collect-and-list",
    golden: None,
    blocked_by: &["native shell", "H1", "H4"],
};
/// The screens: word list, choice dialogs, flash cards, decks, card files.
const SCREENS: Case = Case {
    name: "vocabulary-screens",
    golden: None,
    blocked_by: &["H1", "H2", "H3", "H4"],
};
/// A tapped word is looked up again.
const LOOKUP: Case = Case {
    name: "vocabulary-lookup-handoff",
    golden: None,
    blocked_by: &["core.lookup #352 (draft 23)"],
};
/// The widgets: a word, the cards due.
const WIDGETS: Case = Case {
    name: "vocabulary-widgets",
    golden: None,
    blocked_by: &["widget host call (Core todo 76)", "widget toolkit #366"],
};
/// The words marked in books of their language.
const MARKS: Case = Case {
    name: "vocabulary-marks",
    golden: None,
    blocked_by: &["decoration stage"],
};
/// The phone page that edits decks and cards.
const PHONE: Case = Case {
    name: "vocabulary-phone-page",
    golden: None,
    blocked_by: &[
        "core.net #351",
        "core.sockets",
        "secrets (Services todo 79)",
    ],
};
/// The card folder's files update the decks.
const CARD_FILES: Case = Case {
    name: "vocabulary-card-files",
    golden: None,
    blocked_by: &["core.files #339"],
};
/// A flash card due is the sleep screen; the review opens on waking.
const SLEEP: Case = Case {
    name: "vocabulary-sleep-card",
    golden: None,
    blocked_by: &["sleep and wake stages"],
};

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in [
        &COLLECT,
        &SCREENS,
        &LOOKUP,
        &WIDGETS,
        &MARKS,
        &PHONE,
        &CARD_FILES,
        &SLEEP,
    ] {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.name), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED native shell/H1/H4: word.looked-up event, store::add, the list screen pages store::range"]
fn a_word_looked_up_is_kept_and_listed_newest_first() {
    COLLECT.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H3/H4: native screen commands, dialogs, composition and input are not qualified"]
fn the_screens_list_words_delete_and_export() {
    SCREENS.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #352: core.lookup (draft 23) for the dictionary hand-off of a tapped word"]
fn a_tapped_word_is_looked_up_again() {
    LOOKUP.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED Core todo 76 and #366: the widget host call and the widget toolkit"]
fn the_widgets_show_a_word_and_the_cards_due() {
    WIDGETS.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED decorations: the decoration stage of interface 7 (marks.rs)"]
fn the_words_are_marked_in_books_of_their_language() {
    MARKS.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #351/core.sockets/Services todo 79: the phone page (phone.rs)"]
fn the_phone_page_edits_decks_and_cards() {
    PHONE.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #339: core.files for the card folder (sdcard.rs, sync.rs) and the Notes export folder"]
fn card_files_on_the_card_update_the_decks() {
    CARD_FILES.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED sleep/wake stages: the front of the next card asleep, answered on waking (sleep.rs)"]
fn a_flash_card_due_is_the_sleep_screen_and_review_opens_on_waking() {
    SLEEP.refuse().unwrap();
}
