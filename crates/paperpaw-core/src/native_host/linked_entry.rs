//! Admission of a statically linked entry through the card plugin's D1 reader.
//!
//! A linked entry uses the same metadata object its PpBytes view identifies.
//! Checking that identity first avoids dereferencing an unverified raw pointer.
//! No factory executes here and no signature or placement check is implied.

use super::shell::{self, Make};
use paperpaw_native::host_glue::Services;
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
    features: shell::AdmittedFeatures,
    pub(crate) metadata: metadata::Checked<'static>,
}

impl Admitted {
    /// Copy validated service admission before retaining the linked factory.
    /// This constructs only the host; the registration transaction runs make.
    ///
    /// # Safety
    /// The linked factory must obey Hosted's native factory contract. Its code
    /// and static data must remain loaded for the entire program lifetime.
    /// `image` is a read-only mapping of that many bytes that stays mapped for
    /// the whole run (the program's read-only data), or a null pointer with
    /// zero length (declarations are then copied).
    pub(crate) unsafe fn hosted<S: Services>(
        self,
        services: S,
        id: paperpaw_sdk::Id,
        report: shell::Report<S>,
        resources: super::HostResources,
        image: (*const u8, usize),
    ) -> Result<shell::Hosted<S, ()>, shell::Error> {
        let declared =
            shell::DeclaredServices::try_from_iter(self.metadata.services()).map_err(|error| {
                match error {
                    shell::DeclaredServicesError::Invalid => {
                        shell::Error::Status(paperpaw_native::host_glue::ERR_INVALID)
                    }
                    shell::DeclaredServicesError::NoMemory => shell::Error::Allocation,
                }
            })?;
        // SAFETY: the caller guarantees the factory contract, the static code
        // lifetime and the image; a linked entry retains no unloadable module.
        unsafe {
            shell::Hosted::with_caller(
                services,
                (),
                self.make,
                id,
                image,
                report,
                resources,
                declared.with_features(self.features),
            )
        }
    }

    /// As [`hosted`](Self::hosted), under the owner key and heap row `seat`
    /// of the plugin's earlier host: a parked plugin made again (#248).
    ///
    /// # Safety
    /// As [`hosted`](Self::hosted).
    pub(crate) unsafe fn hosted_for_owner<S: Services + 'static>(
        self,
        services: S,
        id: paperpaw_sdk::Id,
        report: shell::Report<S>,
        resources: super::HostResources,
        image: (*const u8, usize),
        seat: super::resources::OwnerSeat,
    ) -> Result<shell::Hosted<S, ()>, shell::Error> {
        let declared =
            shell::DeclaredServices::try_from_iter(self.metadata.services()).map_err(|error| {
                match error {
                    shell::DeclaredServicesError::Invalid => {
                        shell::Error::Status(paperpaw_native::host_glue::ERR_INVALID)
                    }
                    shell::DeclaredServicesError::NoMemory => shell::Error::Allocation,
                }
            })?;
        // SAFETY: as `hosted`, passed on.
        unsafe {
            shell::Hosted::new_for_owner(
                services,
                (),
                self.make,
                id,
                image,
                report,
                resources,
                seat,
                declared.with_features(self.features),
            )
        }
    }
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
    host: &metadata::Host<'static>,
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
    let features = shell::AdmittedFeatures::from_checked(&checked, host.features)
        .map_err(|_| Refusal::Entry)?;
    Ok(Admitted {
        make,
        features,
        metadata: checked,
    })
}

#[cfg(all(test, target_arch = "x86_64", target_endian = "little"))]
#[path = "linked_entry_tests.rs"]
mod tests;
