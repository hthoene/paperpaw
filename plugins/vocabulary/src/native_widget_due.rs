//! *Cards due*: how many flash cards are due now, new ones to show today
//! counted; without decks, a word on how to make some.

use alloc::vec::Vec;

use paperpaw_sdk::Rect;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::Paint;
use paperpaw_ui_kit::geometry;
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::layout::label;

use crate::cards;
use crate::files::DataFolder;
use crate::native_review::due_now;
use crate::native_review_screen::{HostTime, settings};
use crate::native_widgets::{
    caption_room, captioned, draw_empty, number_if_narrow, pick_caption, read, text_size,
};

/// How many cards are due; `None` without decks.
///
/// # Errors
/// Host refusals and [`Error::NoMemory`].
pub(crate) fn due(host: &Host<'_>) -> Result<Option<usize>, Error> {
    let data = DataStorage::new(host);
    let folder = DataFolder(&data);
    if read(cards::decks(&folder))?.is_empty() {
        return Ok(None);
    }
    let time = HostTime::read(host, &folder)?;
    Ok(Some(due_now(&folder, &time, settings(host)?.new_per_day)))
}

/// The height the widget takes: the frame, the caption and a line of numbers.
///
/// # Errors
/// Font and host refusals.
pub(crate) fn height(cx: &mut Cx<'_>) -> Result<u16, Error> {
    let size = cx.settings.size()?.text(cx.style());
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(geometry::line_px(&cx.font, size)?))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub(crate) fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    _seed: u32,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let Some(count) = due(cx.host)? else {
        return draw_empty(
            cx,
            out,
            &look,
            area,
            (
                "vocabulary.widget.due.empty",
                "vocabulary.widget.due.empty.short",
            ),
        );
    };
    let inner = look.area;
    let title = cx.tr("vocabulary.cards.title")?;
    let caption = pick_caption(cx, inner, &title)?;
    let room = inner.height.saturating_sub(if caption.is_some() {
        caption_room(cx)?
    } else {
        0
    });
    let headline = cx.plural("vocabulary.widget.due.count", count as i64, &[])?;
    let size = text_size(cx, &headline, inner.width, room)?;
    // Too wide: the number alone.
    let shown = number_if_narrow(cx, size, &headline, inner.width)?;
    let big = geometry::line_px(&cx.font, size)?;
    let block = captioned(cx, out, &look, caption, big)?;
    let align = cx.settings.align()?;
    label(
        out,
        &cx.font,
        (size, align),
        Rect::new(inner.x, block.y, inner.width, big),
        shown,
        look.ink,
    )
}
