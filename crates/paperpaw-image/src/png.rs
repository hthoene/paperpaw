//! Non-interlaced PNG, inflated and unfiltered one row at a time.

use alloc::vec::Vec;

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};

use crate::{Downscaler, Fit, ImageError, ImageSink, Reader, zeroed};

/// The deflate history.
const WINDOW: usize = 32 * 1024;

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
}

impl Header {
    fn channels(&self) -> usize {
        match self.color {
            0 | 3 => 1,
            4 => 2,
            2 => 3,
            _ => 4,
        }
    }

    /// Bytes per row without the filter byte.
    fn row_bytes(&self) -> usize {
        (self.width as usize * self.channels() * usize::from(self.depth)).div_ceil(8)
    }

    /// Bytes per complete pixel for filtering (at least 1).
    fn pixel_bytes(&self) -> usize {
        (self.channels() * usize::from(self.depth))
            .div_ceil(8)
            .max(1)
    }
}

fn luma(r: u32, g: u32, b: u32) -> u32 {
    (r * 299 + g * 587 + b * 114 + 500) / 1000
}

/// Turns PNG rows into gray rows, RGB rows for a sink that wants colour,
/// or gray rows with transparency for one that wants that.
struct Rows<'s> {
    header: Header,
    /// Gray of each palette entry, over white. The three tables are 256
    /// entries of a palette image (1,536 B of heap in all, as long as its
    /// rows are read), none for another.
    palette: Vec<u8>,
    /// Each palette entry over white, for colour.
    colors: Vec<[u8; 3]>,
    /// Gray and opacity of each palette entry, for transparency.
    see_through: Vec<[u8; 2]>,
    /// The colour shown as clear in a gray or RGB image (`tRNS`), its raw
    /// samples (gray in the first).
    key: Option<[u16; 3]>,
    color: bool,
    alpha: bool,
    previous: Vec<u8>,
    current: Vec<u8>,
    filled: usize,
    gray: Vec<u8>,
    done: u32,
    scaler: Downscaler,
    sink: &'s mut dyn ImageSink,
}

/// What `PLTE` and `tRNS` say of a palette image's entries: on the heap
/// (1,024 B, until the rows are set up), not in `decode`'s frame.
struct Palette {
    colors: Vec<[u8; 3]>,
    alpha: Vec<u8>,
}

impl Palette {
    fn new() -> Result<Self, ImageError> {
        let mut alpha: Vec<u8> = zeroed(256)?;
        alpha.fill(255);
        Ok(Self {
            colors: zeroed(256)?,
            alpha,
        })
    }
}

/// The tables of `palette` for each way of showing an entry: gray, colour
/// and gray with opacity, over white (all of 256 entries, 1,536 B of heap).
type Tables = (Vec<u8>, Vec<[u8; 3]>, Vec<[u8; 2]>);

#[inline(never)]
fn entry_tables(palette: &Palette) -> Result<Tables, ImageError> {
    let (mut gray, mut rgb, mut see_through): Tables = (zeroed(256)?, zeroed(256)?, zeroed(256)?);
    for (i, &[r, g, b]) in palette.colors.iter().enumerate() {
        let a = u32::from(palette.alpha[i]);
        let over = |v: u8| ((u32::from(v) * a + 255 * (255 - a)) / 255) as u8;
        let y = luma(u32::from(r), u32::from(g), u32::from(b));
        gray[i] = ((y * a + 255 * (255 - a)) / 255) as u8;
        rgb[i] = [over(r), over(g), over(b)];
        see_through[i] = [y as u8, palette.alpha[i]];
    }
    Ok((gray, rgb, see_through))
}

impl Rows<'_> {
    fn push(&mut self, mut data: &[u8]) -> Result<(), ImageError> {
        let row_len = self.header.row_bytes() + 1;
        while !data.is_empty() && self.done < self.header.height {
            let take = (row_len - self.filled).min(data.len());
            self.current[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled == row_len {
                self.unfilter()?;
                if self.alpha {
                    self.convert_alpha();
                } else if self.color {
                    self.convert_rgb();
                } else {
                    self.convert();
                }
                self.scaler.row(&self.gray, self.sink)?;
                core::mem::swap(&mut self.previous, &mut self.current);
                self.filled = 0;
                self.done += 1;
            }
        }
        Ok(())
    }

    fn unfilter(&mut self) -> Result<(), ImageError> {
        let bpp = self.header.pixel_bytes();
        let (filter, row) = self
            .current
            .split_first_mut()
            .ok_or(ImageError::Invalid("empty row"))?;
        let prev = &self.previous[1..];
        match *filter {
            0 => {}
            1 => {
                for i in bpp..row.len() {
                    row[i] = row[i].wrapping_add(row[i - bpp]);
                }
            }
            2 => {
                for i in 0..row.len() {
                    row[i] = row[i].wrapping_add(prev[i]);
                }
            }
            3 => {
                for i in 0..row.len() {
                    let left = if i >= bpp { u16::from(row[i - bpp]) } else { 0 };
                    row[i] = row[i].wrapping_add(((left + u16::from(prev[i])) / 2) as u8);
                }
            }
            4 => {
                for i in 0..row.len() {
                    let a = if i >= bpp { i16::from(row[i - bpp]) } else { 0 };
                    let b = i16::from(prev[i]);
                    let c = if i >= bpp {
                        i16::from(prev[i - bpp])
                    } else {
                        0
                    };
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    let predictor = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    row[i] = row[i].wrapping_add(predictor as u8);
                }
            }
            _ => return Err(ImageError::Invalid("bad PNG filter")),
        }
        Ok(())
    }

    /// Sample `index` of the row as stored, all 16 bits of 16-bit images.
    fn raw(row: &[u8], depth: u8, index: usize) -> u16 {
        match depth {
            8 => u16::from(row[index]),
            16 => u16::from_be_bytes([row[index * 2], row[index * 2 + 1]]),
            bits => {
                let bit = index * usize::from(bits);
                let shift = 8 - usize::from(bits) - bit % 8;
                u16::from(row[bit / 8] >> shift) & ((1 << bits) - 1)
            }
        }
    }

    /// Whether pixel `x` of the row is the clear colour (`tRNS` of a gray
    /// or RGB image).
    fn keyed(&self, row: &[u8], x: usize) -> bool {
        let Some(key) = self.key else {
            return false;
        };
        let h = &self.header;
        match h.color {
            0 => Self::raw(row, h.depth, x) == key[0],
            2 => (0..3).all(|c| Self::raw(row, h.depth, x * 3 + c) == key[c]),
            _ => false,
        }
    }

    /// Sample `index` of the row, scaled to 0 to 255.
    fn sample(row: &[u8], depth: u8, index: usize) -> u32 {
        match depth {
            8 => u32::from(row[index]),
            16 => u32::from(row[index * 2]),
            bits => {
                let bit = index * usize::from(bits);
                let byte = row[bit / 8];
                let shift = 8 - usize::from(bits) - bit % 8;
                let max = (1u32 << bits) - 1;
                let value = u32::from(byte >> shift) & max;
                value * 255 / max
            }
        }
    }

    fn convert(&mut self) {
        let h = &self.header;
        let row = &self.current[1..];
        let channels = h.channels();
        for x in 0..h.width as usize {
            let s = |c: usize| Self::sample(row, h.depth, x * channels + c);
            let over_white = |v: u32, alpha: u32| (v * alpha + 255 * (255 - alpha)) / 255;
            if self.keyed(row, x) {
                self.gray[x] = 255;
                continue;
            }
            self.gray[x] = match h.color {
                0 => s(0),
                2 => luma(s(0), s(1), s(2)),
                3 => {
                    let index = if h.depth == 8 {
                        u32::from(row[x])
                    } else {
                        let bit = x * usize::from(h.depth);
                        let shift = 8 - usize::from(h.depth) - bit % 8;
                        u32::from(row[bit / 8] >> shift) & ((1 << h.depth) - 1)
                    };
                    u32::from(self.palette[index as usize])
                }
                4 => over_white(s(0), s(1)),
                _ => over_white(luma(s(0), s(1), s(2)), s(3)),
            } as u8;
        }
    }
}

impl Rows<'_> {
    /// The current row as gray with opacity (in `gray`, two bytes a
    /// pixel).
    fn convert_alpha(&mut self) {
        let h = &self.header;
        let row = &self.current[1..];
        let channels = h.channels();
        for x in 0..h.width as usize {
            let s = |c: usize| Self::sample(row, h.depth, x * channels + c);
            let keyed = self.keyed(row, x);
            let pixel = match h.color {
                0 => [s(0) as u8, if keyed { 0 } else { 255 }],
                2 => [luma(s(0), s(1), s(2)) as u8, if keyed { 0 } else { 255 }],
                3 => {
                    let index = if h.depth == 8 {
                        usize::from(row[x])
                    } else {
                        let bit = x * usize::from(h.depth);
                        let shift = 8 - usize::from(h.depth) - bit % 8;
                        usize::from(row[bit / 8] >> shift) & ((1 << h.depth) - 1)
                    };
                    self.see_through[index]
                }
                4 => [s(0) as u8, s(1) as u8],
                _ => [luma(s(0), s(1), s(2)) as u8, s(3) as u8],
            };
            self.gray[x * 2..x * 2 + 2].copy_from_slice(&pixel);
        }
    }

    /// The current row as RGB (in `gray`, three bytes a pixel),
    /// transparency over white.
    fn convert_rgb(&mut self) {
        let h = &self.header;
        let row = &self.current[1..];
        let channels = h.channels();
        for x in 0..h.width as usize {
            let s = |c: usize| Self::sample(row, h.depth, x * channels + c);
            let over_white = |v: u32, alpha: u32| ((v * alpha + 255 * (255 - alpha)) / 255) as u8;
            if self.keyed(row, x) {
                self.gray[x * 3..x * 3 + 3].fill(255);
                continue;
            }
            let rgb = match h.color {
                0 => [s(0) as u8; 3],
                2 => [s(0) as u8, s(1) as u8, s(2) as u8],
                3 => {
                    let index = if h.depth == 8 {
                        usize::from(row[x])
                    } else {
                        let bit = x * usize::from(h.depth);
                        let shift = 8 - usize::from(h.depth) - bit % 8;
                        usize::from(row[bit / 8] >> shift) & ((1 << h.depth) - 1)
                    };
                    self.colors[index]
                }
                4 => [over_white(s(0), s(1)); 3],
                _ => {
                    let a = s(3);
                    [
                        over_white(s(0), a),
                        over_white(s(1), a),
                        over_white(s(2), a),
                    ]
                }
            };
            self.gray[x * 3..x * 3 + 3].copy_from_slice(&rgb);
        }
    }
}

// Not inlined into `decode`: each format's frame on the stack alone,
// not all three together.
#[inline(never)]
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub(crate) fn decode(
    reader: &mut Reader<'_>,
    fit: Fit,
    sink: &mut dyn ImageSink,
) -> Result<(), ImageError> {
    let mut signature = [0u8; 8];
    reader.fill(&mut signature)?;
    if &signature != b"\x89PNG\r\n\x1a\n" {
        return Err(ImageError::Invalid("not a PNG"));
    }
    let mut header: Option<Header> = None;
    // Made at the first `PLTE` or `tRNS` of a palette image, or at its
    // rows; dropped once the rows' tables are made from it.
    let mut entries: Option<Palette> = None;
    let mut key: Option<[u16; 3]> = None;
    let mut rows: Option<Rows<'_>> = None;
    let mut sink = Some(sink);
    // Inflate state, created at the first IDAT chunk.
    let mut decoder: Vec<DecompressorOxide> = Vec::new();
    let mut window: Vec<u8> = Vec::new();
    let mut position = 0usize;
    let mut zlib_header = 2usize;
    // Read from the stream a piece at a time: made with the window.
    let mut chunk: Vec<u8> = Vec::new();

    loop {
        let len = reader.u32_be()? as usize;
        let mut kind = [0u8; 4];
        reader.fill(&mut kind)?;
        match &kind {
            b"IHDR" => {
                let width = reader.u32_be()?;
                let height = reader.u32_be()?;
                let depth = reader.byte()?;
                let color = reader.byte()?;
                let _compression = reader.byte()?;
                let _filter = reader.byte()?;
                let interlace = reader.byte()?;
                reader.skip(len.saturating_sub(13))?;
                let valid = matches!(
                    (color, depth),
                    (0, 1 | 2 | 4 | 8 | 16) | (3, 1 | 2 | 4 | 8) | (2 | 4 | 6, 8 | 16)
                );
                if width == 0 || height == 0 || !valid {
                    return Err(ImageError::Invalid("bad PNG header"));
                }
                if interlace != 0 {
                    return Err(ImageError::Unsupported("interlaced PNG"));
                }
                if width > 1 << 16 || height > 1 << 16 {
                    return Err(ImageError::Unsupported("PNG too large"));
                }
                header = Some(Header {
                    width,
                    height,
                    depth,
                    color,
                });
            }
            b"PLTE" => {
                let count = (len / 3).min(256);
                let palette = match &mut entries {
                    Some(palette) => palette,
                    none => none.insert(Palette::new()?),
                };
                for color in palette.colors.iter_mut().take(count) {
                    *color = [reader.byte()?, reader.byte()?, reader.byte()?];
                }
                reader.skip(len - count * 3)?;
            }
            b"tRNS" if header.as_ref().is_some_and(|h| h.color == 3) => {
                let palette = match &mut entries {
                    Some(palette) => palette,
                    none => none.insert(Palette::new()?),
                };
                for a in palette.alpha.iter_mut().take(len) {
                    *a = reader.byte()?;
                }
                reader.skip(len.saturating_sub(256))?;
            }
            // The one colour of a gray or RGB image shown as clear.
            b"tRNS" if header.as_ref().is_some_and(|h| matches!(h.color, 0 | 2)) => {
                let samples = if header.as_ref().is_some_and(|h| h.color == 0) {
                    1
                } else {
                    3
                };
                if len >= samples * 2 {
                    let mut value = [0u16; 3];
                    for sample in value.iter_mut().take(samples) {
                        *sample = u16::from_be_bytes([reader.byte()?, reader.byte()?]);
                    }
                    key = Some(value);
                    reader.skip(len - samples * 2)?;
                } else {
                    reader.skip(len)?;
                }
            }
            b"IDAT" => {
                if rows.is_none() {
                    let h = header
                        .take()
                        .ok_or(ImageError::Invalid("IDAT before IHDR"))?;
                    // Only a palette image has entries to show.
                    let (mut palette, mut colors, mut see_through) =
                        (Vec::new(), Vec::new(), Vec::new());
                    let mut clear_entry = false;
                    if h.color == 3 {
                        let entries = match &mut entries {
                            Some(entries) => entries,
                            none => none.insert(Palette::new()?),
                        };
                        clear_entry = entries.alpha.iter().any(|&a| a < 255);
                        (palette, colors, see_through) = entry_tables(entries)?;
                    }
                    entries = None;
                    let size = fit.size(h.width, h.height);
                    let sink = sink.take().ok_or(ImageError::Invalid("state"))?;
                    // Transparency where the image has it and the sink
                    // wants it; then colour.
                    let has_alpha =
                        matches!(h.color, 4 | 6) || (h.color == 3 && clear_entry) || key.is_some();
                    let with_alpha = has_alpha && sink.wants_alpha();
                    let color = !with_alpha && sink.wants_color();
                    let channels = if with_alpha {
                        2
                    } else if color {
                        3
                    } else {
                        1
                    };
                    let scaler = Downscaler::new(h.width, h.height, size, channels)?;
                    sink.start(scaler.out_width(), scaler.out_height())?;
                    let row_len = h.row_bytes() + 1;
                    rows = Some(Rows {
                        palette,
                        colors,
                        see_through,
                        key,
                        color,
                        alpha: with_alpha,
                        previous: zeroed(row_len)?,
                        current: zeroed(row_len)?,
                        filled: 0,
                        gray: zeroed(h.width as usize * channels)?,
                        done: 0,
                        scaler,
                        sink,
                        header: h,
                    });
                    decoder
                        .try_reserve_exact(1)
                        .map_err(|_| ImageError::OutOfMemory)?;
                    // Made in place: pushed, its 11 KB went over the stack
                    // first.
                    decoder.resize_with(1, DecompressorOxide::default);
                    window = zeroed(WINDOW)?;
                    chunk = zeroed(1024)?;
                }
                let Some(state) = rows.as_mut() else {
                    return Err(ImageError::Invalid("state"));
                };
                let mut left = len;
                while left > 0 {
                    let n = left.min(chunk.len());
                    reader.fill(&mut chunk[..n])?;
                    left -= n;
                    let mut input = &chunk[..n];
                    // The 2-byte zlib header precedes the deflate data.
                    let skip = zlib_header.min(input.len());
                    input = &input[skip..];
                    zlib_header -= skip;
                    while !input.is_empty() {
                        let (status, used, written) = decompress(
                            &mut decoder[0],
                            input,
                            &mut window,
                            position,
                            inflate_flags::TINFL_FLAG_HAS_MORE_INPUT,
                        );
                        input = &input[used..];
                        state.push(&window[position..position + written])?;
                        position = (position + written) & (WINDOW - 1);
                        match status {
                            TINFLStatus::Done => {
                                input = &[];
                            }
                            TINFLStatus::HasMoreOutput | TINFLStatus::NeedsMoreInput => {}
                            _ => return Err(ImageError::Invalid("corrupt PNG data")),
                        }
                        if used == 0 && written == 0 && status != TINFLStatus::HasMoreOutput {
                            break;
                        }
                    }
                }
            }
            b"IEND" => {
                let state = rows.as_mut().ok_or(ImageError::Invalid("no image data"))?;
                // Flush what the decoder still holds.
                loop {
                    let (status, _, written) =
                        decompress(&mut decoder[0], &[], &mut window, position, 0);
                    state.push(&window[position..position + written])?;
                    position = (position + written) & (WINDOW - 1);
                    if written == 0 || status != TINFLStatus::HasMoreOutput {
                        break;
                    }
                }
                if state.done < state.header.height {
                    return Err(ImageError::Invalid("truncated PNG data"));
                }
                return state.scaler.finish(state.sink);
            }
            _ => reader.skip(len)?,
        }
        // CRC
        reader.skip(4)?;
    }
}
