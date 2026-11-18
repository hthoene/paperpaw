//! *Words saved*: how many words are kept, and below it the newest, and from
//! how many books and in how many languages.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Rect;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::layout::label;
use paperpaw_ui_kit::{Paint, geometry, text};

use crate::files::DataFolder;
use crate::native_widgets::{
    caption_room, captioned, draw_empty, number_if_narrow, pick_caption, read, text_size,
};
use crate::store::{self, Summary};

fn summary(host: &Host<'_>) -> Result<Summary, Error> {
    let data = DataStorage::new(host);
    read(store::summary(&DataFolder(&data)))
}

/// The notes below the count: the newest word, where the words come from (on
/// one line where it `fits`, else a line each).
fn notes(
    cx: &mut Cx<'_>,
    summary: &Summary,
    fits: impl Fn(&Cx<'_>, &str) -> Result<bool, Error>,
) -> Result<Vec<String>, Error> {
    let mut notes: Vec<String> = Vec::new();
    notes.try_reserve(3).map_err(|_| Error::NoMemory)?;
    if let Some(newest) = summary
        .newest
        .as_ref()
        .filter(|_| cx.settings.shows("newest").unwrap_or(false))
    {
        notes.push(cx.format(
            "vocabulary.widget.latest",
            &[Arg::Text {
                name: "word",
                value: &newest.word,
            }],
        )?);
    }
    let mut from: Vec<String> = Vec::new();
    from.try_reserve(2).map_err(|_| Error::NoMemory)?;
    if cx.settings.shows("books")? && summary.books > 0 {
        from.push(cx.plural("vocabulary.widget.books", summary.books as i64, &[])?);
    }
    if cx.settings.shows("languages")? && summary.languages > 0 {
        from.push(cx.plural("vocabulary.widget.languages", summary.languages as i64, &[])?);
    }
    let mut joined = String::new();
    for (n, part) in from.iter().enumerate() {
        joined = if n == 0 {
            text::copy(part)?
        } else {
            text::concat(&[&joined, " · ", part])?
        };
    }
    if fits(cx, &joined)? {
        notes.push(joined);
    } else {
        notes.extend(from);
    }
    Ok(notes)
}

/// The height the widget takes: the frame, the caption, the count and the
/// notes.
///
/// # Errors
/// Font and host refusals.
pub(crate) fn height(cx: &mut Cx<'_>) -> Result<u16, Error> {
    let summary = summary(cx.host)?;
    let size = cx.settings.size()?.text(cx.style());
    let small = geometry::line_px(&cx.font, cx.style().small)?;
    let notes = notes(cx, &summary, |_, _| Ok(true))?;
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(geometry::line_px(&cx.font, size)?)
        .saturating_add(small.saturating_mul(notes.len() as u16)))
}

/// Draws the widget in `area`: "128 words", and the notes below.
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
    let summary = summary(cx.host)?;
    if summary.count == 0 {
        return draw_empty(
            cx,
            out,
            &look,
            area,
            ("vocabulary.widget.empty", "vocabulary.widget.empty.short"),
        );
    }
    let inner = look.area;
    let small_px = cx.style().small;
    let small = geometry::line_px(&cx.font, small_px)?;
    let title = cx.tr("vocabulary.title")?;
    let caption = pick_caption(cx, inner, &title)?;
    let room = inner.height.saturating_sub(if caption.is_some() {
        caption_room(cx)?
    } else {
        0
    });
    let headline = cx.plural("vocabulary.widget.words", summary.count as i64, &[])?;
    let width = f32::from(inner.width);
    let fits = |cx: &Cx<'_>, text: &str| -> Result<bool, Error> {
        Ok(text.is_empty() || cx.font.measure_text(small_px, text)? <= width)
    };
    let mut notes = notes(cx, &summary, fits)?;
    let mut kept: Vec<String> = Vec::new();
    kept.try_reserve(notes.len()).map_err(|_| Error::NoMemory)?;
    for note in notes.drain(..) {
        if !note.is_empty() && fits(cx, &note)? {
            kept.push(note);
        }
    }
    let least = geometry::line_px(&cx.font, cx.style().text)?;
    while !kept.is_empty() && least.saturating_add(small.saturating_mul(kept.len() as u16)) > room {
        kept.pop();
    }
    let notes_h = small.saturating_mul(kept.len() as u16);
    let size = text_size(cx, &headline, inner.width, room.saturating_sub(notes_h))?;
    let big = geometry::line_px(&cx.font, size)?;
    let block = captioned(cx, out, &look, caption, big.saturating_add(notes_h))?;
    let align = cx.settings.align()?;
    let row = |y: u16, h: u16| Rect::new(inner.x, y, inner.width, h);
    // Too wide: the number alone ("128 words": "128").
    let shown = number_if_narrow(cx, size, &headline, inner.width)?;
    label(
        out,
        &cx.font,
        (size, align),
        row(block.y, big),
        shown,
        look.ink,
    )?;
    for (n, note) in kept.iter().enumerate() {
        let y = block
            .y
            .saturating_add(big)
            .saturating_add(small.saturating_mul(n as u16));
        label(
            out,
            &cx.font,
            (small_px, align),
            row(y, small),
            note,
            look.faint,
        )?;
    }
    Ok(())
}
