//! Annotation anchors: the conversion and the XPointers of annotations
//! (`docs/abi/library.md` 2.2, op 16), which a reader whose conversion differs
//! uses to relocate an annotation's offsets.
use super::{
    annotations::{AnnotationCursor, AnnotationPageEnd, scan},
    *,
};

/// Bits of `lib_anchor.flags`.
const HAS_CONVERSION: u32 = 1;
const HAS_XPOINTERS: u32 = 1 << 1;

/// The anchors of one annotation, borrowed from the caller's reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor<'a>(wire::ServiceLibAnchor<'a>);

impl<'a> Anchor<'a> {
    fn decode(raw: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row = raw
            .decode::<wire::ServiceLibAnchor<'a>>()
            .map_err(|_| Error::Invalid)?;
        // Only the two defined bits; XPointers appear exactly with their bit, and
        // a conversion of 0 appears only with its bit.
        if row.flags & !(HAS_CONVERSION | HAS_XPOINTERS) != 0
            || row.flags & HAS_XPOINTERS == 0 && !(row.start.is_empty() && row.end.is_empty())
            || row.flags & HAS_CONVERSION == 0 && row.conversion != 0
        {
            return Err(Error::Invalid);
        }
        Ok(Self(row))
    }

    /// The annotation's id, as in [`Annotation::id`].
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.0.id
    }

    /// The conversion the annotation's offsets belong to, when it has one (a
    /// present 0 differs from none).
    #[must_use]
    pub const fn conversion(&self) -> Option<u64> {
        if self.0.flags & HAS_CONVERSION != 0 {
            Some(self.0.conversion)
        } else {
            None
        }
    }

    /// The start and end KOReader XPointers (each at most 1,024 bytes), when the
    /// annotation has them. A longer XPointer in a book's file reads as absent.
    #[must_use]
    pub const fn xpointers(&self) -> Option<(&'a str, &'a str)> {
        if self.0.flags & HAS_XPOINTERS != 0 {
            Some((self.0.start, self.0.end))
        } else {
            None
        }
    }
}

/// A page of anchors, validated in full before any row is exposed.
#[derive(Debug)]
pub struct AnchorPage<'a> {
    bytes: &'a [u8],
    count: usize,
    end: AnnotationPageEnd,
}

impl<'a> AnchorPage<'a> {
    fn decode(bytes: &'a [u8], cursor: AnnotationCursor) -> Result<Self, Error> {
        let mut ids = [0; schema::PAGE_MAX_ROWS];
        let (count, end) = scan(bytes, cursor, |raw, at| {
            let id = Anchor::decode(raw)?.id();
            if ids[..at].contains(&id) {
                return Err(Error::Invalid);
            }
            ids[at] = id;
            Ok(())
        })?;
        Ok(Self { bytes, count, end })
    }

    /// The total number of anchors, the book's annotation generation and the
    /// continuation.
    #[must_use]
    pub const fn end(&self) -> AnnotationPageEnd {
        self.end
    }

    /// Number of anchors in this page.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether this page has no anchors.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Iterate the borrowed anchors without allocation.
    pub fn rows(&self) -> impl Iterator<Item = Anchor<'a>> + use<'a> {
        let mut bytes = self.bytes;
        let mut left = self.count;
        core::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            let (raw, n) = wire::RawRecord::prefix(bytes).ok()?;
            bytes = bytes.get(n..)?;
            Anchor::decode(raw).ok()
        })
    }
}

impl Library {
    /// Page through the anchors of one book's annotations (op 16): the
    /// conversion and the start and end XPointers of every annotation that has
    /// them, in the order of the annotations. The page's end carries the same
    /// annotation generation as [`annotations`](Self::annotations); continue with
    /// its cursor. `input` is 1,024 bytes and `out` 4,096.
    ///
    /// # Errors
    /// `State` for a stale annotation generation (start again from
    /// [`AnnotationCursor::start`]); `NotFound` for an unknown book; `Invalid` for
    /// a malformed page; otherwise the host's refusals.
    pub fn anchors<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        cursor: AnnotationCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<AnchorPage<'a>, Error> {
        let page = wire::ServiceLibPage {
            cursor: cursor.offset,
            annotation_generation: cursor.generation,
        };
        let n = self.annotation_call(host, schema::OP_ANCHORS, &[&book.0, &page], input, out)?;
        AnchorPage::decode(out.get(..n).ok_or(Error::Invalid)?, cursor)
    }
}
