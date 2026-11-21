//! The dashboard while editing: the options of an item (the bottom menu every
//! other screen uses: move, change the size, settings, remove), and an item
//! the buttons are moving or sizing. What the buttons do to cells is
//! `native_arrange` without the host; this file holds the state, asks for the
//! menu, and writes the page once an item is set down or removed.
//!
//! The finger's drag of an item or its corner waits for H4 (an ignored
//! acceptance case); a tap on an item offers its options.
use super::*;
use crate::native_add_screen as add_screen;
use crate::native_arrange::{self as arrange, Choice, Dir, Mode, Put, choices};

use crate::native_item_settings as item_settings;
use paperpaw_sdk::{
    native::{
        callbacks::PointerPhase,
        events::{AnswerOutcome, Answered},
        ui::{Question, RequestId},
    },
    widgets::grid::Cells,
};

/// The options of the item of a cell, asked as a menu: the request, and the
/// cell it is for. Kept across a new layout: the answer comes after the menu's
/// screen has closed, which may have laid the page out again.
#[derive(Debug)]
pub(super) struct Options {
    cell: usize,
    request: RequestId,
    /// The item is a widget that has settings of its own: *Settings* is offered.
    settings: bool,
}

/// An item a finger has taken along: which finger, which item, whether by its
/// corner (sizing it), where it came down and the cells it would take.
#[derive(Debug, Clone, Copy)]
pub(super) struct Drag {
    contact: u32,
    cell: usize,
    resize: bool,
    start: Point,
    to: Cells,
    moved: bool,
}

/// An item the buttons are moving or sizing.
#[derive(Debug, Clone, Copy)]
pub(super) struct Moving {
    cell: usize,
    mode: Mode,
}

impl Dashboard {
    /// The options of the item of `cell` asked as the bottom menu, titled with
    /// the item's name.
    pub(super) fn open_options(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        cell: usize,
    ) -> Result<(), Error> {
        let settings = match self.widget_at(cell) {
            // A refused read (other than for memory) means: no Settings.
            Some(id) => match item_settings::has_settings(host, id) {
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                found => found.unwrap_or(false),
            },
            None => false,
        };
        let title = self
            .laid
            .as_ref()
            .and_then(|laid| laid.cells.get(cell))
            .map_or("", |c| c.name.as_str());
        let mut messages = Messages::new()?;
        let mut words = String::new();
        for (at, (_, key)) in choices(settings).iter().enumerate() {
            let word = messages.copy(host, key)?;
            words = if at == 0 {
                word
            } else {
                paperpaw_ui_kit::text::concat(&[&words, "\n", &word])?
            };
        }
        let question = Question::choice(title, "", &words)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = ui.ask(host, question, &mut input, &mut out)?;
        self.options = Some(Options {
            cell,
            request,
            settings,
        });
        Ok(())
    }

    /// Whether the options' menu of `request` is this page's.
    pub fn asked(&self, request: RequestId) -> bool {
        self.options.as_ref().is_some_and(|o| o.request == request)
    }

    /// The answer of the options' menu: what was chosen is done to the item it
    /// was asked for. A menu closed with nothing chosen leaves the page as it is.
    pub fn options_answered(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        answer: Answered,
    ) -> Result<(), Error> {
        let Some(options) = self.options.as_ref() else {
            return Ok(());
        };
        if options.request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let (cell, settings) = (options.cell, options.settings);
        self.options = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let chosen = usize::try_from(answer.choice())
            .ok()
            .and_then(|at| choices(settings).get(at))
            .map(|(choice, _)| *choice);
        match chosen {
            Some(choice) => self.choose(host, ui, cell, choice),
            None => Ok(()),
        }
    }

    /// The id of the widget in the item of `cell`, if the item is a widget.
    fn widget_at(&self, cell: usize) -> Option<&str> {
        let laid = self.laid.as_ref()?;
        let item = laid.cells.get(cell)?.item;
        match &laid.items.get(item)?.what {
            What::Widget(id) => Some(id.as_str()),
            What::Tool(_) => None,
        }
    }

    /// An input while an item is moved or sized; `None` if not, then the page
    /// takes it.
    pub(super) fn arrange_input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        if let Some(moving) = self.moving {
            return self.moving_input(host, ui, moving, input).map(Some);
        }
        self.drag_input(host, ui, input)
    }

    /// A finger while editing: on an item it takes the item along (by its
    /// corner it sizes it); lifted where it came down it is a tap (the item's
    /// options), lifted elsewhere the item goes there where the cells are free.
    /// What a finger takes is its own, with no gesture made of it.
    fn drag_input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let Input::Pointer {
            position,
            phase,
            contact,
            ..
        } = *input
        else {
            return Ok(None);
        };
        if !self.editing {
            return Ok(None);
        }
        match (phase, self.drag) {
            // A finger down again is a fresh start: one whose end was lost (no up,
            // no cancel) does not keep a later one from taking an item.
            (PointerPhase::Down, drag) if drag.is_none_or(|drag| drag.contact == contact) => {
                self.drag = None;
                Ok(self.drag_start(position, contact))
            }
            (PointerPhase::Move, Some(drag)) if drag.contact == contact => {
                self.drag_to(host, drag, position)?;
                Ok(Some(Handled::Yes))
            }
            (PointerPhase::Up | PointerPhase::Cancel, Some(drag)) if drag.contact == contact => {
                self.drag = None;
                self.drag_end(host, ui, drag, phase == PointerPhase::Up)?;
                Ok(Some(Handled::Yes))
            }
            _ => Ok(None),
        }
    }

    /// The item under a finger that came down at `at`, and whether at its corner.
    fn drag_start(&mut self, at: Point, contact: u32) -> Option<Handled> {
        let laid = self.laid.as_ref()?;
        let (cell, found) = laid
            .cells
            .iter()
            .enumerate()
            .find(|(_, cell)| inside(cell.rect, at))?;
        let item = laid.items.get(found.item)?;
        let to = item.at?;
        let corner = corner_rect(found.rect, laid.handle);
        self.drag = Some(Drag {
            contact,
            cell,
            resize: inside(corner, at),
            start: at,
            to,
            moved: false,
        });
        Some(Handled::Yes)
    }

    /// The finger moved: where the item would land. Drawn again only where that
    /// is other cells (a finger trembling in a cell asks for nothing: a refresh
    /// takes most of a second on some panels).
    fn drag_to(&mut self, host: &Host<'_>, mut drag: Drag, at: Point) -> Result<(), Error> {
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let Some(cell) = laid.cells.get(drag.cell) else {
            return Ok(());
        };
        let Some(from) = laid.items.get(cell.item).and_then(|item| item.at) else {
            return Ok(());
        };
        let Some(to) = arrange::dragged(
            &laid.frame.geometry,
            from,
            (drag.start, at),
            drag.resize,
            (cell.least, cell.span),
        ) else {
            return Ok(());
        };
        if to != drag.to {
            drag.to = to;
            drag.moved |= to != from;
            self.drag = Some(drag);
            redraw(host)?;
        }
        Ok(())
    }

    /// The finger lifted (`up`) or was taken away: a tap offers the item's
    /// options, a drag puts the item where it would land if the cells are free.
    fn drag_end(&mut self, host: &Host<'_>, ui: &Ui, drag: Drag, up: bool) -> Result<(), Error> {
        if !up {
            return redraw(host);
        }
        if !drag.moved {
            let at = self.laid.as_ref().and_then(|laid| {
                laid.targets
                    .iter()
                    .position(|t| t.owner == Owner::Tool(drag.cell))
            });
            if let Some(at) = at {
                self.focus.to(at);
            }
            return self.open_options(host, ui, drag.cell);
        }
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let Some(item) = laid.cells.get(drag.cell).map(|cell| cell.item) else {
            return Ok(());
        };
        // Put on a copy: what is in memory is changed only once it is written.
        let mut items = crate::native_grid::copy_items(&laid.items)?;
        match arrange::put(&mut items, item, drag.to, &laid.frame.geometry) {
            Put::Done => self.write(host, &items, &[]).map(drop),
            Put::InTheWay => {
                add_screen::say(host, "core.grid.in-the-way")?;
                redraw(host)
            }
            Put::Same => redraw(host),
        }
    }

    /// Where the item a finger has taken would land: a thick frame where the
    /// cells are free, a thin one where not.
    pub(super) fn drag_frame(
        &self,
        paint: &mut Vec<Paint>,
        style: &Style,
        laid: &Laid,
    ) -> Result<(), Error> {
        let Some(drag) = self.drag.filter(|drag| drag.moved) else {
            return Ok(());
        };
        let Some(item) = laid.cells.get(drag.cell).map(|cell| cell.item) else {
            return Ok(());
        };
        let free = arrange::free(&laid.items, item, drag.to, &laid.frame.geometry);
        let width = u32::from(style.line).max(1) * if free { 4 } else { 1 };
        paintpush(
            paint,
            Paint::Focus(
                laid.frame.geometry.rect(drag.to),
                style.corner,
                Some(width),
                style.ink,
            ),
        )
    }

    /// The disc in the corner of a cell that a finger sizes it by, filled as
    /// in Beta 3: half a row across (16 px at least), flush with the cell's
    /// corner, in the corner of the larger touch area `side`.
    pub(super) fn corner_mark(
        &self,
        paint: &mut Vec<Paint>,
        style: &Style,
        rect: Rect,
        side: u16,
    ) -> Result<(), Error> {
        let corner = corner_rect(rect, side);
        let shown = (corner.width / 2).max(16).min(corner.width);
        let disc = Rect::new(
            corner.x.saturating_add(corner.width - shown),
            corner.y.saturating_add(corner.height - shown),
            shown,
            shown,
        );
        paintpush(paint, Paint::Focus(disc, style.corner, None, style.ink))
    }

    fn choose(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        cell: usize,
        choice: Choice,
    ) -> Result<(), Error> {
        match choice {
            Choice::Move => self.pick_up(host, cell, Mode::Move),
            Choice::Resize => self.pick_up(host, cell, Mode::Resize),
            Choice::Settings => self.settings(host, ui, cell),
            Choice::Remove => self.remove(host, cell),
        }
    }

    /// The widget's own settings: its placement (one placed before there were
    /// placements gets its number first, kept on the page), then the settings
    /// open on its page.
    fn settings(&mut self, host: &Host<'_>, ui: &Ui, cell: usize) -> Result<(), Error> {
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let Some(item) = laid.cells.get(cell).map(|c| c.item) else {
            return Ok(());
        };
        let Some(What::Widget(id)) = laid.items.get(item).map(|i| &i.what) else {
            return Ok(());
        };
        let shown = Shown::current(host)?;
        let placed = item_settings::placement(host, &shown.src, id)?;
        let renamed = placed != *id;
        if renamed {
            let mut items = crate::native_grid::copy_items(&laid.items)?;
            let Some(entry) = items.get_mut(item) else {
                return Ok(());
            };
            entry.what = What::Widget(paperpaw_ui_kit::text::copy(&placed)?);
            // Not saved (too big): the settings would show a placement the page
            // does not hold.
            if !self.write(host, &items, &[])? {
                return Ok(());
            }
        }
        item_settings::open(host, ui, &placed)
    }

    /// The item of `cell` is picked up: the buttons move or size it.
    fn pick_up(&mut self, host: &Host<'_>, cell: usize, mode: Mode) -> Result<(), Error> {
        let at = self.laid.as_ref().and_then(|laid| {
            laid.targets
                .iter()
                .position(|t| t.owner == Owner::Tool(cell))
        });
        if let Some(at) = at {
            self.focus.to(at);
        }
        self.moving = Some(Moving { cell, mode });
        add_screen::say(host, arrange::help(mode))?;
        redraw(host)
    }

    fn moving_input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        moving: Moving,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        match *input {
            Input::Action { action, .. } => {
                if action == "core.confirm" || action == "core.back" {
                    self.moving = None;
                    self.keep(host)?;
                } else if let Some(dir) = Dir::of(action) {
                    self.step(host, ui, moving, dir)?;
                }
                Ok(Handled::Yes)
            }
            // A finger while the buttons move an item is not for the page.
            Input::Gesture { .. } => Ok(Handled::Yes),
            _ => Ok(Handled::No),
        }
    }

    /// One step of the item being moved or sized: where it goes, the widget
    /// placed again; where another item is in the way, said. Where the host
    /// refuses to place the widget again the page is laid out anew from the
    /// settings, so that what is in memory and what shows agree.
    fn step(&mut self, host: &Host<'_>, ui: &Ui, moving: Moving, dir: Dir) -> Result<(), Error> {
        let stepped = self.step_cells(host, ui, moving, dir);
        if stepped.is_err() {
            self.moving = None;
            self.stale = true;
        }
        stepped
    }

    fn step_cells(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        moving: Moving,
        dir: Dir,
    ) -> Result<(), Error> {
        let Some(laid) = self.laid.as_mut() else {
            return Ok(());
        };
        let Laid {
            cells,
            items,
            targets,
            frame,
            ..
        } = laid;
        let Some(cell) = cells.get_mut(moving.cell) else {
            return Ok(());
        };
        let Some(at) = items.get(cell.item).and_then(|item| item.at) else {
            return Ok(());
        };
        let to = arrange::stepped(moving.mode, at, dir, cell.least);
        match arrange::put(items, cell.item, to, &frame.geometry) {
            Put::Same => Ok(()),
            Put::InTheWay => add_screen::say(host, "core.grid.in-the-way"),
            Put::Done => {
                cell.rect = frame.geometry.rect(to);
                if let Some(target) = targets
                    .iter_mut()
                    .find(|t| t.owner == Owner::Tool(moving.cell))
                {
                    target.rect = cell.rect;
                }
                Self::place_again(host, ui, cell)?;
                redraw(host)
            }
        }
    }

    /// The widget of a cell placed at the rectangle it has now.
    fn place_again(host: &Host<'_>, ui: &Ui, cell: &Cell) -> Result<(), Error> {
        match cell.widget.as_ref() {
            // Refused: the widget stays where it was placed before.
            Some(made) => match made.place(host, ui, cell.rect) {
                Err(Error::NoMemory) => Err(Error::NoMemory),
                _ => Ok(()),
            },
            None => Ok(()),
        }
    }

    /// Item of `cell` removed: what its placement set goes with it.
    fn remove(&mut self, host: &Host<'_>, cell: usize) -> Result<(), Error> {
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let Some(item) = laid.cells.get(cell).map(|c| c.item) else {
            return Ok(());
        };
        let (rest, reset) = arrange::removed(&laid.items, item)?;
        let mut resets = Vec::new();
        if let Some(id) = reset {
            push(&mut resets, id)?;
        }
        self.write(host, &rest, &resets).map(drop)
    }

    /// The items as they are now, kept.
    fn keep(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let items = crate::native_grid::copy_items(&laid.items)?;
        self.write(host, &items, &[]).map(drop)
    }

    /// Stages `items` as the page, and what the removed placements set reset.
    /// Laid out anew from the settings afterwards, which is the old layout where
    /// the change was too big to save; `false` comes back then.
    fn write(
        &mut self,
        host: &Host<'_>,
        items: &[crate::native_grid::Item],
        resets: &[String],
    ) -> Result<bool, Error> {
        let Some(laid) = self.laid.as_ref() else {
            return Ok(false);
        };
        let shown = Shown::current(host)?;
        let in_layout = laid.wide.then_some(&shown.frame.geometry);
        let writes = native_pages::store(&shown.src, laid.slot, items, in_layout)?;
        let saved = add_screen::apply(host, &writes, resets)?;
        self.stale = true;
        redraw(host)?;
        Ok(saved)
    }
}

/// The corner of `rect` a finger sizes it by: `side` square, at most half the
/// cell each way.
fn corner_rect(rect: Rect, side: u16) -> Rect {
    let width = side.min(rect.width / 2);
    let height = side.min(rect.height / 2);
    let side = width.min(height);
    Rect::new(
        rect.x.saturating_add(rect.width.saturating_sub(side)),
        rect.y.saturating_add(rect.height.saturating_sub(side)),
        side,
        side,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_are_offered_in_the_order_the_menu_has_its_rows() {
        let plain = choices(false);
        assert_eq!(plain.len(), 3);
        assert_eq!(plain[0].0, Choice::Move);
        assert_eq!(plain[2].0, Choice::Remove);
        assert_eq!(choices(true)[2].0, Choice::Settings);
    }
}
