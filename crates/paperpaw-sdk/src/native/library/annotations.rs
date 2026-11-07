//! Per-book annotations; the annotation generation is independent of the catalog.
use super::*;

/// Position in one book's annotations. Obtain continuations from a page end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnnotationCursor {
    pub(super) offset: u32,
    pub(super) generation: u32,
}
impl AnnotationCursor {
    /// Start again after a stale-generation refusal.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            offset: 0,
            generation: 0,
        }
    }
}
/// Kind of mark, including host-defined kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnnotationKind {
    /// A bookmark.
    Bookmark,
    /// A highlight.
    Highlight,
    /// Another kind, numbered from zero independently of the wire discriminant.
    Other(u32),
}
/// A complete annotation row borrowing the caller's reply buffer.
#[derive(Clone, Copy, Debug)]
pub struct Annotation<'a>(wire::ServiceLibAnnotation<'a>);
impl<'a> Annotation<'a> {
    fn decode(raw: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row = raw
            .decode::<wire::ServiceLibAnnotation<'a>>()
            .map_err(|_| Error::Invalid)?;
        if row.flags & !31 != 0 || row.percent > 100 {
            return Err(Error::Invalid);
        }
        Ok(Self(row))
    }
    /// Kind of annotation.
    #[must_use]
    pub const fn kind(self) -> AnnotationKind {
        match self.0.kind {
            0 => AnnotationKind::Bookmark,
            1 => AnnotationKind::Highlight,
            n => AnnotationKind::Other(n - 2),
        }
    }
    /// Conversion associated with these offsets, including a present zero value.
    #[must_use]
    pub const fn conversion(self) -> Option<u64> {
        if self.0.flags & 8 != 0 {
            Some(self.0.conversion)
        } else {
            None
        }
    }
    /// Whether this annotation has XPointers available through the anchors operation.
    #[must_use]
    pub const fn has_xpointers(self) -> bool {
        self.0.flags & 16 != 0
    }
    /// Whether the annotation is marked as having a note.
    #[must_use]
    pub const fn noted(self) -> bool {
        self.0.flags & 1 != 0
    }
    /// Whether the text preview is cut; use annotation_text to read it whole.
    #[must_use]
    pub const fn text_cut(self) -> bool {
        self.0.flags & 2 != 0
    }
    /// Whether the note preview is cut; use annotation_text to read it whole.
    #[must_use]
    pub const fn note_cut(self) -> bool {
        self.0.flags & 4 != 0
    }
}
macro_rules! getter {
    ($field:ident, $ty:ty, $doc:literal) => {
        impl<'a> Annotation<'a> {
            #[doc = $doc]
            #[must_use]
            pub const fn $field(self) -> $ty {
                self.0.$field
            }
        }
    };
}
getter!(
    id,
    u32,
    "Stable per-book annotation id; also the index for full text queries."
);
getter!(
    start_section,
    u32,
    "Start section in the annotation's conversion."
);
getter!(start_offset, u32, "Start offset within the start section.");
getter!(
    end_section,
    u32,
    "End section in the annotation's conversion."
);
getter!(end_offset, u32, "End offset within the end section.");
getter!(percent, u32, "Reading progress from 0 through 100 percent.");
getter!(
    created,
    u64,
    "Creation time in seconds since 1970; zero means unknown."
);
getter!(
    updated,
    u64,
    "Update time in seconds since 1970; zero means unknown."
);
getter!(
    client_key,
    &'a str,
    "Optional caller identity; empty when absent."
);
getter!(text, &'a str, "Text preview, at most 512 UTF-8 bytes.");
getter!(note, &'a str, "Note preview, at most 512 UTF-8 bytes.");

/// Validated per-book count, annotation generation and continuation.
#[derive(Clone, Copy, Debug)]
pub struct AnnotationPageEnd {
    total: u32,
    pub(super) generation: u32,
    next: Option<AnnotationCursor>,
}
impl AnnotationPageEnd {
    /// Count of all annotations in this book, including marks without notes.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// This book's annotation generation, independent of its catalog generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
    /// Continue the same book, or none on the last page.
    #[must_use]
    pub const fn next(self) -> Option<AnnotationCursor> {
        self.next
    }
}
/// Fully checked bounded page with no facade allocations.
/// Rows cannot outlive or survive reuse of the caller's output buffer.
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, library::{Library, BookRef, AnnotationCursor, Annotation}};
/// fn invalid<'a>(lib: &Library, host: &Host<'_>, out: &mut [u8]) -> Annotation<'a> {
///     let mut input = [0; 1024];
///     let page = lib.annotations(host, BookRef::path("book.epub").unwrap(),
///         AnnotationCursor::start(), &mut input, out).unwrap();
///     let row = page.rows().next().unwrap().unwrap();
///     out.fill(0);
///     row
/// }
/// ```
#[derive(Debug)]
pub struct AnnotationPage<'a> {
    bytes: &'a [u8],
    count: usize,
    end: AnnotationPageEnd,
}
impl<'a> AnnotationPage<'a> {
    /// Count and cursor, available without consuming rows.
    #[must_use]
    pub const fn end(&self) -> AnnotationPageEnd {
        self.end
    }
    /// Number of annotation rows in this reply.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Whether this reply has no annotations.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Iterate validated rows borrowed from the reply buffer.
    pub fn rows(&self) -> impl Iterator<Item = Result<Annotation<'a>, Error>> + 'a {
        let mut bytes = self.bytes;
        let mut left = self.count;
        core::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            Some(
                wire::RawRecord::prefix(bytes)
                    .map_err(|_| Error::Invalid)
                    .and_then(|(raw, n)| {
                        bytes = bytes.get(n..).ok_or(Error::Invalid)?;
                        Annotation::decode(raw)
                    }),
            )
        })
    }
    pub(super) fn decode(bytes: &'a [u8], cursor: AnnotationCursor) -> Result<Self, Error> {
        let mut rest = bytes;
        let mut count = 0;
        let end = loop {
            let (raw, n) = wire::RawRecord::prefix(rest).map_err(|_| Error::Invalid)?;
            rest = rest.get(n..).ok_or(Error::Invalid)?;
            if raw.tag() == wire::SERVICE_END_TAG {
                break raw
                    .decode::<wire::ServiceEnd>()
                    .map_err(|_| Error::Invalid)?;
            }
            count += 1;
            if count > schema::PAGE_MAX_ROWS {
                return Err(Error::Invalid);
            }
            Annotation::decode(raw)?;
        };
        if !rest.is_empty() || end.flags > 1 || end.cursor.is_some() || end.generation == 0 {
            return Err(Error::Invalid);
        }
        if cursor.generation != 0 && cursor.generation != end.generation {
            return Err(Error::State);
        }
        let expected = cursor
            .offset
            .checked_add(u32::try_from(count).map_err(|_| Error::Invalid)?)
            .ok_or(Error::Invalid)?;
        let last = end.flags == 1;
        if end.next != expected
            || end.next > end.total
            || last != (end.next == end.total)
            || !last && count == 0
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            bytes,
            count,
            end: AnnotationPageEnd {
                total: end.total,
                generation: end.generation,
                next: (!last).then_some(AnnotationCursor {
                    offset: end.next,
                    generation: end.generation,
                }),
            },
        })
    }
}
impl Library {
    pub(super) fn annotation_call(
        &self,
        host: &Host<'_>,
        op: u32,
        records: &[&dyn wire::WireEncode],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let cap = self.service.max_reply(op).ok_or(Error::Unsupported)?;
        let n = wire::records_encode_of(records, input).map_err(Error::from_status)?;
        let n = self.service.call(host, op, &input[..n], out)?;
        if n > cap {
            return Err(Error::Invalid);
        }
        Ok(n)
    }
    /// Page through one book's annotations in reading order. The first page's
    /// total counts all annotations, including those without notes. Keep the
    /// book's catalog generation separate from this cursor's annotation generation.
    ///
    /// # Errors
    /// State for stale generations; Invalid for malformed replies; otherwise
    /// local encoding limits or host refusals. Restart after State.
    pub fn annotations<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        cursor: AnnotationCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<AnnotationPage<'a>, Error> {
        let page = wire::ServiceLibPage {
            cursor: cursor.offset,
            annotation_generation: cursor.generation,
        };
        let n =
            self.annotation_call(host, schema::OP_ANNOTATIONS, &[&book.0, &page], input, out)?;
        AnnotationPage::decode(&out[..n], cursor)
    }
}
