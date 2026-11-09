//! *Today's reading cat* as the sleep screen: the cat of today's mood large in
//! the middle on paper, the minutes read and the goal below it. Where there is
//! no picture (yet) the mood is put in words above the minutes, as the widget
//! does.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{
        Error,
        callbacks::DrawFrame,
        image::{ImageId, ImageStyle},
    },
};
use paperpaw_ui_kit::widget::{Align, Cx, layout::label};
use paperpaw_ui_kit::{Paint, geometry, mem::push, paint};

use crate::model::Today;
use crate::native_widget::{mood_key, progress};

/// The room of the cat and of the lines below it, in the middle of `area`.
/// `line` is the height of a line of text, `gap` the space between the cat and
/// the line.
pub(crate) fn layout(area: Rect, margin: u16, gap: u16, line: u16, picture: bool) -> Layout {
    let room_w = area.width.saturating_sub(margin.saturating_mul(2));
    let room_h = if picture {
        area.height
            .saturating_sub(margin.saturating_mul(2))
            .saturating_sub(gap)
            .saturating_sub(line)
            .min(room_w)
    } else {
        0
    };
    let used = room_h.saturating_add(gap).saturating_add(line);
    let top = area.y.saturating_add(area.height.saturating_sub(used) / 2);
    let x = area.x.saturating_add(margin);
    Layout {
        cat: Rect::new(x, top, room_w, room_h),
        words: Rect::new(x, top.saturating_sub(line), room_w, line),
        minutes: Rect::new(
            x,
            top.saturating_add(room_h).saturating_add(gap),
            room_w,
            line,
        ),
    }
}

/// Where the parts of the screen go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    pub cat: Rect,
    pub words: Rect,
    pub minutes: Rect,
}

/// Draws the screen into `frame`.
///
/// # Errors
/// Font, settings, catalog and drawing refusals; [`Error::NoMemory`].
pub(crate) fn draw(
    cx: &mut Cx<'_>,
    frame: &mut DrawFrame<'_>,
    today: &Today,
    picture: Option<ImageId>,
) -> Result<(), Error> {
    let area = Rect::new(
        geometry::clamp_u16(i64::from(frame.rect.x)),
        geometry::clamp_u16(i64::from(frame.rect.y)),
        geometry::clamp_u16(i64::from(frame.rect.width)),
        geometry::clamp_u16(i64::from(frame.rect.height)),
    );
    let style = cx.style();
    let (paper, ink, size, margin) = (style.paper, style.ink, style.text, style.margin);
    let gap = style.gap.saturating_mul(3);
    let line = geometry::line_px(&cx.font, size)?;
    let at = layout(area, margin, gap, line, picture.is_some());
    let mut out: Vec<Paint> = Vec::new();
    push(&mut out, Paint::Fill(area, paper))?;
    if picture.is_none() {
        let words = cx.tr(mood_key(today.mood))?;
        label(
            &mut out,
            &cx.font,
            (size, Align::Center),
            at.words,
            &words,
            ink,
        )?;
    }
    let text = progress(cx, today)?;
    label(
        &mut out,
        &cx.font,
        (size, Align::Center),
        at.minutes,
        &text,
        ink,
    )?;
    paint::replay(frame, &cx.font, out.iter())?;
    if let Some(image) = picture {
        frame
            .draw_canvas()?
            .draw_image(image, paint::frame_rect(at.cat), ImageStyle::new())
            .ok();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
