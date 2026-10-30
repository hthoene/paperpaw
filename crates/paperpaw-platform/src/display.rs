//! Display description and the display interface implemented by shells.

use crate::geometry::Rect;
use crate::pixel::{Color, PixelFormat};

/// How the logical (upright) screen is rotated relative to the panel's
/// native scan direction, clockwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

/// E-ink refresh modes, from slowest/cleanest to fastest/dirtiest.
///
/// Shells map these to the closest waveform their panel supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum RefreshMode {
    /// Black and white only, the fastest (A2-like): pen strokes, a
    /// slider dragged. Ordered from the lightest to the strongest.
    Mono,
    /// Fast, low quality (A2/DU-like): menus, highlights, pen strokes.
    Fast,
    /// Normal partial update without flashing: page turns.
    Partial,
    /// Every gray the panel shows, without flashing (GC16-like): a page
    /// with pictures.
    Gray,
    /// Flashing full update that removes ghosting.
    Full,
    /// The panel cleared to white before the frame shows, as an e-ink
    /// transition (to a cover, a sleep screen).
    Clear,
}

impl RefreshMode {
    /// Whether it flashes the panel and so removes ghosting.
    pub fn cleans(self) -> bool {
        matches!(self, RefreshMode::Full | RefreshMode::Clear)
    }
}

/// Properties of a colour filter panel (Kaleido, Gallery, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ColorPanel {
    /// Colour resolution relative to the gray resolution, e.g. `2` when the
    /// colour filter covers 2×2 pixels.
    pub color_divisor: u8,
    /// Typical saturation the panel can show, in percent of sRGB.
    pub saturation_percent: u8,
}

impl ColorPanel {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(color_divisor: u8, saturation_percent: u8) -> Self {
        Self {
            color_divisor,
            saturation_percent,
        }
    }

    #[must_use]
    pub const fn with_color_divisor(mut self, color_divisor: u8) -> Self {
        self.color_divisor = color_divisor;
        self
    }

    #[must_use]
    pub const fn with_saturation_percent(mut self, saturation_percent: u8) -> Self {
        self.saturation_percent = saturation_percent;
        self
    }
}

impl ColorPanel {
    /// How much stronger pictures' colours are drawn, in percent, to make
    /// up for what the filter mutes: at most twice as strong.
    pub fn boost_percent(&self) -> u16 {
        (10_000 / u16::from(self.saturation_percent.max(1))).clamp(100, 200)
    }
}

/// Everything the core needs to know about a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DisplayInfo {
    /// Logical width after rotation, in pixels.
    pub width: u16,
    /// Logical height after rotation, in pixels.
    pub height: u16,
    /// Physical pixel density.
    pub dpi: u16,
    /// Pixel format of the framebuffer the core renders into.
    pub format: PixelFormat,
    /// Rotation of the logical screen relative to the panel.
    pub rotation: Rotation,
    /// Colour filter properties, `None` for grayscale panels.
    pub color: Option<ColorPanel>,
}

impl DisplayInfo {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(
        width: u16,
        height: u16,
        dpi: u16,
        format: PixelFormat,
        rotation: Rotation,
        color: Option<ColorPanel>,
    ) -> Self {
        Self {
            width,
            height,
            dpi,
            format,
            rotation,
            color,
        }
    }

    #[must_use]
    pub const fn with_width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    #[must_use]
    pub const fn with_height(mut self, height: u16) -> Self {
        self.height = height;
        self
    }

    #[must_use]
    pub const fn with_dpi(mut self, dpi: u16) -> Self {
        self.dpi = dpi;
        self
    }

    #[must_use]
    pub const fn with_format(mut self, format: PixelFormat) -> Self {
        self.format = format;
        self
    }

    #[must_use]
    pub const fn with_rotation(mut self, rotation: Rotation) -> Self {
        self.rotation = rotation;
        self
    }

    #[must_use]
    pub const fn with_color(mut self, color: Option<ColorPanel>) -> Self {
        self.color = color;
        self
    }
}

/// A read-only view of a framebuffer.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct FrameView<'a> {
    pub width: u16,
    pub height: u16,
    pub format: PixelFormat,
    /// Bytes per row.
    pub stride: usize,
    pub data: &'a [u8],
}

impl<'a> FrameView<'a> {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(
        width: u16,
        height: u16,
        format: PixelFormat,
        stride: usize,
        data: &'a [u8],
    ) -> Self {
        Self {
            width,
            height,
            format,
            stride,
            data,
        }
    }

    #[must_use]
    pub const fn with_width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    #[must_use]
    pub const fn with_height(mut self, height: u16) -> Self {
        self.height = height;
        self
    }

    #[must_use]
    pub const fn with_format(mut self, format: PixelFormat) -> Self {
        self.format = format;
        self
    }

    #[must_use]
    pub const fn with_stride(mut self, stride: usize) -> Self {
        self.stride = stride;
        self
    }

    #[must_use]
    pub const fn with_data(mut self, data: &'a [u8]) -> Self {
        self.data = data;
        self
    }
}

impl FrameView<'_> {
    /// Decodes the pixel at `(x, y)`. Out-of-range coordinates read as white.
    pub fn pixel(&self, x: u16, y: u16) -> Color {
        if x >= self.width || y >= self.height {
            return Color::WHITE;
        }
        let row = y as usize * self.stride;
        let read = |index: usize| self.data.get(row + index).copied().unwrap_or(0xff);
        match self.format {
            PixelFormat::Rgb565 => {
                let i = x as usize * 2;
                let v = u16::from_le_bytes([read(i), read(i + 1)]);
                let expand = |value: u16, bits: u32| {
                    let max = (1u32 << bits) - 1;
                    ((value as u32 * 255 + max / 2) / max) as u8
                };
                Color::rgb(
                    expand(v >> 11, 5),
                    expand((v >> 5) & 0x3f, 6),
                    expand(v & 0x1f, 5),
                )
            }
            PixelFormat::Rgb888 => {
                let i = x as usize * 3;
                Color::rgb(read(i), read(i + 1), read(i + 2))
            }
            format => {
                let bpp = format.bits_per_pixel() as usize;
                let bit = x as usize * bpp;
                let shift = 8 - bpp - bit % 8;
                let max = (1u16 << bpp) - 1;
                let level = (read(bit / 8) >> shift) as u16 & max;
                Color::gray((level * 255 / max) as u8)
            }
        }
    }
}

/// A physical (or emulated) display.
pub trait Display {
    /// Describes the display.
    fn info(&self) -> DisplayInfo;

    /// Transfers `region` of `frame` to the panel and refreshes it.
    fn refresh(&mut self, frame: FrameView<'_>, region: Rect, mode: RefreshMode);
}
