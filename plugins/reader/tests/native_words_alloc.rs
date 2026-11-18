//! Every allocation of the stage input of a lookup may be refused: each ends
//! in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_reader::native_words::{self, Looked};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn the_input_of_a_lookup_survives_refused_allocations() {
    let long = "é".repeat(1_000);
    fail_alloc::sweep(|| {
        for (word, path) in [("serendipity", "Books/a.epub"), (long.as_str(), "b.epub")] {
            native_words::announce(
                &Looked {
                    word,
                    sentence: "A sentence.",
                    language: "en",
                    path,
                    title: "Title",
                    section: 3,
                    offset: 40,
                },
                |_| Ok(()),
            )?;
        }
        Ok(())
    });
}
