//! The folders of the running plugins installed on the card, for `core.files`
//! root 1 and `image_prepare`: a data plugin holds published files only (no
//! code, no secrets), and settings name its pictures and folders
//! (`data-files.md`: `[core] logo = plugins/<folder>/logo.png`), so root 1
//! shows `plugins/<folder>/…` read only while the plugin runs (`files.md`
//! 1.1); a running code plugin's install folder the same way, its code and
//! the host's files in it hidden ([`host_file`]; sleep's preset pictures).
//!
//! The kernel keeps this copy up to date whenever the set of plugins changes
//! ([`crate::Kernel::data_plugins_changed`]): a plugin that is switched off,
//! removed or failed is not in it, nor one linked into the firmware (it has
//! no folder). Allocation is fallible and a
//! set in use answers "no" (the safe side: the folder stays hidden); a rebuild
//! that runs out of memory clears it.
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

    /// Forgets every folder (the safe side: all of them are hidden) and asks
    /// the next sync to rebuild.
    pub(crate) fn clear(&self) {
        if let Ok(mut names) = self.names.try_borrow_mut() {
            *names = Vec::new();
            self.seen.set(0);
        }
    }

    /// Replaces the set with `names`, made for registry generation `seen`.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` (the set is kept as it was); `ERR_BUSY` (in use).
    pub(crate) fn replace<'a>(
        &self,
        names: impl Iterator<Item = &'a str> + Clone,
        seen: u32,
    ) -> Result<(), i32> {
        // Counted first (the iterator is cheap to run twice), so the list
        // takes exactly its room, once.
        let mut next: Vec<String> = Vec::new();
        next.try_reserve_exact(names.clone().count())
            .map_err(|_| ERR_NO_MEMORY)?;
        for name in names {
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
    /// removed, failed or updated). Cheap otherwise: one compare. A rebuild
    /// that runs out of memory clears the set and the next call tries again;
    /// a busy set is kept as it was.
    pub(crate) fn native_data_plugins_sync(&self) {
        let Some(hosts) = self.shared.native_instances.hosts() else {
            return;
        };
        let folders = hosts.data_plugins();
        let generation = self.shared.registry_generation.get();
        if folders.seen.get() == generation {
            return;
        }
        let running =
            |id: &paperpaw_sdk::Id| self.shared.plugins.get(id.as_str()).is_some_and(shown);
        let names = self.shared.data_folders.iter().filter_map(|(id, folder)| {
            // Only `plugins/<name>`: a folder anywhere else is not shown.
            let name = folder.strip_prefix("plugins/")?;
            (running(id) && !name.is_empty() && !name.contains('/')).then_some(name)
        });
        // A rebuild that cannot be made (no memory) leaves no folder shown, never
        // a plugin that was switched off or removed meanwhile; the next pass
        // tries again. Busy keeps the set for that retry.
        if folders.replace(names, generation) == Err(ERR_NO_MEMORY) {
            folders.clear();
        }
    }
}

/// Whether root 1 shows the install folder of `info`: a data or code plugin
/// on the card that runs (`files.md` 1.1). One built in has no folder.
fn shown(info: &paperpaw_sdk::PluginInfo) -> bool {
    matches!(
        info.kind,
        paperpaw_sdk::PluginKind::Data | paperpaw_sdk::PluginKind::Code
    ) && info.state == paperpaw_sdk::PluginState::Running
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

/// A root-1 path below a plugin's folder: the folder (`plugins/<name>`) and
/// the entry right below it; `None` for the folder itself or another path.
pub(crate) fn below_folder(path: &str) -> Option<(&str, &str)> {
    let name = plugin_folder_of(path)?;
    let folder_len = path.find('/')? + 1 + name.len();
    let entry = path.get(folder_len + 1..)?.split('/').next()?;
    (!entry.is_empty()).then(|| (&path[..folder_len], entry))
}

/// The code and the host's files in a plugin's install folder: root 1 never
/// shows them (`files.md` 1.1).
fn host_named(name: &str) -> bool {
    paperpaw_sdk::native::CODE_FILES
        .iter()
        .copied()
        .chain([paperpaw_sdk::install::marks::FILE])
        .any(|hidden| super::files_service::same_fold(name, hidden))
}

/// Whether `entry`, right below the plugin folder `folder`, is its code or
/// one of the host's files: by its spelling, or as an 8.3 alias (a name with
/// a `~`) where it and one of them both exist, so it may be that file.
/// Over-hides where it cannot tell.
pub(crate) fn host_file(
    storage: &dyn paperpaw_platform::Storage,
    folder: &str,
    entry: &str,
) -> bool {
    use super::storage_files::exists_in;
    host_named(entry)
        || (entry.contains('~')
            && exists_in(storage, folder, entry)
            && paperpaw_sdk::native::CODE_FILES
                .iter()
                .copied()
                .chain([paperpaw_sdk::install::marks::FILE])
                .any(|hidden| exists_in(storage, folder, hidden)))
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
            .with("plugins/moles/plugin.ini", "[plugin]\nname = Moles\n")
            .with("plugins/quiet/plugin.ini", "[plugin]\nname = Quiet\n");
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .storage(storage)
            .user_config("[plugins]\nquiet = off\n")
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
                set.contains("quiet"),
            )
        };
        // `quiet` is switched off in the configuration: listed, not running, hidden.
        assert_eq!(folders(&kernel), (true, true, false, false));
        // A plugin that is removed is no longer shown, the other stays.
        kernel.unload_plugin(&Id::try_parse("owls").unwrap());
        kernel.native_data_plugins_sync();
        assert_eq!(folders(&kernel), (false, true, false, false));
    }

    #[test]
    fn a_running_code_plugins_folder_is_shown_and_its_code_never() {
        use paperpaw_sdk::{
            Id, Manifest, ManifestRef, PluginInfo, PluginKind, PluginState, RequiredFor, Version,
        };
        let info = |kind, state| {
            let manifest = Manifest::new(Id::new_static("sleepy"), "Sleepy", Version::new(1, 0, 0));
            let manifest = ManifestRef::try_new(manifest, None).unwrap();
            PluginInfo::new(
                manifest,
                state,
                true,
                RequiredFor::default(),
                kind,
                None,
                None,
            )
        };
        assert!(shown(&info(PluginKind::Code, PluginState::Running)));
        assert!(shown(&info(PluginKind::Data, PluginState::Running)));
        assert!(!shown(&info(PluginKind::Code, PluginState::Off)));
        assert!(!shown(&info(PluginKind::BuiltIn, PluginState::Running)));
        // Right below the folder: its code and the host's files are hidden.
        let card = paperpaw_sdk::MemoryStorage::new();
        assert_eq!(
            below_folder("plugins/sleepy/code.ppn"),
            Some(("plugins/sleepy", "code.ppn"))
        );
        assert_eq!(
            below_folder("plugins/sleepy/art/a.png"),
            Some(("plugins/sleepy", "art"))
        );
        assert_eq!(below_folder("plugins/sleepy"), None);
        assert_eq!(below_folder("plugins/sleepy/"), None);
        for hidden in ["code.ppn", "CODE.PPN", ".install-marks", "code.so"] {
            assert!(host_file(&card, "plugins/sleepy", hidden), "{hidden}");
        }
        assert!(!host_file(&card, "plugins/sleepy", "owl.png"));
        // An 8.3 alias where the code is there may be the code: hidden.
        let card = card
            .with("plugins/sleepy/code.ppn", "x")
            .with("plugins/sleepy/CODE~1.PPN", "x");
        assert!(host_file(&card, "plugins/sleepy", "CODE~1.PPN"));
    }

    #[test]
    fn a_cleared_set_shows_nothing_and_asks_for_a_rebuild() {
        let set = DataPluginFolders::default();
        set.replace(["owls"].into_iter(), 7).unwrap();
        set.clear();
        assert!(!set.contains("owls"));
        assert_eq!(set.seen.get(), 0, "the next sync rebuilds");
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
