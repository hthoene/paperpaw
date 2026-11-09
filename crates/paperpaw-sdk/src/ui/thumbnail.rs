//! Pictures made small once and kept in memory ([`Thumbnail`]).

use alloc::vec::Vec;

use paperpaw_document::ImageRef;
use paperpaw_graphics::Canvas;
use paperpaw_graphics::diffuse::Atkinson;
use paperpaw_graphics::round::corner_inset;
use paperpaw_image::{ColorFile, Downscaler, GrayFile, ImageError, ImageFile, ImageSink, luma};
use paperpaw_platform::{PixelFormat, Storage};

/// Bytes of the picture's rows read at a time.
const ROWS_BYTES: usize = 8192;
/// A thumbnail kept on the storage ([`Thumbnail::keep`]): this, its
/// width and height (2 bytes each, little-endian), its bits a pixel (1),
/// its stamp (8), then its pixels. `QIT2` was black and white ordered
/// dithered, `QIT3` black and white alone: made anew.
const MAGIC: &[u8; 4] = b"QIT4";
const HEADER: usize = 17;

/// How a thumbnail keeps its pixels, by the panel it is shown on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    /// Black and white, and four grays beside for frames shown in grays
    /// (see [`Screen::grays`](crate::Screen::grays)), each diffused as
    /// such a panel shows pictures ([`Atkinson`]). Each row holds the
    /// black and white pixels first, eight a byte, the first in the high
    /// bit, set for white; then the grays, four a byte, the first in the
    /// high bits.
    MonoGrays,
    /// Sixteen grays: two pixels a byte, the first in the high bits.
    Gray,
    /// Red, green, blue.
    Rgb,
}

impl Depth {
    fn bits(self) -> u8 {
        match self {
            Depth::MonoGrays => 3,
            Depth::Gray => 4,
            Depth::Rgb => 24,
        }
    }

    fn of_bits(bits: u8) -> Option<Depth> {
        [Depth::MonoGrays, Depth::Gray, Depth::Rgb]
            .into_iter()
            .find(|d| d.bits() == bits)
    }

    /// The bytes of a row `width` pixels wide.
    fn row(self, width: u16) -> usize {
        let width = usize::from(width);
        match self {
            Depth::MonoGrays => width.div_ceil(8) + width.div_ceil(4),
            Depth::Gray => width.div_ceil(2),
            Depth::Rgb => width * 3,
        }
    }
}

/// A picture made small once and kept in memory, to draw it again and
/// again without reading the storage: covers in a list, drawn in every
/// band of every frame. Kept as the panel shows it ([`Thumbnail::new`]):
/// in black and white and in four grays beside, for panels of black and
/// white alone that show frames in grays too (a cover of 70 by 100
/// pixels takes 2.7 KB), in sixteen grays otherwise (3.5 KB), in colour
/// where the panel has colour and the picture too.
///
/// [`draw_image`](crate::ui::draw_image) reads a picture's rows from the
/// storage each time it is drawn; a thumbnail reads it once, when made
/// (e.g. when a page of a list is loaded), each pixel the mean of all it
/// covers.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    width: u16,
    height: u16,
    depth: Depth,
    pixels: Vec<u8>,
}

impl Thumbnail {
    /// `image` made `width` × `height` (at most its own size; pass a size
    /// that keeps its proportions, e.g. from
    /// [`fit_image`](crate::ui::fit_image)), as a panel of `format` shows
    /// it. `None` if it cannot be read or there is no memory for it.
    pub fn new(
        storage: &dyn Storage,
        image: &ImageRef,
        width: u16,
        height: u16,
        format: PixelFormat,
    ) -> Option<Self> {
        let file = ImageFile::open(storage, &image.path)?;
        let depth = if format.is_color() && file.color {
            Depth::Rgb
        } else if format.gray_levels() == Some(2) {
            Depth::MonoGrays
        } else {
            Depth::Gray
        };
        let channels = if file.color { 3 } else { 1 };
        let mut scaler = Downscaler::new(
            file.width,
            file.height,
            (u32::from(width), u32::from(height)),
            channels,
        )
        .ok()?;
        let mut sink = Pack {
            depth,
            width: 0,
            height: 0,
            rows: 0,
            pixels: Vec::new(),
            error: None,
            error_grays: None,
            grays: Vec::new(),
        };
        sink.start(scaler.out_width(), scaler.out_height()).ok()?;
        let row = file.width as usize * channels;
        let rows = (ROWS_BYTES / row.max(1)).max(1);
        let mut data = Vec::new();
        data.try_reserve_exact(rows * row).ok()?;
        data.resize(rows * row, 0);
        let mut y = 0;
        while y < file.height {
            let got = if file.color {
                ColorFile {
                    path: file.path.clone(),
                    width: file.width,
                    height: file.height,
                }
                .read_rows(storage, y, &mut data)
            } else {
                GrayFile {
                    path: file.path.clone(),
                    width: file.width,
                    height: file.height,
                }
                .read_rows(storage, y, &mut data)
            };
            if got == 0 {
                return None;
            }
            for pixels in data.chunks(row).take(got as usize) {
                scaler.row(pixels, &mut sink).ok()?;
            }
            y += got;
        }
        scaler.finish(&mut sink).ok()?;
        Some(Self {
            width: sink.width,
            height: sink.height,
            depth,
            pixels: sink.pixels,
        })
    }

    /// The thumbnail kept at `path` by [`Thumbnail::keep`] with `stamp`,
    /// if it is there: e.g. a cover in a list, read as a few kilobytes
    /// instead of the picture. `fits` is the box it was made for (or a
    /// larger one): a header claiming more is not read, so a file cut
    /// short or another program's takes no memory for what is not there.
    pub fn kept(storage: &dyn Storage, path: &str, stamp: u64, fits: (u16, u16)) -> Option<Self> {
        let mut header = [0u8; HEADER];
        if storage.read_at(path, 0, &mut header).ok()? != HEADER || &header[..4] != MAGIC {
            return None;
        }
        let mut kept_stamp = [0u8; 8];
        kept_stamp.copy_from_slice(&header[9..]);
        if u64::from_le_bytes(kept_stamp) != stamp {
            return None;
        }
        let width = u16::from_le_bytes([header[4], header[5]]);
        let height = u16::from_le_bytes([header[6], header[7]]);
        let depth = Depth::of_bits(header[8])?;
        if width > fits.0 || height > fits.1 {
            return None;
        }
        let len = depth.row(width) * usize::from(height);
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).ok()?;
        pixels.resize(len, 0);
        if storage.read_at(path, HEADER as u64, &mut pixels).ok()? != pixels.len() {
            return None;
        }
        Some(Self {
            width,
            height,
            depth,
            pixels,
        })
    }

    /// Keeps it at `path` with `stamp`, what it was made of and for (e.g.
    /// a hash of the picture's file and the size asked), for
    /// [`Thumbnail::kept`]; whether it was kept.
    pub fn keep(&self, storage: &dyn Storage, path: &str, stamp: u64) -> bool {
        let mut data = Vec::new();
        if data.try_reserve_exact(HEADER + self.pixels.len()).is_err() {
            return false;
        }
        data.extend_from_slice(MAGIC);
        data.extend_from_slice(&self.width.to_le_bytes());
        data.extend_from_slice(&self.height.to_le_bytes());
        data.push(self.depth.bits());
        data.extend_from_slice(&stamp.to_le_bytes());
        data.extend_from_slice(&self.pixels);
        storage.write(path, &data).is_ok()
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// The bytes it keeps.
    pub fn bytes(&self) -> usize {
        self.pixels.len()
    }

    /// Draws it with its top left corner at `x`, `y`: the rows inside the
    /// canvas clip only. A picture made small: it does not count as a
    /// large one that leaves ghosts ([`Canvas::small_pictures`]).
    pub fn draw(&self, canvas: &mut Canvas<'_>, x: u16, y: u16) {
        self.draw_round(canvas, x, y, 0);
    }

    /// Draws it as [`Thumbnail::draw`] does, its corners rounded by
    /// `radius` pixels (e.g. a cover, as a book's): the pixels outside the
    /// arcs are left out, what is behind them stays.
    pub fn draw_round(&self, canvas: &mut Canvas<'_>, x: u16, y: u16, radius: u32) {
        canvas.small_pictures(|canvas| self.draw_rows(canvas, x, y, radius));
    }

    fn draw_rows(&self, canvas: &mut Canvas<'_>, x: u16, y: u16, radius: u32) {
        if canvas.is_blind() {
            let area = paperpaw_platform::Rect::new(x, y, self.width, self.height);
            return canvas.note_picture(area);
        }
        let clip = canvas.clip();
        let top = u32::from(y);
        let first = top.max(u32::from(clip.y));
        let last = (top + u32::from(self.height)).min(clip.bottom());
        if first >= last {
            return;
        }
        let width = usize::from(self.width);
        let stride = self.depth.row(self.width);
        let (w, h) = (u32::from(self.width), u32::from(self.height));
        let r = radius.min(w / 2).min(h / 2);
        // The first and the last pixel of row `n` drawn.
        let span = |n: u32| {
            let inset = if r == 0 {
                0
            } else {
                corner_inset(r, n, h) as usize
            };
            (inset, width - inset)
        };
        if self.depth == Depth::Rgb {
            for row_y in first..last {
                let at = (row_y - top) as usize * stride;
                let (from, to) = span(row_y - top);
                let rgb = &self.pixels[at + from * 3..at + to * 3];
                canvas.draw_rgb_row(i32::from(x) + from as i32, row_y as i32, rgb);
            }
            return;
        }
        let mut row = alloc::vec![0u8; width];
        // A frame in grays shows the grays, one in black and white the
        // black and white pixels.
        let grays = canvas.picture_levels().is_some_and(|levels| levels > 2);
        let mono = width.div_ceil(8);
        for row_y in first..last {
            let at = (row_y - top) as usize * stride;
            let bytes = &self.pixels[at..at + stride];
            for (i, out) in row.iter_mut().enumerate() {
                *out = match self.depth {
                    Depth::MonoGrays if grays => {
                        let byte = bytes[mono + i / 4];
                        ((byte >> (6 - 2 * (i % 4))) & 3) * 85
                    }
                    Depth::MonoGrays => {
                        if bytes[i / 8] & (0x80 >> (i % 8)) != 0 {
                            255
                        } else {
                            0
                        }
                    }
                    _ => {
                        let byte = bytes[i / 2];
                        let level = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };
                        level * 17
                    }
                };
            }
            let (from, to) = span(row_y - top);
            canvas.draw_gray_row(i32::from(x) + from as i32, row_y as i32, &row[from..to]);
        }
    }
}

/// Packs the rows of a picture made small.
struct Pack {
    depth: Depth,
    width: u16,
    height: u16,
    /// Rows packed so far.
    rows: u16,
    pixels: Vec<u8>,
    /// In black and white, the error the rows leave to the next.
    error: Option<Atkinson>,
    /// In four grays, the same.
    error_grays: Option<Atkinson>,
    /// A row's grays, for the diffusion.
    grays: Vec<u8>,
}

impl Pack {
    fn push_gray(&mut self, gray: impl Iterator<Item = u8>) {
        if self.depth == Depth::MonoGrays {
            self.grays.clear();
            self.grays.extend(gray);
            let row = core::mem::take(&mut self.grays);
            // Black and white, one bit a pixel; then four grays, two.
            for (levels, bits) in [(2u8, 1u8), (4, 2)] {
                let error = if levels == 2 {
                    &mut self.error
                } else {
                    &mut self.error_grays
                };
                let mut byte = 0u8;
                let mut n = 0;
                let mut put = |gray: u8, pixels: &mut Vec<u8>| {
                    let level = (u16::from(gray) * u16::from(levels - 1) + 127) / 255;
                    byte |= (level as u8) << (8 - bits * (n + 1));
                    n += 1;
                    if n * bits == 8 {
                        pixels.push(byte);
                        (byte, n) = (0, 0);
                    }
                };
                let pixels = &mut self.pixels;
                match error {
                    Some(error) => error.row(&row, levels, |_, gray| put(gray, pixels)),
                    // Without memory for the diffusion, each rounded.
                    None => row.iter().for_each(|&g| put(g, pixels)),
                }
                if n > 0 {
                    self.pixels.push(byte);
                }
            }
            self.grays = row;
        } else {
            let mut high = None;
            for g in gray {
                // To 16 levels, rounded.
                let level = (u16::from(g) * 15 + 127) / 255;
                match high.take() {
                    None => high = Some(level as u8),
                    Some(h) => self.pixels.push(h << 4 | level as u8),
                }
            }
            if let Some(h) = high {
                self.pixels.push(h << 4);
            }
        }
        self.rows += 1;
    }
}

impl ImageSink for Pack {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        let (w, h) = (
            u16::try_from(width).map_err(|_| ImageError::TooLarge)?,
            u16::try_from(height).map_err(|_| ImageError::TooLarge)?,
        );
        self.width = w;
        self.height = h;
        let bytes = self.depth.row(w) * usize::from(h);
        if self.depth == Depth::MonoGrays {
            self.error = Atkinson::new(usize::from(w));
            self.error_grays = Atkinson::new(usize::from(w));
        }
        self.pixels
            .try_reserve_exact(bytes)
            .map_err(|_| ImageError::OutOfMemory)
    }

    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        if self.depth == Depth::Rgb {
            for &g in gray {
                self.pixels.extend_from_slice(&[g, g, g]);
            }
        } else {
            self.push_gray(gray.iter().copied());
        }
        Ok(())
    }

    fn wants_color(&self) -> bool {
        self.depth == Depth::Rgb
    }

    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        if self.depth == Depth::Rgb {
            self.pixels.extend_from_slice(rgb);
        } else {
            let gray = rgb
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| luma(u32::from(p[0]), u32::from(p[1]), u32::from(p[2])));
            self.push_gray(gray);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use paperpaw_graphics::Framebuffer;
    use paperpaw_image::GrayWriter;
    use paperpaw_platform::{Color, PixelFormat, Rect};

    fn picture(storage: &MemoryStorage) -> ImageRef {
        // 200 × 100: black on the left half, white on the right.
        let mut writer = GrayWriter::new(storage, "p.gray");
        writer.start(200, 100).unwrap();
        let row: Vec<u8> = (0..200).map(|x| if x < 100 { 0 } else { 255 }).collect();
        for _ in 0..100 {
            writer.row(&row).unwrap();
        }
        writer.finish().unwrap();
        ImageRef {
            path: "p.gray".into(),
            width: 200,
            height: 100,
        }
    }

    #[test]
    fn made_small_once_and_drawn_from_memory() {
        let storage = MemoryStorage::new();
        let image = picture(&storage);
        let thumb = Thumbnail::new(&storage, &image, 21, 10, PixelFormat::Gray8).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (21, 10));
        // Two pixels a byte, rows whole bytes.
        assert_eq!(thumb.bytes(), 11 * 10);
        // Gone from the storage: drawn from memory all the same.
        storage.remove("p.gray").unwrap();
        let mut frame = Framebuffer::new(40, 20, PixelFormat::Gray8).unwrap();
        let mut canvas = frame.canvas();
        canvas.clear(Color::WHITE);
        thumb.draw(&mut canvas, 5, 5);
        let view = frame.view();
        assert_eq!(view.pixel(5, 5), Color::BLACK);
        assert_eq!(view.pixel(25, 14), Color::WHITE);
        // Outside it, untouched.
        assert_eq!(view.pixel(4, 5), Color::WHITE);
        // Only the rows in the clip.
        let mut frame = Framebuffer::new(40, 20, PixelFormat::Gray8).unwrap();
        let mut canvas = frame.canvas();
        canvas.clear(Color::WHITE);
        thumb.draw(&mut canvas.clipped(Rect::new(0, 0, 40, 8)), 5, 5);
        assert_eq!(frame.view().pixel(5, 7), Color::BLACK);
        assert_eq!(frame.view().pixel(5, 9), Color::WHITE);
    }

    #[test]
    fn kept_and_read_again_with_its_stamp() {
        let storage = MemoryStorage::new();
        let image = picture(&storage);
        let made = Thumbnail::new(&storage, &image, 21, 10, PixelFormat::Gray8).unwrap();
        assert!(made.keep(&storage, "p.thumb", 7));
        assert_eq!(
            storage.size("p.thumb").unwrap() as usize,
            HEADER + made.bytes()
        );
        // Read from its own file, the picture gone.
        storage.remove("p.gray").unwrap();
        let read = Thumbnail::kept(&storage, "p.thumb", 7, (21, 10)).unwrap();
        assert_eq!(
            (read.width, read.height, read.depth, &read.pixels),
            (made.width, made.height, made.depth, &made.pixels)
        );
        // Another stamp, a broken file, none: not kept.
        assert!(Thumbnail::kept(&storage, "p.thumb", 8, (21, 10)).is_none());
        storage.write("p.thumb", b"QIT1").unwrap();
        assert!(Thumbnail::kept(&storage, "p.thumb", 7, (21, 10)).is_none());
        assert!(Thumbnail::kept(&storage, "none.thumb", 7, (21, 10)).is_none());
    }

    #[test]
    fn kept_in_black_and_white_for_such_panels() {
        let storage = MemoryStorage::new();
        let image = picture(&storage);
        let thumb = Thumbnail::new(&storage, &image, 21, 10, PixelFormat::Mono1).unwrap();
        // Eight pixels a byte, then four, rows whole bytes.
        assert_eq!(thumb.bytes(), (3 + 6) * 10);
        assert!(thumb.keep(&storage, "m.thumb", 1));
        let read = Thumbnail::kept(&storage, "m.thumb", 1, (21, 10)).unwrap();
        assert_eq!((read.depth, read.bytes()), (Depth::MonoGrays, 90));
        // In black and white, and in a frame in grays.
        for format in [PixelFormat::Mono1, PixelFormat::Gray2] {
            let mut frame = Framebuffer::new(30, 12, format).unwrap();
            let mut canvas = frame.canvas();
            canvas.clear(Color::WHITE);
            read.draw(&mut canvas, 2, 1);
            let view = frame.view();
            assert_eq!(view.pixel(2, 1), Color::BLACK, "{format:?}");
            assert_eq!(view.pixel(22, 10), Color::WHITE, "{format:?}");
        }
    }

    #[test]
    fn a_gray_shows_in_grays_where_the_frame_has_them() {
        let storage = MemoryStorage::new();
        let mut writer = GrayWriter::new(&storage, "g.gray");
        writer.start(8, 8).unwrap();
        for _ in 0..8 {
            writer.row(&[85; 8]).unwrap();
        }
        writer.finish().unwrap();
        let image = ImageRef {
            path: "g.gray".into(),
            width: 8,
            height: 8,
        };
        let thumb = Thumbnail::new(&storage, &image, 8, 8, PixelFormat::Mono1).unwrap();
        let mut frame = Framebuffer::new(8, 8, PixelFormat::Gray2).unwrap();
        thumb.draw(&mut frame.canvas(), 0, 0);
        let view = frame.view();
        assert!(
            (0..8).all(|y| (0..8).all(|x| view.pixel(x, y) == Color::gray(85))),
            "a level of four grays stays as it is"
        );
    }

    #[test]
    fn rounded_corners_leave_what_is_behind() {
        let storage = MemoryStorage::new();
        let image = picture(&storage);
        // Black on the left half, white on the right.
        let thumb = Thumbnail::new(&storage, &image, 40, 20, PixelFormat::Mono1).unwrap();
        let mut frame = Framebuffer::new(50, 30, PixelFormat::Mono1).unwrap();
        let mut canvas = frame.canvas();
        // Behind it: white on the left, black on the right.
        canvas.clear(Color::WHITE);
        canvas.fill(Rect::new(25, 0, 25, 30), Color::BLACK);
        thumb.draw_round(&mut canvas, 5, 5, 4);
        let view = frame.view();
        // In the corners, what is behind.
        assert_eq!(view.pixel(5, 5), Color::WHITE);
        assert_eq!(view.pixel(5, 24), Color::WHITE);
        assert_eq!(view.pixel(44, 5), Color::BLACK);
        assert_eq!(view.pixel(44, 24), Color::BLACK);
        // Along the edges and within, the picture.
        assert_eq!(view.pixel(5, 14), Color::BLACK);
        assert_eq!(view.pixel(9, 5), Color::BLACK);
        assert_eq!(view.pixel(44, 14), Color::WHITE);
        assert_eq!(view.pixel(40, 5), Color::WHITE);
    }

    #[test]
    fn unreadable_pictures_make_none() {
        let storage = MemoryStorage::new();
        let image = ImageRef {
            path: "none.gray".into(),
            width: 10,
            height: 10,
        };
        assert!(Thumbnail::new(&storage, &image, 5, 5, PixelFormat::Gray8).is_none());
    }
}
