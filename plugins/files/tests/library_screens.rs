//! The library with the real kernel and the standard app, on a device
//! with buttons only and on one with touch only: an empty library, a
//! library of many books (every one reachable on both), books in folders
//! and the folders as collections on the shelves, books with odd names,
//! both orders, the grid of covers, books removed while the list shows,
//! and what a book's sheet writes to the storage.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;

const TEXT: &[u8] = b"Some text of a book.\n\nAnd more of it.\n";

/// What the row above the books says while all books show.
const ALL_BOOKS: &str = "All books";

/// A library of `count` books named `Book 01.txt` and on in `books`.
fn many(count: usize) -> Vec<(String, Vec<u8>)> {
    (1..=count)
        .map(|n| (format!("books/Book {n:02}.txt"), TEXT.to_vec()))
        .collect()
}

fn storage_of(config: &str, files: &[(String, Vec<u8>)]) -> Shared {
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, d)| (p.as_str(), d.as_slice()))
        .collect();
    Shared::memory(config, &files)
}

fn library(device: DeviceInfo, config: &str, files: &[(String, Vec<u8>)]) -> App {
    let mut app = App::new(device, storage_of(config, files));
    app.run(actions::LIBRARY);
    assert_eq!(app.screen(), "files.browser");
    app
}

/// The rows the list shows now, by the first part of their label (a
/// book's title, a shelf's name).
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| first(&p.label))
        .collect()
}

/// The title in a row's label: its first part.
fn first(label: &str) -> String {
    label.split(", ").next().unwrap_or_default().to_string()
}

/// Every row of the list, page by page: with buttons by going down from
/// row to row (right from cover to cover in the grid, where down goes a
/// row of covers), on touch screens by the arrow to the next page.
fn every_row(app: &mut App, touch: bool) -> Vec<String> {
    every_entry(app, touch, actions::DOWN)
}

fn every_entry(app: &mut App, touch: bool, step: Id) -> Vec<String> {
    // From the first book: the first page; with buttons up to the row
    // above the books, then on.
    app.repeat(actions::PREV_PAGE, 20);
    if !touch {
        for _ in 0..200 {
            if app.focused().is_some_and(|f| first(&f) == ALL_BOOKS) {
                break;
            }
            app.run(actions::UP);
        }
        app.run(step.clone());
    }
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..200 {
        if touch {
            let before = seen.len();
            for row in rows(app) {
                if !seen.contains(&row) {
                    seen.push(row);
                }
            }
            if seen.len() == before {
                break;
            }
            app.tap_next_page();
        } else {
            let Some(focused) = app.focused() else {
                break;
            };
            let focused = first(&focused);
            // Past the last book, the row above the books; then the first.
            if seen.first() == Some(&focused) {
                break;
            }
            if focused != ALL_BOOKS && !seen.contains(&focused) {
                seen.push(focused);
            }
            app.run(step.clone());
        }
    }
    seen
}

/// Chooses what the library shows (`option` of the row above the books):
/// with buttons up to that row and confirm, on touch screens a tap on it.
fn show(app: &mut App, option: &str) {
    if app.kernel.device().input.touch {
        app.tap_on(ALL_BOOKS);
    } else {
        for _ in 0..200 {
            if app.focused().is_some_and(|f| first(&f) == ALL_BOOKS) {
                break;
            }
            app.run(actions::UP);
        }
        app.run(actions::CONFIRM);
    }
    assert_eq!(app.screen(), "core.choice-dialog", "{}", app.described());
    choose(app, option);
    assert_eq!(app.screen(), "files.browser", "{}", app.described());
}

/// Opens the row named `label`: by buttons, down to it and confirm; by
/// touch, a tap on it.
fn open(app: &mut App, label: &str) {
    if app.kernel.device().input.touch {
        app.tap_on(label);
        return;
    }
    for _ in 0..200 {
        if app.focused().is_some_and(|f| first(&f) == label) {
            app.run(actions::CONFIRM);
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no row {label}:\n{}", app.described());
}

#[test]
fn an_empty_library_says_where_books_come_from() {
    let _turn = turn();
    for device in DEVICES {
        let mut app = library(device, "", &[]);
        assert!(app.shows("No books here yet"), "{}", app.described());
        assert!(rows(&mut app).is_empty());
        // Buttons on nothing do nothing, and back leaves.
        app.run(actions::DOWN);
        app.run(actions::CONFIRM);
        assert_eq!(app.screen(), "files.browser");
        app.run(actions::BACK);
        assert_eq!(app.screen(), "home.dashboard");
    }
}

/// Used to panic: `Listing::page` underflowed for an empty folder (as the
/// library showed again).
/// A card the device cannot use (exFAT on an X4): no catalog ever comes,
/// so the library does not say it is collecting books for ever.
#[test]
fn a_card_that_cannot_be_read_shows_an_empty_library() {
    let _turn = turn();
    for device in DEVICES {
        let storage = Shared(std::rc::Rc::new(paperpaw_platform::NoStorage));
        let mut app = App::new(device, storage);
        // The hints first.
        if app.screen() == "help.overlay" {
            app.run(actions::CONFIRM);
        }
        app.run(actions::LIBRARY);
        app.wait(3000);
        assert!(!app.shows("Collecting"), "{}", app.described());
        assert!(app.shows("No books here yet"), "{}", app.described());
    }
}

#[test]
fn an_empty_library_shows_again_after_its_menu_closes() {
    let _turn = turn();
    for device in DEVICES {
        let mut app = library(device, "", &[]);
        app.run(actions::MENU);
        assert_eq!(app.screen(), "menu.list");
        app.run(actions::BACK);
        assert_eq!(app.screen(), "files.browser");
        assert!(app.shows("No books here yet"), "{}", app.described());
    }
}

#[test]
fn a_library_folder_that_is_missing_shows_as_empty() {
    let _turn = turn();
    for device in DEVICES {
        let mut app = library(
            device,
            "[core]\nlibrary-folder = nowhere/at/all\n",
            &many(3),
        );
        assert!(app.shows("No books here yet"), "{}", app.described());
        assert!(app.shows("nowhere/at/all"), "{}", app.described());
    }
}

#[test]
fn every_one_of_many_books_is_reached_by_buttons_and_by_touch() {
    let _turn = turn();
    let books = many(57);
    let names: Vec<String> = (1..=57).map(|n| format!("Book {n:02}")).collect();
    for device in DEVICES {
        let touch = device.input.touch;
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &books);
        let seen = every_row(&mut app, touch);
        assert_eq!(seen, names, "touch: {touch}");
    }
}

#[test]
fn page_buttons_turn_whole_pages_and_come_back() {
    let _turn = turn();
    let mut app = library(X4, "[files]\nsort = files.sort.name\n", &many(57));
    let first_page = rows(&mut app);
    assert!(first_page.len() > 2 && first_page.len() < 57);
    app.run(actions::NEXT_PAGE);
    let second = rows(&mut app);
    assert!(second.iter().all(|r| !first_page.contains(r)), "{second:?}");
    // The focus went along: on the second page.
    let focused = first(&app.focused().unwrap());
    assert!(second.contains(&focused), "{focused} in {second:?}");
    app.run(actions::PREV_PAGE);
    assert_eq!(rows(&mut app), first_page);
}

#[test]
fn a_focus_moved_on_a_page_refreshes_its_rows_fast() {
    let _turn = turn();
    // The first press shows the focus, the next ones move it on the page
    // and up to the row above the books: in the list down and back, in
    // the grid along its first row of covers and back.
    let (down, up) = (actions::DOWN, actions::UP);
    let (right, left) = (actions::RIGHT, actions::LEFT);
    let list = [
        down.clone(),
        down.clone(),
        up.clone(),
        up.clone(),
        up.clone(),
    ];
    let grid = [right.clone(), right, left.clone(), left, up];
    // Covers in grays too: a step is no page in grays.
    let gray = "[files]\nview = files.view.covers\ncovers = files.covers.gray\n";
    for (config, moves) in [
        ("", list),
        ("[files]\nview = files.view.covers\n", grid.clone()),
        (gray, grid),
    ] {
        let mut app = library(X4, config, &many(30));
        app.run(down.clone());
        let screen = X4.display;
        let half = u32::from(screen.width) * u32::from(screen.height) / 2;
        for action in moves {
            app.now += 100;
            let effects = app.kernel.run_action(Instant(app.now), action.clone());
            let (area, mode) = effects.refresh().expect("a refresh");
            app.settle(effects);
            // Only what changed, however far apart: not most of the screen
            // and its covers, refreshed in full.
            assert_eq!(mode, paperpaw_sdk::RefreshMode::Fast, "{config:?} {action}");
            let rows = u32::from(area.width) * u32::from(area.height);
            assert!(rows <= half, "{config:?} {action}: {area:?}");
        }
    }
}

#[test]
fn the_grid_of_covers_reaches_every_book_too() {
    let _turn = turn();
    let books = many(23);
    let names: Vec<String> = (1..=23).map(|n| format!("Book {n:02}")).collect();
    for device in DEVICES {
        let touch = device.input.touch;
        let config = "[files]\nsort = files.sort.name\nview = files.view.covers\n";
        let mut app = library(device, config, &books);
        let seen = every_entry(&mut app, touch, actions::RIGHT);
        assert_eq!(seen, names, "touch: {touch}");
        if !touch {
            // Down goes a row of covers: from the first to the fourth.
            assert_eq!(first(&app.focused().unwrap()), "Book 01");
            app.run(actions::DOWN);
            assert_eq!(first(&app.focused().unwrap()), "Book 04");
        }
    }
}

#[test]
fn books_in_folders_are_listed_and_their_folders_are_collections() {
    let _turn = turn();
    let files = vec![
        ("books/Zeta.txt".to_string(), TEXT.to_vec()),
        ("books/Alpha/Beta/Gamma/Deep.txt".to_string(), TEXT.to_vec()),
        ("books/Alpha/Side.txt".to_string(), TEXT.to_vec()),
        ("books/Omega/Other.txt".to_string(), TEXT.to_vec()),
    ];
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &files);
        // Every book, those in folders too, by title.
        assert_eq!(rows(&mut app), ["Deep", "Other", "Side", "Zeta"]);
        // The folders are on the shelves, each with the books below it.
        show(&mut app, "Shelves");
        let shelves = rows(&mut app);
        assert!(shelves.contains(&"Alpha".to_string()), "{shelves:?}");
        assert!(shelves.contains(&"Omega".to_string()), "{shelves:?}");
        open(&mut app, "Alpha");
        assert_eq!(app.screen(), "files.list.shelf");
        assert!(app.shows("Alpha"), "the folder's name as the title");
        assert_eq!(rows(&mut app), ["Deep", "Side"]);
        // Back: the shelves as they were.
        app.run(actions::BACK);
        assert_eq!(app.screen(), "files.browser");
        assert!(rows(&mut app).contains(&"Alpha".to_string()));
        // Back at the library leaves it.
        app.run(actions::BACK);
        assert_eq!(app.screen(), "home.dashboard");
    }
}

#[test]
fn books_with_odd_names_show_and_open() {
    let _turn = turn();
    let long = "L".repeat(180);
    let files = vec![
        ("books/Ünïcödé 日本語 📚.txt".to_string(), TEXT.to_vec()),
        (format!("books/{long}.txt"), TEXT.to_vec()),
        ("books/README".to_string(), TEXT.to_vec()),
        ("books/.hidden.txt".to_string(), TEXT.to_vec()),
        ("books/two.dots.in.name.txt".to_string(), TEXT.to_vec()),
        ("books/UPPER.TXT".to_string(), TEXT.to_vec()),
        ("books/empty.txt".to_string(), Vec::new()),
        ("books/Book.sdr/metadata.lua".to_string(), TEXT.to_vec()),
    ];
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &files);
        let touch = device.input.touch;
        let shown = every_row(&mut app, touch);
        // Back to the first page.
        app.repeat(actions::PREV_PAGE, 5);
        // No extension, hidden files and other readers' folders are no books.
        assert_eq!(shown.len(), 5, "{shown:?}");
        assert!(
            shown.iter().any(|r| r.contains("Ünïcödé 日本語")),
            "{shown:?}"
        );
        assert!(shown.iter().any(|r| r.starts_with("LLLL")), "{shown:?}");
        assert!(!shown.iter().any(|r| r.contains("README")), "{shown:?}");
        assert!(!shown.iter().any(|r| r.contains("hidden")), "{shown:?}");
        assert!(!shown.iter().any(|r| r.contains("sdr")), "{shown:?}");
        // Each opens in the reader and comes back.
        for label in &shown {
            if touch {
                // On the page it is on.
                for _ in 0..5 {
                    if app.find(label).is_some() {
                        break;
                    }
                    app.tap_next_page();
                }
                app.tap_on(label);
            } else {
                // From where the focus is, down to this one.
                while first(&app.focused().unwrap_or_default()) != *label {
                    app.run(actions::DOWN);
                }
                app.run(actions::CONFIRM);
            }
            assert_eq!(app.screen(), "reader.screen", "{label}");
            app.run(actions::BACK);
            assert_eq!(app.screen(), "files.browser", "{label}");
        }
    }
}

#[test]
fn the_order_by_title_ignores_case_and_folders() {
    let _turn = turn();
    let files = vec![
        ("books/banana.txt".to_string(), TEXT.to_vec()),
        ("books/Apple.txt".to_string(), TEXT.to_vec()),
        ("books/cherry.txt".to_string(), TEXT.to_vec()),
        ("books/zfolder/Damson.txt".to_string(), TEXT.to_vec()),
        ("books/Afolder/Elder.txt".to_string(), TEXT.to_vec()),
    ];
    let mut app = library(X4, "[files]\nsort = files.sort.name\n", &files);
    let shown = every_row(&mut app, false);
    let shown: Vec<String> = shown.iter().map(|r| r.to_lowercase()).collect();
    assert_eq!(shown, ["apple", "banana", "cherry", "damson", "elder"]);
}

#[test]
fn recently_read_books_come_first_in_the_order_they_were_read() {
    let _turn = turn();
    let files = many(5);
    for device in DEVICES {
        let touch = device.input.touch;
        let mut app = library(device, "", &files);
        assert_eq!(
            every_row(&mut app, touch),
            ["Book 01", "Book 02", "Book 03", "Book 04", "Book 05"]
        );
        for book in ["books/Book 04.txt", "books/Book 02.txt"] {
            app.open_slot(slots::READER, Params::new().with(params::PATH, book));
            assert_eq!(app.screen(), "reader.screen");
            app.run(actions::BACK);
        }
        assert_eq!(app.screen(), "files.browser");
        assert_eq!(
            every_row(&mut app, touch),
            ["Book 02", "Book 04", "Book 01", "Book 03", "Book 05"]
        );
        // By title: as before.
        app.set("files.sort", "files.sort.name");
        assert_eq!(
            every_row(&mut app, touch),
            ["Book 01", "Book 02", "Book 03", "Book 04", "Book 05"]
        );
    }
}

#[test]
fn the_row_above_the_books_chooses_what_shows_and_the_title_opens_the_menu() {
    let _turn = turn();
    // Buttons: up from the first book reaches the row above the books.
    let mut app = library(X4, "", &many(3));
    app.run(actions::UP);
    assert_eq!(app.focused().as_deref(), Some(ALL_BOOKS));
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "core.choice-dialog");
    app.run(actions::BACK);
    assert_eq!(app.screen(), "files.browser");
    // Down from it, the first book again.
    app.run(actions::DOWN);
    assert_eq!(first(&app.focused().unwrap()), "Book 01");
    // The menu button: the book's sheet; on the row above, the menu.
    app.run(actions::UP);
    app.run(actions::MENU);
    assert_eq!(app.screen(), "menu.list");
    assert!(app.shows("Sort by"), "{}", app.described());

    let mut app = library(TOUCH, "", &many(3));
    app.tap_on("Library");
    assert_eq!(app.screen(), "menu.list");
    app.run(actions::BACK);
    app.tap_on(ALL_BOOKS);
    assert_eq!(app.screen(), "core.choice-dialog");
}

#[test]
fn a_book_removed_while_the_list_shows_goes_from_it() {
    let _turn = turn();
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &many(4));
        assert_eq!(rows(&mut app).len(), 4);
        app.storage.remove("books/Book 02.txt").unwrap();
        // Told: the list as it is now (on the X4 once the catalog was
        // made, away from the list).
        app.send(
            paperpaw_sdk::builtin::events::LIBRARY_CHANGED,
            Params::new(),
        );
        app.let_catalog_work();
        assert_eq!(rows(&mut app), ["Book 01", "Book 03", "Book 04"]);
    }
}

#[test]
fn opening_a_book_removed_unannounced_does_not_break_the_library() {
    let _turn = turn();
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &many(2));
        assert_eq!(rows(&mut app), ["Book 01", "Book 02"]);
        // Gone without anyone telling: the list still shows it.
        app.storage.remove("books/Book 01.txt").unwrap();
        if device.input.touch {
            app.tap_on("Book 01");
        } else {
            app.run(actions::CONFIRM);
        }
        // Not opened: the library says so and lists what is there.
        assert_eq!(app.screen(), "files.browser", "{}", app.described());
        assert_eq!(rows(&mut app), ["Book 02"]);
    }
}

/// Used to panic: `Listing::page` underflowed for an empty folder (as the
/// library kept its focus).
#[test]
fn every_book_of_the_whole_library_folder_removed_leaves_it_empty() {
    let _turn = turn();
    let mut app = library(TOUCH, "", &many(3));
    for n in 1..=3 {
        app.storage
            .remove(&format!("books/Book {n:02}.txt"))
            .unwrap();
    }
    app.send(
        paperpaw_sdk::builtin::events::LIBRARY_CHANGED,
        Params::new(),
    );
    app.let_catalog_work();
    assert!(app.shows("No books here yet"), "{}", app.described());
    // A tap where the rows were opens no book: with nothing there, a tap
    // in the middle opens the menu, as on any screen.
    app.tap(Point::new(240, 300));
    assert_eq!(app.screen(), "menu.list");
}

/// The sheet of the book `title`: held on touch screens, the menu button
/// with buttons only.
fn sheet_of(app: &mut App, title: &str) {
    if app.kernel.device().input.touch {
        app.long_press_on(title);
    } else {
        while first(&app.focused().unwrap_or_default()) != title {
            app.run(actions::DOWN);
        }
        app.run(actions::MENU);
    }
    assert_eq!(app.screen(), "files.list.sheet", "{}", app.described());
}

/// Chooses `option` of the choice or sheet shown: by buttons, down to it
/// and confirm; by touch, a tap on it.
fn choose(app: &mut App, option: &str) {
    if app.kernel.device().input.touch {
        app.tap_on(option);
        return;
    }
    for _ in 0..20 {
        if app
            .focused()
            .is_some_and(|f| f.to_lowercase().starts_with(&option.to_lowercase()))
        {
            app.run(actions::CONFIRM);
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no option {option}:\n{}", app.described());
}

#[test]
fn favourites_are_kept_and_shown_on_their_shelf() {
    let _turn = turn();
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &many(3));
        sheet_of(&mut app, "Book 02");
        choose(&mut app, "Add to favourites");
        assert_eq!(app.screen(), "files.browser");
        assert_eq!(
            paperpaw_sdk::library::favourites(&app.storage),
            ["books/Book 02.txt"]
        );
        app.run(Id::new_static("files.favourites"));
        assert!(app.shows("Book 02"), "{}", app.described());
        assert!(!app.shows("Book 01"), "{}", app.described());
        app.run(actions::BACK);
        // And off again.
        sheet_of(&mut app, "Book 02");
        choose(&mut app, "Remove from favourites");
        assert!(paperpaw_sdk::library::favourites(&app.storage).is_empty());
    }
}

#[test]
fn a_finished_book_moves_to_the_archive_folder_with_its_name_kept_free() {
    let _turn = turn();
    let mut files = many(2);
    // A book of the same name is in the archive already.
    files.push(("books/Archive/Book 01.txt".to_string(), b"old".to_vec()));
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &files);
        // The archive's books are not among the others.
        assert_eq!(rows(&mut app), ["Book 01", "Book 02"]);
        sheet_of(&mut app, "Book 01");
        choose(&mut app, "Mark as finished");
        assert!(app.shows("Finished"), "{}", app.described());
        sheet_of(&mut app, "Book 01");
        choose(&mut app, "Move to the archive");
        assert_eq!(app.screen(), "files.browser");
        assert!(!app.storage.exists("books/Book 01.txt"));
        // The one there before is kept; this one beside it.
        assert_eq!(
            app.storage.text("books/Archive/Book 01.txt").as_deref(),
            Some("old")
        );
        assert_eq!(
            app.storage.text("books/Archive/Book 01 (2).txt"),
            Some(String::from_utf8(TEXT.to_vec()).unwrap())
        );
        // Gone from the list at once; the archive is on the shelves.
        assert_eq!(rows(&mut app), ["Book 02"]);
        show(&mut app, "Shelves");
        open(&mut app, "Archive");
        assert_eq!(rows(&mut app).len(), 2, "{}", app.described());
    }
}

#[test]
fn a_book_in_the_archive_is_not_offered_to_be_archived_again() {
    let _turn = turn();
    let files = vec![("books/Archive/Done.txt".to_string(), TEXT.to_vec())];
    let mut app = library(TOUCH, "", &files);
    // Every book in the archive: the list says where they are.
    assert!(app.shows("archive"), "{}", app.described());
    show(&mut app, "Shelves");
    app.tap_on("Archive");
    sheet_of(&mut app, "Done");
    assert!(!app.shows("Move to the archive"), "{}", app.described());
}

#[test]
fn book_information_opens_from_the_sheet() {
    let _turn = turn();
    for device in DEVICES {
        let mut app = library(device, "", &many(1));
        sheet_of(&mut app, "Book 01");
        choose(&mut app, "Book information");
        assert_eq!(app.screen(), "files.info");
        assert!(app.shows("Book 01"), "{}", app.described());
        app.run(actions::BACK);
        assert_eq!(app.screen(), "files.browser");
    }
}

#[test]
fn searching_finds_books_by_their_title() {
    let _turn = turn();
    for device in DEVICES {
        let files = vec![
            ("books/Moby Dick.txt".to_string(), TEXT.to_vec()),
            ("books/Dracula.txt".to_string(), TEXT.to_vec()),
        ];
        let mut app = library(device, "", &files);
        app.will_type("  moby ");
        app.run(Id::new_static("files.search"));
        assert_eq!(app.screen(), "files.list.shelf", "{}", app.described());
        assert!(app.shows("Moby Dick"), "{}", app.described());
        assert!(!app.shows("Dracula"), "{}", app.described());
    }
}

#[test]
fn an_empty_search_opens_nothing() {
    let _turn = turn();
    let mut app = library(X4, "", &many(1));
    app.will_type("   ");
    app.run(Id::new_static("files.search"));
    assert_eq!(app.screen(), "files.browser");
}

#[test]
fn many_books_in_many_folders_show_at_once_and_their_folders_on_the_shelves() {
    let _turn = turn();
    // 400 books in 20 folders: the first page shows books at once.
    let files: Vec<(String, Vec<u8>)> = (0..400)
        .map(|n| {
            (
                format!("books/Shelf {:02}/Book {n:03}.txt", n % 20),
                TEXT.to_vec(),
            )
        })
        .collect();
    let mut app = library(X4, "[files]\nsort = files.sort.name\n", &files);
    let shown = rows(&mut app);
    assert_eq!(shown.first().map(String::as_str), Some("Book 000"));
    show(&mut app, "Shelves");
    let shelves = every_row(&mut app, false);
    let folders = shelves.iter().filter(|s| s.starts_with("Shelf ")).count();
    assert_eq!(folders, 20, "{shelves:?}");
}

/// Adds a dozen entries to the book menu, as many plugins might.
struct Senders;

impl paperpaw_sdk::Plugin for Senders {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            paperpaw_sdk::Id::new_static("senders"),
            "Senders",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        for n in 1..=12 {
            let id = paperpaw_sdk::Id::parse(&format!("senders.to-{n:02}")).unwrap();
            r.add_action(
                paperpaw_sdk::ActionSpec::new(id.clone(), format!("Send to {n:02}"))
                    .handler(|_: &paperpaw_sdk::Env<'_>, _, _: &mut paperpaw_sdk::Commands| {}),
            )?;
            r.add_menu_entry(
                paperpaw_sdk::MenuEntry::new(paperpaw_sdk::builtin::menus::BOOK, id).order(50),
            )?;
        }
        Ok(())
    }
}

/// With many entries in the book menu the sheet stays on the screen and
/// goes on pages: every entry is reached, by buttons and by touch.
#[test]
fn a_long_book_menu_goes_on_pages() {
    let _turn = turn();
    for device in DEVICES {
        let mut app = App::with(device, storage_of("", &many(1)), Senders);
        app.run(actions::LIBRARY);
        sheet_of(&mut app, "Book 01");
        let height = i64::from(device.display.height);
        for part in app.parts() {
            assert!(
                i64::from(part.area.y) + i64::from(part.area.height) <= height,
                "{:?} below the screen:\n{}",
                part.label,
                app.described()
            );
        }
        assert!(
            !app.shows("Send to 12"),
            "all on one page:\n{}",
            app.described()
        );
        if device.input.touch {
            // The pager's arrow to the next page, right of its number.
            let pager = app
                .parts()
                .into_iter()
                .find(|p| p.label.contains(" / "))
                .unwrap_or_else(|| panic!("no pager:\n{}", app.described()));
            let y = i32::from(pager.area.y) + i32::from(pager.area.height) / 2;
            let x = i32::from(device.display.width) * 3 / 4;
            for _ in 0..5 {
                if app.shows("Send to 12") {
                    break;
                }
                app.tap(Point::new(x, y));
            }
        } else {
            for _ in 0..30 {
                if app.focused().is_some_and(|f| f.starts_with("Send to 12")) {
                    break;
                }
                app.run(actions::DOWN);
            }
        }
        assert!(app.shows("Send to 12"), "{}", app.described());
    }
}

/// Touch and buttons both, as the X4 Pro has them.
const BOTH: DeviceInfo = X4
    .with_id("test-x4-pro")
    .with_name("Test X4 Pro")
    .with_input(paperpaw_platform::InputCaps::new(
        true,
        true,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ));

/// The sheet's question about deleting, asked by choosing *Delete …*;
/// the focus (with buttons) on keeping the book.
fn ask_to_delete(app: &mut App, title: &str) {
    sheet_of(app, title);
    choose(app, "Delete …");
    assert_eq!(app.screen(), "files.list.sheet", "{}", app.described());
    assert!(app.shows("cannot be undone"), "{}", app.described());
    if !app.kernel.device().input.touch {
        assert_eq!(app.focused().as_deref(), Some("Keep it"));
    }
}

#[test]
fn a_book_deleted_from_its_sheet_goes_with_what_is_kept_for_it() {
    let _turn = turn();
    for device in [X4, TOUCH, BOTH] {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &many(3));
        let path = "books/Book 02.txt";
        // What is kept for it: its details, a favourite, its place.
        paperpaw_sdk::library::set_finished(&app.storage, path, true);
        paperpaw_sdk::library::set_favourite(&app.storage, path, true);
        let place = paperpaw_sdk::positions::file(path);
        app.storage.write(&place, b"place").unwrap();
        ask_to_delete(&mut app, "Book 02");
        // Deleted: by buttons up to the answer, by touch a tap on it.
        if app.kernel.device().input.touch {
            app.tap_on("Delete");
        } else {
            app.run(actions::UP);
            assert_eq!(app.focused().as_deref(), Some("Delete"));
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.screen(), "files.browser", "{}", app.described());
        assert!(!app.storage.exists(path));
        assert!(paperpaw_sdk::library::favourites(&app.storage).is_empty());
        assert!(paperpaw_sdk::library::book(&app.storage, path).is_none());
        assert!(!app.storage.exists(&place), "its place is kept");
        // Gone from the list at once, the others there.
        assert_eq!(rows(&mut app), ["Book 01", "Book 03"], "{}", device.id);
    }
}

#[test]
fn a_book_asked_about_is_kept_by_the_answer_or_back() {
    let _turn = turn();
    for device in [X4, TOUCH, BOTH] {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &many(2));
        ask_to_delete(&mut app, "Book 01");
        if app.kernel.device().input.touch {
            app.tap_on("Keep it");
        } else {
            app.run(actions::CONFIRM);
        }
        // The sheet's actions again, the book there.
        assert_eq!(app.screen(), "files.list.sheet");
        assert!(app.shows("Delete …"), "{}", app.described());
        assert!(!app.shows("cannot be undone"), "{}", app.described());
        // Back while asked: kept too, the sheet stays.
        choose(&mut app, "Delete …");
        app.run(actions::BACK);
        assert_eq!(app.screen(), "files.list.sheet");
        assert!(app.shows("Delete …"), "{}", app.described());
        app.run(actions::BACK);
        assert!(app.storage.exists("books/Book 01.txt"));
        assert_eq!(rows(&mut app), ["Book 01", "Book 02"], "{}", device.id);
    }
}

/// The sheet opened for `path`, as the library opens it.
fn sheet_for(app: &mut App, path: &str) {
    app.now += 100;
    let params = Params::new().with("path", path);
    let effects = app.kernel.open_screen(
        paperpaw_sdk::Instant(app.now),
        Id::new_static("files.list.sheet"),
        params,
    );
    app.settle(effects);
    assert_eq!(app.screen(), "files.list.sheet", "{}", app.described());
}

#[test]
fn what_is_not_a_book_is_never_offered_to_be_deleted() {
    let _turn = turn();
    let mut app = library(TOUCH, "", &many(1));
    app.storage.write(".paperpaw/kept.txt", b"kept").unwrap();
    for path in [
        "paperpaw.ini",
        ".paperpaw/kept.txt",
        "books/../paperpaw.ini",
    ] {
        sheet_for(&mut app, path);
        assert!(!app.shows("Delete …"), "{path}: {}", app.described());
        app.run(actions::BACK);
    }
    assert!(app.storage.exists(".paperpaw/kept.txt"));
}

/// A screen too short for all of the sheet's actions: they go on pages.
const SHORT: DeviceInfo = X4.with_display(X4.display.with_height(520));

#[test]
fn on_a_sheet_of_pages_the_answers_sit_at_its_bottom_edge() {
    let _turn = turn();
    let mut app = library(SHORT, "", &many(1));
    sheet_of(&mut app, "Book 01");
    // To *Delete …*, on the sheet's second page.
    choose(&mut app, "Delete …");
    assert!(app.shows("cannot be undone"), "{}", app.described());
    let parts = app.parts();
    let keep = parts
        .iter()
        .find(|p| p.label == "Keep it")
        .expect("Keep it");
    let delete = parts.iter().find(|p| p.label == "Delete").expect("Delete");
    let bottom = u32::from(keep.area.y) + u32::from(keep.area.height);
    // At the sheet's bottom edge (half a gap above the screen's), no
    // pager's strip left empty below them.
    assert!(bottom + 12 >= 520, "{:?}", keep.area);
    assert!(delete.area.y < keep.area.y);
}
