//! A plugin source folder (`docs/abi/device.md` 5.1, 5.2) read in bounded
//! steps: what it holds, what an install would copy, and what its
//! `plugin.ini` says (id, name, version).
//!
//! The source of an install is a plugin folder, as in interface 6
//! (`install::local`): `plugin.ini`, translations, data and, for a code
//! plugin, `code.ppn`. A bare `.ppn`, a path that names a file or nothing,
//! is not a source (reason 4).
//!
//! * **The walk.** A queue of at most [`DIRS_MAX`] folders, each listed in
//!   pieces of [`PIECE`] entries ([`Storage::list_from`]), so a step costs the
//!   entries it reads however large the folder. A path no plugin file may have
//!   ([`paperpaw_sdk::install::is_safe_path`]), more than [`FILES_MAX`] files
//!   or more than [`DIRS_MAX`] folders end it with reason 4.
//! * **Bytes** are the sum of the sizes of the files, which is what lands
//!   on the card.
//! * **The manifest** is read whole into the job's I/O buffer (at most
//!   [`MANIFEST_MAX`] bytes, the buffer's size) and its `[plugin]` section read
//!   as the loader does.
//! * **Names** follow `install::is_safe_path`, the rule of interface 6's
//!   local install: a dot file (`._x`, `.DS_Store`) or an unsafe name in the
//!   folder ends it with reason 4, as it did there.
//! * **Links** (a desktop shell's symlinks) are not told apart here: a shell
//!   lists what a card holds. The desktop rule of #786 R1 applies before the
//!   desktop shell serves installs.
use alloc::{string::String, vec::Vec};
use paperpaw_platform::{Storage, StorageError, firmware::Text};
use paperpaw_sdk::{Version, install, native::CODE_FILES};

use super::files_service::same_fold;

use super::plugin_job::{ID_MAX, NAME_MAX, VERSION_MAX, reason};

/// Entries read from one folder in one step.
pub(crate) const PIECE: usize = 16;
/// Most files a plugin has (interface 6's `MAX_FILES`).
pub(crate) const FILES_MAX: u32 = install::MAX_FILES as u32;
/// Most folders a plugin has, the root included.
pub(crate) const DIRS_MAX: usize = 16;
/// Largest `plugin.ini` read.
pub(crate) const MANIFEST_MAX: usize = 4 * 1024;
/// Longest source path.
const PATH_MAX: usize = 512;

/// What the walk found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) files: u32,
    pub(crate) bytes: u64,
    /// `plugin.ini` is there.
    pub(crate) manifest: bool,
    /// The top-level code file: its index in [`CODE_FILES`].
    pub(crate) code: Option<usize>,
}

/// What one step of the walk came to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Walked {
    More,
    Done(Found),
    /// Ended with this reason (`device.md` 5.5).
    Refused(u8),
}

/// The longest name of [`CODE_FILES`] (15 bytes) with room to spare.
const CODE_NAME_MAX: usize = 16;

/// The walk over one source folder.
pub(crate) struct Walk {
    root: String,
    /// Folders to list, as paths below the root (`""` is the root).
    dirs: Vec<String>,
    at: usize,
    place: u64,
    found: Found,
    /// The name the top-level code file has on the card (a FAT lists the
    /// case it was stored in, which may not be the one of [`CODE_FILES`]).
    code_name: Text<CODE_NAME_MAX>,
}

impl Walk {
    /// A walk over the folder at `root`. No memory is `None`.
    pub(crate) fn new(root: &str) -> Option<Self> {
        let root = root.trim_end_matches('/');
        if root.is_empty() || root.len() > PATH_MAX {
            return None;
        }
        let mut owned = String::new();
        owned.try_reserve_exact(root.len()).ok()?;
        owned.push_str(root);
        let mut dirs = Vec::new();
        dirs.try_reserve_exact(DIRS_MAX).ok()?;
        dirs.push(String::new());
        Some(Self {
            root: owned,
            dirs,
            at: 0,
            place: 0,
            code_name: Text::new(""),
            found: Found {
                files: 0,
                bytes: 0,
                manifest: false,
                code: None,
            },
        })
    }

    /// The folder's own name: the default id.
    pub(crate) fn folder_name(&self) -> &str {
        self.root.rsplit('/').next().unwrap_or(&self.root)
    }

    pub(crate) fn root(&self) -> &str {
        &self.root
    }

    /// The code file's name as the card lists it.
    pub(crate) fn code_name(&self) -> &str {
        self.code_name.as_str()
    }

    /// Takes over what one listed piece found and moves on.
    fn settle(
        &mut self,
        next: Option<u64>,
        found: Found,
        code_name: Option<Text<CODE_NAME_MAX>>,
        subdirs: Vec<String>,
    ) -> Walked {
        self.found = found;
        if let Some(name) = code_name {
            self.code_name = name;
        }
        self.dirs.extend(subdirs);
        match next {
            Some(place) => self.place = place,
            None => {
                self.at += 1;
                self.place = 0;
            }
        }
        if self.at >= self.dirs.len() {
            Walked::Done(self.found)
        } else {
            Walked::More
        }
    }

    /// One piece of the folder being listed.
    #[inline(never)]
    pub(crate) fn step(&mut self, storage: &dyn Storage) -> Walked {
        let Some(rel) = self.dirs.get(self.at) else {
            return Walked::Done(self.found);
        };
        let mut path = String::new();
        let wanted = self.root.len() + 1 + rel.len();
        if path.try_reserve_exact(wanted).is_err() {
            return Walked::Refused(reason::OTHER);
        }
        path.push_str(&self.root);
        if !rel.is_empty() {
            path.push('/');
            path.push_str(rel);
        }
        let mut seen = 0_usize;
        let mut failed: Option<u8> = None;
        let mut subdirs: Vec<String> = Vec::new();
        let mut found = self.found;
        let mut code_name = None;
        let top = rel.is_empty();
        let known = self.dirs.len();
        let listed = storage.list_from(&path, self.place, &mut |entry| {
            if seen == PIECE || failed.is_some() {
                return false;
            }
            seen += 1;
            let mut name = String::new();
            let wanted = rel.len() + 1 + entry.name.len();
            if name.try_reserve_exact(wanted).is_err() {
                failed = Some(reason::OTHER);
                return false;
            }
            if !top {
                name.push_str(rel);
                name.push('/');
            }
            name.push_str(entry.name);
            if !install::is_safe_path(&name) {
                failed = Some(reason::DAMAGED);
                return false;
            }
            if entry.is_dir {
                if known + subdirs.len() >= DIRS_MAX || subdirs.try_reserve(1).is_err() {
                    failed = Some(reason::DAMAGED);
                    return false;
                }
                subdirs.push(name);
                return true;
            }
            found.files += 1;
            found.bytes = found.bytes.saturating_add(entry.size);
            if found.files > FILES_MAX {
                failed = Some(reason::DAMAGED);
                return false;
            }
            // FAT finds a name whatever its case, so does this.
            if top && same_fold(entry.name, "plugin.ini") {
                found.manifest = true;
            }
            if top
                && let Some(index) = CODE_FILES
                    .iter()
                    .position(|code| same_fold(code, entry.name))
            {
                // `code.ppn` is the one this device reads; it wins.
                if found.code != Some(0) {
                    found.code = Some(index);
                    code_name = Some(Text::new(entry.name));
                }
            }
            true
        });
        if let Some(reason) = failed {
            return Walked::Refused(reason);
        }
        match listed {
            Ok(next) => self.settle(next, found, code_name, subdirs),
            // A path that names nothing, or a file: not a plugin folder.
            Err(StorageError::NotFound | StorageError::InvalidPath) => {
                Walked::Refused(reason::DAMAGED)
            }
            Err(StorageError::OutOfMemory) => Walked::Refused(reason::OTHER),
            Err(_) => Walked::Refused(reason::CARD),
        }
    }
}

/// The reason a card error ends with: no memory is 1, not a card error.
fn card_reason(error: StorageError) -> u8 {
    match error {
        StorageError::OutOfMemory => reason::OTHER,
        StorageError::NotFound => reason::DAMAGED,
        _ => reason::CARD,
    }
}

/// What `plugin.ini` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub(crate) id: Text<ID_MAX>,
    pub(crate) name: Text<NAME_MAX>,
    pub(crate) version: Text<VERSION_MAX>,
    pub(crate) parsed: Version,
}

/// Reads `plugin.ini` of the folder at `root`: the id it names, the folder's
/// name `folder` where it names none, its name (the id where none) and its
/// version (0.1.0 where none), as the loader reads them.
///
/// # Errors
/// A reason of 5.5: 4 for a manifest that is not text, too large, names an
/// id or a version no plugin has; 16 for a card error; 1 for no memory.
#[inline(never)]
pub(crate) fn read_manifest(
    storage: &dyn Storage,
    root: &str,
    folder: &str,
    buf: &mut [u8],
) -> Result<Manifest, u8> {
    let mut path = String::new();
    path.try_reserve_exact(root.len() + 11)
        .map_err(|_| reason::OTHER)?;
    path.push_str(root);
    path.push_str("/plugin.ini");
    let size = storage.size(&path).map_err(card_reason)?;
    if size > MANIFEST_MAX as u64 {
        return Err(reason::DAMAGED);
    }
    // Read into the job's buffer: no second 4 KiB beside it.
    let data = buf.get_mut(..size as usize).ok_or(reason::OTHER)?;
    let mut at = 0;
    while at < data.len() {
        match storage.read_at(&path, at as u64, &mut data[at..]) {
            Ok(0) => return Err(reason::CARD),
            Ok(n) => at += n,
            Err(error) => return Err(card_reason(error)),
        }
    }
    let source = core::str::from_utf8(data).map_err(|_| reason::DAMAGED)?;
    let (mut id, mut name, mut version) = (None, None, None);
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(|_| reason::DAMAGED)?;
        if entry.section != "plugin" {
            continue;
        }
        let value = entry.value.as_str();
        let value = value.trim();
        match entry.key {
            // A longer id or version names no plugin: refused, not cut.
            "id" if value.len() <= ID_MAX => id = Some(Text::<ID_MAX>::new(value)),
            "id" => return Err(reason::DAMAGED),
            "name" => name = Some(Text::<NAME_MAX>::new(value)),
            "version" if value.len() <= VERSION_MAX => {
                version = Some(Text::<VERSION_MAX>::new(value));
            }
            "version" => return Err(reason::DAMAGED),
            _ => {}
        }
    }
    let id = id.unwrap_or_else(|| Text::new(folder));
    if id.as_str().is_empty() || !install::is_plugin_name(id.as_str()) {
        return Err(reason::DAMAGED);
    }
    let version = version.unwrap_or_else(|| Text::new("0.1.0"));
    let parsed = Version::try_parse(version.as_str()).map_err(|_| reason::DAMAGED)?;
    let name = name.unwrap_or_else(|| Text::new(id.as_str()));
    Ok(Manifest {
        id,
        name,
        version,
        parsed,
    })
}
