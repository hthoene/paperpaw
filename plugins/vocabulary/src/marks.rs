//! Words being learnt, marked where they occur in books: **BLOCKED on the
//! reader's decorations stage** (`reader.decorations@1`, Plugin Engineer's;
//! see `PORTING-7.md`). What is ready is what needs no host: the words of the
//! list in the language of the book being read, read from the file only when
//! the words changed (a signature, [`Changes`]), and the language a word was
//! kept in, for the lookup a tap on a marked word makes. The stage's
//! subscription that turns a page's words into marks (role
//! `vocabulary.learning`) and the tap's hand-off (`core.lookup`, #352) are
//! added when those stages exist.

use alloc::string::{String, ToString};
use paperpaw_sdk::Storage;
use paperpaw_sdk::words::{WordSet, fold};

use crate::store::{self, Changes};

/// What the marks depend on: the changes made to the words.
pub(crate) fn signature(changes: &Changes) -> u64 {
    u64::from(changes.count())
}

/// The words of the list for a book in `language`, read again only when the
/// list changed or the language is another.
pub(crate) fn words<'a>(
    cache: &'a mut Option<(u64, String, WordSet)>,
    storage: &dyn Storage,
    language: &str,
    changes: &Changes,
) -> Option<&'a WordSet> {
    let signature = signature(changes);
    let language = primary(language);
    let fresh = matches!(
        &*cache,
        Some((s, kept, _)) if *s == signature && kept.eq_ignore_ascii_case(language)
    );
    if !fresh {
        // The old set goes before the new one is read.
        *cache = None;
        let words = words_in(storage, language);
        *cache = Some((signature, language.to_string(), words));
    }
    cache.as_ref().map(|(_, _, words)| words)
}

/// The language without its region: `en-GB` is `en`.
fn primary(language: &str) -> &str {
    language.split(['-', '_']).next().unwrap_or_default().trim()
}

/// Whether a word kept in `kept` belongs to a book in `language` (both
/// primary, see [`primary`]), whatever their case; where either is not
/// known, it does.
fn same_language(kept: &str, language: &str) -> bool {
    kept.is_empty() || language.is_empty() || kept.eq_ignore_ascii_case(language)
}

/// The words of the list in `language`.
fn words_in(storage: &dyn Storage, language: &str) -> WordSet {
    // One string with a word a line, rather than a string for each.
    let mut words = String::new();
    let _ = store::each_line(storage, store::file(), |line| {
        let mut fields = line.splitn(3, '\t');
        let (Some(word), Some(kept)) = (fields.next(), fields.next()) else {
            return;
        };
        if same_language(primary(kept), language) {
            words.push_str(word);
            words.push('\n');
        }
    });
    WordSet::new(words.lines())
}

/// The language `word` was last kept in; empty where it is not in the
/// list.
fn language_of(storage: &dyn Storage, word: &str) -> String {
    let key = fold(word);
    let mut language = String::new();
    let _ = store::each_line(storage, store::file(), |line| {
        let mut fields = line.splitn(3, '\t');
        if let (Some(kept), Some(kept_language)) = (fields.next(), fields.next())
            && fold(kept) == key
        {
            language = kept_language.to_string();
        }
    });
    language
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    const WORDS: &str = "Rabbit\ten\t1\t10\tbooks/alice.txt\tAlice\tThe White Rabbit.\n\
                         white rabbit\ten-GB\t1\t10\tbooks/alice.txt\tAlice\t\n\
                         Hase\tde\t1\t10\tbooks/hase.txt\tHase\t\n\
                         Hatter\t\t1\t10\tbooks/alice.txt\tAlice\t\n";

    #[test]
    fn the_words_of_the_list_in_the_books_language() {
        let storage = MemoryStorage::new().with(store::file(), WORDS);
        let changes = Changes::default();
        let mut cache = None;
        let set = words(&mut cache, &storage, "EN", &changes).unwrap();
        assert!(set.contains("rabbit") && set.contains("white rabbit"));
        // A word of no known language belongs to every book.
        assert!(set.contains("hatter"));
        assert!(!set.contains("hase"));
    }

    #[test]
    fn words_of_other_languages_are_skipped_and_an_unknown_language_has_all() {
        let storage = MemoryStorage::new().with(store::file(), WORDS);
        let changes = Changes::default();
        let mut cache = None;
        let set = words(&mut cache, &storage, "de", &changes).unwrap();
        assert!(set.contains("hase") && set.contains("hatter"));
        assert!(!set.contains("rabbit"));
        let all = words(&mut cache, &storage, "", &changes).unwrap();
        assert!(all.contains("hase") && all.contains("rabbit"));
    }

    #[test]
    fn the_list_is_read_again_when_it_changed() {
        let storage = MemoryStorage::new().with(store::file(), WORDS);
        let changes = Changes::default();
        let mut cache = None;
        assert!(
            !words(&mut cache, &storage, "en", &changes)
                .unwrap()
                .contains("quire")
        );
        let before = signature(&changes);
        let quire = store::Word {
            word: "quire".to_string(),
            language: "en".to_string(),
            ..store::Word::default()
        };
        store::add(&storage, &quire).unwrap();
        // Nothing told of the change: the set read before is kept.
        assert!(
            !words(&mut cache, &storage, "en", &changes)
                .unwrap()
                .contains("quire")
        );
        changes.changed();
        assert_ne!(signature(&changes), before);
        assert!(
            words(&mut cache, &storage, "en", &changes)
                .unwrap()
                .contains("quire")
        );
    }

    #[test]
    fn a_tap_looks_the_word_up_in_its_language() {
        let storage = MemoryStorage::new().with(store::file(), WORDS);
        assert_eq!(language_of(&storage, "HASE"), "de");
        assert_eq!(language_of(&storage, "rabbit!"), "en");
        assert_eq!(language_of(&storage, "unknown"), "");
    }
}
