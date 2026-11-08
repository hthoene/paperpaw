//! Words being learnt, marked where they occur in books: a decorator
//! (ADR 0021) finds the words of the list on each page, in the book's
//! language, and marks them in the role `vocabulary.learning`; a tap on
//! one looks it up.
//!
//! The words are read from the file only when they changed, into a
//! [`WordSet`] for the language of the book being read. Whether they did
//! is known without reading anything ([`store::changes`]): readers ask
//! as they draw a page.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, params, slots};
use paperpaw_sdk::decorations::{Decorated, DecoratorSpec, PageText};
use paperpaw_sdk::words::{WordSet, fold};
use paperpaw_sdk::{Command, Commands, Env, Id, Params, Storage};

use crate::store::{self, Changes};
use crate::{LEARNING, MARK};

/// The decorator.
pub(crate) const DECORATOR: Id = Id::new_static("vocabulary.words");

/// The words of a language, as they were after a number of changes.
pub(crate) type Cache = RefCell<Option<(u64, String, WordSet)>>;

/// The decorator, switched by [`MARK`], hearing taps.
pub(crate) fn decorator(changes: &Changes) -> DecoratorSpec {
    let cache: Cache = RefCell::new(None);
    let (marks, signed) = (changes.clone(), changes.clone());
    DecoratorSpec::new(DECORATOR, "vocabulary.decorator", move |env, page| {
        decorate(&cache, env.storage(), page, &marks)
    })
    .switch(&MARK)
    .taps()
    .signature(move |_| signature(&signed))
}

/// What the marks depend on: the changes made to the words.
pub(crate) fn signature(changes: &Changes) -> u64 {
    u64::from(changes.count())
}

/// The marks of `page`: the words of the list in its language.
pub(crate) fn decorate(
    cache: &Cache,
    storage: &dyn Storage,
    page: &PageText<'_>,
    changes: &Changes,
) -> Vec<Decorated> {
    let Ok(mut cache) = cache.try_borrow_mut() else {
        return Vec::new();
    };
    let signature = signature(changes);
    let language = primary(page.language);
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
    match &*cache {
        Some((_, _, words)) => words.decorate(page.words, &LEARNING),
        None => Vec::new(),
    }
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

/// A tap on a marked word: looks it up, in the language it was kept in.
pub(crate) fn tapped(env: &Env<'_>, tapped: &events::DecorationTapped, out: &mut Commands) {
    if tapped.decorator != DECORATOR {
        return;
    }
    let word = tapped.text.trim_matches(|c: char| !c.is_alphanumeric());
    if word.is_empty() {
        return;
    }
    out.push(Command::OpenSlot(
        slots::LOOKUP,
        Params::new()
            .with(params::WORD, word.to_string())
            .with(params::LANGUAGE, language_of(env.storage(), word)),
    ));
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
    use paperpaw_sdk::decorations::PageWord;
    use paperpaw_sdk::document::Locator;

    const WORDS: &str = "Rabbit\ten\t1\t10\tbooks/alice.txt\tAlice\tThe White Rabbit.\n\
                         white rabbit\ten-GB\t1\t10\tbooks/alice.txt\tAlice\t\n\
                         Hase\tde\t1\t10\tbooks/hase.txt\tHase\t\n\
                         Hatter\t\t1\t10\tbooks/alice.txt\tAlice\t\n";

    fn words<'a>(texts: &[&'a str]) -> Vec<PageWord<'a>> {
        texts
            .iter()
            .enumerate()
            .map(|(n, text)| {
                PageWord::new(
                    text,
                    Locator::new(0, n as u32 * 10),
                    Locator::new(0, n as u32 * 10 + text.len() as u32),
                )
            })
            .collect()
    }

    fn page<'a>(language: &'a str, words: &'a [PageWord<'a>]) -> PageText<'a> {
        PageText::new("books/alice.txt", language, words)
    }

    #[test]
    fn marks_the_words_of_the_list() {
        let storage = MemoryStorage::new().with(store::file(), WORDS);
        let cache = Cache::default();
        let changes = Changes::default();
        let text = words(&[
            "The",
            "White",
            "Rabbit,",
            "the",
            "rabbit",
            "and",
            "(Hatter).",
        ]);
        let marks = decorate(&cache, &storage, &page("EN", &text), &changes);
        let ranges: Vec<_> = marks.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(
            ranges,
            [
                (text[1].start, text[2].end),
                (text[4].start, text[4].end),
                (text[6].start, text[6].end),
            ]
        );
        assert!(marks.iter().all(|m| m.role == LEARNING));
    }

    #[test]
    fn skips_words_of_other_languages() {
        let storage = MemoryStorage::new().with(store::file(), WORDS);
        let cache = Cache::default();
        let changes = Changes::default();
        let text = words(&["Der", "Hase", "und", "das", "Rabbit"]);
        let marks = decorate(&cache, &storage, &page("de", &text), &changes);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].start, text[1].start);
        // A book of no known language marks every word.
        assert_eq!(
            decorate(&cache, &storage, &page("", &text), &changes).len(),
            2
        );
    }

    #[test]
    fn reads_the_list_again_when_it_changed() {
        let storage = MemoryStorage::new().with(store::file(), WORDS);
        let cache = Cache::default();
        let changes = Changes::default();
        let text = words(&["a", "quire", "of", "ink"]);
        assert!(decorate(&cache, &storage, &page("en", &text), &changes).is_empty());
        let before = signature(&changes);
        let quire = store::Word {
            word: "quire".to_string(),
            language: "en".to_string(),
            ..store::Word::default()
        };
        store::add(&storage, &quire).unwrap();
        changes.changed();
        assert_ne!(signature(&changes), before);
        assert_eq!(
            decorate(&cache, &storage, &page("en", &text), &changes).len(),
            1
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
