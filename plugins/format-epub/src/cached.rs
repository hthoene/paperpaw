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

/// Index entry `i` of a chapter and the start of the next one, if there
/// is one, read at once as they lie side by side; `None` where fewer
/// bytes came back (then they are read one by one, as that tells).
fn entry_and_next(
    storage: &dyn Storage,
    paths: &CachePaths,
    blocks: u32,
    i: u32,
) -> Result<Option<(Entry, Option<u32>)>, StorageError> {
    let mut both = [0u8; INDEX as usize + 4];
    let has_next = i + 1 < blocks;
    let want = if has_next { both.len() } else { INDEX as usize };
    let read = storage.read_at(&paths.index, u64::from(i * INDEX), &mut both[..want])?;
    if read < want {
        return Ok(None);
    }
    let mut entry: Entry = [0; INDEX as usize];
    entry.copy_from_slice(&both[..INDEX as usize]);
    let at = INDEX as usize;
    let next =
        has_next.then(|| u32::from_le_bytes([both[at], both[at + 1], both[at + 2], both[at + 3]]));
    Ok(Some((entry, next)))
}

/// Reads block `i` of a chapter.
fn read_block(
    storage: &dyn Storage,
    paths: &CachePaths,
    length: u32,
    blocks: u32,
    i: u32,
    color: bool,
) -> Result<Block, DocumentError> {
    let (entry, end) = match entry_and_next(storage, paths, blocks, i)? {
        Some((entry, next)) => (entry, next.unwrap_or(length)),
        None => {
            let mut entry = [0u8; INDEX as usize];
            storage.read_at(&paths.index, u64::from(i * INDEX), &mut entry)?;
            let end = if i + 1 < blocks {
                u32_at(storage, &paths.index, u64::from((i + 1) * INDEX))?
            } else {
                length
            };
            (entry, end)
        }
    };
    read_entry(storage, paths, entry, end, color)
}

/// Reads the block of index entry `entry`, ending at `end`.
fn read_entry(
    storage: &dyn Storage,
    paths: &CachePaths,
    entry: Entry,
    end: u32,
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
    storage.read_at(&paths.text, u64::from(at), &mut header)?;
    let len = usize::from(u16::from_le_bytes([header[4], header[5]]));
    let run_count = usize::from(u16::from_le_bytes([header[6], header[7]]));
    // A record the converter cannot have written: the cache is damaged.
    if len > convert::LIMIT || run_count > len {
        return Err(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
            "cache record",
        )));
    }
    let langs = usize::from(entry[21]);
    if langs > usize::from(paperpaw_sdk::document::MAX_INLINE_LANGS) {
        return Err(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
            "cache record",
        )));
    }
    // The text, its runs and its inline languages after it, read at once.
    let runs_len = run_count * convert::RUN;
    let mut text = alloc::vec![0u8; len + runs_len + langs * 4];
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
    let inline_langs: Vec<Lang> = text
        .get(len + runs_len..filled)
        .unwrap_or_default()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| Lang::from_bits(u32::from_le_bytes(*b)).unwrap_or(Lang::NONE))
        .collect();
    let runs = text
        .get(len..filled.min(len + runs_len))
        .unwrap_or_default()
        .as_chunks::<{ convert::RUN }>()
        .0
        .iter()
        .map(|r| {
            Run::new(
                u32::from(u16::from_le_bytes([r[0], r[1]])),
                u32::from(u16::from_le_bytes([r[2], r[3]])),
                TextStyle::from_bits(u32::from_le_bytes([r[4], r[5], r[6], r[7]])),
            )
        })
        .collect();
    text.truncate(filled.min(len));
    // The block keeps its text, not the room the runs took.
    text.shrink_to_fit();
    let mut kind = convert::decode_kind(header[0], header[1]);
    let mut text = String::from_utf8(text)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    let mut image = None;
    if kind == BlockKind::Image {
        // `cache name<TAB>alternative text`, the picture in either kind,
        // or noted to be decoded when its page shows.
        let (path, alt) = text.split_once('\t').unwrap_or((&text, ""));
        image = crate::pictures::open_or_noted(storage, path, color);
        let alt = String::from(alt);
        if image.is_none() {
            // Not decodable: its alternative text instead.
            kind = BlockKind::Paragraph;
        }
        text = alt;
    }
    Ok(Block::new(kind, start, end, text)
        .continues(convert::continues(header[2]))
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
        .inline_langs(&inline_langs)
        .image(image))
}

/// The block of a converted chapter containing `offset`, or the first
/// after it; `None` past its end. `last` is the block found the time
/// before, and becomes this one: read on, the next is mostly the one right
/// after it, found with two reads where a search takes a dozen.
pub(crate) fn block_at(
    storage: &dyn Storage,
    paths: &CachePaths,
    length: u32,
    blocks: u32,
    offset: u32,
    color: bool,
    last: &mut Option<u32>,
) -> Result<Option<Block>, DocumentError> {
    if blocks == 0 || offset >= length {
        return Ok(None);
    }
    let start = |block: u32| -> Result<u32, DocumentError> {
        if block >= blocks {
            return Ok(u32::MAX);
        }
        Ok(u32_at(storage, &paths.index, u64::from(block * INDEX))?)
    };
    // The block after the last one, or the last one again: its entry and
    // the next start read at once, and kept for reading the block.
    let mut found = None;
    if let Some(before) = *last {
        for block in [before + 1, before] {
            if block >= blocks {
                continue;
            }
            if let Some((entry, next)) = entry_and_next(storage, paths, blocks, block)? {
                let from = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
                if from <= offset && offset < next.unwrap_or(u32::MAX) {
                    *last = Some(block);
                    return read_entry(storage, paths, entry, next.unwrap_or(length), color)
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
    *last = Some(block);
    read_block(storage, paths, length, blocks, block, color).map(Some)
}

/// Longer lines of cache files are cut: nothing sensible is that long.
const MAX_LINE: usize = 2048;

/// Calls `visit` with each line of a cache file until it returns
/// `false`, reading in small pieces: without its line break, invalid
/// UTF-8 replaced, cut at [`MAX_LINE`] bytes.
pub(crate) fn lines(storage: &dyn Storage, path: &str, mut visit: impl FnMut(&str) -> bool) {
    lines_from(storage, path, 0, |line, _| visit(line));
}

/// As [`lines`], from byte `from` on (where a line starts); `visit` also
/// gets where the line after it starts.
pub(crate) fn lines_from(
    storage: &dyn Storage,
    path: &str,
    from: u64,
    mut visit: impl FnMut(&str, u64) -> bool,
) {
    let mut chunk = [0u8; 512];
    let mut line: Vec<u8> = Vec::new();
    let mut offset = from;
    loop {
        let Ok(n) = storage.read_at(path, offset, &mut chunk) else {
            return;
        };
        for (i, &byte) in chunk[..n].iter().enumerate() {
            if byte != b'\n' {
                if line.len() < MAX_LINE {
                    line.push(byte);
                }
                continue;
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let next = offset + i as u64 + 1;
            if !visit(&String::from_utf8_lossy(&line), next) {
                return;
            }
            line.clear();
        }
        offset += n as u64;
        if n == 0 {
            if !line.is_empty() {
                visit(&String::from_utf8_lossy(&line), offset);
            }
            return;
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
        lines_from(storage, &paths.anchors, start, |line, next| {
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
    lines(storage, &paths.links, |line| {
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
