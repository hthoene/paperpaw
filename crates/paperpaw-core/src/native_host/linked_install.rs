//! Installing a statically linked native plugin: the kernel checks the entry
//! and its metadata against what it serves, hosts the plugin and registers it
//! through the same transaction a loaded one uses.
use super::{
    flat::FlatDepth,
    linked_entry::{self, Refusal},
    shell,
};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_UNSUPPORTED, PpEntry, Services};
use paperpaw_native::{bundle, metadata};
use paperpaw_sdk::Id;

impl crate::Kernel {
    /// Admit, host and register a native plugin linked into this program.
    ///
    /// `host` is the shell's card-plugin host, the very `metadata::Host` it
    /// admits loaded native plugins with, so a linked built-in gets the same
    /// checks (profile, services, features). The entry is checked first
    /// (`linked_entry::check`: its metadata view, target profile, versions
    /// and requirements against `host`), then the plugin is hosted and registered exactly as
    /// [`register_native`](Self::register_native) does; `make` runs only in
    /// that registration, and not at all when the entry, the metadata or the
    /// id is refused.
    ///
    /// # Errors
    /// `ERR_DENIED` for the host's reserved id `core`;
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
    /// `image` is a read-only mapping of that many bytes which stays mapped,
    /// and is never written by anyone (no writable alias, no remap, no flash
    /// write into its range), for the whole process; it holds the plugin's declaration bytes (the program's
    /// read-only data): the declarations are then kept as offsets into it and
    /// not copied, as for a loaded plugin, and the titles and other texts they
    /// hold are read from it where the registries keep them (no copy either,
    /// #248). A null pointer with zero length gives none (the declarations are
    /// copied, under the smaller owned-copy limits).
    /// `fixed` is the plugin's tables of settings, setting groups and ids kept
    /// in the program (empty for none): a setting or group it declares that
    /// equals an entry of its table is kept as that entry (Beta 3's way, about
    /// 4 bytes of heap for a setting) and not as a copy, an id found in its id
    /// table is that entry; any other is copied as before.
    #[allow(
        clippy::too_many_arguments,
        reason = "admission supplies the shell host, the owner resources, both sinks and the metadata view"
    )]
    pub unsafe fn register_linked<S: Services + 'static>(
        &mut self,
        entry: &'static PpEntry,
        metadata: &'static [u8],
        host: &metadata::Host<'static>,
        id: &'static str,
        services: S,
        resources: super::HostResources,
        depth: &FlatDepth,
        teardown: shell::Report<S>,
        report: fn(&S, &str, u32, i32),
        image: (*const u8, usize),
        fixed: super::FixedTables,
    ) -> Result<(), shell::Error> {
        // SAFETY: this function's contract, passed on.
        unsafe {
            self.register_linked_with(
                entry, metadata, host, id, services, resources, depth, teardown, report, image,
                fixed, None,
            )
        }
    }

    /// [`register_linked`](Self::register_linked) for a plugin the kernel may
    /// park (#248, `docs/measurements/parked-plugins.md`): where its `make`
    /// only registers, it holds nothing of the host, its declarations all lie
    /// in `image` and no other plugin's callback can reach it, its instance is
    /// dropped right after its declarations are published and made again on
    /// first use. `services` makes the shell's services handle for each
    /// hosting; `id` and `host` are kept for them.
    ///
    /// # Errors
    /// As [`register_linked`](Self::register_linked).
    ///
    /// # Safety
    /// As [`register_linked`](Self::register_linked), for every hosting of the
    /// plugin, not only the first.
    #[allow(
        clippy::too_many_arguments,
        reason = "as `register_linked`, with the services' maker"
    )]
    pub unsafe fn register_linked_parked<S: Services + 'static>(
        &mut self,
        entry: &'static PpEntry,
        metadata: &'static [u8],
        host: &metadata::Host<'static>,
        id: &'static str,
        services: fn() -> S,
        resources: super::HostResources,
        depth: &FlatDepth,
        teardown: shell::Report<S>,
        report: fn(&S, &str, u32, i32),
        image: (*const u8, usize),
        fixed: super::FixedTables,
    ) -> Result<(), shell::Error> {
        let Some(kept) = resources.try_clone() else {
            return Err(shell::Error::Allocation);
        };
        let park = super::park_linked::ParkArgs {
            entry,
            metadata,
            host: *host,
            id,
            services,
            resources: kept,
            teardown,
            image,
            fixed,
        };
        // SAFETY: this function's contract, passed on.
        unsafe {
            self.register_linked_with(
                entry,
                metadata,
                host,
                id,
                services(),
                resources,
                depth,
                teardown,
                report,
                image,
                fixed,
                Some(park),
            )
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "as `register_linked`, with what parking keeps"
    )]
    unsafe fn register_linked_with<S: Services + 'static>(
        &mut self,
        entry: &'static PpEntry,
        metadata: &'static [u8],
        host: &metadata::Host<'static>,
        id: &'static str,
        services: S,
        resources: super::HostResources,
        depth: &FlatDepth,
        teardown: shell::Report<S>,
        report: fn(&S, &str, u32, i32),
        image: (*const u8, usize),
        fixed: super::FixedTables,
        park: Option<super::park_linked::ParkArgs<S>>,
    ) -> Result<(), shell::Error> {
        // Before anything is built and long before `make`: the host's own id
        // (reserved, ui.md 4.3), an id that is not a plugin id (1 to 40 bytes
        // of `[a-z0-9-]`, as for a loaded plugin), or one that is taken.
        if id == crate::RESERVED_PLUGIN_ID {
            return Err(shell::Error::Status(ERR_DENIED));
        }
        if !bundle::is_plugin_id(id) || self.shared.native_instances.find(id).is_some() {
            return Err(shell::Error::Status(ERR_INVALID));
        }
        // A plugin id (one segment of `[a-z0-9-]`) is a valid `Id`, so this
        // cannot panic, and no copy of it is made, not even to check it:
        // every copy is the firmware's text, none takes heap (#248, D4).
        let caller = Id::new_static(id);
        let admitted = linked_entry::check(entry, metadata, host).map_err(|refusal| {
            shell::Error::Status(match refusal {
                Refusal::Entry => ERR_INVALID,
                Refusal::Metadata(_) => ERR_UNSUPPORTED,
            })
        })?;
        // The id is a linked plugin's for `core.files` before the plugin can
        // run; a registration that fails takes it back.
        let linked = resources.hosts().linked().clone();
        linked.add(id).map_err(shell::Error::Status)?;
        // SAFETY: the caller guarantees the factory contract and static code.
        let host = match unsafe { admitted.hosted(services, caller, teardown, resources, image) } {
            Ok(host) => host,
            Err(error) => {
                linked.remove(id);
                return Err(error);
            }
        };
        let mut fixed = fixed;
        fixed.owner = id;
        if !image.0.is_null() && image.1 > 0 {
            // SAFETY: this function's contract: `image` is a read-only mapping
            // of `image.1` bytes that stays mapped, unchanged, for the whole
            // run (the program's read-only data), so it is `'static`. The
            // texts read from it are checked as UTF-8 once more where they
            // are taken (`native_values::Kept`).
            fixed.image = unsafe { core::slice::from_raw_parts(image.0, image.1) };
        }
        let mut park = park;
        if let Some(park) = park.as_mut() {
            // The image as the declarations are kept against.
            park.fixed = fixed;
        }
        let registered = self.register_native_fixed(host, depth, report, fixed, park);
        if registered.is_err() {
            linked.remove(id);
        }
        registered
    }
}

#[cfg(all(test, target_arch = "x86_64", target_endian = "little"))]
#[path = "linked_install_tests.rs"]
mod tests;
