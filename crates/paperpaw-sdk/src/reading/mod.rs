//! Building blocks of reading, for readers and for the plugins around
//! them: a book through the plugins' text filters ([`FilteredDocument`]),
//! the starts of its pages kept on the device ([`PageIndex`]), how it is
//! opened (`open_options`); with the feature `unstable` (as the layout
//! it builds on, until 1.0) the type of its pages and the page drawn
//! (`PageType`, `ReadingFaces`, `word_boxes`, `draw_lines`).
//!
//! The words of the page shown: any plugin asks the reader with
//! `ask_page_words` and hears them as [`PageWords`] (a dictionary that
//! lists the page's hard words, a summary, a word count). The reader
//! answers while a book is open; nothing comes otherwise. Decorators get
//! the words with every page without asking (see
//! [`decorations`](crate::decorations)).

mod filtered;
mod index;
#[cfg(feature = "unstable")]
mod typeset;
#[cfg(feature = "unstable")]
mod visual;

pub use index::PageIndex;
#[cfg(feature = "unstable")]
pub use typeset::{
    EmbeddedFonts, PageType, ReadingFaces, StatusRoom, draw_image, draw_lines, small_caps_parts,
    uses_book_fonts, word_boxes,
};

use alloc::string::String;

use crate::Rect;
use crate::document::Locator;
/// How a line goes ([`WordBox::bidi`]): also without the `unstable`
/// layout, for page layers.
pub use paperpaw_layout::LineBidi;

/// A word on the page, with its box on the screen and its place in the
/// book (`word_boxes`, page layers' `PageView::words`).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WordBox {
    pub area: Rect,
    pub text: String,
    pub start: Locator,
    pub end: Locator,
    /// The word runs right to left (Arabic, Hebrew): its start is at the
    /// right of its box, so a selection from it grows to the left.
    pub rtl: bool,
    /// How its line goes, as the layout gave it (its `PageLine::bidi`): the
    /// direction of its paragraph ([`LineBidi::is_rtl`]: a line of an
    /// Arabic book that holds only an English quote is one of a
    /// right-to-left paragraph), a vertical line, the paragraph's first
    /// line.
    pub bidi: LineBidi,
}

impl WordBox {
    pub fn new(area: Rect, text: impl Into<String>, start: Locator, end: Locator) -> Self {
        Self {
            area,
            text: text.into(),
            start,
            end,
            rtl: false,
            bidi: LineBidi::default(),
        }
    }

    /// The same, running right to left or not (see [`WordBox::rtl`]).
    #[must_use]
    pub fn rtl(mut self, rtl: bool) -> Self {
        self.rtl = rtl;
        self
    }

    /// The same, on a line that goes as `bidi` says (see [`WordBox::bidi`]).
    #[must_use]
    pub fn bidi(mut self, bidi: LineBidi) -> Self {
        self.bidi = bidi;
        self
    }

    /// The word without the punctuation around it, as it is looked up.
    pub fn lookup_text(&self) -> &str {
        self.text.trim_matches(|c: char| !c.is_alphanumeric())
    }
}
