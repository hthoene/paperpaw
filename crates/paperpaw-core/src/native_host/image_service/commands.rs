//! `command.image_prepare` and `command.image_release` (`document.md` 5): the
//! admission of a batch's command (shape, roots, path, flags, the plugin's
//! room: no side effects, so a batch is still all or nothing) and what the
//! host does with it after the callback returns.
use super::{ImageTable, MAX_SIDE, prepare};
use crate::native_host::commands::WireCommand;
use alloc::string::String;
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED,
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

/// A root-0 or root-1 path: relative, no `..`, `\`, NUL, empty segments or
/// leading `/` (the storage slots' rules).
fn path_ok(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && path.split('/').all(|part| !part.is_empty() && part != "..")
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

    fn prepare(&mut self, root: u32, path: &str, flags: u32) -> Result<(), i32> {
        match root {
            0 => {}
            1 if self.storage_root => {}
            1 => return Err(ERR_DENIED),
            _ => return Err(ERR_UNSUPPORTED),
        }
        if flags & !COLOUR != 0 {
            return Err(ERR_UNSUPPORTED);
        }
        if !path_ok(path) {
            return Err(ERR_INVALID);
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

/// Resolves a plugin's root 0 or 1 to a card path.
pub(crate) trait ImageRoots {
    /// `ERR_DENIED`, `ERR_NOT_FOUND`, ... where the plugin has no such root.
    fn resolve(&self, owner: u64, root: u32, path: &str) -> Result<String, i32>;
}

impl ImageTable {
    /// How many more requests `owner` may queue now.
    #[must_use]
    pub(crate) fn prepare_room(&self, owner: u64) -> usize {
        self.inner.try_borrow().map_or(0, |inner| {
            let used = inner.waiting.iter().filter(|w| w.owner == owner).count()
                + inner
                    .ready
                    .iter()
                    .filter(|r| r.owner == owner && r.image == 0)
                    .count();
            let prepared = inner
                .entries
                .iter()
                .filter(|e| e.owner == owner && e.origin == super::Origin::Prepared)
                .count();
            prepare::WAITING
                .saturating_sub(used)
                .min(super::PREPARED_IMAGES.saturating_sub(used + prepared))
        })
    }

    /// Does an admitted command for `owner`. A request whose source does not
    /// resolve, or that cannot be queued for want of memory, is answered with
    /// its status (4, 5) on `event.image_ready`, never lost; releasing an id
    /// that is not there is nothing.
    ///
    /// # Errors
    /// `ERR_BUSY` (the table is borrowed: apply again), `ERR_FULL`.
    pub(crate) fn apply(
        &self,
        owner: u64,
        command: ImageCommand,
        roots: &dyn ImageRoots,
    ) -> Result<(), i32> {
        match command {
            ImageCommand::Release { image } => match self.release(owner, image) {
                Err(paperpaw_native::host_glue::ERR_BUSY) => {
                    Err(paperpaw_native::host_glue::ERR_BUSY)
                }
                _ => Ok(()),
            },
            ImageCommand::Prepare {
                root,
                path,
                side,
                cookie,
                colour,
            } => {
                let side = side.min(MAX_SIDE);
                let queued = roots
                    .resolve(owner, root, &path)
                    .and_then(|source| self.prepare(owner, &source, side, cookie, colour));
                match queued {
                    Ok(()) => Ok(()),
                    Err(ERR_NO_MEMORY) => self.refuse(owner, cookie, prepare::NO_MEMORY),
                    Err(paperpaw_native::host_glue::ERR_BUSY) => {
                        Err(paperpaw_native::host_glue::ERR_BUSY)
                    }
                    Err(ERR_FULL) => Err(ERR_FULL),
                    Err(_) => self.refuse(owner, cookie, prepare::CARD),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
