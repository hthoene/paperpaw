//! Files takes a book for a book again (#487): the library tells it by the
//! formats the plugins declared, so a book's actions offer *Open*, and
//! deleting it asks about the book and removes it from the library too.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::{MemoryStorage, Storage};
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// The labels of what the screen shows.
fn labels(device: &mut Device) -> Vec<String> {
    let mut labels = Vec::new();
    device.call(|kernel, _| {
        labels = kernel.describe().into_iter().map(|p| p.label).collect();
        paperpaw_core::Effects::default()
    });
    labels
}

/// The paths of the catalog's books, its log of changes applied.
fn catalog(card: &MemoryStorage) -> Vec<String> {
    let mut paths = Vec::new();
    paperpaw_core::kit::library::catalog::each(card, &mut |book| paths.push(book.path));
    paths
}

#[test]
fn files_opens_a_book_and_deletes_it_from_the_library() {
    let card = Rc::new(
        MemoryStorage::new()
            .with("paperpaw.ini", "[help]\nshow = false\n")
            .with(".paperpaw/plugins/setup/done", "")
            .with("books/alice.epub", ALICE),
    );
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, card.clone(), true, None).unwrap();
    device.start();
    // The walk puts the book into the catalog.
    run_script(&mut device, "wait:20000").unwrap();
    assert_eq!(catalog(&card), ["books/alice.epub"]);
    // The folder `books`, then its book.
    run_script(
        &mut device,
        "open:file-manager.browser,wait:1000,run:core.confirm,wait:1000,run:core.confirm,wait:1000",
    )
    .unwrap();
    let offered = labels(&mut device);
    assert!(offered.iter().any(|l| l == "Open"), "{offered:?}");
    let delete = offered.iter().position(|l| l == "Delete").expect("Delete");
    let open = offered.iter().position(|l| l == "Open").unwrap();
    let steps = "run:core.down,".repeat(delete - open);
    run_script(&mut device, &format!("{steps}run:core.confirm,wait:1000")).unwrap();
    let asked = labels(&mut device);
    assert!(
        asked.iter().any(|l| l.starts_with("Delete the book")),
        "{asked:?}"
    );
    // The answer starts on the safe side: up to "Yes, delete".
    let yes = asked.iter().position(|l| l == "Yes, delete").expect("Yes");
    let cancel = asked.iter().position(|l| l == "Cancel").expect("Cancel");
    let step = if yes < cancel {
        "run:core.up,"
    } else {
        "run:core.down,"
    };
    run_script(&mut device, &format!("{step}run:core.confirm,wait:5000")).unwrap();
    assert!(card.size("books/alice.epub").is_err(), "the file is gone");
    assert!(catalog(&card).is_empty(), "{:?}", catalog(&card));
}
