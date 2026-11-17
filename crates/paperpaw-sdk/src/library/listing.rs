//! Sorted, paged listings of the books and folders of a folder, that
//! never hold more than a page.
//!
//! Folders with thousands of books must work on a device with a few
//! hundred kilobytes of RAM, so a listing is never loaded completely.
//! Pages are found by *keyset pagination*: one pass over the directory
//! keeps only the `n` entries that follow (or precede) a known entry in
//! sort order. Each step costs a directory scan but only `n` entries of
//! memory.

use crate::Formats;

/// Folders other readers keep next to a book, e.g. KOReader's
/// `Book.sdr` with its reading position: not part of the library.
/// Compares bytes, as the last four bytes of a name may lie inside a
/// character (e.g. a folder named in Chinese).
/// Whether a listing shows the entry `name`: folders but sidecars, files
/// a format plugin opens, nothing hidden.
pub(crate) fn shown(formats: &Formats, name: &str, is_dir: bool) -> bool {
    let shown = if is_dir {
        !is_sidecar(name)
    } else {
        formats.supports(name)
    };
    shown && !name.starts_with('.')
}

fn is_sidecar(name: &str) -> bool {
    let name = name.as_bytes();
    name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(b".sdr")
}
