//! Lossless annotation text uses bounded LIB text chunks rather than previews.
use super::*;
use crate::native::text_part;

/// Which complete annotation string to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnnotationTextKind {
    /// Full marked text.
    Text,
    /// Full note.
    Note,
}
impl AnnotationTextKind {
    const fn wire(self) -> u32 {
        match self {
            Self::Text => 5,
            Self::Note => 6,
        }
    }
}
/// Select one annotation string and its byte offset for a bounded text call.
#[derive(Clone, Copy, Debug)]
pub struct AnnotationTextQuery {
    id: u32,
    kind: AnnotationTextKind,
    offset: u32,
}
impl AnnotationTextQuery {
    /// Read from the start of one annotation's full text or note.
    #[must_use]
    pub const fn new(id: u32, kind: AnnotationTextKind) -> Self {
        Self {
            id,
            kind,
            offset: 0,
        }
    }
    /// Continue at the byte offset returned by the preceding text part.
    #[must_use]
    pub const fn from(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }
}
/// A borrowed UTF-8 chunk of an annotation string, at most 1,024 bytes.
#[derive(Clone, Copy, Debug)]
pub struct AnnotationTextPart<'a> {
    text: &'a str,
    total: u32,
    next: Option<u32>,
}
impl<'a> AnnotationTextPart<'a> {
    /// This chunk's complete UTF-8 text.
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.text
    }
    /// Full string length in UTF-8 bytes.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// Next byte offset, or none when the string is complete.
    #[must_use]
    pub const fn next_offset(self) -> Option<u32> {
        self.next
    }
}
impl Library {
    /// Read a complete annotation's text or note in caller-buffered chunks.
    /// Start at offset zero and continue with next_offset until none. The id is
    /// the annotation id, not its index in the page. This operation has no
    /// generation input: callers needing a stable export must handle edits.
    ///
    /// # Errors
    /// Invalid for a wrong string, inconsistent length or nonprogressing reply;
    /// otherwise local encoding limits or host refusals, including NotFound.
    pub fn annotation_text<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        query: AnnotationTextQuery,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<AnnotationTextPart<'a>, Error> {
        let AnnotationTextQuery { id, kind, offset } = query;
        let request = wire::ServiceLibTextAt {
            kind: kind.wire(),
            index: id,
            offset,
        };
        let n = self.annotation_call(host, schema::OP_TEXT, &[&book.0, &request], input, out)?;
        let (text, total, next) =
            text_reply(out.get(..n).ok_or(Error::Invalid)?, kind.wire(), id, offset)?;
        Ok(AnnotationTextPart { text, total, next })
    }
}

/// Validate one `lib_text` reply for the string asked (`kind`, `index`) from `offset`: the
/// text, its whole length and where the next part starts (`text_part::next_offset`).
pub(super) fn text_reply<'a>(
    bytes: &'a [u8],
    kind: u32,
    index: u32,
    offset: u32,
) -> Result<(&'a str, u32, Option<u32>), Error> {
    let row = wire::RawRecord::one(bytes)
        .and_then(|raw| raw.decode::<wire::ServiceLibText<'a>>())
        .map_err(|_| Error::Invalid)?;
    let next = text_part::next_offset(
        (kind, index, offset),
        text_part::Answer {
            kind: row.kind,
            index: row.index,
            total: row.total,
        },
        row.text.len(),
    )?;
    Ok((row.text, row.total, next))
}
