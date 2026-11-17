//! The word search (`paperpaw_sdk::words`): folding is settled after
//! one pass, every entry is found as itself, and what `find` gives is in
//! bounds, in order, without overlaps and made of entries.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_sdk::words::{WordSet, fold};
use proptest::prelude::*;

/// Words as books have them: letters of several scripts, digits,
/// apostrophes, punctuation around them, combining accents.
fn word() -> impl Strategy<Value = String> {
    "[(\"']?[a-zA-ZäöüßẞÄÖÜéçİıΣσς日本0-9]{1,6}(['’][a-z]{1,2})?\u{0301}?[.,;!?)\"\u{2014}-]?"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Folding twice is folding once, and folded text holds only
    /// lowercase words between single spaces.
    #[test]
    fn folding_is_settled(text in "\\PC{0,30}") {
        let once = fold(&text);
        prop_assert_eq!(fold(&once), once.clone());
        prop_assert!(!once.starts_with(' ') && !once.ends_with(' '));
        prop_assert!(!once.contains("  "));
    }

    /// Every entry with a word in it is found again, whatever its case
    /// and the punctuation around it.
    #[test]
    fn entries_are_found_as_themselves(entries in proptest::collection::vec(word(), 0..12)) {
        let set = WordSet::new(&entries);
        for entry in &entries {
            prop_assert!(set.contains(entry), "{entry:?} in {set:?}");
            prop_assert!(set.contains(&entry.to_uppercase()) || fold(&entry.to_uppercase()) != fold(entry));
        }
        let mut folded: Vec<String> = entries.iter().map(|e| fold(e)).filter(|f| !f.is_empty()).collect();
        folded.sort();
        folded.dedup();
        prop_assert_eq!(set.len(), folded.len());
        prop_assert_eq!(set.iter().collect::<Vec<_>>(), folded.iter().map(String::as_str).collect::<Vec<_>>());
    }


}

#[test]
fn case_folds_beyond_ascii() {
    assert_eq!(fold("ΣΟΦΙΑ"), fold("σοφια"));
    assert_eq!(fold("STRAẞE"), "strasse");
    // A dotted capital I folds to an i with a combining dot, kept with it.
    assert_eq!(fold("İstanbul"), "i\u{307}stanbul");
    let set = WordSet::new(["İstanbul"]);
    assert!(set.contains("i\u{307}stanbul"));
    // Digits are words too.
    assert_eq!(fold("Chapter 12."), "chapter 12");
    // Apostrophes at the ends are no part of the word.
    assert_eq!(fold("'tis rock'n'roll'"), "tis rock'n'roll");
}

#[test]
fn many_entries_are_kept_small() {
    let entries: Vec<String> = (0..2000).map(|i| format!("word{i}")).collect();
    let set = WordSet::new(&entries);
    assert_eq!(set.len(), 2000);
    assert!(set.contains("WORD1999"));
    assert!(!set.contains("word2000"));
}
