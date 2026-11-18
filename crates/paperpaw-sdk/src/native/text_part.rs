//! The one check of a part of a long text that LIB op 4 and DOC op 4 both answer
//! (`docs/abi/library.md` 2.1, `docs/abi/document.md` 2.2): at most 1,024 bytes from the
//! offset, cut only at a UTF-8 boundary (up to three bytes short of the cap), the whole rest
//! when it fits.
use super::Error;

/// Most bytes of text one part holds.
const PART_MAX: u32 = 1024;

/// What a reply says about its part: the kind and index it answers and the text's whole length.
#[derive(Clone, Copy)]
pub(super) struct Answer {
    pub kind: u32,
    pub index: u32,
    pub total: u32,
}

/// Check that a part of `len` bytes answers the text asked for (`kind`, `index`) from `offset`,
/// and return where the next part starts (`None` when the text is complete).
///
/// # Errors
/// `Invalid` for another text, an offset past the end, or a length the rules above do not allow.
pub(super) fn next_offset(
    asked: (u32, u32, u32),
    answer: Answer,
    len: usize,
) -> Result<Option<u32>, Error> {
    let (kind, index, offset) = asked;
    let remaining = answer.total.checked_sub(offset).ok_or(Error::Invalid)?;
    let len = u32::try_from(len).map_err(|_| Error::Invalid)?;
    let want = remaining.min(PART_MAX);
    let next = offset.checked_add(len).ok_or(Error::Invalid)?;
    if answer.kind != kind
        || answer.index != index
        || len > want
        || len + 3 < want
        || remaining <= PART_MAX && len != remaining
    {
        return Err(Error::Invalid);
    }
    Ok((next < answer.total).then_some(next))
}
