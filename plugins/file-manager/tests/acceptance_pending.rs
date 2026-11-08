//! What needs a host that does not exist yet. Each case is ignored and says
//! what it waits for; none passes by itself.
#![allow(clippy::unwrap_used, reason = "test assertions")]

#[test]
#[ignore = "needs a host that admits menu entries and icons, and core.files with the card (uses.storage) in the kernel"]
fn the_browser_lists_renames_moves_copies_and_deletes_on_the_card() {
    unreachable!("open from Tools, a folder of 1,000 files a page at a time, each change");
}

#[test]
#[ignore = "needs core.library (ops 19-21, books_in, tell_moved): Services/Host work"]
fn a_book_that_moves_keeps_its_reading_place_and_notes() {
    unreachable!("tell_moved on move and rename, books_in before delete, BOOK_REMOVED");
}

#[test]
#[ignore = "needs the readers service (document.md op 21) and core.format (ops 19, 20)"]
fn a_book_opens_from_the_file_manager() {
    unreachable!("Open, Open with, Reading app in the options of a file or folder");
}
