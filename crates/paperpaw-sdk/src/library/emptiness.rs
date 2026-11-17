//! Whether the catalog lists nothing, told from its first bytes: for the
//! kernel, which lets a walk run beside a library that holds no index and
//! need not make the catalog again to know it is empty.

use super::catalog::{HEADER, catalog_path};
use crate::Storage;

/// Whether the catalog certainly lists no book: its header line ends and
/// nothing follows (a walk of a library with nothing in it). A view of
/// such a catalog holds no index, so a walk may run beside it where it
/// must wait for a full one; and a walk made once already need not be
/// made again to know. Where it cannot be told (no catalog, a read that
/// fails, a header that does not end in what is read, another version,
/// anything after it) it is `false`: as if there were books. Read from
/// the catalog's first bytes, never all of it.
pub fn is_empty(storage: &dyn Storage) -> bool {
    let mut buf = [0u8; 96];
    let Ok(n) = storage.read_at(&catalog_path(), 0, &mut buf) else {
        return false;
    };
    let text = &buf[..n];
    let Some(after) = text.strip_prefix(HEADER.as_bytes()) else {
        return false;
    };
    // The header ends at a newline, or where the file does; a full read
    // that shows no end says nothing.
    let rest = match after.iter().position(|&b| b == b'\n') {
        Some(end) => &after[end..],
        None if n < buf.len() => &[][..],
        None => return false,
    };
    // Blank lines only, and the read reached the end of the file.
    n < buf.len() && rest.iter().all(u8::is_ascii_whitespace)
}
