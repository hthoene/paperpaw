//! The font files of a plugin's own data folder (`font.acquire` key
//! `data:<path>`, `docs/abi/screens-input.md` section 4): opened when
//! acquired, owned by the handle, and stale (`ERR_STATE`) once the file is
//! changed or removed.
use super::{
    environment::EnvironmentScope,
    resources::OwnerResources,
    shared::try_box,
    storage::{PREFIX, relative_path, status as storage_status},
};
use alloc::{boxed::Box, string::String};
use core::cell::Cell;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::{FontError, FontHandle, FontOpenError, FontSet, Fonts, Id, PackLimits};

/// The glyphs a data font keeps at most, on a device of the small memory
/// class (the X4: eight handles take 32 KiB at most of its 256 KiB heap) and
/// elsewhere. The cache grows as glyphs are drawn, and a refused allocation
/// only keeps a glyph less (`paperpaw_text::GlyphCache`): a draw never fails
/// for it.
pub(crate) const CACHE_SMALL: usize = 4 * 1024;
pub(crate) const CACHE_LARGE: usize = 16 * 1024;

/// The cache of a device whose font packs are read within `limits`: the large
/// one only where the limits are the large class's (a class to come is small).
pub(crate) fn cache_for(limits: PackLimits) -> usize {
    if limits == PackLimits::LARGE {
        CACHE_LARGE
    } else {
        CACHE_SMALL
    }
}

/// What the card said of the file when it was opened: any change of it is a
/// change of the file.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<i64>,
    identity: Option<u64>,
}

impl Stamp {
    fn of(card: &dyn Storage, path: &str) -> Result<Self, StorageError> {
        Ok(Self {
            size: card.size(path)?,
            modified: card.modified(path),
            identity: card.entry_identity(path)?,
        })
    }
}

/// An opened font file of the plugin's data folder.
pub(crate) struct DataFont {
    set: FontSet,
    /// The file on the card (`.paperpaw/plugins/<id>/<path>`).
    path: String,
    stamp: Stamp,
    /// The callback the file was last looked at in: asked once for each, not
    /// at every text call.
    checked: Cell<Option<u64>>,
}

impl DataFont {
    /// Opens the file at `relative` in the calling plugin's data folder: the
    /// path is checked as every storage path is (relative; no `..`, no root)
    /// and joined under `.paperpaw/plugins/<id>/`, then opened as the chosen
    /// family's and a book's fonts are ([`paperpaw_sdk::open_font_file`]).
    ///
    /// # Errors
    /// `ERR_INVALID` (or `ERR_NO_SPACE`) for a path that is not one and for a
    /// file that is no font; `ERR_NOT_FOUND`; `ERR_NO_MEMORY`; `ERR_STATE` for
    /// a frame without fonts or an owner without a plugin; `ERR_UNSUPPORTED`
    /// where no card is wired.
    pub(crate) fn open(
        scope: &EnvironmentScope,
        owner: &OwnerResources,
        relative: &str,
    ) -> Result<Box<Self>, i32> {
        let relative = relative_path(relative.as_bytes())?;
        let plugin = owner.bound_caller()?;
        let card = owner.hosts().font_card().ok_or(ERR_UNSUPPORTED)?;
        let mut path = String::new();
        path.try_reserve_exact(PREFIX.len() + plugin.as_str().len() + 1 + relative.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        path.push_str(PREFIX);
        path.push_str(plugin.as_str());
        path.push('/');
        path.push_str(relative);
        let limits =
            scope.with_frame(|frame| frame.fonts().map(Fonts::pack_limits).ok_or(ERR_STATE))??;
        // Before the file is opened: a change in between is a stale handle.
        let stamp = Stamp::of(&**card, &path).map_err(|error| match error {
            StorageError::NotFound => ERR_NOT_FOUND,
            other => storage_status(other),
        })?;
        let font =
            paperpaw_sdk::open_font_file(card, &path, limits).map_err(|error| match error {
                FontOpenError::NotFound => ERR_NOT_FOUND,
                FontOpenError::Storage(error) => storage_status(error),
                FontOpenError::Font(FontError::OutOfMemory) => ERR_NO_MEMORY,
                _ => ERR_INVALID,
            })?;
        let mut set = FontSet::new(cache_for(limits));
        set.add(Id::new_static("core.data-font"), font);
        try_box(Self {
            set,
            path,
            stamp,
            checked: Cell::new(None),
        })
        .ok_or(ERR_NO_MEMORY)
    }

    /// The font, ready to measure and draw.
    pub(crate) fn handle(&self) -> Result<FontHandle<'_>, i32> {
        self.set.get(0).ok_or(ERR_STATE)
    }

    /// `ERR_STATE` once the file is not as it was opened: changed, replaced
    /// or removed. The card is asked once for each `epoch` (callback); where
    /// the frame has none, at every call. A plugin that rewrites its own font
    /// file within the callback it draws in is not noticed until the next one:
    /// what the pack gives meanwhile is read with the pack's own checks, so a
    /// glyph that cannot be read draws nothing (a failed read), never a panic.
    pub(crate) fn check(&self, owner: &OwnerResources, epoch: Option<u64>) -> Result<(), i32> {
        if epoch.is_some() && self.checked.get() == epoch {
            return Ok(());
        }
        let card = owner.hosts().font_card().ok_or(ERR_STATE)?;
        match Stamp::of(&**card, &self.path) {
            Ok(now) if now == self.stamp => {
                self.checked.set(epoch);
                Ok(())
            }
            _ => Err(ERR_STATE),
        }
    }
}
