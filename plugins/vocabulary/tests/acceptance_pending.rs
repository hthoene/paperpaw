//! What the interface-6 integration tests (`collecting.rs`, `cards.rs`, `widgets.rs`) prove
//! with a whole kernel and the plugin in it. The words' data layer is tested in
//! `src/store.rs`, `src/store/format_tests.rs`, `src/lines.rs`, `src/files.rs` and
//! `src/alloc_refusal_tests.rs`; these need host services or ported parts that are not there
//! yet and stay ignored. Each names the missing piece.

#[test]
#[ignore = "needs the native shell (Plugin impl, declarations) and a host that runs native plugins"]
fn a_word_looked_up_is_kept_and_listed_newest_first() {
    unreachable!("word.looked-up event; store::add; the list screen pages store::range");
}

#[test]
#[ignore = "needs core.ui screens on interface 7 (word list, choice dialogs, flash cards, decks, card files)"]
fn the_screens_list_words_delete_and_export() {
    unreachable!("tap looks up again, hold offers go-to/delete; Export writes into Notes");
}

#[test]
#[ignore = "needs core.lookup (#352, draft 23) for the dictionary hand-off of a tapped word"]
fn a_tapped_word_is_looked_up_again() {
    unreachable!("open the lookup provider with the word and the language it was kept in");
}

#[test]
#[ignore = "needs the widget host call (Core todo 76) and the widget toolkit of Port A (#366)"]
fn the_widgets_show_a_word_and_the_cards_due() {
    unreachable!("word of the day / newest widget; cards-due widget");
}

#[test]
#[ignore = "needs the decoration stage of interface 7 for marks in books (words.rs marks.rs)"]
fn the_words_are_marked_in_books_of_their_language() {
    unreachable!("decorator role vocabulary.learning; a tap on a mark looks it up");
}

#[test]
#[ignore = "needs core.net (#351) and secrets (Services todo 79) for the phone page, and core.sockets"]
fn the_phone_page_edits_decks_and_cards() {
    unreachable!("a page served on the local network while the screen is open (phone.rs)");
}

#[test]
#[ignore = "needs core.files (#339) for the card folder sync (listing the folder) and the Notes export folder"]
fn card_files_on_the_card_update_the_decks() {
    unreachable!("sdcard.rs / sync.rs: scan the folder, import csv/tsv/apkg, ledger");
}

#[test]
#[ignore = "needs the sleep-screen and wake stages of interface 7"]
fn a_flash_card_due_is_the_sleep_screen_and_review_opens_on_waking() {
    unreachable!("sleep.rs: the front of the next card; answered on waking");
}
