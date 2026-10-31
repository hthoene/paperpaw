//! A finished band of a frame, for a render hook to change before the
//! shell gets it (ADR 0012: the screen is drawn in bands of 100 rows).
//!
//! A hook sees pixels, not glyph coverage: in a one-bit band the edge of a
//! letter is already black or white. It may change the band's rows and its
//! `carry`, nothing else; where it goes and how large it is are the
//! kernel's.
//!
//! Coordinates are **frame (panel) coordinates**: the strip of the panel
//! the frame format holds, in the order bands are made. In a rotated
//! session the logical screen is turned against it; [`BandView::rotation`]
//! says by how much.

use core::fmt;
use core::ops::BitOr;

use paperpaw_platform::{Color, PixelFormat, Rect, Rotation};

/// Which kind of pass of a frame a band belongs to: one value in
/// [`BandView::pass`], and a set where a hook says which passes it wants
/// (join them with `|`, ask with [`FramePass::contains`]). Constants of one
/// open type, so more can come without a new plugin interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FramePass(u16);

impl FramePass {
    /// The ordinary frame in the panel's own format (black and white on
    /// the X4).
    pub const MONO: Self = Self(1);
    /// The slow pass in four grays that shows smooth letters and pictures.
    pub const GRAY: Self = Self(2);
    /// A pass of a colour panel. Reserved: none is made yet.
    pub const COLOUR: Self = Self(4);

    /// The pass's number: one bit each, the others are free.
    pub const fn value(self) -> u16 {
        self.0
    }

    /// The passes of a number.
    pub const fn from_value(value: u16) -> Self {
        Self(value)
    }

    /// Whether every pass of `other` is in this set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0 && other.0 != 0
    }
}

impl BitOr for FramePass {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// What is in a [`Region`] of a band: constants of one open type. A hook
/// ignores the kinds it does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionKind(u16);

impl RegionKind {
    /// The text of a book page.
    pub const PAGE_TEXT: Self = Self(1);
    /// A picture.
    pub const IMAGE: Self = Self(2);
    /// The interface around a page: headers, status lines, buttons.
    pub const CHROME: Self = Self(3);
    /// The keyboard on the screen.
    pub const KEYBOARD: Self = Self(4);

    /// The kind's number; 0 is no kind.
    pub const fn value(self) -> u16 {
        self.0
    }

    /// The kind of a number.
    pub const fn from_value(value: u16) -> Self {
        Self(value)
    }
}

/// A rectangle of a band and what is in it: a hint, see
/// [`BandView::regions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    kind: RegionKind,
    rect: Rect,
    /// Zero now.
    spare: u32,
}

impl Region {
    /// `kind` in `rect` (frame coordinates).
    pub const fn new(kind: RegionKind, rect: Rect) -> Self {
        Self {
            kind,
            rect,
            spare: 0,
        }
    }

    /// What is in it.
    pub const fn kind(&self) -> RegionKind {
        self.kind
    }

    /// Where it is, in frame coordinates.
    pub const fn rect(&self) -> Rect {
        self.rect
    }
}

/// One band of a frame as a render hook sees and changes it.
///
/// `gray`, `put_gray` and `map_gray` work whatever the packing (a format
/// this release does not know reads 0 and ignores writes; `row` and
/// `row_mut` still give its bytes). Coordinates out of range are ignored,
/// never a panic.
///
/// For the same band, `above`, `carry` and settings a hook must give the
/// same rows and carry: no clock, no random (a hook has none to ask).
pub struct BandView<'a> {
    rows: &'a mut [u8],
    above: &'a [u8],
    carry: &'a mut [u8],
    regions: &'a [Region],
    stride: u32,
    area: Rect,
    format: PixelFormat,
    pass: FramePass,
    inverted: bool,
    /// Bit 0: the first band of the pass (the carry has just been zeroed).
    flags: u32,
    /// Slot 0: the rotation (0 to 3 quarter turns); 1 to 3 reserved, zero.
    slots: [u32; 4],
}

impl<'a> BandView<'a> {
    /// A band of `area` (frame coordinates, the full panel width) whose
    /// `rows` hold the format's own packing. With no rows above, no carry
    /// and no regions, as the kernel has none until it runs hooks.
    pub fn new(rows: &'a mut [u8], area: Rect, format: PixelFormat, pass: FramePass) -> Self {
        Self {
            rows,
            above: &[],
            carry: &mut [],
            regions: &[],
            stride: format.stride(area.width) as u32,
            area,
            format,
            pass,
            inverted: false,
            flags: 0,
            slots: [0; 4],
        }
    }

    /// With the `above` rows before the band (final: hooks already
    /// applied).
    #[must_use]
    pub fn with_above(mut self, above: &'a [u8]) -> Self {
        self.above = above;
        self
    }

    /// With the kernel's scratch for the pass, `carry`, and whether this
    /// is the first band of the pass (the carry has just been zeroed).
    #[must_use]
    pub fn with_carry(mut self, carry: &'a mut [u8], first: bool) -> Self {
        self.carry = carry;
        self.flags = (self.flags & !1) | u32::from(first);
        self
    }

    /// With what the band holds, as hints.
    #[must_use]
    pub fn with_regions(mut self, regions: &'a [Region]) -> Self {
        self.regions = regions;
        self
    }

    /// With the logical screen's rotation against the panel, and whether
    /// the frame is inverted (night mode: black and white swapped in it).
    #[must_use]
    pub fn with_screen(mut self, rotation: Rotation, inverted: bool) -> Self {
        self.slots[0] = match rotation {
            Rotation::Deg90 => 1,
            Rotation::Deg180 => 2,
            Rotation::Deg270 => 3,
            _ => 0,
        };
        self.inverted = inverted;
        self
    }

    /// Where the band is, in frame coordinates.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// The frame's pixel format.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// Which pass this band belongs to (one value).
    pub fn pass(&self) -> FramePass {
        self.pass
    }

    /// Width in pixels.
    pub fn width(&self) -> u16 {
        self.area.width
    }

    /// Height in rows.
    pub fn height(&self) -> u16 {
        self.area.height
    }

    /// Whether black and white are swapped in the frame (night mode).
    pub fn inverted(&self) -> bool {
        self.inverted
    }

    /// The logical screen's rotation against the panel: grain direction,
    /// watermarks and region tests can be turned into logical terms with
    /// it.
    pub fn rotation(&self) -> Rotation {
        match self.slots[0] {
            1 => Rotation::Deg90,
            2 => Rotation::Deg180,
            3 => Rotation::Deg270,
            _ => Rotation::Deg0,
        }
    }

    /// Whether this is the first band the hook sees in this pass: the
    /// carry has just been zeroed.
    pub fn is_first(&self) -> bool {
        self.flags & 1 != 0
    }

    /// The kernel's scratch for the pass, as large as it granted (at most
    /// what the hook asked for, perhaps none): zeroed at the start of
    /// each pass and kept from band to band, top to bottom. Check its
    /// length.
    pub fn carry(&self) -> &[u8] {
        self.carry
    }

    /// [`BandView::carry`], to change.
    pub fn carry_mut(&mut self) -> &mut [u8] {
        self.carry
    }

    /// What the band holds, as hints in frame coordinates, in the order it
    /// was drawn (later on top); they may overlap. **Empty means unknown,
    /// not nothing**: treat the whole band as without regions. Ignore the
    /// kinds you do not know.
    pub fn regions(&self) -> &[Region] {
        self.regions
    }

    /// The row at screen `y`, packed as the format has it: a row of the
    /// band, or one of the rows above it. `None` outside them.
    pub fn row(&self, y: u16) -> Option<&[u8]> {
        let stride = self.stride as usize;
        let top = self.area.y;
        if y >= top {
            let at = usize::from(y - top);
            self.rows.get(at * stride..(at + 1) * stride)
        } else {
            let above = self.above.len().checked_div(stride)?;
            let back = usize::from(top - y);
            let at = above.checked_sub(back)?;
            self.above.get(at * stride..(at + 1) * stride)
        }
    }

    /// The row at screen `y` of the band, to change. `None` outside it
    /// (the rows above are final).
    pub fn row_mut(&mut self, y: u16) -> Option<&mut [u8]> {
        let stride = self.stride as usize;
        let at = usize::from(y.checked_sub(self.area.y)?);
        if at >= usize::from(self.area.height) {
            return None;
        }
        self.rows.get_mut(at * stride..(at + 1) * stride)
    }

    /// The gray, 0 (black) to 255 (white), of the pixel at `x`, `y`
    /// (screen y; rows above count). 0 outside, and in a format this
    /// release does not know.
    pub fn gray(&self, x: u16, y: u16) -> u8 {
        if x >= self.area.width {
            return 0;
        }
        self.row(y).map_or(0, |row| read(self.format, row, x))
    }

    /// Sets the pixel at `x`, `y` of the band to `gray`, as near as the
    /// format has (one bit: from 128 on is white). Ignored outside the
    /// band and in a format this release does not know.
    pub fn put_gray(&mut self, x: u16, y: u16, gray: u8) {
        let (width, format) = (self.area.width, self.format);
        if x < width
            && let Some(row) = self.row_mut(y)
        {
            write(format, row, x, gray);
        }
    }

    /// Every pixel of the band through `f`, row by row from the top left.
    pub fn map_gray(&mut self, mut f: impl FnMut(u8) -> u8) {
        let (width, format) = (self.area.width, self.format);
        let top = self.area.y;
        for y in top..top.saturating_add(self.area.height) {
            if let Some(row) = self.row_mut(y) {
                for x in 0..width {
                    let was = read(format, row, x);
                    write(format, row, x, f(was));
                }
            }
        }
    }
}

impl fmt::Debug for BandView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BandView")
            .field("area", &self.area)
            .field("format", &self.format)
            .field("pass", &self.pass)
            .field("carry", &self.carry.len())
            .field("regions", &self.regions.len())
            .finish_non_exhaustive()
    }
}

/// The gray of pixel `x` of a packed `row` (most significant bit first).
fn read(format: PixelFormat, row: &[u8], x: u16) -> u8 {
    let x = usize::from(x);
    match format {
        PixelFormat::Mono1 => row
            .get(x / 8)
            .map_or(0, |b| if b >> (7 - x % 8) & 1 == 1 { 255 } else { 0 }),
        PixelFormat::Gray2 => row
            .get(x / 4)
            .map_or(0, |b| (b >> (6 - 2 * (x % 4)) & 3) * 85),
        PixelFormat::Gray4 => row
            .get(x / 2)
            .map_or(0, |b| (b >> (4 - 4 * (x % 2)) & 15) * 17),
        PixelFormat::Gray8 => row.get(x).copied().unwrap_or(0),
        PixelFormat::Rgb888 => row
            .get(x * 3..x * 3 + 3)
            .map_or(0, |p| Color::rgb(p[0], p[1], p[2]).luma()),
        PixelFormat::Rgb565 => row.get(x * 2..x * 2 + 2).map_or(0, |p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
            Color::rgb(
                (r * 255 / 31) as u8,
                (g * 255 / 63) as u8,
                (b * 255 / 31) as u8,
            )
            .luma()
        }),
        _ => 0,
    }
}

/// Sets pixel `x` of a packed `row` to the nearest gray the format has.
fn write(format: PixelFormat, row: &mut [u8], x: u16, gray: u8) {
    let x = usize::from(x);
    let set = |byte: Option<&mut u8>, shift: usize, mask: u8, value: u8| {
        if let Some(byte) = byte {
            *byte = *byte & !(mask << shift) | (value & mask) << shift;
        }
    };
    match format {
        PixelFormat::Mono1 => set(row.get_mut(x / 8), 7 - x % 8, 1, u8::from(gray >= 128)),
        PixelFormat::Gray2 => set(
            row.get_mut(x / 4),
            6 - 2 * (x % 4),
            3,
            ((u16::from(gray) + 42) / 85) as u8,
        ),
        PixelFormat::Gray4 => set(
            row.get_mut(x / 2),
            4 - 4 * (x % 2),
            15,
            ((u16::from(gray) + 8) / 17) as u8,
        ),
        PixelFormat::Gray8 => {
            if let Some(byte) = row.get_mut(x) {
                *byte = gray;
            }
        }
        PixelFormat::Rgb888 => {
            if let Some(pixel) = row.get_mut(x * 3..x * 3 + 3) {
                pixel.fill(gray);
            }
        }
        PixelFormat::Rgb565 => {
            if let Some(pixel) = row.get_mut(x * 2..x * 2 + 2) {
                let (r, g) = (u16::from(gray) * 31 / 255, u16::from(gray) * 63 / 255);
                pixel.copy_from_slice(&(r << 11 | g << 5 | r).to_le_bytes());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "band_tests.rs"]
mod tests;
