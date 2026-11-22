//! Lists of headings and rows with notes, at their own heights, in pages.
use crate::{
    Messages, Page, Paint, Style, header,
    list::{self, backdrop, columns, ink, marked_columns},
    paint, text,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{canvas::PartRole, fonts::TextOrigin};
use paperpaw_sdk::{
    Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot,
    },
};

mod band;
mod changed;
mod empty;
mod extra;
mod grip;
mod head;
mod pages;
mod row;
pub(crate) use row::draw_row;
mod stepper;
mod track;
pub use changed::{RowKey, RowsChange};
pub use empty::Progress;
pub(crate) use empty::Spot;
use empty::{draw_empty, draw_progress};
pub use extra::RowExtra;
use grip::{draw_drag, draw_row_with_grip};
use head::Head;
use row::{note_line_count, title_lines};
use stepper::{draw_stepper, stepper_width};
use track::Stepped;
pub use track::Track;

/// The lines a note takes at most.
pub const NOTE_LINES: usize = 2;
/// The lines a note takes at most in a row that shows its text whole.
pub const WHOLE_NOTE_LINES: usize = 4;
/// The lines a title takes at most in a row that shows its text whole.
pub const WHOLE_TITLE_LINES: usize = 2;
/// The lines the title of a row of body text (`plain`, not `small`) takes at
/// most where it shows its text whole (what a plugin's page says it does).
pub const WHOLE_BODY_LINES: usize = 4;

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
    /// A named icon at the left (what [`crate::icons::glyph`] found for it);
    /// `None` for a row without one, or where no icon font is bound.
    pub icon: Option<paperpaw_sdk::native::fonts::IconGlyph>,
    /// A number: round − and + buttons at the right (touch screens), each a tap
    /// target, with the value tight between them.
    pub stepper: bool,
    /// What few rows have, behind one thin pointer (a field of every row of
    /// every list is a field of every plugin's frame, so it is as small as it
    /// can be): a slider's [`Track`] and the bold part of the title. See
    /// [`RowItem::track`] and [`RowItem::bold`].
    pub extra: Option<alloc::boxed::Box<[RowExtra; 1]>>,
    /// Shown faint, passed by the focus and not chosen by a tap.
    pub disabled: bool,
    /// A switch: on or off (the row is told as a switch to whoever asks what
    /// is shown); `None` for a row that is no switch.
    pub on: Option<bool>,
    /// An input field's look: an outline round the row when it has no focus
    /// (the settings' search), where other rows have a separator.
    pub field: bool,
    /// Its text is shown whole where it is long: the title wrapped to
    /// [`WHOLE_TITLE_LINES`] ([`WHOLE_BODY_LINES`] in a `plain` row) and the note
    /// to [`WHOLE_NOTE_LINES`], not cut with "…".
    pub whole: bool,
    /// A line of what a page says, not a row to choose: its title and icon in
    /// the small size, in a row as high as its lines.
    pub small: bool,
    /// Text the page reads out, not a row to choose: passed by the focus and
    /// not chosen by a tap, but in the full ink, not faint like `disabled`.
    pub plain: bool,
    /// Its title and value are kept whole, as Beta 3's settings kept them: the
    /// title wrapped beside the value (or the switch) to [`WHOLE_TITLE_LINES`],
    /// and a value too long for that on a line of its own under it, at the
    /// right, not cut with "…".
    pub wrap_value: bool,
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
            icon: None,
            stepper: false,
            extra: None,
            disabled: false,
            on: None,
            field: false,
            whole: false,
            small: false,
            plain: false,
            wrap_value: false,
        }))
    }

    /// A row of title and value with the bytes `bold` of the title drawn bold
    /// (the word found in a search hit's snippet).
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the texts cannot be copied.
    pub fn row_marked(title: &str, value: &str, bold: (u16, u16)) -> Result<Self, Error> {
        let mut row = match Self::row(title, value)? {
            Self::Row(row) => row,
            Self::Heading(_) => RowItem::default(),
        };
        row.extra_mut()?.bold = Some(bold);
        Ok(Self::Row(row))
    }

    /// This item with its title and value kept whole (see
    /// [`RowItem::wrap_value`]): a long title wrapped, a long value under it,
    /// not cut.
    #[must_use]
    pub fn with_wrapped_value(mut self) -> Self {
        if let Self::Row(row) = &mut self {
            row.wrap_value = true;
        }
        self
    }

    /// This item with the icon named `name` at the start of its row; as it was
    /// where the name is empty, the item is a heading, or no icon font has it.
    ///
    /// # Errors
    /// [`Error::NoMemory`] only.
    pub fn with_icon(mut self, host: &Host<'_>, name: &str) -> Result<Self, Error> {
        if !name.is_empty()
            && let Self::Row(row) = &mut self
        {
            row.icon = crate::icons::glyph(host, name)?;
        }
        Ok(self)
    }

    /// A row that looks like a field to type in: outlined while it has no
    /// focus.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the text cannot be copied.
    pub fn field(title: &str) -> Result<Self, Error> {
        Ok(Self::Row(RowItem {
            title: text::copy(title)?,
            field: true,
            ..RowItem::default()
        }))
    }

    /// A row with a note below that shows its text whole: a long title wraps
    /// to two lines and the note to four, where [`Item::noted`] cuts them.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the texts cannot be copied.
    pub fn whole(title: &str, value: &str, note: &str) -> Result<Self, Error> {
        Ok(Self::Row(RowItem {
            whole: true,
            ..match Self::noted(title, value, note)? {
                Self::Row(row) => row,
                Self::Heading(_) => RowItem::default(),
            }
        }))
    }

    /// Whether focus can rest on it.
    #[must_use]
    pub const fn selectable(&self) -> bool {
        matches!(self, Self::Row(row) if !row.disabled && !row.plain)
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
    /// A job that takes a while, centred below the rows (on a list of one
    /// page): a bar with a label under it; only they move
    /// ([`ItemList::redraw_progress`] redraws them alone). Behind a reference
    /// so that the footer, which the plugins' frames carry, stays small.
    Progress(&'a Progress<'a>),
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
    pub(crate) steps: Vec<Stepped>,
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
    /// The room above the first page's rows, if the owner has one.
    head: Option<Head>,
    /// Where the head's room, or else the bar of a [`Footer::Progress`], was in
    /// the last frame. One field for both: the list is carried in the frames
    /// of every plugin, and a field of its own grew them past the X4's band.
    spot: Spot,
    /// A picture over the words of an empty list ([`Footer::Empty`]).
    empty_icon: Option<paperpaw_sdk::native::fonts::IconGlyph>,
    /// The heights of the items as the last frame measured them (measuring
    /// wraps every row's text, which is most of a long list's frame, so a
    /// frame under the same look and width keeps them), and the look and
    /// header the bands of a frame share ([`band::Band`]). Empty after an item
    /// changed; taken and kept in functions of their own, and one pointer: more
    /// in the list or more code inlined into [`Self::draw_with`] and the
    /// functions that change an item grew the frame of every plugin that has a
    /// list past the band of the X4 (#288).
    kept: Option<alloc::boxed::Box<band::Kept>>,
    /// The arrow keys stay on the page (`focus::keep_arrows_stay`).
    arrows_stay: Option<bool>,
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
        self.drop_heights();
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
        self.drop_heights();
        if !self.items.get(self.focus).is_some_and(Item::selectable) {
            self.focus = self.nearest(self.focus);
        }
        Ok(())
    }

    /// Replaces the value (the text at the right) of the row at `index`, in
    /// place: for a number that moves (a download's progress) where the rest
    /// of the list stays as it is. `false` where `index` is no row.
    pub fn set_value(&mut self, index: usize, value: String) -> bool {
        self.drop_heights();
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
        self.drop_heights();
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

    /// Puts the icon named `name` over the words of an empty list
    /// ([`Footer::Empty`]); none where no icon font has it.
    ///
    /// # Errors
    /// [`Error::NoMemory`] only.
    pub fn set_empty_icon(&mut self, host: &Host<'_>, name: &str) -> Result<(), Error> {
        self.empty_icon = if name.is_empty() {
            None
        } else {
            crate::icons::glyph(host, name)?
        };
        Ok(())
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
                    "core.down" | "core.next" | "core.right" => {
                        self.on_page(self.after(self.focus), true)
                    }
                    "core.up" | "core.previous" | "core.left" => {
                        self.on_page(self.before(self.focus), false)
                    }
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
        for (minus, plus, item, _) in &self.steps {
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
        crate::focus::keep_arrows_stay(host, &mut self.arrows_stay, input);
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

    /// The heights of the items: the kept ones where they were measured under
    /// this look and width, else measured now, and what they are measured
    /// under. The caller gives them back ([`Self::keep_heights`]) once the
    /// frame is drawn.
    #[inline(never)]
    fn take_heights(
        &mut self,
        font: &Font<'_>,
        style: &Style,
        width: u16,
    ) -> Result<(Vec<u16>, u32), Error> {
        let under = measured_under(style, width, self.items.len());
        match self.kept.as_mut() {
            Some(kept) if kept.under == under && kept.heights.len() == self.items.len() => {
                Ok((core::mem::take(&mut kept.heights), under))
            }
            _ => {
                self.kept = None;
                Ok((heights_of(font, style, width, &self.items)?, under))
            }
        }
    }

    /// Drops what is kept: an item changed. Out of line, as the drop of
    /// a vector in each function that changes an item grew their frames.
    #[inline(never)]
    fn drop_heights(&mut self) {
        self.kept = None;
    }

    /// Keeps the heights of a frame for the next one.
    #[inline(never)]
    fn keep_heights(&mut self, heights: Vec<u16>, under: u32) {
        if let Some(kept) = self.kept.as_mut() {
            kept.heights = heights;
            kept.under = under;
        } else if let Ok(kept) = crate::mem::boxed(band::Kept::new(heights, under)) {
            self.kept = Some(kept);
        }
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
        let mut band = match self.take_band(frame) {
            Some(band) => band,
            None => band::Band::read(host, frame, (&display, &font), footer)?,
        };
        let (style, cards) = (&band.style, band.cards);
        let (width, height) = (display.display_width(), display.display_height());
        let top = style.header.saturating_add(style.gap);
        let mut area = Rect::new(0, top, width, height.saturating_sub(top));
        if let Some((rect, _)) = &band.note {
            area.height = area.height.saturating_sub(rect.height);
        }
        let (heights, under) = self.take_heights(&font, style, width)?;
        let head_high = self.head_high(style, area.height, &heights);
        let starts = paginate_with(&self.items, &heights, area.height, head_high)?;
        let page = starts
            .iter()
            .rposition(|start| *start <= self.focus)
            .unwrap_or(0);
        band.prepare_header(host, &font, title, (&display, page, starts.len()))?;
        let style = &band.style;
        let end = starts.get(page + 1).copied().unwrap_or(self.items.len());
        let mut body = Vec::new();
        paint::push(&mut body, Paint::Fill(style.page(area), style.paper))?;
        let mut y = top;
        let on_page = if page == 0 { head_high } else { 0 };
        let head_at = self.draw_head(&mut body, (&font, style), Rect::new(0, y, width, on_page))?;
        y = y.saturating_add(on_page);
        let drawn = row::paint_rows(
            self,
            &mut body,
            (&font, style),
            &heights,
            starts[page]..end,
            Rect::new(0, y, width, area.height),
            (
                shown,
                cards,
                (
                    frame.clip_top.saturating_sub(frame.rect.y),
                    frame.clip_bottom.saturating_sub(frame.rect.y),
                ),
            ),
        )?;
        if let Some((from, to)) = self.drag {
            draw_drag(&mut body, style, &drawn.hits, from, to)?;
        }
        if let Some((rect, lines)) = &band.note {
            draw_note(&mut body, &font, style, *rect, lines)?;
        }
        let below = Rect::new(0, drawn.y, width, height.saturating_sub(drawn.y));
        let mut bar = None;
        let code = footer_below(
            &mut body,
            &font,
            style,
            footer,
            (below, starts.len()),
            self.empty_icon,
            &mut bar,
        )?;
        let header = band.header.take().unwrap_or_default();
        paint::replay_with_icons(host, frame, &font, header.paint.iter().chain(body.iter()))?;
        if let Some((qr, room)) = code {
            paint::draw_code(frame, qr, room)?;
        }
        font.release()?;
        self.header = Page {
            paint: Vec::new(),
            back: header.back,
            pager: header.pager,
        };
        self.keep_heights(heights, under);
        if band.goes_on(frame) {
            band.header = Some(header);
            self.keep_band(band);
        }
        self.starts = starts;
        self.hits = drawn.hits;
        self.steps = drawn.steps;
        self.grip_rects = drawn.grips;
        self.spot = match (head_at, bar) {
            (Some(rect), _) => Spot::Head(rect),
            (None, Some(rect)) => Spot::Band(rect),
            (None, None) => Spot::None,
        };
        self.origin = (frame.rect.x, frame.rect.y);
        Ok(())
    }
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

/// A note in `rect`, as a list's [`Footer::Note`] is drawn: the paper, a light
/// rule along the top (none where the theme has no separators) and the text in
/// small letters, wrapped to as many lines as `rect` is high enough for (the
/// last ends in "…" where there was more). For the screens that are no list
/// (a hint below buttons, a status line).
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn note_in(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    text: &str,
) -> Result<(), Error> {
    let line = crate::geometry::line_px(font, style.small)?.max(1);
    let fits = usize::from((rect.height.saturating_sub(style.line) / line).max(1));
    let lines = text::wrap_with(
        |l| font.measure_text(style.small, l),
        text,
        f32::from(rect.width.saturating_sub(2 * style.margin)).max(1.0),
        fits,
    )?;
    draw_note(paint, font, style, rect, &lines)
}

/// What a footer draws below the rows: the empty page's words (over `icon`), or
/// the code.
fn footer_below<'a>(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    footer: Footer<'a>,
    (area, pages): (Rect, usize),
    icon: Option<paperpaw_sdk::native::fonts::IconGlyph>,
    band: &mut Option<Rect>,
) -> Result<Option<(&'a paperpaw_sdk::qr::QrCode, Rect)>, Error> {
    match footer {
        Footer::Empty { title, text } if pages == 1 => {
            draw_empty(paint, font, style, area, (title, text), icon)?;
        }
        Footer::Progress(progress) if pages == 1 => {
            *band = Some(draw_progress(paint, font, style, area, progress)?);
        }
        _ => {}
    }
    code_footer(paint, font, style, footer, area, pages)
}

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

/// Where each page starts: items are placed one after the other and a page
/// ends where the next does not fit. A heading never ends a page alone.
///
/// # Errors
/// [`Error::Invalid`] when `heights` and `items` differ in length;
/// [`Error::NoMemory`] when a start cannot be recorded. Nothing is returned
/// partially.
pub fn paginate(items: &[Item], heights: &[u16], room: u16) -> Result<Vec<usize>, Error> {
    paginate_with(items, heights, room, 0)
}

/// [`paginate`] where the first page has `taken` pixels less room (the head above
/// its rows).
///
/// # Errors
/// As [`paginate`].
pub fn paginate_with(
    items: &[Item],
    heights: &[u16],
    room: u16,
    taken: u16,
) -> Result<Vec<usize>, Error> {
    if items.len() != heights.len() {
        return Err(Error::Invalid);
    }
    let mut starts = Vec::new();
    starts.try_reserve(1).map_err(|_| Error::NoMemory)?;
    starts.push(0);
    let mut used = u32::from(taken);
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
        Item::Row(row) => {
            let own = looked(style, row);
            let style = own.as_ref().unwrap_or(style);
            let (_, band, _) = title_lines(font, style, width, row)?;
            // The track of a slider has a row of its own under the title.
            let band = if row.track().is_some() {
                band.saturating_add(style.row)
            } else {
                band
            };
            if row.note.is_empty() {
                return Ok(band);
            }
            let count = note_line_count(font, style, width, row)?;
            let line = crate::geometry::line_height(font, style.small)?;
            let notes = crate::geometry::px((line * count as f32).min(crate::geometry::MAX_PX))?;
            Ok(band.saturating_add(notes).saturating_add(style.gap / 2))
        }
    }
}

/// Whether a row is text a page reads out (or small print), not a row to choose.
fn page_text(row: &RowItem) -> bool {
    row.plain || row.small
}

/// The lines a title of a row that shows its text whole takes at most: four for
/// text a page reads out, two for a row to choose and for small print.
fn title_most(row: &RowItem) -> usize {
    if row.plain && !row.small {
        WHOLE_BODY_LINES
    } else {
        WHOLE_TITLE_LINES
    }
}

/// The lines a title wraps to (at most `most`): none where it fits on one line.
fn wrap_title(
    mut measure: impl FnMut(&str) -> Result<f32, Error>,
    title: &str,
    room: f32,
    most: usize,
) -> Result<Vec<String>, Error> {
    if measure(title)? <= room {
        return Ok(Vec::new());
    }
    text::wrap_with(measure, title, room, most)
}

/// The style a row is laid out and drawn in where it differs from the list's:
/// a small row has the small size for its text and a band as high as one line
/// of it; `None` for any other row.
fn looked(style: &Style, row: &RowItem) -> Option<Style> {
    row.small.then(|| Style {
        text: style.small,
        row: (style.small * 1.5) as u16,
        ..style.clone()
    })
}

fn note_room(style: &Style, width: u16, zone: bool) -> f32 {
    let lead = if zone { zone_room(style) } else { 0 };
    f32::from(width.saturating_sub(2 * style.margin + lead)).max(1.0)
}

/// The room a switch takes at the end of a row: its width and the margin.
fn switch_room(style: &Style) -> u16 {
    switch_size(style).0.saturating_add(style.margin)
}

/// The width and the height of a switch: a pill about twice as wide as it is
/// tall, as tall as the text.
fn switch_size(style: &Style) -> (u16, u16) {
    let high = (style.text * 0.95) as u16;
    (high.saturating_mul(2).saturating_add(high / 5), high)
}

/// A switch at the right end of `band`: on, a filled pill with the knob at the
/// right; off, an outline with the knob at the left. `ink` draws it, `ground`
/// is what the row is drawn on.
fn switch_pill(
    paint: &mut Vec<Paint>,
    style: &Style,
    band: Rect,
    on: bool,
    (ink, ground): (paperpaw_sdk::Color, paperpaw_sdk::Color),
) -> Result<(), Error> {
    let (width, high) = switch_size(style);
    let x = band
        .x
        .saturating_add(band.width)
        .saturating_sub(style.margin)
        .saturating_sub(width);
    let y = band.y.saturating_add(band.height.saturating_sub(high) / 2);
    let pill = Rect::new(x, y, width, high);
    let line = u32::from(style.line).max(2);
    let radius = u32::from(high) / 2;
    let knob = high.saturating_sub(high / 3) & !1;
    let gap = high.saturating_sub(knob) / 2;
    let (shape, knob_color) = if on {
        (None, ground)
    } else {
        (Some(line), ink)
    };
    paint::push(paint, Paint::Focus(pill, radius, shape, ink))?;
    let knob_x = if on {
        x.saturating_add(width)
            .saturating_sub(gap)
            .saturating_sub(knob)
    } else {
        x.saturating_add(gap)
    };
    let knob_rect = Rect::new(knob_x, y.saturating_add(gap), knob, knob);
    paint::push(
        paint,
        Paint::Focus(knob_rect, u32::from(knob) / 2, None, knob_color),
    )
}

fn zone_room(style: &Style) -> u16 {
    style.row.max(4) / 2 + style.margin
}

/// What the heights of `len` items are measured under: the look, the width.
fn measured_under(style: &Style, width: u16, len: usize) -> u32 {
    [
        u32::from(width),
        style.text.to_bits(),
        style.small.to_bits(),
        style.title.to_bits(),
        style.head.to_bits(),
        u32::from(style.row) | u32::from(style.heading) << 16,
        u32::from(style.margin) | u32::from(style.gap) << 16,
        u32::from(style.line),
        len as u32,
    ]
    .iter()
    .fold(0x811c_9dc5, |hash, part| {
        (hash ^ part).wrapping_mul(0x0100_0193)
    })
}

/// The height of each of `items`.
fn heights_of(
    font: &Font<'_>,
    style: &Style,
    width: u16,
    items: &[Item],
) -> Result<Vec<u16>, Error> {
    let mut heights = Vec::new();
    heights
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        heights.push(item_height(font, style, width, item)?);
    }
    Ok(heights)
}

/// A title that is a catalog key translated; text already translated (a
/// group's name) is not a key and is shown as it is.
fn translated(messages: &mut Messages, host: &Host<'_>, title: &str) -> Result<String, Error> {
    match messages.copy(host, title) {
        Err(Error::NotFound | Error::Invalid) => text::copy(title),
        other => other,
    }
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

/// The title and the value: on one line, or the lines a long title was
/// wrapped to, centred in the band, the value beside the first.
fn draw_title(
    paint: &mut Vec<Paint>,
    (font, style): (&Font<'_>, &Style),
    cols: Rect,
    row: &RowItem,
    lines: &[String],
    ink: paperpaw_sdk::Color,
    value: &str,
) -> Result<(), Error> {
    // Text a page reads out takes the room of both margins, no more.
    let alone = row.whole && page_text(row) && row.value.is_empty();
    let line_of = |paint: &mut Vec<Paint>, rect: Rect, text: &str, value: &str| {
        if alone {
            let x = f32::from(rect.x) + f32::from(style.margin);
            text::label_in(
                paint,
                font,
                style.rtl,
                rect,
                text,
                style.text,
                x,
                rect,
                ink,
                Some(rect),
            )
        } else {
            columns(paint, font, style, rect, text, value, ink)
        }
    };
    if lines.is_empty() {
        if let Some(bold) = row.bold().filter(|_| !alone) {
            let title = (row.title.as_str(), bold);
            return marked_columns(paint, font, style, cols, title, value, ink);
        }
        return line_of(paint, cols, &row.title, value);
    }
    let line = crate::geometry::line_px(font, style.text)?;
    let block = line.saturating_mul(lines.len() as u16);
    let top = cols.y.saturating_add(cols.height.saturating_sub(block) / 2);
    for (index, text) in lines.iter().enumerate() {
        let at = top.saturating_add(line.saturating_mul(index as u16));
        let rect = Rect::new(cols.x, at, cols.width, line);
        let value = if index == 0 { value } else { "" };
        line_of(paint, rect, text, value)?;
    }
    Ok(())
}

/// The title's band and the note's area of a row with `shift` pixels taken at
/// their start (for a picture or an icon).
fn shifted(rect: Rect, band: Rect, shift: u16) -> (Rect, Rect) {
    (
        Rect::new(
            band.x.saturating_add(shift),
            band.y,
            band.width.saturating_sub(shift),
            band.height,
        ),
        Rect::new(
            rect.x.saturating_add(shift),
            rect.y,
            rect.width.saturating_sub(shift),
            rect.height,
        ),
    )
}

/// Draws the icon of a row at the start of its first line (`band`) and tells
/// how far the title moves: the icon and a gap, before the title's own margin.
pub(crate) fn row_icon(
    paint: &mut Vec<Paint>,
    style: &Style,
    (rect, band): (Rect, Rect),
    glyph: paperpaw_sdk::native::fonts::IconGlyph,
    ink: paperpaw_sdk::Color,
) -> Result<u16, Error> {
    let size = style.text * 1.2;
    let side = size as u16;
    let at = style.flip(
        rect,
        Rect::new(
            rect.x.saturating_add(style.margin),
            band.y.saturating_add(band.height.saturating_sub(side) / 2),
            side,
            side,
        ),
    );
    paint::push(
        paint,
        Paint::Icon {
            glyph,
            size,
            origin: TextOrigin::new(f32::from(at.x), i32::from(at.y) + (size * 0.86) as i32),
            color: ink,
        },
    )?;
    Ok(icon_room(style))
}

/// How far an icon at the start of a row moves its title: the icon and a gap.
fn icon_room(style: &Style) -> u16 {
    ((style.text * 1.2) as u16).saturating_add(style.margin / 2)
}

/// The value a row shows at the right: none for a switch, which shows its
/// state, nor where its − and + show (`stepper`).
fn shown_value(row: &RowItem, stepper: bool) -> &str {
    // A stepper draws its value between its buttons.
    if row.on.is_some() || stepper {
        ""
    } else {
        &row.value
    }
}

/// What is behind a row: the focus, the separator, or a field's outline.
fn row_backdrop(
    paint: &mut Vec<Paint>,
    style: &Style,
    rect: Rect,
    row: &RowItem,
    focused: bool,
) -> Result<(), Error> {
    if row.field && !focused {
        let (outline, radius, line) = field_outline(style, rect);
        return paint::push(paint, Paint::Focus(outline, radius, Some(line), style.ink));
    }
    // Text the page reads out has no line under it.
    if row.small || row.plain {
        return Ok(());
    }
    backdrop(paint, style, rect, focused)
}

/// The ink of a faint row: its own grey, except on a panel of two grays, where
/// that grey is drawn as paper and the row would not show at all; there it is
/// the ink of notes (Beta 3's facts of a plugin were drawn so).
pub(crate) const fn disabled_ink(style: &Style) -> paperpaw_sdk::Color {
    if style.mono {
        style.note
    } else {
        style.disabled
    }
}

/// Where a field row's outline goes: its rectangle, corner radius and line
/// (the focused row's own, drawn thin).
fn field_outline(style: &Style, rect: Rect) -> (Rect, u32, u32) {
    let inset = rect.inset(
        (style.margin / 2).min(rect.width / 8),
        style.line.min(rect.height / 8),
    );
    let radius = if inset.width < inset.height || !style.pill {
        style.corner
    } else {
        u32::from(inset.height) / 2
    };
    (inset, radius, u32::from(style.line).max(1))
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
mod tests;
