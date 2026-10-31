use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::{Color, Point, Rect};

use crate::blit::{Bitmap, Blit, Dither};
use crate::framebuffer::Framebuffer;
use crate::round::{self, corner_inset};

/// A drawing surface clipped to a region of a [`Framebuffer`].
///
/// Coordinates are screen coordinates, or relative to an origin set with
/// [`Canvas::offset`] (a screen drawn into part of the screen, below a
/// bar); everything outside the clip region is silently discarded.
#[derive(Debug)]
pub struct Canvas<'a> {
    framebuffer: &'a mut Framebuffer,
    /// In screen coordinates.
    clip: Rect,
    /// Where `(0, 0)` lies on the screen.
    origin: (i32, i32),
    /// Where what is drawn is described, if anyone asks (see
    /// [`Canvas::describing`]).
    described: Option<&'a mut Vec<Described>>,
    /// What is described is told as doing nothing now (see
    /// [`Canvas::disabling`]).
    disabled: bool,
}

/// What a part of a screen is, as its drawing tells: for reading the
/// screen aloud, automation and tests. Drawing helpers describe what
/// they draw; most of the time nobody asks and it costs nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Described {
    pub role: Role,
    /// Its text, if it has one of its own.
    pub label: String,
    pub area: Rect,
    /// It has the focus (buttons move it).
    pub focused: bool,
    /// Switched on or off, for switches and buttons that are either.
    pub on: Option<bool>,
    /// It does nothing now (drawn faint), e.g. a menu entry that cannot
    /// be chosen yet.
    pub disabled: bool,
    /// What the canvas showed when it was described (its clip): the
    /// screen, cell or row it was drawn into. An area reaching past it
    /// was cut (see `paperpaw-dev`'s checks).
    pub clip: Rect,
}

impl Described {
    /// A part in `role` with `label` at `area`, drawn into a canvas
    /// showing `area` alone: without the focus, neither on nor off.
    pub fn new(role: Role, label: impl Into<String>, area: Rect) -> Self {
        Self {
            role,
            label: label.into(),
            area,
            focused: false,
            on: None,
            disabled: false,
            clip: area,
        }
    }

    /// Doing nothing now, or not.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// With the focus or not.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Switched on or off, or neither.
    pub fn on(mut self, on: Option<bool>) -> Self {
        self.on = on;
        self
    }

    /// Drawn into a canvas that showed `clip`.
    pub fn clip(mut self, clip: Rect) -> Self {
        self.clip = clip;
        self
    }
}

/// A part being described ([`Canvas::describe`]): told to whoever asks
/// when it goes, at the end of the statement that made it.
pub struct Describe<'c> {
    sink: Option<&'c mut Vec<Described>>,
    role: Role,
    label: &'c str,
    area: Rect,
    clip: Rect,
    focused: bool,
    on: Option<bool>,
    disabled: bool,
}

impl Describe<'_> {
    /// It has the focus (buttons move it), or not.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Switched on or off, for switches and buttons that are either.
    pub fn on(mut self, on: Option<bool>) -> Self {
        self.on = on;
        self
    }

    /// It does nothing now (drawn faint), or not.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Drop for Describe<'_> {
    fn drop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.push(Described {
                role: self.role,
                label: String::from(self.label),
                area: self.area,
                focused: self.focused,
                on: self.on,
                disabled: self.disabled,
                clip: self.clip,
            });
        }
    }
}

impl core::fmt::Debug for Describe<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Describe")
            .field("role", &self.role)
            .field("label", &self.label)
            .field("area", &self.area)
            .finish_non_exhaustive()
    }
}

/// What a described part is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Role {
    /// A title over what follows (a header, a section's heading).
    Heading,
    /// A row of a list; its text is that drawn inside it.
    Row,
    Button,
    /// A switch in a row.
    Switch,
    /// A number set by moving a knob along a track; its text is the value.
    Slider,
    /// Text.
    Text,
    /// A box ticked or not, of several that may each be; `on` says which.
    Check,
    /// One of several of which one is chosen; `on` says whether it is.
    Radio,
    /// Where text is typed; its text is that typed so far.
    Field,
    /// A picture; its text says what it shows, where known.
    Image,
    /// Leads elsewhere when chosen, e.g. a link in a book.
    Link,
    /// A question over the screen, e.g. a choice of a few options; its
    /// text is the question.
    Dialog,
    /// How far something has come; its text the share done, e.g. `42%`.
    Progress,
}

/// Rings are drawn at most this many pixels across their radius, far
/// past any screen, so that their arithmetic stays in range.
const MAX_RADIUS: u32 = 1 << 24;

impl<'a> Canvas<'a> {
    pub(crate) fn new(framebuffer: &'a mut Framebuffer, clip: Rect) -> Self {
        let clip = clip.intersection(&framebuffer.band()).unwrap_or_default();
        Self {
            framebuffer,
            clip,
            origin: (0, 0),
            described: None,
            disabled: false,
        }
    }

    /// This canvas with `(0, 0)` moved by `(dx, dy)`: what is drawn at
    /// `(x, y)` lands at `(x + dx, y + dy)` of the canvas before, and
    /// [`clip`](Self::clip), [`clipped`](Self::clipped) and what is
    /// described are in the moved coordinates too. For a screen drawn into
    /// part of the screen (below a bar, a tab of another screen).
    pub fn offset(mut self, dx: i32, dy: i32) -> Self {
        self.origin = (self.origin.0 + dx, self.origin.1 + dy);
        self
    }

    /// Where `(0, 0)` of this canvas lies on the screen.
    pub fn origin(&self) -> Point {
        Point::new(self.origin.0, self.origin.1)
    }

    /// `area` moved from this canvas's coordinates to the screen's, cut
    /// to what a `Rect` holds: a part moved past the screen's top or left
    /// edge is cut off, not moved back onto it.
    fn to_screen(&self, area: Rect) -> Rect {
        let fit = |v: i64| v.clamp(0, i64::from(u16::MAX)) as u16;
        let (ox, oy) = (i64::from(self.origin.0), i64::from(self.origin.1));
        let (left, top) = (i64::from(area.x) + ox, i64::from(area.y) + oy);
        let (right, bottom) = (left + i64::from(area.width), top + i64::from(area.height));
        let (x, y) = (fit(left), fit(top));
        Rect::new(x, y, fit(right) - x, fit(bottom) - y)
    }

    /// The clip region in this canvas's coordinates, wide.
    fn local_clip(&self) -> (i64, i64, i64, i64) {
        let (ox, oy) = (i64::from(self.origin.0), i64::from(self.origin.1));
        let clip = self.clip;
        (
            i64::from(clip.x) - ox,
            i64::from(clip.y) - oy,
            i64::from(clip.right()) - ox,
            i64::from(clip.bottom()) - oy,
        )
    }

    /// This canvas, describing what is drawn into `sink`.
    #[doc(hidden)]
    pub fn describing(mut self, sink: &'a mut Vec<Described>) -> Self {
        self.described = Some(sink);
        self
    }

    /// Whether it keeps no pixels ([`Framebuffer::blind`]): what is drawn
    /// is only described, so drawing that costs (glyphs, pictures) can be
    /// left out.
    #[doc(hidden)]
    pub fn is_blind(&self) -> bool {
        self.framebuffer.is_blind()
    }

    /// Tells that a picture lies at `area` where nothing is drawn
    /// ([`Canvas::is_blind`]), for
    /// [`Framebuffer::take_all_pictured`](crate::Framebuffer::take_all_pictured);
    /// its pixels count where they are drawn.
    #[doc(hidden)]
    pub fn note_picture(&mut self, area: Rect) {
        if !self.framebuffer.is_blind() {
            return;
        }
        let (x, y) = (i32::from(area.x), i32::from(area.y));
        let (w, h) = (u32::from(area.width), u32::from(area.height));
        if let Some(shown) = self.clip_signed(x, y, w, h) {
            let pixels = u32::from(shown.width) * u32::from(shown.height);
            self.framebuffer.count_picture(pixels);
        }
    }

    /// Whether anyone asks what is drawn (see [`Canvas::describe`]).
    pub fn is_describing(&self) -> bool {
        self.described.is_some()
    }

    /// Tells what is drawn at `area`, for those who ask: a part in
    /// `role` with `label`, said as it is once the statement ends;
    /// `focused`, `on` and `disabled`
    /// say more first. Nothing is kept (nor its text copied) where nobody
    /// asks.
    pub fn describe<'c>(&'c mut self, role: Role, label: &'c str, area: Rect) -> Describe<'c> {
        let clip = self.clip;
        let area = self.to_screen(area);
        Describe {
            sink: self.described.as_deref_mut(),
            role,
            label,
            area,
            clip,
            focused: false,
            on: None,
            disabled: self.disabled,
        }
    }

    /// The grays a picture is shown in where the frame has few (2 to 16:
    /// black and white, four grays, sixteen), for those who draw pictures
    /// to dither them themselves ([`diffuse`](crate::diffuse)), from the
    /// picture's first row so that every band and every part drawn again
    /// comes out the same; none where the frame has many grays or colour.
    pub fn picture_levels(&self) -> Option<u8> {
        match self.framebuffer.format().gray_levels() {
            Some(levels @ 2..=16) => u8::try_from(levels).ok(),
            _ => None,
        }
    }

    /// From what coverage (1 to 255) a pixel at the edge of a letter is
    /// drawn where the frame is black and white
    /// ([`Framebuffer::set_mono_coverage`](crate::Framebuffer::set_mono_coverage)):
    /// for those who draw text without grays elsewhere too, so that it
    /// looks the same on every panel and in every pass.
    pub fn mono_coverage(&self) -> u8 {
        self.framebuffer.mono_coverage()
    }

    /// The error of a picture diffused from its first row ([`Atkinson`]),
    /// as the band above left it at row `y`
    /// ([`Canvas::keep_diffusion`]): the picture goes on from there
    /// without reading its rows above again. `key` says which picture,
    /// where and how large (the same everywhere it is drawn so). Taken:
    /// a band drawn again starts from the first row.
    ///
    /// [`Atkinson`]: crate::diffuse::Atkinson
    #[doc(hidden)]
    pub fn take_diffusion(&mut self, key: u64, y: i32) -> Option<crate::diffuse::Atkinson> {
        let row = u16::try_from(y + self.origin.1).ok()?;
        self.framebuffer.take_diffusion(key, row)
    }

    /// Keeps the error of picture `key` at row `y` for the next band
    /// ([`Canvas::take_diffusion`]), where this band ends there. Freed
    /// when the next band begins elsewhere or the frame ends
    /// ([`Framebuffer::end_frame`](crate::Framebuffer::end_frame)).
    #[doc(hidden)]
    pub fn keep_diffusion(&mut self, key: u64, y: i32, atkinson: crate::diffuse::Atkinson) {
        if let Ok(row) = u16::try_from(y + self.origin.1) {
            self.framebuffer.keep_diffusion(key, row, atkinson);
        }
    }

    /// The visible region of this canvas, in its coordinates.
    pub fn clip(&self) -> Rect {
        let (left, top, right, bottom) = self.local_clip();
        let fit = |v: i64| v.clamp(0, i64::from(u16::MAX)) as u16;
        let (x, y) = (fit(left), fit(top));
        let (width, height) = (
            fit(right) - x.min(fit(right)),
            fit(bottom) - y.min(fit(bottom)),
        );
        Rect::new(x, y, width, height)
    }

    /// A canvas restricted to `region` (intersected with this canvas).
    pub fn clipped(&mut self, region: Rect) -> Canvas<'_> {
        let region = self.to_screen(region);
        let clip = self.clip.intersection(&region).unwrap_or_default();
        Canvas {
            framebuffer: self.framebuffer,
            clip,
            origin: self.origin,
            described: self.described.as_deref_mut(),
            disabled: self.disabled,
        }
    }

    /// This canvas, telling every part drawn into it as doing nothing now
    /// where `disabled` (e.g. a menu entry that cannot be chosen yet,
    /// whatever helper draws it); as it is otherwise.
    pub fn disabling(&mut self, disabled: bool) -> Canvas<'_> {
        Canvas {
            framebuffer: self.framebuffer,
            clip: self.clip,
            origin: self.origin,
            described: self.described.as_deref_mut(),
            disabled: self.disabled || disabled,
        }
    }

    /// Fills the whole clip region.
    pub fn clear(&mut self, color: Color) {
        self.framebuffer.fill_clipped(self.clip, color);
    }

    /// Fills `area`. The `…_rect` forms take signed coordinates, for
    /// shapes that reach past the screen's edges.
    pub fn fill(&mut self, area: Rect, color: Color) {
        let (x, y, width, height) = signed(area);
        self.fill_rect(x, y, width, height, color);
    }

    /// The colour drawn at `point` (in this canvas's coordinates), to
    /// composite what this screen drew in this band; `None` outside the
    /// clip region and the band, and on a canvas that keeps no pixels
    /// ([`Canvas::is_blind`]) or only describes. In night mode it is the
    /// colour before the swap of black and white.
    ///
    /// **Read-back is for compositing pixels you drew in this band, never
    /// for decisions:** a screen is drawn again for each band and a point in
    /// another band is `None`, so a drawing that reads back to decide what
    /// to draw shows seams at the bands' edges.
    pub fn pixel(&self, point: Point) -> Option<Color> {
        if self.described.is_some() {
            return None;
        }
        let x = i64::from(point.x) + i64::from(self.origin.0);
        let y = i64::from(point.y) + i64::from(self.origin.1);
        let inside = x >= i64::from(self.clip.x)
            && y >= i64::from(self.clip.y)
            && x < i64::from(self.clip.right())
            && y < i64::from(self.clip.bottom());
        if !inside {
            return None;
        }
        self.framebuffer
            .read_pixel(u16::try_from(x).ok()?, u16::try_from(y).ok()?)
    }

    /// The gray, 0 (black) to 255 (white), drawn at `point`: see
    /// [`Canvas::pixel`], whose rules it has.
    pub fn gray_at(&self, point: Point) -> Option<u8> {
        self.pixel(point).map(Color::luma)
    }

    /// Fills `area` with the gray `gray`, as a gray between the frame's
    /// levels: [`Dither::ORDERED`] as pictures are (a pattern of the levels
    /// around it; a frame with many grays gets the gray), [`Dither::NONE`]
    /// the nearest level. Every other value, the reserved numbers and
    /// those of later releases, is drawn as [`Dither::ORDERED`] for now
    /// (the kind is not honoured yet).
    pub fn fill_dithered(&mut self, area: Rect, gray: u8, dither: Dither) {
        if dither == Dither::NONE {
            self.fill(area, Color::gray(gray));
        } else {
            self.fill_shaded(area, Color::gray(gray));
        }
    }

    /// Fills `area` with `color` as a tint, e.g. a light line between
    /// rows or a slider's track: on panels of black and white alone, which
    /// would show a light gray white, every other pixel black instead (it
    /// looks gray, as dithering does).
    pub fn fill_tint(&mut self, area: Rect, color: Color) {
        if self.framebuffer.format().gray_levels() != Some(2) || color.luma() < 128 {
            return self.fill(area, color);
        }
        let (x, y, width, height) = signed(area);
        if let Some(rect) = self.clip_signed(x, y, width, height) {
            self.framebuffer.tint_clipped(rect);
        }
    }

    /// Fills `area` with `color` as a shade, e.g. behind marked text: where
    /// the panel has few grays (black and white alone, four), ordered
    /// dithered by its lightness, so a light gray stays a light pattern
    /// the text reads well on; in colour where the panel has colour.
    pub fn fill_shaded(&mut self, area: Rect, color: Color) {
        let (x, y, width, height) = signed(area);
        if let Some(rect) = self.clip_signed(x, y, width, height) {
            self.framebuffer.shade_clipped(rect, color);
        }
    }

    /// [`fill_shaded`](Self::fill_shaded) with the corners rounded by
    /// `radius` pixels, e.g. a light card behind a widget.
    pub fn fill_round_shaded(&mut self, area: Rect, radius: u32, color: Color) {
        let (x, y, width, height) = signed(area);
        let r = radius.min(width / 2).min(height / 2);
        for row in self.visible_rows(y, height) {
            let inset = if r == 0 {
                0
            } else {
                corner_inset(r, row, height)
            };
            let (left, top) = (
                i64::from(x) + i64::from(inset),
                i64::from(y) + i64::from(row),
            );
            let w = u64::from(width - 2 * inset);
            if let Some(rect) = self.clip_wide(left, top, w, 1) {
                self.framebuffer.shade_clipped(rect, color);
            }
        }
    }

    /// Draws small pictures with `draw` (e.g. covers in a list): they do
    /// not count as pictures that leave ghosts on e-paper, which a full
    /// refresh clears after large ones
    /// ([`Framebuffer::take_pictured`]).
    #[doc(hidden)]
    pub fn small_pictures(&mut self, draw: impl FnOnce(&mut Canvas<'_>)) {
        let was = self.framebuffer.set_counting(false);
        draw(self);
        self.framebuffer.set_counting(was);
    }

    /// Washes what is drawn in `area` out towards the paper, e.g. what
    /// lies below a menu, so that the menu stands out (on panels of black
    /// and white, every other pixel paper).
    pub fn wash(&mut self, area: Rect) {
        if let Some(rect) = self.clip_signed(
            i32::from(area.x),
            i32::from(area.y),
            u32::from(area.width),
            u32::from(area.height),
        ) {
            self.framebuffer.wash_clipped(rect);
        }
    }

    /// Draws the outline of `area`, `thickness` wide, growing inwards.
    pub fn stroke(&mut self, area: Rect, thickness: u32, color: Color) {
        let (x, y, width, height) = signed(area);
        self.stroke_rect(x, y, width, height, thickness, color);
    }

    /// Fills `area` with its corners rounded by `radius` pixels.
    pub fn fill_round(&mut self, area: Rect, radius: u32, color: Color) {
        let (x, y, width, height) = signed(area);
        self.fill_round_rect(x, y, width, height, radius, color);
    }

    /// Rounds the corners of what is drawn in `area` by `radius` pixels,
    /// e.g. a picture: what lies outside the arcs is filled with `color`,
    /// the paper behind it. Only the corners' rows are touched.
    pub fn fill_corners(&mut self, area: Rect, radius: u32, color: Color) {
        let (x, y, width, height) = signed(area);
        let r = radius.min(width / 2).min(height / 2);
        for row in (0..r).chain(height - r..height) {
            let inset = corner_inset(r, row, height);
            let y = y + row as i32;
            self.fill_rect(x, y, inset, 1, color);
            self.fill_rect(x + (width - inset) as i32, y, inset, 1, color);
        }
    }

    /// Draws the outline of `area` with its corners rounded by `radius`
    /// pixels, `thickness` wide, growing inwards.
    pub fn stroke_round(&mut self, area: Rect, thickness: u32, radius: u32, color: Color) {
        let (x, y, width, height) = signed(area);
        self.stroke_round_rect(x, y, width, height, thickness, radius, color);
    }

    /// Fills a rectangle given in signed coordinates.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: Color) {
        self.fill_wide(x.into(), y.into(), width.into(), height.into(), color);
    }

    /// [`fill_rect`](Self::fill_rect) in coordinates wide enough for the
    /// sums of any of its arguments.
    fn fill_wide(&mut self, x: i64, y: i64, width: u64, height: u64, color: Color) {
        if let Some(rect) = self.clip_wide(x, y, width, height) {
            self.framebuffer.fill_clipped(rect, color);
        }
    }

    /// Draws the outline of a rectangle with the given border thickness,
    /// growing inwards.
    pub fn stroke_rect(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        thickness: u32,
        color: Color,
    ) {
        let t = thickness.min(width / 2 + 1).min(height / 2 + 1);
        let (x, y) = (i64::from(x), i64::from(y));
        let (width, height, t) = (u64::from(width), u64::from(height), u64::from(t));
        self.fill_wide(x, y, width, t, color);
        self.fill_wide(x, y + height as i64 - t as i64, width, t, color);
        self.fill_wide(x, y, t, height, color);
        self.fill_wide(x + width as i64 - t as i64, y, t, height, color);
    }

    /// Fills a rectangle with corners rounded by `radius` pixels (none
    /// for 0).
    pub fn fill_round_rect(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        radius: u32,
        color: Color,
    ) {
        let r = radius.min(width / 2).min(height / 2);
        if r == 0 {
            return self.fill_rect(x, y, width, height, color);
        }
        for row in self.visible_rows(y, height) {
            let inset = corner_inset(r, row, height);
            self.fill_wide(
                i64::from(x) + i64::from(inset),
                i64::from(y) + i64::from(row),
                u64::from(width - 2 * inset),
                1,
                color,
            );
        }
    }

    /// Draws the outline of a rectangle with corners rounded by `radius`
    /// pixels, `thickness` wide, growing inwards.
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    pub fn stroke_round_rect(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        thickness: u32,
        radius: u32,
        color: Color,
    ) {
        let r = radius.min(width / 2).min(height / 2);
        let t = thickness.min(width / 2 + 1).min(height / 2 + 1);
        if r == 0 {
            return self.stroke_rect(x, y, width, height, thickness, color);
        }
        // Each row: the outer span, less the inner rectangle's span.
        let (inner_w, inner_h) = (width.saturating_sub(2 * t), height.saturating_sub(2 * t));
        let inner_r = r.saturating_sub(t);
        for row in self.visible_rows(y, height) {
            let outer = corner_inset(r, row, height);
            let left = i64::from(x) + i64::from(outer);
            let top = i64::from(y) + i64::from(row);
            let span = u64::from(width - 2 * outer);
            let inner_row = row.checked_sub(t).filter(|r| *r < inner_h);
            match inner_row {
                Some(inner_row) if inner_w > 0 => {
                    let inner = t + corner_inset(inner_r, inner_row, inner_h);
                    let side = u64::from(inner.saturating_sub(outer));
                    self.fill_wide(left, top, side, 1, color);
                    let right = i64::from(x) + i64::from(width - inner);
                    self.fill_wide(right, top, side, 1, color);
                }
                _ => self.fill_wide(left, top, span, 1, color),
            }
        }
    }

    /// Draws a straight line from `from` to `to`, `width` pixels thick
    /// (a square pen moved along it).
    pub fn draw_line(&mut self, from: Point, to: Point, width: u32, color: Color) {
        let (x0, y0) = (i64::from(from.x), i64::from(from.y));
        let (dx, dy) = (i64::from(to.x) - x0, i64::from(to.y) - y0);
        let steps = dx.unsigned_abs().max(dy.unsigned_abs()).max(1);
        // The pen's place at step `step` of `steps` in one coordinate,
        // rounded towards the start.
        let at = |start: i64, d: i64, step: u64| {
            let moved = (d.unsigned_abs() * step / steps) as i64;
            if d < 0 { start - moved } else { start + moved }
        };
        let pen = i64::from(width.max(1));
        let half = i64::from(width / 2);
        // Row by row, over the rows of the canvas it reaches: the steps
        // whose pen covers a row follow one another, and as the pen moves
        // by a pixel at most from one to the next, together they cover a
        // single span of the row, from the first one's pen to the last's.
        let (_, clip_top, _, clip_bottom) = self.local_clip();
        let first_row = (y0.min(y0 + dy) - half).max(clip_top);
        let end_row = (y0.max(y0 + dy) - half + pen).min(clip_bottom);
        for row in first_row..end_row {
            // The steps whose pen covers the row are those at `low..=high`.
            let (low, high) = (row + half - pen + 1, row + half);
            let (first, end) = if dy >= 0 {
                (
                    first_step(steps, |s| at(y0, dy, s) >= low),
                    first_step(steps, |s| at(y0, dy, s) > high),
                )
            } else {
                (
                    first_step(steps, |s| at(y0, dy, s) <= high),
                    first_step(steps, |s| at(y0, dy, s) < low),
                )
            };
            if first >= end {
                continue;
            }
            let (a, b) = (at(x0, dx, first), at(x0, dx, end - 1));
            let left = a.min(b) - half;
            let right = a.max(b) - half + pen;
            self.fill_wide(left, row, (right - left) as u64, 1, color);
        }
    }

    /// Fills a circle of `radius` pixels around `center` (the corner
    /// between four pixels, so the circle is `2 * radius` across).
    pub fn fill_circle(&mut self, center: Point, radius: u32, color: Color) {
        self.fill_ring(center, radius, radius, 0, 360, color);
    }

    /// Draws a circle's outline of `radius` pixels around `center`,
    /// `thickness` wide, growing inwards.
    pub fn stroke_circle(&mut self, center: Point, radius: u32, thickness: u32, color: Color) {
        self.fill_ring(center, radius, thickness, 0, 360, color);
    }

    /// Draws an arc of a ring of `radius` pixels around `center`,
    /// `thickness` wide, growing inwards: `sweep` degrees clockwise from
    /// `from` (degrees clockwise from twelve o'clock, see
    /// [`round`](crate::round)), e.g. a progress ring.
    pub fn fill_ring(
        &mut self,
        center: Point,
        radius: u32,
        thickness: u32,
        from: i32,
        sweep: u32,
        color: Color,
    ) {
        let radius = radius.min(MAX_RADIUS);
        let arc = round::Arc::new(from, sweep);
        let ring = round::Ring::new(radius, thickness, arc);
        let r = i64::from(radius);
        let size = 2 * u64::from(radius);
        let (x, y) = (i64::from(center.x) - r, i64::from(center.y) - r);
        // Row by row, only the stretches the ring may cover (the box
        // round it is mostly outside it or in its hole), with smooth
        // edges where the panel has grays.
        let Some(shown) = self.clip_wide(x, y, size, size) else {
            return;
        };
        let top = y + i64::from(self.origin.1);
        for row in i64::from(shown.y)..i64::from(shown.bottom()) {
            let dy = row - top;
            let solid = ring.solid(dy - r);
            for (first, last) in ring.stretches(dy - r).into_iter().flatten() {
                // In the box's columns, what is covered in whole as one
                // stretch, the pixels by it as the ring covers them.
                let (first, last) = (first + r, last + r);
                let mut at = first.max(0);
                let end = (last + 1).min(size as i64);
                for (whole_first, whole_last) in solid.into_iter().flatten() {
                    let whole = (whole_first + r).max(at)..(whole_last + r + 1).min(end);
                    if whole.start >= whole.end {
                        continue;
                    }
                    self.ring_pixels(&ring, (x, y, r), dy, at..whole.start, color);
                    let (from, to) = (whole.start, whole.end);
                    self.fill_wide(x + from, y + dy, (to - from) as u64, 1, color);
                    at = whole.end;
                }
                self.ring_pixels(&ring, (x, y, r), dy, at..end, color);
            }
        }
    }

    /// The pixels of row `dy` of a ring's box (at `x`, `y`, `r` round),
    /// in the columns `columns` of it, as much of each as the ring covers.
    fn ring_pixels(
        &mut self,
        ring: &round::Ring,
        (x, y, r): (i64, i64, i64),
        dy: i64,
        columns: core::ops::Range<i64>,
        color: Color,
    ) {
        let from = columns.start;
        if from < columns.end {
            let width = (columns.end - from) as u64;
            self.mask_wide(x + from, y + dy, width, 1, color, |dx, _| {
                ring.coverage(dx + from - r, dy - r)
            });
        }
    }

    /// Draws a hand from `center` towards `degrees` (clockwise from
    /// twelve o'clock), `length` pixels long and `width` thick, as on a
    /// clock face; `tail` pixels reach past the centre the other way.
    pub fn draw_hand(
        &mut self,
        center: Point,
        degrees: i32,
        length: u32,
        tail: u32,
        width: u32,
        color: Color,
    ) {
        let reach = |pixels: u32| i32::try_from(pixels).unwrap_or(i32::MAX);
        let tip = round::point_at(center, reach(length), degrees);
        let back = round::point_at(center, reach(tail), degrees.rem_euclid(360) + 180);
        self.draw_line(back, tip, width, color);
    }

    pub fn set_pixel(&mut self, point: Point, color: Color) {
        self.fill_rect(point.x, point.y, 1, 1, color);
    }

    /// Draws a row of 8-bit gray pixels (an image row) starting at
    /// `(x, y)`; dithered on panels with few gray levels.
    pub fn draw_gray_row(&mut self, x: i32, y: i32, gray: &[u8]) {
        let width = u32::try_from(gray.len()).unwrap_or(u32::MAX);
        let Some(rect) = self.clip_signed(x, y, width, 1) else {
            return;
        };
        let x = x + self.origin.0;
        let from = (i32::from(rect.x) - x) as usize;
        let row = &gray[from..from + usize::from(rect.width)];
        self.framebuffer.put_gray_row(rect.x, rect.y, row);
        self.framebuffer.mark_dirty(rect);
    }

    /// Draws a row of gray pixels with transparency (an image row, two
    /// bytes a pixel: gray, and how opaque it is) starting at `(x, y)`,
    /// blended over what is drawn there and dithered as pictures are;
    /// `opacity` (0 to 255) makes the whole row more see-through, e.g. a
    /// picture over a page that is to show through it.
    pub fn draw_gray_alpha_row(&mut self, x: i32, y: i32, gray_alpha: &[u8], opacity: u8) {
        let width = u32::try_from(gray_alpha.len() / 2).unwrap_or(u32::MAX);
        let Some(rect) = self.clip_signed(x, y, width, 1) else {
            return;
        };
        let x = x + self.origin.0;
        let opacity = u32::from(opacity);
        for px in rect.x..rect.right() as u16 {
            let at = (i32::from(px) - x) as usize * 2;
            let alpha = (u32::from(gray_alpha[at + 1]) * opacity + 127) / 255;
            self.framebuffer
                .put_gray_over(px, rect.y, gray_alpha[at], alpha as u8);
        }
        self.framebuffer.mark_dirty(rect);
    }

    /// Draws a row of RGB pixels (an image row, three bytes a pixel)
    /// starting at `(x, y)`: as they are on colour frames, as gray
    /// (dithered like gray rows) on gray ones.
    pub fn draw_rgb_row(&mut self, x: i32, y: i32, rgb: &[u8]) {
        let width = u32::try_from(rgb.len() / 3).unwrap_or(u32::MAX);
        let Some(rect) = self.clip_signed(x, y, width, 1) else {
            return;
        };
        let x = x + self.origin.0;
        for px in rect.x..rect.right() as u16 {
            let at = (i32::from(px) - x) as usize * 3;
            self.framebuffer
                .put_rgb(px, rect.y, [rgb[at], rgb[at + 1], rgb[at + 2]]);
        }
        self.framebuffer.mark_dirty(rect);
    }

    /// Draws `bitmap` as `blit` says, fast enough for every frame (e.g.
    /// an emulated game's screen): a picture, as gray rows are, dithered
    /// as they are unless `blit` says otherwise.
    pub fn blit(&mut self, bitmap: &Bitmap<'_>, blit: Blit) {
        if self.framebuffer.is_blind() {
            return;
        }
        let scale = u64::from(blit.scale);
        let (x, y) = (i64::from(blit.at.x), i64::from(blit.at.y));
        let (width, height) = (
            u64::from(bitmap.width()) * scale,
            u64::from(bitmap.height()) * scale,
        );
        let Some(rect) = self.clip_wide(x, y, width, height) else {
            return;
        };
        let origin = (x + i64::from(self.origin.0), y + i64::from(self.origin.1));
        crate::blit::draw(self.framebuffer, rect, origin, bitmap, blit);
        self.framebuffer.mark_dirty(rect);
    }

    /// Blends `color` over a `width`×`height` area at `(x, y)` using
    /// `coverage(dx, dy)` (0 = transparent, 255 = opaque), used for
    /// anti-aliased glyphs and icons.
    pub fn draw_mask(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: Color,
        coverage: impl Fn(u32, u32) -> u8,
    ) {
        let Some(rect) = self.clip_signed(x, y, width, height) else {
            return;
        };
        // The offsets lie inside the area, so they fit.
        let from = (
            (i64::from(rect.x) - i64::from(x) - i64::from(self.origin.0)) as u32,
            (i64::from(rect.y) - i64::from(y) - i64::from(self.origin.1)) as u32,
        );
        // A row at a time where the format allows (most panels).
        if self.framebuffer.blend_mask(rect, from, color, &coverage) {
            return self.framebuffer.mark_dirty(rect);
        }
        let coverage = |dx: i64, dy: i64| coverage(dx as u32, dy as u32);
        self.mask_wide(
            x.into(),
            y.into(),
            width.into(),
            height.into(),
            color,
            coverage,
        );
    }

    /// [`draw_mask`](Self::draw_mask) in wide coordinates, pixel by pixel.
    pub(crate) fn mask_wide(
        &mut self,
        x: i64,
        y: i64,
        width: u64,
        height: u64,
        color: Color,
        coverage: impl Fn(i64, i64) -> u8,
    ) {
        let Some(rect) = self.clip_wide(x, y, width, height) else {
            return;
        };
        let (x, y) = (x + i64::from(self.origin.0), y + i64::from(self.origin.1));
        for py in rect.y..rect.bottom() as u16 {
            for px in rect.x..rect.right() as u16 {
                let alpha = coverage(i64::from(px) - x, i64::from(py) - y);
                if alpha > 0 {
                    self.framebuffer.blend_pixel(px, py, color, alpha);
                }
            }
        }
        self.framebuffer.mark_dirty(rect);
    }

    /// The rows of a shape `height` rows high at `y` that the clip region
    /// shows, so that a shape far larger than it costs no more than it.
    fn visible_rows(&self, y: i32, height: u32) -> core::ops::Range<u32> {
        let (y, height) = (i64::from(y), i64::from(height));
        let (_, clip_top, _, clip_bottom) = self.local_clip();
        let first = (clip_top - y).clamp(0, height);
        let end = (clip_bottom - y).clamp(first, height);
        first as u32..end as u32
    }

    fn clip_signed(&self, x: i32, y: i32, width: u32, height: u32) -> Option<Rect> {
        self.clip_wide(x.into(), y.into(), width.into(), height.into())
    }

    /// The part of an area (in this canvas's coordinates) that the clip
    /// region shows, in screen coordinates.
    fn clip_wide(&self, x: i64, y: i64, width: u64, height: u64) -> Option<Rect> {
        let (x, y) = (x + i64::from(self.origin.0), y + i64::from(self.origin.1));
        let clip = self.clip;
        let size = |size: u64| i64::try_from(size).unwrap_or(i64::MAX);
        let left = x.max(i64::from(clip.x));
        let top = y.max(i64::from(clip.y));
        let right = x.saturating_add(size(width)).min(i64::from(clip.right()));
        let bottom = y.saturating_add(size(height)).min(i64::from(clip.bottom()));
        if right <= left || bottom <= top {
            return None;
        }
        Some(Rect::new(
            left as u16,
            top as u16,
            (right - left) as u16,
            (bottom - top) as u16,
        ))
    }
}

/// The first of the steps `0..=steps` at which `reached` holds, given that
/// from there on it holds at every step (`steps + 1` if it never does).
fn first_step(steps: u64, reached: impl Fn(u64) -> bool) -> u64 {
    let (mut low, mut high) = (0, steps + 1);
    while low < high {
        let middle = low + (high - low) / 2;
        if reached(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

/// A rectangle's place and size as the signed forms take them.
fn signed(area: Rect) -> (i32, i32, u32, u32) {
    (
        i32::from(area.x),
        i32::from(area.y),
        u32::from(area.width),
        u32::from(area.height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_platform::PixelFormat;

    /// A ring drawn by its stretches of each row comes out as drawn over
    /// its whole box, pixel by pixel: rings thick, thin and partial, a
    /// disc, at the edges of a canvas and cut by a clip.
    #[test]
    fn a_ring_drawn_by_stretches_is_the_ring_drawn_by_pixels() {
        let cases = [
            (60, 60, 20, 6, 0, 360),
            (30, 30, 25, 1, 0, 360),
            (30, 30, 25, 2, 0, 100),
            (10, 50, 30, 30, 0, 360),
            (50, 20, 18, 5, 40, 250),
            (5, 5, 12, 3, 0, 360),
            (58, 58, 9, 9, 0, 360),
            (30, 30, 1, 1, 0, 360),
            (30, 30, 3, 1, 0, 90),
        ];
        for (cx, cy, radius, thickness, from, sweep) in cases {
            let mut by_stretches = Framebuffer::new(64, 64, PixelFormat::Gray4).unwrap();
            let mut by_pixels = Framebuffer::new(64, 64, PixelFormat::Gray4).unwrap();
            for fb in [&mut by_stretches, &mut by_pixels] {
                let mut canvas = fb.canvas();
                canvas.clear(Color::WHITE);
            }
            {
                let mut canvas = by_stretches.canvas();
                let mut canvas = canvas.clipped(Rect::new(4, 3, 55, 58));
                canvas.fill_ring(
                    Point::new(cx, cy),
                    radius,
                    thickness,
                    from,
                    sweep,
                    Color::BLACK,
                );
            }
            {
                let ring = round::Ring::new(radius, thickness, round::Arc::new(from, sweep));
                let r = i64::from(radius);
                let mut canvas = by_pixels.canvas();
                let mut canvas = canvas.clipped(Rect::new(4, 3, 55, 58));
                canvas.mask_wide(
                    i64::from(cx) - r,
                    i64::from(cy) - r,
                    2 * r as u64,
                    2 * r as u64,
                    Color::BLACK,
                    |dx, dy| ring.coverage(dx - r, dy - r),
                );
            }
            let (a, b) = (by_stretches.view(), by_pixels.view());
            for y in 0..64 {
                for x in 0..64 {
                    assert_eq!(
                        a.pixel(x, y),
                        b.pixel(x, y),
                        "ring {radius}/{thickness} {from}+{sweep} at {cx},{cy}: pixel {x},{y}"
                    );
                }
            }
        }
    }

    /// Drawn on a canvas moved by an offset, everything lands moved, cut
    /// by the clip where it was, and is described where it shows.
    #[test]
    fn an_offset_moves_what_is_drawn() {
        let mut moved = Framebuffer::new(20, 20, PixelFormat::Gray4).unwrap();
        let mut plain = Framebuffer::new(20, 20, PixelFormat::Gray4).unwrap();
        let mut described = Vec::new();
        {
            let mut canvas = moved.canvas();
            let mut canvas = canvas.clipped(Rect::new(0, 5, 20, 15));
            let canvas = canvas.clipped(Rect::new(0, 0, 20, 20)).offset(3, 5);
            assert_eq!(canvas.clip(), Rect::new(0, 0, 17, 15));
            let mut canvas = canvas.describing(&mut described);
            canvas.fill(Rect::new(1, 1, 4, 2), Color::BLACK);
            canvas.draw_line(Point::new(0, 6), Point::new(8, 6), 1, Color::BLACK);
            canvas.draw_gray_row(2, 9, &[0, 0, 255, 0]);
            canvas.draw_mask(
                10,
                10,
                2,
                2,
                Color::BLACK,
                |dx, _| if dx == 0 { 255 } else { 0 },
            );
            canvas.fill_rect(-5, -9, 4, 4, Color::BLACK);
            canvas.describe(Role::Button, "OK", Rect::new(1, 1, 4, 2));
        }
        {
            let mut canvas = plain.canvas();
            canvas.fill(Rect::new(4, 6, 4, 2), Color::BLACK);
            canvas.draw_line(Point::new(3, 11), Point::new(11, 11), 1, Color::BLACK);
            canvas.draw_gray_row(5, 14, &[0, 0, 255, 0]);
            canvas.draw_mask(
                13,
                15,
                2,
                2,
                Color::BLACK,
                |dx, _| if dx == 0 { 255 } else { 0 },
            );
        }
        assert_eq!(moved.view().data, plain.view().data);
        assert_eq!(described[0].area, Rect::new(4, 6, 4, 2));
    }

    #[test]
    fn washed_out_towards_the_paper() {
        let area = Rect::new(3, 0, 12, 2);
        let mut mono = Framebuffer::new(20, 2, PixelFormat::Mono1).unwrap();
        mono.canvas().clear(Color::BLACK);
        mono.canvas().fill_tint(area, Color::gray(190));
        mono.canvas().wash(area);
        let view = mono.view();
        for y in 0..2 {
            for x in 0..20 {
                let inside = area.contains(Point::new(x as i32, y as i32));
                let paper = inside && (x + y) % 2 == 1;
                let expected = if paper { Color::WHITE } else { Color::BLACK };
                assert_eq!(view.pixel(x, y), expected, "({x}, {y})");
            }
        }
        // In night mode the paper is black.
        let mut night = Framebuffer::new(16, 1, PixelFormat::Mono1).unwrap();
        night.set_inverted(true);
        night.canvas().clear(Color::BLACK);
        night.canvas().wash(Rect::new(0, 0, 16, 1));
        // Stored inverted: drawn black is kept white, the paper black.
        assert_eq!(night.view().pixel(0, 0), Color::WHITE);
        assert_eq!(night.view().pixel(1, 0), Color::BLACK);
        // Grays: two thirds of the way.
        let mut gray = Framebuffer::new(4, 1, PixelFormat::Gray8).unwrap();
        gray.canvas().clear(Color::BLACK);
        gray.canvas().wash(Rect::new(0, 0, 4, 1));
        assert_eq!(gray.view().pixel(0, 0), Color::gray(170));
    }

    #[test]
    fn shades_keep_their_lightness_in_black_and_white() {
        let count = |format, gray: u8| {
            let mut fb = Framebuffer::new(16, 16, format).unwrap();
            fb.canvas()
                .fill_shaded(Rect::new(0, 0, 16, 16), Color::gray(gray));
            let view = fb.view();
            (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|&(x, y)| view.pixel(x, y) == Color::BLACK)
                .count()
        };
        // A light gray: a few black pixels, not none (it would vanish) and
        // not half (text on it would be hard to read).
        let light = count(PixelFormat::Mono1, 205);
        assert!((20..=70).contains(&light), "{light} of 256");
        assert!(count(PixelFormat::Mono1, 90) > light);
        assert_eq!(count(PixelFormat::Mono1, 255), 0);
        // Where there are enough grays, the gray itself.
        let mut gray = Framebuffer::new(2, 1, PixelFormat::Gray8).unwrap();
        gray.canvas()
            .fill_shaded(Rect::new(0, 0, 2, 1), Color::gray(205));
        assert_eq!(gray.view().pixel(1, 0), Color::gray(205));
        // Not a picture.
        let mut mono = Framebuffer::new(8, 8, PixelFormat::Mono1).unwrap();
        mono.canvas()
            .fill_shaded(Rect::new(0, 0, 8, 8), Color::gray(128));
        assert_eq!(mono.take_pictured(), 0);
    }

    #[test]
    fn small_pictures_do_not_count() {
        let mut fb = Framebuffer::new(10, 2, PixelFormat::Gray8).unwrap();
        fb.canvas().draw_gray_row(0, 0, &[100; 10]);
        fb.canvas()
            .small_pictures(|canvas| canvas.draw_gray_row(0, 1, &[100; 10]));
        assert_eq!(fb.take_pictured(), 10);
        // Counted again after.
        fb.canvas().draw_gray_row(0, 1, &[100; 4]);
        assert_eq!(fb.take_pictured(), 4);
    }

    #[test]
    fn tints_stay_visible_in_black_and_white() {
        let area = Rect::new(2, 1, 6, 2);
        let mut mono = Framebuffer::new(10, 4, PixelFormat::Mono1).unwrap();
        mono.canvas().clear(Color::WHITE);
        mono.canvas().fill_tint(area, Color::gray(190));
        let view = mono.view();
        // Every other pixel black, in a checker; none outside.
        for y in 0..4 {
            for x in 0..10 {
                let inside = area.contains(Point::new(x as i32, y as i32));
                let black = inside && (x + y) % 2 == 0;
                let expected = if black { Color::BLACK } else { Color::WHITE };
                assert_eq!(view.pixel(x, y), expected, "({x}, {y})");
            }
        }
        // With grays: the gray itself; dark ones black as they are.
        let mut gray = Framebuffer::new(10, 4, PixelFormat::Gray8).unwrap();
        gray.canvas().fill_tint(area, Color::gray(190));
        assert_eq!(gray.view().pixel(3, 1), Color::gray(190));
        mono.canvas().fill_tint(area, Color::gray(60));
        assert_eq!(mono.view().pixel(3, 1), Color::BLACK);
    }

    #[test]
    fn round_corners_leave_the_corners() {
        let mut fb = Framebuffer::new(20, 12, PixelFormat::Gray8).unwrap();
        fb.canvas().fill_round_rect(0, 0, 20, 12, 4, Color::BLACK);
        let gray = |x, y| fb.view().pixel(x, y).r;
        // The corners stay white, the middle and the straight edges fill.
        assert_eq!(gray(0, 0), 255);
        assert_eq!(gray(19, 11), 255);
        assert_eq!(gray(10, 0), 0);
        assert_eq!(gray(0, 6), 0);
        assert_eq!(gray(10, 6), 0);
    }

    #[test]
    fn corners_filled_are_what_round_shapes_leave() {
        let mut round = Framebuffer::new(20, 12, PixelFormat::Gray8).unwrap();
        round
            .canvas()
            .fill_round_rect(2, 1, 16, 10, 4, Color::BLACK);
        let mut cornered = Framebuffer::new(20, 12, PixelFormat::Gray8).unwrap();
        cornered.canvas().fill_rect(2, 1, 16, 10, Color::BLACK);
        cornered
            .canvas()
            .fill_corners(Rect::new(2, 1, 16, 10), 4, Color::WHITE);
        for y in 0..12 {
            for x in 0..20 {
                assert_eq!(round.view().pixel(x, y), cornered.view().pixel(x, y));
            }
        }
        // No radius: nothing.
        cornered
            .canvas()
            .fill_corners(Rect::new(2, 1, 16, 10), 0, Color::WHITE);
        assert_eq!(cornered.view().pixel(2, 6), Color::BLACK);
        assert_eq!(cornered.view().pixel(10, 1), Color::BLACK);
    }

    #[test]
    fn round_outlines_are_hollow() {
        let mut fb = Framebuffer::new(20, 12, PixelFormat::Gray8).unwrap();
        fb.canvas()
            .stroke_round_rect(0, 0, 20, 12, 2, 4, Color::BLACK);
        let gray = |x, y| fb.view().pixel(x, y).r;
        assert_eq!(gray(0, 0), 255);
        assert_eq!(gray(10, 0), 0);
        assert_eq!(gray(0, 6), 0);
        assert_eq!(gray(10, 6), 255);
        // Without a radius, as stroke_rect.
        let mut square = Framebuffer::new(6, 6, PixelFormat::Gray8).unwrap();
        square
            .canvas()
            .stroke_round_rect(0, 0, 6, 6, 1, 0, Color::BLACK);
        assert_eq!(square.view().pixel(0, 0).r, 0);
    }

    /// With a negative offset, what lies past the screen's top or left
    /// edge is cut off: clipping and describing stay where they are drawn.
    #[test]
    fn negative_offsets_cut_what_lies_outside() {
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        {
            let mut canvas = fb.canvas().offset(-4, -4);
            let mut inner = canvas.clipped(Rect::new(2, 2, 4, 4));
            inner.clear(Color::BLACK);
        }
        assert_eq!(fb.take_dirty(), Some(Rect::new(0, 0, 2, 2)));
        let mut described = Vec::new();
        {
            let mut canvas = fb.canvas().describing(&mut described).offset(-4, -6);
            canvas.describe(Role::Row, "row", Rect::new(1, 1, 5, 9));
        }
        assert_eq!(described[0].area, Rect::new(0, 0, 2, 4));
    }

    #[test]
    fn clips_to_screen_and_region() {
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        {
            let mut canvas = fb.canvas();
            canvas.fill_rect(-5, -5, 8, 8, Color::BLACK);
        }
        assert_eq!(fb.take_dirty(), Some(Rect::new(0, 0, 3, 3)));

        {
            let mut canvas = fb.canvas();
            let mut inner = canvas.clipped(Rect::new(5, 5, 2, 2));
            inner.clear(Color::BLACK);
            inner.fill_rect(0, 0, 100, 100, Color::BLACK);
        }
        assert_eq!(fb.take_dirty(), Some(Rect::new(5, 5, 2, 2)));
        assert_eq!(fb.view().pixel(4, 4), Color::WHITE);
    }

    /// Which pixels of a frame are black, row by row.
    fn black(fb: &Framebuffer) -> Vec<Vec<bool>> {
        let view = fb.view();
        (0..view.height)
            .map(|y| {
                (0..view.width)
                    .map(|x| view.pixel(x, y) == Color::BLACK)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn lines_are_drawn_as_a_square_pen_moved_along_them() {
        // What the line covers: a square pen at every step along it.
        let pen_steps = |from: Point, to: Point, width: u32| {
            let mut fb = Framebuffer::new(24, 20, PixelFormat::Gray8).unwrap();
            let (dx, dy) = (to.x - from.x, to.y - from.y);
            let steps = dx.abs().max(dy.abs()).max(1);
            let half = (width / 2) as i32;
            for step in 0..=steps {
                let x = from.x + dx * step / steps;
                let y = from.y + dy * step / steps;
                let pen = width.max(1);
                fb.canvas()
                    .fill_rect(x - half, y - half, pen, pen, Color::BLACK);
            }
            black(&fb)
        };
        let points = [
            (-7, -3),
            (0, 0),
            (3, 17),
            (11, 5),
            (23, 19),
            (30, 8),
            (5, -9),
            (12, 12),
        ];
        for &(x1, y1) in &points {
            for &(x2, y2) in &points {
                for width in [0, 1, 2, 3, 6] {
                    let (from, to) = (Point::new(x1, y1), Point::new(x2, y2));
                    let mut fb = Framebuffer::new(24, 20, PixelFormat::Gray8).unwrap();
                    fb.canvas().draw_line(from, to, width, Color::BLACK);
                    assert_eq!(
                        black(&fb),
                        pen_steps(from, to, width),
                        "{from:?} {to:?} {width}"
                    );
                }
            }
        }
    }

    #[test]
    fn lines_from_far_away_are_clipped_first() {
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        let (left, right) = (Point::new(i32::MIN, 5), Point::new(i32::MAX, 5));
        fb.canvas().draw_line(left, right, 1, Color::BLACK);
        let rows = black(&fb);
        for (y, row) in rows.iter().enumerate() {
            assert!(row.iter().all(|&b| b == (y == 5)), "row {y}");
        }
        // Corner to corner of the whole plane, with a pen wider than it.
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        let (min, max) = (
            Point::new(i32::MIN, i32::MIN),
            Point::new(i32::MAX, i32::MAX),
        );
        fb.canvas().draw_line(min, max, 1, Color::BLACK);
        assert!((0..10).all(|i| black(&fb)[i][i]));
        fb.canvas().draw_line(max, min, u32::MAX, Color::BLACK);
        assert!(black(&fb).iter().flatten().all(|&b| b));
    }

    #[test]
    fn huge_shapes_are_clipped_first() {
        // Rounded: radius and rows far past the screen.
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        fb.canvas()
            .fill_round_rect(-5, -5, u32::MAX, 70_000, u32::MAX, Color::BLACK);
        fb.canvas()
            .fill_round_rect(-5, -5, 30, u32::MAX, 4, Color::BLACK);
        assert!(black(&fb).iter().flatten().all(|&b| b));
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        fb.canvas()
            .stroke_round_rect(-5, 2, u32::MAX, u32::MAX, 2, 4, Color::BLACK);
        assert!(black(&fb)[2][5] && black(&fb)[3][5] && !black(&fb)[4][5]);
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        fb.canvas()
            .fill_round_shaded(Rect::new(0, 0, 10, 10), u32::MAX, Color::BLACK);
        assert!(black(&fb)[5][5]);
        // Square: the far edges stay where they are, off the screen.
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        fb.canvas().stroke_rect(0, 5, 10, u32::MAX, 1, Color::BLACK);
        let rows = black(&fb);
        assert!(rows[..5].iter().flatten().all(|&b| !b), "nothing above");
        assert!(rows[5].iter().all(|&b| b) && rows[7][0] && rows[7][9] && !rows[7][5]);
        // Round: a disc larger than any screen covers it.
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        fb.canvas()
            .fill_circle(Point::new(5, 5), u32::MAX, Color::BLACK);
        assert!(black(&fb).iter().flatten().all(|&b| b));
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        let center = Point::new(i32::MAX, i32::MIN);
        fb.canvas()
            .fill_ring(center, u32::MAX, 3, i32::MAX, 90, Color::BLACK);
        fb.canvas()
            .draw_hand(center, i32::MAX, u32::MAX, u32::MAX, 2, Color::BLACK);
    }

    #[test]
    fn stroke_draws_only_the_border() {
        let mut fb = Framebuffer::new(10, 10, PixelFormat::Gray8).unwrap();
        fb.canvas().stroke_rect(0, 0, 10, 10, 2, Color::BLACK);
        let view = fb.view();
        assert_eq!(view.pixel(1, 1), Color::BLACK);
        assert_eq!(view.pixel(8, 8), Color::BLACK);
        assert_eq!(view.pixel(2, 2), Color::WHITE);
        assert_eq!(view.pixel(5, 7), Color::WHITE);
    }
}
