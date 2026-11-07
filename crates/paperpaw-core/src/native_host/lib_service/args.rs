//! Input rules shared by the ops of `core.library` (`library.md` 2): a book
//! is named by `lib_ref` as the first record, then the op's own records.
use super::super::registrations::{self as wire, Record};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE};

/// A book as the caller named it: by root-1 path, or by catalog handle with the
/// catalog generation it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BookRef<'a> {
    Path(&'a str),
    Key { key: u32, generation: u32 },
}

/// `lib_ref`: exactly one of `path` and `key`, else `ERR_INVALID`. A key
/// needs the catalog generation it came from; 0 is no generation, so a key
/// without one is stale (`ERR_STATE`).
pub(super) fn book_ref<'a>(reference: &wire::ServiceLibRef<'a>) -> Result<BookRef<'a>, i32> {
    match (reference.path.is_empty(), reference.key) {
        (false, None) => {
            if path_valid(reference.path) {
                Ok(BookRef::Path(reference.path))
            } else {
                Err(ERR_INVALID)
            }
        }
        (true, Some(key)) if reference.generation != 0 => Ok(BookRef::Key {
            key,
            generation: reference.generation,
        }),
        (true, Some(_)) => Err(ERR_STATE),
        _ => Err(ERR_INVALID),
    }
}

/// A root-1 path: relative, the storage grammar (no control characters, so
/// no newline), at most 512 bytes.
pub(super) fn path_valid(path: &str) -> bool {
    path.len() <= 512 && paperpaw_platform::storage::is_valid_path(path)
}

/// An input of `lib_ref` first, then at most `N - 1` more records of the op's
/// own, in wire order. The op decides which records it wants, so a missing,
/// repeated or out-of-order one is its `ERR_INVALID`.
pub(super) struct Parsed<'a, const N: usize> {
    pub(super) reference: wire::ServiceLibRef<'a>,
    pub(super) rest: [Option<Record<'a>>; N],
    pub(super) count: usize,
}

pub(super) fn with_ref<const N: usize>(input: &[u8]) -> Result<Parsed<'_, N>, i32> {
    let mut reference = None;
    let mut rest = [None; N];
    let mut count = 0;
    wire::host_records_visit(input, N + 1, &mut |record| {
        match (&reference, record) {
            (None, Record::ServiceLibRef(first)) => reference = Some(first),
            (Some(_), other) if count < N => {
                rest[count] = Some(other);
                count += 1;
            }
            _ => return Err(ERR_INVALID),
        }
        Ok(())
    })?;
    Ok(Parsed {
        reference: reference.ok_or(ERR_INVALID)?,
        rest,
        count,
    })
}

/// An input of exactly one record (ops that take no `lib_ref`).
pub(super) fn single(input: &[u8]) -> Result<Record<'_>, i32> {
    wire::records_decode_single(input)
}
