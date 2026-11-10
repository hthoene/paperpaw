//! What needs a host that does not exist yet. Each case is ignored and says
//! what it waits for; none passes by itself.
#![allow(clippy::unwrap_used, reason = "test assertions")]

#[test]
#[ignore = "needs a host that admits menu entries and icons, and core.files with the card (uses.storage) in the kernel"]
fn the_browser_lists_renames_moves_copies_and_deletes_on_the_card() {
    unreachable!("open from Tools, a folder of 1,000 files a page at a time, each change");
}

#[test]
#[ignore = "needs the activation of native plugins (#944, #417) and the host's core.library and core.document"]
fn a_book_opens_and_is_deleted_from_the_file_manager() {
    unreachable!("Open, Open with and Delete (through the library) in the options of a book");
}

#[test]
#[ignore = "needs the host's journalled moves (files.md 2.6) behind core.files"]
fn a_book_that_moves_keeps_its_reading_place_and_notes() {
    unreachable!("a move and a rename on the card, the host follows them");
}
