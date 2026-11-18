//! Prepared drawing: owned operations laid out first, replayed in the draw callback.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error,
        callbacks::{DrawFrame, DrawPurpose, FrameRect},
        canvas::{Canvas, PartFlags, PartRole},
        fonts::{Font, IconGlyph, Ink, TextOrigin},
    },
};

/// One drawing operation with owned text and scalar geometry only, so
/// nothing callback-scoped (fonts, canvas) outlives its callback.
#[derive(Debug)]
#[non_exhaustive]
pub enum Paint {
    /// A filled rectangle.
    Fill(Rect, Color),
    /// A light separator on a monochrome display, in alternating pixels.
    Stipple(Rect, Color),
    /// A rounded rectangle: bounds, radius, stroke thickness (`None`: filled), colour.
    Focus(Rect, u32, Option<u32>, Color),
    /// A section card extended past a page cut, clipped to its visible rows.
    Section {
        rect: FrameRect,
        clip: Rect,
        radius: u32,
        thickness: Option<u32>,
        color: Color,
    },
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
    /// A text run drawn bolder (a lit letter among faint ones): the regular font
    /// with its glyphs widened by `max(1, size / 20)` pixels, as Beta 3 did.
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
    /// Tells what is drawn (`canvas.describe`): a part of `role` at `rect`, with
    /// the text drawn inside it as its label. Draws nothing; skipped by the
    /// replay where nobody asks, and once nobody does, its successors too.
    Describe {
        /// What the part is.
        role: PartRole,
        /// Focused, on, disabled.
        flags: PartFlags,
        /// Where it is, in screen pixels.
        rect: Rect,
    },
    /// Tells what is drawn, as [`Paint::Describe`], for a part whose label is
    /// not drawn as text inside it (an icon, a sign): `label` names it.
    DescribeNamed {
        /// What the part is.
        role: PartRole,
        /// Focused, on, disabled.
        flags: PartFlags,
        /// Where it is, in screen pixels.
        rect: Rect,
        /// What it is called.
        label: String,
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

/// Whether anybody asks what this frame shows (`canvas.describe`): one call
/// that tells an empty text, which the host leaves out of what it collects.
/// Ask once at the start of a draw callback and keep the answer in
/// [`Style::describing`](crate::Style::describing): nothing is queued, nor
/// copied, where nobody asks. Where the frame cannot be asked, nobody does.
#[must_use]
pub fn asking(frame: &mut DrawFrame<'_>) -> bool {
    let Ok(canvas) = frame.draw_canvas() else {
        return false;
    };
    canvas
        .describe_part(
            PartRole::Text,
            "",
            FrameRect::new(0, 0, 0, 0),
            PartFlags::NONE,
        )
        .unwrap_or(false)
}

/// Pushes a [`Paint::Describe`] of `role` at `rect`, to come before the text
/// drawn inside it: focused where `focused`, faint (does nothing) where
/// `disabled`. Nothing is queued where `describing` is not set (see
/// [`asking`]).
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn describe(
    paint: &mut Vec<Paint>,
    describing: bool,
    role: PartRole,
    rect: Rect,
    focused: bool,
    disabled: bool,
) -> Result<(), Error> {
    if !describing {
        return Ok(());
    }
    let mut flags = PartFlags::NONE;
    if focused {
        flags = flags | PartFlags::FOCUSED;
    }
    if disabled {
        flags = flags | PartFlags::DISABLED;
    }
    push(paint, Paint::Describe { role, flags, rect })
}

/// Pushes a [`Paint::DescribeNamed`]: as [`describe`], for a part that shows
/// an icon or a sign and no words, named `label` (a button without a name is
/// told as nothing a reader could say or a test could find).
///
/// # Errors
/// [`Error::NoMemory`] when the list or the label cannot grow.
pub fn describe_named(
    paint: &mut Vec<Paint>,
    describing: bool,
    role: PartRole,
    rect: Rect,
    label: &str,
    focused: bool,
    disabled: bool,
) -> Result<(), Error> {
    if !describing {
        return Ok(());
    }
    let mut flags = PartFlags::NONE;
    if focused {
        flags = flags | PartFlags::FOCUSED;
    }
    if disabled {
        flags = flags | PartFlags::DISABLED;
    }
    let label = crate::text::copy(label)?;
    push(
        paint,
        Paint::DescribeNamed {
            role,
            flags,
            rect,
            label,
        },
    )
}

/// Pushes the [`Paint::Describe`] of a switch, `on` or off, at `rect`: inside a
/// row it makes the row on or off. Nothing is queued where `describing` is not
/// set (see [`asking`]).
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn describe_switch(
    paint: &mut Vec<Paint>,
    describing: bool,
    rect: Rect,
    on: bool,
    disabled: bool,
) -> Result<(), Error> {
    if !describing {
        return Ok(());
    }
    let mut flags = if on { PartFlags::ON } else { PartFlags::OFF };
    if disabled {
        flags = flags | PartFlags::DISABLED;
    }
    push(
        paint,
        Paint::Describe {
            role: PartRole::Switch,
            flags,
            rect,
        },
    )
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
/// Propagates canvas and font errors.
pub fn replay<'a>(
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    ops: impl IntoIterator<Item = &'a Paint>,
) -> Result<(), Error> {
    replay_icons(frame, font, None, ops)
}

/// How many pixels a [`Paint::Bold`] run widens its glyphs: a twentieth of the
/// size, at least one.
#[must_use]
pub fn bold_extra(size: f32) -> u32 {
    // A size that is not finite and positive is refused by the draw itself.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a saturating float-to-integer cast is wanted; the host clamps the widening to 16"
    )]
    let twentieth = (size / 20.0) as u32;
    twentieth.max(1)
}

/// Draws the QR code `code`, black on white with the four modules of quiet
/// zone the standard asks for, as large as a whole number of pixels per
/// module allows in `area` and centred in it: each run of dark modules in a
/// row is one rectangle. Nothing is drawn where not even one pixel a module
/// fits.
///
/// # Errors
/// Propagates canvas errors.
pub fn draw_code(
    frame: &mut DrawFrame<'_>,
    code: &paperpaw_sdk::qr::QrCode,
    area: Rect,
) -> Result<(), Error> {
    let Some(modules) = u16::try_from(code.size())
        .ok()
        .and_then(|size| size.checked_add(8))
    else {
        return Ok(());
    };
    let scale = area.width.min(area.height) / modules;
    if scale == 0 {
        return Ok(());
    }
    let side = scale * modules;
    let x = area.x.saturating_add((area.width - side) / 2);
    let y = area.y.saturating_add((area.height - side) / 2);
    let canvas = frame.draw_canvas()?;
    canvas.fill_rectangle(frame_rect(Rect::new(x, y, side, side)), Color::WHITE)?;
    let (x0, y0) = (x + 4 * scale, y + 4 * scale);
    for row in 0..code.size() {
        let mut at = 0;
        while at < code.size() {
            if !code.dark(at, row) {
                at += 1;
                continue;
            }
            let start = at;
            while at < code.size() && code.dark(at, row) {
                at += 1;
            }
            // Both fit a `u16`: the code is at most 57 modules a side.
            let (left, top) = (x0 + start as u16 * scale, y0 + row as u16 * scale);
            let width = (at - start) as u16 * scale;
            canvas.fill_rectangle(frame_rect(Rect::new(left, top, width, scale)), Color::BLACK)?;
        }
    }
    Ok(())
}

/// [`replay`], with the icon font bound for the draw where `ops` has an icon
/// run (and released after it): the way a list of rows with icons is drawn.
///
/// # Errors
/// Propagates canvas, font and allocation errors.
pub fn replay_with_icons<'a>(
    host: &paperpaw_sdk::native::Host<'_>,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    ops: impl Iterator<Item = &'a Paint> + Clone,
) -> Result<(), Error> {
    let icons = if ops.clone().any(|op| matches!(op, Paint::Icon { .. })) {
        crate::icons::font(host)?
    } else {
        None
    };
    let drawn = replay_icons(frame, font, icons.as_ref(), ops);
    if let Some(icons) = icons {
        icons.release()?;
    }
    drawn
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
    let (clip_top, clip_bottom) = band_rows(
        frame.purpose == DrawPurpose::Widget,
        frame.rect.y,
        (frame.clip_top, frame.clip_bottom),
    );
    let mut canvas = frame.draw_canvas()?;
    // Whether anybody asks what is drawn; once not, no more is told.
    let mut asked = true;
    for op in ops {
        // A frame is drawn in bands: an operation wholly outside this one
        // changes no pixel and costs a host call (a text about 2 ms on the X4).
        if outside_band(op, clip_top, clip_bottom) {
            continue;
        }
        match op {
            Paint::Describe { role, flags, rect } => {
                if asked {
                    // Only a refused area leaves the part out and goes on;
                    // nobody asking, or a host that cannot be asked, ends it.
                    asked = matches!(
                        canvas.describe_part(*role, "", frame_rect(*rect), *flags),
                        Ok(true) | Err(Error::Invalid)
                    );
                }
            }
            Paint::DescribeNamed {
                role,
                flags,
                rect,
                label,
            } => {
                if asked {
                    asked = matches!(
                        canvas.describe_part(*role, label, frame_rect(*rect), *flags),
                        Ok(true) | Err(Error::Invalid)
                    );
                }
            }
            Paint::Fill(rect, color) => canvas.fill_rectangle(frame_rect(*rect), *color)?,
            Paint::Stipple(rect, color) => {
                stipple(&canvas, *rect, *color, (clip_top, clip_bottom))?
            }
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
            section @ Paint::Section { .. } => replay_section(&mut canvas, section)?,
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
                let ink = Ink::new().extra(bold_extra(*size));
                font.draw_inked(&canvas, *size, *origin, *color, text, ink)?;
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

/// The rows of a frame's band in the coordinates of its operations. A screen's
/// are relative to its pane, whose top is `rect_y` in screen pixels; a widget's
/// rectangle, clip and operations are all in the coordinates of the pane it
/// stands in (the widget draws at its own `rect.y`), so its band is the clip as
/// it is: taking the rectangle's top off it left every operation of a widget
/// below the first band outside the band, and nothing of it was drawn.
const fn band_rows(widget: bool, rect_y: i32, (top, bottom): (i32, i32)) -> (i32, i32) {
    if widget {
        (top, bottom)
    } else {
        (top - rect_y, bottom - rect_y)
    }
}

/// Whether `op` lies wholly outside the rows `top..bottom` of its frame (it
/// draws nothing there). Texts are taken to reach a size and a half above and
/// below their baseline, strokes their width past their line; what tells and
/// does not draw is never outside.
fn outside_band(op: &Paint, top: i32, bottom: i32) -> bool {
    if top >= bottom {
        return false;
    }
    let apart = |from: i32, to: i32| to <= top || from >= bottom;
    let rows = |rect: &Rect, margin: i32| {
        apart(
            i32::from(rect.y).saturating_sub(margin),
            (i32::from(rect.y) + i32::from(rect.height)).saturating_add(margin),
        )
    };
    // Floats of a font size: well under `i32`.
    #[allow(clippy::cast_possible_truncation, reason = "a size in pixels")]
    let reach = |size: f32| (size * 1.5) as i32 + 2;
    match op {
        Paint::Fill(rect, _) | Paint::Stipple(rect, _) => rows(rect, 0),
        Paint::Focus(rect, _, thickness, _) => rows(rect, thickness.map_or(0, |t| t as i32) + 2),
        Paint::Section { clip, .. } => rows(clip, 0),
        Paint::Line {
            from, to, width, ..
        } => {
            let margin = (*width as i32).saturating_add(2);
            apart(
                from.y.min(to.y).saturating_sub(margin),
                from.y.max(to.y).saturating_add(margin),
            )
        }
        Paint::Bold { size, origin, .. } | Paint::Icon { size, origin, .. } => apart(
            origin.baseline.saturating_sub(reach(*size)),
            origin.baseline.saturating_add(reach(*size)),
        ),
        Paint::Text {
            size, origin, clip, ..
        } => {
            clip.as_ref().is_some_and(|rect| rows(rect, 0))
                || apart(
                    origin.baseline.saturating_sub(reach(*size)),
                    origin.baseline.saturating_add(reach(*size)),
                )
        }
        _ => false,
    }
}

fn replay_section(canvas: &mut Canvas<'_>, op: &Paint) -> Result<(), Error> {
    if let Paint::Section {
        rect,
        clip,
        radius,
        thickness,
        color,
    } = op
    {
        let clipped = canvas.clip_region(frame_rect(*clip))?;
        match thickness {
            Some(line) => clipped.stroke_rounded_rectangle(*rect, *radius, *line, *color)?,
            None => clipped.fill_rounded_rectangle(*rect, *radius, *color)?,
        }
        clipped.restore()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Paint, band_rows, bold_extra, describe};
    use alloc::vec::Vec;
    use paperpaw_sdk::{
        Rect,
        native::canvas::{PartFlags, PartRole},
    };

    #[test]
    fn a_widget_below_the_first_band_is_drawn_in_its_own_rows() {
        // A widget at y 312, its first band 312..384: its operations lie at 312 and
        // below, in the pane's coordinates, as its clip does.
        assert_eq!(band_rows(true, 312, (312, 384)), (312, 384));
        // A screen's operations are relative to its pane, whose top is at 40.
        assert_eq!(band_rows(false, 40, (240, 490)), (200, 450));
        assert_eq!(band_rows(false, 0, (0, 250)), (0, 250));
    }

    #[test]
    fn a_part_is_queued_with_its_flags_and_only_where_somebody_asks() {
        let rect = Rect::new(0, 10, 100, 20);
        let mut paint = Vec::new();
        describe(&mut paint, true, PartRole::Row, rect, true, true).unwrap();
        assert!(matches!(
            paint.as_slice(),
            [Paint::Describe { role: PartRole::Row, flags, rect: at }]
                if *flags == (PartFlags::FOCUSED | PartFlags::DISABLED) && *at == rect
        ));
        let mut none = Vec::new();
        describe(&mut none, false, PartRole::Row, rect, true, false).unwrap();
        assert!(none.is_empty(), "nothing is queued where nobody asks");
        assert_eq!(none.capacity(), 0, "nor is room made for it");
    }

    #[test]
    fn a_named_part_keeps_its_name_and_only_where_somebody_asks() {
        let rect = Rect::new(4, 4, 20, 20);
        let mut paint = Vec::new();
        super::describe_named(&mut paint, true, PartRole::Button, rect, "+", true, false).unwrap();
        assert!(matches!(
            paint.as_slice(),
            [Paint::DescribeNamed { role: PartRole::Button, flags, rect: at, label }]
                if *flags == PartFlags::FOCUSED && *at == rect && label == "+"
        ));
        let mut none = Vec::new();
        super::describe_named(&mut none, false, PartRole::Button, rect, "+", false, false).unwrap();
        assert!(none.is_empty(), "nothing is queued where nobody asks");
    }

    #[test]
    fn a_switch_is_told_on_or_off_and_faint_where_disabled() {
        let rect = Rect::new(0, 0, 10, 10);
        let mut paint = Vec::new();
        super::describe_switch(&mut paint, true, rect, true, false).unwrap();
        super::describe_switch(&mut paint, true, rect, false, true).unwrap();
        super::describe_switch(&mut paint, false, rect, true, false).unwrap();
        let flags: Vec<u32> = paint
            .iter()
            .filter_map(|op| match op {
                Paint::Describe {
                    role: PartRole::Switch,
                    flags,
                    ..
                } => Some(flags.bits()),
                _ => None,
            })
            .collect();
        assert_eq!(flags, [0b0110, 0b1010]);
    }

    #[test]
    fn bold_widens_by_a_twentieth_of_the_size_and_at_least_one_pixel() {
        for (size, extra) in [
            (1.0, 1),
            (12.0, 1),
            (39.9, 1),
            (40.0, 2),
            (100.0, 5),
            (1_000.0, 50),
            (f32::NAN, 1),
            (-5.0, 1),
        ] {
            assert_eq!(bold_extra(size), extra, "size {size}");
        }
    }
}

/// A stippled rectangle, ink black: one host call (a checker whose corner
/// pixel is inked where the rectangle's own is: `x ^ y` even), or one pixel at
/// a time on a host without the slot (#291: a separator is 200 pixels, and on
/// the X4 every host call costs about a millisecond).
fn stipple(
    canvas: &Canvas<'_>,
    rect: Rect,
    color: Color,
    (clip_top, clip_bottom): (i32, i32),
) -> Result<(), Error> {
    if color.luma() < 128 {
        return canvas.fill_rectangle(frame_rect(rect), Color::BLACK);
    }
    let odd = (rect.x ^ rect.y) & 1 == 1;
    match canvas.fill_checker(frame_rect(rect), Color::BLACK, odd) {
        Err(Error::Unsupported) => {}
        other => return other,
    }
    for y in rect.y..rect.y.saturating_add(rect.height) {
        if i32::from(y) < clip_top || i32::from(y) >= clip_bottom {
            continue;
        }
        for x in rect.x..rect.x.saturating_add(rect.width) {
            if stipple_ink(x, y, color) {
                canvas.fill_rectangle(frame_rect(Rect::new(x, y, 1, 1)), Color::BLACK)?;
            }
        }
    }
    Ok(())
}

// Beta 3's fill_tint uses an alternating pattern for every light monochrome tint.
fn stipple_ink(x: u16, y: u16, color: Color) -> bool {
    color.luma() < 128 || (x ^ y) & 1 == 0
}

#[cfg(test)]
mod stipple_tests {
    use super::*;

    #[test]
    fn faint_mono_separators_keep_ink_instead_of_rounding_to_white() {
        for y in 0..4 {
            let pixels: alloc::vec::Vec<_> = (0..64)
                .map(|x| stipple_ink(x, y, Color::gray(210)))
                .collect();
            assert_eq!(pixels.iter().filter(|ink| **ink).count(), 32, "row {y}");
            assert!(pixels.windows(2).all(|pair| pair[0] != pair[1]));
            assert!((0..64).all(|x| stipple_ink(x, y, Color::BLACK)));
        }
    }
}
