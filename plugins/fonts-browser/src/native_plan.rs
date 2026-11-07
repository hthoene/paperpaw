//! What installing a font takes, without the host: which files are
//! downloaded where (each saved by `core.net` to this plugin's data folder,
//! then checked against its SHA-1), and the folder the host is told to
//! install (`command.install_plugin`, root 0). The same rules as the
//! interface-6 `Package` check, with fallible allocation, and the file sizes
//! that `docs/abi/network.md` §8 audits (`max_file` 512 KiB for packs,
//! 1 MiB for font files).
use crate::native_catalog::Font;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    install::{MAX_FILES, is_plugin_name, is_safe_path},
    native::{Error, net::MAX_URL},
};
use paperpaw_ui_kit::text::concat;

/// Where the files of fonts being installed are kept, in the data folder.
pub const STAGING: &str = "staging";
/// The most a pack is, and a font file.
pub const MAX_PACK: u64 = 512 * 1024;
pub const MAX_OUTLINE: u64 = 1024 * 1024;

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
        let max_file = if outlines { MAX_OUTLINE } else { MAX_PACK };
        let plugin = font.plugin()?;
        let files = &font.files;
        let fits = is_plugin_name(&plugin)
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
            && files.iter().any(|f| f.path == "plugin.ini");
        if !fits {
            return Ok(None);
        }
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
