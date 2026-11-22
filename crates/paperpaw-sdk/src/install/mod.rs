//! Installing, updating and removing data plugins from the internet, for
//! stores and browsers of fonts and plugins (ADR 0023).
//!
//! A [`Package`] names a plugin's files, each with its address, size and
//! digest, as a plugin library lists them (a code plugin's code is one
//! more file). An `Installer` downloads them one after another into a
//! folder PaperPaw does not load (`plugins/.<id>.part`) and checks each by
//! its size and digest. Once all are there the job is complete: the owner
//! looks at what came if it wants (a code plugin's author, see
//! [`native::author`](crate::native::author)) and has the kernel install it
//! (`Installer::install`, `Ask::InstallPlugin`): in place of every
//! version of it before, which stays until the new one is all there, and
//! with what was chosen from it still chosen. Nothing is unpacked and
//! nothing is held whole in memory: each file goes from the network
//! straight to the storage.
//!
//! What is left to do is kept in the plugin's own folder, so a download
//! goes on while none of its screens is open: the plugin hears the
//! answers with `Staging::on` and hands them to
//! `Installer::received`.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Files of one package at most.
pub const MAX_FILES: usize = 64;
/// The longest path of a file in a plugin's folder.
const MAX_PATH: usize = 100;

/// Whether a data plugin made for the SDK `sdk` (e.g. `0.5`) with data
/// formats `formats` is read by a kernel of the SDK `kernel`.
pub fn fits_sdk(kernel: crate::Version, sdk: &str, formats: u32) -> bool {
    let sdk_fits = sdk.trim().is_empty()
        || crate::Version::parse(sdk)
            .is_some_and(|v| (v.major, v.minor) <= (kernel.major, kernel.minor));
    sdk_fits && formats <= crate::DATA_FORMATS
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
