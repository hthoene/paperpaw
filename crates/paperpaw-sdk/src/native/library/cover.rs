//! A book's cover as a picture the host can draw (`docs/abi/library.md` 2.1, op 7).
use super::*;
use crate::native::image::{ImageId, ImageInfo};

/// A book's cover: an image id of the caller and its size
/// (`docs/abi/document.md` 5). Draw it with
/// [`Canvas::draw_image`](crate::native::canvas::Canvas::draw_image) and give it
/// back with `image_release` when the list no longer shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Cover {
    image: ImageId,
    info: ImageInfo,
}

impl Cover {
    /// The cover's image id.
    #[must_use]
    pub const fn image(&self) -> ImageId {
        self.image
    }

    /// The cover's size and flags.
    #[must_use]
    pub const fn info(&self) -> ImageInfo {
        self.info
    }
}

impl Library {
    /// The catalog's cover of a book (op 7): an image id of the caller, made from
    /// the book's format on first use and kept in the host's cover cache. `side` 0
    /// is the list's size (the larger of 300 and 3/8 of the screen's long side),
    /// else the longest side, at most the screen's long side. `input` is 1,024
    /// bytes; `out` 4,096.
    ///
    /// # Errors
    /// `NotFound` for a book without a cover or an unknown book; `Busy` while the
    /// cover is being made (ask again when `library_changed` bit 6 says it is
    /// ready); `State` for a stale catalog handle; `Invalid` for an answer that is
    /// not one `doc_image` record with an image id; otherwise the host's refusals.
    pub fn cover(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        side: u32,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Cover, Error> {
        let ask = wire::ServiceLibCover { side };
        let n = self.call(host, schema::OP_COVER, &[book.record(), &ask], input, out)?;
        let row = one(out.get(..n).ok_or(Error::Invalid)?)?
            .decode::<wire::ServiceDocImage>()
            .map_err(|_| Error::Invalid)?;
        Ok(Cover {
            image: ImageId::new(row.image)?,
            info: ImageInfo::from_row(&row),
        })
    }
}
