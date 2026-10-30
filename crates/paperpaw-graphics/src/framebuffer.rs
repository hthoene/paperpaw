use alloc::vec::Vec;

use paperpaw_platform::{Color, FrameView, PixelFormat, Rect};

use crate::GraphicsError;
use crate::canvas::Canvas;
use crate::diffuse::Atkinson;

/// From what coverage (of 255) a pixel at an edge takes the colour drawn
/// in black and white, at first: three eighths (see
/// [`Framebuffer::set_mono_coverage`]).
pub const MONO_COVERAGE: u8 = 96;

/// Pixels in a device's native format, plus dirty-region tracking.
///
/// A framebuffer holds either the whole screen or a *band* of rows
/// ([`Framebuffer::banded`]): the screen is then drawn band by band, each
/// passed to the panel before the next. Coordinates are always screen
/// coordinates; drawing outside the current band is discarded.
#[derive(Debug)]
pub struct Framebuffer {
    width: u16,
    height: u16,
    format: PixelFormat,
    stride: usize,
    /// First screen row held.
    top: u16,
    /// Number of rows held.
    rows: u16,
    data: Vec<u8>,
    dirty: Option<Rect>,
    inverted: bool,
    /// Pictures stay as they are while everything else is inverted.
    pictures_kept: bool,
    /// From what coverage an edge pixel takes the colour, in black and
    /// white.
    mono_coverage: u8,
    /// Saturation of RGB pixels drawn, in percent (100: as they are).
    saturation: u16,
    /// Pixels of pictures drawn since [`Framebuffer::take_pictured`].
    pictured: u32,
    /// Pictures drawn now count there (not small ones, see
    /// [`Canvas::small_pictures`](crate::Canvas::small_pictures)).
    counting: bool,
    /// Pixels of all pictures drawn since
    /// [`Framebuffer::take_all_pictured`], small ones too.
    all_pictured: u32,
    /// Holds no pixels: what is drawn is only described (see
    /// [`Framebuffer::blind`]).
    blind: bool,
    /// Pictures diffused across the end of the band: each one's key, the
    /// screen row where the next band takes it up, and its error there
    /// (see [`Canvas::keep_diffusion`](crate::Canvas::keep_diffusion)).
    diffusions: Vec<(u64, u16, Atkinson)>,
}

/// Pictures a band leaves diffused for the next one, at most; more start
/// from their first row again.
const DIFFUSIONS: usize = 8;

impl Framebuffer {
    /// Allocates a white framebuffer. Fails instead of aborting when memory
    /// is short, which matters on devices with a few hundred KB of RAM.
    pub fn new(width: u16, height: u16, format: PixelFormat) -> Result<Self, GraphicsError> {
        Self::banded(width, height, height, format)
    }

    /// A framebuffer for a `width`×`height` screen that holds `rows` rows
    /// at a time, starting with the top band.
    pub fn banded(
        width: u16,
        height: u16,
        rows: u16,
        format: PixelFormat,
    ) -> Result<Self, GraphicsError> {
        if format.is_color() && !cfg!(feature = "color") {
            return Err(GraphicsError::UnsupportedFormat(format));
        }
        let rows = rows.clamp(1, height.max(1));
        let stride = format.stride(width);
        let bytes = stride * rows as usize;
        let mut data = Vec::new();
        data.try_reserve_exact(bytes)
            .map_err(|_| GraphicsError::OutOfMemory { bytes })?;
        data.resize(bytes, 0xff);
        Ok(Self {
            width,
            height,
            format,
            stride,
            top: 0,
            rows,
            data,
            dirty: None,
            inverted: false,
            pictures_kept: false,
            mono_coverage: MONO_COVERAGE,
            saturation: 100,
            pictured: 0,
            all_pictured: 0,
            counting: true,
            blind: false,
            diffusions: Vec::new(),
        })
    }

    /// A framebuffer for a `width`×`height` screen that holds no pixels:
    /// drawing on it costs no memory and draws nothing, but a canvas of
    /// it describes what is drawn ([`Canvas::describing`]) as on the
    /// screen, e.g. to say what a screen shows (48 KB less on a
    /// 800×480 screen in black and white).
    pub fn blind(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            format: PixelFormat::Mono1,
            stride: 0,
            top: 0,
            rows: height.max(1),
            data: Vec::new(),
            dirty: None,
            inverted: false,
            pictures_kept: false,
            mono_coverage: MONO_COVERAGE,
            saturation: 100,
            pictured: 0,
            all_pictured: 0,
            counting: true,
            blind: true,
            diffusions: Vec::new(),
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    /// Whether it holds no pixels ([`Framebuffer::blind`]).
    pub fn is_blind(&self) -> bool {
        self.blind
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// The full screen rectangle.
    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.width, self.height)
    }

    /// Rows held at a time; the screen height unless banded.
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Holds `rows` rows at a time from now on, e.g. fewer to give memory
    /// back (more bands a frame, drawn more slowly), from the top band.
    /// Only where it holds bands already, not all of the screen: what it
    /// holds is drawn anew each band.
    pub fn set_rows(&mut self, rows: u16) -> Result<(), GraphicsError> {
        let rows = rows.clamp(1, self.height.max(1));
        if self.blind || self.rows >= self.height || rows == self.rows {
            return Ok(());
        }
        let bytes = self.stride * rows as usize;
        if bytes > self.data.len() {
            self.data
                .try_reserve_exact(bytes - self.data.len())
                .map_err(|_| GraphicsError::OutOfMemory { bytes })?;
        }
        self.data.resize(bytes, 0xff);
        self.data.shrink_to_fit();
        self.rows = rows;
        self.top = 0;
        self.dirty = None;
        self.diffusions.clear();
        Ok(())
    }

    /// The part of the screen currently held.
    pub fn band(&self) -> Rect {
        Rect::new(
            0,
            self.top,
            self.width,
            self.rows.min(self.height - self.top),
        )
    }

    /// Moves the band to start at screen row `top`. The pixels are not
    /// cleared; draw the band completely afterwards.
    pub fn set_top(&mut self, top: u16) {
        self.top = top.min(self.height.saturating_sub(1));
        // Only what the band now begins with is still of use.
        let top = self.top;
        self.diffusions.retain(|(_, row, _)| *row == top);
    }

    /// The frame drawn in all its bands: nothing kept for the next band
    /// any more.
    pub fn end_frame(&mut self) {
        self.diffusions = Vec::new();
    }

    /// The error of picture `key` at screen row `row`, where the band
    /// before left it.
    pub(crate) fn take_diffusion(&mut self, key: u64, row: u16) -> Option<Atkinson> {
        let at = self
            .diffusions
            .iter()
            .position(|(k, r, _)| *k == key && *r == row)?;
        Some(self.diffusions.swap_remove(at).2)
    }

    /// Keeps the error of picture `key` at screen row `row` for the next
    /// band, where this band ends there and the frame goes on.
    pub(crate) fn keep_diffusion(&mut self, key: u64, row: u16, atkinson: Atkinson) {
        let end = u32::from(self.top) + u32::from(self.rows);
        if u32::from(row) != end || row >= self.height {
            return;
        }
        self.diffusions.retain(|(k, _, _)| *k != key);
        if self.diffusions.len() < DIFFUSIONS && self.diffusions.try_reserve(1).is_ok() {
            self.diffusions.push((key, row, atkinson));
        }
    }

    /// Copies `rows` (full-width rows in the same format, starting at
    /// screen row `top`) into this framebuffer, e.g. to keep a copy of the
    /// whole screen from bands. Rows outside the current band are skipped.
    pub fn write_rows(&mut self, top: u16, rows: FrameView<'_>) {
        if self.blind {
            return;
        }
        if rows.format != self.format || rows.width != self.width {
            return;
        }
        let band = self.band();
        for y in 0..rows.height {
            let screen_y = top.saturating_add(y);
            if screen_y < band.y || u32::from(screen_y) >= band.bottom() {
                continue;
            }
            let from = usize::from(y) * rows.stride;
            let to = usize::from(screen_y - self.top) * self.stride;
            let len = self.stride.min(rows.stride);
            if let Some(source) = rows.data.get(from..from + len) {
                self.data[to..to + len].copy_from_slice(source);
            }
        }
    }

    /// Number of bytes used by the pixel data.
    pub fn byte_len(&self) -> usize {
        self.data.len()
    }

    /// The rows held, starting at screen row [`Framebuffer::band`]`.y`.
    pub fn view(&self) -> FrameView<'_> {
        self.view_rows(self.band())
    }

    /// The held rows that `region` covers, as full-width rows starting at
    /// screen row `region.y` (clamped to the band).
    pub fn view_rows(&self, region: Rect) -> FrameView<'_> {
        if self.blind {
            return FrameView::new(self.width, 0, self.format, self.stride, &[]);
        }
        let band = self.band();
        let first = region.y.clamp(band.y, band.bottom() as u16);
        let last = region.bottom().clamp(u32::from(first), band.bottom()) as u16;
        let start = usize::from(first - self.top) * self.stride;
        let end = usize::from(last - self.top) * self.stride;
        FrameView::new(
            self.width,
            last - first,
            self.format,
            self.stride,
            &self.data[start..end],
        )
    }

    /// A canvas covering the current band (the whole screen unless banded).
    pub fn canvas(&mut self) -> Canvas<'_> {
        let band = self.band();
        Canvas::new(self, band)
    }

    /// When inverted, every colour is drawn inverted (night mode). Changing
    /// it does not touch existing pixels; redraw afterwards.
    pub fn set_inverted(&mut self, inverted: bool) {
        self.inverted = inverted;
    }

    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    /// Whether pictures stay as they are while the rest is inverted
    /// ([`Framebuffer::set_inverted`]), e.g. photos in night mode, which
    /// would look like negatives.
    pub fn set_pictures_kept(&mut self, kept: bool) {
        self.pictures_kept = kept;
    }

    pub fn pictures_kept(&self) -> bool {
        self.pictures_kept
    }

    /// In black and white: from what coverage (1 to 255) a pixel at the
    /// edge of a letter or a shape takes the colour drawn; lower draws
    /// text fuller ([`MONO_COVERAGE`] at first, 128 is half).
    pub fn set_mono_coverage(&mut self, alpha: u8) {
        self.mono_coverage = alpha.max(1);
    }

    pub fn mono_coverage(&self) -> u8 {
        self.mono_coverage
    }

    /// Draws RGB pixels (pictures) with their saturation at `percent`,
    /// e.g. stronger for colour e-ink, whose filter mutes colours
    /// ([`ColorPanel::boost_percent`](paperpaw_platform::ColorPanel)).
    /// Grays stay gray.
    pub fn set_saturation(&mut self, percent: u16) {
        self.saturation = percent;
    }

    pub fn saturation(&self) -> u16 {
        self.saturation
    }

    /// Returns and resets the region changed since the last call.
    pub fn take_dirty(&mut self) -> Option<Rect> {
        self.dirty.take()
    }

    pub(crate) fn mark_dirty(&mut self, rect: Rect) {
        self.dirty = Some(match self.dirty {
            Some(dirty) => dirty.union(&rect),
            None => rect,
        });
    }

    /// Fills `rect`, which must lie inside the framebuffer.
    pub(crate) fn fill_clipped(&mut self, rect: Rect, color: Color) {
        if self.blind {
            return;
        }
        if rect.is_empty() {
            return;
        }
        let value = self.encode(color);
        let bpp = self.format.bits_per_pixel();
        // Formats of several pixels a byte: whole bytes at once, the
        // pixels of the bytes at either end one by one (every band is
        // cleared before it is drawn).
        let pattern = match bpp {
            1 => Some(if value & 1 != 0 { 0xFF } else { 0x00 }),
            2 => Some((value as u8 & 0b11) * 0x55),
            4 => Some((value as u8 & 0x0F) * 0x11),
            _ => None,
        };
        let (left, right) = (rect.x, rect.right() as u16);
        for y in rect.y..rect.bottom() as u16 {
            let row = usize::from(y - self.top) * self.stride;
            if self.format == PixelFormat::Gray8 {
                self.data[row + usize::from(left)..row + usize::from(right)].fill(value as u8);
                continue;
            }
            let Some(pattern) = pattern else {
                for x in left..right {
                    self.store(x, y, value);
                }
                continue;
            };
            let per = 8 / u16::from(bpp);
            let first = left.div_ceil(per) * per;
            let last = right / per * per;
            if first >= last {
                for x in left..right {
                    self.store(x, y, value);
                }
                continue;
            }
            for x in left..first {
                self.store(x, y, value);
            }
            self.data[row + usize::from(first / per)..row + usize::from(last / per)].fill(pattern);
            for x in last..right {
                self.store(x, y, value);
            }
        }
        self.mark_dirty(rect);
    }

    /// Washes `rect` (inside the framebuffer) out towards the paper, e.g.
    /// what lies below a menu: two thirds of the way; on panels of black
    /// and white, every other pixel paper (in a checker, the other one
    /// than [`Canvas::fill_tint`](crate::Canvas::fill_tint)'s, so tinted
    /// lines stay).
    pub(crate) fn wash_clipped(&mut self, rect: Rect) {
        if self.blind {
            return;
        }
        if rect.is_empty() {
            return;
        }
        if self.format.gray_levels() == Some(2) {
            let paper = self.encode(Color::WHITE) & 1 != 0;
            self.checker(rect, true, paper);
        } else {
            for y in rect.y..rect.bottom() as u16 {
                for x in rect.x..rect.right() as u16 {
                    self.blend_pixel(x, y, Color::WHITE, 170);
                }
            }
        }
        self.mark_dirty(rect);
    }

    /// Draws every other pixel of `rect` (inside the band) black in black
    /// and white, those of even x + y, as
    /// [`Canvas::fill_tint`](crate::Canvas::fill_tint) draws a tint there.
    /// Only the pixels drawn are marked dirty: a line one pixel wide may
    /// leave out its ends.
    pub(crate) fn tint_clipped(&mut self, rect: Rect) {
        if self.blind || rect.is_empty() {
            return;
        }
        let ink = self.encode(Color::BLACK) & 1 != 0;
        self.checker(rect, false, ink);
        let drawn = if rect.width > 1 && rect.height > 1 {
            rect
        } else {
            // A line: from its first pixel drawn to its last.
            let skip = ((u32::from(rect.x) + u32::from(rect.y)) % 2) as u16;
            let length = rect.width.max(rect.height);
            if skip >= length {
                return;
            }
            let span = (length - 1 - skip) / 2 * 2 + 1;
            if rect.height == 1 {
                Rect::new(rect.x + skip, rect.y, span, 1)
            } else {
                Rect::new(rect.x, rect.y + skip, 1, span)
            }
        };
        self.mark_dirty(drawn);
    }

    /// Sets every other pixel of `rect` (inside the band, black and white)
    /// to the bit `on`, in a checker: those of odd x + y where `odd`, of
    /// even x + y otherwise. Whole bytes at once, the pixels of the bytes
    /// at either end one by one.
    fn checker(&mut self, rect: Rect, odd: bool, on: bool) {
        for y in rect.y..rect.bottom() as u16 {
            let row = usize::from(y - self.top) * self.stride;
            // In a byte the leftmost pixel is its highest bit: 0x55 holds
            // those of odd x, 0xAA those of even x.
            let mask: u8 = if (y % 2 == 0) == odd { 0x55 } else { 0xAA };
            let (left, right) = (rect.x, rect.right() as u16);
            let (first, last) = (left.div_ceil(8) * 8, right / 8 * 8);
            let paint = |data: &mut [u8], at: usize, mask: u8| {
                if on {
                    data[at] |= mask;
                } else {
                    data[at] &= !mask;
                }
            };
            if first < last {
                for byte in usize::from(first / 8)..usize::from(last / 8) {
                    paint(&mut self.data, row + byte, mask);
                }
            }
            let ends = if first < last {
                [(left, first), (last, right)]
            } else {
                [(left, right), (right, right)]
            };
            for (from, to) in ends {
                for x in from..to {
                    if (u32::from(x) + u32::from(y)) % 2 == u32::from(odd) {
                        paint(&mut self.data, row + usize::from(x / 8), 0x80 >> (x % 8));
                    }
                }
            }
        }
    }

    /// Blends `color` with coverage `alpha` (0 to 255) over the pixel at
    /// `(x, y)`, which must lie inside the framebuffer. Does not mark the
    /// pixel dirty; callers mark whole areas.
    pub(crate) fn blend_pixel(&mut self, x: u16, y: u16, color: Color, alpha: u8) {
        if self.blind {
            return;
        }
        // In black and white, a pixel at an edge takes the colour from a
        // part covered (somewhat less than half at first): thin strokes
        // of small text keep their weight instead of breaking up.
        if self.format == PixelFormat::Mono1 {
            if alpha >= self.mono_coverage {
                let encoded = self.encode(color);
                self.store(x, y, encoded);
            }
            return;
        }
        // Gray formats mix in gray: every glyph's edge comes here, and on
        // small devices the round through red, green and blue is felt.
        if alpha != 255
            && let Some(levels) = self.format.gray_levels()
            && self.format.bits_per_pixel() <= 8
        {
            let max = u32::from(levels - 1);
            let mut under = self.load(x, y) * 255 / max;
            if self.inverted {
                under = 255 - under;
            }
            let (a, over) = (u32::from(alpha), u32::from(color.luma()));
            let mixed = (over * a + under * (255 - a) + 127) / 255;
            let encoded = self.encode(Color::gray(mixed as u8));
            return self.store(x, y, encoded);
        }
        let value = if alpha == 255 {
            color
        } else {
            let stored = self.view().pixel(x, y - self.top);
            let below = if self.inverted {
                stored.inverted()
            } else {
                stored
            };
            let mix = |under: u8, over: u8| {
                let (a, u, o) = (u32::from(alpha), u32::from(under), u32::from(over));
                ((o * a + u * (255 - a) + 127) / 255) as u8
            };
            Color::rgb(
                mix(below.r, color.r),
                mix(below.g, color.g),
                mix(below.b, color.b),
            )
        };
        let encoded = self.encode(value);
        self.store(x, y, encoded);
    }

    /// Pixels of pictures (photos, illustrations) drawn since the last
    /// call: panels that keep ghosts of them want a full refresh after.
    pub fn take_pictured(&mut self) -> u32 {
        core::mem::take(&mut self.pictured)
    }

    /// Pixels of all pictures drawn since the last call, small ones too
    /// (covers in a list), also where they are only described
    /// ([`Framebuffer::blind`]): whether a frame is best shown in grays.
    pub fn take_all_pictured(&mut self) -> u32 {
        core::mem::take(&mut self.all_pictured)
    }

    /// `pixels` of a picture drawn: they count as
    /// [`Framebuffer::take_pictured`] and
    /// [`Framebuffer::take_all_pictured`] say.
    pub(crate) fn count_picture(&mut self, pixels: u32) {
        self.pictured = self
            .pictured
            .saturating_add(pixels * u32::from(self.counting));
        self.all_pictured = self.all_pictured.saturating_add(pixels);
    }

    /// Whether pictures drawn from now count as pictures
    /// ([`Framebuffer::take_pictured`]); the earlier setting.
    pub(crate) fn set_counting(&mut self, counting: bool) -> bool {
        core::mem::replace(&mut self.counting, counting)
    }

    /// Sets the pixel at `(x, y)` (inside the band) to `gray`, ordered
    /// dithered for formats with few gray levels, so photos and
    /// illustrations keep their tones.
    pub(crate) fn put_gray(&mut self, x: u16, y: u16, gray: u8) {
        if self.blind {
            return;
        }
        self.count_picture(1);
        let gray = self.dithered(x, y, gray);
        // Inverted once more to stay as it is, where pictures are kept.
        let gray = if self.inverted && self.pictures_kept {
            255 - gray
        } else {
            gray
        };
        let value = self.encode(Color::gray(gray));
        self.store(x, y, value);
    }

    /// Puts the pixel of a picture with transparency at `(x, y)` (inside
    /// the band): gray `gray` as opaque as `alpha` says (0 to 255) over
    /// what is drawn there, the mix dithered where the format has few
    /// grays, as pictures are.
    pub(crate) fn put_gray_over(&mut self, x: u16, y: u16, gray: u8, alpha: u8) {
        if alpha == 0 || self.blind {
            return;
        }
        if alpha == 255 {
            return self.put_gray(x, y, gray);
        }
        self.count_picture(1);
        // What shows there now, as the page means it (not inverted).
        let under = match self.format.gray_levels() {
            Some(levels) if self.format.bits_per_pixel() <= 8 => {
                let level = self.load(x, y) * 255 / u32::from(levels - 1);
                if self.inverted { 255 - level } else { level }
            }
            _ => {
                let stored = self.view().pixel(x, y - self.top);
                let below = if self.inverted {
                    stored.inverted()
                } else {
                    stored
                };
                u32::from(below.luma())
            }
        };
        // The picture's gray as `put_gray` would show it: kept as it is
        // where pictures keep their tones in night mode.
        let over = u32::from(if self.inverted && self.pictures_kept {
            255 - gray
        } else {
            gray
        });
        let a = u32::from(alpha);
        let mixed = ((over * a + under * (255 - a) + 127) / 255) as u8;
        let mixed = self.dithered(x, y, mixed);
        let value = self.encode(Color::gray(mixed));
        self.store(x, y, value);
    }

    /// A row of grays from `(x, y)` on, as [`Framebuffer::put_gray`] puts
    /// each, the dither's offsets worked out once for the row rather than
    /// for every pixel (a division each, slow on chips without one: a
    /// picture over the whole screen is some 400,000 pixels).
    pub(crate) fn put_gray_row(&mut self, x: u16, y: u16, grays: &[u8]) {
        if self.blind {
            return;
        }
        self.count_picture(u32::try_from(grays.len()).unwrap_or(u32::MAX));
        let offsets: [i32; 4] = core::array::from_fn(|column| self.offset(column as u16, y));
        let keep = self.inverted && self.pictures_kept;
        for (px, &gray) in (x..).zip(grays) {
            let gray = (i32::from(gray) + offsets[usize::from(px % 4)]).clamp(0, 255) as u8;
            let gray = if keep { 255 - gray } else { gray };
            let value = self.encode(Color::gray(gray));
            self.store(px, y, value);
        }
    }

    /// `gray` at `(x, y)` with the offset of an ordered dither where the
    /// format has few gray levels (as is elsewhere), so areas of a gray
    /// between them show as a pattern of the levels around it.
    fn dithered(&self, x: u16, y: u16, gray: u8) -> u8 {
        (i32::from(gray) + self.offset(x, y)).clamp(0, 255) as u8
    }

    /// The ordered dither's offset at `(x, y)`: none where the format has
    /// many grays (or colours).
    pub(crate) fn offset(&self, x: u16, y: u16) -> i32 {
        /// 4×4 Bayer matrix.
        const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
        match self.format.gray_levels() {
            // The threshold's offset, ((t + 0.5) / 16 - 0.5) of a level's
            // step (255 / (levels - 1)), in whole numbers: chips without
            // floating point reckon it for every pixel of a picture.
            Some(levels @ 2..=16) => {
                let threshold = i32::from(BAYER[usize::from(y % 4)][usize::from(x % 4)]);
                (2 * threshold - 15) * 255 / (32 * i32::from(levels - 1))
            }
            _ => 0,
        }
    }

    /// Fills `rect` (inside the band) with `color`, ordered dithered where
    /// the format has few grays (a light gray stays visible as a light
    /// pattern rather than turning white); colour formats get the colour.
    /// It is no picture (see [`Framebuffer::take_pictured`]).
    pub(crate) fn shade_clipped(&mut self, rect: Rect, color: Color) {
        if self.blind {
            return;
        }
        if !matches!(self.format.gray_levels(), Some(2..=16)) {
            return self.fill_clipped(rect, color);
        }
        if rect.is_empty() {
            return;
        }
        let luma = color.luma();
        for y in rect.y..rect.bottom() as u16 {
            // The dither repeats every four pixels: its values for the row
            // worked out once (a division each).
            let values: [u32; 4] = core::array::from_fn(|column| {
                self.encode(Color::gray(self.dithered(column as u16, y, luma)))
            });
            for x in rect.x..rect.right() as u16 {
                self.store(x, y, values[usize::from(x % 4)]);
            }
        }
        self.mark_dirty(rect);
    }

    /// Sets the pixel at `(x, y)` (inside the band) to `rgb` on colour
    /// formats; on gray ones to its gray, as [`Framebuffer::put_gray`].
    pub(crate) fn put_rgb(&mut self, x: u16, y: u16, rgb: [u8; 3]) {
        if self.blind {
            return;
        }
        if self.format.is_color() {
            self.count_picture(1);
            let rgb = saturate(rgb, self.saturation);
            let color = Color::rgb(rgb[0], rgb[1], rgb[2]);
            // Inverted once more to stay as it is, where pictures are kept.
            let color = if self.inverted && self.pictures_kept {
                color.inverted()
            } else {
                color
            };
            let value = self.encode(color);
            self.store(x, y, value);
        } else {
            self.put_gray(x, y, Color::rgb(rgb[0], rgb[1], rgb[2]).luma());
        }
    }

    /// The rows held, the bytes of a row and the screen row of the
    /// first, for drawing that writes whole bytes (see
    /// [`Canvas::blit`](crate::Canvas::blit)).
    pub(crate) fn rows_mut(&mut self) -> (&mut [u8], usize, u16) {
        (&mut self.data, self.stride, self.top)
    }

    /// The stored value of a pixel of a format of 8 bits or fewer.
    fn load(&self, x: u16, y: u16) -> u32 {
        let row = usize::from(y - self.top) * self.stride;
        let bpp = usize::from(self.format.bits_per_pixel());
        let bit = usize::from(x) * bpp;
        let byte = self.data[row + bit / 8];
        u32::from(byte >> (8 - bpp - bit % 8)) & ((1 << bpp) - 1)
    }

    /// Writes an encoded pixel value.
    fn store(&mut self, x: u16, y: u16, value: u32) {
        let row = usize::from(y - self.top) * self.stride;
        let x = usize::from(x);
        match self.format.bits_per_pixel() as usize {
            bpp @ (1 | 2 | 4) => {
                let bit = x * bpp;
                let shift = 8 - bpp - bit % 8;
                let mask = (((1u16 << bpp) - 1) as u8) << shift;
                let byte = &mut self.data[row + bit / 8];
                *byte = (*byte & !mask) | (((value as u8) << shift) & mask);
            }
            8 => self.data[row + x] = value as u8,
            16 => {
                let bytes = (value as u16).to_le_bytes();
                self.data[row + x * 2..row + x * 2 + 2].copy_from_slice(&bytes);
            }
            _ => {
                let bytes = [(value >> 16) as u8, (value >> 8) as u8, value as u8];
                self.data[row + x * 3..row + x * 3 + 3].copy_from_slice(&bytes);
            }
        }
    }

    /// Converts a colour to the stored pixel value (nearest level).
    pub(crate) fn encode(&self, color: Color) -> u32 {
        let color = if self.inverted {
            color.inverted()
        } else {
            color
        };
        let quantize = |levels: u32| (color.luma() as u32 * (levels - 1) + 127) / 255;
        match self.format {
            PixelFormat::Mono1 => quantize(2),
            PixelFormat::Gray2 => quantize(4),
            PixelFormat::Gray4 => quantize(16),
            PixelFormat::Gray8 => color.luma() as u32,
            PixelFormat::Rgb565 => {
                let (r, g, b) = (color.r as u32, color.g as u32, color.b as u32);
                (((r * 31 + 127) / 255) << 11)
                    | (((g * 63 + 127) / 255) << 5)
                    | ((b * 31 + 127) / 255)
            }
            PixelFormat::Rgb888 => {
                ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32
            }
            // Future formats: rejected by `new` until supported.
            _ => 0,
        }
    }
}

/// `rgb` with its distance from its gray scaled to `percent`.
fn saturate(rgb: [u8; 3], percent: u16) -> [u8; 3] {
    if percent == 100 {
        return rgb;
    }
    let luma = i32::from(Color::rgb(rgb[0], rgb[1], rgb[2]).luma());
    rgb.map(|c| (luma + (i32::from(c) - luma) * i32::from(percent) / 100).clamp(0, 255) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_whole_bytes_and_the_pixels_at_either_end() {
        for format in [PixelFormat::Mono1, PixelFormat::Gray2, PixelFormat::Gray4] {
            for (x, width) in [(0, 37), (3, 1), (5, 2), (7, 25), (9, 30), (16, 16)] {
                let mut fb = Framebuffer::new(40, 3, format).unwrap();
                fb.fill_clipped(Rect::new(x, 1, width, 1), Color::BLACK);
                let view = fb.view();
                for py in 0..3 {
                    for px in 0..40 {
                        let inside = py == 1 && px >= x && px < x + width;
                        let expected = if inside { Color::BLACK } else { Color::WHITE };
                        assert_eq!(
                            view.pixel(px, py),
                            expected,
                            "{format:?} {x}+{width} at ({px}, {py})"
                        );
                    }
                }
            }
        }
    }

    /// Tints and shades drawn by whole bytes or rows come out as drawn
    /// pixel by pixel, the same pixels marked dirty: at every place in a
    /// byte, lines one pixel wide or high too, inverted or not.
    #[test]
    fn tints_and_shades_are_drawn_as_each_pixel() {
        let places = [(0, 1), (1, 1), (3, 2), (7, 1), (5, 12), (9, 30), (16, 16)];
        let mut cases = Vec::new();
        for &(x, width) in &places {
            for &(y, height) in &[(0, 1), (1, 1), (2, 3), (1, 6)] {
                cases.push(Rect::new(x, y, width, height));
            }
        }
        for format in [PixelFormat::Mono1, PixelFormat::Gray2, PixelFormat::Gray4] {
            for inverted in [false, true] {
                for &rect in &cases {
                    let mut fast = Framebuffer::new(48, 8, format).unwrap();
                    fast.set_inverted(inverted);
                    for (i, byte) in fast.data.iter_mut().enumerate() {
                        *byte = (i as u8).wrapping_mul(73) ^ 0x5a;
                    }
                    let mut each = Framebuffer::new(48, 8, format).unwrap();
                    each.set_inverted(inverted);
                    each.data.clone_from(&fast.data);
                    if format == PixelFormat::Mono1 {
                        fast.tint_clipped(rect);
                        for y in rect.y..rect.bottom() as u16 {
                            for x in rect.x..rect.right() as u16 {
                                if (x + y) % 2 == 0 {
                                    each.fill_clipped(Rect::new(x, y, 1, 1), Color::BLACK);
                                }
                            }
                        }
                        assert_eq!(fast.data, each.data, "tint {inverted} {rect:?}");
                        assert_eq!(fast.take_dirty(), each.take_dirty(), "tint {rect:?}");
                    }
                    let gray = (rect.x * 13 + rect.y * 41 + rect.width * 7) as u8;
                    let color = Color::gray(gray);
                    fast.shade_clipped(rect, color);
                    for y in rect.y..rect.bottom() as u16 {
                        for x in rect.x..rect.right() as u16 {
                            let value = each.encode(Color::gray(each.dithered(x, y, color.luma())));
                            each.store(x, y, value);
                        }
                    }
                    each.mark_dirty(rect);
                    assert_eq!(fast.data, each.data, "shade {format:?} {inverted} {rect:?}");
                    assert_eq!(fast.take_dirty(), each.take_dirty(), "shade {rect:?}");
                }
            }
        }
    }

    #[test]
    fn saturation_strengthens_colours_not_grays() {
        assert_eq!(saturate([200, 100, 100], 100), [200, 100, 100]);
        let [r, g, b] = saturate([200, 100, 100], 200);
        assert!(r > 200 && g < 100 && g == b);
        assert_eq!(saturate([90, 90, 90], 200), [90, 90, 90]);
        assert_eq!(saturate([255, 0, 0], 200), [255, 0, 0]);
    }

    fn formats() -> &'static [PixelFormat] {
        if cfg!(feature = "color") {
            &[
                PixelFormat::Mono1,
                PixelFormat::Gray2,
                PixelFormat::Gray4,
                PixelFormat::Gray8,
                PixelFormat::Rgb565,
                PixelFormat::Rgb888,
            ]
        } else {
            &[
                PixelFormat::Mono1,
                PixelFormat::Gray2,
                PixelFormat::Gray4,
                PixelFormat::Gray8,
            ]
        }
    }

    #[test]
    fn bands_change_their_rows() {
        let mut band = Framebuffer::banded(8, 10, 4, PixelFormat::Gray2).unwrap();
        let four = band.byte_len();
        band.set_rows(2).unwrap();
        assert_eq!((band.rows(), band.byte_len()), (2, four / 2));
        band.set_rows(4).unwrap();
        assert_eq!((band.rows(), band.byte_len()), (4, four));
        // All of the screen held: as it is.
        let mut full = Framebuffer::new(8, 10, PixelFormat::Gray2).unwrap();
        full.set_rows(2).unwrap();
        assert_eq!(full.rows(), 10);
    }

    #[test]
    fn bands_hold_their_rows_only() {
        let mut full = Framebuffer::new(8, 10, PixelFormat::Gray2).unwrap();
        let mut band = Framebuffer::banded(8, 10, 4, PixelFormat::Gray2).unwrap();
        assert_eq!(band.byte_len(), full.byte_len() * 4 / 10);
        for top in [0, 4, 8] {
            band.set_top(top);
            let mut canvas = band.canvas();
            canvas.clear(Color::WHITE);
            // A diagonal line across the whole screen.
            for i in 0..10 {
                canvas.set_pixel(paperpaw_platform::Point::new(i % 8, i), Color::BLACK);
            }
            assert_eq!(band.band().height, if top == 8 { 2 } else { 4 });
            full.write_rows(top, band.view());
        }
        for i in 0..10u16 {
            assert_eq!(full.view().pixel(i % 8, i), Color::BLACK, "row {i}");
            assert_eq!(full.view().pixel((i + 1) % 8, i), Color::WHITE, "row {i}");
        }
        assert_eq!(full.view_rows(Rect::new(0, 3, 8, 2)).height, 2);
        // A region reaching past the largest row number still ends with
        // the band.
        assert_eq!(full.view_rows(Rect::new(0, 5, 8, u16::MAX)).height, 5);
    }

    #[test]
    fn starts_white_and_round_trips_black_and_white() {
        for &format in formats() {
            let mut fb = Framebuffer::new(13, 7, format).unwrap();
            assert_eq!(fb.view().pixel(12, 6), Color::WHITE, "{format:?}");
            fb.fill_clipped(Rect::new(3, 2, 5, 3), Color::BLACK);
            let view = fb.view();
            assert_eq!(view.pixel(3, 2), Color::BLACK, "{format:?}");
            assert_eq!(view.pixel(7, 4), Color::BLACK, "{format:?}");
            assert_eq!(view.pixel(8, 4), Color::WHITE, "{format:?}");
            assert_eq!(view.pixel(2, 2), Color::WHITE, "{format:?}");
        }
    }

    #[test]
    fn quantizes_to_nearest_gray_level() {
        let mut fb = Framebuffer::new(4, 1, PixelFormat::Gray2).unwrap();
        fb.fill_clipped(Rect::new(0, 0, 1, 1), Color::gray(80));
        fb.fill_clipped(Rect::new(1, 0, 1, 1), Color::gray(180));
        assert_eq!(fb.view().pixel(0, 0), Color::gray(85));
        assert_eq!(fb.view().pixel(1, 0), Color::gray(170));
    }

    #[test]
    fn blends_over_existing_pixels() {
        let mut fb = Framebuffer::new(2, 1, PixelFormat::Gray8).unwrap();
        fb.blend_pixel(0, 0, Color::BLACK, 128);
        assert_eq!(fb.view().pixel(0, 0), Color::gray(127));
        fb.set_inverted(true);
        fb.fill_clipped(Rect::new(1, 0, 1, 1), Color::WHITE);
        fb.blend_pixel(1, 0, Color::BLACK, 255);
        assert_eq!(fb.view().pixel(1, 0), Color::WHITE);
    }

    #[test]
    fn a_row_of_grays_is_put_as_each_of_its_pixels() {
        let grays: Vec<u8> = (0..=255u8).step_by(3).collect();
        let width = grays.len() as u16 + 5;
        for format in [
            PixelFormat::Mono1,
            PixelFormat::Gray2,
            PixelFormat::Gray4,
            PixelFormat::Gray8,
        ] {
            for (inverted, kept) in [(false, false), (true, false), (true, true)] {
                let mut each = Framebuffer::new(width, 5, format).unwrap();
                let mut row = Framebuffer::new(width, 5, format).unwrap();
                for fb in [&mut each, &mut row] {
                    fb.set_inverted(inverted);
                    fb.set_pictures_kept(kept);
                }
                for y in 0..5 {
                    for (i, &gray) in grays.iter().enumerate() {
                        each.put_gray(3 + i as u16, y, gray);
                    }
                    row.put_gray_row(3, y, &grays);
                }
                assert_eq!(each.data, row.data, "{format:?} {inverted} {kept}");
                assert_eq!(each.take_pictured(), row.take_pictured());
            }
        }
    }

    #[test]
    fn pictures_kept_as_they_are_when_inverted() {
        let mut fb = Framebuffer::new(2, 1, PixelFormat::Gray8).unwrap();
        fb.set_inverted(true);
        fb.set_pictures_kept(true);
        // A picture's dark pixel stays dark; drawn text turns.
        fb.put_gray(0, 0, 20);
        fb.canvas().fill_rect(1, 0, 1, 1, Color::BLACK);
        assert_eq!(fb.view().pixel(0, 0), Color::gray(20));
        assert_eq!(fb.view().pixel(1, 0), Color::WHITE);
        fb.set_pictures_kept(false);
        fb.put_gray(0, 0, 20);
        assert_eq!(fb.view().pixel(0, 0), Color::gray(235));
    }

    #[test]
    fn inversion_applies_to_new_drawing() {
        let mut fb = Framebuffer::new(2, 1, PixelFormat::Mono1).unwrap();
        fb.set_inverted(true);
        fb.fill_clipped(Rect::new(0, 0, 1, 1), Color::WHITE);
        assert_eq!(fb.view().pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn tracks_dirty_region() {
        let mut fb = Framebuffer::new(20, 20, PixelFormat::Gray4).unwrap();
        assert_eq!(fb.take_dirty(), None);
        fb.fill_clipped(Rect::new(1, 1, 2, 2), Color::BLACK);
        fb.fill_clipped(Rect::new(10, 5, 2, 2), Color::BLACK);
        assert_eq!(fb.take_dirty(), Some(Rect::new(1, 1, 11, 6)));
        assert_eq!(fb.take_dirty(), None);
    }

    #[cfg(not(feature = "color"))]
    #[test]
    fn rejects_color_without_feature() {
        assert_eq!(
            Framebuffer::new(1, 1, PixelFormat::Rgb888).unwrap_err(),
            GraphicsError::UnsupportedFormat(PixelFormat::Rgb888)
        );
    }
}
