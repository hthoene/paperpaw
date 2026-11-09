//! Prepare and release the plugin's own pictures (`docs/abi/document.md` 5).
use super::{CommandSender, Error, wire};
use crate::native::image::{ImageId, PrepareImage};

impl CommandSender<'_, '_> {
    /// Have the host decode a picture file in the background
    /// (`command.image_prepare`). One request is decoded per main-loop turn; at most
    /// 4 wait and 8 prepared pictures are kept per plugin. The end comes as one
    /// [`Event::ImageReady`](crate::native::events::Event) for the request's cookie
    /// (subscribe to `EventKind::ImageReady` first).
    ///
    /// # Errors
    /// Returns Invalid for a path over 512 bytes (nothing is staged), NoSpace for
    /// short scratch, or the host's refusal when the command is submitted. Limits
    /// the host meets later (the waiting and prepared caps, a missing file, a
    /// library root without `uses.storage`) come as an [`ImageReady`](crate::native::image::ImageReady) status or a
    /// refusal of the host's choosing, per `document.md` 5.
    pub fn image_prepare(&mut self, request: &PrepareImage<'_>) -> Result<(), Error> {
        self.submit(request.command()?)
    }

    /// Give a prepared picture back (`command.image_release`). An id that is
    /// already gone is not an error the host reports; release every id once.
    ///
    /// # Errors
    /// Returns NoSpace for short scratch or the host's refusal.
    pub fn image_release(&mut self, image: ImageId) -> Result<(), Error> {
        self.submit(wire::Command::ImageRelease { image: image.get() })
    }
}
