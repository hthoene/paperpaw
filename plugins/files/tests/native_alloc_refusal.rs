//! Every allocation of the model's rows, scopes and texts may be refused:
//! each ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_files::{
    native_cells::{self, Facts},
    native_choose, native_covers,
    native_info::{self, Info},
    native_list::{self, BookList, Metrics, Row},
    native_model::{self, BookLine, Filter, Order, Say, Scope, Selection, Window},
    native_sheet::{self, Action},
    native_shelves::{self, Begun, NameLine, SeriesNames, SeriesSource, Shelf},
    native_states::Condition,
};
use paperpaw_sdk::{
    Rect,
    native::{
        Error,
        image::ImageId,
        library::{CatalogCursor, CatalogPhase},
    },
};

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
    fn with(&mut self, key: &str, _: &str, value: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::concat(&[key, value])
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

#[test]
fn the_cells_the_covers_the_sheet_and_the_chooser_survive_refused_allocations() {
    // The fixtures are built here, outside the refused runs.
    let books = vec![
        book(false, Some(40), false),
        book(false, None, true),
        book(true, Some(100), false),
    ];
    let mut window: Window<CatalogCursor, BookLine> = Window::new(CatalogCursor::start());
    window.total = 3;
    window.books = books;
    window.next = Some(CatalogCursor::start());
    let empty: Window<CatalogCursor, BookLine> = Window::new(CatalogCursor::start());
    let selection = Selection::default();
    let search = Selection {
        scope: Scope::Search("x".into()),
        filter: Filter::All,
        order: Order::Name,
    };
    let later = [CatalogCursor::start()];
    let info = Info {
        title: "T".into(),
        author: "A".into(),
        rows: vec![("Folder".into(), "books".into())],
        cover: Some((1, 1)),
    };
    let metrics = Metrics {
        area: Rect::new(0, 50, 480, 400),
        row: 40,
        gap: 8,
        margin: 12,
        columns: 2,
    };
    let kept = [ImageId::new(1).unwrap()];
    let lone = String::from("books/Crime/a.epub");
    let words = ["Open".to_string(), "Delete".to_string()];
    let scanning = Condition::Scanning {
        books: 9,
        ppm: 300_000,
    };
    fail_alloc::sweep(|| {
        for (selection, window, starts, condition) in [
            (&selection, &window, &[][..], Condition::Ready),
            (&selection, &window, &later[..], scanning),
            (&selection, &empty, &[][..], Condition::Preparing),
            (&selection, &empty, &[][..], Condition::CardGone),
            (&search, &empty, &[][..], Condition::Ready),
        ] {
            let facts = Facts {
                selection,
                window,
                starts,
                condition,
                archived: true,
                folder: "books",
            };
            let built = native_cells::build(&mut Words, &facts)?;
            for grid in [false, true] {
                native_list::pages(&built.cells, &metrics, grid)?;
            }
            let mut list = BookList::new(built.cells, false);
            list.set_focus(1);
            list.visible_books()?;
        }
        native_cells::build(
            &mut Words,
            &Facts {
                selection: &selection,
                window: &empty,
                starts: &[],
                condition: Condition::Ready,
                archived: false,
                folder: "books",
            },
        )?;
        native_list::Cell::plain(Row::Note, "a", "b")?;
        native_info::cells(&info)?;
        Condition::of(Ok((CatalogPhase::Walking, 3, 100_000)), true)?;
        Condition::of(Err(Error::Io), false)?;
        let mut covers = native_covers::Covers::new(8, 3)?;
        covers.set(1, native_covers::Slot::Ready(kept[0]));
        covers.drop_all(|_| {});
        for finished in [false, true] {
            let actions = native_sheet::actions(native_sheet::Facts {
                finished,
                can_archive: true,
                can_delete: true,
            })?;
            assert!(actions.contains(&Action::Open));
        }
        for attempt in 0..3 {
            native_sheet::archive_path("books/Archive", &lone, attempt)?;
        }
        native_sheet::choices(&words)?;
        native_choose::picks("books", 40)?;
        native_choose::join("books", "Crime")?;
        native_choose::join("", "Crime")?;
        native_choose::folder_name("  Sci/Fi: *new*? ")?;
        native_choose::folder_name("...")?;
        Ok(())
    });
}
