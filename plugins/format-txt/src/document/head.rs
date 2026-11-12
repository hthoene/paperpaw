//! What opening a text learns from its head (the first [`super::SNIFF`]
//! bytes), read in pieces of [`CHUNK`] bytes so no more than one piece is
//! held at once (#248: the library's details open of a text was the start
//! peak with the whole head held). Each answer is the one the whole head
//! read at once gives.

use alloc::vec::Vec;

use super::{Breaks, DocumentError, is_space, read_onto};
use paperpaw_sdk::Storage;

/// The most of the file held at once while opening.
pub(super) const CHUNK: u32 = 4096;

/// The Project Gutenberg start marker.
const START: &[u8] = b"*** START OF";

/// Lines up to this length are counted one by one for the typical length;
/// longer ones (at most a few in a head) are kept in a list.
const SHORT: usize = 512;

/// What the head says before its encoding is known.
pub(super) struct Scan {
    /// Whether `0..to` is UTF-8 (a sequence cut off at `to` counts unless
    /// the head is the whole file).
    pub(super) utf8: bool,
    /// Where the Project Gutenberg start marker is, and where the line
    /// after it starts (`to` where none does in the head).
    pub(super) marker: Option<(u32, u32)>,
}

/// Reads `from..to` of `path` into `buf`, replacing what it held.
fn piece(
    storage: &dyn Storage,
    path: &str,
    from: u32,
    to: u32,
    buf: &mut Vec<u8>,
) -> Result<(), DocumentError> {
    buf.clear();
    read_onto(storage, path, from, to, buf)
}

/// Tells `0..to` of `path`'s encoding and finds its start marker, a piece at
/// a time. Pieces overlap by the marker's length less one, so a marker on a
/// border is found; the UTF-8 check goes on where the last whole character
/// ended (at most three bytes before a piece's end, inside the overlap).
pub(super) fn scan(
    storage: &dyn Storage,
    path: &str,
    to: u32,
    complete: bool,
) -> Result<Scan, DocumentError> {
    let mut buf = Vec::new();
    let mut utf8 = true;
    // The first byte the UTF-8 check has not read (a character's first).
    let mut checked = 0u32;
    // The marker, and where the newline ending its line is looked for from.
    let mut marker: Option<(u32, u32)> = None;
    let mut line = None;
    let mut pos = 0u32;
    while pos < to {
        let end = pos.saturating_add(CHUNK).min(to);
        piece(storage, path, pos, end, &mut buf)?;
        let last = end == to;
        if utf8 {
            let rest = buf.get((checked - pos) as usize..).unwrap_or_default();
            match core::str::from_utf8(rest) {
                Ok(_) => checked = end,
                Err(error) if error.error_len().is_some() => utf8 = false,
                Err(_) if last => utf8 = !complete,
                Err(error) => checked += error.valid_up_to() as u32,
            }
        }
        if marker.is_none() {
            marker = super::find(&buf, START).map(|at| (pos + at as u32, pos + at as u32));
        }
        if let (Some((at, from)), None) = (marker, line) {
            let rest = buf.get((from - pos) as usize..).unwrap_or_default();
            match rest.iter().position(|&b| b == b'\n') {
                Some(i) => line = Some(from + i as u32 + 1),
                None => marker = Some((at, end)),
            }
        }
        if last {
            break;
        }
        // `checked` is at least `end - 3` while the check goes on.
        pos = end - (START.len() as u32 - 1);
    }
    Ok(Scan {
        utf8,
        marker: marker.map(|(at, _)| (at, line.unwrap_or(to))),
    })
}

/// Gives `visit` each line of `0..to` (split at `\n`, the last one cut at
/// `to`), a piece at a time, each piece starting at a line's start. A line
/// longer than a piece (no header line is) is left out.
pub(super) fn lines(
    storage: &dyn Storage,
    path: &str,
    to: u32,
    visit: &mut dyn FnMut(&[u8]) -> Result<(), DocumentError>,
) -> Result<(), DocumentError> {
    let mut buf = Vec::new();
    let mut pos = 0u32;
    // Inside a line longer than a piece: up to its end is left out.
    let mut skipping = false;
    if to == 0 {
        return visit(&[]);
    }
    while pos < to {
        let end = pos.saturating_add(CHUNK).min(to);
        piece(storage, path, pos, end, &mut buf)?;
        if skipping {
            match buf.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    skipping = false;
                    pos += i as u32 + 1;
                    // The last line, empty, ends at `to`.
                    if pos == to {
                        visit(&[])?;
                    }
                }
                None => pos = end,
            }
            continue;
        }
        let whole = if end == to {
            buf.len()
        } else {
            match buf.iter().rposition(|&b| b == b'\n') {
                Some(i) => i,
                None => {
                    skipping = true;
                    pos = end;
                    continue;
                }
            }
        };
        for line in buf[..whole].split(|&b| b == b'\n') {
            visit(line)?;
        }
        if end == to {
            break;
        }
        pos += whole as u32 + 1;
        if pos == to {
            visit(&[])?;
        }
    }
    Ok(())
}

/// How paragraphs break in `from..to` of `path` and how long its lines
/// typically are, as [`super::detect_breaks`] and
/// [`super::typical_line_length`] tell from the bytes held at once.
pub(super) fn look(
    storage: &dyn Storage,
    path: &str,
    from: u32,
    to: u32,
) -> Result<(Breaks, usize), DocumentError> {
    let mut buf = Vec::new();
    let mut look = Look::new()?;
    let mut pos = from;
    while pos < to {
        let end = pos.saturating_add(CHUNK).min(to);
        piece(storage, path, pos, end, &mut buf)?;
        for &byte in &buf {
            look.byte(byte)?;
        }
        pos = end;
    }
    look.end()
}

/// The lines of a sample so far, counted as they go by.
struct Look {
    lines: usize,
    blank: usize,
    /// The current line's length, and its length without trailing space.
    len: usize,
    trimmed: usize,
    /// Whether the last byte was a line break (none yet: the sample is
    /// empty, one empty line).
    after_break: bool,
    /// How many lines of each length up to [`SHORT`] (index 0 unused).
    short: Vec<u16>,
    long: Vec<usize>,
}

impl Look {
    fn new() -> Result<Self, DocumentError> {
        let mut short = Vec::new();
        short
            .try_reserve_exact(SHORT + 1)
            .map_err(|_| DocumentError::OutOfMemory)?;
        short.resize(SHORT + 1, 0);
        Ok(Self {
            lines: 0,
            blank: 0,
            len: 0,
            trimmed: 0,
            after_break: false,
            short,
            long: Vec::new(),
        })
    }

    fn byte(&mut self, byte: u8) -> Result<(), DocumentError> {
        if byte == b'\n' {
            self.line()?;
            self.after_break = true;
            return Ok(());
        }
        self.after_break = false;
        self.len += 1;
        if !is_space(byte) {
            self.trimmed = self.len;
        }
        Ok(())
    }

    /// Ends the current line.
    fn line(&mut self) -> Result<(), DocumentError> {
        self.lines += 1;
        if self.trimmed == 0 {
            self.blank += 1;
        } else if let Some(count) = self.short.get_mut(self.trimmed) {
            *count = count.saturating_add(1);
        } else {
            self.long
                .try_reserve(1)
                .map_err(|_| DocumentError::OutOfMemory)?;
            self.long.push(self.trimmed);
        }
        self.len = 0;
        self.trimmed = 0;
        Ok(())
    }

    fn end(mut self) -> Result<(Breaks, usize), DocumentError> {
        // A trailing line break does not start another (blank) line for the
        // breaks; the empty line after it has no length either way.
        if self.after_break {
            self.len = 0;
        } else {
            self.line()?;
        }
        let breaks = if self.blank >= 2 && self.blank * 100 >= self.lines * 8 {
            Breaks::BlankLines
        } else {
            Breaks::Lines
        };
        Ok((breaks, self.typical()))
    }

    /// The length most lines reach (75th percentile), 0 below 40 or with
    /// fewer than 8 lines that have one.
    fn typical(&mut self) -> usize {
        let counted: usize = self.short.iter().map(|&n| usize::from(n)).sum();
        let total = counted + self.long.len();
        if total < 8 {
            return 0;
        }
        let mut rank = total * 3 / 4;
        let mut typical = 0;
        for (len, &n) in self.short.iter().enumerate() {
            if rank < usize::from(n) {
                typical = len;
                break;
            }
            rank -= usize::from(n);
        }
        if typical == 0 {
            paperpaw_sdk::sort::sort(&mut self.long);
            typical = self.long.get(rank).copied().unwrap_or(0);
        }
        if typical >= 40 { typical } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{detect_breaks, find, is_utf8, line_after, typical_line_length};
    use super::*;
    use alloc::format;
    use alloc::string::String;
    use paperpaw_sdk::{DirEntry, MemoryStorage, StorageError};

    const PATH: &str = "book.txt";

    /// The encoding, the marker and its line, the header lines and the look.
    type Told = (bool, Option<(u32, u32)>, Vec<Vec<u8>>, Breaks, usize);

    /// What the whole head held at once says, as opening read it before.
    fn whole(bytes: &[u8], to: u32) -> Told {
        let head = &bytes[..to as usize];
        let complete = to as usize == bytes.len();
        let marker = find(head, START).map(|at| (at as u32, line_after(head, at) as u32));
        // A line longer than a piece is left out (none in a real header).
        let lines = marker.map_or_else(Vec::new, |(at, _)| {
            let header = &head[..at as usize];
            let count = header.split(|&b| b == b'\n').count();
            header
                .split(|&b| b == b'\n')
                .enumerate()
                .filter(|(i, line)| {
                    line.len() < CHUNK as usize || (*i + 1 == count && line.len() == CHUNK as usize)
                })
                .map(|(_, line)| line.to_vec())
                .collect()
        });
        let from = marker.map_or(0, |(_, line)| line as usize);
        let sample = &head[from..];
        let look = (detect_breaks(sample), typical_line_length(sample).unwrap());
        (is_utf8(head, complete), marker, lines, look.0, look.1)
    }

    fn pieces(bytes: &[u8], to: u32) -> Told {
        let storage = MemoryStorage::new().with(PATH, bytes.to_vec());
        let scan = scan(&storage, PATH, to, to as usize == bytes.len()).unwrap();
        let mut seen = Vec::new();
        if let Some((at, _)) = scan.marker {
            lines(&storage, PATH, at, &mut |line| {
                seen.push(line.to_vec());
                Ok(())
            })
            .unwrap();
        }
        let from = scan.marker.map_or(0, |(_, line)| line);
        let (breaks, typical) = look(&storage, PATH, from, to).unwrap();
        (scan.utf8, scan.marker, seen, breaks, typical)
    }

    fn same(bytes: &[u8]) {
        let to = (bytes.len() as u32).min(super::super::SNIFF);
        let (a, b) = (pieces(bytes, to), whole(bytes, to));
        let short = |lines: &[Vec<u8>]| lines.iter().map(Vec::len).collect::<Vec<_>>();
        assert_eq!(
            (a.0, a.1, a.3, a.4),
            (b.0, b.1, b.3, b.4),
            "{} bytes",
            bytes.len()
        );
        assert_eq!(short(&a.2), short(&b.2), "{} bytes", bytes.len());
        assert!(a.2 == b.2, "{} bytes: lines differ", bytes.len());
    }

    /// Texts with what a piece's border could cut: a marker, its line, a
    /// character of several bytes, a line break, long and short lines.
    fn texts() -> Vec<Vec<u8>> {
        let mut texts: Vec<Vec<u8>> = Vec::new();
        for cut in [
            0usize, 1, 5, 11, 12, 13, 2000, 4084, 4090, 4093, 4094, 4095, 4096, 4097,
        ] {
            let pad = "x".repeat(cut);
            texts.push(format!("{pad}*** START OF THE BOOK ***\r\nText.\n").into_bytes());
            texts.push(format!("{pad}é is two bytes, € three, 𝄞 four.\n").into_bytes());
            let mut header = String::from("Title: Ä title\r\nAuthor: B\r\n");
            header.push_str(&pad);
            header.push_str("\n*** START OF IT\nOne line of text that is long enough to count.\n");
            header.push_str(
                &"A wrapped line of prose, some sixty bytes or more in all.\n".repeat(90),
            );
            texts.push(header.into_bytes());
            let mut latin = format!("{pad}caf").into_bytes();
            latin.push(0xe9);
            latin.extend_from_slice(b" au lait\n");
            texts.push(latin);
        }
        texts.push(Vec::new());
        texts.push(b"\n".to_vec());
        texts.push(b"\n\n".to_vec());
        texts.push(b"one\n\ntwo\n\nthree".to_vec());
        texts.push(format!("{}\n", "y".repeat(9000)).repeat(3).into_bytes());
        texts.push(format!("Title: {}\n*** START OF\n", "t".repeat(5000)).into_bytes());
        texts.push("Para one\nline two\n\n".repeat(3000).into_bytes());
        texts.push("a\n".repeat(9000).into_bytes());
        texts.push(" \r\n".repeat(7000).into_bytes());
        // A character cut off at the head's end, in a longer file.
        let mut cut = "e".repeat(16 * 1024 - 1).into_bytes();
        cut.extend_from_slice("€ and on".as_bytes());
        texts.push(cut);
        texts
    }

    #[test]
    fn pieces_tell_what_the_whole_head_tells() {
        for text in texts() {
            same(&text);
        }
    }

    #[test]
    fn a_header_line_longer_than_a_piece_is_left_out() {
        let text = format!("Title: {}\nAuthor: A\n*** START OF\n", "t".repeat(5000));
        let (_, marker, lines, ..) = pieces(text.as_bytes(), text.len() as u32);
        assert_eq!(marker, Some((5018, text.len() as u32)));
        assert_eq!(lines, [b"Author: A".to_vec(), Vec::new()]);
    }

    /// A storage that keeps the largest read asked of it.
    struct Largest {
        inner: MemoryStorage,
        most: core::cell::Cell<usize>,
    }

    impl Storage for Largest {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.most.set(self.most.get().max(buf.len()));
            self.inner.read_at(path, offset, buf)
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
    fn opening_reads_one_piece_at_a_time() {
        let prose = "A wrapped line of prose, some sixty bytes or more in all.\r\n".repeat(1500);
        let text = format!(
            "Title: A Book\r\nAuthor: Somebody\r\n{}*** START OF THE BOOK ***\r\n{prose}\
             *** END OF THE BOOK ***\r\n{}",
            "Header line.\r\n".repeat(600),
            "Licence line.\r\n".repeat(600),
        );
        let storage = Largest {
            inner: MemoryStorage::new().with(PATH, text),
            most: core::cell::Cell::new(0),
        };
        let document = super::super::TxtDocument::open(&storage, PATH).unwrap();
        assert_eq!(document.metadata.title.as_deref(), Some("A Book"));
        assert_eq!(document.breaks, Breaks::Lines);
        assert!(document.wrap_width >= 40);
        assert!(
            storage.most.get() <= CHUNK as usize + START.len(),
            "{}",
            storage.most.get()
        );
    }
}
