//! Rows in section lists, with the same content in plain lists and cards.
use super::*;

pub(crate) fn draw_row(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    row: &RowItem,
    focused: bool,
) -> Result<Option<(Rect, Rect)>, Error> {
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
) -> Result<Option<(Rect, Rect)>, Error> {
    let own = looked(style, row);
    let style = own.as_ref().unwrap_or(style);
    if let Some(more) = cards.filter(|_| !row.field || focused) {
        crate::cards::backdrop(paint, style, rect, focused, more)?;
    } else {
        row_backdrop(paint, style, rect, row, focused)?;
    }
    paint::describe(
        paint,
        style.describing,
        PartRole::Row,
        rect,
        focused,
        row.disabled,
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
    if row.stepper {
        let (free, pair) = draw_stepper(paint, (font, style), band, row, ink)?;
        cols = free;
        buttons = Some(pair);
    }
    draw_title(paint, (font, style), cols, row, &title_block, ink)?;
    if row.note.is_empty() {
        return Ok(buttons);
    }
    draw_note(paint, font, style, (rect, band, note_at), row, focused)?;
    Ok(buttons)
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
    for text in &lines {
        let area = Rect::new(note_at.x, y, note_at.width, line);
        text::label_in(
            paint,
            font,
            style.rtl,
            note_at,
            text,
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
    pub(super) steps: Vec<(Rect, Rect, usize)>,
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
    (shown, cards): (bool, Option<crate::cards::Cards>),
    clip: (i32, i32),
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
                if let Some((minus, plus)) = draw_row_with_grip(
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
                    drawn.steps.push((minus, plus, index));
                }
                drawn.hits.try_reserve(1).map_err(|_| Error::NoMemory)?;
                drawn.hits.push((rect, index));
            }
        }
        drawn.y = drawn.y.saturating_add(high);
    }
    Ok(drawn)
}
