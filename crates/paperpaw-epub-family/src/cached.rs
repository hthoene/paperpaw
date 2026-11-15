//! Reading converted chapters from the cache (see [`crate::convert`]):
//! blocks, anchors and links, for EPUB chapters and HTML pages alike.

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::document::{
    Align, Block, BlockKind, DocumentError, Link, OffsetMap, Run, TextStyle,
};
use paperpaw_sdk::lang::Lang;
use paperpaw_sdk::{Storage, StorageError};

use crate::convert::{self, CachePaths, HEADER, INDEX};

/// The length of a converted chapter's text, if its conversion is
/// complete: `<n>.end` says it, and `<n>.txt` is as long.
pub(crate) fn converted_length(storage: &dyn Storage, paths: &CachePaths) -> Option<u32> {
    let length = u32_at(storage, &paths.end, 0).ok()?;
    (storage.size(&paths.text).ok()? == u64::from(length)).then_some(length)
}

pub(crate) fn u32_at(storage: &dyn Storage, path: &str, offset: u64) -> Result<u32, StorageError> {
    let mut buf = [0u8; 4];
    if storage.read_at(path, offset, &mut buf)? != 4 {
        return Err(StorageError::Io);
    }
    Ok(u32::from_le_bytes(buf))
}

/// An index entry as stored.
type Entry = [u8; INDEX as usize];

/// How many index entries are read at once (and kept for the blocks that
/// follow): 32 of 24 bytes, 768 bytes of heap per open book.
const WINDOW: u32 = 32;

/// Where reading blocks one after the other is: the block found last, and
/// the index entries around it, read at once (a page is laid out a block at a
/// time: one read of the index serves 32 blocks, #250).
#[derive(Debug, Default)]
pub(crate) struct Seek {
    last: Option<u32>,
    /// The index entries from block `first` on.
    window: Vec<u8>,
    first: u32,
}

impl Seek {
    /// The entries of `blocks` kept for block `i` and, where there is one, the
    /// one after it: the entry and the start of the next block.
    fn kept(&self, blocks: u32, i: u32) -> Option<(Entry, Option<u32>)> {
        // Checked all through: `i` comes from a book's own numbers.
        let want = if i < blocks.saturating_sub(1) { 2 } else { 1 };
        let at = usize::try_from(i.checked_sub(self.first)?)
            .ok()?
            .checked_mul(INDEX as usize)?;
        let bytes = self
            .window
            .get(at..at.checked_add(want * INDEX as usize)?)?;
        let mut entry: Entry = [0; INDEX as usize];
        entry.copy_from_slice(&bytes[..INDEX as usize]);
        let next = (want == 2).then(|| {
            let n = INDEX as usize;
            u32::from_le_bytes([bytes[n], bytes[n + 1], bytes[n + 2], bytes[n + 3]])
        });
        Some((entry, next))
    }
}

/// Index entry `i` of a chapter and the start of the next one, if there
/// is one: from the window kept, or read with the entries after them at
/// once; `None` where fewer bytes came back (then they are read one by one,
/// as that tells).
fn entry_and_next(
    storage: &dyn Storage,
    paths: &CachePaths,
    blocks: u32,
    seek: &mut Seek,
    i: u32,
) -> Result<Option<(Entry, Option<u32>)>, DocumentError> {
    if let Some(found) = seek.kept(blocks, i) {
        return Ok(Some(found));
    }
    let count = WINDOW.min(blocks.saturating_sub(i));
    let bytes = (count * INDEX) as usize;
    seek.window.clear();
    crate::fallible::reserve_exact(&mut seek.window, bytes)?;
    seek.window.resize(bytes, 0);
    let read = storage.read_at(
        &paths.index,
        u64::from(i) * u64::from(INDEX),
        &mut seek.window,
    )?;
    // Whole entries only.
    seek.window.truncate(read / INDEX as usize * INDEX as usize);
    seek.first = i;
    Ok(seek.kept(blocks, i))
}

/// Reads block `i` of a chapter.
fn read_block(
    storage: &dyn Storage,
    paths: &CachePaths,
    (length, blocks): (u32, u32),
    seek: &mut Seek,
    (i, color): (u32, bool),
) -> Result<Block, DocumentError> {
    let (entry, end, stop) = match entry_and_next(storage, paths, blocks, seek, i)? {
        Some((entry, next)) => (entry, next.unwrap_or(length), record_end(next, length)),
        None => {
            let mut entry = [0u8; INDEX as usize];
            storage.read_at(&paths.index, u64::from(i) * u64::from(INDEX), &mut entry)?;
            let next = if i + 1 < blocks {
                Some(u32_at(
                    storage,
                    &paths.index,
                    (u64::from(i) + 1) * u64::from(INDEX),
                )?)
            } else {
                None
            };
            (entry, next.unwrap_or(length), record_end(next, length))
        }
    };
    read_entry(storage, paths, entry, (end, stop), color)
}

/// Where the record of a block ends in the text file: where the next record
/// begins, which is its header before the next block's start; the end of
/// the file for the last block.
fn record_end(next: Option<u32>, length: u32) -> Option<u32> {
    Some(next.map_or(length, |start| start.saturating_sub(HEADER)))
}

/// The runs and the inline languages a record holds after its text.
fn decode_lists(
    text: &[u8],
    len: usize,
    runs_len: usize,
    filled: usize,
) -> Result<(Vec<Run>, Vec<Lang>), DocumentError> {
    let lang_bytes = text.get(len + runs_len..filled).unwrap_or_default();
    let mut inline_langs: Vec<Lang> = Vec::new();
    crate::fallible::reserve_exact(&mut inline_langs, lang_bytes.len() / 4)?;
    inline_langs.extend(
        lang_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| Lang::from_bits(u32::from_le_bytes(*b)).unwrap_or(Lang::NONE)),
    );
    let run_bytes = text
        .get(len..filled.min(len + runs_len))
        .unwrap_or_default();
    let mut runs: Vec<Run> = Vec::new();
    crate::fallible::reserve_exact(&mut runs, run_bytes.len() / convert::RUN)?;
    runs.extend(run_bytes.as_chunks::<{ convert::RUN }>().0.iter().map(|r| {
        Run::new(
            u32::from(u16::from_le_bytes([r[0], r[1]])),
            u32::from(u16::from_le_bytes([r[2], r[3]])),
            TextStyle::from_bits(u32::from_le_bytes([r[4], r[5], r[6], r[7]])),
        )
    }));
    Ok((runs, inline_langs))
}

/// The most a record of the text file takes: its header, the text, a run of
/// each of its bytes at worst, and the inline languages.
const MAX_RECORD: usize = HEADER as usize
    + convert::LIMIT
    + convert::LIMIT * convert::RUN
    + paperpaw_sdk::document::MAX_INLINE_LANGS as usize * 4;

/// What a record's header and its index entry say: the length of its text, of
/// its runs, and the number of its inline languages. A record the converter
/// cannot have written means the cache is damaged.
fn record_sizes(
    header: &[u8; HEADER as usize],
    entry: Entry,
) -> Result<(usize, usize, usize), DocumentError> {
    let len = usize::from(u16::from_le_bytes([header[4], header[5]]));
    let run_count = usize::from(u16::from_le_bytes([header[6], header[7]]));
    let langs = usize::from(entry[21]);
    if len > convert::LIMIT
        || run_count > len
        || langs > usize::from(paperpaw_sdk::document::MAX_INLINE_LANGS)
    {
        return Err(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
            "cache record",
        )));
    }
    Ok((len, run_count * convert::RUN, langs))
}

/// The record `len` bytes long at `at` of the text file, in one read: its
/// header, text, runs and languages. `None` where the file gave fewer bytes or
/// the room is not there: the caller then reads the header first, which tells
/// a damaged index from a lack of memory.
fn whole_record(
    storage: &dyn Storage,
    path: &str,
    at: u32,
    len: usize,
) -> Result<Option<Vec<u8>>, DocumentError> {
    let Ok(mut record) = crate::fallible::zeroed(len) else {
        return Ok(None);
    };
    let mut filled = 0;
    while filled < len {
        let n = storage.read_at(path, u64::from(at) + filled as u64, &mut record[filled..])?;
        if n == 0 {
            return Ok(None);
        }
        filled += n;
    }
    Ok(Some(record))
}

/// Reads the block of index entry `entry`, ending at `end`. `record_end` is
/// where its record ends in the text file, where it is known (the next
/// record begins there): the record is then read whole, with one read.
fn read_entry(
    storage: &dyn Storage,
    paths: &CachePaths,
    entry: Entry,
    (end, record_end): (u32, Option<u32>),
    color: bool,
) -> Result<Block, DocumentError> {
    let start = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
    let mut header = [0u8; HEADER as usize];
    let at =
        start
            .checked_sub(HEADER)
            .ok_or(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
                "cache index",
            )))?;
    let whole = match record_end {
        Some(stop) if stop > at && (stop - at) as usize <= MAX_RECORD => {
            whole_record(storage, &paths.text, at, (stop - at) as usize)?
        }
        _ => None,
    };
    let whole = whole.filter(|record| record.len() >= header.len());
    let (text, filled, len, runs_len) = match whole {
        Some(mut record) => {
            header.copy_from_slice(&record[..HEADER as usize]);
            record.drain(..HEADER as usize);
            let (len, runs_len, langs) = record_sizes(&header, entry)?;
            let filled = record.len().min(len + runs_len + langs * 4);
            record.truncate(filled);
            (record, filled, len, runs_len)
        }
        None => {
            storage.read_at(&paths.text, u64::from(at), &mut header)?;
            let (len, runs_len, langs) = record_sizes(&header, entry)?;
            // The text, its runs and its inline languages after it, read at once.
            let mut text = crate::fallible::zeroed(len + runs_len + langs * 4)?;
            let mut filled = 0;
            while filled < text.len() {
                let n = storage.read_at(
                    &paths.text,
                    u64::from(start) + filled as u64,
                    &mut text[filled..],
                )?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            (text, filled, len, runs_len)
        }
    };
    let (runs, inline_langs) = decode_lists(&text, len, runs_len, filled)?;
    // The block keeps its text, not the room the runs took: a copy of
    // exactly its size where the buffer held more.
    let text_bytes = &text[..filled.min(len)];
    let mut kept = Vec::new();
    crate::fallible::reserve_exact(&mut kept, text_bytes.len())?;
    kept.extend_from_slice(text_bytes);
    drop(text);
    let text = kept;
    let mut kind = convert::decode_kind(header[0], header[1]);
    let mut text = match String::from_utf8(text) {
        Ok(text) => text,
        Err(e) => crate::fallible::lossy(e.as_bytes())?.into_owned(),
    };
    let mut image = None;
    if kind == BlockKind::Image {
        // `cache name<TAB>alternative text`, the picture in either kind,
        // or noted to be decoded when its page shows.
        let (path, alt) = text.split_once('\t').unwrap_or((&text, ""));
        image = crate::pictures::open_or_noted(storage, path, color)?;
        let alt = crate::fallible::string(alt)?;
        if image.is_none() {
            // Not decodable: its alternative text instead.
            kind = BlockKind::Paragraph;
        }
        text = alt;
    }
    let mut block = Block::new(kind, start, end, text)
        .continues(convert::continues(header[2]))
        .continued(convert::continued(header[2]))
        .map(OffsetMap::IDENTITY)
        .runs(runs)
        .align(Align::from_bits(header[3]))
        .indent(Some(i16::from_le_bytes([entry[4], entry[5]])).filter(|&i| i != convert::NO_INDENT))
        .space(Some(u16::from_le_bytes([entry[6], entry[7]])).filter(|&s| s != convert::NO_SPACE))
        .size(Some(u16::from_le_bytes([entry[8], entry[9]])).filter(|&s| s != 0))
        .margins(
            u16::from_le_bytes([entry[10], entry[11]]),
            u16::from_le_bytes([entry[12], entry[13]]),
        )
        .line_height(Some(u16::from_le_bytes([entry[14], entry[15]])).filter(|&h| h != 0))
        .lang(
            Lang::from_bits(u32::from_le_bytes([
                entry[16], entry[17], entry[18], entry[19],
            ]))
            .unwrap_or(Lang::NONE),
        )
        .dir(convert::dir_from_bits(entry[20]))
        .drop_cap(entry[22])
        .image(image);
    // The list as a boxed slice of exactly its size (no growth to refuse).
    block.inline_langs = (!inline_langs.is_empty()).then(|| inline_langs.into_boxed_slice());
    Ok(block)
}

/// The block of a converted chapter containing `offset`, or the first
/// after it; `None` past its end. `seek` holds the block found the time
/// before, and becomes this one: read on, the next is mostly the one right
/// after it, found with two reads where a search takes a dozen.
pub(crate) fn block_at(
    storage: &dyn Storage,
    paths: &CachePaths,
    length: u32,
    blocks: u32,
    offset: u32,
    color: bool,
    seek: &mut Seek,
) -> Result<Option<Block>, DocumentError> {
    if blocks == 0 || offset >= length {
        return Ok(None);
    }
    let start = |block: u32| -> Result<u32, DocumentError> {
        if block >= blocks {
            return Ok(u32::MAX);
        }
        Ok(u32_at(
            storage,
            &paths.index,
            u64::from(block) * u64::from(INDEX),
        )?)
    };
    // The block after the last one, or the last one again: its entry and
    // the next start read at once, and kept for reading the block.
    let mut found = None;
    if let Some(before) = seek.last {
        for block in [before + 1, before] {
            if block >= blocks {
                continue;
            }
            if let Some((entry, next)) = entry_and_next(storage, paths, blocks, seek, block)? {
                let from = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
                if from <= offset && offset < next.unwrap_or(u32::MAX) {
                    seek.last = Some(block);
                    return read_entry(
                        storage,
                        paths,
                        entry,
                        (next.unwrap_or(length), record_end(next, length)),
                        color,
                    )
                    .map(Some);
                }
            } else if start(block)? <= offset && offset < start(block + 1)? {
                found = Some(block);
                break;
            }
        }
    }
    let block = match found {
        Some(block) => block,
        None => {
            // The last block whose text starts at or before the offset.
            let (mut low, mut high) = (0u32, blocks);
            while low < high {
                let mid = low + (high - low) / 2;
                if start(mid)? <= offset {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            low.saturating_sub(1)
        }
    };
    seek.last = Some(block);
    read_block(storage, paths, (length, blocks), seek, (block, color)).map(Some)
}

/// Longer lines of cache files are cut: nothing sensible is that long.
const MAX_LINE: usize = 2048;

/// Calls `visit` with each line of a cache file until it returns
/// `false`, reading in small pieces: without its line break, invalid
/// UTF-8 replaced, cut at [`MAX_LINE`] bytes. No room for a line is
/// `OutOfMemory`; a file that cannot be read ends the lines.
pub(crate) fn lines(
    storage: &dyn Storage,
    path: &str,
    mut visit: impl FnMut(&str) -> bool,
) -> Result<(), DocumentError> {
    lines_from(storage, path, 0, |line, _| visit(line))
}

/// As [`lines`], from byte `from` on (where a line starts); `visit` also
/// gets where the line after it starts.
pub(crate) fn lines_from(
    storage: &dyn Storage,
    path: &str,
    from: u64,
    mut visit: impl FnMut(&str, u64) -> bool,
) -> Result<(), DocumentError> {
    let mut chunk = [0u8; 512];
    let mut line: Vec<u8> = Vec::new();
    let mut offset = from;
    loop {
        let Ok(n) = storage.read_at(path, offset, &mut chunk) else {
            return Ok(());
        };
        for (i, &byte) in chunk[..n].iter().enumerate() {
            if byte != b'\n' {
                if line.len() < MAX_LINE {
                    crate::fallible::push(&mut line, byte)?;
                }
                continue;
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let next = offset + i as u64 + 1;
            if !visit(&crate::fallible::lossy(&line)?, next) {
                return Ok(());
            }
            line.clear();
        }
        offset += n as u64;
        if n == 0 {
            if !line.is_empty() {
                visit(&crate::fallible::lossy(&line)?, offset);
            }
            return Ok(());
        }
    }
}

/// Where an anchor of a document was found last: its chapter, and where
/// its line starts in the chapter's `<n>.anc`. Anchors are mostly looked
/// up in the order of the text (the entries of the contents, the printed
/// pages), often one twice in a row, and each is then found there or
/// right after it, where a search from the start would read the list
/// again for each.
#[derive(Debug, Default)]
pub(crate) struct AnchorHint(Cell<(u32, u64)>);

impl AnchorHint {
    /// Forgets the place, as when its chapter is converted again.
    pub fn reset(&self) {
        self.0.set((0, 0));
    }
}

/// The offset of anchor `id` in converted chapter `section`, looked for
/// from where `hint` says on, then from the start up to there.
pub(crate) fn anchor(
    storage: &dyn Storage,
    paths: &CachePaths,
    section: u32,
    id: &str,
    hint: &AnchorHint,
) -> Option<u32> {
    let (last, from) = hint.0.get();
    let from = if last == section { from } else { 0 };
    let mut found = None;
    for (start, end) in [(from, u64::MAX), (0, from)] {
        let mut at = start;
        // No room for a line finds no anchor here.
        let _ = lines_from(storage, &paths.anchors, start, |line, next| {
            if at >= end {
                return false;
            }
            if let Some((name, offset)) = line.split_once('\t')
                && name == id
            {
                found = offset.parse().ok();
                hint.0.set((section, at));
            }
            at = next;
            found.is_none()
        });
        if found.is_some() || from == 0 {
            break;
        }
    }
    found
}

/// The links of a converted chapter that overlap `from..to`, in order.
pub(crate) fn links(storage: &dyn Storage, paths: &CachePaths, from: u32, to: u32) -> Vec<Link> {
    let mut links = Vec::new();
    // No room for a line leaves the links read so far.
    let _ = lines(storage, &paths.links, |line| {
        let mut parts = line.splitn(3, '\t');
        let (Some(start), Some(end), Some(href)) = (parts.next(), parts.next(), parts.next())
        else {
            return true;
        };
        let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) else {
            return true;
        };
        if start >= to {
            return false;
        }
        if end > from {
            links.push(Link::new(start, end, href));
        }
        true
    });
    links
}

#[cfg(test)]
mod kept_tests {
    use super::*;

    fn kept_window(first: u32, entries: usize) -> Seek {
        Seek {
            last: None,
            window: alloc::vec![7; entries * INDEX as usize],
            first,
        }
    }

    #[test]
    fn a_block_number_at_the_end_of_the_range_gives_none_not_a_panic() {
        let seek = kept_window(0, 2);
        // `i + 1` and the offsets of such a number overflow; a book's own
        // numbers are not trusted.
        assert!(seek.kept(u32::MAX, u32::MAX).is_none());
        assert!(seek.kept(u32::MAX, u32::MAX - 1).is_none());
        assert!(kept_window(u32::MAX, 1).kept(u32::MAX, u32::MAX).is_some());
    }

    #[test]
    fn the_last_block_of_a_chapter_has_no_next_start() {
        let seek = kept_window(4, 2);
        let (_, next) = seek.kept(6, 5).unwrap();
        assert_eq!(next, None);
        let (_, next) = seek.kept(6, 4).unwrap();
        assert_eq!(next, Some(u32::from_le_bytes([7; 4])));
    }
}

#[cfg(test)]
mod huge_chapter_tests {
    use super::*;
    use paperpaw_sdk::DirEntry;

    /// A storage that answers every small read with zeros and no window
    /// read at all: a chapter whose index claims more blocks than u32
    /// byte offsets can address (a book's own numbers are not trusted).
    struct Zeros;

    impl Storage for Zeros {
        fn read_at(&self, _: &str, _: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            if buf.len() <= INDEX as usize {
                buf.fill(0);
                return Ok(buf.len());
            }
            Ok(0)
        }
        fn size(&self, _: &str) -> Result<u64, StorageError> {
            Err(StorageError::Io)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Io)
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Io)
        }
        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Err(StorageError::Io)
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Io)
        }
        fn rename(&self, _: &str, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Io)
        }
    }

    fn seek() -> Seek {
        Seek {
            last: None,
            window: Vec::new(),
            first: 0,
        }
    }

    #[test]
    fn reading_a_block_past_the_range_of_u32_offsets_does_not_overflow() {
        let paths = CachePaths::new("/c", "0").unwrap();
        // Block 200 million of 4 billion: its next entry sits at
        // `(i + 1) * 24`, past 4 GiB.
        let _ = read_block(
            &Zeros,
            &paths,
            (10, u32::MAX),
            &mut seek(),
            (200_000_000, false),
        );
    }

    #[test]
    fn finding_a_block_among_billions_does_not_overflow_the_offsets() {
        let paths = CachePaths::new("/c", "0").unwrap();
        // The search halves a range of 4 billion blocks: `mid * 24` is
        // past 4 GiB on its first steps.
        let _ = block_at(&Zeros, &paths, 10, u32::MAX, 5, false, &mut seek());
    }
}
