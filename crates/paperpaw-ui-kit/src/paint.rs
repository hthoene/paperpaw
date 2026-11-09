//! Prepared drawing: owned operations laid out first, replayed in the draw callback.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error,
        callbacks::{DrawFrame, FrameRect},
        fonts::{Font, IconGlyph, TextOrigin},
    },
};

/// One drawing operation with owned text and scalar geometry only, so
/// nothing callback-scoped (fonts, canvas) outlives its callback.
#[derive(Debug)]
#[non_exhaustive]
pub enum Paint {
    /// A filled rectangle.
    Fill(Rect, Color),
    /// A rounded rectangle: bounds, radius, stroke thickness (`None`: filled), colour.
    Focus(Rect, u32, Option<u32>, Color),
    /// A straight line from one point to the other, `width` pixels thick.
    Line {
        /// Where it starts.
        from: Point,
        /// Where it ends.
        to: Point,
        /// Pen width in pixels.
        width: u32,
        /// Colour.
        color: Color,
    },
    /// A text run in the bold font of the interface (a lit letter among faint
    /// ones); drawn only by [`replay_with`] given that font.
    Bold {
        /// What to draw.
        text: String,
        /// Size in pixels.
        size: f32,
        /// Where the baseline starts.
        origin: TextOrigin,
        /// Text colour.
        color: Color,
    },
    /// A named icon: one character of the bound icon font; drawn only by
    /// [`replay_icons`] given that font.
    Icon {
        /// The character the host gave for the icon's name.
        glyph: IconGlyph,
        /// Size in pixels.
        size: f32,
        /// Where the baseline starts.
        origin: TextOrigin,
        /// Colour.
        color: Color,
    },
    /// A text run, optionally clipped.
    Text {
        /// What to draw.
        text: String,
        /// Size in pixels.
        size: f32,
        /// Where the baseline starts.
        origin: TextOrigin,
        /// Text colour.
        color: Color,
        /// Clip, if any.
        clip: Option<Rect>,
    },
}

/// A header's drawing and its tap regions.
#[derive(Debug, Default)]
pub struct Page {
    /// What to draw.
    pub paint: Vec<Paint>,
    /// The back arrow's tap region (touch devices).
    pub back: Option<Rect>,
    /// Previous arrow, label and next arrow regions (touch devices, several pages).
    pub pager: Option<(Rect, Rect, Rect)>,
}

impl Page {
    /// The action a tap at `point` means: back, or one page either way. The
    /// label between the arrows does nothing.
    #[must_use]
    pub fn action(&self, point: Point) -> Option<&'static str> {
        if self.back.is_some_and(|rect| rect.contains(point)) {
            return Some("core.back");
        }
        let (previous, _, next) = self.pager?;
        if previous.contains(point) {
            Some("core.prev-page")
        } else if next.contains(point) {
            Some("core.next-page")
        } else {
            None
        }
    }
}

/// Pushes `op` with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn push(paint: &mut Vec<Paint>, op: Paint) -> Result<(), Error> {
    paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
    paint.push(op);
    Ok(())
}

/// A host rectangle for a widget rectangle.
#[must_use]
pub fn frame_rect(rect: Rect) -> FrameRect {
    FrameRect::new(
        i32::from(rect.x),
        i32::from(rect.y),
        u32::from(rect.width),
        u32::from(rect.height),
    )
}

/// Draws `ops` in order on the frame's canvas with `font`.
///
/// # Errors
/// Propagates canvas and font errors; [`Error::Unsupported`] for a bold run.
pub fn replay<'a>(
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    ops: impl IntoIterator<Item = &'a Paint>,
) -> Result<(), Error> {
    replay_with(frame, font, None, ops)
}

/// [`replay`] with a bold font for [`Paint::Bold`] runs.
///
/// # Errors
/// Propagates canvas and font errors; [`Error::Unsupported`] for a bold run
/// without a bold font.
pub fn replay_with<'a>(
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    bold: Option<&Font<'_>>,
    ops: impl IntoIterator<Item = &'a Paint>,
) -> Result<(), Error> {
    replay_all(frame, font, (bold, None), ops)
}

/// [`replay`] with the icon font for [`Paint::Icon`] runs: an icon is left
/// out where there is no icon font (the callers gave a name the host knows only
/// with one).
///
/// # Errors
/// Propagates canvas and font errors.
pub fn replay_icons<'a>(
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    icons: Option<&Font<'_>>,
    ops: impl IntoIterator<Item = &'a Paint>,
) -> Result<(), Error> {
    replay_all(frame, font, (None, icons), ops)
}

/// Draws `ops` in order with `font`, the bold font of [`Paint::Bold`] runs and
/// the icon font of [`Paint::Icon`] runs.
///
/// # Errors
/// Propagates canvas and font errors; [`Error::Unsupported`] for a bold run
/// without a bold font. An icon without an icon font is not drawn.
pub fn replay_all<'a>(
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    (bold, icons): (Option<&Font<'_>>, Option<&Font<'_>>),
    ops: impl IntoIterator<Item = &'a Paint>,
) -> Result<(), Error> {
    let mut canvas = frame.draw_canvas()?;
    for op in ops {
        match op {
            Paint::Fill(rect, color) => canvas.fill_rectangle(frame_rect(*rect), *color)?,
            Paint::Focus(rect, radius, thickness, color) => match thickness {
                Some(thickness) => {
                    canvas.stroke_rounded_rectangle(
                        frame_rect(*rect),
                        *radius,
                        *thickness,
                        *color,
                    )?;
                }
                None => canvas.fill_rounded_rectangle(frame_rect(*rect), *radius, *color)?,
            },
            Paint::Line {
                from,
                to,
                width,
                color,
            } => canvas.draw_segment(*from, *to, *width, *color)?,
            Paint::Bold {
                text,
                size,
                origin,
                color,
            } => {
                let bold = bold.ok_or(Error::Unsupported)?;
                bold.render_text(&canvas, *size, *origin, *color, text)?;
            }
            Paint::Icon {
                glyph,
                size,
                origin,
                color,
            } => {
                if let Some(icons) = icons {
                    icons.render_text(&canvas, *size, *origin, *color, glyph.as_str())?;
                }
            }
            Paint::Text {
                text,
                size,
                origin,
                color,
                clip,
            } => {
                if let Some(rect) = clip {
                    let region = canvas.clip_region(frame_rect(*rect))?;
                    font.render_text(&region, *size, *origin, *color, text)?;
                    region.restore()?;
                } else {
                    font.render_text(&canvas, *size, *origin, *color, text)?;
                }
            }
        }
    }
    Ok(())
}
