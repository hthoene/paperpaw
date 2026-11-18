//! Keeping the words looked up: the list, the setting, the cards to be, and when
//! the heap refuses.
use paperpaw_sdk::MemoryStorage;

use super::*;
use crate::alloc_refusal_tests::Quiet;
use crate::stages_subscribed::reader_word_looked_up_v1::WordLooked;
use crate::sync::tests::heap::{armed, fixture};

fn looked(word: &'static str) -> WordLooked<'static> {
    WordLooked {
        word,
        sentence: "A sentence with the word.",
        path: "Books/a.epub",
        section: 3,
        offset: 40,
        language: "en",
        title: "A book",
    }
}

fn words(storage: &MemoryStorage) -> alloc::vec::Vec<Word> {
    store::range(storage, 0, 100)
        .unwrap()
        .into_iter()
        .map(|(_, word)| word)
        .collect()
}

#[test]
fn a_word_is_kept_with_its_sentence_its_book_and_its_place() {
    let storage = MemoryStorage::new();
    keep(&storage, true, false, &looked(" quire ")).unwrap();
    let kept = words(&storage);
    assert_eq!(kept.len(), 1);
    let word = &kept[0];
    assert_eq!(
        (
            word.word.as_str(),
            word.language.as_str(),
            word.title.as_str()
        ),
        ("quire", "en", "A book")
    );
    assert_eq!(word.sentence, "A sentence with the word.");
    assert_eq!(
        (word.path.as_str(), word.section, word.offset),
        ("Books/a.epub", 3, 40)
    );
}

#[test]
fn the_same_word_again_is_kept_once_as_the_newest() {
    let storage = MemoryStorage::new();
    keep(&storage, true, false, &looked("quire")).unwrap();
    keep(&storage, true, false, &looked("verso")).unwrap();
    keep(&storage, true, false, &looked("quire")).unwrap();
    let kept: alloc::vec::Vec<_> = words(&storage).into_iter().map(|w| w.word).collect();
    // `range` lists the oldest first: the word looked up again is now the last.
    assert_eq!(kept, ["verso", "quire"]);
}

#[test]
fn blank_words_and_a_switched_off_setting_keep_nothing() {
    let storage = MemoryStorage::new();
    keep(&storage, true, false, &looked("   ")).unwrap();
    keep(&storage, false, true, &looked("quire")).unwrap();
    assert!(words(&storage).is_empty());
    assert!(!storage.exists(".paperpaw/plugins/vocabulary/words.txt"));
}

#[test]
fn where_cards_are_wanted_the_word_waits_to_become_one() {
    let storage = MemoryStorage::new();
    keep(&storage, true, true, &looked("quire")).unwrap();
    assert_eq!(words(&storage).len(), 1);
    assert_eq!(cards::take_lookups(&storage, "Looked up").unwrap(), 1);
    let deck = crate::sync::tests::deck_id(&storage, "Looked up");
    let cards = crate::sync::tests::cards_of(&storage, deck);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].front, "quire");
    assert_eq!(cards[0].example, "A sentence with the word.");
}

#[test]
fn when_the_heap_refuses_nothing_panics_and_the_error_is_told() {
    let make = || fixture(|| Quiet(MemoryStorage::new()));
    let (whole, count) = armed(usize::MAX, || keep(&make(), true, true, &looked("quire")));
    assert!(whole.is_ok());
    assert!(count > 0);
    for at in 1..=count {
        let storage = make();
        let (result, _) = armed(at, || keep(&storage, true, true, &looked("quire")));
        if let Err(error) = result {
            assert_eq!(error, StorageError::TooLarge, "refusing allocation {at}");
        }
    }
}
