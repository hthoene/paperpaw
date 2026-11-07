//! The to-do list's own screen: *Add* first, then the items; a tap ticks one
//! off or on, holding it (or the menu button) offers to edit, move or remove
//! it. The list is the file of [`crate::native_todo_list`]; the widgets showing
//! it are told when it changes.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered},
        formatting::Arg,
        ui::{Instance, Notify, Open, Question, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
    uitext,
};

use crate::native_plugin::{Asked, NativeWidgets};
use crate::native_todo_list::{List, Loaded, MAX_ITEMS, cleaned, load, save};

/// The callback token of the list's screen.
pub const SCREEN: u32 = crate::declarations::TODO_SCREEN;
/// The callback token of the action that opens it.
pub const OPEN: u32 = crate::declarations::TODO_OPEN;
/// The id of the screen.
const SCREEN_ID: &str = "widgets.todo.list";
/// The id of the action.
pub const OPEN_ACTION: &str = "widgets.todo.open";
/// The longest item the dialog takes.
const TEXT_LIMIT: usize = 4 * 1024;

/// The list on the screen.
#[derive(Debug, Default)]
pub struct TodoView {
    list: List,
    /// The list could not be read: it is shown empty and not changed.
    unreadable: bool,
    rows: ItemList,
    empty: (String, String),
}

/// What can be done with an item, in the order offered: its tick, edit, move
/// up, move down, remove.
#[derive(Debug, Clone, Copy)]
struct Options {
    names: [&'static str; 5],
    len: usize,
}

impl Options {
    fn new(done: bool, at: usize, count: usize) -> Self {
        let mut found = Self {
            names: [""; 5],
            len: 0,
        };
        found.add(if done { "untick" } else { "tick" });
        found.add("edit");
        if at > 0 {
            found.add("up");
        }
        if at + 1 < count {
            found.add("down");
        }
        found.add("remove");
        found
    }

    fn add(&mut self, name: &'static str) {
        if let Some(slot) = self.names.get_mut(self.len) {
            *slot = name;
            self.len += 1;
        }
    }

    fn all(&self) -> &[&'static str] {
        self.names.get(..self.len).unwrap_or(&[])
    }
}

/// Opens the host's `core.ui` for `f` and releases it after.
fn with_ui<R>(host: &Host<'_>, f: impl FnOnce(&Ui) -> Result<R, Error>) -> Result<R, Error> {
    let ui = Ui::open(host)?;
    let made = f(&ui);
    let released = ui.release(host);
    let made = made?;
    released?;
    Ok(made)
}

impl TodoView {
    fn rebuild(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(self.list.items.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        rows.push(Item::row(&messages.copy(host, "widgets.todo.add")?, "+")?);
        let done = messages.copy(host, "widgets.todo.state.done")?;
        for item in &self.list.items {
            let value = if item.done.is_some() { &done } else { "" };
            rows.push(Item::row(&item.text, value)?);
        }
        let message = if self.unreadable {
            "widgets.todo.unreadable"
        } else {
            "widgets.todo.empty"
        };
        self.empty = (
            messages.copy(host, "widgets.todo")?,
            messages.copy(host, message)?,
        );
        self.rows.replace(rows);
        Ok(())
    }
}

impl NativeWidgets {
    /// The action that opens the list's screen.
    pub(crate) fn open_todo_screen(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).open(Open::screen(SCREEN_ID)?)
    }

    /// The screen was opened: its list is read.
    pub(crate) fn todo_opened(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let today = crate::native_draw::epoch_day(host)?;
        let mut view = TodoView::default();
        match load(host, today)? {
            Loaded::List(list) => view.list = list,
            Loaded::Unreadable => view.unreadable = true,
        }
        view.rebuild(host)?;
        self.screens.open(instance, view)
    }

    /// Draws the screen on top.
    pub(crate) fn todo_draw(
        &mut self,
        host: &Host<'_>,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        let view = self.screens.top(SCREEN).ok_or(Error::State)?;
        let title = Messages::new()?.copy(host, "widgets.todo")?;
        let footer = if view.unreadable || view.list.items.is_empty() {
            Footer::Empty {
                title: &view.empty.0,
                text: &view.empty.1,
            }
        } else {
            Footer::None
        };
        view.rows.draw_with(host, &mut frame, &title, true, footer)
    }

    /// Input for the screen on top.
    pub(crate) fn todo_input(
        &mut self,
        host: &Host<'_>,
        input: Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(Handled::No);
        };
        // The menu button offers what else can be done with an item.
        if matches!(
            input,
            Input::Action {
                action: "core.menu",
                ..
            }
        ) {
            let focus = view.rows.focus();
            self.offer(host, focus)?;
            return Ok(Handled::Yes);
        }
        let step = view.rows.input(host, &input)?;
        match step {
            ItemStep::Chosen(row) => self.choose(host, row)?,
            ItemStep::Held(row) => self.offer(host, row)?,
            _ => {}
        }
        Ok(ItemList::handled(step))
    }

    fn notify(host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        let mut scratch = buffer(1024)?;
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    }

    fn redraw(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// Row `row` chosen: *Add* asks for an item, an item is ticked off or on.
    fn choose(&mut self, host: &Host<'_>, row: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        if view.unreadable {
            return Self::notify(host, "widgets.todo.unreadable");
        }
        if row == 0 {
            return self.ask_text(host, "widgets.todo.add", "", Asked::Add);
        }
        self.tick(host, row - 1)
    }

    /// What can be done with the item of `row`.
    fn offer(&mut self, host: &Host<'_>, row: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let Some(index) = row.checked_sub(1) else {
            return self.choose(host, row);
        };
        let Some(item) = view.list.items.get(index) else {
            return Ok(());
        };
        if view.unreadable {
            return Self::notify(host, "widgets.todo.unreadable");
        }
        let title = copy(&item.text)?;
        let wanted = Options::new(item.done.is_some(), index, view.list.items.len());
        let mut messages = Messages::new()?;
        let mut choices = String::new();
        for (n, option) in wanted.all().iter().enumerate() {
            let label = messages.copy(host, &concat(&["widgets.todo.option.", option])?)?;
            choices = concat(&[&choices, if n > 0 { "\n" } else { "" }, &label])?;
        }
        let question = Question::choice(&title, "", &choices)?;
        self.ask(host, question, Asked::Options { index, title })
    }

    /// Asks a question unless one is open: its keyboard or list is on screen
    /// and its answer is the one that counts.
    fn ask(&mut self, host: &Host<'_>, question: Question<'_>, asked: Asked) -> Result<(), Error> {
        if self.asked.is_some() {
            return Ok(());
        }
        let request = with_ui(host, |ui| {
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut out = buffer(SERVICE_BUFFER)?;
            ui.ask(host, question, &mut input, &mut out)
        })?;
        self.asked = Some((request, asked));
        Ok(())
    }

    /// Asks for a text on the keyboard (the title is a translation key), with
    /// `initial` to start with.
    fn ask_text(
        &mut self,
        host: &Host<'_>,
        title: &str,
        initial: &str,
        asked: Asked,
    ) -> Result<(), Error> {
        if self.asked.is_some() {
            return Ok(());
        }
        let title = Messages::new()?.copy(host, title)?;
        let request = with_ui(host, |ui| {
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut out = buffer(SERVICE_BUFFER)?;
            let request = ui.ask(host, Question::text(&title, "")?, &mut input, &mut out)?;
            if !initial.is_empty() {
                uitext::write_all(ui, host, request.text(), initial)?;
            }
            Ok(request)
        })?;
        self.asked = Some((request, asked));
        Ok(())
    }

    /// The answer to a question of the screen.
    pub(crate) fn todo_answered(
        &mut self,
        host: &Host<'_>,
        (request, asked): (RequestId, Asked),
        answer: Answered,
    ) -> Result<(), Error> {
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match asked {
            Asked::Add | Asked::Edit { .. } => {
                let typed = with_ui(host, |ui| {
                    uitext::read_all(ui, host, request.text(), TEXT_LIMIT)
                })?;
                let text = cleaned(&typed)?;
                if text.is_empty() {
                    return Ok(());
                }
                match asked {
                    Asked::Edit { index } => self.edit(host, index, text),
                    _ => self.add(host, text),
                }
            }
            Asked::Options { index, title } => {
                let Some(view) = self.screens.top(SCREEN) else {
                    return Ok(());
                };
                // Still the item asked about.
                let Some(item) = view.list.items.get(index).filter(|i| i.text == title) else {
                    return Ok(());
                };
                let wanted = Options::new(item.done.is_some(), index, view.list.items.len());
                match wanted.all().get(answer.choice() as usize).copied() {
                    Some("tick" | "untick") => self.tick(host, index),
                    Some("edit") => {
                        let initial = copy(&title)?;
                        self.ask_text(host, "widgets.todo.edit", &initial, Asked::Edit { index })
                    }
                    Some("up") => self.move_item(host, index, true),
                    Some("down") => self.move_item(host, index, false),
                    Some("remove") => self.remove(host, index),
                    _ => Ok(()),
                }
            }
            Asked::Setting(_) => Ok(()),
        }
    }

    /// Keeps the list on the card, rebuilds the screen and tells the widgets.
    fn changed(&mut self, host: &Host<'_>, focus: Option<usize>) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        if save(host, &view.list).is_err() {
            Self::notify(host, "widgets.todo.not-saved")?;
        }
        view.rebuild(host)?;
        if let Some(focus) = focus {
            view.rows.set_focus(focus);
        } else {
            let last = view.list.items.len();
            view.rows.set_focus(view.rows.focus().min(last));
        }
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in self
            .items
            .iter()
            .filter(|item| item.token == crate::native_todo::TOKEN)
        {
            commands.widget_changed(item.instance)?;
        }
        Self::redraw(host)
    }

    fn tick(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let today = crate::native_draw::epoch_day(host)?;
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let Some(item) = view.list.items.get_mut(index) else {
            return Ok(());
        };
        item.done = if item.done.is_some() {
            None
        } else {
            Some(today)
        };
        self.changed(host, None)
    }

    fn add(&mut self, host: &Host<'_>, text: String) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        if view.list.items.len() >= MAX_ITEMS {
            let count = i64::try_from(MAX_ITEMS).unwrap_or(i64::MAX);
            let full = Messages::new()?.format(
                host,
                "widgets.todo.full",
                &[Arg::Int {
                    name: "count",
                    value: count,
                }],
            )?;
            let mut scratch = buffer(1024)?;
            return CommandSender::new(host, &mut scratch).notify(Notify::new(&full)?);
        }
        push(
            &mut view.list.items,
            crate::native_todo_list::Item { text, done: None },
        )?;
        // The new item in sight.
        let last = view.list.items.len();
        self.changed(host, Some(last))
    }

    fn edit(&mut self, host: &Host<'_>, index: usize, text: String) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let Some(item) = view.list.items.get_mut(index) else {
            return Ok(());
        };
        item.text = text;
        self.changed(host, None)
    }

    fn move_item(&mut self, host: &Host<'_>, index: usize, up: bool) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let other = if up {
            index.checked_sub(1)
        } else {
            Some(index + 1).filter(|n| *n < view.list.items.len())
        };
        let Some(other) = other else {
            return Ok(());
        };
        view.list.items.swap(index, other);
        // The row of the item, after *Add*.
        self.changed(host, Some(other + 1))
    }

    fn remove(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        if index < view.list.items.len() {
            view.list.items.remove(index);
        }
        self.changed(host, None)
    }
}

#[cfg(test)]
mod tests {
    use super::Options;

    #[test]
    fn the_options_follow_the_place_and_state_of_the_item() {
        assert_eq!(Options::new(false, 0, 1).all(), ["tick", "edit", "remove"]);
        assert_eq!(
            Options::new(true, 0, 3).all(),
            ["untick", "edit", "down", "remove"]
        );
        assert_eq!(
            Options::new(false, 1, 3).all(),
            ["tick", "edit", "up", "down", "remove"]
        );
        assert_eq!(
            Options::new(false, 2, 3).all(),
            ["tick", "edit", "up", "remove"]
        );
    }
}
