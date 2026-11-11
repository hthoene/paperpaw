//! The folders of the running data plugins, for `core.files` root 1 and
//! `image_prepare`: a data plugin holds published files only (no code, no
//! secrets), and settings name its pictures and folders (`data-files.md`:
//! `[core] logo = plugins/<folder>/logo.png`), so root 1 shows
//! `plugins/<folder>/…` read only while the plugin runs (`files.md` 1.1).
//!
//! The kernel keeps this copy up to date whenever the set of plugins changes
//! ([`crate::Kernel::data_plugins_changed`]): a plugin that is switched off,
//! removed, failed or a code plugin is not in it. Allocation is fallible and a
//! set in use answers "no" (the safe side: the folder stays hidden).
use alloc::{string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};

/// The folder names (below `plugins/`) of the running data plugins.
#[derive(Default)]
pub(crate) struct DataPluginFolders {
    names: RefCell<Vec<String>>,
    /// The registry generation the names were made for (0: none yet).
    seen: Cell<u32>,
}

impl core::fmt::Debug for DataPluginFolders {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DataPluginFolders")
            .field(
                "folders",
                &self.names.try_borrow().map_or(0, |all| all.len()),
            )
            .finish()
    }
}

impl DataPluginFolders {
    /// Whether `name` (one path segment) is a running data plugin's folder, as
    /// the card compares names. An alias the card would resolve to it (an
    /// 8.3 name) is not: it is refused.
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.names.try_borrow().is_ok_and(|all| {
            all.iter()
                .any(|have| super::files_service::same_fold(have, name))
        })
    }

    /// Replaces the set with `names`, made for registry generation `seen`.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (the set is kept as it was); `ERR_BUSY` (in use).
    pub(crate) fn replace<'a>(
        &self,
        names: impl Iterator<Item = &'a str>,
        seen: u32,
    ) -> Result<(), i32> {
        let mut next: Vec<String> = Vec::new();
        for name in names {
            next.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            let mut copy = String::new();
            copy.try_reserve_exact(name.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            copy.push_str(name);
            next.push(copy);
        }
        *self.names.try_borrow_mut().map_err(|_| ERR_BUSY)? = next;
        self.seen.set(seen);
        Ok(())
    }
}

impl crate::Kernel {
    /// Brings the data plugins' folders up to date with the plugins that run,
    /// when the registry changed since (a plugin loaded, switched off,
    /// removed, failed or updated). Cheap otherwise: one compare. Fails soft:
    /// a refusal leaves the set as it was and the next call tries again.
    pub(crate) fn native_data_plugins_sync(&self) {
        let Some(hosts) = self.shared.native_instances.hosts() else {
            return;
        };
        let folders = hosts.data_plugins();
        let generation = self.shared.registry_generation.get();
        if folders.seen.get() == generation {
            return;
        }
        let running = |id: &paperpaw_sdk::Id| {
            self.shared.plugins.get(id.as_str()).is_some_and(|info| {
                info.kind == paperpaw_sdk::PluginKind::Data
                    && info.state == paperpaw_sdk::PluginState::Running
            })
        };
        let names = self.shared.data_folders.iter().filter_map(|(id, folder)| {
            // Only `plugins/<name>`: a folder anywhere else is not shown.
            let name = folder.strip_prefix("plugins/")?;
            (running(id) && !name.is_empty() && !name.contains('/')).then_some(name)
        });
        folders.replace(names, generation).ok();
    }
}

/// The folder of a root-1 path below `plugins/`: `Some(name)` when the first
/// segment is `plugins` (in any case, as the card compares) and a second one
/// follows, `None` for any other path.
pub(crate) fn plugin_folder_of(path: &str) -> Option<&str> {
    let mut parts = path.split('/');
    let first = parts.next()?;
    if !super::files_service::same_fold(first, "plugins") {
        return None;
    }
    parts.next().filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_is_the_second_segment_of_a_plugins_path() {
        assert_eq!(plugin_folder_of("plugins/owls/owl.png"), Some("owls"));
        assert_eq!(plugin_folder_of("PLUGINS/owls"), Some("owls"));
        assert_eq!(plugin_folder_of("plugins"), None);
        assert_eq!(plugin_folder_of("books/plugins/owls"), None);
        assert_eq!(plugin_folder_of(".paperpaw/x"), None);
    }

    #[test]
    fn the_kernel_keeps_the_running_data_plugins_folders() {
        use paperpaw_sdk::{Id, MemoryStorage};
        let storage = MemoryStorage::new()
            .with("plugins/owls/plugin.ini", "[plugin]\nname = Owls\n")
            .with("plugins/moles/plugin.ini", "[plugin]\nname = Moles\n");
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .storage(storage)
            .build()
            .unwrap();
        // No pool yet: nothing to keep.
        kernel.native_data_plugins_sync();
        kernel
            .shared
            .native_instances
            .test_insert(Id::try_parse("probe").unwrap(), None, true)
            .unwrap();
        kernel.native_data_plugins_sync();
        let folders = |kernel: &crate::Kernel| {
            let hosts = kernel.shared.native_instances.hosts().unwrap();
            let set = hosts.data_plugins();
            (
                set.contains("owls"),
                set.contains("moles"),
                set.contains("plugins"),
            )
        };
        assert_eq!(folders(&kernel), (true, true, false));
        // A plugin that is removed is no longer shown, the other stays.
        kernel.unload_plugin(&Id::try_parse("owls").unwrap());
        kernel.native_data_plugins_sync();
        assert_eq!(folders(&kernel), (false, true, false));
    }

    #[test]
    fn it_matches_a_folder_by_fold_and_never_an_alias_or_prefix() {
        let set = DataPluginFolders::default();
        set.replace(["owls", "large-print"].into_iter(), 1).unwrap();
        assert!(set.contains("owls"));
        assert!(set.contains("OWLS"));
        assert!(!set.contains("owl"));
        assert!(!set.contains("owls2"));
        assert!(!set.contains("LARGE-~1"));
        assert!(!set.contains(""));
        set.replace(core::iter::empty(), 2).unwrap();
        assert!(!set.contains("owls"));
    }
}
