//! `command.image_prepare` and `command.image_release` (`document.md` 5): the
//! admission of a batch's command (shape, roots, path, flags, the plugin's
//! room: no side effects, so a batch is still all or nothing) and what the
//! host does with it after the callback returns.
use super::{ImageTable, MAX_SIDE, prepare};
use crate::native_host::commands::WireCommand;
use alloc::string::String;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND,
    ERR_UNSUPPORTED,
};

/// `image_prepare.flags` bit 0: colour, where the screen has it.
const COLOUR: u32 = 1;

/// What the stage checks a batch against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageAdmit {
    /// The plugin has `uses.storage`: root 1 is open to it.
    pub(crate) storage_root: bool,
    /// How many more requests the plugin may queue now
    /// ([`ImageTable::prepare_room`]); each admitted one takes one.
    pub(crate) room: usize,
}

/// An admitted command, owned, for the host to apply after the callback.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ImageCommand {
    Prepare {
        root: u32,
        path: String,
        side: u32,
        cookie: u32,
        colour: bool,
    },
    Release {
        image: u32,
    },
}

/// The path of a picture as `core.files` checks one (sizes, UTF-8 and
/// grammar, `storage::relative_path`), and not the root itself: `ERR_INVALID`
/// or `ERR_NO_SPACE`.
fn checked(path: &str) -> Result<(), i32> {
    if path.is_empty() {
        return Err(ERR_INVALID);
    }
    crate::native_host::storage::relative_path(path.as_bytes()).map(|_| ())
}

impl ImageAdmit {
    /// Checks `command` if it is an image command and takes its room:
    /// `None` for any other command, which is the caller's.
    pub(crate) fn admit(&mut self, command: &WireCommand<'_>) -> Option<Result<(), i32>> {
        match command {
            WireCommand::ImagePrepare {
                root, path, flags, ..
            } => Some(self.prepare(*root, path, *flags)),
            WireCommand::ImageRelease { .. } => Some(Ok(())),
            _ => None,
        }
    }

    /// [`ImageAdmit::admit`], and the text the command stages (an image
    /// command's path; a release stages none) counted into `bytes`, the
    /// callback's command byte budget with its `limit` (`commands.md` 3):
    /// `ERR_NO_SPACE` where it is over.
    pub(crate) fn admit_counted(
        &mut self,
        command: &WireCommand<'_>,
        bytes: &mut usize,
        limit: usize,
    ) -> Option<Result<(), i32>> {
        let result = self.admit(command)?;
        if result.is_ok()
            && let WireCommand::ImagePrepare { path, .. } = command
        {
            *bytes = bytes.saturating_add(path.len());
            if *bytes > limit {
                return Some(Err(ERR_NO_SPACE));
            }
        }
        Some(result)
    }

    fn prepare(&mut self, root: u32, path: &str, flags: u32) -> Result<(), i32> {
        match root {
            0 => {}
            1 if self.storage_root => {}
            1 => return Err(ERR_DENIED),
            // The plugin's own install folder: read only, no feature.
            2 => {}
            _ => return Err(ERR_UNSUPPORTED),
        }
        if flags & !COLOUR != 0 {
            return Err(ERR_UNSUPPORTED);
        }
        checked(path)?;
        // Root 1 never reaches the install folders, the kernel's folder or its
        // settings file.
        if root == 1 {
            let first = path.split('/').next().unwrap_or_default();
            if super::super::owner_paths::protected_first(first) {
                return Err(ERR_INVALID);
            }
        }
        self.room = self.room.checked_sub(1).ok_or(ERR_FULL)?;
        Ok(())
    }
}

/// Whether `command` is one of the two image commands.
pub(crate) fn owned_kind(command: &WireCommand<'_>) -> bool {
    matches!(
        command,
        WireCommand::ImagePrepare { .. } | WireCommand::ImageRelease { .. }
    )
}

/// Copies an admitted image command, fallibly; `None` for another command.
#[inline(never)]
pub(crate) fn owned(command: &WireCommand<'_>) -> Option<Result<ImageCommand, i32>> {
    match command {
        WireCommand::ImagePrepare {
            root,
            path,
            side,
            cookie,
            flags,
        } => {
            let mut text = String::new();
            if text.try_reserve_exact(path.len()).is_err() {
                return Some(Err(ERR_NO_MEMORY));
            }
            text.push_str(path);
            Some(Ok(ImageCommand::Prepare {
                root: *root,
                path: text,
                side: *side,
                cookie: *cookie,
                colour: *flags & COLOUR != 0,
            }))
        }
        WireCommand::ImageRelease { image } => Some(Ok(ImageCommand::Release { image: *image })),
        _ => None,
    }
}

/// The card path of `path` in the install folder of the plugin `plugin`
/// (root 2), as `core.files` resolves it: `plugins/<plugin>/<path>`, the
/// room taken fallibly. The folder is read only here (a picture is only
/// read). A built-in plugin's folder counts as empty (`built_in`, from the
/// same knowledge as `FilesHost::built_in`), whatever a leftover install
/// left on the card: every path is not found.
///
/// # Errors
/// `ERR_INVALID` or `ERR_NO_SPACE` for a path `core.files` would refuse, or
/// a plugin id that is not one folder name or leaves no room for it,
/// `ERR_NOT_FOUND` for a built-in plugin, `ERR_NO_MEMORY`.
pub(crate) fn bundle_source(plugin: &str, built_in: bool, path: &str) -> Result<String, i32> {
    use crate::native_host::storage::{MAX_FOLDER, relative_path};
    const KIND: &str = "plugins/";
    if KIND.len() + plugin.len() + 1 > MAX_FOLDER
        || plugin.contains('/')
        || relative_path(plugin.as_bytes()).is_err()
    {
        return Err(ERR_INVALID);
    }
    checked(path)?;
    if built_in {
        return Err(ERR_NOT_FOUND);
    }
    let mut source = String::new();
    source
        .try_reserve_exact(KIND.len() + plugin.len() + 1 + path.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    source.push_str(KIND);
    source.push_str(plugin);
    source.push('/');
    source.push_str(path);
    Ok(source)
}

/// Resolves a plugin's root 0, 1 or 2 to a card path.
pub(crate) trait ImageRoots {
    /// `ERR_DENIED`, `ERR_NOT_FOUND`, ... where the plugin has no such root;
    /// root 2 is [`bundle_source`] of the plugin's id.
    fn resolve(&self, owner: u64, root: u32, path: &str) -> Result<String, i32>;
}

impl ImageTable {
    /// How many more requests `owner` may queue now.
    #[must_use]
    pub(crate) fn prepare_room(&self, owner: u64) -> usize {
        self.inner.try_borrow().map_or(0, |inner| inner.room(owner))
    }

    /// Does an admitted command for `owner`, and keeps it for a retry where
    /// it could not be done: only `Ok` means it is queued or answered (a
    /// request whose source does not resolve is answered with status 4 on
    /// `event.image_ready`; one that cannot be queued for want of memory with
    /// status 5, its answer's room taken first so it is never lost).
    /// Releasing an id that is not there is nothing.
    ///
    /// # Errors
    /// `ERR_BUSY` (the table is borrowed), `ERR_FULL` (no room now),
    /// `ERR_NO_MEMORY` (neither the request nor its answer can be kept now):
    /// in each the caller keeps the command and applies it again.
    pub(crate) fn apply(
        &self,
        owner: u64,
        command: &ImageCommand,
        roots: &dyn ImageRoots,
    ) -> Result<(), i32> {
        match command {
            ImageCommand::Release { image } => match self.release(owner, *image) {
                Err(ERR_BUSY) => Err(ERR_BUSY),
                _ => Ok(()),
            },
            ImageCommand::Prepare {
                root,
                path,
                side,
                cookie,
                colour,
            } => {
                let side = (*side).min(MAX_SIDE);
                let source = match roots.resolve(owner, *root, path) {
                    Ok(source) => source,
                    Err(ERR_NO_MEMORY) => return Err(ERR_NO_MEMORY),
                    // A path that names nothing is a missing file, on every root.
                    Err(ERR_NOT_FOUND) => {
                        return self.refuse(owner, *cookie, prepare::NOT_FOUND);
                    }
                    Err(_) => return self.refuse(owner, *cookie, prepare::CARD),
                };
                match self.prepare(owner, &source, side, *cookie, *colour) {
                    Err(ERR_NO_MEMORY) => self.refuse(owner, *cookie, prepare::NO_MEMORY),
                    other => other,
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
