//! Lists of headings and rows with notes, at their own heights, in pages.
use crate::{
    Messages, Page, Paint, Style, header,
    list::{self, backdrop, columns, ink},
    paint, text,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::canvas::PartRole;
use paperpaw_sdk::{
    Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::CommandSender,
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
    /// A switch: on or off (the row is told as a switch to whoever asks what
    /// is shown); `None` for a row that is no switch.
    pub on: Option<bool>,
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
            on: None,
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
    /// A QR code centred in the room below the rows (on a list of one page),
    /// with `caption` (an address to type, wrapped) under it. Drawn straight
    /// to the canvas, in runs: no operation is kept per module.
    Code {
        /// The code to draw, black on white with its quiet zone.
        code: &'a paperpaw_sdk::qr::QrCode,
        /// What is said under it.
        caption: &'a str,
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
    /// The first this many items carry a grip (a finger takes the row along).
    grips: usize,
    /// The grips of the rows of the last frame.
    pub(crate) grip_rects: Vec<(Rect, usize)>,
    /// The row a finger has taken along and the row it would land on.
    drag: Option<(usize, usize)>,
    /// Where the last frame's region began on the screen: the rows are drawn
    /// in screen pixels, a redraw area is asked for in the region's.
    origin: (i32, i32),
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

    /// Adds `item` at the end, keeping the focus where it was (on the first
    /// row, where the list had none). The list grows in place (a vector's
    /// amortized growth), so one that gains a row at a time is never built
    /// again from its rows.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the list cannot grow.
    pub fn push(&mut self, item: Item) -> Result<(), Error> {
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(item);
        if !self.items.get(self.focus).is_some_and(Item::selectable) {
            self.focus = self.nearest(self.focus);
        }
        Ok(())
    }

    /// Replaces the value (the text at the right) of the row at `index`, in
    /// place: for a number that moves (a download's progress) where the rest
    /// of the list stays as it is. `false` where `index` is no row.
    pub fn set_value(&mut self, index: usize, value: String) -> bool {
        match self.items.get_mut(index) {
            Some(Item::Row(row)) => {
                row.value = value;
                true
            }
            _ => false,
        }
    }

    /// Zeroes the text of every item and empties the list: for a list that
    /// showed a secret (a password drawn for the user to read) and is closed.
    pub fn wipe(&mut self) {
        for item in core::mem::take(&mut self.items) {
            match item {
                Item::Heading(title) => crate::mem::wipe_text(title),
                Item::Row(row) => {
                    crate::mem::wipe_text(row.title);
                    crate::mem::wipe_text(row.value);
                    crate::mem::wipe_text(row.note);
                }
            }
        }
        self.focus = 0;
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

    /// The first `movable` items carry a grip (≡) at their end, which a finger
    /// can take along ([`Self::grip_at`], [`Self::drop_row`]); none by default.
    pub fn set_grips(&mut self, movable: usize) {
        self.grips = movable;
    }

    /// The row a finger has taken along and the row it would land on (framed,
    /// and a bar where it lands), or `None`.
    pub fn set_drag(&mut self, drag: Option<(usize, usize)>) {
        self.drag = drag;
    }

    /// The item whose grip is at `position` in the last frame.
    #[must_use]
    pub fn grip_at(&self, position: Point) -> Option<usize> {
        self.grip_rects
            .iter()
            .find(|(rect, _)| rect.contains(position))
            .map(|(_, item)| *item)
    }

    /// The row an item dragged to height `y` would land on: the row of the
    /// last frame under `y`, kept to the rows that carry a grip (above them,
    /// the first; below them, the last). `None` without such a row.
    #[must_use]
    pub fn drop_row(&self, y: i32) -> Option<usize> {
        let mut last = None;
        for (rect, item) in &self.hits {
            if *item >= self.grips {
                continue;
            }
            last = Some(*item);
            if y < i32::from(rect.y) + i32::from(rect.height) {
                return last;
            }
        }
        last
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
                    self.redraw_focus(host, self.focus, to)?;
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

    /// Asks for the frame a focus step from row `from` to row `to` needs: the
    /// two rows alone where both are on the page drawn last, the screen where
    /// the step leaves the page or the host refuses an area.
    fn redraw_focus(&self, host: &Host<'_>, from: usize, to: usize) -> Result<(), Error> {
        let mut scratch = [0; 64];
        self.redraw_focus_on(&mut CommandSender::new(host, &mut scratch), from, to)
    }

    fn redraw_focus_on(
        &self,
        to_host: &mut impl crate::list::Redraws,
        from: usize,
        to: usize,
    ) -> Result<(), Error> {
        crate::list::redraw_step(to_host, self.origin, [self.rect_of(from), self.rect_of(to)])
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
        let mut style = Style::read(host, &display, &font)?;
        style.describing = paint::asking(frame);
        let (width, height) = (display.display_width(), display.display_height());
        let top = style.header.saturating_add(style.gap);
        let mut area = Rect::new(0, top, width, height.saturating_sub(top));
        let mut note = None;
        if let Footer::Note(text) = footer {
            let (rect, lines) = note_block(&font, &style, width, height, text)?;
            area.height = area.height.saturating_sub(rect.height);
            note = Some((rect, lines));
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
        // The header was the last to need the catalog: its buffers (4 KiB) go
        // before the rows are painted, the peak of drawing a list (#248).
        drop(messages);
        let end = starts.get(page + 1).copied().unwrap_or(self.items.len());
        let mut body = Vec::new();
        paint::push(&mut body, Paint::Fill(area, style.paper))?;
        let mut hits = Vec::new();
        let mut steps = Vec::new();
        let mut grip_rects = Vec::new();
        let mut y = top;
        for (index, &high) in heights.iter().enumerate().take(end).skip(starts[page]) {
            let rect = Rect::new(0, y, width, high);
            let focused = shown && index == self.focus;
            match &self.items[index] {
                Item::Heading(text) => heading(&mut body, &font, &style, rect, text)?,
                Item::Row(row) => {
                    let grip = (index < self.grips).then_some((&mut grip_rects, index));
                    if let Some((minus, plus)) =
                        draw_row_with_grip(&mut body, &font, &style, rect, row, focused, grip)?
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
        if let Some((from, to)) = self.drag {
            draw_drag(&mut body, &style, &hits, from, to)?;
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
        let below = Rect::new(0, y, width, height.saturating_sub(y));
        let code = code_footer(&mut body, &font, &style, footer, below, starts.len())?;
        paint::replay(frame, &font, header.paint.iter().chain(body.iter()))?;
        if let Some((qr, room)) = code {
            paint::draw_code(frame, qr, room)?;
        }
        font.release()?;
        header.paint = Vec::new();
        self.header = header;
        self.starts = starts;
        self.hits = hits;
        self.steps = steps;
        self.grip_rects = grip_rects;
        self.origin = (frame.rect.x, frame.rect.y);
        Ok(())
    }
}

/// Draws a row, with a grip at its end where `grip` (the list the grips are
/// noted in and the row's index) is given; the steppers' buttons it has.
fn draw_row_with_grip(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    row: &RowItem,
    focused: bool,
    grip: Option<(&mut Vec<(Rect, usize)>, usize)>,
) -> Result<Option<(Rect, Rect)>, Error> {
    let content = match grip {
        Some((grips, index)) => grip_row(paint, style, rect, focused, grips, index)?,
        None => rect,
    };
    draw_row(paint, font, style, content, row, focused)
}

/// The grip of a row in `rect`: as wide as the row is high, at its end (the left
/// where the interface runs right to left).
fn grip_strip(style: &Style, rect: Rect) -> Rect {
    let side = rect.height.min(rect.width);
    style.flip(
        rect,
        Rect::new(
            rect.x.saturating_add(rect.width.saturating_sub(side)),
            rect.y,
            side,
            rect.height,
        ),
    )
}

/// Draws the grip (three bars) of a row and notes it for [`ItemList::grip_at`];
/// what is left of the row for its content.
fn grip_row(
    paint: &mut Vec<Paint>,
    style: &Style,
    rect: Rect,
    focused: bool,
    grips: &mut Vec<(Rect, usize)>,
    index: usize,
) -> Result<Rect, Error> {
    let strip = grip_strip(style, rect);
    // The focus fills the whole row, under its content.
    backdrop(paint, style, rect, focused)?;
    let bar = style.line.max(2);
    let pitch = bar.saturating_mul(3);
    let width = (strip.width / 2).max(bar);
    let x = strip
        .x
        .saturating_add(strip.width.saturating_sub(width) / 2);
    let top = strip.y.saturating_add(
        strip
            .height
            .saturating_sub(bar.saturating_add(pitch.saturating_mul(2)))
            / 2,
    );
    let ink = ink(style, focused);
    for n in 0..3u16 {
        let y = top.saturating_add(n.saturating_mul(pitch));
        paint::push(paint, Paint::Fill(Rect::new(x, y, width, bar), ink))?;
    }
    grips.try_reserve(1).map_err(|_| Error::NoMemory)?;
    grips.push((strip, index));
    let side = strip.width.min(rect.width);
    let x = if style.rtl {
        rect.x.saturating_add(side)
    } else {
        rect.x
    };
    Ok(Rect::new(
        x,
        rect.y,
        rect.width.saturating_sub(side),
        rect.height,
    ))
}

/// A frame round the row taken along and a bar where it would land: below the
/// row it passes when moving down, above it when moving up.
fn draw_drag(
    paint: &mut Vec<Paint>,
    style: &Style,
    hits: &[(Rect, usize)],
    from: usize,
    to: usize,
) -> Result<(), Error> {
    let rect_of = |item: usize| hits.iter().find(|(_, i)| *i == item).map(|(r, _)| *r);
    if let Some(r) = rect_of(from) {
        let t = style.line.saturating_mul(2).max(2).min(r.height / 2);
        for edge in [
            Rect::new(r.x, r.y, r.width, t),
            Rect::new(
                r.x,
                r.y.saturating_add(r.height.saturating_sub(t)),
                r.width,
                t,
            ),
            Rect::new(r.x, r.y, t, r.height),
            Rect::new(
                r.x.saturating_add(r.width.saturating_sub(t)),
                r.y,
                t,
                r.height,
            ),
        ] {
            paint::push(paint, Paint::Fill(edge, style.ink))?;
        }
    }
    if to == from {
        return Ok(());
    }
    // Not on this page (it was turned meanwhile): no bar.
    let Some(r) = rect_of(to) else {
        return Ok(());
    };
    let thick = style.line.saturating_mul(4).max(4);
    let y = if to > from {
        r.y.saturating_add(r.height).saturating_sub(thick / 2)
    } else {
        r.y.saturating_sub(thick / 2)
    };
    let margin = style.margin;
    let bar = Rect::new(
        r.x.saturating_add(margin),
        y,
        r.width.saturating_sub(margin.saturating_mul(2)),
        thick,
    );
    paint::push(paint, Paint::Fill(bar, style.ink))
}

/// The wrapped lines of a footer note and the strip they take at the bottom.
fn note_block(
    font: &Font<'_>,
    style: &Style,
    width: u16,
    height: u16,
    text: &str,
) -> Result<(Rect, Vec<String>), Error> {
    let lines = text::wrap_with(
        |line| font.measure_text(style.small, line),
        text,
        f32::from(width.saturating_sub(2 * style.margin)).max(1.0),
        4,
    )?;
    let line = crate::geometry::line_height(font, style.small)?;
    let notes = crate::geometry::px((line * lines.len() as f32).min(crate::geometry::MAX_PX))?;
    let high =
        (style.row.saturating_mul(3) / 2).max(style.line.saturating_add(notes).saturating_add(1));
    Ok((
        Rect::new(0, height.saturating_sub(high), width, high),
        lines,
    ))
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
        text::label_in(
            paint,
            font,
            style.rtl,
            rect,
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
/// The caption of a code, centred at the bottom of `area` in small letters;
/// what is left above it for the code.
/// The caption of a [`Footer::Code`] on a list of one page, and the room left
/// for the code above it.
fn code_footer<'a>(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    footer: Footer<'a>,
    area: Rect,
    pages: usize,
) -> Result<Option<(&'a paperpaw_sdk::qr::QrCode, Rect)>, Error> {
    match footer {
        Footer::Code { code, caption } if pages == 1 => Ok(Some((
            code,
            draw_caption(paint, font, style, area, caption)?,
        ))),
        _ => Ok(None),
    }
}

fn draw_caption(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    caption: &str,
) -> Result<Rect, Error> {
    let room = f32::from(area.width.saturating_sub(2 * style.margin)).max(1.0);
    let lines = text::wrap_with(
        |line| font.measure_text(style.small, line),
        caption,
        room,
        3,
    )?;
    let line = crate::geometry::line_px(font, style.small)?;
    let high = crate::geometry::clamp_u16(i64::from(line) * lines.len() as i64);
    let mut y = area.y.saturating_add(area.height.saturating_sub(high));
    for text in &lines {
        let x = (f32::from(area.width) - font.measure_text(style.small, text)?).max(0.0) / 2.0;
        text::label(
            paint,
            font,
            text,
            style.small,
            x,
            Rect::new(0, y, area.width, line),
            style.ink,
            None,
        )?;
        y = y.saturating_add(line);
    }
    Ok(Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(high.saturating_add(style.gap)),
    ))
}

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
    paint::describe(
        paint,
        style.describing,
        PartRole::Heading,
        rect,
        false,
        false,
    )?;
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
    text::label_in(
        paint,
        font,
        style.rtl,
        rect,
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
    // Laid out left to right; where the interface runs right to left the
    // parts are where their mirror images are.
    band = style.flip(rect, band);
    note_at = style.flip(rect, note_at);
    let mut buttons = None;
    // Where the title and the value go: the row less the stepper's buttons.
    let mut cols = band;
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
        let room = band;
        // Mirrored as a whole where the interface runs right to left: the
        // buttons at the left end and the value to their right.
        let free = Rect::new(band.x, band.y, minus.x.saturating_sub(band.x), band.height);
        cols = style.flip(room, free);
        let (minus, plus) = (style.flip(room, minus), style.flip(room, plus));
        for (rect, glyph) in [(minus, "\u{2212}"), (plus, "+")] {
            paint::describe(
                paint,
                style.describing,
                PartRole::Button,
                rect,
                false,
                row.disabled,
            )?;
            let size = style.text;
            let x =
                f32::from(rect.x) + (f32::from(rect.width) - font.measure_text(size, glyph)?) / 2.0;
            text::label(paint, font, glyph, size, x, rect, ink, Some(rect))?;
        }
        buttons = Some((minus, plus));
    }
    columns(paint, font, style, cols, &row.title, &row.value, ink)?;
    if row.note.is_empty() {
        return Ok(buttons);
    }
    let color = if focused && !style.ring {
        style.on_bar()
    } else {
        style.note
    };
    let lines = note_lines(font, style, rect.width, &row.note, row.zone.is_some())?;
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
            paint::push(
                paint,
                Paint::Fill(style.flip(rect, Rect::new(cx, cy, cell, cell)), ink),
            )?;
        } else {
            let edges = [
                Rect::new(cx, cy, cell, 1),
                Rect::new(cx, cy + cell - 1, cell, 1),
                Rect::new(cx, cy, 1, cell),
                Rect::new(cx + cell - 1, cy, 1, cell),
            ];
            for edge in edges {
                paint::push(paint, Paint::Fill(style.flip(rect, edge), ink))?;
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
    use paperpaw_sdk::native::callbacks::FrameRect;

    /// What a focus step asked of the host, and the areas it refuses.
    #[derive(Default)]
    struct Asked {
        areas: Vec<FrameRect>,
        whole: usize,
        refuse_after: Option<usize>,
    }

    impl crate::list::Redraws for Asked {
        fn area(&mut self, area: FrameRect) -> Result<(), Error> {
            if self.refuse_after.is_some_and(|n| self.areas.len() >= n) {
                return Err(Error::Full);
            }
            self.areas.push(area);
            Ok(())
        }
        fn whole(&mut self) -> Result<(), Error> {
            self.whole += 1;
            Ok(())
        }
    }

    /// A list drawn with rows 1 to 3 of 61 px from y 75 on its page, in a
    /// region that began 10 px down the screen.
    fn drawn() -> ItemList {
        let mut list = ItemList::new(items("hrrrrr"));
        list.hits = (1..4)
            .map(|i| (Rect::new(0, 75 + 61 * (i as u16 - 1), 480, 61), i))
            .collect();
        list.origin = (0, 10);
        list
    }

    #[test]
    fn a_step_within_the_page_asks_for_its_two_rows() {
        let mut asked = Asked::default();
        drawn().redraw_focus_on(&mut asked, 1, 2).unwrap();
        assert_eq!(
            asked.areas,
            [
                FrameRect::new(0, 65, 480, 61),
                FrameRect::new(0, 126, 480, 61)
            ]
        );
        assert_eq!(asked.whole, 0);
    }

    #[test]
    fn a_step_off_the_page_or_a_refused_area_asks_for_the_screen() {
        // Row 4 is on the next page: the step turns it.
        let mut asked = Asked::default();
        drawn().redraw_focus_on(&mut asked, 3, 4).unwrap();
        assert_eq!((asked.areas.len(), asked.whole), (0, 1));
        // The host takes the first area and refuses the second.
        let mut asked = Asked {
            refuse_after: Some(1),
            ..Asked::default()
        };
        drawn().redraw_focus_on(&mut asked, 2, 3).unwrap();
        assert_eq!(asked.whole, 1);
        // It refuses the first already.
        let mut asked = Asked {
            refuse_after: Some(0),
            ..Asked::default()
        };
        drawn().redraw_focus_on(&mut asked, 2, 3).unwrap();
        assert_eq!((asked.areas.len(), asked.whole), (0, 1));
    }

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
    fn a_value_is_replaced_in_place_and_a_heading_has_none() {
        let mut list = ItemList::new(items("hrr"));
        list.set_focus(2);
        assert!(list.set_value(1, "42 %".into()));
        assert!(!list.set_value(0, "x".into()), "a heading has no value");
        assert!(!list.set_value(9, "x".into()));
        assert_eq!(list.focus(), 2);
        let Item::Row(row) = &list.items()[1] else {
            panic!("a row")
        };
        assert_eq!((row.title.as_str(), row.value.as_str()), ("r", "42 %"));
    }

    #[test]
    fn a_wiped_list_is_empty_and_its_focus_is_back_at_the_start() {
        let mut list = ItemList::new(items("hrrr"));
        list.set_focus(3);
        list.wipe();
        assert!(list.items().is_empty());
        assert_eq!(list.focus(), 0);
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
