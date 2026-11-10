//! Pictures the host decodes and draws (`docs/abi/document.md` 5): an image
//! id names a decoded picture the host keeps on the card and draws in bands, so
//! no plugin holds a picture in memory. Ask the host to prepare one of the
//! plugin's own files with [`CommandSender::image_prepare`](super::commands::CommandSender),
//! read its size with [`ImageInfo::read`], draw it with [`Canvas::draw_image`],
//! and give it back with `image_release`.
use super::{
    Error, Host,
    binding::status,
    callbacks::FrameRect,
    canvas::Canvas,
    declare::records_wire::{self as wire, RawRecord},
    wire::commands_wire::{COMMAND_IMAGE_PREPARE_PATH_MAX_BYTES, Command},
};
use core::num::NonZeroU32;
use paperpaw_native::host_glue::PpBuffer;

/// Longest side `image_prepare` decodes to; the host clamps a larger one.
pub const SIDE_MAX: u32 = 1_600;
/// Most images prepared and not released at once per plugin.
pub const PREPARED_MAX: usize = 8;
/// Most prepare requests waiting at once per plugin.
pub const WAITING_MAX: usize = 4;

/// Bits of `doc_image.flags` (from `image.info`); bit 0 is also `image_prepare.flags`.
const COLOUR: u32 = 1;
const NOT_DECODED: u32 = 1 << 1;
/// Bit 1 of `image_prepare.flags`: apply a JPEG's EXIF orientation.
const TURN: u32 = 1 << 1;
/// Bits of `canvas.draw_image` flags.
const FILL: u32 = 1;
const SMALL: u32 = 1 << 1;
const STRETCH: u32 = 1 << 2;
const DOTS: u32 = 1 << 3;
const GRAYS: u32 = 1 << 4;
const TRANSPARENCY_SHIFT: u32 = 8;
/// The bytes of a tone table (`image_prepare.tone`).
pub const TONE_BYTES: usize = 256;
/// The bytes of the brightness grid of `event.image_ready`: 8 by 8, row by row.
pub const LUMA_BYTES: usize = 64;

/// The id of a decoded picture the host may draw. Ids are per plugin and never 0;
/// one ends with `image_release`, with its document or when the plugin stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(NonZeroU32);

impl ImageId {
    /// Name an image by its number.
    ///
    /// # Errors
    /// Invalid for 0.
    pub const fn new(id: u32) -> Result<Self, Error> {
        match NonZeroU32::new(id) {
            Some(id) => Ok(Self(id)),
            None => Err(Error::Invalid),
        }
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// What the host knows of a decoded picture (`image.info`, slot 36).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageInfo {
    width: u32,
    height: u32,
    colour: bool,
    not_decoded: bool,
}

impl ImageInfo {
    /// Ask for the size of `image`. Allocates nothing.
    ///
    /// # Errors
    /// `NotFound` for an unknown, ended or another plugin's id; `Invalid` for an
    /// answer that is not one `doc_image` record; `Unsupported` on a host that
    /// draws no images; otherwise the host's refusals.
    pub fn read(host: &Host<'_>, image: ImageId) -> Result<Self, Error> {
        let mut bytes = [0u8; 64];
        // SAFETY: the host binding is live for this call; the output buffer is
        // call-local.
        let result = unsafe {
            (host.table.image_info)(
                host.ctx,
                image.get(),
                PpBuffer {
                    ptr: bytes.as_mut_ptr(),
                    cap: bytes.len(),
                },
            )
        };
        let len = usize::try_from(result).map_err(|_| Error::from_status(result))?;
        let row = RawRecord::one(bytes.get(..len).ok_or(Error::Invalid)?)
            .map_err(|_| Error::Invalid)?
            .decode::<wire::ServiceDocImage>()
            .map_err(|_| Error::Invalid)?;
        Ok(Self::from_row(&row))
    }

    /// The size and the two defined flag bits of a `doc_image` row.
    pub(crate) const fn from_row(row: &wire::ServiceDocImage) -> Self {
        Self {
            width: row.width,
            height: row.height,
            colour: row.flags & COLOUR != 0,
            not_decoded: row.flags & NOT_DECODED != 0,
        }
    }

    /// The picture's width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// The picture's height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Whether the picture is in colour (where the screen has it).
    #[must_use]
    pub const fn colour(&self) -> bool {
        self.colour
    }

    /// Whether the format has not decoded the picture yet: the document must
    /// decode it (`document.md` 5, op 16) before it is drawn. Other flag bits are
    /// ignored.
    #[must_use]
    pub const fn not_decoded(&self) -> bool {
        self.not_decoded
    }
}

/// How `canvas.draw_image` draws: scaled into the rectangle, keeping its shape and
/// centred, unless [`fill`](Self::fill) crops it to the rectangle. The flags the
/// contract defines are the only ones this builder can set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageStyle {
    flags: u32,
}

impl ImageStyle {
    /// Scale to fit, centred, opaque; it counts towards the ghosting refresh.
    #[must_use]
    pub const fn new() -> Self {
        Self { flags: 0 }
    }

    /// Fill the rectangle and crop what does not fit. Replaces
    /// [`stretch`](Self::stretch).
    #[must_use]
    pub const fn fill(mut self) -> Self {
        self.flags = self.flags & !STRETCH | FILL;
        self
    }

    /// Scale to the rectangle and drop the picture's shape. Replaces
    /// [`fill`](Self::fill).
    #[must_use]
    pub const fn stretch(mut self) -> Self {
        self.flags = self.flags & !FILL | STRETCH;
        self
    }

    /// Draw in black and white dots, also on a panel with grays. Replaces
    /// [`grays`](Self::grays); neither is the panel's own rule.
    #[must_use]
    pub const fn dots(mut self) -> Self {
        self.flags = self.flags & !GRAYS | DOTS;
        self
    }

    /// Draw in the panel's grays where it has them. Replaces
    /// [`dots`](Self::dots).
    #[must_use]
    pub const fn grays(mut self) -> Self {
        self.flags = self.flags & !DOTS | GRAYS;
        self
    }

    /// A small picture: it does not count towards the e-paper ghosting full
    /// refresh.
    #[must_use]
    pub const fn small(mut self) -> Self {
        self.flags |= SMALL;
        self
    }

    /// Blend over what is below: 0 is opaque (the default), 255 draws nothing.
    #[must_use]
    pub const fn transparency(mut self, transparency: u8) -> Self {
        self.flags = self.flags & !(0xff << TRANSPARENCY_SHIFT)
            | (transparency as u32) << TRANSPARENCY_SHIFT;
        self
    }

    pub(crate) const fn flags(self) -> u32 {
        self.flags
    }
}

impl Canvas<'_> {
    /// Draw a decoded picture scaled into `rect` (`canvas.draw_image`, slot 37),
    /// clipped by the canvas, in bands read from the card. Only inside a draw
    /// callback.
    ///
    /// # Errors
    /// `State` outside a draw callback; `NotFound` for an unknown or ended id (ask
    /// again for the picture); `Invalid` for a scaled picture over 65,535 pixels on
    /// a side or a cache file whose header does not match; `Io` for a card failure
    /// (bands drawn so far are kept); `NoMemory` when the drawer's block is
    /// refused (nothing is drawn); `Unsupported` on a host that draws no images.
    pub fn draw_image(
        &self,
        image: ImageId,
        rect: FrameRect,
        style: ImageStyle,
    ) -> Result<(), Error> {
        let host = self.host();
        // SAFETY: the callback host outlives this canvas and all inputs are scalars.
        status(unsafe {
            (host.table.canvas_draw_image)(
                host.ctx,
                image.get(),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                style.flags(),
            )
        })
    }
}

/// Where a file to prepare is: the plugin's own data folder (root 0), the
/// library (root 1, needs `uses.storage`) or the plugin's own install folder
/// (root 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrepareRoot {
    /// The plugin's data folder.
    Data,
    /// The library folder.
    Library,
    /// The plugin's own install folder, read only, with no feature: the pictures
    /// it ships. For a plugin built into the image it is empty, and the host answers
    /// `NOT_FOUND` there.
    Bundle,
}

/// A picture file to decode in the background (`command.image_prepare`): JPEG,
/// PNG (not interlaced) or the first frame of a GIF, of at most 8 Mpx. The host
/// answers with one [`ImageReady`] event for the `cookie`.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct PrepareImage<'a> {
    root: PrepareRoot,
    path: &'a str,
    cookie: u32,
    side: u32,
    colour: bool,
    turn: bool,
    tone: Option<&'a [u8; TONE_BYTES]>,
}

impl<'a> PrepareImage<'a> {
    /// Prepare `path` in `root`, decoded to the screen's size, in grey. `cookie`
    /// is the plugin's own number, given back in the event.
    #[must_use]
    pub const fn new(root: PrepareRoot, path: &'a str, cookie: u32) -> Self {
        Self {
            root,
            path,
            cookie,
            side: 0,
            colour: false,
            turn: false,
            tone: None,
        }
    }

    /// The longest side to decode to; 0 is the screen's. A side above 1,600 is
    /// clamped by the host, not refused.
    #[must_use]
    pub const fn side(mut self, side: u32) -> Self {
        self.side = side;
        self
    }

    /// Decode in colour where the screen has it.
    #[must_use]
    pub const fn colour(mut self) -> Self {
        self.colour = true;
        self
    }

    /// Turn a JPEG as its EXIF orientation says while it is decoded (the
    /// size in [`ImageReady`] is the size after the turn). Another format is
    /// not turned.
    #[must_use]
    pub const fn turn(mut self) -> Self {
        self.turn = true;
        self
    }

    /// Map every gray (every colour channel) through `table` while the picture
    /// is decoded: `table[value]` is the new value. The plugin keeps its own
    /// curve; the host defines no brightness or contrast.
    #[must_use]
    pub const fn tone(mut self, table: &'a [u8; TONE_BYTES]) -> Self {
        self.tone = Some(table);
        self
    }

    pub(crate) fn command(&self) -> Result<Command<'a>, Error> {
        if self.path.len() > COMMAND_IMAGE_PREPARE_PATH_MAX_BYTES {
            return Err(Error::Invalid);
        }
        Ok(Command::ImagePrepare {
            root: match self.root {
                PrepareRoot::Data => 0,
                PrepareRoot::Library => 1,
                PrepareRoot::Bundle => 2,
            },
            path: self.path,
            side: self.side,
            cookie: self.cookie,
            flags: if self.colour { COLOUR } else { 0 } | if self.turn { TURN } else { 0 },
            tone: self.tone.map_or(&[], |table| &table[..]),
        })
    }
}

/// How a prepare request ended (`docs/abi/document.md` 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageStatus {
    /// The picture is ready; its id is in the event.
    Ready,
    /// A format the host does not decode (also BMP, interlaced PNG and WebP).
    Unsupported,
    /// The file is damaged. A value a newer host added reads as this.
    Damaged,
    /// Over 8 Mpx, or over a decoder's cap.
    TooLarge,
    /// The card failed: none, or full.
    CardFailure,
    /// Not enough memory now; try again later.
    NoMemory,
    /// No such file at that root and path. This includes root 2 (the install folder) of a plugin
    /// built into the image.
    NotFound,
}

/// A prepare request ended: `event.image_ready`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageReady {
    now_ms: u64,
    cookie: u32,
    status: ImageStatus,
    image: Option<ImageId>,
    width: u32,
    height: u32,
    luma: Option<[u8; LUMA_BYTES]>,
}

impl ImageReady {
    pub(crate) fn decode(r: wire::EventImageReady<'_>) -> Result<Self, Error> {
        let status = match r.status {
            0 => ImageStatus::Ready,
            1 => ImageStatus::Unsupported,
            3 => ImageStatus::TooLarge,
            4 => ImageStatus::CardFailure,
            5 => ImageStatus::NoMemory,
            6 => ImageStatus::NotFound,
            _ => ImageStatus::Damaged,
        };
        // A ready picture has an id; the id of any other answer is not one.
        let ready = status == ImageStatus::Ready;
        let image = if ready {
            Some(ImageId::new(r.image)?)
        } else {
            None
        };
        // The brightness grid comes with a ready picture only: none, or 64 bytes.
        let luma = match r.luma.len() {
            0 => None,
            LUMA_BYTES if ready => <[u8; LUMA_BYTES]>::try_from(r.luma).ok(),
            LUMA_BYTES => None,
            _ => return Err(Error::Invalid),
        };
        Ok(Self {
            now_ms: r.now_ms,
            cookie: r.cookie,
            status,
            image,
            width: if ready { r.width } else { 0 },
            height: if ready { r.height } else { 0 },
            luma,
        })
    }

    /// The host's clock.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The plugin's own number of the request.
    #[must_use]
    pub const fn cookie(&self) -> u32 {
        self.cookie
    }

    /// How it ended.
    #[must_use]
    pub const fn status(&self) -> ImageStatus {
        self.status
    }

    /// The picture, when it is ready. Release it with `image_release` when done.
    #[must_use]
    pub const fn image(&self) -> Option<ImageId> {
        self.image
    }

    /// The width actually made, in pixels (0 unless ready).
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// The height actually made, in pixels (0 unless ready).
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The mean brightness (0 to 255) of an 8 by 8 grid over the prepared
    /// picture, row by row: the plugin works out how bright it is behind any
    /// rectangle from it. `None` unless the picture is ready and the host
    /// made the grid.
    #[must_use]
    pub const fn luma(&self) -> Option<&[u8; LUMA_BYTES]> {
        self.luma.as_ref()
    }
}

#[cfg(test)]
mod tests;
