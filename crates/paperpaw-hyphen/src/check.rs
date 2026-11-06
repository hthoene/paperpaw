//! The check of a pattern table that is read from untrusted storage: one
//! function, used by the kernel's reader and by `paperpaw-dev package`.

use core::fmt;

/// The header of a table, in bytes.
const HEADER: usize = 24;

/// Why a table is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TableError {
    /// Not magic `PPHP` version 1 (the older `QIHP` magic is not accepted here).
    Magic,
    /// The header is cut off, or a count or offset in it does not fit the file.
    Header,
    /// The index runs into the exceptions, is not ascending, or points past the patterns.
    Index,
    /// The exceptions are cut off, or one is not UTF-8.
    Exceptions,
    /// The exceptions do not end at the end of the file.
    Trailing,
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TableError::Magic => "not a PPHP version 1 table",
            TableError::Header => "the table's header does not fit the file",
            TableError::Index => "the table's index is not ascending or points past the patterns",
            TableError::Exceptions => "the table's exceptions are cut off or not UTF-8",
            TableError::Trailing => "the table's exceptions do not end at the end of the file",
        })
    }
}

fn u32_at(table: &[u8], at: usize) -> Option<usize> {
    let bytes: [u8; 4] = table.get(at..at.checked_add(4)?)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(bytes)).ok()
}

/// Checks that `table` is a PPHP version 1 table a reader can use, reading it
/// only through bounds-checked access and allocating nothing.
///
/// - The magic is `PPHP\x01`; `QIHP`, which [`Patterns::new`](crate::Patterns::new)
///   still reads, is refused.
/// - The header gives the block count, the exceptions offset and the exception
///   count; the index (4 bytes a block) must end at or before the exceptions
///   offset, which must lie in the file.
/// - Index offsets, read from the start of the entries, are strictly ascending
///   and each points before the exceptions.
/// - The exceptions are walked: `count` entries of a length byte and that many
///   bytes of UTF-8. The walk must end exactly at the end of the file, so a
///   truncated or padded table is refused.
///
/// The size limit of a plugin kind is not checked here; the caller does that.
///
/// # Errors
///
/// A [`TableError`] naming the first part of the table that is wrong.
pub fn check_table(table: &[u8]) -> Result<(), TableError> {
    if table.get(..5) != Some(b"PPHP\x01".as_slice()) {
        return Err(TableError::Magic);
    }
    let blocks = u32_at(table, 12).ok_or(TableError::Header)?;
    let exceptions = u32_at(table, 16).ok_or(TableError::Header)?;
    let count = u32_at(table, 20).ok_or(TableError::Header)?;
    let entries = blocks
        .checked_mul(4)
        .and_then(|index| index.checked_add(HEADER))
        .ok_or(TableError::Header)?;
    if exceptions > table.len() {
        return Err(TableError::Header);
    }
    if entries > exceptions {
        return Err(TableError::Index);
    }
    let mut last: Option<usize> = None;
    for block in 0..blocks {
        let offset = u32_at(table, HEADER + 4 * block).ok_or(TableError::Index)?;
        if last.is_some_and(|before| offset <= before)
            || entries
                .checked_add(offset)
                .is_none_or(|at| at >= exceptions)
        {
            return Err(TableError::Index);
        }
        last = Some(offset);
    }
    let mut at = exceptions;
    for _ in 0..count {
        let length = usize::from(*table.get(at).ok_or(TableError::Exceptions)?);
        let end = at.checked_add(1 + length).ok_or(TableError::Exceptions)?;
        let word = table.get(at + 1..end).ok_or(TableError::Exceptions)?;
        if core::str::from_utf8(word).is_err() {
            return Err(TableError::Exceptions);
        }
        at = end;
    }
    if at == table.len() {
        Ok(())
    } else {
        Err(TableError::Trailing)
    }
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
        assert_eq!(check_table(&compiled()), Ok(()));
        assert_eq!(check_table(&compile("", "", 2, 2)), Ok(()));
        assert_eq!(check_table(&compile("a1b", "", 1, 1)), Ok(()));
        // More patterns than one index block holds.
        let table = compile(&letters(200), "", 2, 2);
        assert!(u32_at(&table, 12).unwrap() >= 3);
        assert_eq!(check_table(&table), Ok(()));
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
        let at = u32::try_from(exceptions - entries).unwrap();
        table[24..28].copy_from_slice(&at.to_le_bytes());
        assert_eq!(check_table(&table), Err(TableError::Index));
    }

    #[test]
    fn the_exceptions_are_walked() {
        let mut one = header(0, 24, 1);
        one.extend_from_slice(&[3, b'a', b'-', b'b']);
        assert_eq!(check_table(&one), Ok(()));
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
        assert!(matches!(
            check_table(&header(u32::MAX, 24, 0)),
            Err(TableError::Header | TableError::Index)
        ));
        assert_eq!(check_table(&header(0, 25, 0)), Err(TableError::Header));
        assert_eq!(check_table(&header(2, 24, 0)), Err(TableError::Index));
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
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
