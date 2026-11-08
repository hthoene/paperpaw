//! Lists of headings and rows with notes, at their own heights, in pages.
use crate::{
    Messages, Page, Paint, Style, header,
    list::{self, backdrop, columns, ink},
    paint, text,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot,
    },
};

/// The lines a note takes at most.
pub const NOTE_LINES: usize = 2;

/// One row of an [`ItemList`]: a title, a value at the right, a note below.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowItem {
    /// Left text.
    pub title: String,
    /// Right text.
    pub value: String,
    /// Small text below, wrapped to [`NOTE_LINES`].
    pub note: String,
    /// A 3x3 picture of the screen with this zone (0..9) filled, at the left.
    pub zone: Option<u8>,
    /// A number: − and + buttons at the right (touch screens), each a tap target.
    pub stepper: bool,
    /// Shown faint, passed by the focus and not chosen by a tap.
    pub disabled: bool,
}

/// An entry of an [`ItemList`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A heading over the rows that follow; not selectable.
    Heading(String),
    /// A row.
    Row(RowItem),
}

impl Item {
    /// A heading.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the text cannot be copied.
    pub fn heading(title: &str) -> Result<Self, Error> {
        Ok(Self::Heading(text::copy(title)?))
    }

    /// A row of title and value.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the texts cannot be copied.
    pub fn row(title: &str, value: &str) -> Result<Self, Error> {
        Self::noted(title, value, "")
    }

    /// A row with a note below.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the texts cannot be copied.
    pub fn noted(title: &str, value: &str, note: &str) -> Result<Self, Error> {
        Ok(Self::Row(RowItem {
            title: text::copy(title)?,
            value: text::copy(value)?,
            note: text::copy(note)?,
            zone: None,
            stepper: false,
            disabled: false,
        }))
    }

    /// Whether focus can rest on it.
    #[must_use]
    pub const fn selectable(&self) -> bool {
        matches!(self, Self::Row(row) if !row.disabled)
    }
}

/// What an input meant to an item list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemStep {
    /// Not for the list.
    Ignored,
    /// The focus should go to this item.
    Focus(usize),
    /// This item was chosen.
    Chosen(usize),
    /// This item was held: offer what else can be done with it.
    Held(usize),
    /// The header's action should run.
    Header(&'static str),
    /// The − (-1) or + (1) of this stepper row was tapped.
    Step(usize, i8),
}

/// What a list shows besides its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Footer<'a> {
    /// Nothing.
    #[default]
    None,
    /// A note at the bottom of the screen under a light line (what a tap or a
    /// hold does there), as high as its words need, at least a row and a half.
    Note(&'a str),
    /// What a list with nothing in it says, centred below its rows.
    Empty {
        /// Said larger.
        title: &'a str,
        /// Said in small letters, wrapped.
        text: &'a str,
    },
}

/// A paged list of [`Item`]s. Only scalar geometry survives a draw.
#[derive(Debug, Default)]
pub struct ItemList {
    pub(crate) items: Vec<Item>,
    pub(crate) focus: usize,
    /// First item of each page, from the last frame.
    pub(crate) starts: Vec<usize>,
    pub(crate) hits: Vec<(Rect, usize)>,
    /// The − and + buttons of the stepper rows of the last frame.
    pub(crate) steps: Vec<(Rect, Rect, usize)>,
    pub(crate) header: Page,
}

impl ItemList {
    /// A list with the focus on its first row.
    #[must_use]
    pub fn new(items: Vec<Item>) -> Self {
        let mut list = Self {
            items,
            ..Self::default()
        };
        list.focus = list.nearest(0);
        list
    }

    /// Replaces the items, keeping the focus where it was (on the nearest row).
    pub fn replace(&mut self, items: Vec<Item>) {
        self.items = items;
        self.focus = self.nearest(self.focus);
    }

    /// The items.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The focused item.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Focuses item `index`, or the nearest row.
    pub fn set_focus(&mut self, index: usize) {
        self.focus = self.nearest(index);
    }

    /// The selectable item at or before `index`, else the first after it.
    fn nearest(&self, index: usize) -> usize {
        let index = index.min(self.items.len().saturating_sub(1));
        (0..=index)
            .rev()
            .chain(index + 1..self.items.len())
            .find(|i| self.items.get(*i).is_some_and(Item::selectable))
            .unwrap_or(0)
    }

    fn after(&self, from: usize) -> Option<usize> {
        (from + 1..self.items.len()).find(|i| self.items[*i].selectable())
    }

    fn before(&self, from: usize) -> Option<usize> {
        (0..from).rev().find(|i| self.items[*i].selectable())
    }

    fn page_of(&self, item: usize) -> usize {
        self.starts
            .iter()
            .rposition(|start| *start <= item)
            .unwrap_or(0)
    }

    /// What `input` means, without doing anything about it. Page actions go to
    /// the first row of the next or previous page of the last frame.
    #[must_use]
    pub fn decide(&self, input: &Input<'_>) -> ItemStep {
        match *input {
            Input::Action { action, .. } => {
                let page = self.page_of(self.focus);
                let first_of = |page: usize| {
                    let start = self.starts.get(page).copied()?;
                    (start..self.items.len()).find(|i| self.items[*i].selectable())
                };
                let target = match action {
                    "core.confirm" if self.items.get(self.focus).is_some_and(Item::selectable) => {
                        return ItemStep::Chosen(self.focus);
                    }
                    "core.down" | "core.next" | "core.right" => self.after(self.focus),
                    "core.up" | "core.previous" | "core.left" => self.before(self.focus),
                    "core.next-page" => first_of(page + 1).or_else(|| self.after(self.focus)),
                    "core.prev-page" if page > 0 => first_of(page - 1),
                    "core.prev-page" => self.before(self.focus),
                    _ => return ItemStep::Ignored,
                };
                target.map_or(ItemStep::Focus(self.focus), ItemStep::Focus)
            }
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(position),
            Input::Gesture {
                position,
                kind: InputGestureKind::LongPress,
                ..
            } => match self.tap(position) {
                ItemStep::Chosen(item) => ItemStep::Held(item),
                _ => ItemStep::Ignored,
            },
            _ => ItemStep::Ignored,
        }
    }

    /// Where item `index` was drawn in the last frame; `None` off the page.
    #[must_use]
    pub fn rect_of(&self, index: usize) -> Option<Rect> {
        self.hits
            .iter()
            .find(|(_, item)| *item == index)
            .map(|(rect, _)| *rect)
    }

    fn tap(&self, position: Point) -> ItemStep {
        if let Some(action) = self.header.action(position) {
            return ItemStep::Header(action);
        }
        for (minus, plus, item) in &self.steps {
            if minus.contains(position) {
                return ItemStep::Step(*item, -1);
            }
            if plus.contains(position) {
                return ItemStep::Step(*item, 1);
            }
        }
        match self.hits.iter().find(|(rect, _)| rect.contains(position)) {
            // A faint row is taken, and does nothing.
            Some((_, item)) if !self.items[*item].selectable() => ItemStep::Focus(self.focus),
            Some((_, item)) => ItemStep::Chosen(*item),
            None => ItemStep::Ignored,
        }
    }

    /// Does what [`Self::decide`] says: moves the focus (with a redraw) or
    /// runs the header's action. A chosen row is only reported.
    ///
    /// # Errors
    /// Propagates command queueing errors; the focus is unchanged then.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<ItemStep, Error> {
        let step = self.decide(input);
        match step {
            ItemStep::Focus(to) => {
                if to != self.focus {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
                    self.focus = to;
                }
            }
            ItemStep::Chosen(item) | ItemStep::Held(item) | ItemStep::Step(item, _) => {
                self.focus = item;
            }
            ItemStep::Header(action) => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(action)?;
            }
            ItemStep::Ignored => {}
        }
        Ok(step)
    }

    /// `Handled::Yes` for what the list took.
    #[must_use]
    pub fn handled(step: ItemStep) -> Handled {
        if step == ItemStep::Ignored {
            Handled::No
        } else {
            Handled::Yes
        }
    }

    /// Draws the page the focus is on under `title`.
    ///
    /// # Errors
    /// Propagates allocation, font, catalog and drawing errors; the retained
    /// geometry stays that of the last completed frame.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        shown: bool,
    ) -> Result<(), Error> {
        self.draw_with(host, frame, title, shown, Footer::None)
    }

    /// As [`Self::draw`], with a [`Footer`].
    ///
    /// # Errors
    /// As [`Self::draw`].
    pub fn draw_with(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        shown: bool,
        footer: Footer<'_>,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let top = style.header.saturating_add(style.gap);
        let mut area = Rect::new(0, top, width, height.saturating_sub(top));
        let mut note = None;
        if let Footer::Note(text) = footer {
            let lines = text::wrap_with(
                |line| font.measure_text(style.small, line),
                text,
                f32::from(width.saturating_sub(2 * style.margin)).max(1.0),
                4,
            )?;
            let line = crate::geometry::line_height(&font, style.small)?;
            let notes =
                crate::geometry::px((line * lines.len() as f32).min(crate::geometry::MAX_PX))?;
            let high = (style.row.saturating_mul(3) / 2)
                .max(style.line.saturating_add(notes).saturating_add(1));
            area.height = area.height.saturating_sub(high);
            note = Some((
                Rect::new(0, height.saturating_sub(high), width, high),
                lines,
            ));
        }
        let mut heights = Vec::new();
        heights
            .try_reserve_exact(self.items.len())
            .map_err(|_| Error::NoMemory)?;
        for item in &self.items {
            heights.push(item_height(&font, &style, width, item)?);
        }
        let starts = paginate(&self.items, &heights, area.height)?;
        let page = starts
            .iter()
            .rposition(|start| *start <= self.focus)
            .unwrap_or(0);
        let mut header = Page::default();
        let mut messages = Messages::new()?;
        // A title that is a catalog key is translated; text already translated
        // (a group's name) is not a key and is shown as it is.
        let title = match messages.copy(host, title) {
            Err(Error::NotFound | Error::Invalid) => text::copy(title)?,
            other => other?,
        };
        header::prepare(
            &mut header,
            host,
            &font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            page,
            starts.len(),
        )?;
        let end = starts.get(page + 1).copied().unwrap_or(self.items.len());
        let mut body = Vec::new();
        paint::push(&mut body, Paint::Fill(area, style.paper))?;
        let mut hits = Vec::new();
        let mut steps = Vec::new();
        let mut y = top;
        for (index, &high) in heights.iter().enumerate().take(end).skip(starts[page]) {
            let rect = Rect::new(0, y, width, high);
            let focused = shown && index == self.focus;
            match &self.items[index] {
                Item::Heading(text) => heading(&mut body, &font, &style, rect, text)?,
                Item::Row(row) => {
                    if let Some((minus, plus)) =
                        draw_row(&mut body, &font, &style, rect, row, focused)?
                    {
                        steps.try_reserve(1).map_err(|_| Error::NoMemory)?;
                        steps.push((minus, plus, index));
                    }
                    hits.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    hits.push((rect, index));
                }
            }
            y = y.saturating_add(high);
        }
        if let Some((rect, lines)) = &note {
            draw_note(&mut body, &font, &style, *rect, lines)?;
        }
        if let Footer::Empty { title, text } = footer
            && starts.len() == 1
        {
            let area = Rect::new(0, y, width, height.saturating_sub(y));
            draw_empty(&mut body, &font, &style, area, title, text)?;
        }
        paint::replay(frame, &font, header.paint.iter().chain(body.iter()))?;
        font.release()?;
        header.paint = Vec::new();
        self.header = header;
        self.starts = starts;
        self.hits = hits;
        self.steps = steps;
        Ok(())
    }
}

fn draw_note(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    lines: &[String],
) -> Result<(), Error> {
    paint::push(paint, Paint::Fill(rect, style.paper))?;
    if style.separators != "core.separators.none" {
        let width = rect.width.saturating_sub(2 * style.margin);
        paint::push(
            paint,
            Paint::Fill(
                Rect::new(
                    rect.x.saturating_add(style.margin),
                    rect.y,
                    width,
                    style.line,
                ),
                style.rule,
            ),
        )?;
    }
    let line = crate::geometry::line_px(font, style.small)?;
    let mut y = rect.y.saturating_add(style.line);
    for text in lines {
        let area = Rect::new(rect.x, y, rect.width, line);
        text::label(
            paint,
            font,
            text,
            style.small,
            f32::from(style.margin),
            area,
            style.note,
            None,
        )?;
        y = y.saturating_add(line);
    }
    Ok(())
}

/// A title and a few lines, centred in `area`.
fn draw_empty(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    title: &str,
    text: &str,
) -> Result<(), Error> {
    let room = f32::from(area.width.saturating_sub(4 * style.margin)).max(1.0);
    let lines = text::wrap_with(|line| font.measure_text(style.small, line), text, room, 6)?;
    let head = crate::geometry::line_px(font, style.title)?;
    let line = crate::geometry::line_px(font, style.small)?;
    let total = crate::geometry::clamp_u16(
        i64::from(head) + i64::from(style.gap) + i64::from(line) * lines.len() as i64,
    );
    let mut y = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    let centred = |font: &Font<'_>, size: f32, text: &str| -> Result<f32, Error> {
        Ok((f32::from(area.width) - font.measure_text(size, text)?).max(0.0) / 2.0)
    };
    let fitted = text::fit(font, style.title, title, room)?;
    let x = centred(font, style.title, &fitted)?;
    text::label(
        paint,
        font,
        &fitted,
        style.title,
        x,
        Rect::new(0, y, area.width, head),
        style.ink,
        None,
    )?;
    y = y.saturating_add(head.saturating_add(style.gap));
    for text in &lines {
        let x = centred(font, style.small, text)?;
        text::label(
            paint,
            font,
            text,
            style.small,
            x,
            Rect::new(0, y, area.width, line),
            style.muted,
            None,
        )?;
        y = y.saturating_add(line);
    }
    Ok(())
}

/// Where each page starts: items are placed one after the other and a page
/// ends where the next does not fit. A heading never ends a page alone.
///
/// # Errors
/// [`Error::Invalid`] when `heights` and `items` differ in length;
/// [`Error::NoMemory`] when a start cannot be recorded. Nothing is returned
/// partially.
pub fn paginate(items: &[Item], heights: &[u16], room: u16) -> Result<Vec<usize>, Error> {
    if items.len() != heights.len() {
        return Err(Error::Invalid);
    }
    let mut starts = Vec::new();
    starts.try_reserve(1).map_err(|_| Error::NoMemory)?;
    starts.push(0);
    let mut used = 0u32;
    for (index, high) in heights.iter().enumerate() {
        let first = starts.last().copied().unwrap_or(0);
        if used + u32::from(*high) > u32::from(room) && index > first {
            let orphan = index > first + 1 && matches!(items[index - 1], Item::Heading(_));
            let start = if orphan { index - 1 } else { index };
            starts.try_reserve(1).map_err(|_| Error::NoMemory)?;
            starts.push(start);
            used = heights[start..index].iter().map(|h| u32::from(*h)).sum();
        }
        used += u32::from(*high);
    }
    Ok(starts)
}

pub(crate) fn item_height(
    font: &Font<'_>,
    style: &Style,
    width: u16,
    item: &Item,
) -> Result<u16, Error> {
    match item {
        Item::Heading(_) => Ok(style.heading),
        Item::Row(row) if row.note.is_empty() => Ok(style.row),
        Item::Row(row) => {
            let lines = note_lines(font, style, width, &row.note, row.zone.is_some())?;
            let line = crate::geometry::line_height(font, style.small)?;
            let notes =
                crate::geometry::px((line * lines.len() as f32).min(crate::geometry::MAX_PX))?;
            Ok(style
                .row
                .saturating_add(notes)
                .saturating_add(style.gap / 2))
        }
    }
}

fn note_room(style: &Style, width: u16, zone: bool) -> f32 {
    let lead = if zone { zone_room(style) } else { 0 };
    f32::from(width.saturating_sub(2 * style.margin + lead)).max(1.0)
}

fn zone_room(style: &Style) -> u16 {
    style.row.max(4) / 2 + style.margin
}

fn note_lines(
    font: &Font<'_>,
    style: &Style,
    width: u16,
    note: &str,
    zone: bool,
) -> Result<Vec<String>, Error> {
    text::wrap_with(
        |line| font.measure_text(style.small, line),
        note,
        note_room(style, width, zone),
        NOTE_LINES,
    )
}

pub(crate) fn heading(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    title: &str,
) -> Result<(), Error> {
    let line = crate::geometry::line_px(font, style.small)?;
    let area = Rect::new(
        rect.x,
        rect.y.saturating_add(
            rect.height
                .saturating_sub(line.saturating_add(style.gap / 2)),
        ),
        rect.width.saturating_sub(style.margin),
        line,
    );
    text::label(
        paint,
        font,
        title,
        style.small,
        f32::from(style.margin),
        area,
        style.muted,
        None,
    )
}

pub(crate) fn draw_row(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    row: &RowItem,
    focused: bool,
) -> Result<Option<(Rect, Rect)>, Error> {
    backdrop(paint, style, rect, focused)?;
    let ink = if row.disabled {
        style.disabled
    } else {
        ink(style, focused)
    };
    let mut band = Rect::new(rect.x, rect.y, rect.width, style.row.min(rect.height));
    let mut note_at = rect;
    if let Some(zone) = row.zone {
        let shift = zone_room(style).min(rect.width);
        zone_picture(paint, style, band, zone, ink)?;
        band = Rect::new(
            band.x.saturating_add(shift),
            band.y,
            band.width.saturating_sub(shift),
            band.height,
        );
        note_at = Rect::new(
            rect.x.saturating_add(shift),
            rect.y,
            rect.width.saturating_sub(shift),
            rect.height,
        );
    }
    let mut buttons = None;
    if row.stepper {
        // Square buttons at the right end of the row's first line; the value
        // sits to their left.
        let side = band.height.min(band.width / 4);
        let right = band
            .x
            .saturating_add(band.width)
            .saturating_sub(style.margin / 2);
        let plus = Rect::new(right.saturating_sub(side), band.y, side, band.height);
        let minus = Rect::new(plus.x.saturating_sub(side), band.y, side, band.height);
        for (rect, glyph) in [(minus, "\u{2212}"), (plus, "+")] {
            let size = style.text;
            let x =
                f32::from(rect.x) + (f32::from(rect.width) - font.measure_text(size, glyph)?) / 2.0;
            text::label(paint, font, glyph, size, x, rect, ink, Some(rect))?;
        }
        band.width = minus.x.saturating_sub(band.x);
        buttons = Some((minus, plus));
    }
    columns(paint, font, style, band, &row.title, &row.value, ink)?;
    if row.note.is_empty() {
        return Ok(buttons);
    }
    let color = if focused && !style.ring {
        style.paper
    } else {
        style.note
    };
    let lines = note_lines(font, style, rect.width, &row.note, row.zone.is_some())?;
    let line = crate::geometry::line_px(font, style.small)?;
    let mut y = band.y.saturating_add(band.height);
    for text in &lines {
        let area = Rect::new(note_at.x, y, note_at.width, line);
        text::label(
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
    }
    Ok(buttons)
}

/// A small 3x3 grid at the left of `rect` with cell `zone` filled: the part of
/// the screen a tap zone is.
fn zone_picture(
    paint: &mut Vec<Paint>,
    style: &Style,
    rect: Rect,
    zone: u8,
    ink: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let cell = (rect.height / 6).max(4);
    let side = cell * 3;
    let x = rect.x + style.margin;
    let y = rect.y + rect.height.saturating_sub(side) / 2;
    for i in 0..9u16 {
        let (cx, cy) = (x + (i % 3) * cell, y + (i / 3) * cell);
        if i == u16::from(zone) {
            paint::push(paint, Paint::Fill(Rect::new(cx, cy, cell, cell), ink))?;
        } else {
            let edges = [
                Rect::new(cx, cy, cell, 1),
                Rect::new(cx, cy + cell - 1, cell, 1),
                Rect::new(cx, cy, 1, cell),
                Rect::new(cx + cell - 1, cy, 1, cell),
            ];
            for edge in edges {
                paint::push(paint, Paint::Fill(edge, ink))?;
            }
        }
    }
    Ok(())
}

/// Silences the unused-import lint of `list` where only helpers are used.
const _: fn() = || {
    let _ = list::row;
};

#[cfg(test)]
mod tests {
    use super::*;

    fn items(spec: &str) -> Vec<Item> {
        spec.chars()
            .map(|c| {
                if c == 'h' {
                    Item::heading("h").unwrap()
                } else {
                    Item::row("r", "").unwrap()
                }
            })
            .collect()
    }

    fn action(action: &str) -> Input<'_> {
        Input::Action { now_ms: 0, action }
    }

    #[test]
    fn the_focus_starts_on_the_first_row_and_skips_headings() {
        let list = ItemList::new(items("hrrhr"));
        assert_eq!(list.focus(), 1);
        assert_eq!(list.decide(&action("core.down")), ItemStep::Focus(2));
        let mut list = list;
        list.set_focus(2);
        assert_eq!(list.decide(&action("core.down")), ItemStep::Focus(4));
        assert_eq!(list.decide(&action("core.up")), ItemStep::Focus(1));
        list.set_focus(4);
        assert_eq!(list.decide(&action("core.down")), ItemStep::Focus(4));
    }

    #[test]
    fn faint_rows_are_passed_by_the_focus_and_do_nothing_when_tapped() {
        let mut spec = items("rrr");
        if let Item::Row(row) = &mut spec[1] {
            row.disabled = true;
        }
        let mut list = ItemList::new(spec);
        assert_eq!(list.decide(&action("core.down")), ItemStep::Focus(2));
        list.set_focus(2);
        assert_eq!(list.decide(&action("core.up")), ItemStep::Focus(0));
        // The focus never rests on one.
        list.set_focus(1);
        assert_eq!(list.focus(), 0);
        list.hits = Vec::from([(Rect::new(0, 0, 10, 10), 1)]);
        list.focus = 2;
        assert_eq!(
            list.decide(&Input::Gesture {
                now_ms: 0,
                position: Point::new(1, 1),
                kind: InputGestureKind::Tap,
            }),
            ItemStep::Focus(2)
        );
    }

    #[test]
    fn replacing_keeps_the_focus_near_where_it_was() {
        let mut list = ItemList::new(items("hrrrr"));
        list.set_focus(4);
        list.replace(items("hrr"));
        assert_eq!(list.focus(), 2);
        list.replace(items("rrrh"));
        assert_eq!(list.focus(), 2);
        list.replace(Vec::new());
        assert_eq!(list.focus(), 0);
    }

    #[test]
    fn a_heading_never_ends_a_page_alone() {
        let list = items("rrhrr");
        // Three rows fit; the heading would be last of the first page.
        let starts = paginate(&list, &[10, 10, 10, 10, 10], 30).unwrap();
        assert_eq!(starts, [0, 2]);
        // A page that ends before the heading is left as it is.
        let starts = paginate(&list, &[10, 10, 10, 10, 10], 20).unwrap();
        assert_eq!(starts, [0, 2, 4]);
    }

    #[test]
    fn pagination_survives_every_refused_allocation() {
        let list = items("rrhrrrhrr");
        crate::fail_alloc::sweep(|| paginate(&list, &[10; 9], 20).map(|starts| starts.len()));
    }

    #[test]
    fn pages_follow_the_heights() {
        let list = items("rrrr");
        assert_eq!(paginate(&list, &[10, 25, 10, 10], 30).unwrap(), [0, 1, 2]);
        assert_eq!(paginate(&items("rr"), &[40, 40], 30).unwrap(), [0, 1]);
        assert_eq!(paginate(&list, &[40, 40], 30), Err(Error::Invalid));
        assert_eq!(paginate(&[], &[], 30).unwrap(), [0]);
    }

    #[test]
    fn page_actions_go_to_the_first_row_of_the_other_page() {
        let mut list = ItemList::new(items("hrrhrr"));
        list.starts = Vec::from([0, 3]);
        assert_eq!(list.decide(&action("core.next-page")), ItemStep::Focus(4));
        list.set_focus(5);
        assert_eq!(list.decide(&action("core.prev-page")), ItemStep::Focus(1));
        assert_eq!(list.decide(&action("core.confirm")), ItemStep::Chosen(5));
    }

    #[test]
    fn a_long_press_holds_the_row_and_never_the_header() {
        let mut list = ItemList::new(items("hr"));
        list.hits = Vec::from([(Rect::new(0, 40, 100, 40), 1)]);
        let press = |y| Input::Gesture {
            now_ms: 0,
            position: Point::new(5, y),
            kind: InputGestureKind::LongPress,
        };
        assert_eq!(list.decide(&press(45)), ItemStep::Held(1));
        assert_eq!(list.decide(&press(5)), ItemStep::Ignored);
    }

    #[test]
    fn taps_choose_rows_and_headings_are_not_hit() {
        let mut list = ItemList::new(items("hrr"));
        list.hits = Vec::from([
            (Rect::new(0, 40, 100, 40), 1),
            (Rect::new(0, 80, 100, 40), 2),
        ]);
        let tap = |y| Input::Gesture {
            now_ms: 0,
            position: Point::new(5, y),
            kind: InputGestureKind::Tap,
        };
        assert_eq!(list.decide(&tap(45)), ItemStep::Chosen(1));
        assert_eq!(list.decide(&tap(100)), ItemStep::Chosen(2));
        assert_eq!(list.decide(&tap(10)), ItemStep::Ignored);
    }
}
