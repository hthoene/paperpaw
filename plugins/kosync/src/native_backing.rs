//! What the sync asks of the book and the library, which the SDK has no facade
//! for yet (`PORTING-7.md`): the book's fingerprint (`core.library` op 25
//! `book_key`), a place's XPointer and the place of an XPointer (`core.document`
//! ops `xpointer` and `locate`). Until those facades exist each answers
//! "unavailable" (`Ok(None)`): the sync then makes no fetch and sends nothing,
//! as interface 6 did for a book that could not be read, and the signed-in
//! account, its screen and its key work. Each function is where the facade is
//! put in.
use alloc::string::String;

use paperpaw_sdk::native::{Error, Host};

/// Whether a position can be sent at all: the book's key and the place's
/// XPointer are available. Nothing is declared for a push while they are not
/// (it would wake the radio for nothing).
#[must_use]
pub const fn can_send() -> bool {
    false
}

/// The KOReader fingerprint of the book at `path`: the 32 hex digits of its
/// partial MD5 (`library.md` section 6).
///
/// # Errors
/// What the facade says.
pub fn digest(_host: &Host<'_>, _path: &str) -> Result<Option<String>, Error> {
    Ok(None)
}

/// The XPointer of the place `at` (section, offset) of the book at `path`, and
/// the progress (0..1) there as the document counts it.
///
/// # Errors
/// What the facade says.
pub fn xpointer(
    _host: &Host<'_>,
    _path: &str,
    _at: (u32, u32),
) -> Result<Option<(String, f64)>, Error> {
    Ok(None)
}

/// The place (section, offset) of the book at `path` that `xpointer` names, or
/// the one at `progress` (0..1) where the XPointer is empty or not found.
///
/// # Errors
/// What the facade says.
pub fn locate(
    _host: &Host<'_>,
    _path: &str,
    _xpointer: &str,
    _progress: f64,
) -> Result<Option<(u32, u32)>, Error> {
    Ok(None)
}
