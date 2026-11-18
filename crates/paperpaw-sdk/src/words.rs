//! A word search for plugins: many words and phrases at once, found as
//! whole words in a page's text, without regard to case (ADR 0021).
//!
//! Decorators mark words of a page (see [`decorations`](crate::decorations)):
//! words being learnt, the names of characters, terms of a glossary. A
//! [`WordSet`] holds such a list and finds its entries among the words of
//! a page ([`WordSet::find`], [`WordSet::decorate`]), so plugins do not
//! each write their own search.
//!
//! - A word is a run of letters and digits; an apostrophe inside a word
//!   belongs to it (`don't`), as do combining accents (U+0300 to U+036F)
//!   after a letter. Everything else (spaces, punctuation, dashes, quotes)
//!   only separates words, so `Alice,` and `(Rabbit)` are found as `alice`
//!   and `rabbit`, and `well-known` is the two words `well known`.
//! - Words are compared in lowercase (`char::to_lowercase`), with `ß` as
//!   `ss`, as Unicode's case folding has it: `Straße` and `STRASSE` are the
//!   same word. Accents count: `schon` and `schön` are not.
//! - An entry of several words (a phrase) is found across as many words
//!   of the page; where entries overlap, the longest wins.
//!
//! Entries are kept folded, sorted, in one string: a set of two thousand
//! words takes little more than their letters.

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use crate::Id;
use crate::decorations::{Decorated, PageWord};

/// Words and phrases to find (see the [module documentation](self)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordSet {
    /// The entries, folded, sorted and each once, one after the other.
    text: String,
    /// Where each entry begins in `text`, and after them its end.
    starts: Vec<u32>,
}

impl WordSet {
    /// The set of `entries`, each a word or a phrase of several words.
    /// Entries without a word in them (only punctuation) are left out,
    /// and entries that fold to the same words are kept once.
    pub fn new<S: AsRef<str>>(entries: impl IntoIterator<Item = S>) -> Self {
        let mut raw = String::new();
        let mut spans: Vec<(u32, u32)> = Vec::new();
        for entry in entries {
            let from = raw.len();
            if fold_into(entry.as_ref(), &mut raw, from) == 0 {
                continue;
            }
            let (Ok(start), Ok(end)) = (u32::try_from(from), u32::try_from(raw.len())) else {
                // Beyond what offsets hold: no device has that much.
                raw.truncate(from);
                break;
            };
            spans.push((start, end));
        }
        let slice = |&(start, end): &(u32, u32)| &raw[start as usize..end as usize];
        paperpaw_sort::sort_by(&mut spans, |a, b| slice(a).cmp(slice(b)));
        spans.dedup_by(|a, b| slice(a) == slice(b));
        let mut text = String::with_capacity(spans.iter().map(|s| slice(s).len()).sum());
        let mut starts = Vec::with_capacity(spans.len() + 1);
        for span in &spans {
            // Not beyond `raw`, whose offsets fit.
            starts.push(text.len() as u32);
            text.push_str(slice(span));
        }
        if !spans.is_empty() {
            starts.push(text.len() as u32);
        }
        Self { text, starts }
    }

    /// How many entries there are.
    pub fn len(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entries, folded (see [`fold`]), in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &str> + '_ {
        (0..self.len()).map(|n| self.entry(n))
    }

    /// Whether `word` (or a phrase) is an entry, whatever its case and
    /// the punctuation around it.
    pub fn contains(&self, word: &str) -> bool {
        let key = fold(word);
        !key.is_empty() && self.has(&key)
    }

    /// The runs of consecutive `words` that are entries, as ranges of
    /// their indices, in order and not overlapping. Where entries
    /// overlap, the longest wins, and the earlier of two the same length.
    pub fn find(&self, words: &[PageWord<'_>]) -> Vec<Range<usize>> {
        let mut found = Vec::new();
        if self.is_empty() {
            return found;
        }
        let mut key = String::new();
        let mut first = 0;
        while first < words.len() {
            key.clear();
            let mut longest = None;
            for (last, word) in words.iter().enumerate().skip(first) {
                if fold_into(word.text, &mut key, 0) == 0 {
                    // Only punctuation: no entry goes across it.
                    break;
                }
                if self.has(&key) {
                    longest = Some(last + 1);
                }
                if !self.has_longer(&mut key) {
                    break;
                }
            }
            match longest {
                Some(end) => {
                    found.push(first..end);
                    first = end;
                }
                None => first += 1,
            }
        }
        found
    }

    /// What [`find`](Self::find) finds in `words`, as ranges in `role`
    /// for a decorator to give back.
    pub fn decorate(&self, words: &[PageWord<'_>], role: &Id) -> Vec<Decorated> {
        self.find(words)
            .into_iter()
            .filter_map(|range| Decorated::words(&words[range], role))
            .collect()
    }

    fn entry(&self, n: usize) -> &str {
        &self.text[self.starts[n] as usize..self.starts[n + 1] as usize]
    }

    /// The number of the first entry not before `key`.
    fn position(&self, key: &str) -> usize {
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let middle = low + (high - low) / 2;
            if self.entry(middle) < key {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    /// Whether the folded `key` is an entry.
    fn has(&self, key: &str) -> bool {
        let at = self.position(key);
        at < self.len() && self.entry(at) == key
    }

    /// Whether an entry begins with the folded `key` and more words. As
    /// entries hold nothing that sorts before a space, those beginning
    /// with `key` and a space come first among those not before it.
    fn has_longer(&self, key: &mut String) -> bool {
        key.push(' ');
        let at = self.position(key);
        let more = at < self.len() && self.entry(at).starts_with(key.as_str());
        key.pop();
        more
    }
}

impl<S: AsRef<str>> FromIterator<S> for WordSet {
    fn from_iter<I: IntoIterator<Item = S>>(entries: I) -> Self {
        Self::new(entries)
    }
}

/// `text` as entries are kept and compared: its words in lowercase (`ß`
/// as `ss`), one space between them, without the punctuation around
/// them; empty where it holds no word.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    fold_into(text, &mut out, 0);
    out
}

/// Appends the words of `text`, folded, to `out`, with a space before
/// each unless `out` is no longer than `from`; gives the number of words.
fn fold_into(text: &str, out: &mut String, from: usize) -> usize {
    let mut words = 0;
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_alphanumeric() || (in_word && is_accent(c)) {
            if !in_word {
                if out.len() > from {
                    out.push(' ');
                }
                in_word = true;
                words += 1;
            }
            for lower in c.to_lowercase() {
                if lower == 'ß' {
                    out.push_str("ss");
                } else {
                    out.push(lower);
                }
            }
        } else if in_word && is_apostrophe(c) && chars.peek().is_some_and(|n| n.is_alphanumeric()) {
            out.push('\'');
        } else {
            in_word = false;
        }
    }
    words
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '\u{02BC}')
}

/// A combining accent, as in text whose letters are not composed.
fn is_accent(c: char) -> bool {
    ('\u{0300}'..='\u{036F}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Locator;

    fn page<'a>(texts: &[&'a str]) -> Vec<PageWord<'a>> {
        let mut at = 0;
        texts
            .iter()
            .map(|text| {
                let start = Locator::new(0, at);
                at += text.len() as u32;
                let word = PageWord {
                    text,
                    start,
                    end: Locator::new(0, at),
                };
                at += 1;
                word
            })
            .collect()
    }

    #[test]
    fn folds_case_punctuation_and_apostrophes() {
        assert_eq!(fold("  (Alice), "), "alice");
        assert_eq!(fold("Don\u{2019}t STOP!"), "don't stop");
        assert_eq!(fold("'quoted'"), "quoted");
        assert_eq!(fold("well-known"), "well known");
        assert_eq!(fold("..."), "");
    }

    #[test]
    fn whole_words_without_regard_to_case() {
        let set = WordSet::new(["Rabbit", "don't"]);
        assert!(set.contains("rabbit"));
        assert!(set.contains("RABBIT,"));
        assert!(set.contains("Don't"));
        assert!(!set.contains("rabbits"));
        assert!(!set.contains("rab"));
        assert!(!set.contains("don"));
        let words = page(&["The", "(Rabbit)", "rabbits", "RABBIT.", "Don't!"]);
        assert_eq!(set.find(&words), [1..2, 3..4, 4..5]);
    }

    #[test]
    fn phrases_across_words_and_the_longest_wins() {
        let set = WordSet::new([
            "White Rabbit",
            "white",
            "Mad Hatter",
            "the mad hatter's tea",
        ]);
        let words = page(&[
            "The", "white", "rabbit,", "a", "white", "cat;", "the", "Mad", "Hatter.",
        ]);
        assert_eq!(set.find(&words), [1..3, 4..5, 7..9]);
        // A phrase is not found across punctuation that stands alone.
        assert!(set.find(&page(&["Mad", "\u{2014}", "Hatter"])).is_empty());
        // Part of a longer entry is no match.
        assert!(set.find(&page(&["the", "mad", "hatter's"])).is_empty());
        assert_eq!(
            set.find(&page(&["The", "Mad", "Hatter's", "tea!"])),
            alloc::vec![0..4]
        );
    }

    #[test]
    fn umlauts_and_sharp_s() {
        let set = WordSet::new(["Straße", "Äpfel", "schön"]);
        assert!(set.contains("STRASSE"));
        assert!(set.contains("strasse"));
        assert!(set.contains("STRA\u{1E9E}E"));
        assert!(set.contains("ÄPFEL"));
        assert!(set.contains("Schön."));
        assert!(!set.contains("schon"));
        // Accents not composed stay with their word.
        assert_eq!(fold("scho\u{308}n"), "scho\u{308}n");
    }

    #[test]
    fn an_empty_set_finds_nothing() {
        let set = WordSet::new(Vec::<&str>::new());
        assert!(set.is_empty());
        assert!(!set.contains("word"));
        assert!(!set.contains(""));
        assert!(set.find(&page(&["a", "word"])).is_empty());
        let punctuation = WordSet::new(["", "--", "..."]);
        assert!(punctuation.is_empty());
        assert_eq!(WordSet::default(), set);
    }

    #[test]
    fn entries_kept_once_and_sorted() {
        let set: WordSet = ["ink", "Quire", "INK", "quire!"].into_iter().collect();
        assert_eq!(set.len(), 2);
        assert_eq!(set.iter().collect::<Vec<_>>(), ["ink", "quire"]);
    }

    #[test]
    fn decorates_with_the_role() {
        let role = Id::new_static("demo.role");
        let set = WordSet::new(["white rabbit"]);
        let words = page(&["A", "White", "Rabbit!"]);
        let marks = set.decorate(&words, &role);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].start, words[1].start);
        assert_eq!(marks[0].end, words[2].end);
        assert_eq!(marks[0].role, role);
    }
}
