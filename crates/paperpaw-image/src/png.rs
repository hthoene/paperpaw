//! Non-interlaced PNG, inflated and unfiltered one row at a time.

use alloc::vec::Vec;

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};

use crate::{Downscaler, Fit, ImageError, ImageSink, Reader, zeroed};

/// The deflate history (a power of two: the window wraps).
const WINDOW: usize = 32 * 1024;

/// The inflate flag for a window that holds all the data and does not wrap.
fn non_wrapping(whole: bool) -> u32 {
    if whole {
        inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
    } else {
        0
    }
}

/// Where the window is written next after `written` bytes at `position`.
fn advance(position: usize, written: usize, whole: bool) -> usize {
    if whole {
        position + written
    } else {
        (position + written) & (WINDOW - 1)
    }
}

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

// Not inlined into `decode`, so each format's frame is on the stack alone,
// not all three together.
#[inline(never)]
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
    // rows. Dropped once the rows' tables are made from it.
    let mut entries: Option<Palette> = None;
    let mut key: Option<[u16; 3]> = None;
    let mut rows: Option<Rows<'_>> = None;
    let mut sink = Some(sink);
    // Inflate state, created at the first IDAT chunk.
    let mut inflate = Inflate::default();

    loop {
        let len = reader.u32_be()? as usize;
        let mut kind = [0u8; 4];
        reader.fill(&mut kind)?;
        match &kind {
            b"IHDR" => header = Some(read_header(reader, len)?),
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
                let gray = header.as_ref().is_some_and(|h| h.color == 0);
                key = read_key(reader, len, gray)?.or(key);
            }
            b"IDAT" => {
                if rows.is_none() {
                    let h = header
                        .take()
                        .ok_or(ImageError::Invalid("IDAT before IHDR"))?;
                    let sink = sink.take().ok_or(ImageError::Invalid("state"))?;
                    let (state, data) = start_rows(h, entries.take(), key, fit, sink)?;
                    rows = Some(state);
                    inflate.start(data)?;
                }
                let Some(state) = rows.as_mut() else {
                    return Err(ImageError::Invalid("state"));
                };
                inflate.feed(reader, len, state)?;
            }
            b"IEND" => {
                let state = rows.as_mut().ok_or(ImageError::Invalid("no image data"))?;
                inflate.finish(state)?;
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

/// The image header (`IHDR`), `len` bytes, checked.
fn read_header(reader: &mut Reader<'_>, len: usize) -> Result<Header, ImageError> {
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
        return Err(ImageError::TooLarge);
    }
    Ok(Header {
        width,
        height,
        depth,
        color,
    })
}

/// The colour of a `gray` or RGB image that is shown as clear (`tRNS`),
/// `len` bytes. `None` where it is too short to say one.
fn read_key(
    reader: &mut Reader<'_>,
    len: usize,
    gray: bool,
) -> Result<Option<[u16; 3]>, ImageError> {
    let samples = if gray { 1 } else { 3 };
    if len < samples * 2 {
        reader.skip(len)?;
        return Ok(None);
    }
    let mut value = [0u16; 3];
    for sample in value.iter_mut().take(samples) {
        *sample = u16::from_be_bytes([reader.byte()?, reader.byte()?]);
    }
    reader.skip(len - samples * 2)?;
    Ok(Some(value))
}

/// The rows of image `h`, its tables made from the palette `entries`,
/// handed to `sink` at the size `fit` asks for. Also the bytes of its
/// data inflated.
fn start_rows<'s>(
    h: Header,
    mut entries: Option<Palette>,
    key: Option<[u16; 3]>,
    fit: Fit,
    sink: &'s mut dyn ImageSink,
) -> Result<(Rows<'s>, u64), ImageError> {
    // Only a palette image has entries to show.
    let (mut palette, mut colors, mut see_through) = (Vec::new(), Vec::new(), Vec::new());
    let mut clear_entry = false;
    if h.color == 3 {
        let entries = match &mut entries {
            Some(entries) => entries,
            none => none.insert(Palette::new()?),
        };
        clear_entry = entries.alpha.iter().any(|&a| a < 255);
        (palette, colors, see_through) = entry_tables(entries)?;
    }
    drop(entries);
    let size = fit.size(h.width, h.height);
    // Transparency where the image has it and the sink wants it, else
    // colour where the sink wants it.
    let has_alpha = matches!(h.color, 4 | 6) || (h.color == 3 && clear_entry) || key.is_some();
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
    let data = u64::from(h.height).saturating_mul(row_len as u64);
    let rows = Rows {
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
    };
    Ok((rows, data))
}

/// Inflating the image data of the `IDAT` chunks.
struct Inflate {
    decoder: Vec<DecompressorOxide>,
    window: Vec<u8>,
    position: usize,
    /// Inflating into a window of exactly the image's data, which fits the
    /// history: the output never wraps.
    whole: bool,
    /// The data went on past the rows of an image whose window holds just
    /// them: nothing more is inflated.
    surplus: bool,
    zlib_header: usize,
    /// Read from the stream a piece at a time. Made with the window.
    chunk: Vec<u8>,
}

impl Default for Inflate {
    fn default() -> Self {
        Inflate {
            decoder: Vec::new(),
            window: Vec::new(),
            position: 0,
            whole: false,
            surplus: false,
            zlib_header: 2,
            chunk: Vec::new(),
        }
    }
}

impl Inflate {
    /// Makes the decoder and the window for `data` bytes of image data.
    fn start(&mut self, data: u64) -> Result<(), ImageError> {
        // An image whose rows fit the deflate history inflates whole into
        // a window of its size: a 6 KiB icon no longer asks for a 32 KiB
        // piece. A larger one wraps through the full history.
        self.whole = data <= WINDOW as u64;
        self.decoder
            .try_reserve_exact(1)
            .map_err(|_| ImageError::OutOfMemory)?;
        // Made in place: pushed, its 11 KB went over the stack first.
        self.decoder.resize_with(1, DecompressorOxide::default);
        self.window = zeroed(if self.whole {
            (data as usize).max(1)
        } else {
            WINDOW
        })?;
        self.chunk = zeroed(1024)?;
        Ok(())
    }

    /// Inflates an `IDAT` chunk of `len` bytes into the rows.
    fn feed(
        &mut self,
        reader: &mut Reader<'_>,
        len: usize,
        state: &mut Rows<'_>,
    ) -> Result<(), ImageError> {
        let whole = self.whole;
        let mut left = len;
        while left > 0 {
            let n = left.min(self.chunk.len());
            reader.fill(&mut self.chunk[..n])?;
            left -= n;
            let mut input = &self.chunk[..n];
            // The 2-byte zlib header precedes the deflate data.
            let skip = self.zlib_header.min(input.len());
            input = &input[skip..];
            self.zlib_header -= skip;
            while !input.is_empty() && !self.surplus {
                let (status, used, written) = decompress(
                    &mut self.decoder[0],
                    input,
                    &mut self.window,
                    self.position,
                    inflate_flags::TINFL_FLAG_HAS_MORE_INPUT | non_wrapping(whole),
                );
                input = &input[used..];
                state.push(&self.window[self.position..self.position + written])?;
                self.position = advance(self.position, written, whole);
                if whole
                    && status == TINFLStatus::HasMoreOutput
                    && self.position == self.window.len()
                {
                    // More data than the rows: the rest is left unread, as
                    // a wrapping window's is.
                    self.surplus = true;
                    input = &[];
                }
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
        Ok(())
    }

    /// Flushes what the decoder still holds into the rows.
    fn finish(&mut self, state: &mut Rows<'_>) -> Result<(), ImageError> {
        let whole = self.whole;
        loop {
            if self.surplus {
                break;
            }
            let (status, _, written) = decompress(
                &mut self.decoder[0],
                &[],
                &mut self.window,
                self.position,
                non_wrapping(whole),
            );
            state.push(&self.window[self.position..self.position + written])?;
            self.position = advance(self.position, written, whole);
            if whole && status == TINFLStatus::HasMoreOutput && self.position == self.window.len() {
                break;
            }
            if written == 0 || status != TINFLStatus::HasMoreOutput {
                break;
            }
        }
        Ok(())
    }
}
