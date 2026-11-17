//! Where an installed plugin came from: a store, or any other way (the
//! Wi-Fi transfer, a memory card, USB, one's own build). A store updates
//! only what it installed itself, so a plugin of one's own by the same id
//! is never replaced by surprise.
//!
//! The versions a store installed are kept beside the code's names
//! ([`native::installed_code`](crate::native::installed_code)), where
//! neither the Wi-Fi transfer nor plugins' folders reach: a plugin counts
//! as the store's only while its version is one of those. One put in
//! place otherwise counts as local even before its installer said so
//! ([`forget_store_install`]), as long as its version differs. On a
//! device that kept none yet, the plugins installed are taken for the
//! store's once ([`adopt`]): those it installed before keep its updates.















#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

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
