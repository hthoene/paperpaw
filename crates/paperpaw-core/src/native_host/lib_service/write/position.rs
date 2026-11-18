//! `set_position` (`library.md` 2.2): a local write always wins; a sync write
//! wins only over an older sync position.
use super::{
    super::{
        args,
        wire::{self, Record},
    },
    require, result_row,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE, OK};

/// `lib_position.flags`: bit 0 has an XPointer, bit 1 has a conversion, bit 2
/// from sync (the host stamps it from the write's `sync`).
const HAS_XPOINTER: u32 = 1;
const HAS_CONVERSION: u32 = 2;
const POSITION_FLAGS: u32 = 0b111;
/// `lib_position_set.flags`: bit 0 sync, bit 1 confirmed.
const SYNC: u32 = 1;
const CONFIRMED: u32 = 2;
const PPM: u32 = 1_000_000;

/// What the host kept of the book's position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoredPosition {
    pub(crate) from_sync: bool,
    /// Seconds since 1970, 0 for an unknown clock.
    pub(crate) updated: u64,
}

/// Does a sync write of `updated` replace what is stored? Never a local
/// position (`Err(ERR_STATE)`, nothing is stored); over a sync one when it is
/// newer or the stored time is unknown; an equal or older one keeps the stored.
fn sync_wins(stored: Option<StoredPosition>, updated: u64) -> Result<bool, i32> {
    match stored {
        None => Ok(true),
        Some(stored) if !stored.from_sync => Err(ERR_STATE),
        Some(stored) => Ok(stored.updated == 0 || updated > stored.updated),
    }
}

pub(super) fn set(
    host: &dyn super::super::LibHost,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let parsed = args::with_ref::<2>(input)?;
    let book = args::book_ref(host, &parsed.reference)?;
    let [
        Some(Record::ServiceLibPosition(position)),
        Some(Record::ServiceLibPositionSet(how)),
    ] = parsed.rest
    else {
        return Err(ERR_INVALID);
    };
    check(&position, &how)?;
    let sync = how.flags & SYNC != 0;
    // A confirmed sync write is what the user chose: it counts as local.
    let local = !sync || how.flags & CONFIRMED != 0;
    if !local && !sync_wins(host.stored_position(book)?, position.updated)? {
        // Nothing to do: the stored position is as new or newer.
        return book_result(host, book, out);
    }
    host.write_position(book, &position, !local)?;
    book_result(host, book, out)
}

fn book_result(
    host: &dyn super::super::LibHost,
    book: args::BookRef<'_>,
    out: &mut [u8],
) -> Result<usize, i32> {
    let mut written = Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
    host.book_path(book, &mut |path| written = result_row(path, OK, 0, out))?;
    written
}

fn check(
    position: &wire::ServiceLibPosition<'_>,
    how: &wire::ServiceLibPositionSet,
) -> Result<(), i32> {
    require(
        position.flags & !POSITION_FLAGS == 0
            && how.flags & !(SYNC | CONFIRMED) == 0
            // `confirmed` only means something for a sync write.
            && (how.flags & CONFIRMED == 0 || how.flags & SYNC != 0)
            && position.progress_ppm <= PPM
            && (position.flags & HAS_XPOINTER != 0) == !position.xpointer.is_empty()
            // Absent and 0 differ: a conversion value needs its flag.
            && (position.flags & HAS_CONVERSION != 0 || position.conversion == 0),
    )
}
