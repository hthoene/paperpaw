//! Installing, updating and removing data plugins from the internet, for
//! stores and browsers of fonts and plugins (ADR 0023).
//!
//! A [`Package`] names a plugin's files, each with its address, size and
//! digest, as a plugin library lists them (a code plugin's code is one
//! more file). An [`Installer`] downloads them one after another into a
//! folder PaperPaw does not load (`plugins/.<id>.part`) and checks each by
//! its size and digest. Once all are there the job is complete: the owner
//! looks at what came if it wants (a code plugin's author, see
//! [`native::author`](crate::native::author)) and has the kernel install it
//! (`Installer::install`, [`Command::InstallPlugin`]): in place of every
//! version of it before, which stays until the new one is all there, and
//! with what was chosen from it still chosen. Nothing is unpacked and
//! nothing is held whole in memory: each file goes from the network
//! straight to the storage.
//!
//! What is left to do is kept in the plugin's own folder, so a download
//! goes on while none of its screens is open: the plugin hears the
//! answers with `Items::on` and hands them to
//! `Installer::received`.
//!
//! ```
//! use paperpaw_sdk::event::Event;
//! use paperpaw_sdk::install::{Installer, Received};
//! use paperpaw_sdk::{Command, Id};
//!
//! const INSTALLER: Installer =
//!     Installer::new(Id::new_static("my-store"), Event::new("my-store.downloaded"));
//!
//! // In `register`:
//! // r.on(INSTALLER.reply(), |env, reply, out| {
//! //     if let Received::Complete(done) = INSTALLER.received(env, reply, out) {
//! //         INSTALLER.install(env, out);
//! //         out.push(Command::Notify(format!("{} is ready", done.name)));
//! //     }
//! // })?;
//! ```

#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod local;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod marks;
mod origin;

pub use origin::{Origin, Origins, folder_release, folder_version, origin};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use origin::{adopt, forget_store_install, remember_store_install};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::{DATA_FORMATS, SDK_VERSION, Version};

/// Files of one package at most.
pub const MAX_FILES: usize = 64;
/// The longest path of a file in a plugin's folder.
const MAX_PATH: usize = 100;

/// Whether a data plugin made for the SDK `sdk` (e.g. `0.5`) and the data
/// formats `formats` is read by a PaperPaw of the SDK this is built with
/// ([`SDK_VERSION`]); `fits_in` asks the PaperPaw that runs.
pub fn fits(sdk: &str, formats: u32) -> bool {
    fits_sdk(SDK_VERSION, sdk, formats)
}

/// `fits_in` for a kernel of the SDK `kernel`, where the `Env` is not at
/// hand.
pub fn fits_sdk(kernel: Version, sdk: &str, formats: u32) -> bool {
    let sdk_fits = sdk.trim().is_empty()
        || Version::parse(sdk).is_some_and(|v| (v.major, v.minor) <= (kernel.major, kernel.minor));
    sdk_fits && formats <= DATA_FORMATS
}

/// Whether `id` can be a data plugin's id and its folder's name: lower-case
/// letters, digits and `-`, and not the host's own id `core` (reserved, as
/// `paperpaw-core`'s `RESERVED_PLUGIN_ID`).
pub fn is_plugin_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id != "core"
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `path` stays within a plugin's folder: plain names, `/`
/// between them, none starting with `.`.
pub fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        })
}

/// What a file is checked by: its digest, in lower-case hex digits.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Digest {
    /// 40 digits, as plugin libraries gave the files of a folder before
    /// they gave SHA-256 (and as jobs on the card may still have them).
    Sha1(String),
    /// 64 digits, as they give a code plugin's code and the files of a
    /// folder.
    Sha256(String),
}

impl Default for Digest {
    fn default() -> Self {
        Digest::Sha1(String::new())
    }
}

impl Digest {
    /// Whether it has the digits its kind has.
    pub fn is_valid(&self) -> bool {
        let (hex, len) = match self {
            Digest::Sha1(hex) => (hex, 40),
            Digest::Sha256(hex) => (hex, 64),
        };
        hex.len() == len && hex.bytes().all(|b| b.is_ascii_hexdigit())
    }

    /// Takes the digest a plugin library gives for a file as `key`
    /// (`sha256` or `sha1`), in whichever order: SHA-256 where it gives
    /// both. `false` for another key.
    pub fn take(&mut self, key: &str, hex: &str) -> bool {
        let hex = hex.to_ascii_lowercase();
        match key {
            "sha256" => *self = Digest::Sha256(hex),
            "sha1" if !matches!(self, Digest::Sha256(kept) if !kept.is_empty()) => {
                *self = Digest::Sha1(hex);
            }
            "sha1" => {}
            _ => return false,
        }
        true
    }

    /// As kept in a job: `sha1:…` or `sha256:…`.
    pub fn text(&self) -> String {
        match self {
            Digest::Sha1(hex) => format!("sha1:{}", hex.to_ascii_lowercase()),
            Digest::Sha256(hex) => format!("sha256:{}", hex.to_ascii_lowercase()),
        }
    }
}

/// One file of a plugin to download.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackageFile {
    /// Where it goes in the plugin's folder, e.g. `i18n/de.ini`.
    pub path: String,
    /// Where it comes from.
    pub url: String,
    pub size: u64,
    pub digest: Digest,
}

impl PackageFile {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(path: String, url: String, size: u64, digest: Digest) -> Self {
        Self {
            path,
            url,
            size,
            digest,
        }
    }
}

impl PackageFile {
    /// Whether it stays in the plugin's folder and can be checked.
    pub fn is_valid(&self) -> bool {
        is_safe_path(&self.path)
            && !self.url.is_empty()
            && !self.url.contains(['\t', '\n', '\r'])
            && self.digest.is_valid()
    }
}

/// A data plugin to install: its id (the folder it gets), a name for
/// messages, its files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Package {
    pub plugin: String,
    pub name: String,
    pub files: Vec<PackageFile>,
}

impl Package {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(plugin: String, name: String) -> Self {
        Self {
            plugin,
            name,
            files: Vec::new(),
        }
    }

    pub fn files(mut self, files: Vec<PackageFile>) -> Self {
        self.files = files;
        self
    }
}

impl Package {
    /// Whether it can be installed: a fit id, at most [`MAX_FILES`] valid
    /// files, each path once, a `plugin.ini` among them.
    pub fn is_valid(&self) -> bool {
        is_plugin_name(&self.plugin)
            && !self.files.is_empty()
            && self.files.len() <= MAX_FILES
            && self.files.iter().all(PackageFile::is_valid)
            && self.files.iter().any(|f| f.path == "plugin.ini")
            && self
                .files
                .iter()
                .enumerate()
                .all(|(i, f)| self.files[..i].iter().all(|g| g.path != f.path))
    }

    /// Bytes to download.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> PackageFile {
        PackageFile {
            path: path.into(),
            url: format!("https://x/{path}"),
            size: 3,
            digest: Digest::Sha1("a".repeat(40)),
        }
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
        let mut package = Package {
            plugin: "airy".into(),
            name: "Airy".into(),
            files: alloc::vec![file("plugin.ini"), file("i18n/de.ini")],
        };
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
