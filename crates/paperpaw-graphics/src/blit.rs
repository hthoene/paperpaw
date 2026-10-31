//! Packed images drawn fast, scaled by whole numbers
//! ([`Canvas::blit`](crate::Canvas::blit)): for what is drawn every frame,
//! such as the screen of an emulated game.
//!
//! The work that does not change from pixel to pixel is done once per
//! call: the stored value of every value of the image, with the ordered
//! dither's pattern where the frame has few grays. On frames of a few
//! bits a pixel, a row is then put together in words of 32 bits and
//! written as whole bytes.

use core::fmt;

use paperpaw_platform::{Color, PixelFormat, Point, Rect};

use crate::framebuffer::Framebuffer;

/// How the pixels of a [`Bitmap`] are packed: so many bits a pixel, the
/// leftmost pixel in a byte's highest bits, each row starting on a byte,
/// each pixel's value an index into the bitmap's palette. Formats are
/// constants of one type, so more can come without a new plugin
/// interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BitmapFormat {
    bits: u8,
    /// What a value means: 0, a gray (through the palette); others to
    /// come.
    kind: u8,
}

impl BitmapFormat {
    /// 1 bit a pixel, 8 pixels a byte.
    pub const GRAY1: Self = Self::gray(1);
    /// 2 bits a pixel, 4 pixels a byte.
    pub const GRAY2: Self = Self::gray(2);
    /// 4 bits a pixel, 2 pixels a byte.
    pub const GRAY4: Self = Self::gray(4);
    /// 8 bits a pixel.
    pub const GRAY8: Self = Self::gray(8);

    const fn gray(bits: u8) -> Self {
        Self { bits, kind: 0 }
    }

    pub const fn bits_per_pixel(self) -> u8 {
        self.bits
    }

    /// How many values a pixel can have (2 to 256).
    pub const fn values(self) -> u16 {
        1 << self.bits
    }

    /// Bytes of a row of `width` pixels, packed tight.
    pub const fn stride(self, width: u16) -> usize {
        (width as usize * self.bits as usize).div_ceil(8)
    }
}

/// Why a [`Bitmap`] cannot be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BitmapError {
    /// Its width or height is 0.
    ZeroSize,
    /// Rows `stride` bytes apart are shorter than a row's pixels.
    StrideTooSmall { stride: usize, needed: usize },
    /// The data holds `len` bytes, its rows `needed`.
    DataTooShort { len: usize, needed: usize },
    /// The palette has `len` grays for `needed` values.
    PaletteTooShort { len: usize, needed: usize },
}

impl fmt::Display for BitmapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BitmapError::ZeroSize => write!(f, "bitmap of no pixels"),
            BitmapError::StrideTooSmall { stride, needed } => {
                write!(f, "rows of {stride} bytes hold no row of {needed}")
            }
            BitmapError::DataTooShort { len, needed } => {
                write!(f, "bitmap data of {len} bytes, {needed} needed")
            }
            BitmapError::PaletteTooShort { len, needed } => {
                write!(f, "palette of {len} grays, {needed} needed")
            }
        }
    }
}

impl core::error::Error for BitmapError {}

/// A packed image that borrows its pixels, drawn by
/// [`Canvas::blit`](crate::Canvas::blit).
///
/// A pixel's value is an index into its palette, a gray (0 black, 255
/// white) for each value; without one, 0 is black, the largest value
/// white, and those between the grays between, evenly spaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bitmap<'a> {
    format: BitmapFormat,
    width: u16,
    height: u16,
    stride: usize,
    data: &'a [u8],
    palette: Option<&'a [u8]>,
}

impl<'a> Bitmap<'a> {
    /// A `width`×`height` image of `format` in `data`, its rows packed
    /// tight one after the other.
    pub fn new(
        format: BitmapFormat,
        width: u16,
        height: u16,
        data: &'a [u8],
    ) -> Result<Self, BitmapError> {
        if width == 0 || height == 0 {
            return Err(BitmapError::ZeroSize);
        }
        Self {
            format,
            width,
            height,
            stride: format.stride(width),
            data,
            palette: None,
        }
        .checked()
    }

    /// This image with its rows `stride` bytes apart, e.g. a part of a
    /// larger one; the last row need not be followed by the rest.
    pub fn stride(self, stride: usize) -> Result<Self, BitmapError> {
        let needed = self.format.stride(self.width);
        if stride < needed {
            return Err(BitmapError::StrideTooSmall { stride, needed });
        }
        Self { stride, ..self }.checked()
    }

    /// This image with its values shown as the grays of `palette` (0
    /// black to 255 white), one for each value its format has.
    pub fn palette(self, palette: &'a [u8]) -> Result<Self, BitmapError> {
        let needed = usize::from(self.format.values());
        if palette.len() < needed {
            return Err(BitmapError::PaletteTooShort {
                len: palette.len(),
                needed,
            });
        }
        Ok(Self {
            palette: Some(palette),
            ..self
        })
    }

    pub fn format(&self) -> BitmapFormat {
        self.format
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Itself, if its data holds all its rows.
    fn checked(self) -> Result<Self, BitmapError> {
        let row = self.format.stride(self.width);
        let needed = self.stride * (usize::from(self.height) - 1) + row;
        if self.data.len() < needed {
            return Err(BitmapError::DataTooShort {
                len: self.data.len(),
                needed,
            });
        }
        Ok(self)
    }

    /// The bytes of row `y` on.
    fn row(&self, y: usize) -> &'a [u8] {
        self.data.get(y * self.stride..).unwrap_or(&[])
    }

    /// The gray of every value.
    fn grays(&self) -> [u8; 256] {
        let values = usize::from(self.format.values());
        core::array::from_fn(|value| match self.palette {
            Some(palette) => palette.get(value).copied().unwrap_or(0),
            None => (value.min(values - 1) * 255 / (values - 1)) as u8,
        })
    }
}

/// How the grays of a [`Bitmap`] become those of the frame where it has
/// fewer. Kinds are constants of one type, so more can come without a
/// new plugin interface.
///
/// The numbers are stored (a text's ink keeps one), so this type stays a
/// byte. Reserved, so looks do not clash, and not made yet: 2 blue noise,
/// 3 Atkinson (error diffusion over bands is the picture code's own and a
/// render hook's carry does it for hooks), 4 halftone. A number this
/// release does not know draws as [`Dither::ORDERED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dither(u8);

impl Dither {
    /// Ordered, as pictures are: a gray between the frame's levels shows
    /// as a pattern of the levels around it.
    pub const ORDERED: Self = Self(0);
    /// The nearest level: sharp edges, flat areas.
    pub const NONE: Self = Self(1);
}

impl Dither {
    /// The dither's number: 0 ordered, 1 none; 2 to 4 are reserved (see
    /// the type).
    pub const fn value(self) -> u8 {
        self.0
    }

    /// The dither of a number.
    pub const fn from_value(value: u8) -> Self {
        Self(value)
    }
}

impl Default for Dither {
    fn default() -> Self {
        Self::ORDERED
    }
}

/// Where and how a [`Bitmap`] is drawn
/// ([`Canvas::blit`](crate::Canvas::blit)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Blit {
    pub(crate) at: Point,
    pub(crate) scale: u8,
    pub(crate) dither: Dither,
}

impl Blit {
    /// With its top left at `at`, in the canvas's coordinates, unscaled
    /// and ordered dithered.
    pub const fn at(at: Point) -> Self {
        Self {
            at,
            scale: 1,
            dither: Dither::ORDERED,
        }
    }

    /// Each pixel drawn `scale`×`scale` pixels large (1 to 8; others are
    /// taken as the nearest of those).
    pub const fn scale(mut self, scale: u8) -> Self {
        self.scale = if scale < 1 {
            1
        } else if scale > 8 {
            8
        } else {
            scale
        };
        self
    }

    /// Dithered as `dither` says.
    pub const fn dither(mut self, dither: Dither) -> Self {
        self.dither = dither;
        self
    }
}

/// The columns of the image a row of the frame shows: from column
/// `first`, of which `lead` pixels show, then `full` columns in whole and
/// `tail` pixels of the next.
#[derive(Debug, Clone, Copy)]
struct Columns {
    first: usize,
    lead: u32,
    full: usize,
    tail: u32,
}

/// The part of the image that `shown` (on the screen) shows, drawn at
/// `(left, top)` on the screen `scale` times as large: its columns, its
/// first row and how many rows of the frame that row still covers.
fn spans(shown: Rect, (left, top): (i64, i64), scale: u32) -> (Columns, usize, u32) {
    let scale64 = i64::from(scale);
    let dx = (i64::from(shown.x) - left).max(0);
    let dy = (i64::from(shown.y) - top).max(0);
    let width = u32::from(shown.width);
    let lead = (scale - (dx % scale64) as u32).min(width);
    let rest = width - lead;
    let columns = Columns {
        first: (dx / scale64) as usize,
        lead,
        full: (rest / scale) as usize,
        tail: rest % scale,
    };
    let row_lead = scale - (dy % scale64) as u32;
    (columns, (dy / scale64) as usize, row_lead)
}

/// Draws `bitmap` as `blit` says, `(left, top)` its top left on the
/// screen, where `shown` (inside the band) shows it.
pub(crate) fn draw(
    fb: &mut Framebuffer,
    shown: Rect,
    origin: (i64, i64),
    bitmap: &Bitmap<'_>,
    blit: Blit,
) {
    if fb.is_blind() || shown.is_empty() {
        return;
    }
    fb.count_picture(u32::from(shown.width) * u32::from(shown.height));
    let scale = u32::from(blit.scale.clamp(1, 8));
    let (columns, row, row_lead) = spans(shown, origin, scale);
    let job = Job {
        shown,
        columns,
        row,
        row_lead,
        scale,
    };
    let format = fb.format();
    if matches!(format.bits_per_pixel(), 1 | 2 | 4) {
        packed(fb, &job, bitmap, blit.dither);
    } else {
        let values = Values::new(fb, bitmap);
        match bitmap.format.bits_per_pixel() {
            1 => bytewise::<1>(fb, &job, bitmap, &values),
            2 => bytewise::<2>(fb, &job, bitmap, &values),
            4 => bytewise::<4>(fb, &job, bitmap, &values),
            _ => bytewise::<8>(fb, &job, bitmap, &values),
        }
    }
}

/// What a call draws, worked out once.
#[derive(Debug)]
struct Job {
    shown: Rect,
    columns: Columns,
    /// The image's row the first row of the frame shows.
    row: usize,
    /// Rows of the frame that row covers.
    row_lead: u32,
    scale: u32,
}

/// The pixels of a row of the image, one after the other: values `SB`
/// bits each.
struct Pixels<'s, const SB: u32> {
    bytes: &'s [u8],
    at: usize,
    shift: u32,
}

impl<'s, const SB: u32> Pixels<'s, SB> {
    fn new(bytes: &'s [u8], column: usize) -> Self {
        let bit = column * SB as usize;
        Self {
            bytes,
            at: bit / 8,
            shift: 8 - SB - (bit % 8) as u32,
        }
    }

    #[inline(always)]
    fn next(&mut self) -> usize {
        let byte = self.bytes.get(self.at).copied().unwrap_or(0);
        let value = (u32::from(byte) >> self.shift) & ((1 << SB) - 1);
        if self.shift == 0 {
            self.at += 1;
            self.shift = 8 - SB;
        } else {
            self.shift -= SB;
        }
        value as usize
    }
}

/// Grays as `put_gray` stores them on `fb`: dithered by `offset`, kept as
/// they are where pictures keep their tones in night mode.
fn stored(fb: &Framebuffer, gray: u8, offset: i32) -> u32 {
    let gray = (i32::from(gray) + offset).clamp(0, 255) as u8;
    let keep = fb.is_inverted() && fb.pictures_kept();
    fb.encode(Color::gray(if keep { 255 - gray } else { gray }))
}

/// The ordered dither's offsets, by row and column of its matrix; none
/// without it.
fn offsets(fb: &Framebuffer, dither: Dither) -> [[i32; 4]; 4] {
    core::array::from_fn(|y| {
        core::array::from_fn(|x| match dither {
            Dither::NONE => 0,
            _ => fb.offset(x as u16, y as u16),
        })
    })
}

/// The stored values of four pixels side by side (`bpp` bits each, the
/// first leftmost), repeated across 64 bits: a word of a row of the frame
/// whose first pixel lies at a multiple of four, as words of 32 bits do.
fn repeated(values: [u32; 4], bpp: u32) -> u64 {
    let mask = (1 << bpp) - 1;
    let unit = values
        .iter()
        .fold(0, |unit, &value| (unit << bpp) | (value & mask));
    let every = match bpp {
        1 => 0x1111_1111,
        2 => 0x0101_0101,
        _ => 0x0001_0001,
    };
    u64::from(unit * every) * 0x1_0000_0001
}

/// Draws on a frame of 1, 2 or 4 bits a pixel, each row in words.
fn packed(fb: &mut Framebuffer, job: &Job, bitmap: &Bitmap<'_>, dither: Dither) {
    let bpp = u32::from(fb.format().bits_per_pixel());
    let grays = bitmap.grays();
    let offsets = offsets(fb, dither);
    if bitmap.format.bits_per_pixel() == 8 {
        // 256 values times 16 places would be too large a table: the
        // stored value of every gray once, the pattern when it is drawn.
        let levels: [u8; 256] = core::array::from_fn(|gray| stored(fb, gray as u8, 0) as u8);
        let pattern = |y: usize, value: usize| {
            let gray = i32::from(grays[value]);
            let level = |x: usize| {
                let dithered = (gray + offsets[y][x]).clamp(0, 255) as usize;
                u32::from(levels[dithered])
            };
            repeated([level(0), level(1), level(2), level(3)], bpp)
        };
        let uniform = dither == Dither::NONE;
        return rows::<8>(fb, job, bitmap, pattern, uniform);
    }
    let values = usize::from(bitmap.format.values());
    let mut patterns = [[0u64; 16]; 4];
    for (value, &gray) in grays.iter().enumerate().take(values) {
        for (y, row) in patterns.iter_mut().enumerate() {
            let level = |x: usize| stored(fb, gray, offsets[y][x]);
            row[value] = repeated([level(0), level(1), level(2), level(3)], bpp);
        }
    }
    // Rows of the frame showing the same row of the image come out the
    // same where no value's pattern changes from row to row.
    let uniform = patterns.iter().all(|row| row == &patterns[0]);
    let pattern = |y: usize, value: usize| patterns[y][value];
    match bitmap.format.bits_per_pixel() {
        1 => rows::<1>(fb, job, bitmap, pattern, uniform),
        2 => rows::<2>(fb, job, bitmap, pattern, uniform),
        _ => rows::<4>(fb, job, bitmap, pattern, uniform),
    }
}

/// Draws the rows of a frame of 1, 2 or 4 bits a pixel, `pattern(y,
/// value)` the word of a value on rows `y` (modulo 4) of the screen;
/// copies a row the row above shows the same, where `uniform`.
fn rows<const SB: u32>(
    fb: &mut Framebuffer,
    job: &Job,
    bitmap: &Bitmap<'_>,
    pattern: impl Fn(usize, usize) -> u64,
    uniform: bool,
) {
    let bpp = u32::from(fb.format().bits_per_pixel());
    let shown = job.shown;
    let bits = (
        usize::from(shown.x) * bpp as usize,
        shown.right() as usize * bpp as usize,
    );
    let (data, stride, top) = fb.rows_mut();
    let (mut source, mut left) = (job.row, job.row_lead);
    let mut above: Option<usize> = None;
    for y in shown.y..shown.bottom() as u16 {
        let at = usize::from(y - top) * stride;
        if let Some(from) = above.filter(|_| uniform) {
            copy_bits(data, from, at, bits);
        } else {
            let screen_row = usize::from(y % 4);
            let row = Packer::new(&mut data[at..at + stride], bits.0);
            pack_row::<SB>(row, job, bitmap.row(source), bpp, |value| {
                pattern(screen_row, value)
            });
            above = Some(at);
        }
        left -= 1;
        if left == 0 {
            (source, left, above) = (source + 1, job.scale, None);
        }
    }
}

/// Puts a row of the frame together from the columns of the image's row
/// `bytes`, `word(value)` the pattern of a value.
#[inline(always)]
fn pack_row<const SB: u32>(
    mut row: Packer<'_>,
    job: &Job,
    bytes: &[u8],
    bpp: u32,
    word: impl Fn(usize) -> u64,
) {
    let columns = job.columns;
    let mut pixels = Pixels::<SB>::new(bytes, columns.first);
    row.put(word(pixels.next()), columns.lead * bpp);
    let bits = job.scale * bpp;
    for _ in 0..columns.full {
        row.put(word(pixels.next()), bits);
    }
    if columns.tail > 0 {
        row.put(word(pixels.next()), columns.tail * bpp);
    }
    row.finish();
}

/// Puts a row of a frame of a few bits a pixel together in words of 32
/// bits, from the left, each written as four bytes once it is full; the
/// pixels before the first and after the last are kept.
struct Packer<'r> {
    row: &'r mut [u8],
    /// The byte where the word being filled goes.
    at: usize,
    /// Bits of it filled, from the highest.
    filled: u32,
    /// Bits of it to keep as they are (before the first pixel drawn).
    keep: u32,
    /// The word being filled, in the higher half; what reaches past it
    /// in the lower.
    word: u64,
}

impl<'r> Packer<'r> {
    fn new(row: &'r mut [u8], bit: usize) -> Self {
        let filled = (bit % 32) as u32;
        Self {
            row,
            at: bit / 32 * 4,
            filled,
            keep: !(u32::MAX >> filled),
            word: 0,
        }
    }

    /// The next `bits` bits (1 to 32) from `pattern`, which holds the
    /// pattern of every place of a word.
    #[inline(always)]
    fn put(&mut self, pattern: u64, bits: u32) {
        let span = (u64::MAX >> (64 - bits)) << (64 - self.filled - bits);
        self.word |= pattern & span;
        self.filled += bits;
        if self.filled >= 32 {
            let full = (self.word >> 32) as u32;
            if self.keep == 0 {
                if let Some(bytes) = self.row.get_mut(self.at..self.at + 4) {
                    bytes.copy_from_slice(&full.to_be_bytes());
                }
            } else {
                merge(self.row, self.at, full, !self.keep);
                self.keep = 0;
            }
            self.word <<= 32;
            self.filled -= 32;
            self.at += 4;
        }
    }

    /// Writes what is left of the last word.
    fn finish(self) {
        if self.filled > 0 {
            let mask = !(u32::MAX >> self.filled) & !self.keep;
            merge(self.row, self.at, (self.word >> 32) as u32, mask);
        }
    }
}

/// Writes the bits of `word` that `mask` has to the four bytes at `at`.
fn merge(row: &mut [u8], at: usize, word: u32, mask: u32) {
    for (i, (value, mask)) in word
        .to_be_bytes()
        .into_iter()
        .zip(mask.to_be_bytes())
        .enumerate()
    {
        if mask != 0
            && let Some(byte) = row.get_mut(at + i)
        {
            *byte = (*byte & !mask) | (value & mask);
        }
    }
}

/// Copies bits `bits.0..bits.1` of the row at byte `from` to the row at
/// byte `to`.
fn copy_bits(data: &mut [u8], from: usize, to: usize, (start, end): (usize, usize)) {
    let (first, last) = (start / 8, (end - 1) / 8);
    let head = 0xFFu8 >> (start % 8);
    let tail = 0xFFu8 << ((8 - end % 8) % 8);
    let mut copy = |byte: usize, mask: u8| {
        let value = data[from + byte];
        let target = &mut data[to + byte];
        *target = (*target & !mask) | (value & mask);
    };
    if first == last {
        return copy(first, head & tail);
    }
    copy(first, head);
    copy(last, tail);
    data.copy_within(from + first + 1..from + last, to + first + 1);
}

/// The stored value of every value of the image, as the bytes of a pixel
/// of a frame of 8 bits a pixel or more, where nothing is dithered.
struct Values {
    bytes: [[u8; 3]; 256],
    /// Bytes a pixel.
    size: usize,
}

impl Values {
    fn new(fb: &Framebuffer, bitmap: &Bitmap<'_>) -> Self {
        let grays = bitmap.grays();
        let format = fb.format();
        let values = usize::from(bitmap.format.values());
        let mut bytes = [[0; 3]; 256];
        for (entry, &gray) in bytes.iter_mut().zip(&grays).take(values) {
            let value = stored(fb, gray, 0);
            *entry = match format {
                PixelFormat::Rgb565 => {
                    let [low, high] = (value as u16).to_le_bytes();
                    [low, high, 0]
                }
                PixelFormat::Rgb888 => [(value >> 16) as u8, (value >> 8) as u8, value as u8],
                _ => [value as u8, 0, 0],
            };
        }
        let size = usize::from(format.bits_per_pixel() / 8).clamp(1, 3);
        Self { bytes, size }
    }
}

/// Draws on a frame of 8 bits a pixel or more, pixel by pixel from the
/// values worked out before; a row the row above shows the same is
/// copied.
fn bytewise<const SB: u32>(fb: &mut Framebuffer, job: &Job, bitmap: &Bitmap<'_>, values: &Values) {
    let size = values.size;
    let shown = job.shown;
    let (start, end) = (usize::from(shown.x) * size, shown.right() as usize * size);
    let (data, stride, top) = fb.rows_mut();
    let (mut source, mut left) = (job.row, job.row_lead);
    let mut above: Option<usize> = None;
    let columns = job.columns;
    for y in shown.y..shown.bottom() as u16 {
        let at = usize::from(y - top) * stride;
        if let Some(from) = above {
            data.copy_within(from + start..from + end, at + start);
        } else {
            let mut pixels = Pixels::<SB>::new(bitmap.row(source), columns.first);
            let mut out = data[at + start..at + end].chunks_exact_mut(size);
            let mut put = |count: u32| {
                let bytes = &values.bytes[pixels.next()][..size];
                for pixel in out.by_ref().take(count as usize) {
                    pixel.copy_from_slice(bytes);
                }
            };
            put(columns.lead);
            for _ in 0..columns.full {
                put(job.scale);
            }
            put(columns.tail);
            above = Some(at);
        }
        left -= 1;
        if left == 0 {
            (source, left, above) = (source + 1, job.scale, None);
        }
    }
}
