//! Admission of a statically linked entry through the card plugin's D1 reader.
//!
//! A linked entry uses the same metadata object its PpBytes view identifies.
//! Checking that identity first avoids dereferencing an unverified raw pointer.
//! No factory executes here and no signature or placement check is implied.

use super::shell::Make;
use paperpaw_native::{host_glue::PpEntry, metadata};

/// A refusal before a linked plugin factory is allowed to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The same bounded metadata refusal a native card plugin would receive.
    Metadata(metadata::Refusal<'static>),
    /// Entry size, versions, metadata view or required factory disagree.
    Entry,
}

/// The validated factory and its metadata facts; no host reference is retained.
#[derive(Debug)]
pub(crate) struct Admitted {
    pub(crate) make: Make,
    pub(crate) metadata: metadata::Checked<'static>,
}

/// Admit the immutable entry/metadata pair linked by the app or shell.
///
/// Entry metadata must identify this exact object, as for the loaded-library
/// admission path. Pointer equality plus equal lengths proves byte equality
/// without reading through the entry's raw pointer. Malformed pointers are
/// refused as values, never dereferenced. The D1 reader validates the supplied
/// safe slice, including target, entry layout, versions and requirements.
///
/// The caller still establishes the native factory/lifetime contract before
/// invoking make through Hosted. Neither compatibility metadata nor PpEntry
/// contains a Rust fingerprint or draft; they cannot be checked by this helper.
pub(crate) fn check(
    entry: &'static PpEntry,
    bytes: &'static [u8],
    host: &metadata::Host<'_>,
) -> Result<Admitted, Refusal> {
    if entry.size as usize != core::mem::size_of::<PpEntry>()
        || entry.metadata.ptr != bytes.as_ptr()
        || entry.metadata.len != bytes.len()
    {
        return Err(Refusal::Entry);
    }
    let checked = metadata::check(bytes, host).map_err(Refusal::Metadata)?;
    if entry.size != u32::from(checked.entry_bytes)
        || entry.abi_major != checked.abi_major
        || entry.needs_minor != checked.needs_minor
    {
        return Err(Refusal::Entry);
    }
    let make = entry.make.ok_or(Refusal::Entry)?;
    Ok(Admitted {
        make,
        metadata: checked,
    })
}

#[cfg(all(test, target_arch = "x86_64", target_endian = "little"))]
#[path = "linked_entry_tests.rs"]
mod tests;
