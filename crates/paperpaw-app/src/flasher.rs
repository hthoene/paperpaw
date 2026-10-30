//! Plugins chosen from the web flasher's store catalogue, copied and
//! hash-checked by the shell: checked here and marked for the kernel to
//! install. Marks take effect only after the folder is in place.

use paperpaw_core::sdk as paperpaw_sdk;
use paperpaw_sdk::install::marks::{self, From, Marks};
use paperpaw_sdk::native::{self, Author};
use paperpaw_sdk::{Storage, Version};

/// Why a copied plugin cannot be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// Its manifest, version or code does not hold.
    Damaged,
    /// Its code is signed by another author than the installed version.
    OtherAuthor,
    /// Its installation marks could not be kept. Retry on the next start.
    Storage,
}

/// Prepares a verified web bundle's plugin for installation. Data plugins
/// and signed code from the catalogue keep store updates. An unsigned
/// file from one's own flash build stays local. This does not change the
/// installed version's origin, author or code until the kernel places it.
pub fn prepare(
    storage: &dyn Storage,
    plugin: &str,
    folder: &str,
    version: &str,
) -> Result<(), Problem> {
    let version = Version::parse(version).ok_or(Problem::Damaged)?;
    if paperpaw_sdk::install::folder_version(storage, folder) != Some(version) {
        return Err(Problem::Damaged);
    }
    let mut marks = Marks::new(From::Store(version));
    let code = alloc::format!("{folder}/code.ppn");
    if storage.exists(&code) {
        let (header, signature) =
            native::check_file(storage, &code).map_err(|_| Problem::Damaged)?;
        marks = marks.code(header.sha256);
        match signature {
            Some(signature) => match native::author(storage, plugin, &signature.key) {
                Author::Known => {}
                Author::New => marks = marks.author(signature.key),
                _ => return Err(Problem::OtherAuthor),
            },
            None if native::known_author(storage, plugin).is_none() => marks.from = From::Local,
            None => return Err(Problem::OtherAuthor),
        }
    }
    marks::write(storage, folder, &marks).map_err(|_| Problem::Storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_core::Kernel;
    use paperpaw_sdk::install::{Origin, origin};
    use paperpaw_sdk::{Id, MemoryStorage};

    const DEVICE: paperpaw_platform::DeviceInfo = paperpaw_platform::DeviceInfo::new(
        "test",
        "Test",
        paperpaw_platform::DisplayInfo::new(
            480,
            800,
            220,
            paperpaw_platform::PixelFormat::Mono1,
            paperpaw_platform::Rotation::Deg0,
            None,
        ),
        paperpaw_platform::InputCaps::new(true, false, false, &[]),
        paperpaw_platform::MemoryClass::Small,
    );

    #[test]
    fn catalogue_plugin_becomes_store_owned_only_after_placement() {
        let storage = MemoryStorage::new().with(
            "plugins/.dusk.part/plugin.ini",
            "[plugin]\nid = dusk\nversion = 1.2.0-beta.2\n",
        );
        prepare(&storage, "dusk", "plugins/.dusk.part", "1.2.0-beta.2").unwrap();
        let version = Version::parse("1.2.0-beta.2").unwrap();
        assert_eq!(origin(&storage, "dusk", version), Origin::Local);
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .install_plugin(Id::new_static("dusk"), "plugins/.dusk.part")
            .build()
            .unwrap();
        assert_eq!(origin(kernel.storage(), "dusk", version), Origin::Store);
        assert!(!kernel.storage().exists("plugins/dusk/.install-marks"));
        assert_eq!(
            paperpaw_sdk::install::Origins::read(kernel.storage()).release("dusk", version),
            Some("1.2.0-beta.2")
        );
    }

    #[test]
    fn wrong_bundle_version_or_damaged_code_cannot_change_the_installed_origin() {
        let storage = MemoryStorage::new().with(
            "plugins/.dusk.part/plugin.ini",
            "[plugin]\nid = dusk\nversion = 1.0.0\n",
        );
        assert_eq!(
            prepare(&storage, "dusk", "plugins/.dusk.part", "2.0.0"),
            Err(Problem::Damaged)
        );
        assert!(!storage.exists("plugins/.dusk.part/.install-marks"));
        storage
            .write("plugins/.dusk.part/code.ppn", b"broken")
            .unwrap();
        assert_eq!(
            prepare(&storage, "dusk", "plugins/.dusk.part", "1.0.0"),
            Err(Problem::Damaged)
        );
        assert_eq!(
            origin(&storage, "dusk", Version::new(1, 0, 0)),
            Origin::Local
        );
    }

    #[test]
    fn a_later_local_install_remains_local() {
        let storage = MemoryStorage::new().with(
            "plugins/dusk/plugin.ini",
            "[plugin]\nid = dusk\nversion = 1.0.0\n",
        );
        let version = Version::new(1, 0, 0);
        paperpaw_sdk::install::remember_store_install(&storage, "dusk", version);
        marks::write(&storage, "plugins/dusk", &Marks::new(From::Local)).unwrap();
        marks::settle(&storage, "dusk", "plugins/dusk");
        assert_eq!(origin(&storage, "dusk", version), Origin::Local);
    }

    #[test]
    fn refused_kernel_placement_does_not_settle_store_marks() {
        let storage = MemoryStorage::new().with(
            "plugins/.dusk.part/plugin.ini",
            "[plugin]\nid = another-plugin\nversion = 1.0.0\n",
        );
        prepare(&storage, "dusk", "plugins/.dusk.part", "1.0.0").unwrap();
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .install_plugin(Id::new_static("dusk"), "plugins/.dusk.part")
            .build()
            .unwrap();
        assert_eq!(
            origin(kernel.storage(), "dusk", Version::new(1, 0, 0)),
            Origin::Local
        );
        assert!(!kernel.storage().exists("plugins/dusk/plugin.ini"));
    }
}
