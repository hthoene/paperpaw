//! The check of a pattern table that is read from untrusted storage: one
//! function, used by the kernel's reader and by `paperpaw-dev package`.

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

/// What [`check_table`] found in a table it accepts, so that a reader or a tool
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

fn u32_at(table: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = table.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Checks that `table` is a PPHP version 1 table a reader can use, reading it
/// only through bounds-checked access and allocating nothing.
///
/// - The magic is `PPHP\x01`; `QIHP`, which [`Patterns::new`](crate::Patterns::new)
///   still reads, is refused.
/// - The header gives the block count, the exceptions offset and the exception
///   count. The index (4 bytes a block, after the 24-byte header) must end at or
///   before the exceptions offset, which must lie in the file; if the header's
///   numbers do not fit together or the file, the error is [`TableError::Header`].
/// - The limits [`MAX_BLOCKS`] and [`MAX_EXCEPTION_BYTES`] are checked before the
///   index is walked, and [`MAX_BLOCK_BYTES`] as it is walked: [`TableError::TooLarge`].
///   A reader that enforces the same constants never meets a table this refuses
///   for size.
/// - Index offsets, read from the start of the entries, are strictly ascending
///   and each points before the exceptions; otherwise [`TableError::Index`],
///   which is only about the index's contents.
/// - The exceptions are walked: `count` entries of a length byte and that many
///   bytes of UTF-8. The walk must end exactly at the end of the file, so a
///   truncated or padded table is refused. An entry of length 0 is accepted
///   (an empty word hyphenates nothing); `compile` never writes one.
///
/// The size limit of a plugin kind (2 MiB a table) is not checked here; the
/// caller does that.
///
/// # Errors
///
/// A [`TableError`] naming the first part of the table that is wrong.
pub fn check_table(table: &[u8]) -> Result<Layout, TableError> {
    if table.get(..5) != Some(b"PPHP\x01".as_slice()) {
        return Err(TableError::Magic);
    }
    let blocks = u32_at(table, 12).ok_or(TableError::Header)?;
    let exceptions_at = u32_at(table, 16).ok_or(TableError::Header)?;
    let exceptions = u32_at(table, 20).ok_or(TableError::Header)?;
    let (at, count) = (
        usize::try_from(exceptions_at).map_err(|_| TableError::Header)?,
        usize::try_from(blocks).map_err(|_| TableError::Header)?,
    );
    let entries = count
        .checked_mul(4)
        .and_then(|index| index.checked_add(HEADER))
        .ok_or(TableError::Header)?;
    if at > table.len() || entries > at {
        return Err(TableError::Header);
    }
    if blocks > MAX_BLOCKS {
        return Err(TableError::TooLarge);
    }
    let exceptions_len = table.len() - at;
    if exceptions_len > MAX_EXCEPTION_BYTES as usize {
        return Err(TableError::TooLarge);
    }
    // The patterns run from the end of the index to the exceptions offset. A
    // block is the bytes from one index offset to the next, the last to that end.
    let patterns = at - entries;
    let mut largest_block = 0usize;
    let mut last: Option<usize> = None;
    for block in 0..count {
        let offset = u32_at(table, HEADER + 4 * block).ok_or(TableError::Index)?;
        let offset = usize::try_from(offset).map_err(|_| TableError::Index)?;
        if last.is_some_and(|before| offset <= before) || offset >= patterns {
            return Err(TableError::Index);
        }
        if let Some(before) = last {
            largest_block = largest_block.max(offset - before);
        }
        last = Some(offset);
    }
    if let Some(offset) = last {
        largest_block = largest_block.max(patterns - offset);
    }
    if largest_block > MAX_BLOCK_BYTES as usize {
        return Err(TableError::TooLarge);
    }
    let mut cursor = at;
    for _ in 0..exceptions {
        let length = usize::from(*table.get(cursor).ok_or(TableError::Exceptions)?);
        let end = cursor
            .checked_add(1 + length)
            .ok_or(TableError::Exceptions)?;
        let word = table.get(cursor + 1..end).ok_or(TableError::Exceptions)?;
        if core::str::from_utf8(word).is_err() {
            return Err(TableError::Exceptions);
        }
        cursor = end;
    }
    if cursor != table.len() {
        return Err(TableError::Trailing);
    }
    // Every value is bounded by a limit above, so these cannot truncate.
    Ok(Layout {
        blocks,
        largest_block: largest_block as u32,
        exceptions_at,
        exceptions,
        exceptions_len: exceptions_len as u32,
    })
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;
    use crate::compile;

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
