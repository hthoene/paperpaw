//! What the library knows of one book, read without an infallible
//! allocation: every `String` and `Vec` here is reserved with
//! `try_reserve`, so a device short of memory gets
//! [`Error::OutOfMemory`](crate::data::Error::OutOfMemory) instead of an
//! abort, and nothing is returned half read. The plain forms
//! ([`book`](super::book), [`key`](super::key)) are these forms with the
//! error left out (or, for the key, an abort as before).


use super::fnv1a64;

/// The digits of a [`key`](super::key).
pub const KEY_LEN: usize = 16;

/// The book key of `path` (16 hexadecimal digits, [`key`](super::key))
/// written into `out`, without allocating.
pub fn key_into<'o>(path: &str, out: &'o mut [u8; KEY_LEN]) -> &'o str {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let hash = fnv1a64(path);
    for (i, digit) in out.iter_mut().enumerate() {
        *digit = HEX[((hash >> (60 - 4 * i)) & 0xf) as usize];
    }
    // The digits are ASCII.
    core::str::from_utf8(out).unwrap_or_default()
}























