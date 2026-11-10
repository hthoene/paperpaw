//! Small stack strings for the rows: ids joined from parts, a version
//! written out, a message key built from an id. Nothing here allocates.
use core::fmt::{self, Write};

/// Field caps of the `service.reg_*` rows (`registry.md` 3), in bytes.
pub(crate) mod caps {
    /// Ids (`reg_item.id`, `reg_slot.id`, ...): never cut.
    pub(crate) const ID: usize = 128;
    /// Translated titles, cut at a character.
    pub(crate) const TITLE: usize = 128;
    /// A plugin's translated name.
    pub(crate) const PLUGIN_NAME: usize = 128;
    /// A description or help text.
    pub(crate) const TEXT: usize = 1024;
}

/// Text of at most `N` bytes on the stack; writing past it fails (and keeps
/// what fitted), so a caller that must not cut refuses instead.
#[derive(Clone, Copy)]
pub(crate) struct Fixed<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Fixed<N> {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    /// The parts written one after the other, or `None` where they do not fit.
    pub(crate) fn of(parts: &[&str]) -> Option<Self> {
        let mut text = Self::new();
        for part in parts {
            text.write_str(part).ok()?;
        }
        Some(text)
    }

    pub(crate) fn as_str(&self) -> &str {
        // The bytes were written from `str`s whole, so this is valid UTF-8; an
        // impossible failure reads as empty rather than panicking.
        self.bytes
            .get(..self.len)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }
}

impl<const N: usize> Write for Fixed<N> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let target = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        target.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// `text` cut at a UTF-8 boundary to at most `limit` bytes.
pub(crate) fn clip(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// The text of message key `key` in the user's language, else the key itself
/// (the host shows the key where a catalog lacks it), cut to `limit` bytes.
pub(crate) fn translated<'a>(
    translator: &'a paperpaw_i18n::Translator,
    key: &'a str,
    limit: usize,
) -> &'a str {
    clip(translator.get(key).unwrap_or(key), limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_join_and_refuse_to_be_cut() {
        let text = Fixed::<8>::of(&["ab", ".", "cde"]).unwrap();
        assert_eq!(text.as_str(), "ab.cde");
        assert!(Fixed::<4>::of(&["abc", "de"]).is_none());
    }

    #[test]
    fn clip_keeps_whole_characters() {
        assert_eq!(clip("aé", 2), "a");
        assert_eq!(clip("aé", 3), "aé");
        assert_eq!(clip("abc", 0), "");
    }
}
