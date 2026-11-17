//! The names in a plugin's folder, listed once.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_platform::Storage;

/// The names in a plugin's folder, listed once (#305): what the folder does
/// not hold is not looked up name by name. A start looked up some 16 names
/// in every plugin folder, most of them not there, and each such lookup
/// reads the folder to its end.
pub(super) struct Names(Option<Vec<String>>);

impl Names {
    /// More names than this are not kept: every name is looked up then.
    const MOST: usize = 64;

    pub(super) fn of(storage: &dyn Storage, path: &str) -> Names {
        let mut names: Vec<String> = Vec::new();
        let mut many = false;
        let listed = storage.list(path, &mut |entry| {
            many |= names.len() == Names::MOST;
            if !many {
                names.push(entry.name.to_string());
            }
        });
        Names((listed.is_ok() && !many).then_some(names))
    }

    /// Whether `name` may be in the folder: not, only where the folder was
    /// listed without it (in any case of its letters: a card's names are
    /// found in any).
    pub(super) fn has(&self, name: &str) -> bool {
        self.0
            .as_ref()
            .is_none_or(|names| names.iter().any(|n| n.eq_ignore_ascii_case(name)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::load_folder;
    use alloc::rc::Rc;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;
    use core::cell::RefCell;
    use paperpaw_platform::Storage;
    use paperpaw_sdk::MemoryStorage;

    /// A storage that notes the paths it is asked for.
    struct Asked(MemoryStorage, RefCell<Vec<String>>);

    impl Storage for Asked {
        fn read_at(
            &self,
            path: &str,
            at: u64,
            buf: &mut [u8],
        ) -> Result<usize, paperpaw_platform::StorageError> {
            self.1.borrow_mut().push(path.to_string());
            self.0.read_at(path, at, buf)
        }
        fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
            self.1.borrow_mut().push(path.to_string());
            self.0.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.0.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.0.append(path, data)
        }
        fn list(
            &self,
            dir: &str,
            visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), paperpaw_platform::StorageError> {
            self.1.borrow_mut().push(dir.to_string());
            self.0.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
            self.0.remove(path)
        }
    }

    /// A plugin's folder is listed once, and what it does not hold is not
    /// looked up name by name (#305): a folder with a manifest and one
    /// settings file was asked for 16 names more, each a read of the folder
    /// on a card.
    #[test]
    fn a_plugin_folder_is_listed_once_and_what_it_lacks_is_not_looked_up() {
        let asked = Rc::new(Asked(
            MemoryStorage::new()
                .with("plugins/large/plugin.ini", "[plugin]\nname = Large print\n")
                .with("plugins/large/settings.ini", "[core]\nui-size = 14\n"),
            RefCell::default(),
        ));
        let storage: Rc<dyn Storage> = asked.clone();
        let large = load_folder(&storage, "plugins/large").unwrap();
        assert!(
            large
                .later
                .borrow()
                .iter()
                .any(|p| p.ends_with("settings.ini"))
        );
        assert!(!large.code && large.keymap.borrow().is_none());
        let mut paths = asked.1.borrow().clone();
        paths.sort();
        paths.dedup();
        assert_eq!(
            paths,
            [
                "plugins/large",
                "plugins/large/plugin.ini",
                "plugins/large/settings.ini"
            ]
        );
        // All of it is found where it is there, as before.
        let full = Rc::new(Asked(
            MemoryStorage::new()
                .with("plugins/full/plugin.ini", "[plugin]\nname = Full\n")
                .with("plugins/full/keymap.ini", "[keys]\n")
                .with("plugins/full/styles.css", "p { }")
                .with("plugins/full/i18n/de.ini", "[meta]\nlanguage = de\n")
                .with("plugins/full/fonts/A.ttf", "x")
                .with("plugins/full/code.ppn", "x"),
            RefCell::default(),
        ));
        let storage: Rc<dyn Storage> = full.clone();
        let plugin = load_folder(&storage, "plugins/full").unwrap();
        assert!(
            plugin.code && plugin.keymap.borrow().is_some() && plugin.styles.borrow().is_some()
        );
        assert_eq!(plugin.translations.borrow().len(), 1);
        assert_eq!(plugin.fonts.borrow().len(), 1);
    }
}
