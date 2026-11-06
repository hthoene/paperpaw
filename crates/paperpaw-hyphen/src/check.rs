//! The check of a pattern table that is read from untrusted storage: one
//! implementation, read positionally through a bounded buffer so that a table
//! on a card needs no copy in memory ([`check_source`]). The kernel's card
//! reader and `paperpaw-dev package` ([`check_table`], the slice form) share it.

use crate::MAGIC;

use core::fmt;

/// The header of a table, in bytes.
const HEADER: usize = 24;

/// Most index blocks a table may have (the header field at 12).
/// Shipped maximum: de 2,295.
pub const MAX_BLOCKS: u32 = 4096;
/// Longest block in bytes: from one index offset to the next, and for the last
/// block to the end of the patterns (the exceptions offset). Shipped maximum: uk 346.
pub const MAX_BLOCK_BYTES: u32 = 512;
/// Longest exceptions section in bytes: from the exceptions offset to the end of
/// the file. Shipped maximum: ru 3,192.
pub const MAX_EXCEPTION_BYTES: u32 = 8 * 1024;

/// Why a table is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TableError {
    /// Not magic `PPHP` version 1 (the older `QIHP` magic is not accepted here).
    Magic,
    /// The header's own numbers do not fit together or the file: it is cut off,
    /// the index (24 + 4 bytes a block) would end after the exceptions offset, or
    /// the exceptions offset lies past the end of the file.
    Header,
    /// The index's contents are wrong: an offset that is not ascending, or one at
    /// or past the end of the patterns.
    Index,
    /// The exceptions are cut off, or one is not UTF-8.
    Exceptions,
    /// The exceptions do not end at the end of the file.
    Trailing,
    /// More than PaperPaw reads: more than [`MAX_BLOCKS`] blocks, a block longer
    /// than [`MAX_BLOCK_BYTES`] or exceptions longer than [`MAX_EXCEPTION_BYTES`].
    TooLarge,
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TableError::Magic => "not a PPHP version 1 table",
            TableError::Header => "the table's header does not fit the file",
            TableError::Index => "the table's index is not ascending or points past the patterns",
            TableError::Exceptions => "the table's exceptions are cut off or not UTF-8",
            TableError::Trailing => "the table's exceptions do not end at the end of the file",
            TableError::TooLarge => {
                "larger than PaperPaw reads (too many blocks, a block or the exceptions too long)"
            }
        })
    }
}

/// What [`check_source`] found in a table it accepts, so that a reader or a tool
/// does not parse the header again. All offsets and lengths are in bytes.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The header's block count, which is the number of index entries.
    pub blocks: u32,
    /// Bytes of the largest block, as [`MAX_BLOCK_BYTES`] defines a block; 0
    /// when `blocks` is 0.
    pub largest_block: u32,
    /// The header's exceptions offset, absolute from the start of the file.
    pub exceptions_at: u32,
    /// The header's exception count.
    pub exceptions: u32,
    /// The file length minus `exceptions_at`.
    pub exceptions_len: u32,
}

impl core::error::Error for TableError {}

impl Layout {
    /// Where the first pattern is: after the header and the index.
    pub fn entries_at(&self) -> u64 {
        HEADER as u64 + 4 * u64::from(self.blocks)
    }

    /// The size of the whole table in bytes.
    pub fn table_len(&self) -> u64 {
        u64::from(self.exceptions_at) + u64::from(self.exceptions_len)
    }
}

/// The fewest bytes of buffer [`check_source`] works with: one exception entry
/// (a length byte and up to 255 bytes) fits. 512 or more reads less often.
pub const MIN_SCRATCH: usize = 256;

/// Bytes of a table, read at a position: a file on a card, or a slice.
pub trait Source {
    /// Why a read failed.
    type Error;

    /// The size in bytes, as it was when the source was opened.
    fn len(&self) -> u64;

    /// Whether the source has no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Fills `buf` with the bytes from `at`. A read that cannot fill `buf`
    /// completely is an error.
    ///
    /// # Errors
    ///
    /// The source's own error when the bytes cannot be read.
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), Self::Error>;
}

/// Why [`check_source`] could not say that a table is sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckError<E> {
    /// Reading failed.
    Source(E),
    /// The table is refused.
    Table(TableError),
    /// The buffer is shorter than [`MIN_SCRATCH`].
    Scratch,
}

impl<E> From<TableError> for CheckError<E> {
    fn from(error: TableError) -> Self {
        Self::Table(error)
    }
}

/// The little-endian word at `at`; `bytes` holds the whole header.
fn word(bytes: &[u8], at: usize) -> u32 {
    let mut word = [0u8; 4];
    if let Some(from) = bytes.get(at..at + 4) {
        word.copy_from_slice(from);
    }
    u32::from_le_bytes(word)
}

/// Checks that the table in `src` is a PPHP version 1 table a reader can use,
/// and says where its parts are.
///
/// - The magic is `PPHP\x01`; `QIHP`, which [`Patterns::new`](crate::Patterns::new)
///   still reads, is refused.
/// - The header gives the block count, the exceptions offset and the exception
///   count. The index (4 bytes a block, after the 24-byte header) must end at or
///   before the exceptions offset, which must lie in the file; if the header's
///   numbers do not fit together or the file, the error is [`TableError::Header`].
/// - [`MAX_BLOCKS`] and [`MAX_EXCEPTION_BYTES`] are checked before the index is
///   walked, [`MAX_BLOCK_BYTES`] once the walk has ended, so an
///   [`TableError::Index`] anywhere in the index wins over a block that is too
///   long: [`TableError::TooLarge`]. A reader that enforces the same constants
///   never meets a table this refuses for size.
/// - Index offsets, read from the start of the entries, are strictly ascending
///   and each points before the exceptions; otherwise [`TableError::Index`],
///   which is only about the index's contents.
/// - The exceptions are walked: `count` entries of a length byte and that many
///   bytes of UTF-8. The walk must end exactly at the end of the file, so a
///   truncated or padded table is refused. An entry of length 0 is accepted
///   (an empty word hyphenates nothing); `compile` never writes one.
///
/// `scratch` (at least [`MIN_SCRATCH`] bytes) is the only memory used, so
/// nothing grows with the table; the index and the exceptions are read once,
/// in order. The size limit of a plugin kind (2 MiB a table) is not checked
/// here; the caller does that.
///
/// # Errors
///
/// [`CheckError::Table`] naming the first part of the table that is wrong,
/// [`CheckError::Source`] when a read fails, [`CheckError::Scratch`] when
/// `scratch` is too short.
pub fn check_source<S: Source>(
    src: &mut S,
    scratch: &mut [u8],
) -> Result<Layout, CheckError<S::Error>> {
    if scratch.len() < MIN_SCRATCH {
        return Err(CheckError::Scratch);
    }
    let len = src.len();
    // At most the 24 header bytes: fits any `usize`.
    let head = len.min(HEADER as u64) as usize;
    src.read_at(0, &mut scratch[..head])
        .map_err(CheckError::Source)?;
    if head < MAGIC.len() || scratch[..MAGIC.len()] != MAGIC[..] {
        return Err(TableError::Magic.into());
    }
    if head < HEADER {
        return Err(TableError::Header.into());
    }
    let blocks = word(scratch, 12);
    let exceptions_at = word(scratch, 16);
    let exceptions = word(scratch, 20);
    let entries = HEADER as u64 + 4 * u64::from(blocks);
    if u64::from(exceptions_at) > len || entries > u64::from(exceptions_at) {
        return Err(TableError::Header.into());
    }
    if blocks > MAX_BLOCKS {
        return Err(TableError::TooLarge.into());
    }
    let exceptions_len = len - u64::from(exceptions_at);
    if exceptions_len > u64::from(MAX_EXCEPTION_BYTES) {
        return Err(TableError::TooLarge.into());
    }
    let largest_block = check_index(src, scratch, blocks, entries, exceptions_at)?;
    if largest_block > MAX_BLOCK_BYTES {
        return Err(TableError::TooLarge.into());
    }
    check_exceptions(src, scratch, u64::from(exceptions_at), exceptions)?;
    Ok(Layout {
        blocks,
        largest_block,
        exceptions_at,
        exceptions,
        // At most MAX_EXCEPTION_BYTES, checked above.
        exceptions_len: exceptions_len as u32,
    })
}

/// The index: strictly ascending offsets that point into the patterns.
/// Returns the size of the largest block.
fn check_index<S: Source>(
    src: &mut S,
    scratch: &mut [u8],
    blocks: u32,
    entries: u64,
    exceptions_at: u32,
) -> Result<u32, CheckError<S::Error>> {
    // The patterns run from the end of the index to the exceptions offset.
    let patterns = u64::from(exceptions_at) - entries;
    let window = scratch.len() & !3;
    let (mut pos, mut left) = (HEADER as u64, 4 * u64::from(blocks));
    let mut last: Option<u32> = None;
    let mut largest = 0;
    while left > 0 {
        // At most `window`, which is a `usize`.
        let n = left.min(window as u64) as usize;
        src.read_at(pos, &mut scratch[..n])
            .map_err(CheckError::Source)?;
        for chunk in scratch[..n].as_chunks::<4>().0 {
            let offset = u32::from_le_bytes(*chunk);
            if last.is_some_and(|before| offset <= before) || u64::from(offset) >= patterns {
                return Err(TableError::Index.into());
            }
            if let Some(before) = last {
                largest = largest.max(offset - before);
            }
            last = Some(offset);
        }
        pos += n as u64;
        left -= n as u64;
    }
    if let Some(offset) = last {
        // `patterns` is below the u32 exceptions offset.
        largest = largest.max((patterns - u64::from(offset)) as u32);
    }
    Ok(largest)
}

/// `count` entries from `pos` that end exactly at the end of `src`.
fn check_exceptions<S: Source>(
    src: &mut S,
    scratch: &mut [u8],
    mut pos: u64,
    mut count: u32,
) -> Result<(), CheckError<S::Error>> {
    let len = src.len();
    while count > 0 {
        // At most the scratch length, which is a `usize`.
        let n = (len - pos).min(scratch.len() as u64) as usize;
        src.read_at(pos, &mut scratch[..n])
            .map_err(CheckError::Source)?;
        let window = &scratch[..n];
        let mut at = 0;
        while count > 0 {
            let Some(&length) = window.get(at) else { break };
            let Some(word) = window.get(at + 1..at + 1 + usize::from(length)) else {
                break;
            };
            if core::str::from_utf8(word).is_err() {
                return Err(TableError::Exceptions.into());
            }
            at += 1 + word.len();
            count -= 1;
        }
        // A window of MIN_SCRATCH holds a whole entry, unless the table ends.
        if at == 0 {
            return Err(TableError::Exceptions.into());
        }
        pos += at as u64;
    }
    if pos == len {
        Ok(())
    } else {
        Err(TableError::Trailing.into())
    }
}

/// A slice as a [`Source`].
struct Bytes<'a>(&'a [u8]);

/// A read outside the slice (not possible while checking).
struct OutOfRange;

impl Source for Bytes<'_> {
    type Error = OutOfRange;

    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), OutOfRange> {
        let from = usize::try_from(at).map_err(|_| OutOfRange)?;
        let end = from.checked_add(buf.len()).ok_or(OutOfRange)?;
        buf.copy_from_slice(self.0.get(from..end).ok_or(OutOfRange)?);
        Ok(())
    }
}

/// [`check_source`] on a table in memory, for the host tools: accepts exactly
/// the tables the kernel's activation check accepts. It allocates nothing.
///
/// # Errors
///
/// A [`TableError`] naming the first part of the table that is wrong.
pub fn check_table(table: &[u8]) -> Result<Layout, TableError> {
    let mut scratch = [0u8; 2 * MIN_SCRATCH];
    match check_source(&mut Bytes(table), &mut scratch) {
        Ok(layout) => Ok(layout),
        Err(CheckError::Table(why)) => Err(why),
        // Not possible: the buffer is long enough and every read lies inside.
        Err(CheckError::Source(OutOfRange) | CheckError::Scratch) => Err(TableError::Header),
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;
    use crate::compile;

    fn u32_at(table: &[u8], at: usize) -> Option<u32> {
        let bytes: [u8; 4] = table.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(u32::from_le_bytes(bytes))
    }

    fn header(blocks: u32, exceptions: u32, count: u32) -> Vec<u8> {
        let mut t = b"PPHP\x01\x02\x02\x05\0\0\0\0".to_vec();
        t.extend_from_slice(&blocks.to_le_bytes());
        t.extend_from_slice(&exceptions.to_le_bytes());
        t.extend_from_slice(&count.to_le_bytes());
        t
    }

    /// Patterns that are all different, so that there are several index blocks.
    fn letters(count: usize) -> alloc::string::String {
        let mut out = alloc::string::String::new();
        'all: for x in 'a'..='z' {
            for y in 'a'..='z' {
                if out.split(' ').count() > count {
                    break 'all;
                }
                out.push_str(&alloc::format!("{x}{y}1 "));
            }
        }
        out
    }

    fn compiled() -> Vec<u8> {
        compile(
            "hy3ph he2n hena4 hen5at 1na n2at 1tio 2io o2n 1ß 2ü1",
            "ta-ble be-ta",
            2,
            2,
        )
    }

    #[test]
    fn what_compile_makes_passes() {
        assert!(check_table(&compiled()).is_ok());
        let empty = compile("", "", 2, 2);
        let layout = check_table(&empty).unwrap();
        assert_eq!((layout.blocks, layout.largest_block), (0, 0));
        assert_eq!(layout.exceptions_at, 24);
        assert_eq!((layout.exceptions, layout.exceptions_len), (0, 0));
        assert!(check_table(&compile("a1b", "", 1, 1)).is_ok());
        // More patterns than one index block holds.
        let table = compile(&letters(200), "", 2, 2);
        assert!(u32_at(&table, 12).unwrap() >= 3);
        assert!(check_table(&table).is_ok());
    }

    #[test]
    fn the_layout_describes_the_table() {
        let table = compiled();
        let layout = check_table(&table).unwrap();
        assert_eq!(layout.blocks, u32_at(&table, 12).unwrap());
        assert_eq!(layout.exceptions_at, u32_at(&table, 16).unwrap());
        assert_eq!(layout.exceptions, 2);
        let tail = table.len() - layout.exceptions_at as usize;
        assert_eq!(layout.exceptions_len as usize, tail);
        // The only block runs from its offset to the exceptions.
        let entries = 24 + 4 * layout.blocks;
        let offset = u32_at(&table, 24).unwrap();
        assert_eq!(
            layout.largest_block,
            layout.exceptions_at - entries - offset
        );
    }

    /// A table with the given index offsets, `patterns` bytes of patterns and
    /// `exceptions` bytes after them, with the header counts given as such.
    fn raw(offsets: &[u32], patterns: usize, exceptions: &[u8], count: u32) -> Vec<u8> {
        let entries = 24 + 4 * offsets.len();
        let blocks = u32::try_from(offsets.len()).unwrap();
        let mut t = header(blocks, u32::try_from(entries + patterns).unwrap(), count);
        for offset in offsets {
            t.extend_from_slice(&offset.to_le_bytes());
        }
        t.resize(entries + patterns, b'a');
        t.extend_from_slice(exceptions);
        t
    }

    #[test]
    fn each_limit_passes_at_n_and_fails_at_n_plus_one() {
        // Blocks: one byte each.
        let at = |blocks: u32| (0..blocks).collect::<Vec<u32>>();
        let table = raw(&at(MAX_BLOCKS), MAX_BLOCKS as usize, &[], 0);
        let layout = check_table(&table).unwrap();
        assert_eq!((layout.blocks, layout.largest_block), (MAX_BLOCKS, 1));
        let table = raw(&at(MAX_BLOCKS + 1), MAX_BLOCKS as usize + 1, &[], 0);
        assert_eq!(check_table(&table), Err(TableError::TooLarge));
        // The last block ends at the exceptions offset, the others at the next offset.
        let n = MAX_BLOCK_BYTES;
        let table = raw(&[0], n as usize, &[], 0);
        assert_eq!(check_table(&table).unwrap().largest_block, n);
        assert_eq!(
            check_table(&raw(&[0], n as usize + 1, &[], 0)),
            Err(TableError::TooLarge)
        );
        let table = raw(&[0, n, n + 1], n as usize + 2, &[], 0);
        assert_eq!(check_table(&table).unwrap().largest_block, n);
        assert_eq!(
            check_table(&raw(&[0, n + 1], n as usize + 2, &[], 0)),
            Err(TableError::TooLarge)
        );
        // Exceptions: 64 words of 127 bytes make exactly 8 KiB.
        let mut words = Vec::new();
        for _ in 0..64 {
            words.push(127);
            words.resize(words.len() + 127, b'a');
        }
        assert_eq!(words.len(), MAX_EXCEPTION_BYTES as usize);
        let layout = check_table(&raw(&[], 0, &words, 64)).unwrap();
        assert_eq!(layout.exceptions_len, MAX_EXCEPTION_BYTES);
        // One more byte: a zero-length entry, which is accepted below the limit.
        words.push(0);
        assert_eq!(
            check_table(&raw(&[], 0, &words, 65)),
            Err(TableError::TooLarge)
        );
        let layout = check_table(&raw(&[], 0, &[0], 1)).unwrap();
        assert_eq!((layout.exceptions, layout.exceptions_len), (1, 1));
    }

    #[test]
    fn every_truncation_fails() {
        let table = compiled();
        for len in 0..table.len() {
            assert!(
                check_table(&table[..len]).is_err(),
                "{len} of {}",
                table.len()
            );
        }
        assert_eq!(check_table(&table[..3]), Err(TableError::Magic));
        assert_eq!(check_table(&table[..10]), Err(TableError::Header));
    }

    #[test]
    fn an_appended_byte_is_trailing() {
        let mut table = compiled();
        table.push(0);
        assert_eq!(check_table(&table), Err(TableError::Trailing));
    }

    #[test]
    fn the_older_magic_is_refused() {
        let mut table = compiled();
        table[..5].copy_from_slice(b"QIHP\x01");
        assert_eq!(check_table(&table), Err(TableError::Magic));
        table[..5].copy_from_slice(b"PPHP\x02");
        assert_eq!(check_table(&table), Err(TableError::Magic));
    }

    #[test]
    fn an_index_going_down_or_stopping_at_the_exceptions_is_refused() {
        let mut table = compile(&letters(40), "", 2, 2);
        assert!(u32_at(&table, 12).unwrap() >= 3);
        let (a, b) = (24, 28);
        let (x, y) = (table[a..a + 4].to_vec(), table[b..b + 4].to_vec());
        table[a..a + 4].copy_from_slice(&y);
        table[b..b + 4].copy_from_slice(&x);
        assert_eq!(check_table(&table), Err(TableError::Index));
        // An offset at the exceptions offset points past the patterns.
        let mut table = compiled();
        let exceptions = u32_at(&table, 16).unwrap();
        let entries = 24 + 4 * u32_at(&table, 12).unwrap();
        let at = exceptions - entries;
        table[24..28].copy_from_slice(&at.to_le_bytes());
        assert_eq!(check_table(&table), Err(TableError::Index));
    }

    #[test]
    fn the_exceptions_are_walked() {
        let mut one = header(0, 24, 1);
        one.extend_from_slice(&[3, b'a', b'-', b'b']);
        assert_eq!(check_table(&one).map(|l| l.exceptions), Ok(1));
        let mut not_utf8 = header(0, 24, 1);
        not_utf8.extend_from_slice(&[1, 0xFF]);
        assert_eq!(check_table(&not_utf8), Err(TableError::Exceptions));
        let mut long = header(0, 24, 1);
        long.extend_from_slice(&[9, b'a']);
        assert_eq!(check_table(&long), Err(TableError::Exceptions));
        assert_eq!(check_table(&header(0, 24, 2)), Err(TableError::Exceptions));
    }

    #[test]
    fn counts_and_offsets_that_do_not_fit_are_refused() {
        // Where `usize` is 32 bits the block count overflows (Header); elsewhere the index
        // is simply too long for the file (Index). Either way it is refused.
        // is simply too long for the file (Header, too). Either way it is refused.
        assert_eq!(
            check_table(&header(u32::MAX, 24, 0)),
            Err(TableError::Header)
        );
        assert_eq!(check_table(&header(0, 25, 0)), Err(TableError::Header));
        // The index would run into the exceptions: the header does not fit together.
        assert_eq!(check_table(&header(2, 24, 0)), Err(TableError::Header));
        assert_eq!(
            check_table(&header(0, 24, u32::MAX)),
            Err(TableError::Exceptions)
        );
    }

    #[test]
    fn errors_have_texts() {
        use alloc::string::ToString;
        for e in [
            TableError::Magic,
            TableError::Header,
            TableError::Index,
            TableError::Exceptions,
            TableError::Trailing,
            TableError::TooLarge,
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
