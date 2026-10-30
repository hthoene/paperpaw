use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Ref, RefCell};
use core::fmt;

use crate::PLUGIN;
use paperpaw_sdk::document::{
    Align, Block, BlockKind, Document, DocumentError, Locator, Metadata, OffsetMap,
};
use paperpaw_sdk::library::key;
use paperpaw_sdk::{Env, Storage, read_file};

/// Bytes inspected to detect encoding, paragraph style and metadata.
const SNIFF: u32 = 16 * 1024;
/// Chunk size and range for finding the end of a Project Gutenberg text.
const TAIL_CHUNK: u32 = 8 * 1024;
const TAIL_RANGE: u32 = 64 * 1024;
/// Paragraphs longer than this (in bytes) are split into several blocks.
const LIMIT: u32 = 4096;
/// Long paragraphs are split at the first whitespace after multiples of
/// this, so the pieces do not depend on where reading started.
const GRID: u32 = 2048;
/// Extra bytes read around a position.
const MARGIN: u32 = 512;
/// How far a split point may move forward to reach whitespace.
const SPLIT_SEARCH: usize = 256;
/// Bytes read beyond what a block needs, so the blocks after it are found
/// in what was read: about two pages of the X4.
const AHEAD: u32 = 4096;
/// Bytes of a UTF-16 file converted at a time.
const UTF16_PIECE: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Windows1252,
}

/// How paragraphs are separated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Breaks {
    /// By blank lines; single line breaks are hard wraps.
    BlankLines,
    /// Every line is a paragraph.
    Lines,
}

/// A paragraph found while scanning a window of the file.
#[derive(Debug, Clone, Copy)]
struct Para {
    /// First non-whitespace byte (the window start for `partial`).
    start: u32,
    /// Just after the last non-whitespace byte.
    text_end: u32,
    /// Started before the scanned window.
    partial: bool,
    single_line: bool,
}

/// The bytes of the file read last, from `start` on.
#[derive(Clone, Default)]
struct Window {
    start: u32,
    bytes: Vec<u8>,
}

impl fmt::Debug for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let end = self.start as usize + self.bytes.len();
        write!(f, "Window({}..{end})", self.start)
    }
}

/// An open plain text file.
#[derive(Debug, Clone)]
pub struct TxtDocument {
    path: String,
    /// Content range (BOM and Project Gutenberg boilerplate excluded).
    start: u32,
    end: u32,
    encoding: Encoding,
    breaks: Breaks,
    /// Typical length of hard-wrapped lines; shorter lines end a
    /// paragraph (poems, lists). 0 disables the rule.
    wrap_width: usize,
    metadata: Metadata,
    /// What was read around the last block, for the blocks that follow.
    window: RefCell<Window>,
}

impl TxtDocument {
    pub fn open(storage: &dyn Storage, path: &str) -> Result<Self, DocumentError> {
        let mut size = file_size(storage, path)?;
        let mut head = read(storage, path, 0, size.min(SNIFF))?;
        // UTF-16 is read from a copy in UTF-8, made when it is first opened.
        let mut text = String::from(path);
        let big_endian = match head.get(..2) {
            Some([0xff, 0xfe]) => Some(false),
            Some([0xfe, 0xff]) => Some(true),
            _ => None,
        };
        if let Some(big_endian) = big_endian {
            text = utf8_copy(storage, path, size, big_endian)?;
            size = file_size(storage, &text)?;
            head = read(storage, &text, 0, size.min(SNIFF))?;
        }
        let bom = head.starts_with(&[0xef, 0xbb, 0xbf]);
        let encoding = if bom || is_utf8(&head, size <= SNIFF) {
            Encoding::Utf8
        } else {
            Encoding::Windows1252
        };

        let mut document = TxtDocument {
            path: text,
            start: if bom { 3 } else { 0 },
            end: size,
            encoding,
            breaks: Breaks::Lines,
            wrap_width: 0,
            metadata: Metadata::default(),
            window: RefCell::default(),
        };

        // Project Gutenberg: metadata in the header, licence around the text.
        if let Some(marker) = find(&head, b"*** START OF") {
            document.read_header(&head[..marker]);
            document.start = line_after(&head, marker) as u32;
        }
        if let Some(end) = find_end_marker(storage, &document.path, document.start, size)? {
            document.end = end;
        }

        let sample_end = (document.end.min(head.len() as u32)).max(document.start) as usize;
        let sample = &head[document.start as usize..sample_end];
        document.breaks = detect_breaks(sample);
        document.wrap_width = typical_line_length(sample);
        if document.metadata.title.is_none() {
            let name = path.rsplit('/').next().unwrap_or(path);
            let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
            document.metadata.title = Some(String::from(stem));
        }
        Ok(document)
    }

    fn read_header(&mut self, header: &[u8]) {
        for line in header.split(|&b| b == b'\n') {
            let (text, _) = self.decode(line, 0);
            let text = text.trim();
            if let Some(title) = text.strip_prefix("Title:") {
                self.metadata.title = Some(String::from(title.trim()));
            } else if let Some(author) = text.strip_prefix("Author:") {
                self.metadata.authors.push(String::from(author.trim()));
            } else if let Some(language) = text.strip_prefix("Language:") {
                self.metadata.language = language_tag(language.trim()).map(String::from);
            }
        }
    }

    /// Scans `buf` (file bytes from `w0`) for paragraphs.
    fn paragraphs(&self, buf: &[u8], w0: u32) -> Vec<Para> {
        let mut paras: Vec<Para> = Vec::new();
        // The content start behaves like the line after a blank line.
        let mut prev_blank = w0 == self.start;
        // Length of the previous line (unknown for the window's first line).
        let mut prev_len = usize::MAX;
        let mut pos = 0usize;
        loop {
            let line_end = buf[pos..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(buf.len(), |i| pos + i);
            let line = &buf[pos..line_end];
            let partial = pos == 0 && w0 != self.start;
            let lead = line.iter().take_while(|&&b| is_space(b)).count();
            let blank = lead == line.len();
            let trail = line
                .iter()
                .rposition(|&b| !is_space(b))
                .map_or(0, |i| i + 1);
            let at = |i: usize| w0 + (pos + i) as u32;

            match self.breaks {
                Breaks::BlankLines => {
                    if !blank {
                        if partial {
                            paras.push(Para {
                                start: w0,
                                text_end: at(trail),
                                partial: true,
                                single_line: false,
                            });
                        } else if prev_blank || self.starts_paragraph(lead, prev_len) {
                            paras.push(Para {
                                start: at(lead),
                                text_end: at(trail),
                                partial: false,
                                single_line: true,
                            });
                        } else if let Some(last) = paras.last_mut() {
                            last.text_end = at(trail);
                            last.single_line = false;
                        }
                    }
                    prev_blank = blank;
                    prev_len = if partial { usize::MAX } else { trail };
                }
                Breaks::Lines => {
                    let (start, text_end) = if blank {
                        (at(0), at(0))
                    } else {
                        (at(lead), at(trail))
                    };
                    paras.push(Para {
                        start: if partial { w0 } else { start },
                        text_end,
                        partial,
                        single_line: true,
                    });
                }
            }
            if line_end == buf.len() {
                return paras;
            }
            pos = line_end + 1;
        }
    }

    /// Whether a line following a non-blank line starts a new paragraph
    /// even without a blank line: indented lines (tables of contents,
    /// verse) and lines after a clearly short line (poems, lists).
    fn starts_paragraph(&self, lead: usize, prev_len: usize) -> bool {
        lead > 0 || (self.wrap_width > 0 && prev_len < self.wrap_width * 6 / 10)
    }

    /// Start of the piece of a long paragraph near grid point `k`: just
    /// after the first whitespace at or after `k * GRID`.
    fn split_point(&self, buf: &[u8], w0: u32, k: u32) -> u32 {
        let pos = k.saturating_mul(GRID);
        let Some(rel) = pos.checked_sub(w0).map(|r| r as usize) else {
            return pos;
        };
        if rel >= buf.len() {
            return w0 + buf.len() as u32;
        }
        let search = &buf[rel..buf.len().min(rel + SPLIT_SEARCH)];
        if let Some(i) = search.iter().position(|&b| is_space(b)) {
            return pos + i as u32 + 1;
        }
        // No whitespace nearby: split anywhere, but not inside a character.
        let mut r = rel;
        while self.encoding == Encoding::Utf8 && r < buf.len() && buf[r] & 0xc0 == 0x80 {
            r += 1;
        }
        w0 + r as u32
    }

    /// The bytes `from..to` of the file (less at its end), through the
    /// window of the last call: what it holds of them is not read again,
    /// so paging forward reads each part of the file about once.
    fn window(
        &self,
        storage: &dyn Storage,
        from: u32,
        to: u32,
    ) -> Result<Ref<'_, [u8]>, DocumentError> {
        {
            let mut window = self.window.borrow_mut();
            let Window { start, bytes } = &mut *window;
            let end = *start + bytes.len() as u32;
            if from < *start || to > end {
                // Keep what the window holds from `from` on, read the rest.
                let kept = if from >= *start && from < end {
                    (from - *start) as usize
                } else {
                    bytes.len()
                };
                bytes.drain(..kept);
                *start = from;
                let more = from + bytes.len() as u32;
                let ahead = to.saturating_add(AHEAD).min(self.end);
                if let Err(error) = read_onto(storage, &self.path, more, ahead, bytes) {
                    bytes.clear();
                    return Err(error);
                }
            }
        }
        let window = self.window.borrow();
        let (lo, hi) = ((from - window.start) as usize, (to - window.start) as usize);
        Ok(Ref::map(window, |w| {
            &w.bytes[lo.min(w.bytes.len())..hi.min(w.bytes.len())]
        }))
    }

    /// Decodes file bytes starting at source offset `base` into normalized
    /// text: `\r` before `\n` and stray BOMs are dropped, line breaks, tabs
    /// and control characters become spaces.
    fn decode(&self, bytes: &[u8], base: u32) -> (String, OffsetMap) {
        let mut text = String::with_capacity(bytes.len());
        let mut map = OffsetMap::default();
        let mut i = 0;
        while i < bytes.len() {
            let (ch, len) = match self.encoding {
                Encoding::Utf8 => decode_utf8(&bytes[i..]),
                Encoding::Windows1252 => (cp1252(bytes[i]), 1),
            };
            let next = base + (i + len) as u32;
            match ch {
                // A line end of its own (old Macs) is a space, before `\n`
                // it is dropped.
                '\r' if bytes.get(i + 1) != Some(&b'\n') => text.push(' '),
                '\r' | '\u{feff}' => map.checkpoint(text.len(), next),
                c if c.is_control() => {
                    text.push(' ');
                    if len != 1 {
                        map.checkpoint(text.len(), next);
                    }
                }
                c => {
                    text.push(c);
                    if c.len_utf8() != len {
                        map.checkpoint(text.len(), next);
                    }
                }
            }
            i += len;
        }
        (text, map)
    }
}

impl Document for TxtDocument {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, _: u32) -> u32 {
        self.end
    }

    fn progress(&self, at: Locator) -> f32 {
        let length = self.end.saturating_sub(self.start).max(1);
        (at.offset.saturating_sub(self.start) as f32 / length as f32).min(1.0)
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        if at.section != 0 {
            return Ok(None);
        }
        let mut o = at.offset.max(self.start);
        // Each round skips whitespace between paragraphs, a window at most;
        // long runs of blank lines (pages of a PDF) take several rounds.
        // Every round moves `o` forward, so the loop ends.
        while o < self.end {
            let w0 = o.saturating_sub(LIMIT + MARGIN).max(self.start);
            let w1 = o.saturating_add(LIMIT + MARGIN).min(self.end);
            let buf = self.window(storage, w0, w1)?;
            let paras = self.paragraphs(&buf, w0);

            let Some(i) = paras.iter().rposition(|p| p.start <= o) else {
                // Whitespace before the first paragraph of the window, or
                // a window of whitespace only.
                o = paras.first().map_or(w1, |p| p.start);
                continue;
            };
            let p = paras[i];
            let next = paras
                .get(i + 1)
                .map(|n| n.start)
                .or((w1 == self.end).then_some(self.end));
            if o > p.text_end || (o == p.text_end && p.text_end > p.start) {
                // In the whitespace after a paragraph: go to the next one.
                o = next.unwrap_or(w1.max(o + 1));
                continue;
            }

            let long = p.partial || p.text_end - p.start > LIMIT;
            let (start, text_end, end, continues, continued) = if long {
                // Only grid points inside the paragraph split it, so its
                // first piece starts where it does from any position.
                let inside = |k: u32| p.partial || k.saturating_mul(GRID) > p.start;
                let k = o / GRID;
                let (k, mut piece_start) = match self.split_point(&buf, w0, k) {
                    point if inside(k) && point <= o => (k, point),
                    _ if k > 0 && inside(k - 1) => (k - 1, self.split_point(&buf, w0, k - 1)),
                    _ => (p.start / GRID, p.start),
                };
                if !p.partial && piece_start <= p.start {
                    piece_start = p.start;
                }
                let next_point = self.split_point(&buf, w0, k + 1);
                let continued = next_point < p.text_end;
                let (text_end, end) = if continued {
                    let rel = |x: u32| (x - w0) as usize;
                    let piece = &buf[rel(piece_start)..rel(next_point)];
                    let trail = piece
                        .iter()
                        .rposition(|&b| !is_space(b))
                        .map_or(0, |i| i + 1);
                    (piece_start + trail as u32, next_point)
                } else {
                    (p.text_end, next.unwrap_or(p.text_end))
                };
                (
                    piece_start,
                    text_end,
                    end,
                    piece_start != p.start || p.partial,
                    continued,
                )
            } else {
                (
                    p.start,
                    p.text_end,
                    next.unwrap_or(p.text_end),
                    false,
                    false,
                )
            };

            let rel = |x: u32| (x - w0) as usize;
            let (text, map) = self.decode(&buf[rel(start)..rel(text_end)], start);
            let heading =
                self.breaks == Breaks::BlankLines && p.single_line && !long && is_heading(&text);
            return Ok(Some(
                Block::new(
                    if heading {
                        BlockKind::Heading(2)
                    } else {
                        BlockKind::Paragraph
                    },
                    start,
                    end,
                    text,
                )
                .continues(continues)
                .continued(continued)
                .map(map)
                .align(Align::Auto),
            ));
        }
        Ok(None)
    }
}

/// A short line without lowercase letters, e.g. `CHAPTER I.`
fn is_heading(text: &str) -> bool {
    text.chars().count() <= 60
        && text.chars().any(char::is_alphabetic)
        && !text.chars().any(char::is_lowercase)
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
}

/// Searches the end of the file backwards in small chunks for the Project
/// Gutenberg end marker; returns the start of its line.
fn find_end_marker(
    storage: &dyn Storage,
    path: &str,
    start: u32,
    size: u32,
) -> Result<Option<u32>, DocumentError> {
    const MARKER: &[u8] = b"*** END OF";
    let lowest = size.saturating_sub(TAIL_RANGE).max(start);
    let mut to = size;
    while to > lowest {
        let from = to.saturating_sub(TAIL_CHUNK).max(lowest);
        // Overlap chunks so a marker on a chunk border is found.
        let chunk = read(
            storage,
            path,
            from,
            to.saturating_add(MARKER.len() as u32).min(size),
        )?;
        if let Some(marker) = rfind(&chunk, MARKER) {
            // The line start may lie in the previous chunk.
            let line = line_start(&chunk, marker);
            if line > 0 || from == lowest {
                return Ok(Some(from + line as u32));
            }
            let before = read(storage, path, from.saturating_sub(256).max(start), from)?;
            let back = before.len() - line_start(&before, before.len());
            return Ok(Some(from - back as u32));
        }
        to = from;
    }
    Ok(None)
}

fn file_size(storage: &dyn Storage, path: &str) -> Result<u32, DocumentError> {
    u32::try_from(storage.size(path)?)
        .map_err(|_| DocumentError::Malformed(Cow::Borrowed("file larger than 4 GiB")))
}

/// The path of a copy in UTF-8 of the UTF-16 file at `path` (`size`
/// bytes, a byte order mark first), in the plugin's folder: made a piece
/// at a time when it is missing or was made from another file.
fn utf8_copy(
    storage: &dyn Storage,
    path: &str,
    size: u32,
    big_endian: bool,
) -> Result<String, DocumentError> {
    let base = format!("{}/{}", Env::plugin_dir(PLUGIN.as_str()), key(path));
    let (copy, stamp_path) = (format!("{base}.txt"), format!("{base}.source"));
    // Its size and a hash of its first and last 4 KiB tell the file from
    // another of its name (storages tell no time a file was changed).
    let mut ends = read(storage, path, 0, size.min(UTF16_PIECE))?;
    ends.extend_from_slice(&read(
        storage,
        path,
        size.saturating_sub(UTF16_PIECE),
        size,
    )?);
    let stamp = format!("{size} {:016x}\n", paperpaw_sdk::digest::fnv1a64(&ends));
    drop(ends);
    if read_file(storage, &stamp_path, 64).ok().as_deref() == Some(stamp.as_bytes())
        && storage.exists(&copy)
    {
        return Ok(copy);
    }
    // The stamp goes first and comes last: a copy cut short is made again.
    storage.remove(&stamp_path)?;
    storage.write(&copy, b"")?;
    let mut raw = Vec::new();
    let mut utf8 = Vec::new();
    // Each unit takes at most 3 bytes in UTF-8 (a pair of them 4), and a
    // high surrogate left over from the piece before adds 3 more.
    raw.try_reserve_exact(UTF16_PIECE as usize)
        .and_then(|()| utf8.try_reserve_exact(UTF16_PIECE as usize / 2 * 3 + 3))
        .map_err(|_| DocumentError::OutOfMemory)?;
    let mut high = None;
    let mut at = 2;
    while at < size {
        raw.clear();
        read_onto(
            storage,
            path,
            at,
            at.saturating_add(UTF16_PIECE).min(size),
            &mut raw,
        )?;
        // A last odd byte is no unit.
        let units = raw.as_chunks::<2>().0;
        if units.is_empty() {
            break;
        }
        at += 2 * units.len() as u32;
        utf8.clear();
        for &pair in units {
            let unit = if big_endian {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            };
            push_utf16(&mut utf8, &mut high, unit);
        }
        storage.append(&copy, &utf8)?;
    }
    if high.is_some() {
        storage.append(&copy, "\u{fffd}".as_bytes())?;
    }
    storage.write(&stamp_path, stamp.as_bytes())?;
    Ok(copy)
}

/// Appends UTF-16 unit `unit` to `utf8`; `high` keeps a high surrogate
/// until the unit after it. Surrogates without their pair become U+FFFD.
fn push_utf16(utf8: &mut Vec<u8>, high: &mut Option<u16>, unit: u16) {
    let ch = match (high.take(), unit) {
        (Some(first), 0xdc00..=0xdfff) => {
            let code = 0x10000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(unit) - 0xdc00);
            char::from_u32(code)
        }
        (first, _) => {
            if first.is_some() {
                utf8.extend_from_slice("\u{fffd}".as_bytes());
            }
            if (0xd800..=0xdbff).contains(&unit) {
                *high = Some(unit);
                return;
            }
            char::from_u32(u32::from(unit))
        }
    };
    let ch = ch.unwrap_or('\u{fffd}');
    utf8.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
}

/// Reads `from..to`; fails cleanly instead of aborting when memory is short.
fn read(storage: &dyn Storage, path: &str, from: u32, to: u32) -> Result<Vec<u8>, DocumentError> {
    let mut buf = Vec::new();
    read_onto(storage, path, from, to, &mut buf)?;
    Ok(buf)
}

/// Reads `from..to` onto the end of `buf` (less at the end of the file);
/// `buf` holds more than it should after an error.
fn read_onto(
    storage: &dyn Storage,
    path: &str,
    from: u32,
    to: u32,
    buf: &mut Vec<u8>,
) -> Result<(), DocumentError> {
    let start = buf.len();
    let len = to.saturating_sub(from) as usize;
    buf.try_reserve_exact(len)
        .map_err(|_| DocumentError::OutOfMemory)?;
    buf.resize(start + len, 0);
    let mut filled = start;
    while filled < buf.len() {
        let at = u64::from(from) + (filled - start) as u64;
        let n = storage.read_at(path, at, &mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(())
}

/// Whether `bytes` is valid UTF-8; a sequence cut off at the end counts as
/// valid unless `complete`.
fn is_utf8(bytes: &[u8], complete: bool) -> bool {
    match core::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(error) => !complete && error.error_len().is_none(),
    }
}

/// Decodes one UTF-8 character. A byte that starts none is read as
/// Windows-1252, where such bytes in mostly UTF-8 text most likely come
/// from (e.g. a Latin-1 file whose first letters beyond ASCII come after
/// the part looked at to tell its encoding).
fn decode_utf8(bytes: &[u8]) -> (char, usize) {
    let len = match bytes[0] {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        byte => return (cp1252(byte), 1),
    };
    match bytes.get(..len).and_then(|b| core::str::from_utf8(b).ok()) {
        Some(s) => (s.chars().next().unwrap_or('\u{fffd}'), len),
        None => (cp1252(bytes[0]), 1),
    }
}

/// Windows-1252 to Unicode.
fn cp1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{fffd}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{fffd}', 'Ž',
        '\u{fffd}', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ',
        '\u{fffd}', 'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9f => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

/// The length most hard-wrapped lines reach (75th percentile), or 0 if
/// lines are too short to tell.
fn typical_line_length(sample: &[u8]) -> usize {
    let mut lengths: Vec<usize> = sample
        .split(|&b| b == b'\n')
        .map(|line| {
            line.iter()
                .rposition(|&b| !is_space(b))
                .map_or(0, |i| i + 1)
        })
        .filter(|&len| len > 0)
        .collect();
    if lengths.len() < 8 {
        return 0;
    }
    lengths.sort_unstable();
    let typical = lengths[lengths.len() * 3 / 4];
    if typical >= 40 { typical } else { 0 }
}

fn detect_breaks(sample: &[u8]) -> Breaks {
    let mut lines = 0usize;
    let mut blank = 0usize;
    // A trailing line break does not start another (blank) line.
    let sample = sample.strip_suffix(b"\n").unwrap_or(sample);
    for line in sample.split(|&b| b == b'\n') {
        lines += 1;
        if line.iter().all(|&b| is_space(b)) {
            blank += 1;
        }
    }
    // Hard-wrapped texts have a blank line every few lines.
    if blank >= 2 && blank * 100 >= lines * 8 {
        Breaks::BlankLines
    } else {
        Breaks::Lines
    }
}

fn language_tag(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "english" => "en",
        "german" => "de",
        "french" => "fr",
        "spanish" => "es",
        "italian" => "it",
        "dutch" => "nl",
        "portuguese" => "pt",
        "finnish" => "fi",
        "swedish" => "sv",
        "danish" => "da",
        "polish" => "pl",
        "russian" => "ru",
        _ => return None,
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

/// Index just after the line containing `index`.
fn line_after(bytes: &[u8], index: usize) -> usize {
    bytes[index..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |i| index + i + 1)
}

/// Index of the start of the line containing `index`.
fn line_start(bytes: &[u8], index: usize) -> usize {
    bytes[..index]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use paperpaw_sdk::{DirEntry, MemoryStorage, StorageError};

    fn open(content: impl Into<Vec<u8>>) -> (MemoryStorage, TxtDocument) {
        let storage = MemoryStorage::new().with("books/book.txt", content);
        let document = TxtDocument::open(&storage, "books/book.txt").unwrap();
        (storage, document)
    }

    fn blocks(storage: &dyn Storage, document: &TxtDocument) -> Vec<Block> {
        let mut at = Locator::START;
        let mut blocks = Vec::new();
        while let Some(block) = document.block_at(storage, at).unwrap() {
            assert!(block.end > at.offset, "no progress at {at:?}");
            at = Locator::new(0, block.end);
            blocks.push(block);
        }
        blocks
    }

    fn texts(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|b| b.text.as_str()).collect()
    }

    const GUTENBERG: &str = "\u{feff}The Project Gutenberg eBook of Alice\r\n\r\n\
        Title: Alice's Adventures in Wonderland\r\nAuthor: Lewis Carroll\r\n\
        Language: English\r\n\r\n*** START OF THE PROJECT GUTENBERG EBOOK ALICE ***\r\n\r\n\
        CHAPTER I.\r\n\r\nAlice was beginning to get very tired\r\nof sitting by her sister.\r\n\
        \r\n\r\nSo she was considering.\r\n\r\n*** END OF THE PROJECT GUTENBERG EBOOK ***\r\n\
        Licence text that must not be shown.\r\n";

    #[test]
    fn reads_project_gutenberg_files() {
        let (storage, document) = open(GUTENBERG);
        let metadata = document.metadata();
        assert_eq!(
            metadata.title.as_deref(),
            Some("Alice's Adventures in Wonderland")
        );
        assert_eq!(metadata.authors, ["Lewis Carroll"]);
        assert_eq!(metadata.language.as_deref(), Some("en"));

        let blocks = blocks(&storage, &document);
        assert_eq!(
            texts(&blocks),
            [
                "CHAPTER I.",
                "Alice was beginning to get very tired of sitting by her sister.",
                "So she was considering."
            ]
        );
        assert_eq!(blocks[0].kind, BlockKind::Heading(2));
        assert_eq!(blocks[1].kind, BlockKind::Paragraph);
        // Offsets map back to the file despite the dropped "\r".
        let source = GUTENBERG.as_bytes();
        let b = &blocks[1];
        let of = b.text.find("of").unwrap();
        let offset = b.source_offset(of) as usize;
        assert_eq!(&source[offset..offset + 2], b"of");
        assert_eq!(b.text_index(offset as u32), of);
        assert_eq!(b.end, blocks[2].start);
    }

    #[test]
    fn positions_between_paragraphs_go_to_the_next_one() {
        let (storage, document) = open("one\n\n\n\ntwo\n\nthree\n");
        let blocks = blocks(&storage, &document);
        assert_eq!(texts(&blocks), ["one", "two", "three"]);
        let between = document
            .block_at(&storage, Locator::new(0, 5))
            .unwrap()
            .unwrap();
        assert_eq!(between.text, "two");
    }

    #[test]
    fn long_runs_of_blank_lines_do_not_end_the_book() {
        // Text from a PDF: pages of blank lines, far more than one window.
        let blank = " \r\n".repeat(64 * 1024);
        let text = format!("One.\n\nTwo.\n\n{blank}Three.\n\n{blank}{blank}Four.\n");
        let (storage, document) = open(text);
        let blocks = blocks(&storage, &document);
        assert_eq!(texts(&blocks), ["One.", "Two.", "Three.", "Four."]);
        // A position in the blank lines leads to the next paragraph.
        let inside = document
            .block_at(&storage, Locator::new(0, blocks[2].start + 1000))
            .unwrap()
            .unwrap();
        assert_eq!(inside.text, "Four.");
    }

    /// A storage that counts the bytes read from it.
    struct Counting {
        inner: MemoryStorage,
        bytes: core::cell::Cell<usize>,
    }

    impl Storage for Counting {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            let n = self.inner.read_at(path, offset, buf)?;
            self.bytes.set(self.bytes.get() + n);
            Ok(n)
        }

        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.inner.size(path)
        }

        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.inner.write(path, data)
        }

        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.inner.append(path, data)
        }

        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.inner.list(dir, visit)
        }

        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.inner.remove(path)
        }
    }

    #[test]
    fn paging_forward_reads_the_file_about_once() {
        let paragraph = "A paragraph of ordinary prose, long enough to wrap. ".repeat(6);
        let text = format!("{paragraph}\n\n").repeat(800);
        let storage = Counting {
            inner: MemoryStorage::new().with("books/book.txt", text.clone()),
            bytes: core::cell::Cell::new(0),
        };
        let document = TxtDocument::open(&storage, "books/book.txt").unwrap();
        storage.bytes.set(0);
        let blocks = blocks(&storage, &document);
        assert_eq!(blocks.len(), 800);
        let read = storage.bytes.get();
        assert!(
            read < text.len() * 3 / 2,
            "{read} bytes read of {}",
            text.len()
        );
    }

    #[test]
    fn one_paragraph_per_line_without_blank_lines() {
        let (storage, document) = open("first line\nsecond line\n\nthird\n");
        let blocks = blocks(&storage, &document);
        assert_eq!(texts(&blocks), ["first line", "second line", "", "third"]);
        assert_eq!(document.metadata().title.as_deref(), Some("book"));
    }

    #[test]
    fn decodes_windows_1252() {
        let mut bytes = b"Gr\xf6\xdfe \x84Zitat\x93 \x80 5".to_vec();
        bytes.push(b'\n');
        let (storage, document) = open(bytes);
        let block = document
            .block_at(&storage, Locator::START)
            .unwrap()
            .unwrap();
        assert_eq!(block.text, "Größe „Zitat“ € 5");
        // "Zitat" starts at byte 7 in the file but later in the UTF-8 text.
        let index = block.text.find("Zitat").unwrap();
        assert_eq!(block.source_offset(index), 7);
        assert_eq!(block.text_index(7), index);
    }

    #[test]
    fn splits_long_paragraphs_deterministically() {
        let words: Vec<String> = (0..4000).map(|i| format!("word{i}")).collect();
        let paragraph = words.join(" ");
        // Two short paragraphs first, so the sample shows the blank-line style.
        let content = format!("Title.\n\nIntro.\n\n{paragraph}\n\nOutro.\n\n");
        let (storage, document) = open(content.clone());
        let blocks = blocks(&storage, &document);

        assert!(blocks.len() > 10);
        assert!(blocks.iter().all(|b| b.text.len() <= (LIMIT as usize)));
        assert_eq!(texts(&blocks[..2]), ["Title.", "Intro."]);
        assert_eq!(blocks.last().map(|b| b.text.as_str()), Some("Outro."));
        let middle = &blocks[2..blocks.len() - 1];
        assert!(!middle[0].continues);
        assert!(middle[1..].iter().all(|b| b.continues));
        // Each piece but the last says the next continues it.
        let continued: Vec<bool> = middle.iter().map(|b| b.continued).collect();
        assert!(continued[..continued.len() - 1].iter().all(|c| *c));
        assert!(!continued[continued.len() - 1] && !blocks[0].continued);
        let joined: Vec<&str> = middle
            .iter()
            .flat_map(|b| b.text.split_whitespace())
            .collect();
        assert_eq!(joined, words.iter().map(String::as_str).collect::<Vec<_>>());

        // Every position inside a block yields the same block.
        for block in middle {
            let last_char = block.source_offset(block.text.len() - 1);
            for offset in [block.start, (block.start + last_char) / 2, last_char] {
                let found = document
                    .block_at(&storage, Locator::new(0, offset))
                    .unwrap()
                    .unwrap();
                assert_eq!(found.start, block.start, "offset {offset}");
                assert_eq!(found.end, block.end, "offset {offset}");
            }
        }
    }

    #[test]
    fn progress_ignores_boilerplate() {
        let (_, document) = open(GUTENBERG);
        assert_eq!(document.progress(Locator::new(0, document.start)), 0.0);
        assert_eq!(document.progress(Locator::new(0, document.end)), 1.0);
    }

    #[test]
    fn stray_bytes_in_utf8_are_read_as_windows_1252() {
        // UTF-8 (ASCII) as far as the encoding is told, Latin-1 later on.
        let mut bytes = "Plain prose at the start of the file.\n\n"
            .repeat(1000)
            .into_bytes();
        bytes.extend_from_slice(b"Caf\xe9 cr\xe8me, \x93quoted\x94, na\xefve \xc3\xa4.\n");
        let (storage, document) = open(bytes);
        let blocks = blocks(&storage, &document);
        let last = blocks.last().unwrap();
        assert_eq!(last.text, "Café crème, “quoted”, naïve ä.");
        // Offsets still map back to the file.
        let index = last.text.find("naïve").unwrap();
        let offset = last.source_offset(index);
        let file = storage.get("books/book.txt").unwrap();
        assert_eq!(&file[offset as usize..offset as usize + 3], b"na\xef");
        assert_eq!(decode_utf8(&[0xff, b'a']), ('ÿ', 1));
        assert_eq!(decode_utf8(&[0xc3]), ('Ã', 1));
    }

    #[test]
    fn a_replaced_utf_16_file_of_the_same_size_is_copied_anew() {
        let utf16 = |text: &str| -> Vec<u8> {
            core::iter::once(0xfeff)
                .chain(text.encode_utf16())
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        let (storage, document) = open(utf16("Alpha."));
        assert_eq!(texts(&blocks(&storage, &document)), ["Alpha."]);
        storage.write("books/book.txt", &utf16("Bravo.")).unwrap();
        let document = TxtDocument::open(&storage, "books/book.txt").unwrap();
        assert_eq!(texts(&blocks(&storage, &document)), ["Bravo."]);
    }

    #[test]
    fn reads_utf_16_in_either_byte_order() {
        // Characters of one, two and three bytes in UTF-8 and a surrogate
        // pair, at offsets that put some pairs across converted pieces.
        let line = "Grüße 日本 𝄞.";
        let text = format!("{line}\n\n").repeat(2000);
        for big_endian in [false, true] {
            let mut bytes = Vec::new();
            for unit in core::iter::once(0xfeff).chain(text.encode_utf16()) {
                let pair = if big_endian {
                    u16::to_be_bytes(unit)
                } else {
                    u16::to_le_bytes(unit)
                };
                bytes.extend_from_slice(&pair);
            }
            // A high surrogate without its pair, and an odd byte.
            bytes.extend_from_slice(&if big_endian {
                [0xd8, 0x00]
            } else {
                [0x00, 0xd8]
            });
            bytes.push(b'x');
            let (storage, document) = open(bytes);
            assert_eq!(document.metadata().title.as_deref(), Some("book"));
            let blocks = blocks(&storage, &document);
            assert_eq!(blocks.len(), 2001, "big endian: {big_endian}");
            assert!(blocks[..2000].iter().all(|b| b.text == line));
            assert_eq!(blocks[2000].text, "\u{fffd}");
            // Opened again, the copy made before is read.
            let copy = storage.get(&document.path).unwrap();
            let again = TxtDocument::open(&storage, "books/book.txt").unwrap();
            assert_eq!(again.path, document.path);
            assert_eq!(storage.get(&again.path).unwrap(), copy);
        }
    }

    #[test]
    fn indented_and_short_lines_start_paragraphs() {
        let prose = "This is a hard-wrapped line of ordinary prose that reaches the width,\n\
                     and it continues on the next line until the paragraph ends here.\n\n";
        let text = format!(
            "{prose}{prose}{prose}{prose}Contents\n\n CHAPTER I.  Down the Rabbit-Hole\n \
             CHAPTER II. The Pool of Tears\n\nTwinkle, twinkle, little bat!\n\
             How I wonder what you're at!\n\n"
        );
        let (storage, document) = open(text);
        let blocks = blocks(&storage, &document);
        let texts = texts(&blocks);
        assert!(
            texts.contains(&"CHAPTER I.  Down the Rabbit-Hole"),
            "{texts:?}"
        );
        assert!(texts.contains(&"CHAPTER II. The Pool of Tears"));
        assert!(texts.contains(&"Twinkle, twinkle, little bat!"));
        assert!(texts.contains(&"How I wonder what you're at!"));
        // Hard-wrapped prose is still joined.
        assert_eq!(texts[0].matches("width, and it continues").count(), 1);
    }
}
