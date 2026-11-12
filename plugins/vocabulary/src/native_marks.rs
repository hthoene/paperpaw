//! The words being learnt, marked where they occur in books, on interface 7:
//! the plugin subscribes to the reader's stage `reader.decorations@1`
//! (`stages/decorations/`), which sends the text of a page in runs and takes
//! marks back, and is called again when a marked word is tapped (a lookup).
//!
//! Host-free here: the set of words of the list in the book's language, the
//! search for them in a run, the look of the mark and the codes the stage
//! carries it in. Beta 3's decorator (`marks.rs`) found the same words with
//! the SDK's `WordSet`; this one allocates fallibly and keeps nothing between
//! two fires of the stage (#248: the set is read from the list each time).
use crate::store;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;
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

/// The words and phrases of the list that belong to a language, folded and
/// sorted, each once, in one string.
#[derive(Debug, Default)]
pub struct Entries {
    text: String,
    /// Where each entry is in `text`, sorted by the entry.
    spans: Vec<(u32, u32)>,
}

impl Entries {
    /// The entries of the list on `storage` that belong to `language` (primary).
    /// A list that cannot be read to its end gives what was read.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn read(storage: &dyn Storage, language: &str) -> Result<Self, Error> {
        let mut entries = Self::default();
        let mut failed = None;
        // Ignored: a list cut by a card that cannot be read marks what it can.
        store::each_line(storage, store::file(), |line| {
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
        })
        .ok();
        if let Some(error) = failed {
            return Err(error);
        }
        entries.finish();
        Ok(entries)
    }

    /// Adds `word` (a word or a phrase), folded; text with no word in it is
    /// no entry.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn add(&mut self, word: &str) -> Result<(), Error> {
        let from = self.text.len();
        let words = fold_into(word, &mut self.text, from);
        match words {
            Ok(0) => {
                self.text.truncate(from);
                return Ok(());
            }
            Err(error) => {
                self.text.truncate(from);
                return Err(error);
            }
            Ok(_) => {}
        }
        let span = (
            u32::try_from(from).map_err(|_| Error::NoMemory)?,
            u32::try_from(self.text.len()).map_err(|_| Error::NoMemory)?,
        );
        if self.spans.try_reserve(1).is_err() {
            self.text.truncate(from);
            return Err(Error::NoMemory);
        }
        self.spans.push(span);
        Ok(())
    }

    /// Sorts the entries and leaves each once.
    pub fn finish(&mut self) {
        let text = &self.text;
        let entry = |span: &(u32, u32)| text.get(span.0 as usize..span.1 as usize).unwrap_or("");
        self.spans.sort_unstable_by(|a, b| entry(a).cmp(entry(b)));
        self.spans.dedup_by(|a, b| entry(a) == entry(b));
    }

    /// How many entries there are.
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    fn entry(&self, at: usize) -> &str {
        self.spans
            .get(at)
            .and_then(|span| self.text.get(span.0 as usize..span.1 as usize))
            .unwrap_or("")
    }

    /// The number of the first entry not before `key`.
    fn position(&self, key: &str) -> usize {
        let (mut low, mut high) = (0, self.spans.len());
        while low < high {
            let middle = low + (high - low) / 2;
            if self.entry(middle).cmp(key) == Ordering::Less {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    fn has(&self, key: &str) -> bool {
        let at = self.position(key);
        at < self.spans.len() && self.entry(at) == key
    }

    /// Whether an entry begins with the folded `key` and more words.
    fn has_longer(&self, key: &mut String) -> Result<bool, Error> {
        push_char(key, ' ')?;
        let at = self.position(key);
        let more = at < self.spans.len() && self.entry(at).starts_with(key.as_str());
        key.pop();
        Ok(more)
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
                if !self.has_longer(&mut key)? {
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
