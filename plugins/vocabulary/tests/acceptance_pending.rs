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
    blocked_by: &[
        "reader emitter of reader.word-looked-up (Plugin Engineer)",
        "scripted host with stage calls",
    ],
};
/// The screens on a host. (The flash cards' decks and review, the list of words and the card files'
/// screens are ported; what they draw is proved by their tests of the rows and the decisions, not on a
/// host.)
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
    blocked_by: &[
        "a scripted host that measures and draws a widget with fonts and the cards (hosted construction, #944)",
    ],
};
/// The words marked in books of their language.
const MARKS: Case = Case {
    name: "vocabulary-marks",
    golden: None,
    blocked_by: &["reader decorations (reader.decorations@1)"],
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
    blocked_by: &["scripted host with core.ui, core.files and core.work (hosted construction)"],
};
/// A word of the list is gone to in its book, and the list is exported.
const WORD_LIST_ACTIONS: Case = Case {
    name: "vocabulary-word-list-go-to-and-export",
    golden: None,
    blocked_by: &[
        "a scripted host that draws the list and drives its input and dialogs (hosted construction, #944)",
    ],
};
/// A flash card due is the sleep screen; the review opens on waking.
const SLEEP: Case = Case {
    name: "vocabulary-sleep-card",
    golden: None,
    blocked_by: &[
        "a scripted host that shows the sleep slot and wakes (hosted construction, #944)",
    ],
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
        &WORD_LIST_ACTIONS,
        &SLEEP,
    ] {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.name), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED a scripted host that runs the reader's stage reader.word-looked-up@1 (hosted construction) and the reader's emitter (Plugin Engineer): keeping the word is native_collect (tested), the list pages store::range"]
fn a_word_looked_up_is_kept_and_listed_newest_first() {
    COLLECT.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H3/H4: native screen commands, dialogs, composition and input are not qualified"]
fn the_card_files_screens_work() {
    SCREENS.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #352: core.lookup (draft 23) for the dictionary hand-off of a tapped word"]
fn a_tapped_word_is_looked_up_again() {
    LOOKUP.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED a scripted host that measures and draws a widget (hosted construction #944): the widgets are native (native_widgets.rs); made, dropped and declared are tested in tests/scripted_widgets.rs"]
fn the_widgets_show_a_word_and_the_cards_due() {
    WIDGETS.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED reader decorations: the reader's stage reader.decorations@1 (marks.rs keeps the logic ready)"]
fn the_words_are_marked_in_books_of_their_language() {
    MARKS.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED #351/core.sockets/Services todo 79: the phone page (phone.rs)"]
fn the_phone_page_edits_decks_and_cards() {
    PHONE.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED a scripted host with core.ui, core.files and core.work (hosted construction): the card files' screens are native (native_sd*.rs) and their model is tested in native_sd_tests.rs"]
fn card_files_on_the_card_update_the_decks() {
    CARD_FILES.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED a scripted host that drives the list screen, its dialogs and core.files (hosted construction #944): Export and Go to are native (native_export.rs, native_goto.rs); the rows and the export format are tested"]
fn a_word_is_gone_to_in_its_book_and_the_list_is_exported() {
    WORD_LIST_ACTIONS.refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED a scripted host that shows the sleep slot and wakes (hosted construction #944): the pick of the card is native_sleep_card (tested), the offer is declared (tests/declarations.rs)"]
fn a_flash_card_due_is_the_sleep_screen_and_review_opens_on_waking() {
    SLEEP.refuse().unwrap();
}
