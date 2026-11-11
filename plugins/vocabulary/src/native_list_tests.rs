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

#[test]
fn a_word_deleted_is_counted_out_and_a_failed_delete_says_so() {
    let storage = stored(3);
    let held = WordRows::new(&storage, 3).word(0).unwrap();
    assert_eq!(delete(&storage, &held), (true, 2));
    // The same word again: nothing to remove is not a failure; still 2.
    assert_eq!(delete(&storage, &held).1, 2);
    // A storage that cannot be written: not deleted, and what is there is counted.
    let (deleted, count) = delete(&paperpaw_sdk::NoStorage, &held);
    assert!(!deleted);
    assert_eq!(count, 0);
}

#[test]
fn a_tap_on_a_word_says_how_to_delete_it_in_both_languages() {
    for language in ["en", "de"] {
        let said = words(language).copy("vocabulary.hold-to-delete").unwrap();
        assert!(
            !said.is_empty() && said != "vocabulary.hold-to-delete",
            "{language}"
        );
    }
}

/// Coming back to the list (the screen is shown again, and the shell counts the
/// words once more): a word looked up meanwhile is the first row, and the
/// focus stays on a row that is there.
#[test]
fn a_list_shown_again_has_the_words_looked_up_meanwhile() {
    let storage = MemoryStorage::new();
    store::add(&storage, &word("quire", "", "A book")).unwrap();
    let mut view = WordsView::new(store::count(&storage).unwrap());
    assert_eq!(view.count(), 1);
    // The list is covered by a dialog; a word is looked up behind it.
    store::add(&storage, &word("verso", "", "A book")).unwrap();
    store::add(&storage, &word("folio", "", "A book")).unwrap();
    assert_eq!(view.count(), 1, "the screen counts only when shown");
    // Shown again: counted again, as `recount` in the shell does.
    view.recount(store::count(&storage).unwrap());
    assert_eq!(view.count(), 3);
    let rows = WordRows::new(&storage, view.count()).rows(0, 10).unwrap();
    let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
    assert_eq!(titles, ["folio", "verso", "quire"]);
}

/// *Export* is the row after the last word: on the page that holds it, and not
/// with no word, nor where the list has no such row.
#[test]
fn export_is_the_row_after_the_last_word() {
    let storage = MemoryStorage::new();
    for name in ["quire", "verso", "folio"] {
        store::add(&storage, &word(name, "", "A book")).unwrap();
    }
    let rows = WordRows::new(&storage, 3).with_export("Export".into(), "Help".into());
    assert_eq!(rows.len_with_export(), 4);
    // Not on the page of the first three, nor past it.
    assert!(rows.export_row(0, 3).unwrap().is_none());
    assert!(rows.export_row(5, 3).unwrap().is_none());
    // On the page that holds the last word and on the one that starts with it.
    for (first, max) in [(2, 3), (3, 3), (0, 10)] {
        let row = rows.export_row(first, max).unwrap().unwrap();
        assert_eq!((row.title.as_str(), row.note.as_str()), ("Export", "Help"));
        assert_eq!(row.value, "›");
    }
    // The words' own rows are the words only.
    assert_eq!(rows.rows(0, 10).unwrap().len(), 3);
    // No word, no row; a list without the row has none.
    let empty = WordRows::new(&storage, 0).with_export("Export".into(), "Help".into());
    assert_eq!(empty.len_with_export(), 0);
    assert!(empty.export_row(0, 10).unwrap().is_none());
    assert_eq!(WordRows::new(&storage, 3).len_with_export(), 3);
    assert!(
        WordRows::new(&storage, 3)
            .export_row(0, 10)
            .unwrap()
            .is_none()
    );
}

/// The export of the words reads and writes through the storage the plugin
/// gives it: the file is the header and a line a word, oldest first.
#[test]
fn the_export_is_the_header_and_a_line_a_word() {
    let storage = MemoryStorage::new();
    store::add(
        &storage,
        &word("quire", "A quire is a set of sheets.", "A book"),
    )
    .unwrap();
    store::add(&storage, &word("verso", "", "Another")).unwrap();
    store::export(&storage, "export.tsv").unwrap();
    let text = String::from_utf8(storage.get("export.tsv").unwrap()).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("#separator:tab"));
    assert_eq!(lines.next(), Some("#html:false"));
    assert_eq!(
        lines.next(),
        Some("quire\tA quire is a set of sheets.\tA book")
    );
    assert_eq!(lines.next(), Some("verso\t\tAnother"));
    assert_eq!(lines.next(), None);
}
