//! The screen of the status items editor: the rows of a [`Chooser`] in an
//! [`ItemList`], and what taps, holds and buttons make of them. A plugin names
//! a screen of its own, made of this, as the editor of a setting that lists
//! status items or lines (`EditorDecl::Screen`); it opens the view when the
//! screen opens, hands it the inputs and draws it, and sets the setting
//! when the view says it changed.
//!
//! What the interface-6 screen did with a pointer is done by holding, as the
//! kit has no drag: a long press on a shown item picks it up, and a tap on a
//! shown item then puts it there (a tap elsewhere puts it down). With buttons
//! it is as before: *Menu* picks the focused shown item up, the direction and
//! page buttons move it a place, *Confirm*, *Back* or *Menu* put it down.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input, InputGestureKind, PointerPhase},
        commands::{CommandSender, RedrawMode},
        registry::{PageCursor, Registry, StatusScope},
    },
};

use super::{Chooser, Entry, Outcome, Snapshot};
use crate::{Footer, Item, ItemList, ItemStep, Messages, mem, text};

/// Entries read from the registry at most.
pub const MAX_ENTRIES: usize = 128;

/// The words the screen uses, as the user's language has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texts {
    /// Value of a shown item (`core.state.on`).
    pub on: String,
    /// Value of a hidden item (`core.state.off`).
    pub off: String,
    /// Value of the item being moved (`core.move`).
    pub moving: String,
    /// What a tap and a hold do (`core.status-items.hint`).
    pub hint: String,
    /// What is done while an item is picked up (`core.status-items.moving`).
    pub moving_hint: String,
}

impl Texts {
    /// The texts of the core catalog, read through `host`.
    ///
    /// # Errors
    /// Catalog and allocation errors.
    pub fn read(host: &Host<'_>) -> Result<Self, Error> {
        let mut messages = Messages::new()?;
        Ok(Self {
            on: messages.copy(host, "core.state.on")?,
            off: messages.copy(host, "core.state.off")?,
            moving: messages.copy(host, "core.move")?,
            hint: messages.copy(host, "core.status-items.hint")?,
            moving_hint: messages.copy(host, "core.status-items.moving")?,
        })
    }
}

/// What an input did to the view.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Not for the view (the screen goes back for *Back*).
    Ignored,
    /// Taken; the screen is drawn again.
    Redraw,
    /// Taken, nothing to draw (a finger moved within the place it was at).
    Taken,
    /// The setting changed to this text.
    Changed(String),
    /// The header's action should run (back, previous or next page).
    Header(&'static str),
}

/// A step taken in the model whose rows are not in the list yet.
struct Pending {
    step: Step,
    items: Option<Vec<Item>>,
    focus: usize,
    undo: Undo,
}

/// What puts the view back as it was.
struct Undo {
    chooser: Snapshot,
    focus: usize,
}

/// The editor screen's state.
#[derive(Debug)]
pub struct StatusItemsView {
    chooser: Chooser,
    list: ItemList,
    title: String,
    texts: Texts,
    /// The shown item a finger has taken along by its grip.
    drag: Option<Drag>,
}

/// A finger on a grip: its contact, the item's place and where it would land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    contact: u32,
    from: usize,
    to: usize,
}

impl StatusItemsView {
    /// A view of `entries` for the setting whose text is `setting`, under
    /// `title`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(
        entries: Vec<Entry>,
        setting: &str,
        title: String,
        texts: Texts,
    ) -> Result<Self, Error> {
        let chooser = Chooser::new(entries, setting)?;
        let mut view = Self {
            chooser,
            list: ItemList::default(),
            title,
            texts,
            drag: None,
        };
        let items = view.build()?;
        view.list.replace(items);
        Ok(view)
    }

    /// The setting's text now.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn value(&self) -> Result<String, Error> {
        self.chooser.value()
    }

    /// The model.
    #[must_use]
    pub const fn chooser(&self) -> &Chooser {
        &self.chooser
    }

    /// The list.
    #[must_use]
    pub const fn list(&self) -> &ItemList {
        &self.list
    }

    /// The rows from the model: the name of each entry and whether, and as
    /// which, it is shown. The list is not touched.
    fn build(&self) -> Result<Vec<Item>, Error> {
        let rows = self.chooser.rows()?;
        let shown = self.chooser.chosen()?.len();
        let focus = self.list.focus();
        let mut items = Vec::new();
        items
            .try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for (row, entry) in rows.iter().enumerate() {
            let name = self
                .chooser
                .entries()
                .get(*entry)
                .map_or("", |e| e.title.as_str());
            let value = if row >= shown {
                text::copy(&self.texts.off)?
            } else {
                let mut digits = [0; 20];
                let place = text::decimal(row + 1, &mut digits);
                let word = if self.chooser.moving() && row == focus {
                    &self.texts.moving
                } else {
                    &self.texts.on
                };
                // Shown items carry their place: "On (2)".
                if shown > 1 {
                    text::concat(&[word, " (", place, ")"])?
                } else {
                    text::copy(word)?
                }
            };
            mem::push(&mut items, Item::row(name, &value)?)?;
        }
        Ok(items)
    }

    /// What `input` means to the view, with the model and the rows changed
    /// as it says; nothing is sent to the host.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the view is as it was.
    pub fn step(&mut self, input: &Input<'_>) -> Result<Step, Error> {
        let pending = self.prepare(input)?;
        Ok(self.commit(pending))
    }

    /// The model is changed as `input` says and the new rows are made, but
    /// the list keeps its old ones until [`Self::commit`]: what the host
    /// refuses in between is [`Self::undo`]ne.
    fn prepare(&mut self, input: &Input<'_>) -> Result<Pending, Error> {
        let undo = Undo {
            chooser: self.chooser.snapshot()?,
            focus: self.list.focus(),
        };
        let made = self.apply(input);
        match made {
            Ok((step, items)) => Ok(Pending {
                step,
                items,
                focus: self.list.focus(),
                undo,
            }),
            Err(error) => {
                self.undo(undo);
                Err(error)
            }
        }
    }

    fn apply(&mut self, input: &Input<'_>) -> Result<(Step, Option<Vec<Item>>), Error> {
        let step = match *input {
            Input::Action { action, .. } => self.action(input, action)?,
            Input::Gesture { kind, .. } => self.gesture(input, kind)?,
            Input::Pointer {
                position,
                phase,
                contact,
                ..
            } => self.pointer(position, phase, contact)?,
            _ => Step::Ignored,
        };
        let items = match step {
            Step::Ignored | Step::Taken => None,
            _ => Some(self.build()?),
        };
        Ok((step, items))
    }

    fn commit(&mut self, pending: Pending) -> Step {
        if let Some(items) = pending.items {
            self.list.replace(items);
            self.list.set_focus(pending.focus);
        }
        pending.step
    }

    fn undo(&mut self, undo: Undo) {
        self.chooser.restore(undo.chooser);
        self.list.set_focus(undo.focus);
    }

    fn action(&mut self, input: &Input<'_>, action: &str) -> Result<Step, Error> {
        let focus = self.list.focus();
        match self.chooser.action(action, focus, true)? {
            Outcome::Changed(to) => {
                self.list.set_focus(to);
                return Ok(Step::Changed(self.chooser.value()?));
            }
            Outcome::Redraw => return Ok(Step::Redraw),
            Outcome::Ignored => {}
        }
        // Not the chooser's: the list moves its focus.
        match self.list.decide(input) {
            ItemStep::Focus(to) => {
                self.list.set_focus(to);
                Ok(Step::Redraw)
            }
            _ => Ok(Step::Ignored),
        }
    }

    /// A finger on the grip of a shown item takes it along: a bar shows where it
    /// lands, lifting the finger puts it there. Only a contact that comes down
    /// on a grip is taken (the host then gives the view its moves and its end);
    /// the others are left to the gestures.
    fn pointer(
        &mut self,
        position: Point,
        phase: PointerPhase,
        contact: u32,
    ) -> Result<Step, Error> {
        match (phase, self.drag) {
            // The same finger down again: its end got lost, so it starts afresh.
            (PointerPhase::Down, held) if held.is_none_or(|d| d.contact == contact) => {
                self.drag = None;
                let shown = self.chooser.chosen()?.len();
                let row = self.list.grip_at(position).filter(|row| *row < shown);
                let Some(row) = row.filter(|_| shown > 1) else {
                    return Ok(Step::Ignored);
                };
                // A finger takes over from the buttons.
                self.chooser.put_down();
                self.list.set_focus(row);
                self.drag = Some(Drag {
                    contact,
                    from: row,
                    to: row,
                });
                Ok(Step::Redraw)
            }
            (PointerPhase::Move, Some(mut drag)) if drag.contact == contact => {
                let to = self.list.drop_row(position.y).unwrap_or(drag.to);
                if to == drag.to {
                    return Ok(Step::Taken);
                }
                drag.to = to;
                self.drag = Some(drag);
                Ok(Step::Redraw)
            }
            (PointerPhase::Up, Some(drag)) if drag.contact == contact => {
                self.drag = None;
                let to = self.list.drop_row(position.y).unwrap_or(drag.to);
                if self.chooser.move_to(drag.from, to)? {
                    self.list.set_focus(to);
                    return Ok(Step::Changed(self.chooser.value()?));
                }
                Ok(Step::Redraw)
            }
            (PointerPhase::Cancel, Some(drag)) if drag.contact == contact => {
                self.drag = None;
                Ok(Step::Redraw)
            }
            _ => Ok(Step::Ignored),
        }
    }

    fn gesture(&mut self, input: &Input<'_>, kind: InputGestureKind) -> Result<Step, Error> {
        let shown = self.chooser.chosen()?.len();
        match (kind, self.list.decide(input)) {
            // A tap anywhere but on a row puts a picked-up item down, as on
            // interface 6 (a header's action still runs).
            (InputGestureKind::Tap, step)
                if self.chooser.moving() && !matches!(step, ItemStep::Chosen(_)) =>
            {
                self.chooser.put_down();
                Ok(match step {
                    ItemStep::Header(action) => Step::Header(action),
                    _ => Step::Redraw,
                })
            }
            (_, ItemStep::Header(action)) => Ok(Step::Header(action)),
            // A hold picks a shown item up, if another is shown to move it past.
            (InputGestureKind::LongPress, ItemStep::Held(row)) => {
                if row < shown && shown > 1 {
                    self.list.set_focus(row);
                    self.chooser.pick_up();
                    Ok(Step::Redraw)
                } else {
                    Ok(Step::Ignored)
                }
            }
            (InputGestureKind::Tap, ItemStep::Chosen(row)) => {
                if self.chooser.moving() {
                    // Put down where it is tapped, or where it was.
                    let from = self.list.focus();
                    self.chooser.put_down();
                    if row < shown && self.chooser.move_to(from, row)? {
                        self.list.set_focus(row);
                        return Ok(Step::Changed(self.chooser.value()?));
                    }
                    return Ok(Step::Redraw);
                }
                self.list.set_focus(row);
                if self.chooser.toggle(row)? {
                    Ok(Step::Changed(self.chooser.value()?))
                } else {
                    Ok(Step::Redraw)
                }
            }
            _ => Ok(Step::Ignored),
        }
    }

    /// Takes `input`: the setting `setting` is set and the screen redrawn as
    /// the step says.
    ///
    /// # Errors
    /// Allocation, command queueing and host errors.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        setting: &str,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(step) = self.run(input, |step| Self::send(host, setting, step))? else {
            return Ok(Handled::No);
        };
        if !matches!(step, Step::Header(_) | Step::Taken) {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        }
        Ok(Handled::Yes)
    }

    /// As [`Self::step`], with the host's part (`send`) done before the rows
    /// replace the old ones: what the host refuses leaves the model, the
    /// rows and the setting as they were. `None` where the view did not take
    /// the input.
    fn run(
        &mut self,
        input: &Input<'_>,
        send: impl FnOnce(&Step) -> Result<(), Error>,
    ) -> Result<Option<Step>, Error> {
        let pending = self.prepare(input)?;
        if matches!(pending.step, Step::Ignored) {
            return Ok(None);
        }
        if let Err(error) = send(&pending.step) {
            self.undo(pending.undo);
            return Err(error);
        }
        Ok(Some(self.commit(pending)))
    }

    fn send(host: &Host<'_>, setting: &str, step: &Step) -> Result<(), Error> {
        match step {
            Step::Header(action) => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(action)
            }
            Step::Changed(value) => {
                let mut scratch = mem::buffer(512 + value.len())?;
                CommandSender::new(host, &mut scratch).set_setting(
                    setting,
                    paperpaw_sdk::native::settings::SettingValue::Text(value),
                )
            }
            Step::Redraw | Step::Taken | Step::Ignored => Ok(()),
        }
    }

    /// Draws the page the focus is on.
    ///
    /// # Errors
    /// Propagates allocation, font, catalog and drawing errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let hint = if self.chooser.moving() {
            &self.texts.moving_hint
        } else {
            &self.texts.hint
        };
        // Shown items carry a grip where another is shown to move them past.
        let shown = self.chooser.chosen()?.len();
        self.list.set_grips(if shown > 1 { shown } else { 0 });
        self.list.set_drag(self.drag.map(|d| (d.from, d.to)));
        self.list
            .draw_with(host, frame, &self.title, true, Footer::Note(hint))
    }
}

/// The items or lines that are registered, read from the registry (the plugin
/// declares `core.registry` among its uses): their ids and names in the user's
/// language. A line by an id it had before is not known to the registry, so
/// such an id in a setting stays where it is as one of an absent plugin.
///
/// # Errors
/// Registry, allocation and decoding errors; a page whose generation went
/// stale is read again from the start.
pub fn read_entries(host: &Host<'_>, scope: StatusScope) -> Result<Vec<Entry>, Error> {
    let registry = Registry::open(host)?;
    let found = read_pages(&registry, host, scope);
    let released = registry.release(host);
    let found = found?;
    released?;
    Ok(found)
}

fn read_pages(
    registry: &Registry,
    host: &Host<'_>,
    scope: StatusScope,
) -> Result<Vec<Entry>, Error> {
    let mut input = mem::buffer(256)?;
    let mut out = mem::buffer(mem::SERVICE_BUFFER)?;
    for _ in 0..3 {
        let mut entries = Vec::new();
        let mut cursor = PageCursor::start();
        let result = loop {
            match registry.status(host, scope, cursor, &mut input, &mut out) {
                Ok(page) => {
                    for row in page.rows().take(MAX_ENTRIES.saturating_sub(entries.len())) {
                        let entry = Entry {
                            id: text::copy(row.id())?,
                            title: text::copy(row.title())?,
                            also: None,
                        };
                        mem::push(&mut entries, entry)?;
                    }
                    match page.end().next_cursor() {
                        Some(next) if entries.len() < MAX_ENTRIES => cursor = next,
                        _ => break Ok(()),
                    }
                }
                Err(error) => break Err(error),
            }
        };
        match result {
            Ok(()) => return Ok(entries),
            // The registry changed while it was read: again from the start.
            Err(Error::State) => {}
            Err(error) => return Err(error),
        }
    }
    Err(Error::State)
}

#[cfg(test)]
mod tests;
