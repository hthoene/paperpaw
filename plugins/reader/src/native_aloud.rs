//! What reading aloud decides without the host, on interface 7: the timer's
//! choices, the sentence a word ends, the sentence from a word on, and the
//! text as it is spoken. The same rules as `aloud.rs`; allocation is fallible.
use alloc::string::String;

use paperpaw_sdk::native::Error;

/// The most words spoken at once, for sentences without an end.
pub const MAX_WORDS: usize = 80;

/// Minutes until reading aloud stops by itself, for the timer's `choice`
/// (`reader.aloud-timer.30`, or `…off`): none for a choice that is no number.
#[must_use]
pub fn timer_minutes(choice: &str) -> Option<u64> {
    choice
        .rsplit('.')
        .next()
        .and_then(|m| m.parse().ok())
        .filter(|&m| m > 0)
}

/// Whether `word` ends a sentence (closing quotes and brackets after its
/// punctuation are part of it).
#[must_use]
pub fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', '»', '«', '›', '‹', ')', ']', '“'])
        .ends_with(['.', '!', '?', '…'])
}

/// The end (exclusive) of the sentence that starts at word `from`.
#[must_use]
pub fn sentence_end(words: &[&str], from: usize) -> usize {
    let last = words.len().min(from.saturating_add(MAX_WORDS));
    (from..last)
        .find(|&i| ends_sentence(words[i]))
        .map_or(last, |i| i + 1)
}

/// The text of words as it is spoken, each with whether it starts a line:
/// words that hyphenation split at a line end are joined again.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn spoken<'a>(words: impl Iterator<Item = (&'a str, bool)>) -> Result<String, Error> {
    let mut text = String::new();
    for (word, new_line) in words {
        let mut before = text.chars().rev();
        let split = new_line
            && before.next() == Some('-')
            && before.next().is_some_and(char::is_alphabetic)
            && word.chars().next().is_some_and(char::is_lowercase);
        text.try_reserve(word.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        if split {
            text.pop();
        } else if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(word);
    }
    Ok(text)
}
