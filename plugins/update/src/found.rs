//! The update files on the card: the newest one for this device.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::firmware::{
    Firmware, ForeignImage, InstallFailure, UPDATE_DIR, Update, compare_versions,
};
use paperpaw_sdk::{Storage, files};

/// The newest update for this device in [`UPDATE_DIR`]; without one, why
/// the files there do not fit (or [`InstallFailure::Unreadable`] if there
/// are none).
pub(crate) fn find(storage: &dyn Storage, firmware: &Firmware) -> Result<Update, InstallFailure> {
    let mut paths: Vec<String> = Vec::new();
    let _ = storage.list(UPDATE_DIR, &mut |entry| {
        if !entry.is_dir && !entry.name.starts_with('.') && files::has_extension(entry.name, "bin")
        {
            paths.push(format!("{UPDATE_DIR}/{}", entry.name));
        }
    });
    let mut best: Option<Update> = None;
    let mut failure = InstallFailure::Unreadable;
    for path in paths {
        match Update::read(storage, &path).and_then(|u| u.fits(firmware).map(|()| u)) {
            Ok(update) => {
                let newer = best.as_ref().is_none_or(|b| {
                    compare_versions(&update.info.version, &b.info.version).is_gt()
                });
                if newer {
                    best = Some(update);
                }
            }
            Err(why) => failure = why,
        }
    }
    best.ok_or(failure)
}

/// Another firmware's image for this device, in [`UPDATE_DIR`] or at the
/// card's top (where CrossPoint's installer looks too), the first by
/// name: no update file, and not PaperPaw itself (whose install file
/// lies there after coming from CrossPoint). Never offered: the page
/// shows it when the reader opens it.
pub(crate) fn foreign(storage: &dyn Storage, firmware: &Firmware) -> Option<ForeignImage> {
    let mut paths: Vec<String> = Vec::new();
    for dir in [UPDATE_DIR, ""] {
        let _ = storage.list(dir, &mut |entry| {
            if !entry.is_dir
                && !entry.name.starts_with('.')
                && files::has_extension(entry.name, "bin")
            {
                paths.push(if dir.is_empty() {
                    String::from(entry.name)
                } else {
                    format!("{dir}/{}", entry.name)
                });
            }
        });
    }
    paperpaw_sdk::sort::sort(&mut paths);
    paths.into_iter().find_map(|path| {
        if Update::read(storage, &path).is_ok() {
            return None;
        }
        let image = ForeignImage::read(storage, &path).ok()?;
        (image.info.project != firmware.project && image.fits(firmware).is_ok()).then_some(image)
    })
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::firmware::{Firmware, InstallFailure};

    use super::find;

    const CHIP: u16 = 5;

    fn firmware() -> Firmware {
        Firmware::new("0.3.0", "paperpaw-xteink", CHIP).room(1_000_000)
    }

    /// A signed update file of `version` for `project`.
    fn update(project: &[u8], version: &[u8]) -> Vec<u8> {
        let mut image = alloc::vec![0u8; 2000];
        image[0] = 0xE9;
        image[12..14].copy_from_slice(&CHIP.to_le_bytes());
        image[32..36].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
        image[48..48 + version.len()].copy_from_slice(version);
        image[80..80 + project.len()].copy_from_slice(project);
        let trailer = paperpaw_update::sign(&image, &[9; 32]).unwrap();
        image.extend_from_slice(&trailer.to_bytes());
        image
    }

    #[test]
    fn the_newest_for_this_device() {
        let storage = MemoryStorage::new()
            .with("updates/old.bin", update(b"paperpaw-xteink", b"0.2.0"))
            .with("updates/new.BIN", update(b"paperpaw-xteink", b"0.10.0"))
            .with("updates/mid.bin", update(b"paperpaw-xteink", b"0.9.0"))
            .with("updates/other.bin", update(b"other", b"9.0.0"))
            .with("updates/notes.txt", b"x".to_vec());
        let found = find(&storage, &firmware()).unwrap();
        assert_eq!(found.path, "updates/new.BIN");
        assert_eq!(found.info.version, "0.10.0");
    }

    /// An app image of another firmware, not signed.
    fn other_firmware(project: &[u8]) -> Vec<u8> {
        let mut image = alloc::vec![0u8; 2000];
        image[0] = 0xE9;
        image[12..14].copy_from_slice(&CHIP.to_le_bytes());
        image[32..36].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
        image[48..53].copy_from_slice(b"1.2.3");
        image[80..80 + project.len()].copy_from_slice(project);
        image
    }

    #[test]
    fn another_firmware_on_the_card() {
        use super::foreign;
        let storage = MemoryStorage::new()
            .with("updates/new.bin", update(b"paperpaw-xteink", b"0.10.0"))
            .with("firmware.bin", other_firmware(b"crosspoint"))
            .with(
                "updates/paperpaw-x4-install.bin",
                other_firmware(b"paperpaw-xteink"),
            )
            .with("updates/a.bin", other_firmware(b"paperpaw-xteink-pro"));
        let found = foreign(&storage, &firmware()).unwrap();
        assert_eq!(found.path, "firmware.bin");
        assert_eq!(found.info.project, "crosspoint");
        // PaperPaw itself, its updates and others' builds of it are none.
        let own = MemoryStorage::new()
            .with("updates/new.bin", update(b"paperpaw-xteink", b"0.10.0"))
            .with(
                "paperpaw-x4-install.bin",
                other_firmware(b"paperpaw-xteink"),
            );
        assert_eq!(foreign(&own, &firmware()), None);
    }

    #[test]
    fn why_none_fits() {
        let empty = MemoryStorage::new();
        assert_eq!(find(&empty, &firmware()), Err(InstallFailure::Unreadable));
        let other = MemoryStorage::new().with("updates/a.bin", update(b"other", b"1.0.0"));
        assert_eq!(find(&other, &firmware()), Err(InstallFailure::OtherDevice));
        let whole = MemoryStorage::new().with("updates/a.bin", alloc::vec![0xE9; 4096]);
        assert_eq!(find(&whole, &firmware()), Err(InstallFailure::NotAnUpdate));
    }
}
