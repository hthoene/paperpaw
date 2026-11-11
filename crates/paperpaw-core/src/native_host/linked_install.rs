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
    /// `image` is a read-only mapping of that many bytes which stays mapped for
    /// the whole run and holds the plugin's declaration bytes (the program's
    /// read-only data): the declarations are then kept as offsets into it and
    /// not copied, as for a loaded plugin. A null pointer with zero length gives
    /// none (the declarations are copied, under the smaller owned-copy limits).
    /// `fixed` is the plugin's table of settings kept in the program (empty for
    /// none): a setting it declares that equals an entry of the table is kept
    /// as that entry (Beta 3's way, about 4 bytes of heap) and not as a copy;
    /// any other is copied as before.
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
        image: (*const u8, usize),
        fixed: &'static [paperpaw_sdk::SettingSpec],
    ) -> Result<(), shell::Error> {
        // Before anything is built and long before `make`: the host's own id
        // (reserved, ui.md 4.3), an id that is not a plugin id (1 to 40 bytes
        // of `[a-z0-9-]`, as for a loaded plugin), or one that is taken.
        if id == crate::RESERVED_PLUGIN_ID {
            return Err(shell::Error::Status(ERR_DENIED));
        }
        if !bundle::is_plugin_id(id)
            || Id::try_parse(id).is_err()
            || self.shared.native_instances.find(id).is_some()
        {
            return Err(shell::Error::Status(ERR_INVALID));
        }
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
        let host = match unsafe { admitted.hosted(services, id, teardown, resources, image) } {
            Ok(host) => host,
            Err(error) => {
                linked.remove(id);
                return Err(error);
            }
        };
        let registered = self.register_native_fixed(host, depth, report, fixed);
        if registered.is_err() {
            linked.remove(id);
        }
        registered
    }
}

#[cfg(all(test, target_arch = "x86_64", target_endian = "little"))]
#[path = "linked_install_tests.rs"]
mod tests;
