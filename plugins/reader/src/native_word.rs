//! A word of the page on show with its box and its place in the book: what
//! `paperpaw_sdk::reading::WordBox` holds, with its text kept in the word
//! itself where it is short (#405). A page has about ninety words, each of
//! `WordBox` had a `String` of its own: ninety small heap blocks of a few
//! bytes and the allocator's word each, all the time the page is on show. A
//! word of up to [`INLINE`] bytes (nine of ten in English) takes no block;
//! a longer one has its own, as before. The fields are the same, so what
//! reads a word reads it as it read a `WordBox`.
use alloc::{boxed::Box, string::String};
use core::fmt;
use core::ops::Deref;
use paperpaw_sdk::{Rect, document::Locator, layout::LineBidi, native::Error};

/// Bytes of text kept in a word without a heap block of its own.
pub const INLINE: usize = 10;

/// The text of a word: in the word if it has up to [`INLINE`] bytes, else in
/// a block of exactly its size. Twelve bytes on the X4, as a `String`.
#[derive(Clone)]
pub enum Text {
    Inline { len: u8, bytes: [u8; INLINE] },
    Heap(Box<str>),
}

impl Text {
    /// No text.
    #[must_use]
    pub const fn empty() -> Self {
        Self::Inline {
            len: 0,
            bytes: [0; INLINE],
        }
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self
    }

    /// The text `first` and `second` make together.
    ///
    /// # Errors
    /// `NoMemory` when it cannot be allocated.
    pub fn joined(first: &str, second: &str) -> Result<Self, Error> {
        let len = first.len() + second.len();
        if len <= INLINE {
            let mut bytes = [0; INLINE];
            bytes[..first.len()].copy_from_slice(first.as_bytes());
            bytes[first.len()..len].copy_from_slice(second.as_bytes());
            return Ok(Self::Inline {
                len: len as u8,
                bytes,
            });
        }
        let mut whole = String::new();
        whole.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
        whole.push_str(first);
        whole.push_str(second);
        Ok(Self::Heap(whole.into_boxed_str()))
    }

    /// The text of `text`.
    ///
    /// # Errors
    /// `NoMemory` when it cannot be allocated.
    pub fn try_from_str(text: &str) -> Result<Self, Error> {
        Self::joined(text, "")
    }

    /// The text with `more` after it.
    ///
    /// # Errors
    /// `NoMemory` when it cannot be allocated; the text is as it was.
    pub fn try_push_str(&mut self, more: &str) -> Result<(), Error> {
        *self = Self::joined(self, more)?;
        Ok(())
    }
}

impl Deref for Text {
    type Target = str;

    fn deref(&self) -> &str {
        match self {
            Self::Inline { len, bytes } => bytes
                .get(..usize::from(*len))
                .and_then(|kept| core::str::from_utf8(kept).ok())
                .unwrap_or_default(),
            Self::Heap(text) => text,
        }
    }
}

impl From<&str> for Text {
    /// Where an allocation that fails may stop the program (tests).
    fn from(text: &str) -> Self {
        Self::try_from_str(text).unwrap_or_else(|_| Self::Heap(Box::from(text)))
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for Text {}

impl PartialEq<str> for Text {
    fn eq(&self, other: &str) -> bool {
        **self == *other
    }
}

impl PartialEq<&str> for Text {
    fn eq(&self, other: &&str) -> bool {
        **self == **other
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self)
    }
}

/// A word on the page, with its box on the screen and its place in the book.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub area: Rect,
    pub text: Text,
    pub start: Locator,
    pub end: Locator,
    /// The word runs right to left: its start is at the right of its box.
    pub rtl: bool,
    /// How its line goes, as the layout gave it.
    pub bidi: LineBidi,
}

impl Word {
    pub fn new(area: Rect, text: impl Into<Text>, start: Locator, end: Locator) -> Self {
        Self {
            area,
            text: text.into(),
            start,
            end,
            rtl: false,
            bidi: LineBidi::default(),
        }
    }

    /// The same, running right to left or not.
    #[must_use]
    pub fn rtl(mut self, rtl: bool) -> Self {
        self.rtl = rtl;
        self
    }

    /// The same, on a line that goes as `bidi` says.
    #[must_use]
    pub fn bidi(mut self, bidi: LineBidi) -> Self {
        self.bidi = bidi;
        self
    }

    /// The word without the punctuation around it, as it is looked up.
    #[must_use]
    pub fn lookup_text(&self) -> &str {
        self.text.trim_matches(|c: char| !c.is_alphanumeric())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_kept_in_the_word_and_long_text_in_a_block_of_its_size() {
        let short = Text::try_from_str("héllo,").unwrap();
        assert!(matches!(short, Text::Inline { .. }));
        assert_eq!(&*short, "héllo,");
        let long = Text::try_from_str("extraordinarily").unwrap();
        assert!(matches!(long, Text::Heap(_)));
        assert_eq!(&*long, "extraordinarily");
        let ten = Text::try_from_str("abcdefghij").unwrap();
        assert!(matches!(ten, Text::Inline { .. }));
        assert_eq!(&*ten, "abcdefghij");
    }

    #[test]
    fn joining_moves_between_the_two_forms() {
        let mut text = Text::try_from_str("won-").unwrap();
        text.try_push_str("derful").unwrap();
        assert_eq!(&*text, "won-derful");
        text.try_push_str("ly").unwrap();
        assert!(matches!(text, Text::Heap(_)));
        assert_eq!(&*text, "won-derfully");
        assert_eq!(&*Text::joined("", "").unwrap(), "");
    }

    #[test]
    fn a_word_is_no_larger_than_with_a_string() {
        // The text is as large as a `String` on the device (12 bytes), and
        // not larger on the host.
        assert!(core::mem::size_of::<Text>() <= core::mem::size_of::<String>());
    }
}
