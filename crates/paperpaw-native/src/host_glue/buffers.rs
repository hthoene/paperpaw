//! Bounded text and buffer helpers for the host table's copy-out entries
//! (`settings.read`, `i18n.copy`, and the shape `blob.read` shares), ADR 0034.
//!
//! The raw `(pointer, length)` pairs of the table are turned into safe slices
//! in one place, `bytes_in`, `text_in` and `buffer_out`; a backend then works on
//! `&str` and `&[u8]`/`&mut [u8]` only. Nothing here allocates.
//!
//! Two result rules, kept apart:
//!
//! * a **query** (`length` results: repeatable, read-only) follows the
//!   snprintf rule: it writes at most `cap` bytes and returns the length the
//!   whole result needs, so the caller may repeat it with a larger buffer
//!   ([`copy_required`], [`query_text`]);
//! * a **blob read** (`count` results) returns how many bytes were actually
//!   copied ([`copy_count`]); it is never repeated to size a buffer.
//!
//! Checks come before any effect: a bad pointer, a length over the operation's
//! limit, or text that is not UTF-8 is refused before the backend is called.

use super::{ERR_INVALID, ERR_NO_SPACE, ERR_UTF8, PpBuffer, PpBytes};

/// The schema's byte limit for a text-copy operation (`settings.read`,
/// `i18n.copy`, `blob.read`: `max_bytes` 4096 in `abi/abi.def`).
pub const TEXT_MAX: usize = 4096;

/// Reads `pair` as a byte slice of at most `max` bytes.
///
/// Null with length zero is the empty slice (no Rust slice is built from
/// null). Null with a length, a length over `max` or over `isize::MAX` is
/// `ERR_INVALID`.
///
/// # Safety
///
/// A non-null `pair.ptr` is valid for reads of `pair.len` bytes for `'a`, and
/// the bytes are not written while the slice lives.
pub unsafe fn bytes_in<'a>(pair: PpBytes, max: usize) -> Result<&'a [u8], i32> {
    if pair.len == 0 {
        return Ok(&[]);
    }
    if pair.ptr.is_null() || pair.len > max || pair.len > isize::MAX as usize {
        return Err(ERR_INVALID);
    }
    // SAFETY: non-null, bounded, and valid for `len` reads per the contract.
    Ok(unsafe { core::slice::from_raw_parts(pair.ptr, pair.len) })
}

/// Reads `pair` as text of at most `max` bytes; text that is not UTF-8 is
/// `ERR_UTF8`.
///
/// # Safety
///
/// As [`bytes_in`].
pub unsafe fn text_in<'a>(pair: PpBytes, max: usize) -> Result<&'a str, i32> {
    // SAFETY: as documented.
    let bytes = unsafe { bytes_in(pair, max) }?;
    core::str::from_utf8(bytes).map_err(|_| ERR_UTF8)
}

/// Reads `pair` as a writable slice of at most `max` bytes.
///
/// Null with capacity zero is the empty slice. Null with a capacity, a
/// capacity over `max` or over `isize::MAX` is `ERR_INVALID`.
///
/// # Safety
///
/// A non-null `pair.ptr` is valid for writes of `pair.cap` bytes for `'a`, and
/// nothing else reads or writes them while the slice lives.
pub unsafe fn buffer_out<'a>(pair: PpBuffer, max: usize) -> Result<&'a mut [u8], i32> {
    if pair.cap == 0 {
        return Ok(&mut []);
    }
    if pair.ptr.is_null() || pair.cap > max || pair.cap > isize::MAX as usize {
        return Err(ERR_INVALID);
    }
    // SAFETY: non-null, bounded, and valid for `cap` writes per the contract.
    Ok(unsafe { core::slice::from_raw_parts_mut(pair.ptr, pair.cap) })
}

/// The snprintf rule: copies as much of `src` as fits in `out` and returns the
/// length the whole of `src` needs.
///
/// A `src` longer than `limit` cannot be delivered by this operation at all
/// and is `ERR_NO_SPACE` (nothing is written). The returned length is at most
/// `limit`, which is at most `i32::MAX`, so it is a valid nonnegative result.
pub fn copy_required(src: &[u8], out: &mut [u8], limit: usize) -> i32 {
    let limit = limit.min(i32::MAX as usize);
    if src.len() > limit {
        return ERR_NO_SPACE;
    }
    let n = src.len().min(out.len());
    out[..n].copy_from_slice(&src[..n]);
    src.len() as i32
}

/// A blob read: copies as much of `src` as fits in `out` and returns the
/// number of bytes copied (zero for an empty `out` or an empty `src`).
pub fn copy_count(src: &[u8], out: &mut [u8]) -> i32 {
    let n = src.len().min(out.len()).min(i32::MAX as usize);
    out[..n].copy_from_slice(&src[..n]);
    n as i32
}

/// A text query entry end to end: the key is validated as UTF-8 of at most
/// `key_max` bytes, the output buffer as at most [`TEXT_MAX`] bytes, then
/// `backend` is asked for the value of the key and the value is copied by
/// [`copy_required`].
///
/// The key is checked first, then the buffer, and the backend runs only when
/// both are valid, so a refused call has no effect. The backend returns the
/// value, or a negative status (for example a missing key); it is called
/// once, never repeated.
///
/// # Safety
///
/// `key` and `out` meet the contracts of [`text_in`] and [`buffer_out`], and
/// the two do not overlap.
pub unsafe fn query_text<R: AsRef<str>>(
    key: PpBytes,
    out: PpBuffer,
    key_max: usize,
    backend: impl FnOnce(&str) -> Result<R, i32>,
) -> i32 {
    // SAFETY: as documented.
    let key = match unsafe { text_in(key, key_max) } {
        Ok(key) => key,
        Err(status) => return status,
    };
    // SAFETY: as documented.
    let out = match unsafe { buffer_out(out, TEXT_MAX) } {
        Ok(out) => out,
        Err(status) => return status,
    };
    match backend(key) {
        Ok(value) => copy_required(value.as_ref().as_bytes(), out, TEXT_MAX),
        Err(status) if status < 0 => status,
        // A backend that answers a nonnegative error is a bug in the backend.
        Err(_) => ERR_INVALID,
    }
}
