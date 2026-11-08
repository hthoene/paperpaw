//! Small fallible text helpers shared by the parts of the model.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::StorageError;
use paperpaw_sdk::native::Error;

/// A storage's refusal as the host's error.
pub(crate) fn error_of(error: StorageError) -> Error {
    match error {
        StorageError::NotFound => Error::NotFound,
        StorageError::InvalidPath | StorageError::InvalidData => Error::Invalid,
        StorageError::Unsupported => Error::Unsupported,
        StorageError::Full => Error::Full,
        StorageError::TooLarge => Error::NoSpace,
        _ => Error::Io,
    }
}

/// The bytes `bytes` take as text, each run of invalid bytes as one
/// U+FFFD (what `String::from_utf8_lossy` makes).
pub(crate) fn lossy_len(bytes: &[u8]) -> usize {
    bytes
        .utf8_chunks()
        .map(|chunk| chunk.valid().len() + if chunk.invalid().is_empty() { 0 } else { 3 })
        .sum()
}

/// Appends `bytes` as text; the room must be there (`lossy_len`).
pub(crate) fn lossy_into(out: &mut String, bytes: &[u8]) {
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            out.push('\u{FFFD}');
        }
    }
}

/// `bytes` as text, as `String::from_utf8_lossy` makes it.
pub(crate) fn lossy(bytes: &[u8]) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(lossy_len(bytes))
        .map_err(|_| Error::NoMemory)?;
    lossy_into(&mut out, bytes);
    Ok(out)
}

/// Appends `text` to `out`.
pub(crate) fn push(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

/// Appends `c` to `out`.
pub(crate) fn push_char(out: &mut String, c: char) -> Result<(), Error> {
    out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
    out.push(c);
    Ok(())
}

/// Appends the decimal digits of `value`.
pub(crate) fn push_u64(out: &mut String, value: u64) -> Result<(), Error> {
    let mut digits = [0u8; 20];
    push(out, paperpaw_ui_kit::text::decimal_u64(value, &mut digits))
}

/// Appends the decimal digits of `value`, `-` first where it is negative.
pub(crate) fn push_i64(out: &mut String, value: i64) -> Result<(), Error> {
    if value < 0 {
        push_char(out, '-')?;
    }
    push_u64(out, value.unsigned_abs())
}

/// `dir` and `name` as a path (just `name` in the vault's top).
pub(crate) fn join(dir: &str, name: &str) -> Result<String, Error> {
    let mut out = String::new();
    join_into(&mut out, dir, name)?;
    Ok(out)
}

/// [`join`] into `out`, which is emptied first (one buffer for many names).
pub(crate) fn join_into(out: &mut String, dir: &str, name: &str) -> Result<(), Error> {
    out.clear();
    let extra = usize::from(!dir.is_empty());
    let len = dir
        .len()
        .checked_add(name.len())
        .and_then(|n| n.checked_add(extra))
        .ok_or(Error::NoMemory)?;
    out.try_reserve(len).map_err(|_| Error::NoMemory)?;
    out.push_str(dir);
    if !dir.is_empty() {
        out.push('/');
    }
    out.push_str(name);
    Ok(())
}

/// A copy of `text`.
pub(crate) fn copy(text: &str) -> Result<String, Error> {
    paperpaw_ui_kit::text::copy(text)
}

/// A heap buffer of `len` zeroed bytes.
pub(crate) fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    paperpaw_ui_kit::mem::buffer(len)
}

/// A character as it is searched: lower case, the Greek final sigma `ς` as
/// the plain `σ` (so that `ΟΔΟΣ` and `οδος` and `οδοσ` are one word).
fn folded(c: char) -> core::iter::Map<core::char::ToLowercase, fn(char) -> char> {
    fn plain(c: char) -> char {
        if c == 'ς' { 'σ' } else { c }
    }
    c.to_lowercase().map(plain as fn(char) -> char)
}

/// Whether `haystack` has `needle` in it, without regard to case (each
/// character folded on its own, see `folded`).
pub(crate) fn contains_lowercase(haystack: &str, needle: &str) -> bool {
    let mut needle_chars = needle.chars().flat_map(folded);
    let Some(first) = needle_chars.next() else {
        return true;
    };
    haystack.char_indices().any(|(at, _)| {
        let Some(rest) = haystack.get(at..) else {
            return false;
        };
        let mut hay = rest.chars().flat_map(folded);
        hay.next() == Some(first) && needle_chars.clone().all(|n| hay.next() == Some(n))
    })
}
