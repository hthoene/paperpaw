//! Allocation that can be refused: the native provider answers `NoMemory`
//! where memory is short, never aborts. A refusal is [`Refused`].

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_sdk::document::DocumentError;

/// A refused allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refused;

impl From<Refused> for DocumentError {
    fn from(_: Refused) -> Self {
        DocumentError::OutOfMemory
    }
}

/// `value` added to `list`.
pub(crate) fn push<T>(list: &mut Vec<T>, value: T) -> Result<(), Refused> {
    list.try_reserve(1).map_err(|_| Refused)?;
    list.push(value);
    Ok(())
}

/// A copy of `text`.
pub(crate) fn string(text: &str) -> Result<String, Refused> {
    let mut out = String::new();
    out.try_reserve_exact(text.len()).map_err(|_| Refused)?;
    out.push_str(text);
    Ok(out)
}

/// `c` added to `text`.
pub(crate) fn push_char(text: &mut String, c: char) -> Result<(), Refused> {
    text.try_reserve(c.len_utf8()).map_err(|_| Refused)?;
    text.push(c);
    Ok(())
}

/// `more` added to `text`.
pub(crate) fn push_str(text: &mut String, more: &str) -> Result<(), Refused> {
    text.try_reserve(more.len()).map_err(|_| Refused)?;
    text.push_str(more);
    Ok(())
}

/// `bytes` as text, a replacement character for what is not UTF-8.
pub(crate) fn lossy(bytes: &[u8]) -> Result<String, Refused> {
    let mut out = String::new();
    out.try_reserve_exact(bytes.len()).map_err(|_| Refused)?;
    for chunk in bytes.utf8_chunks() {
        push_str(&mut out, chunk.valid())?;
        if !chunk.invalid().is_empty() {
            push_char(&mut out, char::REPLACEMENT_CHARACTER)?;
        }
    }
    Ok(out)
}

struct Writer(String, bool);

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.0.try_reserve(s.len()).is_err() {
            self.1 = true;
            return Err(fmt::Error);
        }
        self.0.push_str(s);
        Ok(())
    }
}

/// `format!`, where a refusal is [`Refused`].
pub(crate) fn formatted(args: fmt::Arguments<'_>) -> Result<String, Refused> {
    let mut out = Writer(String::new(), false);
    match fmt::write(&mut out, args) {
        Ok(()) => Ok(out.0),
        Err(_) => Err(Refused),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossy_replaces_what_is_not_text() {
        assert_eq!(lossy(b"a\xffb").unwrap(), "a\u{fffd}b");
        assert_eq!(lossy("é".as_bytes()).unwrap(), "é");
        assert_eq!(formatted(format_args!("{}-{}", 1, "x")).unwrap(), "1-x");
    }
}
