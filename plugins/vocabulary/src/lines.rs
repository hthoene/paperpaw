//! Reading a text file a line at a time, with fallible allocation.
//!
//! The vocabulary's files (words, decks, cards, answers) are all lines of
//! text. They are streamed: a line at a time is in memory, never a file.
//! Where the heap refuses the line's buffer, the read ends with
//! [`StorageError::TooLarge`] instead of aborting the plugin.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{Storage, StorageError};

/// The longest line read: a longer one is cut here.
pub(crate) const MAX_LINE: usize = 4096;

/// `bytes` as text, invalid UTF-8 replaced by U+FFFD; borrowed where it is
/// valid, else one fallible reservation.
pub(crate) fn lossy(bytes: &[u8]) -> Result<Cow<'_, str>, StorageError> {
    if let Ok(text) = core::str::from_utf8(bytes) {
        return Ok(Cow::Borrowed(text));
    }
    let mut out = String::new();
    // A replacement character is 3 bytes for at least 1 invalid byte.
    out.try_reserve(bytes.len().saturating_mul(3))
        .map_err(|_| StorageError::TooLarge)?;
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            out.push('\u{FFFD}');
        }
    }
    Ok(Cow::Owned(out))
}

/// An owned copy of `text`, made with fallible allocation.
pub(crate) fn copy(text: &str) -> Result<String, StorageError> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| StorageError::TooLarge)?;
    out.push_str(text);
    Ok(out)
}

/// Appends `text` to `out` with fallible allocation.
pub(crate) fn push(out: &mut String, text: &str) -> Result<(), StorageError> {
    out.try_reserve(text.len())
        .map_err(|_| StorageError::TooLarge)?;
    out.push_str(text);
    Ok(())
}

/// Appends `number` in decimal to `out` with fallible allocation.
pub(crate) fn push_number(out: &mut String, number: u32) -> Result<(), StorageError> {
    let mut digits = [0u8; 10];
    let mut at = digits.len();
    let mut rest = number;
    loop {
        at -= 1;
        if let Some(slot) = digits.get_mut(at) {
            *slot = b'0' + (rest % 10) as u8;
        }
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    let text = digits
        .get(at..)
        .and_then(|d| core::str::from_utf8(d).ok())
        .unwrap_or("0");
    push(out, text)
}

/// Calls `visit` with each line of `path` and where it begins, in order,
/// without its line break (`\n` or `\r\n`) and a byte order mark at the
/// start of the file; a missing file has none. A line
/// longer than [`MAX_LINE`] is cut; invalid UTF-8 is replaced. `visit` says
/// whether to go on.
pub(crate) fn each_line(
    storage: &dyn Storage,
    path: &str,
    visit: impl FnMut(u64, &str) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    each_line_from(storage, path, 0, visit)
}

/// [`each_line`] from the line that begins at `from`.
pub(crate) fn each_line_from(
    storage: &dyn Storage,
    path: &str,
    from: u64,
    mut visit: impl FnMut(u64, &str) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    let mut chunk = [0u8; 512];
    let mut line: Vec<u8> = Vec::new();
    let (mut at, mut start) = (from, from);
    let mut emit = |start: u64, line: &mut Vec<u8>| -> Result<ControlFlow<()>, StorageError> {
        // A byte order mark begins the file, not a line.
        if start == 0 && line.starts_with(&[0xEF, 0xBB, 0xBF]) {
            line.drain(..3);
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let flow = visit(start, &lossy(line)?);
        line.clear();
        Ok(flow)
    };
    loop {
        let n = match storage.read_at(path, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) if at == from => return Ok(()),
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        let Some(read) = chunk.get(..n) else {
            return Err(StorageError::Io);
        };
        let mut offset = at;
        for piece in read.split_inclusive(|b| *b == b'\n') {
            let (text, ends) = match piece.split_last() {
                Some((b'\n', text)) => (text, true),
                _ => (piece, false),
            };
            let room = MAX_LINE.saturating_sub(line.len()).min(text.len());
            line.try_reserve(room).map_err(|_| StorageError::TooLarge)?;
            line.extend_from_slice(text.get(..room).unwrap_or_default());
            offset += piece.len() as u64;
            if ends {
                if emit(start, &mut line)?.is_break() {
                    return Ok(());
                }
                start = offset;
            }
        }
        at += n as u64;
    }
    // A last line without a line break is a line too.
    if !line.is_empty() {
        let _ = emit(start, &mut line)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;
    use alloc::vec;
    use paperpaw_sdk::MemoryStorage;

    fn lines(storage: &dyn Storage) -> Vec<(u64, String)> {
        let mut out = Vec::new();
        each_line(storage, "f", |at, line| {
            out.push((at, line.to_string()));
            ControlFlow::Continue(())
        })
        .unwrap();
        out
    }

    #[test]
    fn lines_come_with_where_they_begin() {
        let storage = MemoryStorage::new().with("f", "ab\r\n\ncde");
        assert_eq!(
            lines(&storage),
            vec![
                (0, "ab".to_string()),
                (4, String::new()),
                (5, "cde".to_string())
            ]
        );
    }

    #[test]
    fn a_missing_file_has_no_lines_and_a_long_line_is_cut() {
        let storage = MemoryStorage::new();
        assert!(lines(&storage).is_empty());
        let long = "x".repeat(MAX_LINE * 2 + 7);
        let storage = MemoryStorage::new().with("f", format!("{long}\nnext\n"));
        let got = lines(&storage);
        assert_eq!(got[0].1.len(), MAX_LINE);
        assert_eq!(got[1], (long.len() as u64 + 1, "next".to_string()));
    }

    #[test]
    fn a_byte_order_mark_begins_the_file_not_the_first_line() {
        let storage = MemoryStorage::new().with("f", "\u{feff}a\nb\n");
        // The marks's 3 bytes are in the offsets: `b` begins at 5.
        assert_eq!(
            lines(&storage),
            vec![(0, "a".to_string()), (5, "b".to_string())]
        );
        // A mark in the middle is text.
        let storage = MemoryStorage::new().with("f", "a\n\u{feff}b\n");
        assert_eq!(lines(&storage)[1].1, "\u{feff}b");
    }

    #[test]
    fn invalid_utf8_is_replaced_not_dropped() {
        let storage = MemoryStorage::new().with("f", b"a\xFFb\nok\n".to_vec());
        let got = lines(&storage);
        assert_eq!(got[0].1, "a\u{FFFD}b");
        assert_eq!(got[1].1, "ok");
    }

    #[test]
    fn reading_stops_where_visit_says_so_and_can_start_late() {
        let storage = MemoryStorage::new().with("f", "a\nb\nc\n");
        let mut seen = Vec::new();
        each_line_from(&storage, "f", 2, |at, line| {
            seen.push((at, line.to_string()));
            ControlFlow::Break(())
        })
        .unwrap();
        assert_eq!(seen, vec![(2, "b".to_string())]);
    }
}
