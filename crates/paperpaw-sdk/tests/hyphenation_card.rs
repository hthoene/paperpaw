//! The hyphenation tables of data plugins, read from the card: which
//! plugin has a language (D4), which tag a book's language finds, and how a
//! table that cannot be read is opened once more and then given up (B3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::hyphen::{Patterns, compile};
use paperpaw_sdk::{Hyphenation, HyphenationSpec, Id, MemoryStorage};

/// Patterns that break before every `b`, 1 letter kept on each side, and
/// after every pair of the letters `c` to `z`: 577 patterns in 37 blocks,
/// more than a reader keeps, so a word of other letters reads the card.
fn table() -> Vec<u8> {
    let mut patterns = String::from("1b");
    for first in 'c'..='z' {
        for second in 'c'..='z' {
            patterns.push_str(&format!(" {first}{second}1"));
        }
    }
    compile(&patterns, "", 1, 1)
}

/// What the table in memory says of `word`.
fn in_memory(word: &str) -> Vec<usize> {
    let data = table();
    let mut found = Vec::new();
    Patterns::new(&data)
        .unwrap()
        .hyphenate(word, &mut |at| found.push(at));
    found
}

/// A card: memory storage that counts sizes asked (one per table opened)
/// and fails the next `fail` reads.
#[derive(Default)]
struct Card {
    files: MemoryStorage,
    fail: Cell<u32>,
    opens: Cell<u32>,
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if self.fail.get() > 0 {
            self.fail.set(self.fail.get() - 1);
            return Err(StorageError::Io);
        }
        self.files.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.opens.set(self.opens.get() + 1);
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }
}

/// A registry reading from `card`, with `plugin`'s `tables` (tag, file).
fn registry(card: &Rc<Card>, plugins: &[(&str, &[(&str, &str)])]) -> Hyphenation {
    let mut h = Hyphenation::new();
    h.set_storage(card.clone());
    for (plugin, tables) in plugins {
        h.add_tables(plugin, &format!("plugins/{plugin}"), tables.iter().copied())
            .unwrap();
    }
    h
}

fn breaks(h: &Hyphenation, language: &str, word: &str) -> Option<Vec<usize>> {
    let hyphenator = h.get(language)?;
    let mut found = Vec::new();
    hyphenator.hyphenate(word, &mut |at| found.push(at));
    Some(found)
}

fn card_with(files: &[(&str, &[u8])]) -> Rc<Card> {
    let card = Card::default();
    for (path, data) in files {
        card.files.write(path, data).unwrap();
    }
    Rc::new(card)
}

fn found(h: &Hyphenation, tag: &str) -> Option<(String, String)> {
    h.card_table(tag)
        .map(|t| (t.plugin().to_string(), t.path().to_string()))
}

#[test]
fn the_greatest_id_has_a_language_in_any_order() {
    let card = card_with(&[]);
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0], [0, 2, 1]] {
        let mut h = registry(&card, &[]);
        let mut left_out = Vec::new();
        for i in order {
            let (plugin, file) = match i {
                0 => ("hyphenation", "hyphenation/en.pphp"),
                1 => ("hyphenation-en-gb", "hyphenation/en-gb.pphp"),
                _ => ("en-patterns", "en.pphp"),
            };
            h.add_tables(
                plugin,
                &format!("plugins/{plugin}"),
                [("en", file)].into_iter(),
            )
            .unwrap();
            left_out.extend(h.collisions(plugin).map(|(lost, _)| lost.name()));
        }
        assert_eq!(
            found(&h, "en"),
            Some((
                "hyphenation-en-gb".into(),
                "plugins/hyphenation-en-gb/hyphenation/en-gb.pphp".into()
            )),
            "order {order:?}"
        );
        left_out.sort();
        left_out.dedup();
        assert_eq!(left_out, ["en-patterns:en", "hyphenation:en"], "{order:?}");
    }
}

#[test]
fn ids_compare_in_lowercase() {
    let card = card_with(&[]);
    let h = registry(
        &card,
        &[
            ("Hyphenation-A", &[("en", "a")]),
            ("hyphenation-b", &[("en", "b")]),
        ],
    );
    assert_eq!(found(&h, "en").unwrap().0, "hyphenation-b");
    let h = registry(
        &card,
        &[
            ("hyphenation", &[("en", "a")]),
            ("hyphenation-", &[("en", "b")]),
        ],
    );
    assert_eq!(found(&h, "en").unwrap().0, "hyphenation-");
}

#[test]
fn a_plugin_taken_out_gives_its_language_back_and_only_its_own() {
    let card = card_with(&[]);
    let mut h = registry(
        &card,
        &[
            ("hyphenation", &[("en", "en.pphp")]),
            ("hyphenation-en-gb", &[("en", "gb.pphp")]),
            ("hyphenation-nb", &[("nb", "nb.pphp"), ("no", "nb.pphp")]),
        ],
    );
    assert_eq!(found(&h, "en").unwrap().0, "hyphenation-en-gb");
    h.remove_tables("hyphenation-en-gb");
    assert_eq!(found(&h, "en-GB").unwrap().1, "plugins/hyphenation/en.pphp");
    h.remove_tables("hyphenation-nb");
    assert!(found(&h, "nb").is_none() && found(&h, "no").is_none());
    assert_eq!(found(&h, "en").unwrap().0, "hyphenation");
}

#[test]
fn a_book_language_falls_back_to_shorter_tags_only() {
    let card = card_with(&[]);
    let h = registry(
        &card,
        &[(
            "hyphenation-de",
            &[("de", "de.pphp"), ("DE-CH", "ch.pphp"), ("nb", "nb.pphp")],
        )],
    );
    assert_eq!(
        found(&h, "de-CH-1996").unwrap().1,
        "plugins/hyphenation-de/ch.pphp"
    );
    assert_eq!(
        found(&h, "de_ch").unwrap().1,
        "plugins/hyphenation-de/ch.pphp"
    );
    assert_eq!(
        found(&h, "de_AT").unwrap().1,
        "plugins/hyphenation-de/de.pphp"
    );
    assert_eq!(found(&h, "DE").unwrap().1, "plugins/hyphenation-de/de.pphp");
    for none in ["no", "d", "", "-", "_", "ü-de", "nbx"] {
        assert!(found(&h, none).is_none(), "{none}");
    }
}

#[test]
fn a_table_on_the_card_hyphenates_as_in_memory() {
    let data = table();
    let card = card_with(&[("plugins/hyphenation/en.pphp", &data)]);
    let h = registry(&card, &[("hyphenation", &[("en", "en.pphp")])]);
    let patterns = Patterns::new(&data).unwrap();
    for word in [
        "abab", "ababab", "bbbb", "a", "", "Abababa", "zyzyx", "cdefghij",
    ] {
        let mut want = Vec::new();
        patterns.hyphenate(word, &mut |at| want.push(at));
        assert_eq!(breaks(&h, "en-US", word).unwrap(), want, "{word}");
        // Other minimums for this hyphenator alone.
        let mut want = Vec::new();
        patterns
            .with_minimums(2, 3)
            .hyphenate(word, &mut |at| want.push(at));
        let mut got = Vec::new();
        h.get("en")
            .unwrap()
            .with_minimums(2, 3)
            .hyphenate(word, &mut |at| got.push(at));
        assert_eq!(got, want, "{word}");
    }
    assert_eq!(breaks(&h, "en", "abab").unwrap(), [1, 3]);
    // Opened once, kept open.
    assert_eq!(card.opens.get(), 1);
}

#[test]
fn the_card_comes_before_a_table_compiled_into_a_plugin() {
    let card = card_with(&[("plugins/hyphenation/en.pphp", &table())]);
    let mut h = registry(&card, &[("hyphenation", &[("en", "en.pphp")])]);
    // Breaks before every `a` instead.
    let compiled = Box::leak(compile("1a", "", 1, 1).into_boxed_slice());
    for (id, language) in [("compiled.en", "en"), ("compiled.fr", "fr")] {
        h.add(HyphenationSpec::new(Id::new_static(id), language, compiled))
            .unwrap();
    }
    assert_eq!(breaks(&h, "en", "baba").unwrap(), [2]);
    assert_eq!(breaks(&h, "fr", "baba").unwrap(), [1, 3]);
    assert!(breaks(&h, "de", "baba").is_none());
}

/// B3: a read that fails once (a card that answered late) leaves that word
/// whole; the next word opens the table again. A second failure gives the
/// language up, until the card changes.
#[test]
fn a_failed_read_opens_the_table_once_more_then_gives_it_up() {
    let card = card_with(&[("plugins/hyphenation/en.pphp", &table())]);
    let h = registry(&card, &[("hyphenation", &[("en", "en.pphp")])]);
    assert_eq!(breaks(&h, "en", "abab").unwrap(), [1, 3]);
    // `zy` is in a block not read yet.
    assert!(!in_memory("zyzy").is_empty());
    card.fail.set(1);
    assert_eq!(breaks(&h, "en", "zyzy").unwrap(), Vec::<usize>::new());
    assert_eq!(breaks(&h, "en", "zyzy").unwrap(), in_memory("zyzy"));
    assert_eq!(card.opens.get(), 2);
    // Failing again: given up, and the language is none from then on.
    assert!(!in_memory("wxwx").is_empty());
    card.fail.set(1);
    assert_eq!(breaks(&h, "en", "wxwx").unwrap(), Vec::<usize>::new());
    assert_eq!(breaks(&h, "en", "abab"), None);
    assert!(h.card_table("en").is_none());
    let opens = card.opens.get();
    // The card changed (mounted anew): tried again.
    h.card_changed();
    assert_eq!(breaks(&h, "en", "abab").unwrap(), [1, 3]);
    assert_eq!(card.opens.get(), opens + 1);
}

#[test]
fn a_failed_open_counts_as_a_failure_too() {
    let card = card_with(&[("plugins/hyphenation/en.pphp", &table())]);
    let h = registry(&card, &[("hyphenation", &[("en", "en.pphp")])]);
    card.fail.set(1);
    assert_eq!(breaks(&h, "en", "abab").unwrap(), Vec::<usize>::new());
    assert_eq!(breaks(&h, "en", "abab").unwrap(), [1, 3]);
    // Every read failing: two opens, then nothing more is tried.
    let h = registry(&card, &[("hyphenation", &[("en", "en.pphp")])]);
    card.fail.set(u32::MAX);
    let opens = card.opens.get();
    for _ in 0..5 {
        let _ = breaks(&h, "en", "abab");
    }
    assert_eq!(card.opens.get(), opens + 2);
    assert!(h.get("en").is_none());
}

/// A table that is not sound, or not there, is given up at once; another
/// plugin's table for the language serves instead.
#[test]
fn a_table_not_sound_or_missing_gives_way_to_the_next() {
    let card = card_with(&[
        ("plugins/hyphenation/en.pphp", &table()),
        ("plugins/hyphenation-x/en.pphp", b"PPHP not a table"),
    ]);
    let h = registry(
        &card,
        &[
            ("hyphenation", &[("en", "en.pphp")]),
            ("hyphenation-x", &[("en", "en.pphp")]),
            ("hyphenation-y", &[("en", "missing.pphp")]),
        ],
    );
    assert_eq!(found(&h, "en").unwrap().0, "hyphenation-y");
    // Missing, then not sound: each tried once.
    assert_eq!(breaks(&h, "en", "abab").unwrap(), Vec::<usize>::new());
    assert_eq!(found(&h, "en").unwrap().0, "hyphenation-x");
    assert_eq!(breaks(&h, "en", "abab").unwrap(), Vec::<usize>::new());
    assert_eq!(found(&h, "en").unwrap().0, "hyphenation");
    assert_eq!(breaks(&h, "en", "abab").unwrap(), [1, 3]);
    assert_eq!(card.opens.get(), 3);
}

/// At most two tables are open; the least recently used is closed first.
#[test]
fn two_tables_are_open_at_most() {
    let data = table();
    let card = card_with(&[
        ("plugins/h/de.pphp", &data),
        ("plugins/h/en.pphp", &data),
        ("plugins/h/fr.pphp", &data),
    ]);
    let h = registry(
        &card,
        &[(
            "h",
            &[("de", "de.pphp"), ("en", "en.pphp"), ("fr", "fr.pphp")],
        )],
    );
    for language in ["de", "en", "de", "en", "de"] {
        assert_eq!(breaks(&h, language, "abab").unwrap(), [1, 3]);
    }
    assert_eq!(card.opens.get(), 2);
    // `fr` closes `en`, used longest ago; `de` stays open.
    assert_eq!(breaks(&h, "fr", "abab").unwrap(), [1, 3]);
    assert_eq!(breaks(&h, "de", "abab").unwrap(), [1, 3]);
    assert_eq!(card.opens.get(), 3);
    assert_eq!(breaks(&h, "en", "abab").unwrap(), [1, 3]);
    assert_eq!(card.opens.get(), 4);
}

#[test]
fn a_hyphenator_of_a_removed_plugin_hyphenates_nothing() {
    let card = card_with(&[("plugins/hyphenation/en.pphp", &table())]);
    let mut h = registry(&card, &[("hyphenation", &[("en", "en.pphp")])]);
    assert_eq!(breaks(&h, "en", "abab").unwrap(), [1, 3]);
    h.remove_tables("hyphenation");
    assert!(h.get("en").is_none());
    // Added anew (an update): a new table, opened anew.
    h.add_tables(
        "hyphenation",
        "plugins/hyphenation",
        [("en", "en.pphp")].into_iter(),
    )
    .unwrap();
    assert_eq!(breaks(&h, "en", "abab").unwrap(), [1, 3]);
    assert_eq!(card.opens.get(), 2);
}

/// Hyphenating from inside `out` (a caller that hyphenates a word as it is
/// told a point of another) leaves the inner word whole, never panics.
#[test]
fn hyphenating_again_from_out_leaves_the_inner_word_whole() {
    let card = card_with(&[("plugins/hyphenation/en.pphp", &table())]);
    let h = registry(&card, &[("hyphenation", &[("en", "en.pphp")])]);
    let hyphenator = h.get("en").unwrap();
    let mut outer = Vec::new();
    let mut inner = Vec::new();
    hyphenator.hyphenate("abab", &mut |at| {
        outer.push(at);
        hyphenator.hyphenate("abab", &mut |at| inner.push(at));
    });
    assert_eq!(outer, [1, 3]);
    assert!(inner.is_empty());
}
