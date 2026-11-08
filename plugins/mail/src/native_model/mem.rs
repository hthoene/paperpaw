//! Allocation that can be refused. Every growth reserves first, so a refused
//! allocation is [`Error::NoMemory`] and never an abort, and what is read from
//! a server or a mail is never turned into a panic.
use alloc::string::String;
use alloc::vec::Vec;

pub use paperpaw_sdk::native::Error;
pub use paperpaw_ui_kit::mem::{push, wipe, wipe_text};
pub use paperpaw_ui_kit::text::copy;

/// An empty string with room for `bytes`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn string_with(bytes: usize) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(bytes).map_err(|_| Error::NoMemory)?;
    Ok(out)
}

/// An empty vector with room for `items`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn vec_with<T>(items: usize) -> Result<Vec<T>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(items).map_err(|_| Error::NoMemory)?;
    Ok(out)
}

/// Appends `text`.
///
/// # Errors
/// [`Error::NoMemory`]; `out` is unchanged.
pub fn push_str(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

/// Appends `c`.
///
/// # Errors
/// [`Error::NoMemory`]; `out` is unchanged.
pub fn push_char(out: &mut String, c: char) -> Result<(), Error> {
    out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
    out.push(c);
    Ok(())
}

/// Appends `byte`.
///
/// # Errors
/// [`Error::NoMemory`]; `out` is unchanged.
pub fn push_byte(out: &mut Vec<u8>, byte: u8) -> Result<(), Error> {
    out.try_reserve(1).map_err(|_| Error::NoMemory)?;
    out.push(byte);
    Ok(())
}

/// Appends `bytes`.
///
/// # Errors
/// [`Error::NoMemory`]; `out` is unchanged.
pub fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Error> {
    out.try_reserve(bytes.len()).map_err(|_| Error::NoMemory)?;
    out.extend_from_slice(bytes);
    Ok(())
}

/// `bytes` as text, a byte that is no UTF-8 as U+FFFD (what
/// `String::from_utf8_lossy` makes, without its infallible allocation).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn lossy(bytes: &[u8]) -> Result<String, Error> {
    let mut out = string_with(bytes.len())?;
    for chunk in bytes.utf8_chunks() {
        push_str(&mut out, chunk.valid())?;
        if !chunk.invalid().is_empty() {
            push_char(&mut out, '\u{fffd}')?;
        }
    }
    Ok(out)
}

/// `text` with ASCII letters in lower case.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn lower_ascii(text: &str) -> Result<String, Error> {
    let mut out = copy(text)?;
    out.make_ascii_lowercase();
    Ok(out)
}

/// Whether `text` starts with `prefix`, letters of either case.
pub fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// Whether `text` ends with `suffix`, letters of either case.
pub fn ends_with_ignore_case(text: &str, suffix: &str) -> bool {
    text.len()
        .checked_sub(suffix.len())
        .and_then(|from| text.get(from..))
        .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
}

/// A text that wipes itself when it is dropped: a password, or a line that has
/// one in it. Its `Debug` shows nothing of it.
#[derive(Default)]
pub struct Secret(String);

impl Secret {
    /// An empty secret with room for `bytes`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn with_capacity(bytes: usize) -> Result<Self, Error> {
        string_with(bytes).map(Self)
    }

    /// The secret as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Appends `text`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn push_str(&mut self, text: &str) -> Result<(), Error> {
        push_str(&mut self.0, text)
    }

    /// The string itself, for whoever must write it out and then wipe it
    /// with [`wipe_text`].
    #[must_use]
    pub fn into_string(mut self) -> String {
        core::mem::take(&mut self.0)
    }
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe_text(core::mem::take(&mut self.0));
    }
}

/// A byte buffer that wipes itself when it is dropped.
#[derive(Default)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// An empty buffer with room for `bytes`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn with_capacity(bytes: usize) -> Result<Self, Error> {
        vec_with(bytes).map(Self)
    }

    /// The bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Appends `bytes`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        push_bytes(&mut self.0, bytes)
    }

    /// Appends `byte`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn push_byte(&mut self, byte: u8) -> Result<(), Error> {
        push_byte(&mut self.0, byte)
    }
}

impl core::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SecretBytes(..)")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}
