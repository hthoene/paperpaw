//! Stream the old log and then the current log through the existing parser.

use crate::log::{self, Sessions};
use paperpaw_sdk::native::{
    Error, Host,
    storage::{self, Root},
};

/// A valid CSV record is less than 512 bytes even with maximum-width numbers.
/// Discard oversized damaged records without retaining an unbounded tail.
const LINE_BYTES: usize = 512;

pub(super) fn read(host: &Host<'_>) -> Result<Sessions, Error> {
    let mut sessions = Sessions::default();
    for name in [log::OLD, log::FILE] {
        read_file(host, name, &mut sessions)?;
    }
    Ok(sessions)
}

fn read_file(host: &Host<'_>, name: &str, sessions: &mut Sessions) -> Result<(), Error> {
    let mut offset = 0u64;
    let mut bytes = [0; 512];
    let mut line = [0; LINE_BYTES];
    let mut length = 0;
    let mut oversized = false;
    loop {
        let count = match storage::read_at(host, Root::Data, name, offset, &mut bytes) {
            Ok(count) => count,
            Err(Error::NotFound) if offset == 0 => break,
            Err(error) => return Err(error),
        };
        if count == 0 {
            break;
        }
        offset = offset.checked_add(count as u64).ok_or(Error::Invalid)?;
        for &byte in &bytes[..count] {
            if byte == b'\n' {
                if !oversized {
                    feed(sessions, &line[..length]);
                }
                length = 0;
                oversized = false;
            } else {
                if let Some(out) = line.get_mut(length) {
                    *out = byte;
                    length += 1;
                } else {
                    oversized = true;
                }
            }
        }
    }
    // As before, a final unterminated line is parsed if its fields suffice.
    if !oversized {
        feed(sessions, &line[..length]);
    }
    Ok(())
}

fn feed(sessions: &mut Sessions, bytes: &[u8]) {
    // A damaged line loses only itself, matching Sessions::read.
    if let Ok(text) = core::str::from_utf8(bytes) {
        sessions.read_text(text);
    }
}
