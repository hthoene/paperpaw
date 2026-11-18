//! The widget *Front light*: the level as a number and a bar, and a hint what a
//! tap does where there is room. It draws from the level alone, in whatever
//! room its place has (pixels, not cells).
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Rect,
    native::{Error, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::{clamp_u16, line_px},
    paint,
    widget::{Cx, text::line as one_line},
};

/// How high the widget is in a place: three lines of small text and its frame.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &Cx<'_>) -> Result<u16, Error> {
    let line = line_px(&cx.font, cx.style().small)?;
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(line.saturating_mul(3)))
}

/// What the widget shows: the front light's level in percent, `None` where the
/// device has no front light to read.
pub type Level = Option<i64>;

/// Draws the widget in `area`.
///
/// # Errors
/// Font refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, level: Level) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let (text_px, small) = (cx.style().text, cx.style().small);
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    let label = match level {
        None => cx.tr("edge-light.widget.none")?,
        Some(0) => cx.tr("edge-light.widget.off")?,
        Some(percent) => cx.format(
            "edge-light.widget.level",
            &[Arg::Int {
                name: "level",
                value: percent,
            }],
        )?,
    };
    // A bar under the number where there is room for it.
    let bar_room = level.is_some() && inner.height >= line.saturating_mul(2);
    let bar_height = if bar_room { (line / 2).max(4) } else { 0 };
    let main = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(bar_height),
    );
    one_line(
        out,
        &cx.font,
        main,
        align,
        (text_px, small),
        &label,
        look.ink,
    )?;
    if bar_room {
        let band = Rect::new(
            inner.x,
            inner.y.saturating_add(main.height),
            inner.width,
            bar_height,
        );
        bar(out, band, level.unwrap_or(0), look.ink)?;
    }
    Ok(())
}

/// An outline filled from the left, `percent` of it.
fn bar(out: &mut Vec<Paint>, area: Rect, percent: i64, ink: Color) -> Result<(), Error> {
    if area.width < 8 || area.height < 4 {
        return Ok(());
    }
    paint::push(
        out,
        Paint::Focus(area, u32::from(area.height / 2), Some(2), ink),
    )?;
    let inner = Rect::new(
        area.x.saturating_add(3),
        area.y.saturating_add(3),
        area.width.saturating_sub(6),
        area.height.saturating_sub(6),
    );
    let filled = i64::from(inner.width) * percent.clamp(0, 100) / 100;
    if filled > 0 && inner.height > 0 {
        paint::push(
            out,
            Paint::Focus(
                Rect::new(inner.x, inner.y, clamp_u16(filled), inner.height),
                u32::from(inner.height / 2),
                None,
                ink,
            ),
        )?;
    }
    Ok(())
}
