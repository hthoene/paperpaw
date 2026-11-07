//! A note: a text the reader writes ("If found, please return to …", a
//! reminder), small or large, with a heading of its own if wanted. A tap
//! edits it on the keyboard; so does its setting.
use paperpaw_sdk::{
    Rect,
    native::{Error, Host},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::line_px,
    widget::{
        Cx, Settings, TextSize,
        cx::face_font,
        layout::{break_words, caption, caption_height, label, wrap},
        text::largest_size,
    },
};

/// The longest note kept, in bytes: what the editor holds.
pub const TEXT_LIMIT: usize = crate::native_draw::TEXT_MAX;
/// The longest heading kept, in bytes.
const HEADING_LIMIT: usize = crate::native_draw::TEXT_MAX;

/// What the settings make drawn change, as a number: the note, its heading
/// and its font.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let settings = Settings::new(host, widget);
    let mut mark = 1;
    for (key, limit) in [
        ("text", TEXT_LIMIT),
        ("heading", HEADING_LIMIT),
        ("face", 96),
    ] {
        mark = crate::native_draw::fnv(mark, settings.string(key, limit)?.as_bytes());
    }
    Ok(mark)
}

/// The height of the heading with its gap; 0 without one.
fn heading_height(cx: &Cx<'_>, heading: &str) -> Result<u16, Error> {
    if heading.is_empty() {
        return Ok(0);
    }
    caption_height(&cx.font, cx.style().small, cx.style().gap)
}

/// The height: up to four lines of the note at the set size, the heading, the
/// frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let size = cx.settings.size()?.text(cx.style());
    let face = cx.settings.string("face", 96)?;
    let font = face_font(cx.host, &face)?;
    let frame = cx.shape.frame_height();
    let note = cx.settings.string("text", TEXT_LIMIT)?;
    let (lines, _) = break_words(
        &font,
        size,
        note.trim(),
        f32::from(width.saturating_sub(frame)),
    )?;
    let lines = u16::try_from(lines.len()).unwrap_or(4).clamp(1, 4);
    let heading = cx.settings.string("heading", HEADING_LIMIT)?;
    Ok(line_px(&cx.font, size)?
        .saturating_mul(lines)
        .saturating_add(heading_height(cx, heading.trim())?)
        .saturating_add(frame))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut alloc::vec::Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let note = cx.settings.string("text", TEXT_LIMIT)?;
    let note = note.trim();
    if note.is_empty() {
        let key = if cx.snapshot.has_touch() {
            "widgets.note.none.touch"
        } else {
            "widgets.note.none"
        };
        let message = cx.tr(key)?;
        if line_px(&cx.font, cx.style().small)? <= look.area.height {
            crate::native_draw::message_in(cx, out, &look, &message)?;
        }
        return Ok(());
    }
    let face = cx.settings.string("face", 96)?;
    let font = face_font(cx.host, &face)?;
    let (small, gap) = (cx.style().small, cx.style().gap);
    let align = cx.settings.align()?;
    let heading = cx.settings.string("heading", HEADING_LIMIT)?;
    let heading = heading.trim();
    // The heading where a line of the note fits below it.
    let least = line_px(&font, small)?;
    let mut area = look.area;
    if !heading.is_empty() && area.height >= heading_height(cx, heading)?.saturating_add(least) {
        area = caption(
            out,
            &cx.font,
            (small, gap),
            (area, align, look.faint),
            heading,
        )?;
    }
    let width = f32::from(area.width);
    let size = match cx.settings.size()? {
        TextSize::Fit => largest_size(cx.style().title * 1.8, small, |size| {
            // Broken between words only, every word whole on a line.
            let (lines, too_wide) = break_words(&font, size, note, width)?;
            Ok(!too_wide
                && lines.len() as f32 * f32::from(line_px(&font, size)?) <= f32::from(area.height))
        })?
        .unwrap_or(small),
        size => size.text(cx.style()),
    };
    let line = line_px(&font, size)?;
    // As many lines as fit; none where not even one has room.
    let fits = usize::from(area.height / line.max(1));
    if fits == 0 {
        return Ok(());
    }
    let lines = wrap(&font, size, note, width, fits)?;
    let count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let total = line.saturating_mul(count);
    let top = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    for (n, held) in lines.iter().enumerate() {
        let row = Rect::new(
            area.x,
            top.saturating_add(line.saturating_mul(u16::try_from(n).unwrap_or(0))),
            area.width,
            line,
        );
        label(out, &font, (size, align), row, held, look.ink)?;
    }
    Ok(())
}

// The most a text setting holds is 4084 bytes (the settings value codec): a
// note, a heading, a name or a title up to it is read whole, not refused as too
// long for a smaller guess.
const _: () = {
    assert!(crate::native_draw::TEXT_MAX >= 4084);
    assert!(TEXT_LIMIT == crate::native_draw::TEXT_MAX);
    assert!(HEADING_LIMIT == crate::native_draw::TEXT_MAX);
};
