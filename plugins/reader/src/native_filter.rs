//! The text filters of the `reader.filter@1` stage (aids and the like): what a
//! subscriber answered is checked once ([`Filter::from_reply`]) and then
//! applied to each block before it is laid out ([`Filter::apply`]), in Beta 3's
//! order: bold word beginnings, the replacements, the pictures rule.
//!
//! The reader keeps only the reply's bytes (at most `MAX_REPLY`, 3,840, an
//! exact copy) for as long as the book is open; a block decodes them again
//! without allocating. A block is changed whole or not at all
//! (`Block::try_transform`): a refusal leaves it as the book has it.
//!
//! The rules are those of `plugins/aids/src/reader_filter` (moved here, the
//! reader owns the pipeline); the tests pin both to the same blocks.
use crate::stages::reader_filter_v1 as wire;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{Host, stages::Stage};
use paperpaw_sdk::{
    document::{Align, Block, BlockKind, EditError, Run, TextEdit, TextStyle},
    native::Error,
};

/// The most heap one block's edit holds (half for the working copy, half for
/// the filter's own lists), as the reader takes at most this much text.
pub const BUDGET: usize = 64 * 1024;

/// What a block is after the filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Laid out (changed or not).
    Keep,
    /// Left out of the layout (a picture the rules hide).
    Drop,
}

/// The pictures rule of `filter.rules` (0, or one this reader does not know: shown).
const CAPTION: u32 = 1;
const HIDE: u32 = 2;

/// The books whose pictures are their pages: their pictures are never captioned
/// or hidden.
const PICTURE_BOOKS: [&str; 4] = ["cbz", "cbr", "xtc", "xtch"];

/// Whether the book at `path` is one whose pictures are its pages (Beta 3's
/// `image_only`: comic archives and XTC books).
#[must_use]
pub fn image_only(path: &str) -> bool {
    let ext = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    PICTURE_BOOKS.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// The rules the subscribers of `reader.filter@1` give the book at `path` in
/// `language`, each checked, in the host's order; none where nobody
/// subscribed or the device runs no stages yet. A reply that is not sound is
/// left out, the others stay.
///
/// # Errors
/// [`Error::NoMemory`]; what the host refuses besides "no stages here".
pub fn ask(host: &Host<'_>, path: &str, language: &str) -> Result<Vec<Filter>, Error> {
    let mut filters = Vec::new();
    let stage = Stage::new(host, wire::KEY)?;
    let format = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    let format = format.get(..format.len().min(32)).unwrap_or("");
    let language = language.get(..language.len().min(16)).unwrap_or("");
    let ask = wire::FilterAsk::new(language, format, u32::from(image_only(path)));
    let mut input = [0u8; wire::MAX_INPUT];
    let Ok(len) = ask.encode(&mut input) else {
        return Ok(filters);
    };
    let room = Stage::out_len(wire::MAX_REPLY as u32);
    let mut out = Vec::new();
    out.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
    out.resize(room, 0);
    let mut run = stage.run(input.get(..len).unwrap_or(&[]), &mut out);
    loop {
        match run.next() {
            Ok(Some(reply)) if reply.status().is_ok() => {
                match Filter::from_reply(reply.payload()) {
                    Ok(filter) => {
                        filters.try_reserve(1).map_err(|_| Error::NoMemory)?;
                        filters.push(filter);
                    }
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => {}
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(Error::Unsupported | Error::NotFound) => break,
            Err(error) => return Err(error),
        }
    }
    Ok(filters)
}

/// One subscriber's rules: its reply, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    reply: Vec<u8>,
    signature: u64,
}

impl Filter {
    /// The rules of `reply` (one subscriber's answer to `reader.filter@1`):
    /// one `filter.rules` first, then `filter.pair`s; every rule within its
    /// bounds and the signature that of the rules (so a reply from other
    /// settings than its signature says is refused). Checked whole before
    /// anything is kept.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a reply that is not that, [`Error::NoMemory`].
    pub fn from_reply(reply: &[u8]) -> Result<Self, Error> {
        let batch = wire::reply_decode(reply).map_err(|_| Error::Invalid)?;
        let mut rows = batch.clone();
        let Some(Ok(wire::Record::FilterRules(rules))) = rows.next() else {
            return Err(Error::Invalid);
        };
        if rules.bold > 1 || rules.pictures > HIDE {
            return Err(Error::Invalid);
        }
        for row in rows {
            let Ok(wire::Record::FilterPair(pair)) = row else {
                return Err(Error::Invalid);
            };
            if pair.from.is_empty() || pair.whole_word != 1 {
                return Err(Error::Invalid);
            }
        }
        if signature_of(&batch)? != rules.signature {
            return Err(Error::Invalid);
        }
        let mut kept = Vec::new();
        kept.try_reserve_exact(reply.len())
            .map_err(|_| Error::NoMemory)?;
        kept.extend_from_slice(reply);
        Ok(Self {
            reply: kept,
            signature: rules.signature,
        })
    }

    /// What the rules are, for the layout's key: it changes with them.
    #[must_use]
    pub fn signature(&self) -> u64 {
        self.signature
    }

    /// The rules applied to `block` of the book at `path`: the beginnings of
    /// a paragraph's words in bold, the pairs' words replaced (whole words,
    /// the places found before any is replaced, last first), and a picture
    /// shown, captioned (`[Picture: its text]`, centred, italic) or dropped.
    ///
    /// # Errors
    /// [`Error::NoMemory`], [`Error::NoSpace`] (over [`BUDGET`]) or
    /// [`Error::Invalid`]; the block is then unchanged.
    pub fn apply(&self, path: &str, block: &mut Block) -> Result<Shown, Error> {
        let batch = wire::reply_decode(&self.reply).map_err(|_| Error::Invalid)?;
        let mut rows = batch.clone();
        let Some(Ok(wire::Record::FilterRules(rules))) = rows.next() else {
            return Err(Error::Invalid);
        };
        let limit = BUDGET / 2;
        block
            .try_transform(limit, |edit| {
                if rules.bold == 1 && edit.kind() == BlockKind::Paragraph {
                    bold(edit, limit)?;
                }
                for row in rows {
                    let Ok(wire::Record::FilterPair(pair)) = row else {
                        return Err(Failed::Invalid);
                    };
                    replace(edit, pair.from, pair.to, limit)?;
                }
                picture(edit, path, rules.pictures, rules.caption_word, limit)
            })
            .map_err(Error::from)
    }
}

/// The signatures of `filters` folded into one, in their order.
#[must_use]
pub fn signature(filters: &[Filter]) -> u64 {
    filters
        .iter()
        .fold(0, |all, filter| all.rotate_left(17) ^ filter.signature)
}

/// Every one of `filters` applied to `block` in turn; a block one of them
/// drops is not given to the next.
///
/// # Errors
/// As [`Filter::apply`]: the block keeps what the filters before did.
pub fn apply(filters: &[Filter], path: &str, block: &mut Block) -> Result<Shown, Error> {
    for filter in filters {
        if filter.apply(path, block)? == Shown::Drop {
            return Ok(Shown::Drop);
        }
    }
    Ok(Shown::Keep)
}

/// Why a step of the edit stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failed {
    Invalid,
    Limit,
    NoMemory,
}

impl From<EditError> for Failed {
    fn from(error: EditError) -> Self {
        match error {
            EditError::NoMemory => Self::NoMemory,
            EditError::Limit => Self::Limit,
            _ => Self::Invalid,
        }
    }
}

impl From<Failed> for Error {
    fn from(failed: Failed) -> Self {
        match failed {
            Failed::Invalid => Self::Invalid,
            Failed::Limit => Self::NoSpace,
            Failed::NoMemory => Self::NoMemory,
        }
    }
}

/// The signature of the rules: their canonical bytes with the signature
/// itself 0, hashed (FNV-1a, 64 bits), as the subscriber made it.
fn signature_of(batch: &wire::RecordStream<'_>) -> Result<u64, Error> {
    let mut records = Vec::new();
    records
        .try_reserve_exact(batch.len())
        .map_err(|_| Error::NoMemory)?;
    for record in batch.clone() {
        let mut record = record.map_err(|_| Error::Invalid)?;
        if let wire::Record::FilterRules(rules) = &mut record {
            rules.signature = 0;
        }
        records.push(record);
    }
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(wire::MAX_REPLY)
        .map_err(|_| Error::NoMemory)?;
    scratch.resize(wire::MAX_REPLY, 0);
    let len = wire::reply_encode(&records, &mut scratch).map_err(|_| Error::Invalid)?;
    let bytes = scratch.get(..len).ok_or(Error::Invalid)?;
    Ok(bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    }))
}

/// Whether `text[at..end]` is a whole word: no letter or digit right before
/// or after it.
fn whole_word(text: &str, at: usize, end: usize) -> bool {
    let before = text.get(..at).and_then(|t| t.chars().next_back());
    let after = text.get(end..).and_then(|t| t.chars().next());
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

/// `from` replaced by `to` wherever it is a whole word. The places are found
/// before any is replaced (a replacement beside punctuation does not change
/// which earlier ones qualify) and replaced last first.
fn replace(edit: &mut TextEdit, from: &str, to: &str, limit: usize) -> Result<(), Failed> {
    let matches = |text: &str| {
        text.match_indices(from)
            .map(|(at, _)| at)
            .filter(|at| whole_word(text, *at, *at + from.len()))
            .count()
    };
    let count = matches(edit.text());
    if count
        .checked_mul(core::mem::size_of::<usize>())
        .ok_or(Failed::Limit)?
        > limit
    {
        return Err(Failed::Limit);
    }
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(count)
        .map_err(|_| Failed::NoMemory)?;
    let text = edit.text();
    positions.extend(
        text.match_indices(from)
            .map(|(at, _)| at)
            .filter(|at| whole_word(text, *at, *at + from.len())),
    );
    for at in positions.into_iter().rev() {
        edit.try_replace(at..at + from.len(), to)?;
    }
    Ok(())
}

/// The first half (rounded up) of every word of two letters or more in bold,
/// where no run of the book's own covers it.
fn bold(edit: &mut TextEdit, limit: usize) -> Result<(), Failed> {
    let text = edit.text();
    let mut runs = Vec::new();
    let mut start = None;
    let mut letters = 0usize;
    for (at, ch) in text
        .char_indices()
        .chain(core::iter::once((text.len(), ' ')))
    {
        if ch.is_alphabetic() {
            start.get_or_insert(at);
            letters += 1;
        } else if let Some(begin) = start.take() {
            if letters >= 2 {
                let end = text
                    .get(begin..)
                    .and_then(|word| word.char_indices().nth(letters.div_ceil(2)))
                    .map_or(text.len(), |(offset, _)| begin + offset);
                let styled = edit
                    .runs()
                    .iter()
                    .any(|r| (r.start as usize) < end && begin < r.end as usize);
                if !styled {
                    let run = Run::new(
                        u32::try_from(begin).map_err(|_| Failed::Limit)?,
                        u32::try_from(end).map_err(|_| Failed::Limit)?,
                        TextStyle::BOLD,
                    );
                    push(&mut runs, run, limit)?;
                }
            }
            letters = 0;
        }
    }
    for run in edit.runs() {
        push(&mut runs, *run, limit)?;
    }
    paperpaw_sdk::sort::sort_by_key(&mut runs, |run| run.start);
    edit.try_set_runs(&runs)?;
    Ok(())
}

/// `run` added to `runs`, growing them by one exactly, the old and the new
/// room within `limit` while they grow.
fn push(runs: &mut Vec<Run>, run: Run, limit: usize) -> Result<(), Failed> {
    let needed = runs.len().checked_add(1).ok_or(Failed::Limit)?;
    if needed > runs.capacity() {
        let peak = runs
            .capacity()
            .checked_add(needed)
            .and_then(|n| n.checked_mul(core::mem::size_of::<Run>()))
            .ok_or(Failed::Limit)?;
        if peak > limit {
            return Err(Failed::Limit);
        }
        runs.try_reserve_exact(1).map_err(|_| Failed::NoMemory)?;
    }
    runs.push(run);
    Ok(())
}

/// The pictures rule for a picture block of the book at `path`: shown as it
/// is (and always in a book whose pictures are its pages), dropped, or a line
/// in its place, `[word: its text]` (`[word]` where it has none), centred and
/// italic over the picture's whole span.
fn picture(
    edit: &mut TextEdit,
    path: &str,
    policy: u32,
    word: &str,
    limit: usize,
) -> Result<Shown, Failed> {
    if edit.kind() != BlockKind::Image || image_only(path) {
        return Ok(Shown::Keep);
    }
    match policy {
        HIDE => return Ok(Shown::Drop),
        CAPTION => {}
        _ => return Ok(Shown::Keep),
    }
    let alt = edit.text().trim();
    let length = word
        .len()
        .checked_add(alt.len())
        .and_then(|n| n.checked_add(if alt.is_empty() { 2 } else { 4 }))
        .ok_or(Failed::Limit)?;
    if length > limit {
        return Err(Failed::Limit);
    }
    let mut line = String::new();
    line.try_reserve_exact(length)
        .map_err(|_| Failed::NoMemory)?;
    line.push('[');
    line.push_str(word);
    if !alt.is_empty() {
        line.push_str(": ");
        line.push_str(alt);
    }
    line.push(']');
    let end = u32::try_from(line.len()).map_err(|_| Failed::Limit)?;
    edit.try_set_text(&line)?;
    edit.try_set_runs(&[Run::new(0, end, TextStyle::ITALIC)])?;
    edit.set_kind(BlockKind::Paragraph);
    edit.set_align(Align::Center);
    edit.drop_image();
    Ok(Shown::Keep)
}

#[cfg(test)]
mod tests;
