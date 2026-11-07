//! The data layer when the heap refuses: each allocation of a call is
//! refused in turn. An allocation that would abort (an infallible
//! `format!`, `String::from`, `push`) ends the whole test binary; a refused
//! one must come back as an error, with the files as they were, never a
//! panic or half a list.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

use crate::store::{self, Word};
use crate::sync::tests::heap::{armed, fixture};

/// A storage whose own allocations are not the plugin's: nothing is refused
/// or counted while it works.
#[derive(Debug, Default)]
struct Quiet(MemoryStorage);

impl Quiet {
    fn with(self, path: &str, data: impl Into<Vec<u8>>) -> Self {
        Quiet(self.0.with(path, data.into()))
    }
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        fixture(|| self.0.get(path))
    }
}

impl Storage for Quiet {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        fixture(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        fixture(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.append(path, data))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        fixture(|| self.0.list(dir, visit))
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        fixture(|| self.0.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        fixture(|| self.0.rename(from, to))
    }
}

fn word(name: &str) -> Word {
    Word {
        word: name.into(),
        language: "en".into(),
        section: 3,
        offset: 40,
        path: "books/a.epub".into(),
        title: "A".into(),
        sentence: "The \"quire\" is here.".into(),
    }
}

/// A list of `n` words, `w0` the oldest.
fn list(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        word(&alloc::format!("w{i}")).push_line(&mut out).unwrap();
    }
    out
}

/// Refuses each allocation of `run` in turn on a fresh storage made by
/// `make`: with none refused it must be `Ok`; with one refused it must be
/// `Err(TooLarge)` or `Ok` (an allocation that is optional), and then the
/// storage must have kept the list whole (`intact`, which holds for the
/// list as it was and as the call makes it).
fn sweep<T: core::fmt::Debug>(
    name: &str,
    make: impl Fn() -> Quiet,
    run: impl Fn(&Quiet) -> Result<T, StorageError>,
    intact: impl Fn(&Quiet),
) {
    let storage = fixture(&make);
    let (whole, count) = armed(usize::MAX, || run(&storage));
    whole.unwrap_or_else(|error| panic!("{name}: {error:?} with nothing refused"));
    assert!(
        count > 0,
        "{name} allocates nothing: the sweep proves nothing"
    );
    for at in 1..=count {
        let storage = fixture(&make);
        let (result, _) = armed(at, || run(&storage));
        match result {
            Ok(_) | Err(StorageError::TooLarge) => {}
            Err(error) => panic!("{name}: refusing allocation {at}: {error:?}"),
        }
        intact(&storage);
    }
}

fn words(storage: &Quiet) -> Vec<String> {
    fixture(|| {
        store::range(storage, 0, usize::MAX)
            .unwrap()
            .into_iter()
            .map(|(_, w)| w.word)
            .collect()
    })
}

#[test]
fn reading_the_words_refuses_cleanly() {
    let make = || Quiet::default().with(store::file(), list(40));
    sweep("count", make, |s| store::count(s), |_| {});
    sweep("range", make, |s| store::range(s, 5, 8), |_| {});
    sweep("summary", make, |s| store::summary(s), |_| {});
}

#[test]
fn adding_a_word_refuses_cleanly_and_leaves_the_list() {
    let before = list(40);
    let make = || Quiet::default().with(store::file(), list(40));
    let (new, known) = (word("new"), word("w3"));
    sweep(
        "add",
        make,
        |s| store::add(s, &new),
        |s| {
            // Whole: the old list, or the old list and the word.
            let now = String::from_utf8(s.get(store::file()).unwrap()).unwrap();
            assert!(now == before || now.starts_with(&before), "{now}");
        },
    );
    // A word already in the list (the list is written anew).
    sweep(
        "add known",
        make,
        |s| store::add(s, &known),
        |s| {
            let now = String::from_utf8(s.get(store::file()).unwrap()).unwrap();
            // Written anew (40 words), untouched, or, where the list could
            // not be read to find the word, only added to (as for a read error).
            assert!(
                now == before || now.lines().count() == 40 || now.starts_with(&before),
                "{now}"
            );
        },
    );
}

#[test]
fn removing_a_word_refuses_cleanly_and_leaves_the_list() {
    let before = list(300);
    let make = || Quiet::default().with(store::file(), list(300));
    let gone = word("w7");
    sweep(
        "remove",
        make,
        |s| store::remove(s, &gone),
        |s| {
            let now = String::from_utf8(s.get(store::file()).unwrap()).unwrap();
            assert!(now == before || now.lines().count() == 299, "{now}");
            assert!(s.get(".paperpaw/plugins/vocabulary/words.tmp").is_none());
        },
    );
}

#[test]
fn exporting_refuses_cleanly() {
    let make = || Quiet::default().with(store::file(), list(40));
    sweep("export", make, |s| store::export(s, "out.tsv"), |_| {});
    assert_eq!(words(&make()).len(), 40);
}

/// Where the heap refuses a line buffer while a word is added again, the
/// word is only appended and the old entry stays (as for a read error): the
/// list then has the word twice. Adding it again once it is no longer the
/// newest removes every older entry: nothing is lost, duplicates heal.
#[test]
fn a_later_add_heals_duplicates_left_by_a_refused_read() {
    let make = || Quiet::default().with(store::file(), list(10));
    let again = word("w3");
    let (_, count) = armed(usize::MAX, || store::add(&make(), &again));
    let mut healed = 0;
    for at in 1..=count {
        let storage = fixture(make);
        let _ = armed(at, || store::add(&storage, &again));
        let text = String::from_utf8(storage.get(store::file()).unwrap()).unwrap();
        if text.lines().filter(|l| l.starts_with("w3\t")).count() < 2 {
            continue;
        }
        healed += 1;
        fixture(|| {
            // Another word, then the word once more: one entry of it.
            store::add(&storage, &word("x")).unwrap();
            store::add(&storage, &again).unwrap();
            let words: Vec<String> = store::range(&storage, 0, usize::MAX)
                .unwrap()
                .into_iter()
                .map(|(_, w)| w.word)
                .collect();
            assert_eq!(words.iter().filter(|w| *w == "w3").count(), 1, "{words:?}");
            assert_eq!(words.last().map(String::as_str), Some("w3"));
            assert_eq!(words.len(), 10 + 1, "{words:?}");
        });
    }
    assert!(
        healed > 0,
        "no refusal left a duplicate: the case proves nothing"
    );
}
