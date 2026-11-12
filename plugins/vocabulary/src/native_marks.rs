//! The words being learnt, marked where they occur in books, on interface 7:
//! the plugin subscribes to the reader's stage `reader.decorations@1`
//! (`stages/decorations/`), which sends the text of a page in runs and takes
//! marks back, and is called again when a marked word is tapped (a lookup).
//!
//! Host-free here: the set of words of the list in the book's language, the
//! search for them in a run, the look of the mark and the codes the stage
//! carries it in. Beta 3's decorator (`marks.rs`) found the same words with
//! the SDK's `WordSet`; this one allocates fallibly and keeps nothing between
//! two fires of the stage (#248): the set is read each time, from a side file
//! of hashes that is made again only when the list's size has changed.
use crate::store;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Storage,
    decorations::{AS_IS, LineKind, NONE, THICKNESS, Tone},
    native::Error,
};

/// The role of the marked words (the settings are `<role>.<aspect>`).
pub const ROLE: &str = "vocabulary.learning";
/// The bytes of marks a reply holds (`decorate.marks.ranges`).
pub const MAX_RANGES: usize = 3_600;
/// The bytes of one mark: `run`, `from`, `to`, little-endian u16 each.
pub const MARK: usize = 6;

/// The tones as the stage numbers them: fixed codes (the stage's contract in
/// the reader's `PORTING-7.md`), 0 for none.
const TONES: [(u32, Tone); 11] = [
    (1, Tone::LightGray),
    (2, Tone::Gray),
    (3, Tone::DarkGray),
    (4, Tone::Black),
    (5, Tone::White),
    (6, Tone::Yellow),
    (7, Tone::Green),
    (8, Tone::Blue),
    (9, Tone::Pink),
    (10, Tone::Orange),
    (11, Tone::Red),
];
/// The kinds of line as the stage numbers them, 0 for none.
const LINES: [(u32, LineKind); 5] = [
    (1, LineKind::Solid),
    (2, LineKind::Dotted),
    (3, LineKind::Dashed),
    (4, LineKind::Wavy),
    (5, LineKind::Double),
];
/// `decorate.look.flags`: a frame around the text, a line through it, taps taken.
const FRAME: u32 = 1;
const STRUCK: u32 = 1 << 1;
const TAPS: u32 = 1 << 2;
/// The most a line or frame is thick, in tenths of a millimetre.
const MAX_THICKNESS: i64 = 20;

/// The look of the marked words as the stage carries it (`decorate.look`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub background: u32,
    pub ink: u32,
    pub line: u32,
    pub line_tone: u32,
    pub thickness: u32,
    pub flags: u32,
}

impl Default for Look {
    /// What the role was in Beta 3: a dotted line under the words.
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

/// The role's settings as they are kept (`vocabulary.learning.*`): the ids of
/// the tones and lines, the thickness and the two switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings<'a> {
    pub background: &'a str,
    pub ink: &'a str,
    pub line: &'a str,
    pub line_tone: &'a str,
    pub thickness: i64,
    pub frame: bool,
    pub struck: bool,
}

impl Default for Settings<'_> {
    fn default() -> Self {
        Self {
            background: NONE,
            ink: AS_IS,
            line: LineKind::Dotted.id(),
            line_tone: AS_IS,
            thickness: i64::from(THICKNESS),
            frame: false,
            struck: false,
        }
    }
}

impl Look {
    /// The look the settings say; an id that is no tone or line is none, a
    /// thickness is brought into `1..=20`. Taps are always taken.
    #[must_use]
    pub fn from_settings(settings: &Settings<'_>) -> Self {
        let tone = |id: &str| {
            Tone::from_id(id).map_or(0, |tone| {
                TONES
                    .iter()
                    .find(|(_, t)| *t == tone)
                    .map_or(2, |(code, _)| *code)
            })
        };
        let line = LineKind::from_id(settings.line).map_or(0, |kind| {
            LINES
                .iter()
                .find(|(_, k)| *k == kind)
                .map_or(1, |(code, _)| *code)
        });
        let mut flags = TAPS;
        if settings.frame {
            flags |= FRAME;
        }
        if settings.struck {
            flags |= STRUCK;
        }
        Self {
            background: tone(settings.background),
            ink: tone(settings.ink),
            line,
            line_tone: tone(settings.line_tone),
            thickness: u32::try_from(settings.thickness.clamp(1, MAX_THICKNESS)).unwrap_or(4),
            flags,
        }
    }
}

/// The part of a language tag before its first `-` or `_`: `en-GB` is `en`.
#[must_use]
pub fn primary(language: &str) -> &str {
    language.split(['-', '_']).next().unwrap_or_default().trim()
}

/// Whether a word kept in `kept` belongs to a book in `language` (both
/// primary), whatever their case; where either is not known, it does.
fn same_language(kept: &str, language: &str) -> bool {
    kept.is_empty() || language.is_empty() || kept.eq_ignore_ascii_case(language)
}

/// `c` appended to `out`, or `NoMemory`.
fn push_char(out: &mut String, c: char) -> Result<(), Error> {
    out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
    out.push(c);
    Ok(())
}

/// A combining accent, as in text whose letters are not composed.
fn is_accent(c: char) -> bool {
    ('\u{0300}'..='\u{036F}').contains(&c)
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '\u{02BC}')
}

/// Appends the words of `text`, folded (as the SDK's `fold`: lowercase, `ß` as
/// `ss`, no punctuation around a word, an apostrophe inside one kept) to
/// `out`, with a space before each unless `out` is no longer than `base`; the
/// number of words.
///
/// # Errors
/// [`Error::NoMemory`].
fn fold_into(text: &str, out: &mut String, base: usize) -> Result<usize, Error> {
    let mut words = 0;
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_alphanumeric() || (in_word && is_accent(c)) {
            if !in_word {
                if out.len() > base {
                    push_char(out, ' ')?;
                }
                in_word = true;
                words += 1;
            }
            for lower in c.to_lowercase() {
                if lower == 'ß' {
                    push_char(out, 's')?;
                    push_char(out, 's')?;
                } else {
                    push_char(out, lower)?;
                }
            }
        } else if in_word && is_apostrophe(c) && chars.peek().is_some_and(|n| n.is_alphanumeric()) {
            push_char(out, '\'')?;
        } else {
            in_word = false;
        }
    }
    Ok(words)
}

/// What an entry's hash is of: the entry itself, or the first words of one
/// longer (a phrase can start here).
const ENTRY: u8 = 0;
const PREFIX: u8 = 1;
/// The side file of a language: its magic, then the words file's size (little-endian
/// u32) it was made from, then how many hashes it holds (u32), then the hashes
/// (little-endian u32 each). A file cut short by a power cut or a refused write
/// is not as long as its count says, and is not read.
const MAGIC: &[u8; 4] = b"VM01";
const HEADER: usize = 12;
/// Bytes of the side file read or written at a time.
const CHUNK: usize = 512;
/// Where the side files are; the language is in the name.
const SIDE: &str = ".paperpaw/plugins/vocabulary/marks";

/// A 32-bit hash of a folded entry (FNV-1a with a final mix), the same in every
/// build: the side file keeps it.
fn hash(kind: u8, text: &str) -> u32 {
    let mut h = 0x811c_9dc5_u32 ^ u32::from(kind);
    h = h.wrapping_mul(0x0100_0193);
    for byte in text.bytes() {
        h ^= u32::from(byte);
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

/// The side file of a language, or none where its name is no plain code.
fn side_file(language: &str) -> Result<Option<String>, Error> {
    let plain = language.len() <= 8 && language.bytes().all(|b| b.is_ascii_alphabetic());
    if !plain {
        return Ok(None);
    }
    let mut name = String::new();
    name.try_reserve(SIDE.len() + 10)
        .map_err(|_| Error::NoMemory)?;
    name.push_str(SIDE);
    if !language.is_empty() {
        name.push('-');
        name.push_str(&language.to_ascii_lowercase());
    }
    name.push_str(".idx");
    Ok(Some(name))
}

/// The words and phrases of the list that belong to a language, as sorted
/// 32-bit hashes (4 bytes an entry, and 4 for each start of a phrase): what
/// the page's words are looked up in. Not the text: the list's words are kept
/// folded in a side file of the language, made again when the list's size
/// changes, so a page turn reads some kilobytes rather than the whole list.
/// A hash that two entries or an entry and a page word share (about one in
/// 2^32 for each pair) marks a word that is not in the list.
#[derive(Debug, Default)]
pub struct Entries {
    hashes: Vec<u32>,
}

impl Entries {
    /// The entries of the list on `storage` that belong to `language` (primary).
    /// A list that cannot be read to its end gives what was read.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn read(storage: &dyn Storage, language: &str) -> Result<Self, Error> {
        let Ok(size) = storage.size(store::file()) else {
            return Ok(Self::default());
        };
        let (Ok(size), Some(side)) = (u32::try_from(size), side_file(language)?) else {
            return Ok(Self::build(storage, language)?.0);
        };
        if let Some(set) = Self::load(storage, &side, size)? {
            return Ok(set);
        }
        let (set, whole) = Self::build(storage, language)?;
        if whole {
            // Best effort: a side file that is not kept is made again.
            set.save(storage, &side, size).ok();
        }
        Ok(set)
    }

    /// The entries from the list itself, and whether it was read to its end.
    fn build(storage: &dyn Storage, language: &str) -> Result<(Self, bool), Error> {
        let mut entries = Self::default();
        let mut failed = None;
        let read = store::each_line(storage, store::file(), |line| {
            if failed.is_some() {
                return;
            }
            let mut fields = line.splitn(3, '\t');
            let (Some(word), Some(kept)) = (fields.next(), fields.next()) else {
                return;
            };
            if same_language(primary(kept), language)
                && let Err(error) = entries.add(word)
            {
                failed = Some(error);
            }
        });
        if let Some(error) = failed {
            return Err(error);
        }
        entries.finish();
        Ok((entries, read.is_ok()))
    }

    /// The entries of the side file, if it is there and was made from a list
    /// of `size` bytes.
    fn load(storage: &dyn Storage, side: &str, size: u32) -> Result<Option<Self>, Error> {
        let Ok(length) = storage.size(side) else {
            return Ok(None);
        };
        let Ok(length) = usize::try_from(length) else {
            return Ok(None);
        };
        let mut head = [0u8; HEADER];
        let read = |at: usize, buf: &mut [u8]| matches!(storage.read_at(side, at as u64, buf), Ok(n) if n == buf.len());
        if length < HEADER || !read(0, &mut head) {
            return Ok(None);
        }
        if head.get(..4) != Some(MAGIC.as_slice()) || head.get(4..8) != Some(&size.to_le_bytes()) {
            return Ok(None);
        }
        let count = head
            .get(8..)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map_or(usize::MAX, |bytes| u32::from_le_bytes(bytes) as usize);
        if count
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(HEADER))
            != Some(length)
        {
            return Ok(None);
        }
        let mut hashes = Vec::new();
        hashes
            .try_reserve_exact(count)
            .map_err(|_| Error::NoMemory)?;
        let mut chunk = [0u8; CHUNK];
        let mut at = HEADER;
        while at < length {
            let take = CHUNK.min(length - at);
            let Some(part) = chunk.get_mut(..take) else {
                return Ok(None);
            };
            if !read(at, part) {
                return Ok(None);
            }
            hashes.extend(
                part.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|bytes| u32::from_le_bytes(*bytes)),
            );
            at += take;
        }
        Ok(Some(Self { hashes }))
    }

    /// Writes the side file; a file cut short is not read (its length is
    /// checked), the next page makes it again.
    fn save(
        &self,
        storage: &dyn Storage,
        side: &str,
        size: u32,
    ) -> Result<(), paperpaw_sdk::StorageError> {
        let mut head = [0u8; HEADER];
        head[..4].copy_from_slice(MAGIC);
        head[4..8].copy_from_slice(&size.to_le_bytes());
        let count =
            u32::try_from(self.hashes.len()).map_err(|_| paperpaw_sdk::StorageError::TooLarge)?;
        head[8..].copy_from_slice(&count.to_le_bytes());
        storage.write(side, &head)?;
        let mut chunk = [0u8; CHUNK];
        for part in self.hashes.chunks(CHUNK / 4) {
            let bytes = part.len() * 4;
            for (slot, hash) in chunk.as_chunks_mut::<4>().0.iter_mut().zip(part) {
                *slot = hash.to_le_bytes();
            }
            storage.append(side, chunk.get(..bytes).unwrap_or_default())?;
        }
        Ok(())
    }

    /// Adds `word` (a word or a phrase), folded; text with no word in it is
    /// no entry.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn add(&mut self, word: &str) -> Result<(), Error> {
        let mut folded = String::new();
        if fold_into(word, &mut folded, 0)? == 0 {
            return Ok(());
        }
        self.hashes
            .try_reserve(1 + folded.matches(' ').count())
            .map_err(|_| Error::NoMemory)?;
        self.hashes.push(hash(ENTRY, &folded));
        for (at, _) in folded.match_indices(' ') {
            self.hashes
                .push(hash(PREFIX, folded.get(..at).unwrap_or("")));
        }
        Ok(())
    }

    /// Sorts the hashes and leaves each once.
    pub fn finish(&mut self) {
        self.hashes.sort_unstable();
        self.hashes.dedup();
    }

    /// How many hashes there are (entries and starts of phrases).
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    fn has(&self, key: &str) -> bool {
        self.hashes.binary_search(&hash(ENTRY, key)).is_ok()
    }

    /// Whether an entry begins with the folded `key` and more words.
    fn has_longer(&self, key: &str) -> bool {
        self.hashes.binary_search(&hash(PREFIX, key)).is_ok()
    }

    /// Marks the entries found in `run` (a page's text, words apart by one
    /// space) as run number `number`: `run`, `from`, `to` (bytes of the run) in
    /// six bytes each, appended to `marks` while it holds at most
    /// [`MAX_RANGES`] bytes. The longest entry wins where entries overlap, a
    /// mark covers its words as the page shows them (punctuation around them
    /// too), and no entry goes across a token with no word in it. `false`
    /// where `marks` was full and the rest of the run is left.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn find(&self, run: &str, number: u16, marks: &mut Vec<u8>) -> Result<bool, Error> {
        if self.is_empty() {
            return Ok(true);
        }
        let mut key = String::new();
        let mut at = 0;
        while let Some((start, first_end)) = token(run, at) {
            key.clear();
            let mut longest = None;
            let (mut from, mut end) = (start, first_end);
            loop {
                let folded = run.get(from..end).unwrap_or("");
                if fold_into(folded, &mut key, 0)? == 0 {
                    // Only punctuation: no entry goes across it.
                    break;
                }
                if self.has(&key) {
                    longest = Some(end);
                }
                if !self.has_longer(&key) {
                    break;
                }
                match token(run, end) {
                    Some((next_from, next_end)) => (from, end) = (next_from, next_end),
                    None => break,
                }
            }
            match longest {
                Some(end) => {
                    if marks.len() + MARK > MAX_RANGES {
                        return Ok(false);
                    }
                    let (Ok(from), Ok(to)) = (u16::try_from(start), u16::try_from(end)) else {
                        return Ok(true);
                    };
                    marks.try_reserve(MARK).map_err(|_| Error::NoMemory)?;
                    for part in [number, from, to] {
                        marks.extend_from_slice(&part.to_le_bytes());
                    }
                    at = end;
                }
                None => at = first_end,
            }
        }
        Ok(true)
    }
}

/// The word of `text` at or after byte `at`: where it begins and ends. Words
/// are apart by spaces, as the page's text has them.
fn token(text: &str, at: usize) -> Option<(usize, usize)> {
    let rest = text.get(at..)?;
    let skipped = rest.len() - rest.trim_start_matches(' ').len();
    let start = at + skipped;
    let word = text.get(start..)?;
    if word.is_empty() {
        return None;
    }
    let length = word.find(' ').unwrap_or(word.len());
    Some((start, start + length))
}

/// The word of a tapped mark as a lookup takes it: its text without the
/// punctuation around it; empty where it holds no word.
#[must_use]
pub fn tapped_word(text: &str) -> &str {
    text.trim_matches(|c: char| !c.is_alphanumeric())
}

/// The language the word was last kept in (the list's second field); empty
/// where it is not in the list.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn language_of(storage: &dyn Storage, word: &str) -> Result<String, Error> {
    let mut key = String::new();
    fold_into(word, &mut key, 0)?;
    let mut language = String::new();
    let mut failed = None;
    store::each_line(storage, store::file(), |line| {
        if failed.is_some() {
            return;
        }
        let mut fields = line.splitn(3, '\t');
        let (Some(kept), Some(kept_language)) = (fields.next(), fields.next()) else {
            return;
        };
        let mut folded = String::new();
        let same = fold_into(kept, &mut folded, 0).map(|_| folded == key);
        match same {
            Ok(true) => {
                language.clear();
                if language.try_reserve(kept_language.len()).is_err() {
                    failed = Some(Error::NoMemory);
                } else {
                    language.push_str(kept_language);
                }
            }
            Ok(false) => {}
            Err(error) => failed = Some(error),
        }
    })
    .ok();
    match failed {
        Some(error) => Err(error),
        None => Ok(language),
    }
}

#[cfg(test)]
mod tests;
