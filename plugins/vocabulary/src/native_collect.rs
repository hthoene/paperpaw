//! Keeping the words looked up: what the reader tells through the stage
//! `reader.word-looked-up` (schema 1, `stages/subscribed/`) goes into the list of
//! words, and, where wanted, becomes a card of the deck *Looked up* later.
//! Host-free over [`Storage`]; the shell decodes the call and tells the reader
//! when the word could not be kept.
use alloc::string::String;

use paperpaw_sdk::{Storage, StorageError};

use crate::stages_subscribed::reader_word_looked_up_v1::WordLooked;
use crate::store::{self, Word};
use crate::{cards, lines};

/// A copy of `text`, its room asked for.
fn copy(text: &str) -> Result<String, StorageError> {
    let mut out = String::new();
    lines::push(&mut out, text)?;
    Ok(out)
}

/// Keeps the word of `looked`, newest first, if the reader wants words kept
/// (`collect`); with `from_lookups` it also queues a card (its front the word,
/// its example the sentence), which the decks' screen turns into a card the
/// next time it opens. A word that is only blanks is kept by nobody. The word is
/// kept whether or not the card could be queued.
///
/// # Errors
/// What keeping the word says (a refused allocation is
/// [`StorageError::TooLarge`]).
pub(crate) fn keep(
    storage: &dyn Storage,
    collect: bool,
    from_lookups: bool,
    looked: &WordLooked<'_>,
) -> Result<(), StorageError> {
    if !collect {
        return Ok(());
    }
    let word = looked.word.trim();
    if word.is_empty() {
        return Ok(());
    }
    let result = store::add(
        storage,
        &Word {
            word: copy(word)?,
            language: copy(looked.language)?,
            section: looked.section,
            offset: looked.offset,
            path: copy(looked.path)?,
            title: copy(looked.title)?,
            sentence: copy(looked.sentence)?,
        },
    );
    // A card to be: a line appended, the deck not read now.
    if from_lookups {
        let _ = cards::queue_lookup(storage, word, looked.sentence);
    }
    result
}

#[cfg(test)]
#[path = "native_collect_tests.rs"]
mod tests;
