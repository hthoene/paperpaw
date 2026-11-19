//! A document picture turned a quarter clockwise for its opener
//! (`doc_image_ask.flags` bit 0, `docs/abi/document.md` 5): the turn is made
//! once, into a file of the host's image cache, so that every draw of it reads
//! rows as any other picture's. The turned picture is a prepared image of the
//! opener: it counts among its eight, ends with `image_release` or with the
//! plugin, and its file goes as a prepared image's does.
use super::{ImageTable, Info, TURNED, draw_status, prepare, recipe};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_platform::Storage;
use paperpaw_sdk::image::ImageError;

/// EXIF orientation 6: a quarter turn clockwise.
pub(super) const QUARTER: u8 = 6;

impl ImageTable {
    /// The image `id` of `owner` turned a quarter clockwise, as a prepared
    /// image of `owner`: what `doc_image` says of it (its new id, the turned
    /// size, the picture's `index`, `flags` with [`TURNED`]). The turn has the
    /// device's limits ([`prepare::Within`]): tiles of 32 KiB and 16,384
    /// reads on the X4, 1 MiB and no read limit where there is memory.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND`, `ERR_FULL` (eight prepared images, or requests that
    /// wait), `ERR_UNSUPPORTED` (a part of a file), `ERR_NO_MEMORY` (no room
    /// now, or a picture over the device's turn budget), `ERR_IO`,
    /// `ERR_INVALID` (a file that is not what its id says), `ERR_BUSY`.
    pub(crate) fn turned(&self, storage: &dyn Storage, owner: u64, id: u32) -> Result<Info, i32> {
        let limits = if self.scratch_max.get() <= super::SCRATCH_X4 {
            (32 * 1024, 16_384)
        } else {
            (1 << 20, usize::MAX)
        };
        let found = self.found(owner, id)?;
        if found.at != (0, 0) || found.file != found.size {
            return Err(ERR_UNSUPPORTED);
        }
        let number = {
            let mut inner = self.lock()?;
            if inner.room(owner) == 0 {
                return Err(ERR_FULL);
            }
            let number = inner.seq;
            inner.seq = inner.seq.wrapping_add(1) & 0x7fff_ffff;
            number
        };
        let (kind, width, height) =
            crate::kit::ui::probe_picture(storage, &found.path).map_err(draw_status)?;
        if (width, height) != found.file {
            return Err(ERR_INVALID);
        }
        let channels = match kind {
            crate::kit::ui::PictureKind::Color => 3,
            crate::kit::ui::PictureKind::Alpha => 2,
            _ => 1,
        };
        let path = prepare::file_name(owner, number)?;
        let made = recipe::turn_file(storage, (&found.path, &path), QUARTER, channels, limits)
            .map_err(|error| match error {
                ImageError::OutOfMemory | ImageError::TooLarge => ERR_NO_MEMORY,
                ImageError::Invalid(_) => ERR_INVALID,
                _ => ERR_IO,
            })
            .and_then(|()| self.add_prepared(owner, number, height, width, found.colour));
        match made {
            Ok(image) => Ok(Info {
                image,
                index: found.index,
                width: height,
                height: width,
                flags: u32::from(found.colour) | TURNED,
            }),
            Err(status) => {
                let _ = storage.remove(&path);
                Err(status)
            }
        }
    }
}

#[cfg(test)]
#[path = "turned_tests.rs"]
mod tests;
