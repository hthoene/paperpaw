//! Images for e-ink: JPEG, PNG and GIF decoded to grayscale, row by row.
//!
//! Decoders pull bytes from a [`Source`] and push rows of 8-bit gray to an
//! [`ImageSink`], downscaling while they decode, so memory stays at a few
//! rows (plus 43 KiB for PNG's inflate state) whatever the image size.
//! Brightness is all e-ink shows, so colour images are converted on the
//! fly; a sink that wants colour ([`ImageSink::wants_color`], for colour
//! screens, ADR 0015) gets RGB rows instead ([`ColorFile`]).
//!
//! [`GrayFile`] stores decoded images in a simple format (`PPGI`, width
//! and height as u32, then rows), so readers draw them straight from
//! storage without decoding again.
//!
//! Supported: baseline and progressive JPEG (1 or 3 components, any
//! sampling; progressive ones keep their coefficients until the last
//! scan, as few as the output size needs), non-interlaced PNG (all
//! colour types and depths; transparency over white, or kept for a sink
//! that wants it, [`ImageSink::wants_alpha`], e.g. [`AlphaFile`]) and GIF
//! (the first picture of the file, transparency over white). Interlaced
//! PNG and other formats return [`ImageError::Unsupported`].

#![no_std]

extern crate alloc;

mod alpha;
mod color;
mod gif;
mod gray;
mod jpeg;
mod png;
mod png_write;
mod progressive;

use core::fmt;

pub use alpha::{AlphaFile, AlphaWriter};
pub use color::{ColorFile, ColorWriter};
pub use gray::{GrayFile, GrayWriter};
pub use png_write::PngWriter;

/// Why an image could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageError {
    /// The data ended early or is malformed.
    Invalid(&'static str),
    /// A valid image in a variant this decoder does not handle.
    Unsupported(&'static str),
    /// Reading the source or writing the result failed.
    Io,
    /// Writing to the storage failed, as it says (see [`PngWriter`]).
    Storage(paperpaw_platform::StorageError),
    OutOfMemory,
    /// The sink stopped decoding.
    Stopped,
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::Invalid(what) => write!(f, "invalid image: {what}"),
            ImageError::Unsupported(what) => write!(f, "unsupported image: {what}"),
            ImageError::Io => f.write_str("image could not be read or written"),
            ImageError::Storage(e) => write!(f, "image could not be written: {e}"),
            ImageError::OutOfMemory => f.write_str("not enough memory for the image"),
            ImageError::Stopped => f.write_str("decoding stopped"),
        }
    }
}

/// Where encoded image bytes come from.
pub trait Source {
    /// Reads up to `buf.len()` bytes; 0 at the end.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError>;
}

impl Source for &[u8] {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError> {
        let n = buf.len().min(self.len());
        buf[..n].copy_from_slice(&self[..n]);
        *self = &self[n..];
        Ok(n)
    }
}

/// Receives a decoded image.
pub trait ImageSink {
    /// The size of the (downscaled) image, before its first row.
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError>;
    /// The next row, `width` bytes of gray (0 black, 255 white).
    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError>;

    /// Whether rows are wanted in colour ([`ImageSink::row_rgb`]), where
    /// the image has colour; gray otherwise.
    fn wants_color(&self) -> bool {
        false
    }

    /// The next row in colour, `width` pixels of three bytes (red, green,
    /// blue). Only for sinks that want colour; otherwise it is passed on
    /// as gray.
    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        let gray: alloc::vec::Vec<u8> = rgb
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| luma(u32::from(p[0]), u32::from(p[1]), u32::from(p[2])))
            .collect();
        self.row(&gray)
    }

    /// Whether rows are wanted with transparency
    /// ([`ImageSink::row_alpha`]), where the image has it (PNG); over
    /// white otherwise. It goes before colour: such rows are gray.
    fn wants_alpha(&self) -> bool {
        false
    }

    /// The next row with transparency, `width` pixels of two bytes: gray,
    /// and how opaque it is (0 transparent, 255 opaque). Only for sinks
    /// that want it; otherwise it is passed on over white.
    fn row_alpha(&mut self, gray_alpha: &[u8]) -> Result<(), ImageError> {
        let gray: alloc::vec::Vec<u8> = gray_alpha
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| over_white(p[0], p[1]))
            .collect();
        self.row(&gray)
    }
}

/// Gray `gray` with opacity `alpha` over white.
pub fn over_white(gray: u8, alpha: u8) -> u8 {
    let (g, a) = (u32::from(gray), u32::from(alpha));
    ((g * a + 255 * (255 - a) + 127) / 255) as u8
}

/// The gray of a colour, as e-ink shows it (ITU-R 601).
pub fn luma(r: u32, g: u32, b: u32) -> u8 {
    ((r * 299 + g * 587 + b * 114 + 500) / 1000) as u8
}

/// Image formats by their first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    Jpeg,
    Png,
    Gif,
}

/// The width and height of a JPEG, PNG or GIF from its first bytes
/// (`head`), without decoding it: e.g. to lay a page out before its
/// pictures are decoded. `None` if `head` does not say (another format,
/// or a JPEG whose size comes after more than `head` holds).
pub fn dimensions(head: &[u8]) -> Option<(u32, u32)> {
    let u16_be = |at: usize| {
        Some(u32::from(u16::from_be_bytes([
            *head.get(at)?,
            *head.get(at + 1)?,
        ])))
    };
    let u16_le = |at: usize| {
        Some(u32::from(u16::from_le_bytes([
            *head.get(at)?,
            *head.get(at + 1)?,
        ])))
    };
    let u32_be = |at: usize| {
        Some(u32::from_be_bytes([
            *head.get(at)?,
            *head.get(at + 1)?,
            *head.get(at + 2)?,
            *head.get(at + 3)?,
        ]))
    };
    let size = match Format::detect(head)? {
        Format::Png => (u32_be(16)?, u32_be(20)?),
        Format::Gif => (u16_le(6)?, u16_le(8)?),
        Format::Jpeg => {
            // Segment by segment to the frame's header.
            let mut at = 2;
            loop {
                if *head.get(at)? != 0xff {
                    return None;
                }
                while *head.get(at)? == 0xff {
                    at += 1;
                }
                let marker = *head.get(at)?;
                at += 1;
                match marker {
                    0xd8 | 0x01 | 0xd0..=0xd7 => continue,
                    0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                        break (u16_be(at + 5)?, u16_be(at + 3)?);
                    }
                    _ => at += usize::try_from(u16_be(at)?).ok()?,
                }
            }
        }
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

impl Format {
    pub fn detect(head: &[u8]) -> Option<Format> {
        if head.starts_with(&[0xff, 0xd8]) {
            Some(Format::Jpeg)
        } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Format::Png)
        } else if head.starts_with(b"GIF8") {
            Some(Format::Gif)
        } else {
            None
        }
    }
}

/// The size pictures are decoded to at most: as large as they show on a
/// screen of these sides, upright or turned, and never larger than they
/// are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub short: u32,
    pub long: u32,
}

impl Fit {
    /// A screen of `width` × `height` pixels, either way up.
    pub const fn screen(width: u32, height: u32) -> Self {
        if width < height {
            Self {
                short: width,
                long: height,
            }
        } else {
            Self {
                short: height,
                long: width,
            }
        }
    }

    /// At most `side` pixels on either side.
    pub const fn side(side: u32) -> Self {
        Self {
            short: side,
            long: side,
        }
    }

    /// What fits both.
    pub fn min(self, other: Fit) -> Fit {
        Fit {
            short: self.short.min(other.short),
            long: self.long.min(other.long),
        }
    }

    /// The size a picture of `width` × `height` is decoded to: as large as
    /// it shows on the screen upright or turned, whichever is larger, in
    /// its proportions.
    pub fn size(&self, width: u32, height: u32) -> (u32, u32) {
        let upright = scaled(width, height, self.short, self.long);
        let turned = scaled(width, height, self.long, self.short);
        if turned.0 > upright.0 {
            turned
        } else {
            upright
        }
    }
}

/// `width` × `height` made just small enough for `max_width` ×
/// `max_height`, in its proportions (a pixel at least on each side).
fn scaled(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let (max_width, max_height) = (max_width.max(1), max_height.max(1));
    if width == 0 || height == 0 || (width <= max_width && height <= max_height) {
        return (width, height);
    }
    let (w, h) = (u64::from(width), u64::from(height));
    let (mw, mh) = (u64::from(max_width), u64::from(max_height));
    if w * mh > h * mw {
        // As wide as it may be.
        let h = ((h * mw + w / 2) / w).clamp(1, mh);
        (max_width, h as u32)
    } else {
        let w = ((w * mh + h / 2) / h).clamp(1, mw);
        (w as u32, max_height)
    }
}

/// The pixels of each 8×8 block's side a JPEG is decoded at (8, 4, 2 or
/// 1): the fewest that still give `size` at least.
pub(crate) fn block_side(width: u32, height: u32, size: (u32, u32)) -> usize {
    for n in [1u32, 2, 4] {
        let scale = 8 / n;
        if width.div_ceil(scale) >= size.0 && height.div_ceil(scale) >= size.1 {
            return n as usize;
        }
    }
    8
}

/// Decodes an image of `format`, made as small as `fit` allows.
pub fn decode(
    format: Format,
    source: &mut dyn Source,
    fit: Fit,
    sink: &mut dyn ImageSink,
) -> Result<(), ImageError> {
    let mut reader = Reader::new(source)?;
    match format {
        Format::Jpeg => jpeg::decode(&mut reader, fit, sink),
        Format::Png => png::decode(&mut reader, fit, sink),
        Format::Gif => gif::decode(&mut reader, fit, sink),
    }
}

/// Buffered byte reader over a [`Source`].
pub(crate) struct Reader<'a> {
    source: &'a mut dyn Source,
    /// Read several blocks of a card at a time. On the heap: decoding
    /// takes much of the stack as it is.
    buf: alloc::boxed::Box<[u8]>,
    pos: usize,
    len: usize,
    /// Bytes of the source before those in `buf`.
    before: u64,
}

impl<'a> Reader<'a> {
    fn new(source: &'a mut dyn Source) -> Result<Self, ImageError> {
        Ok(Self {
            source,
            buf: zeroed(4096)?.into_boxed_slice(),
            pos: 0,
            len: 0,
            before: 0,
        })
    }

    /// Bytes taken from the source so far.
    pub(crate) fn position(&self) -> u64 {
        self.before + self.pos as u64
    }

    /// The next byte, `None` at the end.
    fn next(&mut self) -> Result<Option<u8>, ImageError> {
        if self.pos == self.len {
            self.before += self.len as u64;
            (self.pos, self.len) = (0, 0);
            // A source that says it read more than asked for is not believed.
            self.len = self.source.read(&mut self.buf)?.min(self.buf.len());
            if self.len == 0 {
                return Ok(None);
            }
        }
        self.pos += 1;
        Ok(Some(self.buf[self.pos - 1]))
    }

    fn byte(&mut self) -> Result<u8, ImageError> {
        self.next()?.ok_or(ImageError::Invalid("unexpected end"))
    }

    fn u16_be(&mut self) -> Result<u16, ImageError> {
        Ok(u16::from(self.byte()?) << 8 | u16::from(self.byte()?))
    }

    fn u32_be(&mut self) -> Result<u32, ImageError> {
        Ok(u32::from(self.u16_be()?) << 16 | u32::from(self.u16_be()?))
    }

    fn skip(&mut self, mut n: usize) -> Result<(), ImageError> {
        while n > 0 {
            if self.pos == self.len {
                self.byte()?;
                self.pos -= 1;
            }
            let step = n.min(self.len - self.pos);
            self.pos += step;
            n -= step;
        }
        Ok(())
    }

    fn fill(&mut self, out: &mut [u8]) -> Result<(), ImageError> {
        for b in out {
            *b = self.byte()?;
        }
        Ok(())
    }
}

/// A zeroed buffer, or [`ImageError::OutOfMemory`].
pub(crate) fn zeroed<T: Clone + Default>(len: usize) -> Result<alloc::vec::Vec<T>, ImageError> {
    let mut v = alloc::vec::Vec::new();
    v.try_reserve_exact(len)
        .map_err(|_| ImageError::OutOfMemory)?;
    v.resize(len, T::default());
    Ok(v)
}

/// Makes rows smaller, to a size no larger than theirs: each pixel the
/// mean of the pixels it covers, of gray, gray with transparency (two
/// channels, the gray weighted by how opaque each pixel is) or (three)
/// RGB.
#[derive(Debug)]
pub struct Downscaler {
    width: u32,
    height: u32,
    channels: usize,
    out_width: u32,
    out_height: u32,
    /// The pixel of the smaller row each pixel goes to.
    columns: alloc::vec::Vec<u16>,
    /// Pixels of a row each pixel of the smaller one covers.
    counts: alloc::vec::Vec<u16>,
    sums: alloc::vec::Vec<u32>,
    /// Rows summed, and rows come.
    rows: u32,
    seen: u32,
    out: alloc::vec::Vec<u8>,
}

impl Downscaler {
    /// For `width` × `height` made `size` (at most as large), in rows of
    /// `channels` bytes a pixel: 1 (gray), 2 (gray and opacity) or 3
    /// (RGB).
    pub fn new(
        width: u32,
        height: u32,
        size: (u32, u32),
        channels: usize,
    ) -> Result<Self, ImageError> {
        let out_width = size.0.clamp(1, width.max(1));
        let out_height = size.1.clamp(1, height.max(1));
        if out_width > u32::from(u16::MAX) {
            return Err(ImageError::Unsupported("image this large"));
        }
        if !matches!(channels, 1..=3) {
            return Err(ImageError::Unsupported("channels"));
        }
        // Each smaller pixel sums all it covers in 32 bits (gray times
        // opacity, with transparency), and counts the columns it covers
        // in 16.
        let across = u64::from(width.div_ceil(out_width));
        let down = u64::from(height.div_ceil(out_height));
        let most = if channels == 2 { 255 * 255 } else { 255 };
        if across > u64::from(u16::MAX) || across * down * most > u64::from(u32::MAX) {
            return Err(ImageError::Unsupported("image this large made this small"));
        }
        let mut columns = alloc::vec::Vec::new();
        let mut counts: alloc::vec::Vec<u16> = zeroed(out_width as usize)?;
        if out_width != width {
            columns
                .try_reserve_exact(width as usize)
                .map_err(|_| ImageError::OutOfMemory)?;
            for x in 0..u64::from(width) {
                let to = (x * u64::from(out_width) / u64::from(width)) as usize;
                columns.push(to as u16);
                counts[to] = counts[to].saturating_add(1);
            }
        } else {
            counts.fill(1);
        }
        let len = out_width as usize * channels;
        Ok(Self {
            width,
            height,
            channels,
            out_width,
            out_height,
            columns,
            counts,
            sums: zeroed(len)?,
            rows: 0,
            seen: 0,
            out: zeroed(len)?,
        })
    }

    pub fn out_width(&self) -> u32 {
        self.out_width
    }

    pub fn out_height(&self) -> u32 {
        self.out_height
    }

    /// The row of the smaller image row `y` goes to.
    fn out_row(&self, y: u32) -> u64 {
        u64::from(y) * u64::from(self.out_height) / u64::from(self.height.max(1))
    }

    fn emit(&self, row: &[u8], sink: &mut dyn ImageSink) -> Result<(), ImageError> {
        match self.channels {
            3 => sink.row_rgb(row),
            2 => sink.row_alpha(row),
            _ => sink.row(row),
        }
    }

    /// The first `width` pixels of `pixels`; an error if it is shorter.
    fn whole<'p>(&self, pixels: &'p [u8]) -> Result<&'p [u8], ImageError> {
        pixels
            .get(..self.width as usize * self.channels)
            .ok_or(ImageError::Invalid("row shorter than the image"))
    }

    /// Adds a row; emits a smaller one when the rows it covers are in.
    pub fn row(&mut self, pixels: &[u8], sink: &mut dyn ImageSink) -> Result<(), ImageError> {
        if self.seen >= self.height {
            return Ok(());
        }
        let pixels = self.whole(pixels)?;
        self.seen += 1;
        if self.out_width == self.width && self.out_height == self.height {
            return self.emit(pixels, sink);
        }
        self.add(pixels, 1);
        if self.seen == self.height || self.out_row(self.seen) != self.out_row(self.seen - 1) {
            self.flush(sink)?;
        }
        Ok(())
    }

    /// Adds `count` rows of the same `pixels`, as that many calls of
    /// [`Downscaler::row`] would, but each run of them going to the same
    /// smaller row summed at once: for images whose rows below some point
    /// are all alike (e.g. where the data of a JPEG ends early).
    pub(crate) fn rows_alike(
        &mut self,
        pixels: &[u8],
        mut count: u32,
        sink: &mut dyn ImageSink,
    ) -> Result<(), ImageError> {
        let pixels = self.whole(pixels)?;
        while count > 0 && self.seen < self.height {
            if self.out_width == self.width && self.out_height == self.height {
                self.row(pixels, sink)?;
                count -= 1;
                continue;
            }
            // The rows from here on that go to the same smaller row (at
            // least this one).
            let out_row = self.out_row(self.seen);
            let end = ((out_row + 1) * u64::from(self.height))
                .div_ceil(u64::from(self.out_height))
                .min(u64::from(self.height));
            let run = (end - u64::from(self.seen)).min(u64::from(count)) as u32;
            self.add(pixels, run);
            self.seen += run;
            count -= run;
            if self.seen == self.height || self.out_row(self.seen) != self.out_row(self.seen - 1) {
                self.flush(sink)?;
            }
        }
        Ok(())
    }

    /// Sums `times` rows of `pixels` (whole ones) into the smaller row.
    #[inline]
    fn add(&mut self, pixels: &[u8], times: u32) {
        let ch = self.channels;
        if ch == 2 {
            // The gray weighted by how opaque it is: a transparent pixel
            // lends the mean no gray of its own.
            for (x, pixel) in pixels.as_chunks::<2>().0.iter().enumerate() {
                let to = if self.columns.is_empty() {
                    x
                } else {
                    usize::from(self.columns[x])
                };
                let (g, a) = (u32::from(pixel[0]), u32::from(pixel[1]));
                self.sums[to * 2] += g * a * times;
                self.sums[to * 2 + 1] += a * times;
            }
        } else if self.columns.is_empty() {
            for (sum, &v) in self.sums.iter_mut().zip(pixels) {
                *sum += u32::from(v) * times;
            }
        } else if ch == 1 {
            for (&to, &v) in self.columns.iter().zip(pixels) {
                self.sums[usize::from(to)] += u32::from(v) * times;
            }
        } else {
            for (&to, pixel) in self.columns.iter().zip(pixels.chunks_exact(ch)) {
                let at = usize::from(to) * ch;
                for (sum, &v) in self.sums[at..at + ch].iter_mut().zip(pixel) {
                    *sum += u32::from(v) * times;
                }
            }
        }
        self.rows += times;
    }

    /// Emits a partial last row, if any.
    pub fn finish(&mut self, sink: &mut dyn ImageSink) -> Result<(), ImageError> {
        if self.rows > 0 {
            self.flush(sink)?;
        }
        Ok(())
    }

    fn flush(&mut self, sink: &mut dyn ImageSink) -> Result<(), ImageError> {
        let ch = self.channels;
        if ch == 2 {
            for (i, out) in self.out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let covered = u32::from(self.counts[i]).max(1) * self.rows;
                let (weighted, opacity) = (self.sums[i * 2], self.sums[i * 2 + 1]);
                out[0] = weighted.checked_div(opacity).map_or(255, |g| g as u8);
                out[1] = (opacity / covered) as u8;
            }
        } else {
            for (i, out) in self.out.iter_mut().enumerate() {
                let covered = u32::from(self.counts[i / ch]).max(1) * self.rows;
                *out = (self.sums[i] / covered) as u8;
            }
        }
        self.sums.fill(0);
        self.rows = 0;
        let out = core::mem::take(&mut self.out);
        let result = self.emit(&out, sink);
        self.out = out;
        result
    }
}

/// A decoded image in storage, gray ([`GrayFile`]), colour
/// ([`ColorFile`]) or gray with transparency ([`AlphaFile`]), whichever
/// the file is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageFile {
    pub path: alloc::string::String,
    pub width: u32,
    pub height: u32,
    pub color: bool,
    /// Gray with transparency ([`AlphaFile`]).
    pub alpha: bool,
}

impl ImageFile {
    /// Made of its parts.
    pub fn new(path: &str, width: u32, height: u32, color: bool, alpha: bool) -> Self {
        Self {
            path: alloc::string::String::from(path),
            width,
            height,
            color,
            alpha,
        }
    }

    /// Opens a decoded image of any kind, checking its size.
    pub fn open(storage: &dyn paperpaw_platform::Storage, path: &str) -> Option<ImageFile> {
        Some(match Decoded::open(storage, path)? {
            Decoded::Gray(file) => {
                ImageFile::new(&file.path, file.width, file.height, false, false)
            }
            Decoded::Color(file) => {
                ImageFile::new(&file.path, file.width, file.height, true, false)
            }
            Decoded::Alpha(file) => {
                ImageFile::new(&file.path, file.width, file.height, false, true)
            }
        })
    }
}

/// A decoded image in storage, of the kind its first bytes name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decoded {
    Gray(GrayFile),
    Color(ColorFile),
    Alpha(AlphaFile),
}

impl Decoded {
    /// Opens a decoded image of any kind: its header read once, its size
    /// checked.
    pub fn open(storage: &dyn paperpaw_platform::Storage, path: &str) -> Option<Decoded> {
        let (decoded, bytes) = Self::header(storage, path)?;
        let (width, height) = decoded.size();
        let expected = (u64::from(width) * u64::from(height))
            .checked_mul(bytes)?
            .checked_add(12)?;
        (storage.size(path).ok()? == expected).then_some(decoded)
    }

    /// The image whose header is at `path`, trusting it, its size not
    /// checked: for drawing band by band, where one read is all it may
    /// take (a file cut short only reads short).
    pub fn peek(storage: &dyn paperpaw_platform::Storage, path: &str) -> Option<Decoded> {
        Self::header(storage, path).map(|(decoded, _)| decoded)
    }

    /// The image the header names, and its bytes a pixel.
    fn header(storage: &dyn paperpaw_platform::Storage, path: &str) -> Option<(Decoded, u64)> {
        let mut header = [0u8; 12];
        if !gray::read_all(storage, path, 0, &mut header) {
            return None;
        }
        let width = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let height = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        if width == 0 || height == 0 {
            return None;
        }
        let path = alloc::string::String::from(path);
        Some(match &header[..4] {
            b"PPGI" => (
                Decoded::Gray(GrayFile {
                    path,
                    width,
                    height,
                }),
                1,
            ),
            b"PPCI" => (
                Decoded::Color(ColorFile {
                    path,
                    width,
                    height,
                }),
                3,
            ),
            b"PPGA" => (Decoded::Alpha(AlphaFile::new(&path, width, height)), 2),
            _ => return None,
        })
    }

    /// Its width and height.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Decoded::Gray(f) => (f.width, f.height),
            Decoded::Color(f) => (f.width, f.height),
            Decoded::Alpha(f) => (f.width, f.height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[derive(Default)]
    struct Rows(Vec<Vec<u8>>);

    impl ImageSink for Rows {
        fn start(&mut self, _: u32, _: u32) -> Result<(), ImageError> {
            Ok(())
        }
        fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
            self.0.push(gray.to_vec());
            Ok(())
        }
        fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
            self.0.push(rgb.to_vec());
            Ok(())
        }
    }

    #[test]
    fn rows_alike_as_row_by_row() {
        for (width, height, size, channels) in [
            (7, 23, (3, 5), 1),
            (7, 23, (7, 4), 3),
            (5, 9, (5, 9), 1),
            (16, 100, (16, 7), 1),
            (9, 64, (2, 64), 3),
        ] {
            let mut one = Downscaler::new(width, height, size, channels).unwrap();
            let mut runs = Downscaler::new(width, height, size, channels).unwrap();
            let (mut a, mut b) = (Rows::default(), Rows::default());
            let len = width as usize * channels;
            // A few rows each, then runs of alike ones of several lengths.
            let mut y = 0;
            for (count, value) in [(1u32, 10u8), (3, 200), (1, 7), (11, 90), (50, 255), (99, 3)] {
                let row: Vec<u8> = (0..len)
                    .map(|x| value.wrapping_add((x as u8).wrapping_mul(13)))
                    .collect();
                for _ in 0..count {
                    one.row(&row, &mut a).unwrap();
                }
                runs.rows_alike(&row, count, &mut b).unwrap();
                y += count;
                assert_eq!(a.0, b.0, "{width}x{height} to {size:?} after {y} rows");
            }
            one.finish(&mut a).unwrap();
            runs.finish(&mut b).unwrap();
            assert_eq!(a.0, b.0);
            assert_eq!(a.0.len(), runs.out_height() as usize);
        }
    }
}
