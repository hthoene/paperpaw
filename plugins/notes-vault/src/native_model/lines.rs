//! A text file read a line at a time, so that a file of any size costs a
//! small buffer, not its size.
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;

use super::text::{buffer, error_of, lossy_into};

/// The longest a line is kept, in bytes; the rest of it is dropped.
pub const MAX_LINE: usize = 1024;
/// Bytes read from the storage in one call.
const CHUNK: usize = 1024;

/// Calls `each` with the lines of the file at `path` (without their line
/// breaks), until it answers `false`; the lines are cut at [`MAX_LINE`].
///
/// # Errors
/// The storage's refusal as the host's error, [`Error::NoMemory`].
pub fn each(
    storage: &dyn Storage,
    path: &str,
    each: &mut dyn FnMut(&str) -> bool,
) -> Result<(), Error> {
    // One reservation for the chunk and the line, one for the line as text
    // (invalid bytes grow to three).
    let mut bytes = buffer(CHUNK + MAX_LINE)?;
    let mut text = alloc::string::String::new();
    text.try_reserve_exact(MAX_LINE * 3)
        .map_err(|_| Error::NoMemory)?;
    let (chunk, line) = bytes.split_at_mut(CHUNK);
    let mut kept = 0usize;
    let mut at = 0u64;
    loop {
        let n = storage.read_at(path, at, chunk).map_err(error_of)?;
        if n == 0 {
            break;
        }
        at = at.saturating_add(n as u64);
        for &byte in chunk.get(..n).unwrap_or(&[]) {
            if byte == b'\n' {
                text.clear();
                lossy_into(&mut text, line.get(..kept).unwrap_or(&[]));
                if !each(text.trim_end_matches('\r')) {
                    return Ok(());
                }
                kept = 0;
            } else if let Some(slot) = line.get_mut(kept) {
                *slot = byte;
                kept += 1;
            }
        }
    }
    if kept > 0 {
        text.clear();
        lossy_into(&mut text, line.get(..kept).unwrap_or(&[]));
        each(&text);
    }
    Ok(())
}
