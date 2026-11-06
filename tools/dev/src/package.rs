//! Packaging: a data plugin folder checked by the rules of the device's
//! installer, the kernel's loader and the plugin library at paperpaw.org,
//! and compatibility with a PaperPaw release (`--release`).
//!
//! A data plugin is looked at twice:
//!
//! - **As files** ([`folder`]): `plugin.ini` (id, name, version,
//!   `formats`, `[extends]`, `[family]`), every file's path, kind and size
//!   against the SDK's `install` rules (the installer downloads a
//!   [`Package`](paperpaw_sdk::install::Package) of at most
//!   [`MAX_FILES`](paperpaw_sdk::install::MAX_FILES) files with safe
//!   paths) and the website's (the kinds of files a package may hold, its
//!   size, an id and a version named in `plugin.ini`), and which files
//!   PaperPaw reads at all.
//! - **On the device** ([`device`]): installed into an emulated device of
//!   each profile the way the store installs it
//!   (`Command::InstallPlugin` from `plugins/.<id>.part`), so the kernel's
//!   own loader says what it refuses or leaves out; then what it set is
//!   compared with what the app has: settings that exist and values they
//!   take, presets of kinds some screen offers, key bindings to actions
//!   that exist, fonts to extend that exist.
//!
//! Whatever the website or the device would refuse is an error.
//!
//! Plugins with code named with `--plugin` are looked at for their SDK
//! version against the release. `.ppn` files (native code plugins, ADR
//! 0024) are checked as the device checks them (see [`crate::ppn`]);
//! `--release` takes a release's fingerprint for them.

pub mod device;
pub mod folder;
pub mod source;

use paperpaw_sdk::{DATA_FORMATS, SDK_VERSION, Version};

use crate::cli::Context;
use crate::report::{Finding, Report};

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let release = match context.release.as_deref() {
        Some(text) => match Release::parse(text) {
            Some(release) => Some(release),
            // For `.ppn` files (see `ppn`).
            None if paperpaw_sdk::native::Fingerprint::parse(text).is_some() => None,
            None => {
                report.add(Finding::error(
                    "package.release",
                    text,
                    "`--release` takes a PaperPaw version such as 0.4.0, or a release's                      fingerprint for .ppn files.",
                ));
                None
            }
        },
        None => None,
    };
    // A code plugin's folder, named with it: its version and its code's.
    let mut folders = Vec::new();
    for plugin in &context.target.data {
        let files = folder::check(plugin, report);
        if let Some(release) = &release {
            data_release(plugin, files.as_ref(), release, report);
        }
        let version = files.as_ref().and_then(|f| f.manifest.version.as_deref());
        if let Some(version) = version.and_then(|v| Version::parse(v.trim()))
            && context.target.plugins.contains(&plugin.id)
        {
            folders.push((plugin.id.clone(), version));
        }
        device::check(context, plugin, files.as_ref(), report)?;
    }
    if !folders.is_empty() {
        device::code_versions(context, &folders, report)?;
    }
    let code: Vec<&String> = context
        .target
        .plugins
        .iter()
        .filter(|id| !context.target.data.iter().any(|d| &&d.id == id))
        .collect();
    if !code.is_empty() {
        device::code_plugins(context, &code, release.as_ref(), report)?;
    }
    for source in &context.target.sources {
        source::check(source, report);
    }
    crate::ppn::check(context, report)?;
    crate::library::check(context, report)
}

/// A PaperPaw release asked for with `--release`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
}

impl Release {
    /// `0.4.0`, `v0.4.0` or `0.4.0-dev` (the pre-release of 0.4.0).
    pub fn parse(text: &str) -> Option<Release> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = text.split_once('-').map_or(text, |(v, _)| v);
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            return None;
        }
        Version::parse(text).map(|version| Release { version })
    }

    /// Its SDK: a release's SDK has its version.
    pub fn sdk(&self) -> Version {
        self.version
    }

    /// Whether it is of the SDK line this tool is built with.
    pub fn is_this_line(&self) -> bool {
        (self.version.major, self.version.minor) == (SDK_VERSION.major, SDK_VERSION.minor)
    }

    /// Whether it is later than the SDK this tool is built with.
    pub fn is_later(&self) -> bool {
        (self.version.major, self.version.minor) > (SDK_VERSION.major, SDK_VERSION.minor)
    }

    /// The version of the data formats it reads, where known: 1 before
    /// 0.4 (which came with `formats`; before, the key was passed over and
    /// the files read as format 1), this tool's own for its SDK line;
    /// unknown for later releases (formats only grow, so a plugin this
    /// tool passes is read there too).
    pub fn data_formats(&self) -> Option<u32> {
        if self.is_later() {
            None
        } else if self.is_this_line() {
            Some(DATA_FORMATS)
        } else {
            Some(1)
        }
    }

    /// Whether it is earlier than 0.4, which brought `formats`, presets,
    /// packs and families in data plugins, and the store.
    pub fn before_0_4(&self) -> bool {
        (self.version.major, self.version.minor) < (0, 4)
    }

    /// Whether it has a store that installs data plugins (ADR 0023).
    pub fn has_store(&self) -> bool {
        !self.before_0_4()
    }
}

/// A data plugin against `release`: its `formats`, and the SDK its
/// catalogue entry may name so that the release's store lists it.
fn data_release(
    plugin: &crate::target::DataPlugin,
    files: Option<&folder::Folder>,
    release: &Release,
    report: &mut Report,
) {
    let place = "plugin.ini";
    let id = plugin.id.as_str();
    let formats = files.and_then(|f| f.manifest.formats).unwrap_or(1);
    let line = format!("{}.{}", release.version.major, release.version.minor);
    match release.data_formats() {
        Some(reads) if formats > reads => report.add(
            Finding::error(
                "package.release",
                place,
                format!(
                    "Made for data formats {formats}, but PaperPaw {line} reads {reads}: it leaves the plugin out."
                ),
            )
            .plugin(id),
        ),
        Some(_) if release.has_store() => report.add(
            Finding::info(
                "package.release",
                place,
                format!(
                    "PaperPaw {line} reads it (data formats {formats}); give `sdk = \"{line}\"` or lower in its catalogue entry so that the store there lists it."
                ),
            )
            .plugin(id),
        ),
        Some(_) => report.add(
            Finding::info(
                "package.release",
                place,
                format!(
                    "PaperPaw {line} reads it (data formats {formats}) from a folder copied into `plugins`; it has no store to install it from."
                ),
            )
            .plugin(id),
        ),
        None => report.add(
            Finding::info(
                "package.release",
                place,
                format!(
                    "PaperPaw {line} is later than this paperpaw-dev (SDK {}.{}); data formats only grow, so it reads the plugin, but a paperpaw-dev of that release checks it better.",
                    SDK_VERSION.major, SDK_VERSION.minor
                ),
            )
            .plugin(id),
        ),
    }
    // What data plugins could hold before 0.4 (`data_plugin.rs` of
    // v0.3.0): no presets, no packs that extend fonts, no families.
    let Some(files) = files else {
        return;
    };
    if !release.before_0_4() {
        return;
    }
    let mut missing = |what: &str, place: &str| {
        report.add(
            Finding::error(
                "package.release",
                place,
                format!("PaperPaw {line} does not read {what} in data plugins (it came with 0.4), so the plugin does not do this there."),
            )
            .plugin(id),
        );
    };
    if files.has_presets {
        missing("presets.ini", "presets.ini");
    }
    if !files.manifest.extends.is_empty() {
        missing("[extends]", "plugin.ini");
    }
    if !files.manifest.family.is_empty() {
        missing("[family]", "plugin.ini");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_are_read() {
        assert_eq!(
            Release::parse("v0.4.0").unwrap().version,
            Version::new(0, 4, 0)
        );
        assert_eq!(
            Release::parse("0.4.0-dev").unwrap().version,
            Version::new(0, 4, 0)
        );
        assert!(Release::parse("abcdef0123").is_none());
        assert!(Release::parse("").is_none());
        let old = Release::parse("0.3.0").unwrap();
        assert_eq!(old.data_formats(), Some(1));
        let this = Release {
            version: SDK_VERSION,
        };
        assert_eq!(this.data_formats(), Some(DATA_FORMATS));
        let later = Release {
            version: Version::new(SDK_VERSION.major, SDK_VERSION.minor + 1, 0),
        };
        assert_eq!(later.data_formats(), None);
    }
}
