//! What `tests/overview.rs` proves on interface 6 with a whole kernel. The
//! decisions that need no host (list order and cap, counts, the text of the
//! export) are tested in `src/model/tests.rs`; these need host services that
//! are not there yet and stay ignored until they are. Each names what is missing.

#[test]
#[ignore = "the library part is native now (tests/scripted_library.rs); the rows' texts and pixels need the catalog of messages in the scripted host and the screen host (H1/H3)"]
fn the_books_with_notes_are_listed_with_their_counts() {
    unreachable!("one catalog pass, one first-page count per book, no per-book note files read");
}

#[test]
#[ignore = "the walk and the entries are tested (src/native_export_tests.rs); the staging in the data folder and the move onto the card with replace need a host with storage and core.files (the scripted host has neither)"]
fn the_export_is_written_to_a_part_file_and_replaces_the_old_one_when_whole() {
    unreachable!(
        "export.part staged in 4 KiB pieces, moved to Exports/My Clippings.txt with replace when whole, removed on a refusal"
    );
}

#[test]
#[ignore = "needs the reader of interface 7 to read the params `section` and `offset` of its handover (docs/abi/document.md, Opening at a place) and a screen host for the choosing"]
fn choosing_a_note_opens_the_book_at_its_place() {
    unreachable!("command.open into core.reader with path, section and offset of the note");
}

#[test]
#[ignore = "needs a real host with thousands of books: the scan is bounded in memory (native_scan_tests), its time is a device measurement"]
fn a_library_of_thousands_of_books_costs_time_not_memory() {
    unreachable!("books pass one at a time; only the 200 listed books are held");
}
