//! Interlinear, a PaperPaw sample: a translation line under each sentence of
//! a book, from a text file beside it. What it does without a device: the
//! side file read into a small index ([`Index`]), a block's text cut into
//! sentences ([`sentences`]) and the reader's text stage answered
//! ([`answer`]). `main.rs` is the plugin around it.
//!
//! The side file has one line for each sentence: the sentence as the book
//! has it, a tab, its translation. A sentence is found by its first
//! [`KEY_CHARS`] letters and digits, whatever its punctuation, spaces and
//! case, so the book's quotes and the file's need not agree.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;
use paperpaw_sdk::native::{
    Error,
    page_layers::{GlossKind, MAX_GLOSS, MAX_INSERTED, TextAnswer},
};

/// The letters and digits of a sentence its key is made of.
pub const KEY_CHARS: usize = 40;
/// The most sentences of a side file kept (8 bytes each).
pub const MOST_SENTENCES: usize = 2048;
/// The longest line of a side file read; a longer one is left out.
pub const LONGEST_LINE: usize = 2048;
/// The size of a translation under a line, in percent of the text.
pub const GLOSS_SIZE: u16 = 60;
/// The size of the translations after a paragraph, in percent of the text.
pub const AFTER_SIZE: u16 = 85;

/// The key of `text`: its first [`KEY_CHARS`] letters and digits, lower
/// case, hashed (FNV-1a); 0 for a text without any.
#[must_use]
pub fn key(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    let mut taken = 0;
    for c in text.chars().filter(|c| c.is_alphanumeric()) {
        for lower in c.to_lowercase() {
            let mut bytes = [0; 4];
            for &b in lower.encode_utf8(&mut bytes).as_bytes() {
                hash = (hash ^ u32::from(b)).wrapping_mul(0x0100_0193);
            }
        }
        taken += 1;
        if taken == KEY_CHARS {
            break;
        }
    }
    if taken == 0 { 0 } else { hash.max(1) }
}

/// The sentences of a side file: the key of each and where its translation
/// starts in the file, sorted by key.
#[derive(Debug, Default)]
pub struct Index {
    rows: Vec<(u32, u32)>,
    /// The bytes of the line read so far (one that goes on in the next part).
    line: Vec<u8>,
    /// Where the line starts in the file.
    at: u32,
    /// Bytes of the file read.
    read: u32,
    /// The line read so far is too long: left out up to its end.
    skipping: bool,
}

impl Index {
    /// Reads the next `part` of the file.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(&mut self, part: &[u8]) -> Result<(), Error> {
        for &b in part {
            if b == b'\n' {
                self.end_line()?;
            } else if !self.skipping {
                if self.line.len() >= LONGEST_LINE {
                    self.skipping = true;
                    self.line.clear();
                } else {
                    self.line.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    self.line.push(b);
                }
            }
            self.read = self.read.saturating_add(1);
        }
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), Error> {
        let line = core::mem::take(&mut self.line);
        let skipped = core::mem::replace(&mut self.skipping, false);
        let start = self.at;
        self.at = self.read.saturating_add(1);
        if skipped || line.first() == Some(&b'#') || self.rows.len() >= MOST_SENTENCES {
            return Ok(());
        }
        let Ok(text) = core::str::from_utf8(&line) else {
            return Ok(());
        };
        let Some((sentence, translation)) = text.split_once('\t') else {
            return Ok(());
        };
        let key = key(sentence);
        if key == 0 || translation.trim().is_empty() {
            return Ok(());
        }
        let at = start + sentence.len() as u32 + 1;
        self.rows.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.rows.push((key, at));
        Ok(())
    }

    /// The file is read: its last line too, the keys sorted.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(&mut self) -> Result<(), Error> {
        if !self.line.is_empty() {
            self.end_line()?;
        }
        self.line = Vec::new();
        paperpaw_sdk::sort::sort_unstable_by_key(&mut self.rows, |row| row.0);
        Ok(())
    }

    /// Where the translation of the sentence of `key` starts in the file.
    #[must_use]
    pub fn find(&self, key: u32) -> Option<u32> {
        let at = self.rows.partition_point(|row| row.0 < key);
        self.rows
            .get(at)
            .filter(|row| row.0 == key)
            .map(|row| row.1)
    }

    /// The sentences kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no sentence is kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// The translation that starts `bytes` (read from where [`Index::find`]
/// says): up to the end of its line, at most [`MAX_GLOSS`] bytes, at a
/// character.
#[must_use]
pub fn translation(bytes: &[u8]) -> &str {
    let end = bytes
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(bytes.len())
        .min(MAX_GLOSS);
    let text = match core::str::from_utf8(&bytes[..end]) {
        Ok(text) => text,
        Err(error) => core::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or(""),
    };
    text.trim()
}

/// Whether `c` closes a sentence after its end mark (a quote, a bracket).
fn closing(c: char) -> bool {
    matches!(c, '"' | '\'' | '’' | '”' | '»' | '«' | ')' | ']')
}

/// The byte ranges of the sentences of `text`: each ends after `.`, `!`, `?`
/// or `…` and what closes it, where a space or the text's end follows.
#[must_use]
pub fn sentences(text: &str) -> Vec<Range<u32>> {
    let mut found = Vec::new();
    let mut start = None;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if start.is_none() && !c.is_whitespace() {
            start = Some(at);
        }
        if !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        let mut end = at + c.len_utf8();
        while let Some(&(next, c)) = chars.peek() {
            if closing(c) || matches!(c, '.' | '!' | '?' | '…') {
                end = next + c.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let ends = chars.peek().is_none_or(|&(_, c)| c.is_whitespace());
        if let (true, Some(from)) = (ends, start) {
            if found.try_reserve(1).is_err() {
                return found;
            }
            found.push(from as u32..end as u32);
            start = None;
        }
    }
    if let Some(from) = start {
        let end = text.trim_end().len();
        if end > from && found.try_reserve(1).is_ok() {
            found.push(from as u32..end as u32);
        }
    }
    found
}

/// Where the translations go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Under each sentence, on a line of its own under the text.
    Under,
    /// All of a paragraph's together, after it.
    After,
}

/// The answer for a block of `text`: the translation `found` gives each of
/// its sentences, under it or after the paragraph. A sentence without one
/// gets nothing.
///
/// # Errors
/// What the answer refuses (it is full: the sentences after are left out).
pub fn answer(
    answer: &mut TextAnswer<'_>,
    text: &str,
    place: Place,
    mut found: impl FnMut(&str) -> Option<String>,
) -> Result<(), Error> {
    let mut after = String::new();
    for sentence in sentences(text) {
        let Some(part) = text.get(sentence.start as usize..sentence.end as usize) else {
            continue;
        };
        let Some(translation) = found(part) else {
            continue;
        };
        match place {
            Place::Under => {
                answer.gloss(sentence, GlossKind::Gloss, GLOSS_SIZE, &translation)?;
            }
            Place::After => {
                if after.len() + translation.len() < MAX_INSERTED
                    && after.try_reserve(translation.len() + 1).is_ok()
                {
                    if !after.is_empty() {
                        after.push(' ');
                    }
                    after.push_str(&translation);
                }
            }
        }
    }
    if !after.is_empty() {
        answer.insert(&after, AFTER_SIZE, true)?;
    }
    Ok(())
}

/// The side file of the book at `path`: its file name without its extension,
/// `.txt`, in the plugin's data folder (`alice.epub` → `alice.txt`).
#[must_use]
pub fn side_file(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?;
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    if stem.is_empty() {
        return None;
    }
    let mut file = String::new();
    file.try_reserve_exact(stem.len() + 4).ok()?;
    file.push_str(stem);
    file.push_str(".txt");
    Some(file)
}

#[cfg(test)]
mod tests;
