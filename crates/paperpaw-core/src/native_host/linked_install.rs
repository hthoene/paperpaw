//! Installing a statically linked native plugin: the kernel checks the entry
//! and its metadata against what it serves, hosts the plugin and registers it
//! through the same transaction a loaded one uses.
use super::{
    flat::FlatDepth,
    linked_entry::{self, Refusal},
    shell,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED, PpEntry, Services};
use paperpaw_native::metadata;
use paperpaw_sdk::Id;

impl crate::Kernel {
    /// Admit, host and register a native plugin linked into this program.
    ///
    /// `host` is the shell's card-plugin host, the very `metadata::Host` it
    /// admits loaded native plugins with, so a linked built-in gets the same
    /// checks (profile, services, features). The entry is checked first
    /// ([`linked_entry::check`]: its metadata view, target profile, versions
    /// and requirements against `host`), then the plugin is hosted and registered exactly as
    /// [`register_native`](Self::register_native) does; `make` runs only in
    /// that registration, and not at all when the entry, the metadata or the
    /// id is refused.
    ///
    /// # Errors
    /// `ERR_INVALID` for an entry that disagrees with its metadata or its
    /// versions, an id that is not a plugin id, or an id already registered;
    /// `ERR_UNSUPPORTED` for metadata the kernel refuses (another profile, a
    /// service or required feature `host` does not offer); otherwise the errors of
    /// [`register_native`](Self::register_native).
    ///
    /// # Safety
    /// `entry` comes from `native_plugin!(..., linked = NAME)` of a crate that
    /// is linked into this program: its factory obeys the SDK's native factory
    /// contract and its code and static data stay loaded for the whole run.
    #[allow(
        clippy::too_many_arguments,
        reason = "admission supplies the shell host, the owner resources, both sinks and the metadata view"
    )]
    pub unsafe fn register_linked<S: Services + 'static>(
        &mut self,
        entry: &'static PpEntry,
        metadata: &'static [u8],
        host: &metadata::Host<'static>,
        id: &str,
        services: S,
        resources: super::HostResources,
        depth: &FlatDepth,
        teardown: shell::Report<S>,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        // Before anything is built and long before `make`: an id that cannot
        // be a plugin id, or one that is taken.
        if Id::try_parse(id).is_err() || self.shared.native_instances.find(id).is_some() {
            return Err(shell::Error::Status(ERR_INVALID));
        }
        let admitted = linked_entry::check(entry, metadata, host).map_err(|refusal| {
            shell::Error::Status(match refusal {
                Refusal::Entry => ERR_INVALID,
                Refusal::Metadata(_) => ERR_UNSUPPORTED,
            })
        })?;
        // SAFETY: the caller guarantees the factory contract and static code.
        let host = unsafe { admitted.hosted(services, id, teardown, resources) }?;
        self.register_native(host, depth, report)
    }
}

#[cfg(all(test, target_arch = "x86_64", target_endian = "little"))]
#[path = "linked_install_tests.rs"]
mod tests;
