//! The picture recipe of `image_prepare` (`docs/abi/document.md` 5): a tone
//! table applied while a picture is decoded, the turn a JPEG's EXIF
//! orientation asks for, and the 8 by 8 brightness grid `event.image_ready`
//! carries. The host defines no brightness or contrast: the plugin's table is
//! the whole curve.
//!
//! Memory: the table is 256 bytes kept with a waiting request; the decoder
//! needs no more than before (a row buffer of at most three bytes a pixel for
//! a colour row); a turn of a quarter (orientations 5 to 8) reads the picture
//! again in tiles of at most `band` bytes, after the decoder has finished and
//! freed its state; the brightness grid reads one row at a time.
use alloc::vec::Vec;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::Storage;
use paperpaw_sdk::image::{ImageError, ImageSink, luma};

/// The bytes of a tone table (`image_prepare.tone`).
pub(crate) const TONE_BYTES: usize = 256;
/// The bytes of the brightness grid (`image_ready.luma`).
pub(crate) const LUMA_BYTES: usize = 64;
/// The cells of the grid on a side.
const GRID: usize = 8;
/// The header of a decoded picture file (`PPGI`/`PPCI`, width, height).
const HEADER: u64 = 12;
/// Rows of a turned picture are appended in chunks of about this size.
const FLUSH: usize = 8192;
/// JPEG segments looked at for the EXIF block (it comes first).
const SEGMENTS_MAX: usize = 32;
/// IFD entries looked at for the orientation tag.
const ENTRIES_MAX: u64 = 64;

/// What `image_prepare` asks besides the size and the colour.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Recipe {
    /// Apply a JPEG's EXIF orientation while it is decoded.
    pub(crate) turn: bool,
    /// Keep a PNG's or GIF's transparency (a gray picture with its opacity,
    /// drawn over what is below); a JPEG is made as without it.
    pub(crate) see_through: bool,
    /// The luma table: exactly [`TONE_BYTES`] bytes, or none.
    pub(crate) tone: Option<Vec<u8>>,
}

impl Recipe {
    /// A copy, the room taken fallibly.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`.
    pub(crate) fn try_copy(&self) -> Result<Self, i32> {
        let tone = match &self.tone {
            Some(table) => {
                let mut copy = Vec::new();
                copy.try_reserve_exact(table.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                copy.extend_from_slice(table);
                Some(copy)
            }
            None => None,
        };
        Ok(Self {
            turn: self.turn,
            see_through: self.see_through,
            tone,
        })
    }
}

/// A sink that maps every value through a table before the sink behind it
/// sees it: gray rows, and each channel of colour rows.
pub(super) struct ToneSink<'a, S: ImageSink> {
    inner: &'a mut S,
    table: &'a [u8],
    scratch: Vec<u8>,
}

impl<'a, S: ImageSink> ToneSink<'a, S> {
    pub(super) fn new(inner: &'a mut S, table: &'a [u8]) -> Self {
        Self {
            inner,
            table,
            scratch: Vec::new(),
        }
    }

    /// `row` through the table, in the scratch (grown fallibly, once for the
    /// longest row).
    fn mapped(&mut self, row: &[u8]) -> Result<(), ImageError> {
        self.scratch.clear();
        self.scratch
            .try_reserve(row.len())
            .map_err(|_| ImageError::OutOfMemory)?;
        let table = self.table;
        self.scratch.extend(
            row.iter()
                .map(|&value| table.get(usize::from(value)).copied().unwrap_or(value)),
        );
        Ok(())
    }
}

impl<S: ImageSink> ImageSink for ToneSink<'_, S> {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        self.inner.start(width, height)?;
        // A colour row is the longest: room taken once.
        let longest = (width as usize).saturating_mul(3);
        self.scratch
            .try_reserve_exact(longest)
            .map_err(|_| ImageError::OutOfMemory)
    }

    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        self.mapped(gray)?;
        self.inner.row(&self.scratch)
    }

    fn wants_color(&self) -> bool {
        self.inner.wants_color()
    }

    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        self.mapped(rgb)?;
        self.inner.row_rgb(&self.scratch)
    }

    fn wants_alpha(&self) -> bool {
        self.inner.wants_alpha()
    }

    /// The gray of each pixel through the table; its opacity as it is.
    fn row_alpha(&mut self, gray_alpha: &[u8]) -> Result<(), ImageError> {
        self.mapped(gray_alpha)?;
        // Every byte went through the table: the opacities are put back.
        let opacities = gray_alpha.iter().skip(1).step_by(2);
        for (mapped, &opacity) in self.scratch.iter_mut().skip(1).step_by(2).zip(opacities) {
            *mapped = opacity;
        }
        self.inner.row_alpha(&self.scratch)
    }
}

/// Reads `out` whole from `at`: `false` where the file ends first or the card
/// fails.
fn read_all(storage: &dyn Storage, path: &str, at: u64, out: &mut [u8]) -> bool {
    let mut filled = 0;
    while filled < out.len() {
        match storage.read_at(path, at + filled as u64, &mut out[filled..]) {
            Ok(0) | Err(_) => return false,
            Ok(n) => filled += n,
        }
    }
    true
}

/// The EXIF orientation (1 to 8) of the JPEG at `path`, 1 (as it is) where
/// there is none, it is out of range, or the file cannot be read: the walk
/// reads a few small pieces and allocates nothing.
pub(super) fn exif_orientation(storage: &dyn Storage, path: &str) -> u8 {
    let mut at = 2_u64;
    for _ in 0..SEGMENTS_MAX {
        let mut seg = [0_u8; 10];
        let Ok(n) = storage.read_at(path, at, &mut seg) else {
            return 1;
        };
        if n < 4 || seg[0] != 0xff {
            return 1;
        }
        match seg[1] {
            0xff => at += 1,
            0x01 | 0xd0..=0xd8 => at += 2,
            // The scan or the end: no EXIF block came first.
            0xd9 | 0xda => return 1,
            marker => {
                let len = u64::from(u16::from_be_bytes([seg[2], seg[3]]));
                if len < 2 {
                    return 1;
                }
                if marker == 0xe1 && len >= 16 && n >= 10 && &seg[4..10] == b"Exif\0\0" {
                    return tiff_orientation(storage, path, at + 10, len - 8);
                }
                at += 2 + len;
            }
        }
    }
    1
}

/// The orientation tag of the first IFD of the TIFF block at `base`, `avail`
/// bytes long.
fn tiff_orientation(storage: &dyn Storage, path: &str, base: u64, avail: u64) -> u8 {
    let mut head = [0_u8; 8];
    if !read_all(storage, path, base, &mut head) {
        return 1;
    }
    let big = match &head[..2] {
        b"MM" => true,
        b"II" => false,
        _ => return 1,
    };
    let short = |b: &[u8]| {
        let bytes = [b[0], b[1]];
        if big {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    };
    if short(&head[2..4]) != 42 {
        return 1;
    }
    let long = |b: &[u8]| {
        let bytes = [b[0], b[1], b[2], b[3]];
        if big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    };
    let ifd = u64::from(long(&head[4..8]));
    if ifd < 8 || ifd + 2 > avail {
        return 1;
    }
    let mut count = [0_u8; 2];
    if !read_all(storage, path, base + ifd, &mut count) {
        return 1;
    }
    let count = u64::from(short(&count)).min(ENTRIES_MAX);
    for index in 0..count {
        let at = ifd + 2 + 12 * index;
        if at + 12 > avail {
            break;
        }
        let mut entry = [0_u8; 12];
        if !read_all(storage, path, base + at, &mut entry) {
            return 1;
        }
        if short(&entry[..2]) == 0x0112 {
            let value = short(&entry[8..10]);
            // A short, one value.
            return if short(&entry[2..4]) == 3
                && long(&entry[4..8]) == 1
                && (1..=8).contains(&value)
            {
                u8::try_from(value).unwrap_or(1)
            } else {
                1
            };
        }
    }
    1
}

/// How a picture of an EXIF orientation is turned: swap the axes, then mirror
/// across and down, as `out(x, y) = src(...)` says (see [`turn_file`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Turn {
    swap: bool,
    across: bool,
    down: bool,
}

impl Turn {
    /// The turn of orientation 2 to 8; `None` for 1 (as it is) or any other
    /// value.
    fn of(orientation: u8) -> Option<Self> {
        let (swap, across, down) = match orientation {
            2 => (false, true, false),
            3 => (false, true, true),
            4 => (false, false, true),
            5 => (true, false, false),
            6 => (true, false, true),
            7 => (true, true, true),
            8 => (true, true, false),
            _ => return None,
        };
        Some(Self { swap, across, down })
    }
}

/// Whether `orientation` needs a turn at all (2 to 8).
pub(super) fn needs_turn(orientation: u8) -> bool {
    Turn::of(orientation).is_some()
}

/// A buffer of `len` zeros, the room taken fallibly.
fn buffer(len: usize) -> Result<Vec<u8>, ImageError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| ImageError::OutOfMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// Appends the rows a turn makes, in chunks.
struct Appender<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    chunk: Vec<u8>,
    limit: usize,
}

impl Appender<'_> {
    fn push(&mut self, row: &[u8]) -> Result<(), ImageError> {
        if !self.chunk.is_empty() && self.chunk.len() + row.len() > self.limit {
            self.flush()?;
        }
        self.chunk
            .try_reserve(row.len())
            .map_err(|_| ImageError::OutOfMemory)?;
        self.chunk.extend_from_slice(row);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), ImageError> {
        if !self.chunk.is_empty() {
            self.storage
                .append(self.path, &self.chunk)
                .map_err(|_| ImageError::Io)?;
            self.chunk.clear();
        }
        Ok(())
    }
}

/// Writes the picture at `from` (`channels` bytes a pixel: 1 gray, 3 colour)
/// to `to` turned as EXIF `orientation` (2 to 8) says. A mirror or a half turn
/// reads and writes a row at a time. A quarter turn (5 to 8) makes each output
/// row from a column of the source, so it reads the source in tiles of whole
/// columns, as many columns as `band` bytes hold (at least one), a read of a
/// tile's piece of each source row: `height * ceil(width / columns)` reads;
/// more than `reads` of them are `TooLarge`, and nothing is written (the turn
/// is made in one main-loop turn, so its time is bounded).
///
/// # Errors
/// `ImageError::Invalid` for a file that is no picture, `Io` for the card,
/// `TooLarge` for a turn over the read budget,
/// `OutOfMemory` where a buffer is refused.
pub(super) fn turn_file(
    storage: &dyn Storage,
    (from, to): (&str, &str),
    orientation: u8,
    channels: usize,
    (band, reads): (usize, usize),
) -> Result<(), ImageError> {
    let Some(turn) = Turn::of(orientation) else {
        return Err(ImageError::Invalid("orientation"));
    };
    let mut header = [0_u8; HEADER as usize];
    if !read_all(storage, from, 0, &mut header) {
        return Err(ImageError::Io);
    }
    let width = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let height = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if width == 0 || height == 0 {
        return Err(ImageError::Invalid("size"));
    }
    let source_row = width.checked_mul(channels).ok_or(ImageError::TooLarge)?;
    let (out_width, out_height) = if turn.swap {
        (height, width)
    } else {
        (width, height)
    };
    if turn.swap && quarter_reads((width, height, channels), band) > reads {
        return Err(ImageError::TooLarge);
    }
    let mut turned = header;
    turned[4..8].copy_from_slice(&(out_width as u32).to_le_bytes());
    turned[8..12].copy_from_slice(&(out_height as u32).to_le_bytes());
    storage.write(to, &turned).map_err(|_| ImageError::Io)?;
    let out_row = out_width
        .checked_mul(channels)
        .ok_or(ImageError::TooLarge)?;
    let mut out = Appender {
        storage,
        path: to,
        chunk: Vec::new(),
        limit: FLUSH.max(out_row),
    };
    out.chunk
        .try_reserve_exact(out.limit)
        .map_err(|_| ImageError::OutOfMemory)?;
    if turn.swap {
        turn_quarter(
            storage,
            from,
            turn,
            (width, height, channels),
            band,
            &mut out,
        )?;
    } else {
        let mut row = buffer(source_row)?;
        let mut mirrored = buffer(if turn.across { source_row } else { 0 })?;
        for y in 0..height {
            let sy = if turn.down { height - 1 - y } else { y };
            if !read_all(storage, from, HEADER + (sy * source_row) as u64, &mut row) {
                return Err(ImageError::Io);
            }
            if turn.across {
                for (to, pixel) in mirrored
                    .chunks_exact_mut(channels)
                    .zip(row.chunks_exact(channels).rev())
                {
                    to.copy_from_slice(pixel);
                }
                out.push(&mirrored)?;
            } else {
                out.push(&row)?;
            }
        }
    }
    out.flush()
}

/// How many short reads a quarter turn of a `width` by `height` picture makes
/// with tiles of `band` bytes: `height` for each tile of columns.
fn quarter_reads((width, height, channels): (usize, usize, usize), band: usize) -> usize {
    let column_bytes = height.saturating_mul(channels);
    let columns = (band / column_bytes.max(1)).clamp(1, width.max(1));
    height.saturating_mul(width.div_ceil(columns))
}

/// The quarter turns: output row `y` is source column `y` (or `width - 1 - y`
/// where mirrored across), read along the source rows downward or upward.
fn turn_quarter(
    storage: &dyn Storage,
    from: &str,
    turn: Turn,
    (width, height, channels): (usize, usize, usize),
    band: usize,
    out: &mut Appender<'_>,
) -> Result<(), ImageError> {
    let column_bytes = height.checked_mul(channels).ok_or(ImageError::TooLarge)?;
    let columns = (band / column_bytes.max(1)).clamp(1, width);
    let mut tile = buffer(columns.saturating_mul(column_bytes))?;
    let mut row = buffer(column_bytes)?;
    let source_row = width * channels;
    let mut y = 0;
    while y < width {
        let group = columns.min(width - y);
        // The source columns of the output rows `y..y + group`.
        let first = if turn.across { width - y - group } else { y };
        for sy in 0..height {
            let at = HEADER + (sy * source_row + first * channels) as u64;
            let piece = tile
                .get_mut(sy * group * channels..(sy + 1) * group * channels)
                .ok_or(ImageError::Invalid("tile"))?;
            if !read_all(storage, from, at, piece) {
                return Err(ImageError::Io);
            }
        }
        for offset in 0..group {
            let column = if turn.across {
                group - 1 - offset
            } else {
                offset
            };
            for (x, pixel) in row.chunks_exact_mut(channels).enumerate() {
                let sy = if turn.down { height - 1 - x } else { x };
                let from = (sy * group + column) * channels;
                pixel.copy_from_slice(
                    tile.get(from..from + channels)
                        .ok_or(ImageError::Invalid("tile"))?,
                );
            }
            out.push(&row)?;
        }
        y += group;
    }
    Ok(())
}

/// The mean brightness of an 8 by 8 grid over the picture at `path` (`width`
/// by `height`, `channels` bytes a pixel), row by row; `None` where it cannot
/// be read or a row buffer is refused. A cell of a picture under 8 pixels
/// takes the pixel at its place.
pub(super) fn luma_grid(
    storage: &dyn Storage,
    path: &str,
    (width, height, channels): (usize, usize, usize),
) -> Option<[u8; LUMA_BYTES]> {
    if width == 0 || height == 0 {
        return None;
    }
    let mut row = buffer(width.checked_mul(channels)?).ok()?;
    let mut grid = [0_u8; LUMA_BYTES];
    let mut sums = [0_u64; GRID];
    for cell_y in 0..GRID {
        sums.fill(0);
        let y0 = cell_y * height / GRID;
        let y1 = ((cell_y + 1) * height / GRID).clamp(y0 + 1, height);
        for y in y0..y1 {
            let at = HEADER + (y * width * channels) as u64;
            if !read_all(storage, path, at, &mut row) {
                return None;
            }
            for (cell_x, sum) in sums.iter_mut().enumerate() {
                let x0 = cell_x * width / GRID;
                let x1 = ((cell_x + 1) * width / GRID).clamp(x0 + 1, width);
                let pixels = row.get(x0 * channels..x1 * channels)?;
                *sum += if channels == 3 {
                    pixels
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|p| u64::from(luma(p[0].into(), p[1].into(), p[2].into())))
                        .sum::<u64>()
                } else if channels == 2 {
                    // A see-through picture: its gray, the opacity not counted.
                    pixels.iter().step_by(2).map(|&g| u64::from(g)).sum::<u64>()
                } else {
                    pixels.iter().map(|&g| u64::from(g)).sum::<u64>()
                };
            }
        }
        for (cell_x, sum) in sums.iter().enumerate() {
            let x0 = cell_x * width / GRID;
            let x1 = ((cell_x + 1) * width / GRID).clamp(x0 + 1, width);
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            *grid.get_mut(cell_y * GRID + cell_x)? =
                u8::try_from(sum / count.max(1)).unwrap_or(255);
        }
    }
    Some(grid)
}

#[cfg(test)]
#[path = "recipe_tests.rs"]
mod tests;
