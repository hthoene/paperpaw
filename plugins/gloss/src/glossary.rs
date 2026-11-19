//! The word list of a book and the answer for a paragraph: which words of it
//! get a small text over them. What the plugin does without a device.
//!
//! The list is a text file, a line each: a word, a tab, its small text
//! (`#` starts a comment). It is read once when the book opens and kept in
//! memory ([`Glossary`]); a paragraph's words are looked up in it
//! ([`answer`]), nothing is read from the card for a page.
use alloc::{string::String, vec::Vec};
use core::ops::Range;
use paperpaw_sdk::native::{
    Error,
    page_layers::{GlossKind, MAX_GLOSS, TextAnswer},
};

/// The most words of a list kept (8 bytes each).
pub const MOST_WORDS: usize = 2048;
/// The most bytes of small text kept, all words together.
pub const MOST_TEXT: usize = 24 * 1024;
/// The longest small text; a longer one is cut at a character.
pub const LONGEST_GLOSS: usize = 96;
/// The longest line of a list read; a longer one is left out.
pub const LONGEST_LINE: usize = 512;
/// The most characters of a phrase in a script written without spaces.
pub const LONGEST_PHRASE: usize = 8;

/// The key of `word`: its letters and digits (and the `'` and `-` inside it),
/// lower case, hashed (FNV-1a); 0 for none.
#[must_use]
pub fn key(word: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    let mut taken = false;
    for c in word.chars().filter(|&c| c.is_alphanumeric() || inner(c)) {
        let c = if c == '’' { '\'' } else { c };
        for lower in c.to_lowercase() {
            let mut bytes = [0; 4];
            for &b in lower.encode_utf8(&mut bytes).as_bytes() {
                hash = (hash ^ u32::from(b)).wrapping_mul(0x0100_0193);
            }
        }
        taken = true;
    }
    if taken { hash.max(1) } else { 0 }
}

/// Whether `c` may be inside a word (`don't`, `well-known`), not at its ends.
fn inner(c: char) -> bool {
    matches!(c, '\'' | '’' | '-')
}

/// Whether `c` is of a script written without spaces between its words
/// (kana, Han characters).
fn unspaced(c: char) -> bool {
    matches!(
        u32::from(c),
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x2_0000..=0x2_FFFF
    )
}

/// The word list of a book: each word's key and where its small text is
/// kept (a length byte, then the text), sorted by key.
#[derive(Debug, Default)]
pub struct Glossary {
    rows: Vec<(u32, u32)>,
    text: Vec<u8>,
    /// The bytes of the line read so far (one that goes on in the next part).
    line: Vec<u8>,
    /// The line read so far is too long: left out up to its end.
    skipping: bool,
    /// A hash of the file, for the layout's key.
    hash: u32,
}

impl Glossary {
    /// Reads the next `part` of the file.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(&mut self, part: &[u8]) -> Result<(), Error> {
        for &b in part {
            self.hash = (self.hash ^ u32::from(b)).wrapping_mul(0x0100_0193);
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
        }
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), Error> {
        let line = core::mem::take(&mut self.line);
        let skipped = core::mem::replace(&mut self.skipping, false);
        if skipped || line.first() == Some(&b'#') || self.rows.len() >= MOST_WORDS {
            return Ok(());
        }
        let Ok(text) = core::str::from_utf8(&line) else {
            return Ok(());
        };
        let Some((word, small)) = text.split_once('\t') else {
            return Ok(());
        };
        let key = key(word);
        let small = cut(small.trim(), LONGEST_GLOSS);
        if key == 0 || small.is_empty() || self.text.len() + small.len() + 1 > MOST_TEXT {
            return Ok(());
        }
        self.rows.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.text
            .try_reserve(small.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        self.rows.push((key, self.text.len() as u32));
        self.text.push(small.len() as u8);
        self.text.extend_from_slice(small.as_bytes());
        Ok(())
    }

    /// The file is read: its last line too, the words sorted (a word listed
    /// twice keeps its first text).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(&mut self) -> Result<(), Error> {
        if !self.line.is_empty() {
            self.end_line()?;
        }
        self.line = Vec::new();
        paperpaw_sdk::sort::sort_unstable_by_key(&mut self.rows, |row| {
            u64::from(row.0) << 32 | u64::from(row.1)
        });
        Ok(())
    }

    /// The small text of the word of `key`.
    #[must_use]
    pub fn find(&self, key: u32) -> Option<&str> {
        let at = self.rows.partition_point(|row| row.0 < key);
        let row = self.rows.get(at).filter(|row| row.0 == key)?;
        let at = row.1 as usize;
        let len = usize::from(*self.text.get(at)?);
        core::str::from_utf8(self.text.get(at + 1..at + 1 + len)?).ok()
    }

    /// The words kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no word is kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// A number that changes with the file's bytes: part of the layout's key.
    #[must_use]
    pub fn hash(&self) -> u32 {
        self.hash
    }
}

/// `text` cut to at most `most` bytes, at a character.
fn cut(text: &str, most: usize) -> &str {
    let mut end = text.len().min(most);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The words of `text` that `glossary` knows, in order, as byte ranges with
/// their keys: a word is a run of letters and digits (with `'` and `-`
/// inside); in a script written without spaces it is the longest phrase the
/// list has, from each character on.
#[must_use]
pub fn known(text: &str, glossary: &Glossary) -> Vec<(Range<u32>, u32)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let Some(start) = text[at..]
            .char_indices()
            .find(|&(_, c)| c.is_alphanumeric())
            .map(|(i, _)| at + i)
        else {
            break;
        };
        let end = run_end(text, start);
        at = end;
        if text[start..].chars().next().is_some_and(unspaced) {
            phrases(text, start..end, glossary, &mut found);
        } else {
            let k = key(&text[start..end]);
            if glossary.find(k).is_some() && found.try_reserve(1).is_ok() {
                found.push((start as u32..end as u32, k));
            }
        }
    }
    found
}

/// Where the word that starts at `start` ends.
fn run_end(text: &str, start: usize) -> usize {
    let mut end = start;
    let mut chars = text[start..].char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let next = chars.peek().map(|&(_, n)| n);
        if c.is_alphanumeric() || (inner(c) && next.is_some_and(char::is_alphanumeric)) {
            end = start + i + c.len_utf8();
        } else {
            break;
        }
    }
    end
}

/// The phrases of the list in `range` of `text`, longest first.
fn phrases(text: &str, range: Range<usize>, glossary: &Glossary, out: &mut Vec<(Range<u32>, u32)>) {
    let mut from = range.start;
    while from < range.end {
        let mut ends = [0usize; LONGEST_PHRASE];
        let mut n = 0;
        for (i, c) in text[from..range.end].char_indices().take(LONGEST_PHRASE) {
            ends[n] = from + i + c.len_utf8();
            n += 1;
        }
        let hit = ends[..n].iter().rev().find_map(|&end| {
            let k = key(&text[from..end]);
            glossary.find(k).map(|_| (end, k))
        });
        match hit {
            Some((end, k)) if out.try_reserve(1).is_ok() => {
                out.push((from as u32..end as u32, k));
                from = end;
            }
            _ => from += text[from..].chars().next().map_or(1, char::len_utf8),
        }
    }
}

/// The answer for a paragraph of `text`: the small text of each word of it
/// that the list has, over the word, `size` percent of the text. What the
/// answer cannot hold (64 records) is left out.
///
/// # Errors
/// What the answer refuses: it is full, the words after are left out.
pub fn answer(
    answer: &mut TextAnswer<'_>,
    text: &str,
    glossary: &Glossary,
    size: u16,
) -> Result<(), Error> {
    for (range, k) in known(text, glossary) {
        if let Some(small) = glossary.find(k) {
            answer.gloss(range, GlossKind::Ruby, size, cut(small, MAX_GLOSS))?;
        }
    }
    Ok(())
}

/// The data file of the book at `path`: its name without its extension,
/// `.txt` (`alice.epub` → `alice.txt`).
#[must_use]
pub fn book_file(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?;
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    named(stem)
}

/// The data file of a book's language: its first part and `.txt`
/// (`de-AT` → `de.txt`); none for a tag that is no letters.
#[must_use]
pub fn language_file(language: &str) -> Option<String> {
    let primary = language.split(['-', '_']).next()?;
    if primary.is_empty() || primary.len() > 8 || !primary.bytes().all(|b| b.is_ascii_alphabetic())
    {
        return None;
    }
    let mut lower = String::new();
    lower.try_reserve_exact(primary.len()).ok()?;
    lower.push_str(primary);
    lower.make_ascii_lowercase();
    named(&lower)
}

fn named(stem: &str) -> Option<String> {
    if stem.is_empty() {
        return None;
    }
    let mut file = String::new();
    file.try_reserve_exact(stem.len() + 4).ok()?;
    file.push_str(stem);
    file.push_str(".txt");
    Some(file)
}
