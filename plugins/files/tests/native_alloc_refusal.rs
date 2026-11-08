//! Every allocation of the model's rows, scopes and texts may be refused:
//! each ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_files::native_model::{self, BookLine, Say, Scope};
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

struct Words;
impl Say for Words {
    fn say(&mut self, key: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(key)
    }
}

fn book(finished: bool, progress: Option<u8>, new: bool) -> BookLine {
    BookLine {
        key: 1,
        title: "Title".into(),
        author: "Author".into(),
        series: "Series".into(),
        number: "2".into(),
        progress,
        finished,
        favourite: false,
        is_new: new,
        badges: vec!["Badge".into()],
    }
}

#[test]
fn rows_and_scopes_survive_refused_allocations() {
    // The fixtures are built here, outside the refused runs.
    let books = [
        book(true, Some(100), false),
        book(false, Some(45), false),
        book(false, None, true),
        book(false, None, false),
    ];
    fail_alloc::sweep(|| {
        for book in &books {
            native_model::line(&mut Words, book)?;
        }
        for (kind, value) in [
            ("favourites", ""),
            ("author", "Ann"),
            ("series", "Saga"),
            ("tag", "sf"),
            ("folder", "books"),
            ("search", "two words"),
            ("nonsense", ""),
        ] {
            Scope::from_params(kind, value)?;
        }
        Ok(())
    });
}
