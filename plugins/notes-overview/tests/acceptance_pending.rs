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
#[ignore = "needs core.files (PR #339) for the part file and the rename over the old file, and op 4 for the cut values (1,024 B parts)"]
fn the_export_is_written_to_a_part_file_and_replaces_the_old_one_when_whole() {
    unreachable!(
        "Exports/My Clippings.txt.part appended in 6 KiB pieces, renamed when whole, removed on a refusal"
    );
}

#[test]
#[ignore = "needs a native screen host (core.ui compose, Input H3) and the draft-22 doc_open"]
fn choosing_a_note_opens_the_book_at_its_place() {
    unreachable!("the book first, then the goto event to the note's start");
}

#[test]
#[ignore = "needs a real host with thousands of books: the scan is bounded in memory (native_scan_tests), its time is a device measurement"]
fn a_library_of_thousands_of_books_costs_time_not_memory() {
    unreachable!("books pass one at a time; only the 200 listed books are held");
}
