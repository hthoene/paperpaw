//! Prepared drawing: owned operations laid out first, replayed in the draw callback.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error,
        callbacks::{DrawFrame, FrameRect},
        canvas::{PartFlags, PartRole},
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
    let mut canvas = frame.draw_canvas()?;
    // Whether anybody asks what is drawn; once not, no more is told.
    let mut asked = true;
    for op in ops {
        match op {
            Paint::Describe { role, flags, rect } => {
                if asked {
                    // A part the host refuses (an empty area) is left out;
                    // what is drawn does not depend on it.
                    asked = canvas
                        .describe_part(*role, "", frame_rect(*rect), *flags)
                        .unwrap_or(true);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::{Paint, bold_extra, describe};
    use alloc::vec::Vec;
    use paperpaw_sdk::{
        Rect,
        native::canvas::{PartFlags, PartRole},
    };

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
