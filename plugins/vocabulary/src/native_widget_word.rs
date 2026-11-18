//! *Word of the day*: a word to go over, another one each day (or the newest;
//! without a clock, another one for each time the widget is made), with its
//! sentence and its book below it.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Rect;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::snapshot::capture_snapshot;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::layout::label;
use paperpaw_ui_kit::widget::text::Wrapped;
use paperpaw_ui_kit::{Paint, geometry};

use crate::declarations::PICKS;
use crate::files::DataFolder;
use crate::native_widgets::{caption_room, captioned, draw_empty, pick_caption, read, text_size};
use crate::store::{self, Word};

/// The day it is where the reader is, counted from 1970; `None` where the
/// device does not know the time.
///
/// # Errors
/// Host refusals.
pub(crate) fn today_of(host: &Host<'_>) -> Result<Option<i64>, Error> {
    let snapshot = capture_snapshot(host)?;
    Ok(snapshot.unix_seconds().map(|unix| {
        let offset = snapshot.utc_offset_minutes().map_or(0, i64::from) * 60;
        (unix + offset).div_euclid(86_400)
    }))
}

/// The word to show: another one every day, or the newest. Where the device
/// does not know the day, the one the instance's `seed` picks.
fn pick(cx: &Cx<'_>, seed: u32) -> Result<Option<Word>, Error> {
    let data = DataStorage::new(cx.host);
    let folder = DataFolder(&data);
    if cx.settings.with_text("pick", |v| v == PICKS[1])? {
        return Ok(read(store::summary(&folder))?.newest);
    }
    let count = read(store::count(&folder))?;
    if count == 0 {
        return Ok(None);
    }
    let n = match today_of(cx.host)? {
        Some(day) => day.rem_euclid(count as i64) as usize,
        None => seed as usize % count,
    };
    let words = read(store::range(&folder, n, n + 1))?;
    Ok(words.into_iter().next().map(|(_, word)| word))
}

/// The line a text of `size` takes.
fn line(cx: &Cx<'_>, size: f32) -> Result<u16, Error> {
    geometry::line_px(&cx.font, size)
}

/// The height the widget takes: the frame, the caption, the word, two lines of
/// its sentence and its book, as far as they are shown.
///
/// # Errors
/// Font and host refusals.
pub(crate) fn height(cx: &mut Cx<'_>) -> Result<u16, Error> {
    let style = cx.style();
    let small = line(cx, style.small)?;
    let size = cx.settings.size()?.text(style);
    let sentence = if cx.settings.shows("sentence")? {
        small.saturating_mul(2)
    } else {
        0
    };
    let book = if cx.settings.shows("book")? { small } else { 0 };
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(line(cx, size)?)
        .saturating_add(sentence)
        .saturating_add(book))
}

/// The caption: "Word of the day", or "Newest word".
fn caption_text(cx: &mut Cx<'_>) -> Result<String, Error> {
    if cx.settings.with_text("pick", |v| v == PICKS[1])? {
        cx.tr("vocabulary.widget.newest")
    } else {
        cx.tr("vocabulary.widget.word")
    }
}

/// Draws the widget in `area`: the caption, the word, its sentence (as many
/// lines as fit, three at most) and its book.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub(crate) fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    seed: u32,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let Some(word) = pick(cx, seed)? else {
        return draw_empty(
            cx,
            out,
            &look,
            area,
            ("vocabulary.widget.empty", "vocabulary.widget.empty.short"),
        );
    };
    let inner = look.area;
    let small = line(cx, cx.style().small)?;
    let text = caption_text(cx)?;
    let caption = pick_caption(cx, inner, &text)?;
    let cap = if caption.is_some() {
        caption_room(cx)?
    } else {
        0
    };
    let room = inner.height.saturating_sub(cap);
    let book = if cx.settings.shows("book")? && !word.title.is_empty() {
        small
    } else {
        0
    };
    let size = text_size(cx, &word.word, inner.width, room.saturating_sub(book))?;
    let big = line(cx, size)?;
    // The sentence on as many lines as it needs and fit, three at most.
    let fit = (room.saturating_sub(big.saturating_add(book)) / small.max(1)).min(3);
    let sentence = if cx.settings.shows("sentence")? && !word.sentence.is_empty() && fit > 0 {
        Some(Wrapped::new(
            &cx.font,
            &word.sentence,
            cx.style().small,
            (inner.width, fit.saturating_mul(small)),
            false,
        )?)
    } else {
        None
    };
    let sentence_h = match &sentence {
        Some(sentence) => sentence.height(&cx.font)?,
        None => 0,
    };
    let block = captioned(
        cx,
        out,
        &look,
        caption,
        big.saturating_add(sentence_h).saturating_add(book),
    )?;
    let align = cx.settings.align()?;
    let row = |y: u16, h: u16| Rect::new(inner.x, y, inner.width, h);
    label(
        out,
        &cx.font,
        (size, align),
        row(block.y, big),
        &word.word,
        look.ink,
    )?;
    let y = block.y.saturating_add(big);
    if let Some(sentence) = &sentence {
        sentence.draw(out, &cx.font, row(y, sentence_h), align, look.faint)?;
    }
    if book > 0 {
        let from = cx.format(
            "vocabulary.widget.from",
            &[Arg::Text {
                name: "title",
                value: &word.title,
            }],
        )?;
        let small_px = cx.style().small;
        label(
            out,
            &cx.font,
            (small_px, align),
            row(y.saturating_add(sentence_h), small),
            &from,
            look.faint,
        )?;
    }
    Ok(())
}
