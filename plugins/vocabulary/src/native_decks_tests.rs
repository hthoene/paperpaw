//! The rows of the flash cards' screen, in both languages, made over the
//! plugin's own catalogs; and when the heap refuses.

use alloc::string::ToString;

use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::Item;

use super::*;
use crate::sync::tests::heap::{armed, fixture};

use crate::native_words::testing::words;

fn numbers() -> Numbers {
    Numbers {
        due: 7,
        decks: alloc::vec![
            (1, "Spanish".to_string(), 120),
            (4, "Wörter".to_string(), 1),
        ],
        folder: "Cards".to_string(),
        pending: Pending {
            folder: true,
            files: 3,
            todo: 2,
            failed: 0,
        },
        phone: true,
        files: true,
    }
}

fn row(item: &Item) -> (&str, &str, &str) {
    match item {
        Item::Row(row) => (&row.title, &row.value, &row.note),
        Item::Heading(_) => panic!("a heading"),
    }
}

#[test]
fn the_rows_are_review_phone_files_then_the_decks() {
    let (items, targets) = rows(&mut words("en"), &numbers()).unwrap();
    assert_eq!(items.len(), 5);
    assert_eq!(
        targets,
        [
            Target::Review,
            Target::Phone,
            Target::Files,
            Target::Deck(1),
            Target::Deck(4)
        ]
    );
    assert_eq!(row(&items[0]), ("Review", "›", "7 cards due"));
    assert_eq!(row(&items[1]).0, "Edit with your phone");
    assert!(row(&items[1]).2.starts_with("Add and change decks"));
    assert_eq!(row(&items[2]).0, "Cards on the SD card");
    assert_eq!(
        row(&items[2]).2,
        "2 files in Cards are new or changed. Choose this to update the decks."
    );
    assert_eq!(row(&items[3]), ("Spanish", "›", "120 cards"));
    assert_eq!(row(&items[4]), ("Wörter", "›", "1 card"));
}

#[test]
fn the_same_rows_come_in_german() {
    let (items, _) = rows(&mut words("de"), &numbers()).unwrap();
    assert_eq!(row(&items[0]), ("Wiederholen", "›", "7 Karten fällig"));
    assert_eq!(row(&items[1]).0, "Mit dem Handy bearbeiten");
    assert_eq!(row(&items[2]).0, "Karten von der SD-Karte");
    assert!(row(&items[2]).2.starts_with("2 Dateien in Cards sind neu"));
    assert_eq!(row(&items[4]).2, "1 Karte");
}

#[test]
fn the_phone_row_helps_to_make_the_first_deck() {
    let empty = Numbers {
        folder: "Cards".to_string(),
        phone: true,
        files: true,
        ..Numbers::default()
    };
    let (items, targets) = rows(&mut words("en"), &empty).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(targets.len(), 3);
    assert!(row(&items[1]).2.starts_with("Make your first deck"));
    assert_eq!(row(&items[0]).2, "0 cards due");
}

#[test]
fn the_files_row_says_what_there_is_to_do() {
    let say = |pending: Pending, language: &str| {
        files_note(&mut words(language), "Cards", pending).unwrap()
    };
    let none = Pending::default();
    assert!(say(none, "en").starts_with("Choose this to make the folder Cards"));
    let folder = Pending {
        folder: true,
        ..Pending::default()
    };
    assert!(say(folder, "en").starts_with("No CSV, TSV or Anki files in Cards yet"));
    let same = Pending {
        folder: true,
        files: 2,
        ..Pending::default()
    };
    assert!(say(same, "en").starts_with("Up to date."));
    let failed = Pending { failed: 1, ..same };
    assert!(say(failed, "en").starts_with("1 file in Cards could not be made into a deck"));
    assert!(say(failed, "de").starts_with("1 Datei in Cards konnte nicht"));
    // New or changed files come before the ones that failed.
    let both = Pending { todo: 1, ..failed };
    assert!(say(both, "en").starts_with("1 file in Cards is new or changed"));
}

#[test]
fn a_row_opens_its_screen_and_a_deck_is_named() {
    assert_eq!(screen_of(Target::Review), ("vocabulary.review", None));
    assert_eq!(screen_of(Target::Deck(9)), ("vocabulary.review", Some(9)));
    assert_eq!(screen_of(Target::Phone), ("vocabulary.phone", None));
    assert_eq!(screen_of(Target::Files), ("vocabulary.sd-menu", None));
    let view = DecksView::build(&mut words("en"), &numbers()).unwrap();
    assert_eq!(view.target(3), Some(Target::Deck(1)));
    assert_eq!(view.target(5), None);
}

#[test]
fn the_screen_ids_are_those_of_the_interface_6_screens() {
    // Pinned from the interface-6 screens, which are gone (#244).
    assert_eq!(SCREEN, "vocabulary.cards");
    assert_eq!(REVIEW, "vocabulary.review");
    assert_eq!(DECK, "deck");
    assert_eq!(PHONE, "vocabulary.phone");
    assert_eq!(SD_MENU, "vocabulary.sd-menu");
}

#[test]
fn the_numbers_are_read_from_the_decks() {
    let storage = MemoryStorage::new();
    let first = crate::cards::add_deck(&storage, "Spanish").unwrap();
    let second = crate::cards::add_deck(&storage, "Wörter").unwrap();
    let read = Numbers::read(&storage, 3, "Cards", Pending::default()).unwrap();
    assert_eq!(read.due, 3);
    assert_eq!(read.folder, "Cards");
    assert_eq!(
        read.decks,
        [
            (first, "Spanish".to_string(), 0),
            (second, "Wörter".to_string(), 0)
        ]
    );
    assert!(
        Numbers::read(&MemoryStorage::new(), 0, "", Pending::default())
            .unwrap()
            .decks
            .is_empty()
    );
}

#[test]
fn when_the_heap_refuses_the_rows_come_back_as_an_error() {
    for language in ["en", "de"] {
        let numbers = numbers();
        let mut catalog = fixture(|| words(language));
        let (whole, count) = armed(usize::MAX, || DecksView::build(&mut catalog, &numbers));
        assert!(whole.is_ok());
        assert!(
            count > 0,
            "the rows allocate nothing: the sweep proves nothing"
        );
        for at in 1..=count {
            let (result, _) = armed(at, || DecksView::build(&mut catalog, &numbers));
            if let Err(error) = result {
                assert_eq!(error, Error::NoMemory, "refusing allocation {at}");
            }
        }
    }
}

#[test]
fn when_the_heap_refuses_the_numbers_come_back_as_an_error() {
    let storage = fixture(|| {
        let storage = MemoryStorage::new();
        crate::cards::add_deck(&storage, "Spanish").unwrap();
        crate::cards::add_deck(&storage, "Wörter").unwrap();
        storage
    });
    let (whole, count) = armed(usize::MAX, || {
        Numbers::read(&storage, 1, "Cards", Pending::default())
    });
    assert!(whole.is_ok());
    assert!(count > 0);
    for at in 1..=count {
        let (result, _) = armed(at, || {
            Numbers::read(&storage, 1, "Cards", Pending::default())
        });
        if let Err(error) = result {
            assert_eq!(
                error,
                paperpaw_sdk::StorageError::TooLarge,
                "refusing allocation {at}"
            );
        }
    }
}

#[test]
fn the_phone_and_the_card_rows_are_left_out_until_their_screens_exist() {
    let waiting = Numbers {
        phone: false,
        files: false,
        ..numbers()
    };
    let (items, targets) = rows(&mut words("en"), &waiting).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(
        targets,
        [Target::Review, Target::Deck(1), Target::Deck(4)].to_vec()
    );
}
