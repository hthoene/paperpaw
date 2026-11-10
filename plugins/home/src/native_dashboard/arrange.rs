//! The dashboard while editing: the options of an item (a dialog over the
//! page: move, change the size, remove, cancel), and an item the buttons are
//! moving or sizing. What the buttons do to cells is `native_arrange` without
//! the host; this file holds the state, draws the dialog, and writes the page
//! once an item is set down or removed.
//!
//! The finger's drag of an item or its corner waits for H4 (an ignored
//! acceptance case); a tap on an item offers its options.
use paperpaw_ui_kit::dialog::Dialog;

use super::*;
use crate::native_add_screen as add_screen;
use crate::native_arrange::{self as arrange, Choice, Dir, Mode, Put, choices};
use crate::native_item_settings as item_settings;

/// The options of the item of a cell: which button has the focus, and the
/// dialog (made at the first draw: it needs the font).
#[derive(Debug)]
pub(super) struct Options {
    cell: usize,
    focus: usize,
    dialog: Option<Dialog>,
    /// The item is a widget that has settings of its own: *Settings* is offered.
    settings: bool,
}

/// An item the buttons are moving or sizing.
#[derive(Debug, Clone, Copy)]
pub(super) struct Moving {
    cell: usize,
    mode: Mode,
}

impl Dashboard {
    /// The options of the item of `cell` open.
    pub(super) fn open_options(&mut self, host: &Host<'_>, cell: usize) -> Result<(), Error> {
        let settings = match self.widget_at(cell) {
            Some(id) => item_settings::has_settings(host, id)?,
            None => false,
        };
        self.options = Some(Options {
            cell,
            focus: 0,
            dialog: None,
            settings,
        });
        redraw(host)
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

    /// Makes the options' dialog where it is not made yet.
    pub(super) fn ensure_dialog(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let (Some(options), Some(laid)) = (self.options.as_mut(), self.laid.as_ref()) else {
            return Ok(());
        };
        if options.dialog.is_some() {
            return Ok(());
        }
        let title = laid.cells.get(options.cell).map_or("", |c| c.name.as_str());
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let made = Self::make_dialog(host, &display, &font, title, options.settings);
        let released = font.release();
        let dialog = made?;
        released?;
        options.dialog = Some(dialog);
        Ok(())
    }

    fn make_dialog(
        host: &Host<'_>,
        display: &snapshot::Snapshot,
        font: &Font<'_>,
        title: &str,
        settings: bool,
    ) -> Result<Dialog, Error> {
        let style = Style::read(host, display, font)?;
        let mut messages = Messages::new()?;
        let mut words: Vec<String> = Vec::new();
        words
            .try_reserve_exact(choices(settings).len())
            .map_err(|_| Error::NoMemory)?;
        for (_, key) in choices(settings) {
            words.push(messages.copy(host, key)?);
        }
        let mut labels: Vec<&str> = Vec::new();
        labels
            .try_reserve_exact(words.len())
            .map_err(|_| Error::NoMemory)?;
        labels.extend(words.iter().map(String::as_str));
        let (width, height) = (display.display_width(), display.display_height());
        let mut dialog = Dialog::new(font, &style, (width, height), title, &[], &labels)?;
        let at = height.saturating_sub(dialog.height()) / 2;
        dialog.place(at)?;
        Ok(dialog)
    }

    /// The options' dialog as drawn operations, over the page.
    pub(super) fn dialog_paint(&self, font: &Font<'_>, out: &mut Vec<Paint>) -> Result<(), Error> {
        if let Some(Options {
            dialog: Some(dialog),
            focus,
            ..
        }) = &self.options
        {
            dialog.prepare(out, font, Some(*focus))?;
        }
        Ok(())
    }

    /// An input while the options show or an item is moved or sized; `None` if
    /// neither, then the page takes it.
    pub(super) fn arrange_input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        if self.options.is_some() {
            return self.options_input(host, ui, input).map(Some);
        }
        if let Some(moving) = self.moving {
            return self.moving_input(host, ui, moving, input).map(Some);
        }
        Ok(None)
    }

    fn options_input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        self.ensure_dialog(host)?;
        let Some(options) = self.options.as_mut() else {
            return Ok(Handled::No);
        };
        let chosen = match *input {
            Input::Action {
                action: "core.back",
                ..
            } => Some(Choice::Cancel),
            Input::Action {
                action: "core.confirm",
                ..
            } => choices(options.settings)
                .get(options.focus)
                .map(|(choice, _)| *choice),
            Input::Action { action, .. } => {
                let to = options
                    .dialog
                    .as_ref()
                    .and_then(|dialog| dialog.step(options.focus, action));
                if let Some(to) = to.filter(|to| *to != options.focus) {
                    options.focus = to;
                    redraw(host)?;
                }
                None
            }
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => options
                .dialog
                .as_ref()
                .and_then(|dialog| dialog.hit(position))
                .and_then(|button| choices(options.settings).get(button))
                .map(|(choice, _)| *choice),
            // The dialog is modal: everything else is taken.
            _ => None,
        };
        let cell = options.cell;
        if let Some(choice) = chosen {
            self.choose(host, ui, cell, choice)?;
        }
        Ok(Handled::Yes)
    }

    fn choose(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        cell: usize,
        choice: Choice,
    ) -> Result<(), Error> {
        self.options = None;
        match choice {
            Choice::Cancel => redraw(host),
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
            items[item].what = What::Widget(paperpaw_ui_kit::text::copy(&placed)?);
            self.write(host, &items, &[])?;
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
        let Some(widget) = cell.widget.filter(|w: &WidgetHandle| !w.is_own()) else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        ui.widget_place(
            host,
            widget,
            paint::frame_rect(cell.rect),
            &mut input,
            &mut out,
        )
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
        self.write(host, &rest, &resets)
    }

    /// The items as they are now, kept.
    fn keep(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let items = crate::native_grid::copy_items(&laid.items)?;
        self.write(host, &items, &[])
    }

    /// Stages `items` as the page, and what the removed placements set reset.
    /// Laid out anew from the settings afterwards, which is the old layout where
    /// the change was too big to save.
    fn write(
        &mut self,
        host: &Host<'_>,
        items: &[crate::native_grid::Item],
        resets: &[String],
    ) -> Result<(), Error> {
        let Some(laid) = self.laid.as_ref() else {
            return Ok(());
        };
        let shown = Shown::current(host)?;
        let in_layout = laid.wide.then_some(&shown.frame.geometry);
        let writes = native_pages::store(&shown.src, laid.slot, items, in_layout)?;
        add_screen::apply(host, &writes, resets)?;
        self.stale = true;
        redraw(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_are_offered_in_the_order_the_dialog_has_its_buttons() {
        let plain = choices(false);
        assert_eq!(plain.len(), 4);
        assert_eq!(plain[0].0, Choice::Move);
        assert_eq!(plain[3].0, Choice::Cancel);
        assert_eq!(choices(true)[2].0, Choice::Settings);
    }
}
