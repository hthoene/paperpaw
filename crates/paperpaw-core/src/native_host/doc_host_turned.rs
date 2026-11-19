//! Op 16's turned picture on the production host: the image table makes it
//! from the opener's image, on the card the documents are on.
use super::Bound;
use crate::native_host::registrations as wire;
use paperpaw_native::host_glue::ERR_IO;

impl Bound<'_> {
    /// The caller's image `image` turned a quarter clockwise, as `doc_image`
    /// says it (in the caller's id space: `Bound` has no owner of its own) ([`crate::native_host::image_service::ImageTable::turned`]).
    pub(super) fn turned_image(&self, image: u32) -> Result<wire::ServiceDocImage, i32> {
        let storage = self.state.storage.as_deref().ok_or(ERR_IO)?;
        let info = self
            .who
            .hosts
            .images()
            .turned(storage, self.who.owner, image)?;
        Ok(wire::ServiceDocImage {
            image: info.image,
            index: info.index,
            width: info.width,
            height: info.height,
            flags: info.flags,
        })
    }
}
