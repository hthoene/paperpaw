use super::*;
use core::fmt;

/// Current catalog walk phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CatalogPhase {
    /// No books have been found yet.
    Empty,
    /// A walk is finding books.
    Walking,
    /// The catalog is ready.
    Ready,
    /// A phase added by a newer host.
    Unknown(u32),
}
/// Catalog state, including a generation suitable for catalog references.
#[derive(Clone, Copy, Debug)]
pub struct CatalogState(wire::ServiceLibState);
impl CatalogState {
    pub(super) fn decode(record: Record<'_>) -> Result<Self, Error> {
        let Record::ServiceLibState(row) = record else {
            return Err(Error::Invalid);
        };
        if row.generation == 0 || row.walk_ppm > 1_000_000 {
            return Err(Error::Invalid);
        }
        Ok(Self(row))
    }
    /// Current catalog walk phase.
    #[must_use]
    pub const fn phase(self) -> CatalogPhase {
        match self.0.state {
            0 => CatalogPhase::Empty,
            1 => CatalogPhase::Walking,
            2 => CatalogPhase::Ready,
            n => CatalogPhase::Unknown(n),
        }
    }
    /// Number of books found.
    #[must_use]
    pub const fn books(self) -> u32 {
        self.0.books
    }
    /// Current catalog generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.0.generation
    }
    /// Walk progress in parts per million.
    #[must_use]
    pub const fn walk_ppm(self) -> u32 {
        self.0.walk_ppm
    }
}
macro_rules! row {
    ($name:ident,$wire:ident,$doc:literal) => {
        #[doc=$doc]
        #[derive(Clone, Copy, Debug)]
        pub struct $name<'a>(wire::$wire<'a>);
        impl<'a> $name<'a> {
            pub(super) fn decode(record: Record<'a>) -> Result<Self, Error> {
                let Record::$wire(row) = record else {
                    return Err(Error::Invalid);
                };
                validate_row(record)?;
                Ok(Self(row))
            }
        }
    };
}
row!(
    Entry,
    ServiceLibEntry,
    "A borrowed catalog query row, including book badges."
);
row!(
    Book,
    ServiceLibBook,
    "Borrowed book metadata; long text is available separately."
);
fn validate_row(record: Record<'_>) -> Result<(), Error> {
    match record {
        Record::ServiceLibEntry(row) => {
            if row.progress > 100 && row.progress != 255
                || !row.badges.is_empty()
                    && (row.badges.split('\n').count() > 2
                        || row.badges.split('\n').any(str::is_empty))
            {
                return Err(Error::Invalid);
            }
        }
        Record::ServiceLibBook(row)
            if row.generation == 0
                || row.path.is_empty()
                || row.progress > 100 && row.progress != 255 =>
        {
            return Err(Error::Invalid);
        }
        _ => {}
    }
    Ok(())
}
macro_rules! getter {
    ($name:ident, $field:ident,$ty:ty,$doc:literal) => {
        impl<'a> $name<'a> {
            #[doc=$doc]
            #[must_use]
            pub const fn $field(self) -> $ty {
                self.0.$field
            }
        }
    };
}
macro_rules! book_getters {
    ($name:ident) => {
        getter!(
            $name,
            key,
            u32,
            "Opaque catalog key, valid only within its generation."
        );
        getter!(
            $name,
            title,
            &'a str,
            "Display title; the host may clip this field."
        );
        getter!($name, author, &'a str, "Author display text.");
        getter!($name, series, &'a str, "Series display text.");
        getter!($name, number, &'a str, "Series number text.");
        getter!($name, added, u32, "Added day since 1970.");
        impl $name<'_> {
            /// Reading progress, or None when this book has never been opened.
            #[must_use]
            pub fn progress(self) -> Option<u8> {
                u8::try_from(self.0.progress).ok().filter(|n| *n <= 100)
            }
            /// The book is marked finished.
            #[must_use]
            pub const fn finished(self) -> bool {
                self.0.flags & 4 != 0
            }
            /// The book is a favourite.
            #[must_use]
            pub const fn favourite(self) -> bool {
                self.0.flags & 1 != 0
            }
            /// The book has been opened.
            #[must_use]
            pub const fn opened(self) -> bool {
                self.0.flags & 2 != 0
            }
            /// The book was added recently and has not been opened.
            #[must_use]
            pub const fn is_new(self) -> bool {
                self.0.flags & 8 != 0
            }
        }
    };
}
book_getters!(Entry);
book_getters!(Book);
getter!(
    Book,
    generation,
    u32,
    "Generation of this book's catalog handle."
);
getter!(Book, path, &'a str, "Complete card path, never truncated.");
getter!(Book, size, u64, "Book file length in bytes.");
getter!(Book, format, &'a str, "Format identity.");
getter!(
    Book,
    tags,
    u32,
    "Number of tags, available through text queries."
);
getter!(Book, fields, u32, "Number of plugin fields.");
impl<'a> Entry<'a> {
    /// Up to two badge texts supplied by the host.
    pub fn badges(self) -> impl Iterator<Item = &'a str> {
        self.0.badges.split('\n').filter(|badge| !badge.is_empty())
    }
}
impl<'a> Book<'a> {
    /// A generation-bound reference to this book.
    ///
    /// # Errors
    /// Invalid if the host returned generation zero.
    pub fn reference(self) -> Result<BookRef<'a>, Error> {
        BookRef::catalog(self.key(), self.generation())
    }
}

/// Validated terminal metadata for a library list.
#[derive(Clone, Copy, Debug)]
pub struct LibraryPageEnd {
    total: u32,
    generation: u32,
    next: Option<CatalogCursor>,
}
impl LibraryPageEnd {
    /// Total number of matching rows.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// Catalog generation, or zero for an unpaged list.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
    /// Continue this catalog query, or None for its last page or an unpaged list.
    #[must_use]
    pub const fn next(self) -> Option<CatalogCursor> {
        self.next
    }
}
type Decode<'a, T> = fn(Record<'a>) -> Result<T, Error>;
/// A completely checked list borrowing the caller's reply buffer.
pub struct LibraryPage<'a, T> {
    bytes: &'a [u8],
    count: usize,
    end: LibraryPageEnd,
    decode: Decode<'a, T>,
}
impl<T> fmt::Debug for LibraryPage<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibraryPage")
            .field("rows", &self.count)
            .field("end", &self.end)
            .finish()
    }
}
impl<'a, T: 'a> LibraryPage<'a, T> {
    /// Terminal metadata, validated before any row is exposed.
    #[must_use]
    pub const fn end(&self) -> LibraryPageEnd {
        self.end
    }
    /// Rows in this reply.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Whether this reply has no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Iterate the validated borrowed rows without allocating.
    pub fn rows(&self) -> impl Iterator<Item = Result<T, Error>> + 'a {
        let decode = self.decode;
        let mut bytes = self.bytes;
        let mut left = self.count;
        core::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            Some(
                wire::record_decode_prefix(bytes)
                    .map_err(|_| Error::Invalid)
                    .and_then(|(row, n)| {
                        bytes = bytes.get(n..).ok_or(Error::Invalid)?;
                        decode(row)
                    }),
            )
        })
    }
}
pub(super) fn page<'a, T>(
    bytes: &'a [u8],
    cursor: Option<CatalogCursor>,
    cap: usize,
    decode: Decode<'a, T>,
) -> Result<LibraryPage<'a, T>, Error> {
    let mut rest = bytes;
    let mut count = 0;
    let end = loop {
        let (record, n) = wire::record_decode_prefix(rest).map_err(|_| Error::Invalid)?;
        rest = rest.get(n..).ok_or(Error::Invalid)?;
        if let Record::ServiceEnd(end) = record {
            break end;
        }
        count += 1;
        if count > cap {
            return Err(Error::Invalid);
        }
        decode(record)?;
    };
    if !rest.is_empty() {
        return Err(Error::Invalid);
    }
    let count_u32 = u32::try_from(count).map_err(|_| Error::Invalid)?;
    let next = if let Some(cursor) = cursor {
        if end.generation == 0 {
            return Err(Error::Invalid);
        }
        if cursor.generation != 0 && cursor.generation != end.generation {
            return Err(Error::State);
        }
        let expected = cursor.offset.checked_add(count_u32).ok_or(Error::Invalid)?;
        let last = end.flags & 1 != 0;
        if end.flags > 1
            || end.next != expected
            || end.next > end.total
            || last != (end.next == end.total)
            || !last && count == 0
        {
            return Err(Error::Invalid);
        }
        (!last).then_some(CatalogCursor {
            offset: end.next,
            generation: end.generation,
        })
    } else {
        if end.flags != 0 || end.generation != 0 || end.next != 0 || end.total != count_u32 {
            return Err(Error::Invalid);
        }
        None
    };
    Ok(LibraryPage {
        bytes,
        count,
        end: LibraryPageEnd {
            total: end.total,
            generation: end.generation,
            next,
        },
        decode,
    })
}
