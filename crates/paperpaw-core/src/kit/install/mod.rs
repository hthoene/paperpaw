//! Installing, updating and removing data plugins from the internet, for
//! stores and browsers of fonts and plugins (ADR 0023).
//!
//! A [`Package`] names a plugin's files, each with its address, size and
//! digest, as a plugin library lists them (a code plugin's code is one
//! more file). An `Installer` downloads them one after another into a
//! folder PaperPaw does not load (`plugins/.<id>.part`) and checks each by
//! its size and digest. Once all are there the job is complete: the owner
//! looks at what came if it wants (a code plugin's author, see
//! [`native::author`](paperpaw_sdk::native::author)) and has the kernel install it
//! (`Installer::install`, `Command::InstallPlugin`): in place of every
//! version of it before, which stays until the new one is all there, and
//! with what was chosen from it still chosen. Nothing is unpacked and
//! nothing is held whole in memory: each file goes from the network
//! straight to the storage.
//!
//! What is left to do is kept in the plugin's own folder, so a download
//! goes on while none of its screens is open: the plugin hears the
//! answers with `Items::on` and hands them to
//! `Installer::received`.

pub mod local;
pub mod marks;
mod origin;

pub use origin::{Origin, Origins, folder_release, folder_version, origin};
pub use origin::{adopt, forget_store_install, remember_store_install};

pub use paperpaw_sdk::install::Digest;
pub use paperpaw_sdk::install::MAX_FILES;
pub use paperpaw_sdk::install::Package;
pub use paperpaw_sdk::install::PackageFile;
pub use paperpaw_sdk::install::is_plugin_name;
pub use paperpaw_sdk::install::is_safe_path;
use paperpaw_sdk::{DATA_FORMATS, SDK_VERSION, Version};

/// Whether a data plugin made for the SDK `sdk` (e.g. `0.5`) and the data
/// formats `formats` is read by a PaperPaw of the SDK this is built with
/// ([`SDK_VERSION`]); `fits_in` asks the PaperPaw that runs.
pub fn fits(sdk: &str, formats: u32) -> bool {
    fits_sdk(SDK_VERSION, sdk, formats)
}

/// `fits_in` for a kernel of the SDK `kernel`, where the `Place` is not at
/// hand.
pub fn fits_sdk(kernel: Version, sdk: &str, formats: u32) -> bool {
    let sdk_fits = sdk.trim().is_empty()
        || Version::parse(sdk).is_some_and(|v| (v.major, v.minor) <= (kernel.major, kernel.minor));
    sdk_fits && formats <= DATA_FORMATS
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    fn file(path: &str) -> PackageFile {
        PackageFile::new(
            path.into(),
            format!("https://x/{path}"),
            3,
            Digest::Sha1("a".repeat(40)),
        )
    }

    #[test]
    fn paths_stay_in_the_folder() {
        assert!(is_safe_path("plugin.ini"));
        assert!(is_safe_path("i18n/de.ini"));
        for bad in [
            "", "../x", "a/../b", "/abs", "a//b", ".hidden", "a/.b", "a\\b", "a b",
        ] {
            assert!(!is_safe_path(bad), "{bad}");
        }
        assert!(is_plugin_name("airy-theme") && is_plugin_name("core-fonts"));
        for bad in ["", "Airy", "a_b", "../x", "-x", "a.b", "core", "core.evil"] {
            assert!(!is_plugin_name(bad), "{bad}");
        }
    }

    #[test]
    fn packages_are_checked() {
        let mut package = Package::new("airy".into(), "Airy".into());
        package.files = alloc::vec![file("plugin.ini"), file("i18n/de.ini")];
        assert!(package.is_valid());
        assert_eq!(package.size(), 6);
        package.files.push(file("i18n/de.ini"));
        assert!(!package.is_valid(), "a path twice");
        package.files.pop();
        package.files[1].digest = Digest::Sha1("xyz".into());
        assert!(!package.is_valid(), "no SHA-1");
        package.files.remove(1);
        package.files[0].path = "theme.ini".into();
        assert!(!package.is_valid(), "no plugin.ini");
    }

    #[test]
    fn what_fits() {
        assert!(fits("", 1));
        assert!(fits("0.3", 1));
        assert!(fits(
            &format!("{}.{}", SDK_VERSION.major, SDK_VERSION.minor),
            DATA_FORMATS
        ));
        assert!(!fits(
            &format!("{}.{}", SDK_VERSION.major, SDK_VERSION.minor + 1),
            1
        ));
        assert!(!fits("1.0", 1));
        assert!(!fits("0.3", DATA_FORMATS + 1));
        assert!(!fits("new", 1));
        // A later kernel takes what its SDK reads.
        assert!(fits_sdk(Version::new(1, 2, 0), "1.0", 1));
    }

    /// A file a library gives both digests of is checked by its SHA-256,
    /// whichever comes first; one with only a SHA-1 by that.
    #[test]
    fn sha256_is_taken_before_sha1() {
        let (sha1, sha256) = ("a".repeat(40), "B".repeat(64));
        for keys in [["sha1", "sha256"], ["sha256", "sha1"]] {
            let mut digest = Digest::default();
            for key in keys {
                let hex = if key == "sha1" { &sha1 } else { &sha256 };
                assert!(digest.take(key, hex));
            }
            assert_eq!(digest, Digest::Sha256("b".repeat(64)));
        }
        let mut digest = Digest::default();
        assert!(digest.take("sha1", &sha1));
        assert!(!digest.take("md5", "00"));
        assert_eq!(digest, Digest::Sha1(sha1));
    }
}
