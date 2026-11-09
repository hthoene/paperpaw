//! The list of words: its rows newest first one page at a time, the word a
//! row names, and the question about deleting, in both languages.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_i18n::{Arg as Message, Catalog, Translator};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;

use super::*;

struct Catalogued(Translator);

fn words(language: &str) -> Catalogued {
    let source = match language {
        "de" => include_str!("../i18n/de.ini"),
        _ => include_str!("../i18n/en.ini"),
    };
    let mut translator = Translator::new();
    translator.add(Catalog::parse(source).unwrap());
    translator.set_language(language);
    Catalogued(translator)
}

impl Words for Catalogued {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        Ok(self.0.tr(key).to_string())
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let named: Vec<(&str, Message<'_>)> = args
            .iter()
            .filter_map(|arg| match *arg {
                Arg::Text { name, value } => Some((name, Message::Str(value))),
                _ => None,
            })
            .collect();
        Ok(self.0.format(key, &named))
    }
}

fn word(name: &str, sentence: &str, title: &str) -> Word {
    Word {
        word: name.into(),
        language: "en".into(),
        title: title.into(),
        sentence: sentence.into(),
        ..Word::default()
    }
}

/// `n` words, `w0` the oldest; every third has no sentence (its book's title
/// stands in).
fn stored(n: usize) -> MemoryStorage {
    let storage = MemoryStorage::new();
    for i in 0..n {
        let sentence = if i % 3 == 2 {
            String::new()
        } else {
            alloc::format!("sentence {i}")
        };
        store::add(
            &storage,
            &word(&alloc::format!("w{i}"), &sentence, "A Book"),
        )
        .unwrap();
    }
    storage
}

fn titles(rows: &[RowItem]) -> Vec<&str> {
    rows.iter().map(|row| row.title.as_str()).collect()
}

#[test]
fn the_words_are_listed_newest_first_with_their_sentence_or_book() {
    let storage = stored(5);
    let rows = WordRows::new(&storage, 5).rows(0, 5).unwrap();
    assert_eq!(titles(&rows), ["w4", "w3", "w2", "w1", "w0"]);
    assert_eq!(rows[0].note, "sentence 4");
    assert_eq!(rows[2].note, "A Book", "w2 has no sentence");
    assert!(rows.iter().all(|row| row.value.is_empty()));
}

#[test]
fn only_the_rows_of_a_page_are_made() {
    let storage = stored(7);
    let source = WordRows::new(&storage, 7);
    assert_eq!(source.len(), 7);
    assert_eq!(titles(&source.rows(0, 3).unwrap()), ["w6", "w5", "w4"]);
    assert_eq!(titles(&source.rows(3, 3).unwrap()), ["w3", "w2", "w1"]);
    assert_eq!(
        titles(&source.rows(6, 3).unwrap()),
        ["w0"],
        "fewer at the end"
    );
    assert!(source.rows(7, 3).unwrap().is_empty());
    assert!(source.rows(0, 0).unwrap().is_empty());
}

#[test]
fn a_row_names_its_word_and_a_row_past_the_list_names_none() {
    let storage = stored(4);
    let source = WordRows::new(&storage, 4);
    assert_eq!(source.word(0).unwrap().word, "w3");
    assert_eq!(source.word(3).unwrap().word, "w0");
    assert_eq!(source.word(4), Err(Error::NotFound));
}

#[test]
fn a_word_deleted_leaves_the_rows_without_it() {
    let storage = stored(3);
    let held = WordRows::new(&storage, 3).word(1).unwrap();
    assert_eq!(held.word, "w1");
    store::remove(&storage, &held).unwrap();
    let count = store::count(&storage).unwrap();
    assert_eq!(count, 2);
    let rows = WordRows::new(&storage, count).rows(0, 5).unwrap();
    assert_eq!(titles(&rows), ["w2", "w0"]);
}

#[test]
fn the_question_names_the_word_in_both_languages() {
    let en = delete_texts(&mut words("en"), "Hund").unwrap();
    assert_eq!(en.title, "Delete “Hund”?");
    assert_eq!(en.labels, "Delete\nKeep");
    assert!(en.text.contains("for good"));
    let de = delete_texts(&mut words("de"), "Hund").unwrap();
    assert!(de.title.contains("Hund"));
    assert_ne!(de.title, en.title);
    assert_eq!(de.labels.lines().count(), 2);
    // The two labels are what the dialog takes.
    assert!(en.question().is_ok());
    assert!(de.question().is_ok());
}
