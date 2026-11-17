//! Where an installed plugin came from: a store, or any other way (the
//! Wi-Fi transfer, a memory card, USB, one's own build). A store updates
//! only what it installed itself, so a plugin of one's own by the same id
//! is never replaced by surprise.
//!
//! The versions a store installed are kept beside the code's names
//! ([`native::installed_code`](paperpaw_sdk::native::installed_code)), where
//! neither the Wi-Fi transfer nor plugins' folders reach: a plugin counts
//! as the store's only while its version is one of those. One put in
//! place otherwise counts as local even before its installer said so
//! ([`forget_store_install`]), as long as its version differs. On a
//! device that kept none yet, the plugins installed are taken for the
//! store's once ([`adopt`]): those it installed before keep its updates.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;

use crate::Place;
use paperpaw_sdk::{PluginKind, Version};

/// Lines `<id>\t<version>`, the versions a store installed.
const STORE_INSTALLS: &str = paperpaw_sdk::data_path!("plugin-origin.txt");
/// Versions kept for each plugin: the one installed, and those before
/// it in case an install did not complete.
const VERSIONS_KEPT: usize = 4;
/// The longest `plugin.ini` read for its version.
const MAX_MANIFEST: usize = 16 * 1024;

/// Where an installed plugin came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Origin {
    /// Installed or updated by a store, which offers its updates.
    Store,
    /// Any other way: no store replaces it by itself.
    Local,
}

/// Where `plugin`, installed at `version`, came from.
pub fn origin(storage: &dyn Storage, plugin: &str, version: Version) -> Origin {
    Origins::read(storage).of(plugin, version)
}

/// The versions a store installed, read once: for asking about many
/// plugins in a row (a list of them) without reading each time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origins {
    /// Each plugin's versions kept: their numbers, and as written (a
    /// pre-release with them).
    kept: Vec<(String, Version, String)>,
}

impl Origins {
    pub fn read(storage: &dyn Storage) -> Self {
        let text = paperpaw_sdk::read_file(storage, STORE_INSTALLS, 64 * 1024).unwrap_or_default();
        let kept = String::from_utf8_lossy(&text)
            .lines()
            .filter_map(|line| {
                let (id, version) = line.split_once('\t')?;
                let written = version.trim();
                Some((
                    String::from(id),
                    Version::parse(written)?,
                    String::from(written),
                ))
            })
            .collect();
        Self { kept }
    }

    /// Where `plugin`, installed at `version`, came from.
    pub fn of(&self, plugin: &str, version: Version) -> Origin {
        let ours = self
            .kept
            .iter()
            .any(|(id, kept, _)| id == plugin && *kept == version);
        if ours { Origin::Store } else { Origin::Local }
    }

    /// The version of `plugin` a store installed with the numbers of
    /// `version`, as the store named it (`0.5.0-beta.2`: betas of one
    /// release share their numbers); the last such one kept.
    pub fn release(&self, plugin: &str, version: Version) -> Option<&str> {
        self.kept
            .iter()
            .rev()
            .find(|(id, kept, _)| id == plugin && *kept == version)
            .map(|(_, _, written)| written.as_str())
    }
}

/// Remembers that a store installed `plugin` at `version` (a
/// [`Version`], or the version as the store named it, pre-release
/// included); `false` if it could not be kept (the file left as it is
/// where it could not be read).
pub fn remember_store_install(
    storage: &dyn Storage,
    plugin: &str,
    version: impl core::fmt::Display,
) -> bool {
    change(storage, plugin, Some(format!("{version}")))
}

/// Forgets every version a store installed of `plugin`: an installer of
/// any other kind does, so the store no longer updates it. `false` if
/// that could not be kept.
pub fn forget_store_install(storage: &dyn Storage, plugin: &str) -> bool {
    change(storage, plugin, None)
}

/// Takes every plugin installed (with data or code, not one built in) for
/// the store's at its version, where no origin was ever kept on this
/// device: before PaperPaw kept them, the plugins the store installed
/// went unnamed, and they keep its updates. Installers and those who
/// look at origins call it first; once kept, it does nothing. `false` if
/// it could not be kept.
pub fn adopt(env: &Place<'_>) -> bool {
    if env.storage().exists(STORE_INSTALLS) {
        return true;
    }
    let text: String = env
        .plugins()
        .iter()
        .filter(|info| matches!(info.kind, PluginKind::Data | PluginKind::Code))
        .map(|info| {
            let version = info.update.unwrap_or(info.manifest.version);
            format!("{}\t{version}\n", info.manifest.id)
        })
        .collect();
    let _ = env.storage().create_dir(paperpaw_sdk::data::DIR);
    env.storage().write(STORE_INSTALLS, text.as_bytes()).is_ok()
}

/// The version `plugin.ini` in `folder` says, e.g. of a plugin about to be
/// installed.
pub fn folder_version(storage: &dyn Storage, folder: &str) -> Option<Version> {
    folder_release(storage, folder).and_then(|written| Version::parse(&written))
}

/// The same as `plugin.ini` writes it, a pre-release included
/// (`0.5.0-beta.2`), if it is a version.
pub fn folder_release(storage: &dyn Storage, folder: &str) -> Option<String> {
    let path = format!("{}/plugin.ini", folder.trim_end_matches('/'));
    let bytes = paperpaw_sdk::read_file(storage, &path, MAX_MANIFEST).ok()?;
    let source = core::str::from_utf8(&bytes).ok()?;
    paperpaw_config::parse(source)
        .filter_map(Result::ok)
        .find(|entry| entry.section == "plugin" && entry.key == "version")
        .map(|entry| String::from(entry.value.as_str().trim()))
        .filter(|written| Version::parse(written).is_some())
}

/// Keeps `version` for `plugin` (the oldest of its versions going past
/// [`VERSIONS_KEPT`]), or forgets all of its versions.
fn change(storage: &dyn Storage, plugin: &str, version: Option<String>) -> bool {
    let Ok(kept) = paperpaw_sdk::files::load_text_for_update(storage, STORE_INSTALLS, 64 * 1024)
    else {
        return false;
    };
    let kept = kept.unwrap_or_default();
    let mut others: Vec<&str> = Vec::new();
    let mut versions: Vec<String> = Vec::new();
    for line in kept.lines().filter(|line| !line.trim().is_empty()) {
        match line.split_once('\t') {
            Some((id, kept)) if id == plugin => versions.push(String::from(kept.trim())),
            _ => others.push(line),
        }
    }
    match version {
        Some(new) => {
            versions.retain(|kept| *kept != new);
            versions.push(new);
            let first = versions.len().saturating_sub(VERSIONS_KEPT);
            versions.drain(..first);
        }
        None if versions.is_empty() => return true,
        None => versions.clear(),
    }
    let mut text: String = others.iter().map(|line| format!("{line}\n")).collect();
    for kept in &versions {
        text.push_str(&format!("{plugin}\t{kept}\n"));
    }
    let _ = storage.create_dir(paperpaw_sdk::data::DIR);
    storage.write(STORE_INSTALLS, text.as_bytes()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    /// Only the versions a store installed count as the store's; another
    /// installer's forgetting makes the plugin local, and other plugins
    /// keep theirs.
    #[test]
    fn a_plugin_is_the_stores_while_its_version_is_one_it_installed() {
        let storage = MemoryStorage::new();
        assert_eq!(origin(&storage, "dark", v("1.0.0")), Origin::Local);
        assert!(remember_store_install(&storage, "dark", v("1.0.0")));
        assert!(remember_store_install(&storage, "light", v("2.0.0")));
        assert_eq!(origin(&storage, "dark", v("1.0.0")), Origin::Store);
        // Put in place otherwise, at another version: local.
        assert_eq!(origin(&storage, "dark", v("1.0.1")), Origin::Local);
        // An update that did not complete leaves the version before the
        // store's.
        assert!(remember_store_install(&storage, "dark", v("1.1.0")));
        assert_eq!(origin(&storage, "dark", v("1.0.0")), Origin::Store);
        assert!(forget_store_install(&storage, "dark"));
        assert_eq!(origin(&storage, "dark", v("1.1.0")), Origin::Local);
        assert_eq!(origin(&storage, "light", v("2.0.0")), Origin::Store);
        // Only the last few versions are kept.
        for minor in 0..10 {
            remember_store_install(&storage, "light", v(&format!("2.{minor}.0")));
        }
        assert_eq!(origin(&storage, "light", v("2.0.0")), Origin::Local);
        assert_eq!(origin(&storage, "light", v("2.9.0")), Origin::Store);
    }

    /// A record that cannot be read is not written over: it holds every
    /// other plugin's.
    #[test]
    fn an_unreadable_record_is_kept() {
        let storage = MemoryStorage::new();
        let big = alloc::vec![b'x'; 70 * 1024];
        storage.write(STORE_INSTALLS, &big).unwrap();
        assert!(!remember_store_install(&storage, "dark", v("1.0.0")));
        assert_eq!(storage.size(STORE_INSTALLS).unwrap(), big.len() as u64);
    }

    #[test]
    fn the_version_of_a_folder_is_read_from_its_manifest() {
        let storage = MemoryStorage::new();
        storage
            .write(
                "plugins/.dark.part/plugin.ini",
                b"[plugin]\nid = dark\nversion = 1.2.3\n",
            )
            .unwrap();
        assert_eq!(
            folder_version(&storage, "plugins/.dark.part/"),
            Some(v("1.2.3"))
        );
        assert_eq!(folder_version(&storage, "plugins/none"), None);
    }
}
