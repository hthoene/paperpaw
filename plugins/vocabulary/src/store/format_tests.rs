//! `words.txt` as written and read back: any text in any field survives a
//! round trip but for tabs and line breaks (spaces then) and long
//! sentences (cut), the same word is kept once, the file version 1 stays
//! readable, and hostile lines (missing fields, numbers that are none,
//! overlong lines, bytes that are no UTF-8) are skipped without a panic.

extern crate std;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::lines::MAX_LINE;
use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::*;

fn word(word: &str, language: &str) -> Word {
    Word {
        word: word.to_string(),
        language: language.to_string(),
        section: 1,
        offset: 2,
        path: "books/a.txt".to_string(),
        title: "A".to_string(),
        sentence: format!("With {word} in it."),
    }
}

/// The words in the file, oldest first, as the list reads them.
fn all(storage: &dyn Storage) -> Vec<Word> {
    range(storage, 0, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|(_, w)| w)
        .collect()
}

/// What a field becomes in the file: tabs and line breaks as spaces.
fn kept(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn any_word() -> impl Strategy<Value = Word> {
    let text = "[a-zA-Z\t\n\r äöüß€😀,.]{0,40}";
    (
        text,
        "[a-z\t]{0,5}",
        any::<u32>(),
        any::<u32>(),
        text,
        text,
        "[a-zA-Z\t\n\r äöüß€😀,.]{0,400}",
    )
        .prop_map(
            |(word, language, section, offset, path, title, sentence)| Word {
                word,
                language,
                section,
                offset,
                path,
                title,
                sentence,
            },
        )
}

/// A word as the reader passes it on: no tabs or line breaks in the word
/// and its language (any in the rest).
fn listed_word() -> impl Strategy<Value = Word> {
    (any_word(), "[a-zA-ZäöüßÄÖÜ ]{0,12}", "[a-z]{0,3}").prop_map(|(w, word, language)| Word {
        word,
        language,
        ..w
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// A line written is one line, read back as it was written (tabs and
    /// line breaks as spaces, the sentence at most 300 characters).
    #[test]
    fn a_line_round_trips(w in any_word()) {
        let line = w.line().unwrap();
        prop_assert!(line.ends_with('\n'));
        prop_assert_eq!(line.matches('\n').count(), 1);
        prop_assert_eq!(line.matches('\t').count(), 6);
        let back = Word::parse(line.trim_end_matches('\n')).unwrap().unwrap();
        prop_assert_eq!(back.word, kept(&w.word));
        prop_assert_eq!(back.language, kept(&w.language));
        prop_assert_eq!((back.section, back.offset), (w.section, w.offset));
        prop_assert_eq!(back.path, kept(&w.path));
        prop_assert_eq!(back.title, kept(&w.title));
        let sentence: String = kept(&w.sentence).chars().take(MAX_SENTENCE).collect();
        prop_assert_eq!(back.sentence, sentence);
    }

    /// Words added are all in the file, newest last, each once whatever
    /// its case (one added again while newest staying as it was), and the file is read the same line by line however its
    /// lines fall on the chunks it is read in.
    #[test]
    fn words_added_are_read_back(words in proptest::collection::vec(listed_word(), 1..12)) {
        let storage = MemoryStorage::new();
        for w in &words {
            add(&storage, w).unwrap();
        }
        let read = all(&storage);
        // Expected: the last of each word (by lower case and language),
        // in the order they were last added.
        let mut expected: Vec<&Word> = Vec::new();
        for w in &words {
            let same = |e: &&Word| {
                e.language == w.language && e.word.to_lowercase() == w.word.to_lowercase()
            };
            // Looked up again while it is the newest, it stays as kept.
            if expected.last().is_some_and(same) {
                continue;
            }
            expected.retain(|e| !same(e));
            expected.push(w);
        }
        prop_assert_eq!(read.len(), expected.len());
        for (r, e) in read.iter().zip(&expected) {
            prop_assert_eq!(&r.word, &e.word);
            prop_assert_eq!((r.section, r.offset), (e.section, e.offset));
        }
        prop_assert_eq!(count(&storage).unwrap(), expected.len());
        prop_assert_eq!(summary(&storage).unwrap().count, expected.len());
    }

    /// Whatever bytes the file holds, reading it neither panics nor
    /// yields more words than it has lines.
    #[test]
    fn any_bytes_are_read_without_a_panic(mut data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        // Ending with a line break, as PaperPaw writes it (see
        // `a_word_added_to_a_file_without_a_last_line_break_is_its_own`).
        data.push(b'\n');
        let storage = MemoryStorage::new().with(file(), data.clone());
        let lines = data.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count();
        let words = all(&storage);
        prop_assert!(words.len() <= lines);
        prop_assert!(count(&storage).unwrap() <= lines);
        let _ = summary(&storage).unwrap();
        export(&storage, "out.tsv").unwrap();
        add(&storage, &word("new", "en")).unwrap();
        prop_assert_eq!(all(&storage).last().map(|w| w.word.clone()), Some("new".to_string()));
    }
}

#[test]
fn the_same_word_in_another_case_replaces_the_older() {
    let storage = MemoryStorage::new();
    add(&storage, &word("Rabbit", "en")).unwrap();
    add(&storage, &word("hole", "en")).unwrap();
    add(&storage, &word("RABBIT", "en")).unwrap();
    let words: Vec<String> = all(&storage).into_iter().map(|w| w.word).collect();
    assert_eq!(words, ["hole", "RABBIT"]);
}

#[test]
fn the_same_word_in_another_language_is_another_word() {
    let storage = MemoryStorage::new();
    add(&storage, &word("Gift", "en")).unwrap();
    add(&storage, &word("Gift", "de")).unwrap();
    assert_eq!(count(&storage).unwrap(), 2);
    assert_eq!(summary(&storage).unwrap().languages, 2);
}

#[test]
fn duplicates_in_the_file_all_go_when_the_word_is_added_again() {
    let storage = MemoryStorage::new();
    let twice = format!(
        "{}{}{}",
        word("echo", "en").line().unwrap(),
        word("Echo", "en").line().unwrap(),
        word("other", "en").line().unwrap()
    );
    storage.write(file(), twice.as_bytes()).unwrap();
    add(&storage, &word("ECHO", "en")).unwrap();
    let words: Vec<String> = all(&storage).into_iter().map(|w| w.word).collect();
    assert_eq!(words, ["other", "ECHO"]);
}

#[test]
fn hostile_lines_are_skipped() {
    let storage = MemoryStorage::new();
    let mut data = Vec::new();
    // Too few fields, numbers that are none, a negative and an overflowing
    // one, an empty line, a line of tabs.
    for line in [
        "only\tthree\tfields",
        "w\ten\tx\t1\tp\tt\ts",
        "w\ten\t-1\t1\tp\tt\ts",
        "w\ten\t1\t99999999999\tp\tt\ts",
        "",
        "\t\t\t\t\t\t",
    ] {
        data.extend_from_slice(line.as_bytes());
        data.push(b'\n');
    }
    // Bytes that are no UTF-8 in a word: read with a replacement.
    data.extend_from_slice(b"caf\xe9\tfr\t1\t2\tp\tt\ts\n");
    // A line far longer than kept: cut, its fields up to there read.
    let long = format!("long\ten\t3\t4\tp\tt\t{}\n", "x".repeat(5000));
    data.extend_from_slice(long.as_bytes());
    // The last line without a line break.
    data.extend_from_slice(b"last\ten\t5\t6\tp\tt\tend");
    storage.write(file(), &data).unwrap();
    let words = all(&storage);
    let names: Vec<&str> = words.iter().map(|w| w.word.as_str()).collect();
    assert_eq!(names, ["caf\u{fffd}", "long", "last"]);
    assert!(words[1].sentence.len() < MAX_LINE);
    assert_eq!(words[2].sentence, "end");
    assert_eq!(summary(&storage).unwrap().count, 3);
}

#[test]
fn a_line_cut_inside_a_character_is_read() {
    // The cut falls inside "€" (three bytes): the rest is replaced, not a
    // panic.
    let storage = MemoryStorage::new();
    let fields = "w\ten\t1\t2\tp\tt\t";
    let pad = MAX_LINE - fields.len() - 1;
    let line = format!("{fields}{}€€\n", "a".repeat(pad));
    storage.write(file(), line.as_bytes()).unwrap();
    let words = all(&storage);
    assert_eq!(words.len(), 1);
    assert!(words[0].sentence.ends_with('\u{fffd}'));
}

#[test]
fn the_writer_still_writes_version_1() {
    // What version 1 wrote, written again by this version, is the same.
    let v1 = include_str!("../../testdata/words.txt.v1");
    let storage = MemoryStorage::new().with(file(), v1);
    let again: String = all(&storage).iter().map(|w| w.line().unwrap()).collect();
    assert_eq!(again, v1.replace("\r\n", "\n"));
}

/// With a line before that is no word, *Delete* on a word deletes that
/// word. (The list used to number words by the lines of the file and
/// delete by the words read, so it deleted the one after it.)
#[test]
fn deleting_a_word_after_an_unreadable_line_deletes_that_word() {
    let storage = MemoryStorage::new();
    let data = format!(
        "not a word\n{}{}",
        word("first", "en").line().unwrap(),
        word("second", "en").line().unwrap()
    );
    storage.write(file(), data.as_bytes()).unwrap();
    // The list shows each word with its number, and deletes by it.
    let listed = range(&storage, 0, 10).unwrap();
    let (_, first) = listed.iter().find(|(_, w)| w.word == "first").unwrap();
    assert_eq!(first.word, "first");
    remove(&storage, first).unwrap();
    let left: Vec<String> = all(&storage).into_iter().map(|w| w.word).collect();
    assert_eq!(left, ["second"]);
}

/// A file edited by hand often lacks the last line break: the word added
/// next is a line of its own. (It used to be joined to the last line, so
/// it was lost and that word's sentence spoilt.)
#[test]
fn a_word_added_to_a_file_without_a_last_line_break_is_its_own() {
    let storage = MemoryStorage::new();
    let last = word("last", "en").line().unwrap();
    storage
        .write(file(), last.trim_end_matches('\n').as_bytes())
        .unwrap();
    add(&storage, &word("new", "en")).unwrap();
    let words: Vec<String> = all(&storage).into_iter().map(|w| w.word).collect();
    assert_eq!(words, ["last", "new"]);
    assert_eq!(all(&storage)[0].sentence, "With last in it.");
}
