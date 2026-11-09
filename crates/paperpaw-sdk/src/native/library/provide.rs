//! Provided metadata (`docs/abi/library.md` 5, op 24): what another source (calibre)
//! knows better than the book's file.
use super::{Error, Host, wire, *};

/// The tag list of `lib_provided`: at most 32 tags of at most 128 bytes.
const TAGS_MAX: usize = wire::SERVICE_LIB_PROVIDED_TAGS_MAX_ITEMS;
const TAG_MAX_BYTES: usize = wire::SERVICE_LIB_PROVIDED_TAGS_ITEM_MAX_BYTES;

/// Bits of `lib_provided.flags`: the fields in the record's order.
const TITLE: u32 = 1;
const AUTHOR: u32 = 1 << 1;
const SERIES: u32 = 1 << 2;
const NUMBER: u32 = 1 << 3;
const TAGS: u32 = 1 << 4;

/// Metadata a plugin provides for one book: only the fields set here are
/// provided, the rest are left alone. A provided value is never empty (nothing
/// clears a field: leave it out). Provided values win over the file's; the
/// user's own edits win over provided ones, and the last provider wins among
/// providers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Provided<'a> {
    title: &'a str,
    author: &'a str,
    series: &'a str,
    number: &'a str,
    tags: &'a str,
    flags: u32,
}

impl<'a> Provided<'a> {
    /// Nothing provided yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            title: "",
            author: "",
            series: "",
            number: "",
            tags: "",
            flags: 0,
        }
    }

    /// The title (at most 512 bytes).
    ///
    /// # Errors
    /// `Invalid` for an empty or blank one (Unicode whitespace only; a provided value never clears: leave
    /// the field out instead); `NoSpace` for a longer one.
    pub fn title(mut self, title: &'a str) -> Result<Self, Error> {
        if title.trim().is_empty() {
            return Err(Error::Invalid);
        }
        if title.len() > wire::SERVICE_LIB_PROVIDED_TITLE_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        self.title = title;
        self.flags |= TITLE;
        Ok(self)
    }

    /// The author (at most 512 bytes).
    ///
    /// # Errors
    /// `Invalid` for an empty or blank one (Unicode whitespace only; a provided value never clears: leave
    /// the field out instead); `NoSpace` for a longer one.
    pub fn author(mut self, author: &'a str) -> Result<Self, Error> {
        if author.trim().is_empty() {
            return Err(Error::Invalid);
        }
        if author.len() > wire::SERVICE_LIB_PROVIDED_AUTHOR_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        self.author = author;
        self.flags |= AUTHOR;
        Ok(self)
    }

    /// The series (at most 256 bytes).
    ///
    /// # Errors
    /// `Invalid` for an empty or blank one (Unicode whitespace only; a provided value never clears: leave
    /// the field out instead); `NoSpace` for a longer one.
    pub fn series(mut self, series: &'a str) -> Result<Self, Error> {
        if series.trim().is_empty() {
            return Err(Error::Invalid);
        }
        if series.len() > wire::SERVICE_LIB_PROVIDED_SERIES_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        self.series = series;
        self.flags |= SERIES;
        Ok(self)
    }

    /// The number in the series, as text (at most 32 bytes).
    ///
    /// # Errors
    /// `Invalid` for an empty or blank one (Unicode whitespace only; a provided value never clears: leave
    /// the field out instead); `NoSpace` for a longer one.
    pub fn number(mut self, number: &'a str) -> Result<Self, Error> {
        if number.trim().is_empty() {
            return Err(Error::Invalid);
        }
        if number.len() > wire::SERVICE_LIB_PROVIDED_NUMBER_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        self.number = number;
        self.flags |= NUMBER;
        Ok(self)
    }

    /// The tags, one per line: at most 32, each 1 to 128 bytes, 2,048 bytes in all.
    /// They replace this provider's whole tag list.
    ///
    /// # Errors
    /// `Invalid` for no tags, or an empty or blank tag; `NoSpace` for more than 32
    /// tags, a tag over 128 bytes or a text over 2,048 bytes.
    pub fn tags(mut self, tags: &'a str) -> Result<Self, Error> {
        if tags.len() > wire::SERVICE_LIB_PROVIDED_TAGS_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        let mut count = 0;
        for tag in tags.split(wire::SERVICE_LIB_PROVIDED_TAGS_SEPARATOR) {
            count += 1;
            if tag.trim().is_empty() {
                return Err(Error::Invalid);
            }
            if count > TAGS_MAX || tag.len() > TAG_MAX_BYTES {
                return Err(Error::NoSpace);
            }
        }
        self.tags = tags;
        self.flags |= TAGS;
        Ok(self)
    }

    /// Whether any field is provided.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.flags == 0
    }
}

impl Library {
    /// Provide metadata for one book (op 24, needs `uses.library`): the fields set
    /// on `provided` replace the file's, the rest are left alone. Kept with the
    /// book's data, so it follows moves. `input` is 4,096 bytes; `out` 64.
    ///
    /// # Errors
    /// `Invalid` when no field is provided (nothing is sent); `NotFound` for an
    /// unknown book; `State` for a stale catalog handle; `Denied` without
    /// `uses.library`; `Invalid` for a reply that is not empty; otherwise the
    /// host's refusals.
    pub fn provide(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        provided: &Provided<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if provided.is_empty() {
            return Err(Error::Invalid);
        }
        let row = wire::ServiceLibProvided {
            title: provided.title,
            author: provided.author,
            series: provided.series,
            number: provided.number,
            tags: provided.tags,
            flags: provided.flags,
        };
        let n = self.call(host, schema::OP_PROVIDE, &[book.record(), &row], input, out)?;
        if n != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
