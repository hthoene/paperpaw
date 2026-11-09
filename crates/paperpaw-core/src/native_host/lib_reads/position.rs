//! Op 13 `position`: where a book was left, from the position file the
//! readers keep (`paperpaw_sdk::positions`).
use super::fail;
use crate::native_host::lib_service::One;
use crate::native_host::registrations::{self as wire, Record};
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_platform::Storage;
use paperpaw_sdk::positions;

/// `lib_position.flags`: has an XPointer, has a conversion, from sync.
const HAS_XPOINTER: u32 = 1;
const HAS_CONVERSION: u32 = 1 << 1;
/// Bytes of an XPointer the wire carries.
const XPOINTER: usize = 1024;

/// The position of the book at `path`, or `ERR_NOT_FOUND` for none kept.
///
/// `insert` is 0 (the SDK's `Locator` has none) and `from_sync` is not set
/// (the file has no mark of it yet: the writes slice stores one).
pub(super) fn read(storage: &dyn Storage, path: &str, visit: One<'_>) -> Result<(), i32> {
    let position = positions::try_load(storage, path)
        .map_err(fail)?
        .ok_or(ERR_NOT_FOUND)?;
    // A place with an empty XPointer has none; one that does not fit (the
    // SDK already leaves out one over 1,024 bytes as written) is as if it had
    // none, never cut: the rest of the place still answers.
    let xpointer = position
        .xpointer
        .as_deref()
        .filter(|x| !x.is_empty() && x.len() <= XPOINTER)
        .unwrap_or("");
    let mut flags = 0;
    if !xpointer.is_empty() {
        flags |= HAS_XPOINTER;
    }
    if position.conversion.is_some() {
        flags |= HAS_CONVERSION;
    }
    // The file keeps a tenth of a percent: 41.5 % is 415,000 parts per million.
    let percent = if position.percent.is_nan() {
        0.0
    } else {
        position.percent
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=1000"
    )]
    let tenths = (percent.clamp(0.0, 100.0) * 10.0 + 0.5) as u32;
    let progress_ppm = tenths * 1_000;
    visit(Record::ServiceLibPosition(wire::ServiceLibPosition {
        section: position.at.section,
        offset: position.at.offset,
        insert: 0,
        progress_ppm,
        conversion: position.conversion.unwrap_or(0),
        updated: u64::try_from(position.updated).unwrap_or(0),
        xpointer,
        flags,
    }));
    Ok(())
}
