//! Every allocation of the model's rows, scopes and texts may be refused:
//! each ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_files::{
    native_info,
    native_model::{self, BookLine, Say, Scope},
    native_shelves::{self, Begun, NameLine, SeriesNames, SeriesSource, Shelf},
};
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

struct Words;
impl Say for Words {
    fn say(&mut self, key: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(key)
    }
    fn count(&mut self, key: &str, _: i64) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(key)
    }
}

/// Two series, each with a next book, read a name at a time.
struct Two(BookLine);
impl SeriesSource<u32> for Two {
    fn names(&mut self, _: Begun, from: u32) -> Result<SeriesNames<u32>, Error> {
        Ok(SeriesNames {
            names: paperpaw_ui_kit::mem::collect([
                paperpaw_ui_kit::text::copy("Saga"),
                paperpaw_ui_kit::text::copy("Tales"),
            ])?,
            next: (from == 0).then_some(1),
        })
    }
    fn next_of(&mut self, _: Begun, _: &str) -> Result<Option<BookLine>, Error> {
        // The fixture is the test's own allocation; the title is the code's.
        let book = fail_alloc::exempt(|| self.0.clone());
        Ok(Some(native_model::BookLine {
            title: paperpaw_ui_kit::text::copy("Next")?,
            ..book
        }))
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
            ("next", ""),
            ("nonsense", ""),
        ] {
            Scope::from_params(kind, value)?;
        }
        Ok(())
    });
}

#[test]
fn the_shelves_survive_refused_allocations() {
    let names = [
        NameLine {
            text: "Ann".into(),
            cut: true,
            count: 2,
        },
        NameLine {
            text: String::new(),
            cut: false,
            count: 1,
        },
        NameLine {
            text: "books/Crime/".into(),
            cut: false,
            count: 3,
        },
    ];
    let mut source = Two(book(false, None, false));
    let long = "x".repeat(300);
    fail_alloc::sweep(|| {
        for shelf in Shelf::ALL {
            for name in &names {
                native_shelves::name_row(&mut Words, shelf, name)?;
            }
            shelf.scope("a name")?;
        }
        native_shelves::collect_next(&mut source, 0, 10)?;
        native_info::size_text(2_400_000, ",")?;
        native_info::upper("epub")?;
        native_info::ellipsed("a value")?;
        native_info::ellipsed(&long)?;
        Ok(())
    });
}
