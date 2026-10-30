//! How a band of the screen PaperPaw draws goes into a display
//! controller's memory, for the controllers that keep the frame (the
//! SSD1677, and an UltraChip on a device without the memory for a frame
//! of its own): the window of the controller's memory a band covers, and
//! its bytes in the order the controller takes them.
//!
//! The controller's memory is seen as the SSD1677's: 480 rows (gates) of
//! 800 columns (sources), 8 columns a byte (the lowest column in the
//! highest bit, set for white). The upright screen is the panel turned
//! clockwise: the screen's row `y` is column `y`, its column `x` row `x`
//! (the gates are wired bottom to top). A controller whose rows run the
//! other way takes the rows of a window last first ([`Window::rows_back`]).
//!
//! Turned, the screen is 800 × 480 on its side or 480 × 800 upside down;
//! its rows go into the controller's memory as they lie on the panel (on
//! its side, a row of the screen is a row of the controller's).

use alloc::vec::Vec;

use paperpaw_platform::{FrameView, PixelFormat, Rotation};

use crate::display::{Error, HEIGHT, PANEL_HEIGHT, PANEL_WIDTH, WIDTH};

/// Bytes sent at once: each sending has its cost, a row of an upright
/// band takes 12 to 24 bytes.
pub(crate) const CHUNK: usize = 1024;

/// Where a band lies in the controller's memory, and in which order its
/// bytes go: always row by row, each row from its lowest column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Window {
    /// Columns `x..x + width` (a multiple of 8) of rows `y..y + height`.
    pub x: u16,
    pub width: u16,
    pub y: u16,
    pub height: u16,
    /// How the screen is turned.
    pub rotation: Rotation,
    /// The controller's rows run the other way: the window's rows are
    /// sent last first (the window itself the controller's to place).
    pub rows_back: bool,
}

impl Window {
    /// The window of screen rows `top..top + height` turned as `rotation`
    /// says.
    pub fn of(rotation: Rotation, top: u16, height: u16) -> Self {
        let (x, width, y, height) = match rotation {
            Rotation::Deg0 | Rotation::Deg180 => {
                // Whole bytes.
                let width = height.div_ceil(8) * 8;
                let x = if rotation == Rotation::Deg180 {
                    PANEL_WIDTH - top - width
                } else {
                    top
                };
                (x, width, 0, PANEL_HEIGHT)
            }
            _ => {
                let y = if rotation == Rotation::Deg90 {
                    top
                } else {
                    PANEL_HEIGHT - top - height
                };
                (0, PANEL_WIDTH, y, height)
            }
        };
        Self {
            x,
            width,
            y,
            height,
            rotation,
            rows_back: false,
        }
    }
}

/// Whether the pixel at `(x, y)` of a band is lighter than middle gray:
/// gray formats read straight from their bytes, others through their
/// colour.
pub(crate) fn light<'a>(rows: &'a FrameView<'a>) -> impl Fn(u16, u16) -> bool + 'a {
    let bpp = usize::from(rows.format.bits_per_pixel());
    let gray = rows.format.gray_levels().is_some() && bpp <= 8;
    let max = (1u32 << bpp.min(8)) - 1;
    move |x: u16, y: u16| {
        if gray {
            let bit = usize::from(x) * bpp;
            let at = usize::from(y) * rows.stride + bit / 8;
            let byte = rows.data.get(at).copied().unwrap_or(0xFF);
            let level = u32::from(byte >> (8 - bpp - bit % 8)) & max;
            level * 255 >= 128 * max
        } else {
            let c = rows.pixel(x, y);
            u16::from(c.r) * 77 + u16::from(c.g) * 150 + u16::from(c.b) * 29 >= 128 * 256
        }
    }
}

/// Sends the `height` rows of a band into `window` (from [`Window::of`]):
/// each pixel's bit (`bit`; black and white rows the quick way), `pad`
/// for rows past the band's end, gathered in `buffer` and passed to
/// `send` a chunk at a time.
pub(crate) fn send(
    window: &Window,
    rows: &FrameView<'_>,
    height: u16,
    bit: &dyn Fn(u16, u16) -> bool,
    pad: bool,
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    buffer.clear();
    if matches!(window.rotation, Rotation::Deg90 | Rotation::Deg270) {
        side(window, rows, height, bit, buffer, send)
    } else if rows.format == PixelFormat::Mono1 {
        mono(window, rows, height, buffer, send)
    } else {
        upright(window, height, bit, pad, buffer, send)
    }
}

/// Sends what `buffer` holds.
fn flush(
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    if buffer.is_empty() {
        return Ok(());
    }
    send(buffer)?;
    buffer.clear();
    Ok(())
}

/// An upright band (or upside down), pixel by pixel: each column of the
/// screen is a row of the controller's, the top of the band in the
/// lowest column.
fn upright(
    window: &Window,
    height: u16,
    light: &dyn Fn(u16, u16) -> bool,
    pad: bool,
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let bytes = height.div_ceil(8);
    // Upside down, the screen's last column lies in the first row.
    let flip = window.rotation == Rotation::Deg180;
    for i in 0..WIDTH {
        let row = if window.rows_back { WIDTH - 1 - i } else { i };
        let x = if flip { WIDTH - 1 - row } else { row };
        for j in 0..bytes {
            let byte = if flip { bytes - 1 - j } else { j };
            let mut bits = 0u8;
            for bit in 0..8u16 {
                let y = byte * 8 + bit;
                if if y >= height { pad } else { light(x, y) } {
                    bits |= 0x80 >> bit;
                }
            }
            buffer.push(if flip { bits.reverse_bits() } else { bits });
        }
        if buffer.len() >= CHUNK {
            flush(buffer, send)?;
        }
    }
    flush(buffer, send)
}

/// [`upright`] for black and white, as the X4 draws: eight rows by eight
/// columns at a time, turned with a few shifts where pixel by pixel took
/// a quarter of a page's time.
fn mono(
    window: &Window,
    rows: &FrameView<'_>,
    height: u16,
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let flip = window.rotation == Rotation::Deg180;
    let bytes = usize::from(height.div_ceil(8));
    // Each of eight columns' bytes, top to bottom.
    let mut columns = [0u8; 8 * (HEIGHT as usize / 8)];
    let blocks = usize::from(WIDTH) / 8;
    for i in 0..blocks {
        let block = if window.rows_back { blocks - 1 - i } else { i };
        let column = if flip { blocks - 1 - block } else { block };
        for group in 0..bytes {
            let mut eight = [0xFFu8; 8];
            for (i, row) in eight.iter_mut().enumerate() {
                let y = group * 8 + i;
                if y < usize::from(height) {
                    *row = rows
                        .data
                        .get(y * rows.stride + column)
                        .copied()
                        .unwrap_or(0xFF);
                }
            }
            for (c, byte) in transpose8(eight).into_iter().enumerate() {
                columns[c * bytes + group] = byte;
            }
        }
        for i in 0..8 {
            let row = if window.rows_back { 7 - i } else { i };
            let c = if flip { 7 - row } else { row };
            let column = &columns[c * bytes..(c + 1) * bytes];
            if flip {
                buffer.extend(column.iter().rev().map(|b| b.reverse_bits()));
            } else {
                buffer.extend_from_slice(column);
            }
        }
        if buffer.len() >= CHUNK {
            flush(buffer, send)?;
        }
    }
    flush(buffer, send)
}

/// A band of the screen on its side: each of its rows is a row of the
/// controller's, right to left (turned clockwise), or left to right from
/// the band's last row.
fn side(
    window: &Window,
    rows: &FrameView<'_>,
    height: u16,
    light: &dyn Fn(u16, u16) -> bool,
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let clockwise = window.rotation == Rotation::Deg90;
    let bytes = usize::from(PANEL_WIDTH / 8);
    for i in 0..height {
        let i = if window.rows_back { height - 1 - i } else { i };
        let y = if clockwise { i } else { height - 1 - i };
        let at = usize::from(y) * rows.stride;
        match rows.data.get(at..at + bytes) {
            // Black and white: as it is, or each byte the other way.
            Some(row) if rows.format == PixelFormat::Mono1 => {
                if clockwise {
                    buffer.extend(row.iter().rev().map(|b| b.reverse_bits()));
                } else {
                    buffer.extend_from_slice(row);
                }
            }
            _ => {
                for byte in 0..bytes {
                    let mut bits = 0u8;
                    for bit in 0..8 {
                        let x = (byte * 8 + bit) as u16;
                        let x = if clockwise { PANEL_WIDTH - 1 - x } else { x };
                        if light(x, y) {
                            bits |= 0x80 >> bit;
                        }
                    }
                    buffer.push(bits);
                }
            }
        }
        if buffer.len() >= CHUNK {
            flush(buffer, send)?;
        }
    }
    flush(buffer, send)
}

/// Turns eight bytes (rows, the leftmost pixel in the highest bit) into
/// eight bytes of their columns (the top row in the highest bit).
pub(crate) fn transpose8(rows: [u8; 8]) -> [u8; 8] {
    let mut x = u64::from_be_bytes(rows);
    let t = (x ^ (x >> 7)) & 0x00AA_00AA_00AA_00AA;
    x ^= t ^ (t << 7);
    let t = (x ^ (x >> 14)) & 0x0000_CCCC_0000_CCCC;
    x ^= t ^ (t << 14);
    let t = (x ^ (x >> 28)) & 0x0000_0000_F0F0_F0F0;
    x ^= t ^ (t << 28);
    x.to_be_bytes()
}
