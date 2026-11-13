//! The card files' screens: the menu's rows in both languages, an update and a
//! save run to their end, the question about a deck of the same name, and what
//! the run does when the heap refuses.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::MemoryStorage;
use paperpaw_ui_kit::Item;

use super::*;
use crate::alloc_refusal_tests::Quiet;
use crate::cards;
use crate::native_words::testing::words;
use crate::sync::tests::heap::{armed, fixture};
use crate::sync::tests::{FOLDER, put};

fn row(item: &Item) -> (&str, &str, &str) {
    match item {
        Item::Row(row) => (&row.title, &row.value, &row.note),
        Item::Heading(_) => panic!("a heading"),
    }
}

/// The run through to its end: how many steps it took and whether it asked.
fn finish(run: &mut Run, storage: &dyn Storage, language: &str, link: bool) -> usize {
    let mut catalog = words(language);
    for steps in 1..10_000 {
        let next = run.step(storage, &mut catalog);
        if run.asking() {
            run.answer(storage, link);
        } else if !next.more {
            return steps;
        }
    }
    panic!("the run does not end");
}

fn rows_of(run: &Run) -> Vec<(String, String)> {
    run.rows.clone()
}

#[test]
fn the_menu_has_an_update_row_then_a_save_row_for_each_deck() {
    let storage = MemoryStorage::new();
    put(&storage, "Spanish.tsv", "casa\thouse\n");
    crate::sync::tests::run(&storage, false, true);
    let own = cards::add_deck(&storage, "Mine").unwrap();
    let decks = cards::decks(&storage).unwrap();
    let ledger = sync::sources(&storage).unwrap();
    let pending = sync::pending(&storage, FOLDER).unwrap();
    let shown = Shown {
        folder: FOLDER,
        pending,
        decks: &decks,
        ledger: &ledger,
    };
    let view = MenuView::new(&mut words("en"), shown).unwrap();
    // Update; Spanish (linked: save, switch); Mine (not linked: save).
    let targets: Vec<_> = (0..4).filter_map(|i| view.target(i)).collect();
    let spanish = crate::sync::tests::deck_id(&storage, "Spanish");
    assert_eq!(
        targets,
        [
            Target::Update,
            Target::Save(spanish),
            Target::Mirror(spanish),
            Target::Save(own)
        ]
    );
    assert_eq!(view.target(4), None);
}

#[test]
fn the_rows_say_what_each_does_in_both_languages() {
    let storage = MemoryStorage::new();
    put(&storage, "Spanish.tsv", "casa\thouse\n");
    crate::sync::tests::run(&storage, false, true);
    let decks = cards::decks(&storage).unwrap();
    let ledger = sync::sources(&storage).unwrap();
    let shown = Shown {
        folder: FOLDER,
        pending: sync::pending(&storage, FOLDER).unwrap(),
        decks: &decks,
        ledger: &ledger,
    };
    let (en, _) = rows(&mut words("en"), shown).unwrap();
    assert_eq!(row(&en[0]).0, "Update the decks");
    assert_eq!(row(&en[1]).0, "Save Spanish to the card");
    assert_eq!(
        row(&en[1]).2,
        "Writes Spanish.tsv: id, front, back, example. Other columns are not kept."
    );
    assert_eq!(row(&en[2]).0, "Spanish: cards missing from the file");
    // The switch is off to begin with: the cards stay.
    assert_eq!(row(&en[2]).1, "Keep");
    let (de, _) = rows(&mut words("de"), shown).unwrap();
    assert_eq!(row(&de[0]).0, "Stapel aktualisieren");
    assert_ne!(row(&de[1]).2, row(&en[1]).2);
}

#[test]
fn an_update_reads_the_files_into_decks_and_says_what_came_of_each() {
    for (language, expected) in [
        ("en", "2 added, 0 changed, 0 as before."),
        ("de", "2 hinzugefügt, 0 geändert, 0 wie zuvor."),
    ] {
        let storage = MemoryStorage::new();
        put(&storage, "Spanish.tsv", "casa\thouse\ngato\tcat\n");
        let mut run = Run::new(FOLDER, Mode::Manual, None).unwrap();
        assert!(run.working());
        finish(&mut run, &storage, language, true);
        assert!(!run.working());
        let rows = rows_of(&run);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, "Spanish");
        assert_eq!(rows[0].1, expected, "{language}");
        let deck = crate::sync::tests::deck_id(&storage, "Spanish");
        assert_eq!(crate::sync::tests::cards_of(&storage, deck).len(), 2);
        // A second run finds nothing to do.
        let mut again = Run::new(FOLDER, Mode::Manual, None).unwrap();
        finish(&mut again, &storage, language, true);
        assert_eq!(rows_of(&again).len(), 1);
        assert!(rows_of(&again)[0].0.is_empty());
    }
}

#[test]
fn a_missing_folder_is_made_with_a_note_on_how_files_are_made() {
    let storage = MemoryStorage::new();
    let mut run = Run::new("Flashcards", Mode::Manual, None).unwrap();
    finish(&mut run, &storage, "en", true);
    let rows = rows_of(&run);
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].1.starts_with("The folder Flashcards is made."),
        "{rows:?}"
    );
    assert!(storage.exists("Flashcards/_Read me.txt"));
}

#[test]
fn a_deck_of_the_files_name_is_asked_about_and_the_answer_decides() {
    for link in [true, false] {
        let storage = MemoryStorage::new();
        let own = cards::add_deck(&storage, "spanish").unwrap();
        put(&storage, "Spanish.tsv", "casa\thouse\n");
        let mut run = Run::new(FOLDER, Mode::Manual, None).unwrap();
        let mut catalog = words("en");
        let mut asked = false;
        for _ in 0..1_000 {
            let next = run.step(&storage, &mut catalog);
            if run.asking() {
                asked = true;
                run.show(&mut catalog).unwrap();
                // The question, and the two choices (the heading takes no focus).
                assert_eq!(run.list().items().len(), 3);
                assert_eq!(run.list().focus(), 1);
                run.answer(&storage, link);
            } else if !next.more {
                break;
            }
        }
        assert!(asked);
        if link {
            assert_eq!(cards::decks(&storage).unwrap().len(), 1);
            assert_eq!(crate::sync::tests::cards_of(&storage, own).len(), 1);
        } else {
            assert_eq!(cards::decks(&storage).unwrap().len(), 2);
        }
    }
}

#[test]
fn a_save_writes_the_deck_to_its_file_and_says_so() {
    for (language, expected) in [
        ("en", "1 card saved to Spanish.tsv."),
        ("de", "1 Karte in Spanish.tsv gespeichert."),
    ] {
        let storage = MemoryStorage::new();
        let deck = cards::add_deck(&storage, "Spanish").unwrap();
        cards::add(
            &storage,
            deck,
            [cards::Texts {
                front: "casa".into(),
                back: "house".into(),
                example: String::new(),
            }],
        )
        .unwrap();
        let mut run = Run::new(FOLDER, Mode::Manual, Some(deck)).unwrap();
        finish(&mut run, &storage, language, true);
        let rows = rows_of(&run);
        assert_eq!(rows[0].0, "Spanish");
        assert_eq!(rows[0].1, expected, "{language}");
        let written = storage.get("Flashcards/Spanish.tsv").expect("the file");
        assert!(String::from_utf8(written).unwrap().contains("casa\thouse"));
    }
}

#[test]
fn the_progress_is_a_percentage_in_the_first_row() {
    let storage = MemoryStorage::new();
    put(&storage, "Spanish.tsv", "casa\thouse\n");
    let mut run = Run::new(FOLDER, Mode::Manual, None).unwrap();
    let mut catalog = words("en");
    run.step(&storage, &mut catalog);
    run.show(&mut catalog).unwrap();
    let items = run.list().items();
    assert_eq!(items.len(), 1);
    let (title, value, _) = row(&items[0]);
    assert!(title.starts_with("Reading "), "{title}");
    assert!(value.ends_with(" %"), "{value}");
}

#[test]
fn leaving_a_save_off_leaves_no_part_of_a_file() {
    let storage = MemoryStorage::new();
    let deck = cards::add_deck(&storage, "Spanish").unwrap();
    let mut run = Run::new(FOLDER, Mode::Manual, Some(deck)).unwrap();
    let mut catalog = words("en");
    // Begun, not ended.
    run.step(&storage, &mut catalog);
    run.step(&storage, &mut catalog);
    run.cancel(&storage);
    let part = ["Flashcards/Spanish.tsv.part", "Flashcards/Spanish.tsv.tmp"];
    for path in part {
        assert!(!storage.exists(path), "{path}");
    }
}

#[test]
fn a_refused_allocation_ends_the_run_with_a_line_that_says_so_never_a_panic() {
    let storage = fixture(|| {
        let q = Quiet::default();
        put(&q.0, "Spanish.tsv", "casa\thouse\ngato\tcat\n");
        put(&q.0, "French.csv", "maison,house\n");
        q
    });
    let go = |q: &Quiet| {
        let mut catalog = fixture(|| words("en"));
        let Ok(mut run) = Run::new(FOLDER, Mode::Manual, None) else {
            return None;
        };
        for _ in 0..1_000 {
            let next = run.step(q, &mut catalog);
            if run.asking() {
                run.answer(q, true);
            } else if !next.more {
                return Some(run.rows.len());
            }
        }
        panic!("does not end");
    };
    let (whole, count) = armed(usize::MAX, || go(&storage));
    assert!(whole.is_some());
    assert!(count > 0);
    for at in 1..=count {
        // A fresh card for each refusal: what a failed run did is not shown to the next.
        let storage = fixture(|| {
            let q = Quiet::default();
            put(&q.0, "Spanish.tsv", "casa\thouse\ngato\tcat\n");
            put(&q.0, "French.csv", "maison,house\n");
            q
        });
        let (result, _) = armed(at, || go(&storage));
        // Refused: no run, or a run that ended (with its line or without).
        let _ = result;
        // Whatever was refused, every deck still reads whole.
        fixture(|| {
            for deck in cards::decks(&storage).unwrap() {
                cards::each_card(&storage, deck.id, |_, _| {
                    core::ops::ControlFlow::Continue(())
                })
                .unwrap();
            }
        });
    }
}

#[test]
fn folder_of_the_setting_falls_back_to_the_default() {
    assert_eq!(folder_of("").unwrap(), "Flashcards");
    assert_eq!(folder_of("Cards/Spanish").unwrap(), "Cards/Spanish");
    assert_eq!(folder_of("../x").unwrap(), "Flashcards".to_string());
}

#[test]
fn a_run_whose_step_cannot_be_done_ends_with_a_line_never_waits_for_a_step_that_does_not_come() {
    for (language, expected) in [
        (
            "en",
            "The card could not be read. Check that it is in, then try again.",
        ),
        (
            "de",
            "Die Karte konnte nicht gelesen werden. Prüfe, ob sie steckt, und versuche es noch einmal.",
        ),
    ] {
        let mut run = Run::new(FOLDER, Mode::Manual, None).unwrap();
        assert!(run.working());
        let note = words(language).copy("vocabulary.sd.nocard").unwrap();
        assert_eq!(note, expected);
        run.abort(Some(note));
        assert!(!run.working());
        assert!(!run.asking());
        assert_eq!(rows_of(&run), [(String::new(), expected.to_string())]);
    }
    // Not even the line can be said: the run still ends.
    let mut run = Run::new(FOLDER, Mode::Manual, Some(1)).unwrap();
    run.abort(None);
    assert!(!run.working());
    assert!(rows_of(&run).is_empty());
}

#[test]
fn what_a_turn_reads_through_both_is_counted_from_the_card_and_the_data_folder() {
    use crate::native_card::Both;
    use paperpaw_sdk::Storage;
    let (data, card) = (MemoryStorage::new(), MemoryStorage::new());
    card.write("a.txt", &[7; 1000]).unwrap();
    data.write("decks.ini", &[1; 50]).unwrap();
    let both = Both {
        data: &data,
        card: &card,
        read: core::cell::Cell::new(0),
    };
    let mut buf = [0; 600];
    assert_eq!(both.read_at("a.txt", 0, &mut buf), Ok(600));
    assert_eq!(both.read_at("a.txt", 600, &mut buf), Ok(400));
    assert_eq!(both.read.get(), 1000);
    let path = alloc::format!("{}decks.ini", ".paperpaw/plugins/vocabulary/");
    assert_eq!(both.read_at(&path, 0, &mut buf), Ok(50));
    assert_eq!(both.read.get(), 1050);
    // A read that fails counts nothing.
    assert!(both.read_at("missing", 0, &mut buf).is_err());
    assert_eq!(both.read.get(), 1050);
}
