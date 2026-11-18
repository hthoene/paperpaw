//! What installing a font takes, without the host: which files are
//! downloaded where (each saved by `core.net` to this plugin's data folder,
//! then checked against its SHA-1), and the folder the host is told to
//! install (`command.install_plugin`, root 0). The same rules as the
//! interface-6 `Package` check, with fallible allocation, and the file sizes
//! that `docs/abi/network.md` §8 audits (`max_file` 4 MiB for packs,
//! 8 MiB for font files).
use crate::native_catalog::Font;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    install::{MAX_FILES, is_safe_path},
    native::{Error, net::MAX_URL},
};
use paperpaw_ui_kit::text::concat;

/// Where the files of fonts being installed are kept, in the data folder.
pub const STAGING: &str = "staging";
/// The longest id a plugin has.
const MAX_PLUGIN_ID: usize = 40;
/// The most a pack is, and a font file. Interface 6 asked for no cap of its
/// own (the host's); the library's packs are up to about 700 KB.
pub const MAX_PACK: u64 = 4 * 1024 * 1024;
pub const MAX_OUTLINE: u64 = 8 * 1024 * 1024;

/// The `max_file` of each request on this device.
#[must_use]
pub const fn max_file(outlines: bool) -> u64 {
    if outlines { MAX_OUTLINE } else { MAX_PACK }
}

/// Whether `font` can be installed here: an id that is a plugin's, every
/// file inside the folder with a digest and a size between 1 byte and the
/// caps, a path once, `plugin.ini` among them. A font that does not is not
/// offered.
#[must_use]
pub fn fits(font: &Font, outlines: bool) -> bool {
    let max_file = max_file(outlines);
    let files = &font.files;
    // `font-` and the id make the plugin's id.
    font.id.len() + 5 <= MAX_PLUGIN_ID
        && !files.is_empty()
        && files.len() <= MAX_FILES
        && files.iter().enumerate().all(|(n, f)| {
            is_safe_path(&f.path)
                && !f.url.is_empty()
                && f.url.len() <= MAX_URL
                && !f.url.contains(['\t', '\n', '\r'])
                && f.sha1.len() == 40
                && f.sha1.bytes().all(|b| b.is_ascii_hexdigit())
                && f.size > 0
                && f.size <= max_file
                && files[..n].iter().all(|g| g.path != f.path)
        })
        && files.iter().any(|f| f.path == "plugin.ini")
        && font
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !font.id.is_empty()
}

/// What the end of an install request says (`event.install_done`,
/// `docs/abi/device.md` §5): only a finished install is *ready*; a refused
/// confirmation, one that timed out or was dismissed, and any failure leave
/// nothing to use. The staging folder goes in every case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ended {
    /// The font is installed: say so.
    pub ready: bool,
}

/// What an `install_done` with `status` (0 is OK) and `reason` (0 is none)
/// means for the font.
#[must_use]
pub fn ended(status: i32, reason: u32) -> Ended {
    Ended {
        ready: status == 0 && reason == 0,
    }
}

/// One file to download.
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    /// Where it is saved, in the data folder.
    pub save: String,
    pub url: String,
    /// What the catalog says its size is; the download is cut at
    /// [`Plan::max_file`] anyway.
    pub size: u64,
    pub sha1: String,
}

/// The download of one font.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// The plugin it becomes, e.g. `font-fredoka`.
    pub plugin: String,
    /// The folder with its files, in the data folder: what is installed.
    pub folder: String,
    pub steps: Vec<Step>,
    /// The `max_file` of each request.
    pub max_file: u64,
}

impl Plan {
    /// The plan for `font`; `None` if it cannot be installed (an id that
    /// is no plugin's, a file that leaves the folder, with no digest or
    /// too large, a path twice, no `plugin.ini`).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(font: &Font, outlines: bool) -> Result<Option<Self>, Error> {
        if !fits(font, outlines) {
            return Ok(None);
        }
        let max_file = max_file(outlines);
        let plugin = font.plugin()?;
        let files = &font.files;
        let folder = concat(&[STAGING, "/", &plugin])?;
        let mut steps = Vec::new();
        steps
            .try_reserve_exact(files.len())
            .map_err(|_| Error::NoMemory)?;
        for file in files {
            steps.push(Step {
                save: concat(&[&folder, "/", &file.path])?,
                url: paperpaw_ui_kit::text::copy(&file.url)?,
                size: file.size,
                sha1: paperpaw_ui_kit::text::copy(&file.sha1)?,
            });
        }
        Ok(Some(Self {
            plugin,
            folder,
            steps,
            max_file,
        }))
    }

    /// Bytes to download.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.steps.iter().map(|s| s.size).sum()
    }
}

#[cfg(test)]
mod tests;
