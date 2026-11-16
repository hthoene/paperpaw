//! Page layers and page frames: what plugins draw on a book's pages
//! beside the words, and how the text is set in columns.
//!
//! - A *layer* ([`PageLayerSpec`], `Items::add_page_layer`) draws on
//!   every page a reader shows: side notes in a margin, a reading ruler,
//!   change bars, a sign that the page turned. It gets where the page's
//!   lines are ([`PageView`]) and draws statelessly, on some devices once
//!   for every band of the page (the X4 draws a page in eight). A layer
//!   may [reserve](PageLayerSpec::reserve) room at a side of the text: the
//!   text is laid out without it, and the layer is told where it is
//!   ([`PageView::room`]). Layers of an order below 0 are drawn behind
//!   the text, the others over it.
//! - A layer that [takes taps](PageLayerSpec::taps) hears
//!   [`events::PAGE_LAYER_TAPPED`](crate::builtin::events::PAGE_LAYER_TAPPED)
//!   when its room is tapped, or chosen with the buttons as links are.
//! - A *frame* ([`PageFrameSpec`], `Items::add_page_frame`) sets a
//!   page's text in columns: two columns, two pages side by side. The
//!   setting [`PAGE_FRAME`](crate::builtin::settings::PAGE_FRAME) (each
//!   book may have its own) chooses one; its name is the translation of
//!   its id.
//!
//! Readers find both in `Env::page_layers` ([`PageLayers`]). The shipped
//! reader's `PageType` applies the rooms and
//! the frame to the page's box; a reader of one's own does the same with
//! [`PageLayers::rooms`] and [`PageLayers::columns`].
//!
//! A layer that needs the page's words where they are drawn (to write
//! over or between them: a translation above each line, an interlinear
//! reading) asks for them ([`PageLayerSpec::words`]) and finds them in
//! [`PageView::words`]; readers measure them once a page, and only where
//! a layer asks. Room between the lines for it comes from a text filter
//! ([`Block::leading_above`](crate::document::Block::leading_above)),
//! which each line's [`LineBox::leading`] tells.
//!
//! A layer that takes input on what it drew (a translation that opens,
//! a note) says where its items are ([`PageLayerSpec::items`], from the
//! page as it draws it): a tap on one, or Select on one chosen with the
//! buttons as links are, comes to its plugin as
//! [`events::PAGE_LAYER_TAPPED`](crate::builtin::events::PAGE_LAYER_TAPPED)
//! with the item's id, and the reader does nothing else with it. Taps
//! anywhere else, and every other button, stay the reader's.
//!
//! A layer whose drawing changes while the page stays (its data changed)
//! asks for the screen to be drawn again
//! ([`Command::Redraw`](crate::Command::Redraw)): readers keep nothing of
//! what layers drew.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::document::Locator;
use crate::reading::WordBox;
use crate::setting::Setting;
use crate::{Canvas, Env, Id, Rect};

/// Columns a frame may set a page in at most.
pub const MAX_COLUMNS: usize = 4;

/// A side of a page's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    fn name(self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Bottom => "bottom",
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

/// A line of a page as shown: its box and the text it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LineBox {
    /// From the top of its tallest letters to below its lowest.
    pub area: Rect,
    pub start: Locator,
    /// Just after its last character.
    pub end: Locator,
    /// The room above `area` that the line's block asked for
    /// ([`Block::leading_above`](crate::document::Block::leading_above)),
    /// in pixels: between the line before and this one's letters. 0 for
    /// none.
    pub leading: u16,
}

impl LineBox {
    pub fn new(area: Rect, start: Locator, end: Locator) -> Self {
        Self {
            area,
            start,
            end,
            leading: 0,
        }
    }

    /// With `leading` pixels of room above it.
    pub fn leading(mut self, leading: u16) -> Self {
        self.leading = leading;
        self
    }
}

/// A page as a reader shows it, for layers.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct PageView<'a> {
    /// The book, and its language (empty where unknown).
    pub path: &'a str,
    pub language: &'a str,
    /// Where the page's text begins, and where the next page's does.
    pub start: Locator,
    pub end: Locator,
    /// The text's box: the page without its margins, the status lines and
    /// the layers' rooms.
    pub content: Rect,
    /// The columns the text is set in, left to right (the content alone
    /// without a frame).
    pub columns: &'a [Rect],
    /// The page's lines, in reading order.
    pub lines: &'a [LineBox],
    /// The page's words with their boxes and places in the book, in
    /// reading order, as `reading::word_boxes`
    /// measures them, where a layer that is on asked for them
    /// ([`PageLayerSpec::words`]); else empty (and not measured).
    pub words: &'a [WordBox],
    /// The room this layer reserved, if it did.
    pub room: Option<Rect>,
    /// The status lines, where they are shown.
    pub top_status: Option<Rect>,
    pub bottom_status: Option<Rect>,
}

impl<'a> PageView<'a> {
    /// The page of `path` from `start` to `end`, its text in `content`;
    /// no lines, no room, no status lines, one column.
    pub fn new(path: &'a str, start: Locator, end: Locator, content: Rect) -> Self {
        Self {
            path,
            language: "",
            start,
            end,
            content,
            columns: &[],
            lines: &[],
            words: &[],
            room: None,
            top_status: None,
            bottom_status: None,
        }
    }

    pub fn language(mut self, language: &'a str) -> Self {
        self.language = language;
        self
    }

    pub fn columns(mut self, columns: &'a [Rect]) -> Self {
        self.columns = columns;
        self
    }

    pub fn lines(mut self, lines: &'a [LineBox]) -> Self {
        self.lines = lines;
        self
    }

    pub fn words(mut self, words: &'a [WordBox]) -> Self {
        self.words = words;
        self
    }

    pub fn room(mut self, room: Option<Rect>) -> Self {
        self.room = room;
        self
    }

    pub fn status(mut self, top: Option<Rect>, bottom: Option<Rect>) -> Self {
        self.top_status = top;
        self.bottom_status = bottom;
        self
    }

    /// The line at the height of `point` nearest to it (in the column it
    /// is in), e.g. the one beside a tap in a margin; the room above a
    /// line ([`LineBox::leading`]) is the line's.
    pub fn line_at(&self, point: crate::Point) -> Option<&LineBox> {
        let (x, y) = (point.x, point.y);
        let beside = |line: &&LineBox| {
            i32::from(line.area.y) - i32::from(line.leading) <= y && y < line.area.bottom() as i32
        };
        let distance = |line: &&LineBox| {
            let (left, right) = (i32::from(line.area.x), line.area.right() as i32);
            if x < left {
                left - x
            } else {
                (x - right).max(0)
            }
        };
        self.lines.iter().filter(beside).min_by_key(distance)
    }
}

/// Something a layer drew that takes a tap, or Select when chosen with
/// the buttons ([`PageLayerSpec::items`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LayerItem {
    /// The layer's own id for it, told back when it is tapped.
    pub id: u32,
    /// Where it is on the screen.
    pub area: Rect,
    /// What the reader says it does while it is chosen with the buttons,
    /// e.g. "Show the translation"; empty for the layer's name.
    pub label: alloc::string::String,
}

impl LayerItem {
    pub fn new(id: u32, area: Rect) -> Self {
        Self {
            id,
            area,
            label: alloc::string::String::new(),
        }
    }

    pub fn label(mut self, label: impl Into<alloc::string::String>) -> Self {
        self.label = label.into();
        self
    }
}

/// The items of a layer on a page, as it draws them.
pub type ItemsFn = Box<dyn Fn(&Env<'_>, &PageView<'_>) -> Vec<LayerItem>>;

/// How much room a layer reserves, and at which side.
pub type ReserveFn = Box<dyn Fn(&Env<'_>) -> Option<(Side, u16)>>;
/// What a layer draws on a page.
pub type DrawLayerFn = Box<dyn Fn(&mut Canvas<'_>, &Env<'_>, &PageView<'_>)>;

/// A layer contribution (see the [module documentation](self)).
#[non_exhaustive]
pub struct PageLayerSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// Layers are drawn by order, smallest first: below 0 behind the text,
    /// from 0 over it. Rooms are reserved in the same order, the first
    /// at the text's edge.
    pub order: i16,
    /// A setting that switches it on and off, if any.
    pub switch: Option<Id>,
    /// It hears taps on its room
    /// ([`events::PAGE_LAYER_TAPPED`](crate::builtin::events::PAGE_LAYER_TAPPED)).
    pub taps: bool,
    /// It draws by the page's words ([`PageView::words`]).
    pub words: bool,
    /// Where its items are on a page ([`PageLayerSpec::items`]).
    pub items: Option<ItemsFn>,
    pub reserve: Option<ReserveFn>,
    pub draw: DrawLayerFn,
}

impl fmt::Debug for PageLayerSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageLayerSpec")
            .field("id", &self.id)
            .field("order", &self.order)
            .field("switch", &self.switch)
            .field("taps", &self.taps)
            .field("words", &self.words)
            .finish_non_exhaustive()
    }
}

impl PageLayerSpec {
    /// Layer `id`, named `title`, drawing with `draw`; order 0, always
    /// on, no room, no taps, no words.
    pub fn new(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        draw: impl Fn(&mut Canvas<'_>, &Env<'_>, &PageView<'_>) + 'static,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            order: 0,
            switch: None,
            taps: false,
            words: false,
            items: None,
            reserve: None,
            draw: Box::new(draw),
        }
    }

    pub fn order(mut self, order: i16) -> Self {
        self.order = order;
        self
    }

    /// Drawn only while `switch` is on.
    pub fn switch(mut self, switch: &Setting<bool>) -> Self {
        self.switch = Some(switch.into());
        self
    }

    /// Hears taps on its room.
    pub fn taps(mut self) -> Self {
        self.taps = true;
        self
    }

    /// Items it drew that take taps, and Select when chosen with the
    /// buttons: `items` says where they are on a page (as it draws them,
    /// from the same [`PageView`]). A tap on one, or Select, comes to the
    /// plugin as [`events::PAGE_LAYER_TAPPED`](crate::builtin::events::PAGE_LAYER_TAPPED)
    /// with the item's id; nothing else happens with it. Items of layers
    /// drawn later (over the others) are hit first.
    pub fn items(
        mut self,
        items: impl Fn(&Env<'_>, &PageView<'_>) -> Vec<LayerItem> + 'static,
    ) -> Self {
        self.items = Some(Box::new(items));
        self
    }

    /// Draws by the page's words: the reader gives them in
    /// [`PageView::words`] while it is on (measured once a page, where a
    /// layer asks).
    pub fn words(mut self) -> Self {
        self.words = true;
        self
    }

    /// Room at a side of the text, `None` for none, in pixels (from
    /// millimetres with `env.metrics()`). Pages are laid out anew when it
    /// changes: it depends on settings only, not on the page.
    pub fn reserve(mut self, reserve: impl Fn(&Env<'_>) -> Option<(Side, u16)> + 'static) -> Self {
        self.reserve = Some(Box::new(reserve));
        self
    }
}

/// The columns a frame sets a page's text in, from the text's box.
pub type ColumnsFn = Box<dyn Fn(&Env<'_>, Rect) -> Vec<Rect>>;

/// A frame contribution (see the [module documentation](self)): its
/// name is the translation of its id.
///
/// Its columns are best of one width: where a reader looks for the start
/// of the line a place is in (a place kept, a link's target), it breaks
/// lines as the first column does. Pages are laid out column by column
/// from their starts all the same.
#[non_exhaustive]
pub struct PageFrameSpec {
    pub id: Id,
    pub columns: ColumnsFn,
}

impl fmt::Debug for PageFrameSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageFrameSpec")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl PageFrameSpec {
    /// Frame `id`, setting the text's box in the columns `columns`
    /// gives, left to right.
    pub fn new(id: Id, columns: impl Fn(&Env<'_>, Rect) -> Vec<Rect> + 'static) -> Self {
        Self {
            id,
            columns: Box::new(columns),
        }
    }

    /// `count` columns of the same width, `gap` pixels apart.
    pub fn columns(id: Id, count: u16, gap: u16) -> Self {
        Self::new(id, move |_, area| split(area, count, gap))
    }
}

/// `area` split in `count` columns (at most [`MAX_COLUMNS`]) of the same
/// width, `gap` apart; the last takes the pixels left over.
pub fn split(area: Rect, count: u16, gap: u16) -> Vec<Rect> {
    let count = count.clamp(1, MAX_COLUMNS as u16);
    let gaps = gap.saturating_mul(count - 1);
    let width = area.width.saturating_sub(gaps) / count;
    let right = u16::try_from(area.right()).unwrap_or(u16::MAX);
    (0..count)
        .map(|i| {
            let x = area
                .x
                .saturating_add(i.saturating_mul(width.saturating_add(gap)));
            let width = match i + 1 == count {
                true => right.saturating_sub(x),
                false => width,
            };
            Rect::new(x, area.y, width, area.height)
        })
        .collect()
}

/// The registered layers, by order, and the frames.
#[derive(Debug, Default)]
pub struct PageLayers {
    layers: Vec<PageLayerSpec>,
    frames: Vec<PageFrameSpec>,
}

impl PageLayers {
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `layer` after those of a smaller or the same order.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, layer: PageLayerSpec) {
        let at = self
            .layers
            .iter()
            .position(|l| l.order > layer.order)
            .unwrap_or(self.layers.len());
        self.layers.insert(at, layer);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_frame(&mut self, frame: PageFrameSpec) {
        self.frames.push(frame);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.layers.shrink_to_fit();
        self.frames.shrink_to_fit();
    }

    /// Whether there are no layers (there may be frames).
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Whether there are frames.
    pub fn has_frames(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Whether neither layers nor frames change pages: nothing to ask
    /// them.
    pub fn is_idle(&self) -> bool {
        self.layers.is_empty() && self.frames.is_empty()
    }

    /// The layer `id`, e.g. for its name.
    pub fn get(&self, id: &Id) -> Option<&PageLayerSpec> {
        self.layers.iter().find(|l| l.id == *id)
    }

    /// The layer switched on and off by setting `id`, if any.
    pub fn get_by_switch(&self, id: &str) -> Option<&PageLayerSpec> {
        self.layers
            .iter()
            .find(|l| l.switch.as_ref().is_some_and(|s| s.as_str() == id))
    }

    /// The layers that are on, by order.
    pub fn on<'s>(&'s self, env: &'s Env<'_>) -> impl Iterator<Item = &'s PageLayerSpec> {
        self.layers.iter().filter(move |l| is_on(env, l))
    }

    /// The items of the layers that are on, on `page` ([`PageLayerSpec::items`]),
    /// with their layers: those drawn last (on top) first, as taps hit
    /// them.
    pub fn items(&self, env: &Env<'_>, page: &PageView<'_>) -> Vec<(Id, LayerItem)> {
        let mut items = Vec::new();
        for layer in self.on(env).collect::<Vec<_>>().into_iter().rev() {
            if let Some(of) = &layer.items {
                items.extend(
                    of(env, page)
                        .into_iter()
                        .map(|item| (layer.id.clone(), item)),
                );
            }
        }
        items
    }

    /// The item of a layer that is on at `point` on `page`, if any.
    pub fn item_at(
        &self,
        env: &Env<'_>,
        page: &PageView<'_>,
        point: crate::Point,
    ) -> Option<(Id, LayerItem)> {
        self.items(env, page)
            .into_iter()
            .find(|(_, item)| item.area.contains(point))
    }

    /// Whether a layer that is on draws by the page's words
    /// ([`PageLayerSpec::words`]): the reader then gives them.
    pub fn want_words(&self, env: &Env<'_>) -> bool {
        self.on(env).any(|l| l.words)
    }

    /// The frames' ids, in the order of the plugins.
    pub fn frames(&self) -> impl Iterator<Item = &Id> {
        self.frames.iter().map(|f| &f.id)
    }

    /// The rooms the layers that are on reserve around `content`, by
    /// order, the first at its edge; and what is left for the text. The
    /// text keeps at least half of its width and height: a room beyond
    /// is narrowed.
    pub fn rooms(&self, env: &Env<'_>, content: Rect) -> (Vec<(Id, Rect)>, Rect) {
        let mut rooms = Vec::new();
        let mut left = content;
        for layer in self.on(env) {
            let Some((side, size)) = layer.reserve.as_ref().and_then(|r| r(env)) else {
                continue;
            };
            let room = match side {
                Side::Left | Side::Right => {
                    let size = size.min(left.width.saturating_sub(content.width / 2));
                    left.width -= size;
                    let x = match side {
                        Side::Left => {
                            left.x += size;
                            left.x - size
                        }
                        _ => left.x + left.width,
                    };
                    Rect::new(x, left.y, size, left.height)
                }
                _ => {
                    let size = size.min(left.height.saturating_sub(content.height / 2));
                    left.height -= size;
                    let y = match side {
                        Side::Top => {
                            left.y += size;
                            left.y - size
                        }
                        _ => left.y + left.height,
                    };
                    Rect::new(left.x, y, left.width, size)
                }
            };
            if room.width > 0 && room.height > 0 {
                rooms.push((layer.id.clone(), room));
            }
        }
        (rooms, left)
    }

    /// The columns of the frame [`PAGE_FRAME`](crate::builtin::settings::PAGE_FRAME)
    /// chooses for text in `content`, left to right: within it, at most
    /// [`MAX_COLUMNS`]; `content` alone without a frame (or where it
    /// gives none).
    pub fn columns(&self, env: &Env<'_>, content: Rect) -> Vec<Rect> {
        let chosen = env.settings().read(&crate::builtin::settings::PAGE_FRAME);
        let mut columns: Vec<Rect> = self
            .frames
            .iter()
            .find(|f| f.id.as_str() == &*chosen)
            .map(|frame| (frame.columns)(env, content))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|column| column.intersection(&content))
            .filter(|column| column.width > 0 && column.height > 0)
            .take(MAX_COLUMNS)
            .collect();
        if columns.is_empty() {
            columns.push(content);
        }
        columns
    }

    /// What the layers' rooms and the frame's columns are now, for a
    /// layout's key: pages are laid out anew when it changes.
    pub fn key(&self, env: &Env<'_>, content: Rect) -> alloc::string::String {
        use core::fmt::Write;
        let (rooms, text) = self.rooms(env, content);
        let mut key = alloc::string::String::new();
        for (id, room) in &rooms {
            let side = self
                .get(id)
                .and_then(|l| l.reserve.as_ref())
                .and_then(|r| r(env))
                .map_or("", |(side, _)| side.name());
            let _ = write!(key, "{id}:{side}:{}x{};", room.width, room.height);
        }
        for column in self.columns(env, text) {
            let _ = write!(
                key,
                "{},{},{}x{};",
                column.x, column.y, column.width, column.height
            );
        }
        key
    }

    /// Draws the layers that are on whose order is below 0 (`behind`) or
    /// not, each with its room of `rooms`.
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        page: &PageView<'_>,
        rooms: &[(Id, Rect)],
        behind: bool,
    ) {
        for layer in self.on(env).filter(|l| (l.order < 0) == behind) {
            let room = rooms.iter().find(|(id, _)| *id == layer.id).map(|r| r.1);
            (layer.draw)(canvas, env, &page.room(room));
        }
    }

    /// The layer that takes taps whose room of `rooms` holds `(x, y)`.
    pub fn tapped<'s>(
        &'s self,
        env: &Env<'_>,
        rooms: &[(Id, Rect)],
        x: i32,
        y: i32,
    ) -> Option<&'s Id> {
        let point = crate::Point::new(x, y);
        rooms
            .iter()
            .filter(|(_, room)| room.contains(point))
            .find_map(|(id, _)| {
                self.layers
                    .iter()
                    .find(|l| l.taps && l.id == *id && is_on(env, l))
            })
            .map(|l| &l.id)
    }
}

fn is_on(env: &Env<'_>, layer: &PageLayerSpec) -> bool {
    layer
        .switch
        .as_ref()
        .is_none_or(|s| env.settings().bool(s.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_split_the_box_evenly() {
        // Near the end of the coordinates and with many columns: no
        // overflow, at most four.
        assert_eq!(split(Rect::new(u16::MAX - 10, 0, 10, 5), 3, 300).len(), 3);
        assert_eq!(split(Rect::new(0, 0, 400, 5), 50, 2).len(), MAX_COLUMNS);
        let columns = split(Rect::new(10, 20, 410, 300), 2, 10);
        assert_eq!(
            columns,
            [Rect::new(10, 20, 200, 300), Rect::new(220, 20, 200, 300)]
        );
        assert_eq!(
            split(Rect::new(0, 0, 90, 10), 1, 10),
            [Rect::new(0, 0, 90, 10)]
        );
    }

    #[test]
    fn the_line_beside_a_point_is_the_nearest_on_its_height() {
        let at = |n| Locator::new(0, n);
        let lines = [
            LineBox::new(Rect::new(10, 10, 100, 20), at(0), at(10)),
            LineBox::new(Rect::new(10, 30, 100, 20), at(10), at(20)),
            LineBox::new(Rect::new(130, 10, 100, 20), at(20), at(30)),
        ];
        let page = PageView::new("b", at(0), at(30), Rect::new(10, 10, 220, 40)).lines(&lines);
        assert_eq!(
            page.line_at(crate::Point::new(5, 35)).map(|l| l.start),
            Some(at(10))
        );
        assert_eq!(
            page.line_at(crate::Point::new(240, 15)).map(|l| l.start),
            Some(at(20))
        );
        assert_eq!(
            page.line_at(crate::Point::new(5, 15)).map(|l| l.start),
            Some(at(0))
        );
        assert!(page.line_at(crate::Point::new(5, 60)).is_none());
    }
}
