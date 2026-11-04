//! Covers read when a book first shows (#98, #398): the library's walk
//! reads a new book's details alone, and the list reads the covers of the
//! books it shows, one a tick, so input comes between.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::library;

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
const KAFKA: &[u8] = include_bytes!("../../../samples/library/books/die-verwandlung.epub");

/// The covers the EPUB format decoded, on the whole storage.
fn covers(storage: &Shared) -> Vec<String> {
    let mut found = Vec::new();
    let mut dirs = vec![String::from(".paperpaw/plugins/format-epub")];
    while let Some(dir) = dirs.pop() {
        let _ = storage.list(&dir, &mut |e| {
            let path = format!("{dir}/{}", e.name);
            if e.is_dir {
                dirs.push(path);
            } else if e.name.starts_with("cover-") {
                found.push(path);
            }
        });
    }
    found
}

/// The app at home with `files`, the library walked (as a view asks,
/// none showing its books).
fn walked(device: DeviceInfo, files: &[(&str, &[u8])]) -> App {
    let mut app = App::new(device, Shared::memory("", files));
    let paperpaw_sdk::Command::Emit(event, params) = library::request_catalog() else {
        unreachable!("an event");
    };
    app.send(event, params);
    assert!(
        matches!(
            library::catalog::state(&app.storage),
            library::catalog::State::Ready { .. }
        ),
        "{}",
        app.described()
    );
    app
}

#[test]
fn the_walk_reads_no_cover_and_the_list_those_it_shows() {
    let _turn = turn();
    let mut app = walked(
        X4,
        &[("books/alice.epub", ALICE), ("books/kafka.epub", KAFKA)],
    );
    assert!(matches!(
        library::catalog::state(&app.storage),
        library::catalog::State::Ready { .. }
    ));
    assert_eq!(covers(&app.storage), Vec::<String>::new());
    let alice = library::book(&app.storage, "books/alice.epub").unwrap();
    assert!(alice.title.is_some_and(|t| t.contains("Alice")));
    assert!(!alice.indexed && alice.cover.is_none());
    // Shown: their covers read, kept by the library.
    app.run(actions::LIBRARY);
    app.let_catalog_work();
    assert_eq!(app.screen(), "files.browser");
    assert_eq!(covers(&app.storage).len(), 2, "{}", app.described());
    for path in ["books/alice.epub", "books/kafka.epub"] {
        let book = library::book(&app.storage, path).unwrap();
        assert!(book.indexed, "{path}");
        let cover = book.cover.unwrap();
        assert!(app.storage.exists(&cover.path), "{path}");
    }
}

#[test]
fn the_list_reads_one_cover_a_tick() {
    let _turn = turn();
    let mut app = walked(
        X4,
        &[("books/alice.epub", ALICE), ("books/kafka.epub", KAFKA)],
    );
    assert!(covers(&app.storage).is_empty());
    app.now += 100;
    let mut effects = app.kernel.run_action(Instant(app.now), actions::LIBRARY);
    let mut most = 0;
    for _ in 0..1000 {
        let before = covers(&app.storage).len();
        let Some(at) = effects.wake_at() else {
            break;
        };
        app.now = app.now.max(at.0);
        effects = app.kernel.tick(Instant(app.now));
        most = most.max(covers(&app.storage).len() - before);
    }
    assert_eq!(most, 1);
    assert_eq!(covers(&app.storage).len(), 2);
}

/// While keys come (the reader going through the library), no cover is
/// begun: a key pressed would wait for it, seconds for a large one on the
/// X4. Once they stop for a while, the covers shown are read.
#[test]
fn no_cover_is_begun_while_keys_come() {
    let _turn = turn();
    let mut app = walked(
        X4,
        &[("books/alice.epub", ALICE), ("books/kafka.epub", KAFKA)],
    );
    app.now += 100;
    let mut wake = app
        .kernel
        .run_action(Instant(app.now), actions::LIBRARY)
        .wake_at();
    for n in 0..20 {
        let key = app.now + 300;
        while let Some(at) = wake.filter(|at| at.0 < key) {
            app.now = app.now.max(at.0);
            wake = app.kernel.tick(Instant(app.now)).wake_at();
        }
        app.now = key;
        let action = if n % 2 == 0 {
            actions::DOWN
        } else {
            actions::UP
        };
        wake = app.kernel.run_action(Instant(app.now), action).wake_at();
        assert!(covers(&app.storage).is_empty(), "{n}");
    }
    let last_key = app.now;
    let mut first = None;
    for _ in 0..1000 {
        let Some(at) = wake else {
            break;
        };
        app.now = app.now.max(at.0);
        wake = app.kernel.tick(Instant(app.now)).wake_at();
        if first.is_none() && !covers(&app.storage).is_empty() {
            first = Some(app.now);
        }
    }
    assert_eq!(covers(&app.storage).len(), 2);
    assert!(
        first.is_some_and(|at| at >= last_key + 600),
        "{first:?} {last_key}"
    );
}

#[test]
fn a_replaced_book_shows_its_own_cover() {
    let _turn = turn();
    let mut app = walked(
        LARGE,
        &[("books/a.epub", ALICE), ("books/kafka.epub", KAFKA)],
    );
    app.run(actions::LIBRARY);
    app.let_catalog_work();
    let alice = library::book(&app.storage, "books/a.epub")
        .unwrap()
        .cover
        .unwrap();
    let kafka = library::book(&app.storage, "books/kafka.epub")
        .unwrap()
        .cover
        .unwrap();
    assert_ne!((alice.width, alice.height), (kafka.width, kafka.height));
    // Another book copied over it, the library told (the list shows: the
    // catalog is made beside it, on a device of much memory).
    app.storage.write("books/a.epub", KAFKA).unwrap();
    app.send(events::LIBRARY_CHANGED, Params::new());
    let book = library::book(&app.storage, "books/a.epub").expect("read again");
    assert!(book.title.is_some_and(|t| t.contains("Verwandlung")));
    let cover = book.cover.expect("read again as it shows");
    assert_eq!((cover.width, cover.height), (kafka.width, kafka.height));
    assert_eq!(
        app.storage.size(&cover.path).unwrap(),
        app.storage.size(&kafka.path).unwrap()
    );
}
