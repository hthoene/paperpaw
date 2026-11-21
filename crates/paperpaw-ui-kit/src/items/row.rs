//! Rows in section lists, with the same content in plain lists and cards.
use super::*;

pub(crate) fn draw_row(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    row: &RowItem,
    focused: bool,
) -> Result<Option<Buttons>, Error> {
    draw_row_as(paint, font, style, rect, row, focused, None)
}

pub(super) fn draw_row_as(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    row: &RowItem,
    focused: bool,
    cards: Option<bool>,
) -> Result<Option<Buttons>, Error> {
    let own = looked(style, row);
    let style = own.as_ref().unwrap_or(style);
    if let Some(more) = cards.filter(|_| !row.field || focused) {
        crate::cards::backdrop(paint, style, rect, focused, more)?;
    } else {
        row_backdrop(paint, style, rect, row, focused)?;
    }
    // A row of body text is never chosen: told as one that does nothing.
    paint::describe(
        paint,
        style.describing,
        PartRole::Row,
        rect,
        focused,
        row.disabled || row.plain,
    )?;
    if let Some(on) = row.on {
        paint::describe_switch(paint, style.describing, rect, on, row.disabled)?;
    }
    let ground = if focused && !style.ring {
        style.ink
    } else {
        style.paper
    };
    let ink = if row.disabled {
        disabled_ink(style)
    } else {
        ink(style, focused)
    };
    let (title_block, band_height) = title_lines(font, style, rect.width, row)?;
    let mut band = Rect::new(rect.x, rect.y, rect.width, band_height.min(rect.height));
    let mut note_at = rect;
    if let Some(zone) = row.zone {
        let shift = zone_room(style).min(rect.width);
        zone_picture(paint, style, band, zone, ink)?;
        (band, note_at) = shifted(rect, band, shift);
    } else if let Some(glyph) = row.icon {
        let shift = row_icon(paint, style, (rect, band), glyph, ink)?;
        (band, note_at) = shifted(rect, band, shift);
    }
    // Laid out left to right; where the interface runs right to left the
    // parts are where their mirror images are.
    band = style.flip(rect, band);
    note_at = style.flip(rect, note_at);
    let mut buttons = None;
    // Where the title and the value go: the row less the stepper's buttons.
    let mut cols = band;
    if let Some(on) = row.on {
        // The switch at the end of the row's first line; the title keeps the
        // room before it, and the value of such a row is not shown.
        let room = switch_room(style);
        switch_pill(paint, style, band, on, (ink, ground))?;
        cols = Rect::new(band.x, band.y, band.width.saturating_sub(room), band.height);
    }
    let mut noted = band;
    if row.stepper {
        let (free, (minus, plus)) = draw_stepper(paint, (font, style), band, row, ink)?;
        cols = free;
        let track = match row.track() {
            Some(track) => {
                // The track in the row under the buttons, and the note under it.
                noted.height = noted.height.saturating_add(style.row);
                Some(draw_track(paint, style, (rect, band), track, ink)?)
            }
            None => None,
        };
        buttons = Some((minus, plus, track));
    }
    let mut value = shown_value(row);
    if !value.is_empty() && value_below(font, style, rect.width, row)? {
        // The value on a line of its own under the title, at the right.
        let line = crate::geometry::line_px(font, style.text)?.min(cols.height);
        let strip = Rect::new(
            cols.x,
            cols.y.saturating_add(cols.height - line),
            cols.width,
            line,
        );
        cols.height -= line;
        columns(paint, font, style, strip, "", value, ink)?;
        value = "";
    }
    draw_title(paint, (font, style), cols, row, &title_block, ink, value)?;
    if row.note.is_empty() {
        return Ok(buttons);
    }
    draw_note(paint, font, style, (rect, noted, note_at), row, focused)?;
    Ok(buttons)
}

/// The minus, the plus and the track of a stepper row.
pub(super) type Buttons = (Rect, Rect, Option<Rect>);

/// Draws the track of a slider row in the row under `band`, across almost
/// the whole width of `rect`, and gives it.
fn draw_track(
    paint: &mut Vec<Paint>,
    style: &Style,
    (rect, band): (Rect, Rect),
    track: Track,
    ink: paperpaw_sdk::Color,
) -> Result<Rect, Error> {
    let low = Rect::new(
        rect.x,
        band.y.saturating_add(band.height),
        rect.width,
        band.height,
    );
    let place = low.inset(style.margin, band.height / 8);
    let radius = crate::slider::knob_radius(place);
    let knob = crate::slider::knob_x(place, track.value, track.range).clamp(0, i32::from(u16::MAX));
    let start = place.x.saturating_add(radius);
    let end = place.x.saturating_add(place.width).saturating_sub(radius);
    let line = Rect::new(start, place.y, end.saturating_sub(start), place.height);
    crate::parts::slider(paint, style, line, (knob as u16, radius), &[], ink)?;
    Ok(place)
}

fn draw_note(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    (rect, band, note_at): (Rect, Rect, Rect),
    row: &RowItem,
    focused: bool,
) -> Result<(), Error> {
    let color = if focused && !style.ring {
        style.on_bar()
    } else {
        style.note
    };
    let lines = note_lines(font, style, rect.width, row)?;
    let line = crate::geometry::line_px(font, style.small)?;
    let mut y = band.y.saturating_add(band.height);
    for text in lines {
        let area = Rect::new(note_at.x, y, note_at.width, line);
        if !style.rtl {
            // Wrapped to fit above: a second measure would find it fits.
            text::push_run(
                paint,
                font,
                text,
                style.small,
                f32::from(note_at.x) + f32::from(style.margin),
                area,
                color,
                Some(rect),
            )?;
            y = y.saturating_add(line);
            continue;
        }
        text::label_in(
            paint,
            font,
            style.rtl,
            note_at,
            &text,
            style.small,
            f32::from(note_at.x) + f32::from(style.margin),
            area,
            color,
            Some(rect),
        )?;
        y = y.saturating_add(line);
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct Drawn {
    pub(super) hits: Vec<(Rect, usize)>,
    pub(super) steps: Vec<Stepped>,
    pub(super) grips: Vec<(Rect, usize)>,
    pub(super) y: u16,
}

pub(super) fn paint_rows(
    list: &ItemList,
    body: &mut Vec<Paint>,
    (font, style): (&Font<'_>, &Style),
    heights: &[u16],
    range: core::ops::Range<usize>,
    area: Rect,
    (shown, cards, clip): (bool, Option<crate::cards::Cards>, (i32, i32)),
) -> Result<Drawn, Error> {
    let mut drawn = Drawn {
        y: area.y,
        ..Drawn::default()
    };
    let end = range.end;
    crate::cards::page(
        body,
        style,
        cards,
        (&list.items, heights),
        range.clone(),
        area,
    )?;
    for (index, &high) in heights.iter().enumerate().take(end).skip(range.start) {
        let rect = Rect::new(area.x, drawn.y, area.width, high);
        let focused = shown && index == list.focus;
        match &list.items[index] {
            Item::Heading(text) => heading(body, font, style, rect, text)?,
            // A row outside the band being drawn is not laid out (the words of
            // a note are measured one by one): only where it is, for the touch (#291).
            Item::Row(row)
                if !(row.stepper || index < list.grips)
                    && (i32::from(rect.y.saturating_add(high)) <= clip.0
                        || i32::from(rect.y) >= clip.1) =>
            {
                drawn.hits.try_reserve(1).map_err(|_| Error::NoMemory)?;
                drawn.hits.push((rect, index));
            }
            Item::Row(row) => {
                let grip = (index < list.grips).then_some((&mut drawn.grips, index));
                if let Some((minus, plus, track)) = draw_row_with_grip(
                    body,
                    font,
                    style,
                    rect,
                    row,
                    (
                        focused,
                        cards.map(|_| {
                            matches!(
                                list.items.get(index + 1).filter(|_| index + 1 < end),
                                Some(Item::Row(_))
                            )
                        }),
                    ),
                    grip,
                )? {
                    drawn.steps.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    drawn.steps.push((minus, plus, index, track));
                }
                drawn.hits.try_reserve(1).map_err(|_| Error::NoMemory)?;
                drawn.hits.push((rect, index));
            }
        }
        drawn.y = drawn.y.saturating_add(high);
    }
    Ok(drawn)
}

/// How many lines the note of a row takes (what [`note_lines`] gives, without
/// wrapping where one measure of the whole note says it): a note that fits a
/// line is one, and where at most two lines are shown, one that does not is
/// two. Wrapping measures a word at a time, about fifteen measures a row, and
/// the X4 spends about 3 ms on each (#291).
#[inline(never)]
pub(super) fn note_line_count(
    font: &Font<'_>,
    style: &Style,
    width: u16,
    row: &RowItem,
) -> Result<usize, Error> {
    let most = if row.whole {
        WHOLE_NOTE_LINES
    } else {
        NOTE_LINES
    };
    let plain = plain_note(&row.note);
    if plain && most == 2 && !row.note.is_empty() {
        let room = note_room(style, width, row.zone.is_some());
        let whole = font.measure_text(style.small, &row.note)?;
        return Ok(if whole <= room { 1 } else { 2 });
    }
    Ok(note_lines(font, style, width, row)?.len())
}

/// Whether wrapping `note` would only join its words with single spaces.
fn plain_note(note: &str) -> bool {
    !note.starts_with(' ')
        && !note.ends_with(' ')
        && !note.contains("  ")
        && !note.chars().any(|c| c.is_whitespace() && c != ' ')
}

#[inline(never)]
pub(super) fn note_lines(
    font: &Font<'_>,
    style: &Style,
    width: u16,
    row: &RowItem,
) -> Result<Vec<String>, Error> {
    let room = note_room(style, width, row.zone.is_some());
    // A note that fits a line is one measure, not the word-by-word wrap that
    // is drawn again for every band of a frame.
    if !row.note.is_empty()
        && plain_note(&row.note)
        && font.measure_text(style.small, &row.note)? <= room
    {
        let mut lines = Vec::new();
        lines.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
        lines.push(text::copy(&row.note)?);
        return Ok(lines);
    }
    text::wrap_with(
        |line| font.measure_text(style.small, line),
        &row.note,
        room,
        if row.whole {
            WHOLE_NOTE_LINES
        } else {
            NOTE_LINES
        },
    )
}

/// The lines of a row's title and the height of its first band: one line in
/// a row, or, where the row shows its text whole and the title is too long
/// for its column, the lines the title wraps to.
pub(super) fn title_lines(
    font: &Font<'_>,
    style: &Style,
    width: u16,
    row: &RowItem,
) -> Result<(Vec<String>, u16), Error> {
    let mut lines = Vec::new();
    if row.whole && !row.stepper && !row.title.is_empty() {
        // Text a page reads out takes both margins; any other row also the gap
        // before its value.
        let margins = if page_text(row) && row.value.is_empty() {
            2.0
        } else {
            3.0
        };
        let room = (f32::from(width) - margins * f32::from(style.margin)).max(1.0);
        let room = if row.icon.is_some() {
            (room - f32::from(icon_room(style))).max(1.0)
        } else {
            room
        };
        let value = font.measure_text(style.text, &row.value)?.min(room * 0.4);
        let title_room = (room - value).max(1.0);
        let most = title_most(row);
        lines = wrap_title(
            |line| font.measure_text(style.text, line),
            &row.title,
            title_room,
            most,
        )?;
    }
    // The value on a line of its own takes one more.
    let below = if value_below(font, style, width, row)? {
        crate::geometry::line_px(font, style.text)?
    } else {
        0
    };
    if lines.len() < 2 {
        return Ok((Vec::new(), style.row.saturating_add(below)));
    }
    let line = crate::geometry::line_px(font, style.text)?;
    let block = line.saturating_mul(lines.len() as u16);
    let band = style.row.max(block.saturating_add(style.gap));
    Ok((lines, band.saturating_add(below)))
}

/// Whether the value of a row that asks for it is too long for the line of
/// its title, and goes to a line of its own.
pub(super) fn value_below(
    font: &Font<'_>,
    style: &Style,
    width: u16,
    row: &RowItem,
) -> Result<bool, Error> {
    if !row.wrap_value || row.stepper || row.on.is_some() || row.value.is_empty() {
        return Ok(false);
    }
    let lead = if row.icon.is_some() {
        icon_room(style)
    } else if row.zone.is_some() {
        zone_room(style)
    } else {
        0
    };
    let room = (f32::from(width.saturating_sub(lead)) - 3.0 * f32::from(style.margin)).max(1.0);
    let title = font.measure_text(style.text, &row.title)?.min(room * 0.6);
    Ok(font.measure_text(style.text, &row.value)? > room - title)
}
