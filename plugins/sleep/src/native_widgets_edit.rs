//! Editing a list of widgets shown above each other (the sleep screen's
//! `sleep.widgets`, the editor of every setting that names the slot
//! `core.edit-widgets`, #293): one screen, `sleep.edit-widgets`, in two modes.
//!
//! - **The list**: the widgets of the setting the screen was opened for
//!   (parameter `id`), then *Add a widget*. A widget's grip takes it along
//!   with a finger; choosing one asks what to do with it (move it up or down,
//!   its own settings where it has some, remove it). Every change is kept at
//!   once.
//! - **Adding** (parameter `add`): every widget not in the list yet (the
//!   spaces always), by their kind; choosing one adds it at the end and goes
//!   back.
//!
//! A setting that is not a list the registry knows is shown empty and never
//! written.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input, PointerPhase},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered},
        formatting::Arg,
        registry::{PageCursor, Registry, RegistrySettingKind},
        settings::SettingValue,
        ui::{HandoverId, Open, Param, Question, RequestId, Ui},
    },
    widgets::categories,
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer, push},
    style::setting_text,
    text::copy,
};

/// The screen, as declared.
pub const SCREEN: &str = "sleep.edit-widgets";
/// Parameters: the setting edited, and the adding mode.
const ID: &str = "id";
const ADD: &str = "add";
/// Widgets that may be in a list more than once: room, not a thing.
const SPACE: &str = "core.space";
const FILL: &str = "core.fill";
/// Pages of the registry read at most.
const MOST_PAGES: usize = 8;
/// Room for the commands of a change: a setting of a whole list.
const COMMANDS: usize = 2048;

/// A widget the registry knows: its id, name and kind.
#[derive(Debug)]
struct Known {
    id: String,
    title: String,
    kind: String,
}

/// A finger on a grip: its contact, the widget's place and where it lands.
#[derive(Debug, Clone, Copy)]
struct Drag {
    contact: u32,
    from: usize,
    to: usize,
}

/// What the screen shows.
#[derive(Debug)]
enum Mode {
    /// The list; the widget whose options are asked, with them.
    List {
        asked: Option<(RequestId, usize, Vec<&'static str>)>,
        drag: Option<Drag>,
    },
    /// What can be added, by id.
    Add(Vec<String>),
}

/// One open editor.
#[derive(Debug)]
pub struct Editor {
    /// The setting, where the registry knows it as a list.
    setting: Option<String>,
    title: String,
    /// The widgets of the setting, as it holds them now.
    widgets: Vec<String>,
    known: Vec<Known>,
    list: ItemList,
    mode: Mode,
}

/// The ids of a list setting's value, in their order.
fn ids(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|id| !id.is_empty())
}

/// The name of widget `id` among `known`; where none is known (its plugin is
/// not installed), that it is not, naming the plugin.
fn name_in(
    host: &Host<'_>,
    messages: &mut Messages,
    known: &[Known],
    id: &str,
) -> Result<String, Error> {
    if let Some(known) = known.iter().find(|k| k.id == base(id)) {
        return copy(&known.title);
    }
    let plugin = base(id).split_once('.').map_or(id, |(plugin, _)| plugin);
    let plugin = Arg::Text {
        name: "plugin",
        value: plugin,
    };
    messages.format(host, "sleep.widgets.missing", &[plugin])
}

/// What a widget's place says of it: the id before its placement.
fn base(id: &str) -> &str {
    id.split_once('@').map_or(id, |(base, _)| base)
}

fn repeatable(id: &str) -> bool {
    id == SPACE || id == FILL
}

fn redraw(host: &Host<'_>) -> Result<Handled, Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Fast)?;
    Ok(Handled::Yes)
}

impl Editor {
    /// The editor of the setting its hand-over names, in its mode.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals of the hand-over.
    #[inline(never)]
    pub fn open(
        ui: &Ui,
        host: &Host<'_>,
        registry: Option<&Registry>,
        handover: Option<HandoverId>,
    ) -> Result<Self, Error> {
        let (mut id, mut add) = (String::new(), false);
        if let Some(handover) = handover {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            for param in ui.params(host, handover, &mut input, &mut out)? {
                let param = param?;
                match param.key() {
                    ID => id = copy(param.value())?,
                    ADD => add = true,
                    _ => {}
                }
            }
        }
        let mut messages = Messages::new()?;
        let (setting, title) = match registry {
            Some(registry) => match list_setting(host, registry, &id)? {
                Some(title) => (Some(id), title),
                None => (None, messages.copy(host, "core.widgets.edit.screen")?),
            },
            None => (None, messages.copy(host, "core.widgets.edit.screen")?),
        };
        let known = match registry {
            Some(registry) => known(host, registry, &mut messages)?,
            None => Vec::new(),
        };
        let mut editor = Self {
            setting,
            title,
            widgets: Vec::new(),
            known,
            list: ItemList::default(),
            mode: Mode::List {
                asked: None,
                drag: None,
            },
        };
        editor.read(host)?;
        if add {
            editor.title = messages.copy(host, "core.widgets.edit.add")?;
            editor.mode = Mode::Add(Vec::new());
        }
        editor.rows(host, &mut messages)?;
        Ok(editor)
    }

    /// The setting's widgets, read again.
    fn read(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.widgets.clear();
        if let Some(setting) = &self.setting {
            let value = match setting_text(host, setting) {
                Ok(value) => value,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => String::new(),
            };
            for id in ids(&value) {
                push(&mut self.widgets, copy(id)?)?;
            }
        }
        Ok(())
    }

    /// The rows of the mode, the focus kept where it was.
    fn rows(&mut self, host: &Host<'_>, messages: &mut Messages) -> Result<(), Error> {
        let mut items = Vec::new();
        match &mut self.mode {
            Mode::List { drag, .. } => {
                for id in &self.widgets {
                    let name = name_in(host, messages, &self.known, id)?;
                    push(&mut items, Item::row(&name, "")?)?;
                }
                let add = messages.copy(host, "core.widgets.edit.add")?;
                push(&mut items, Item::row(&add, "›")?.with_icon(host, "plus")?)?;
                self.list.replace(items);
                self.list.set_grips(self.widgets.len());
                self.list.set_drag(drag.map(|d| (d.from, d.to)));
            }
            Mode::Add(offered) => {
                offered.clear();
                // By their kind, as `categories::ALL` orders them, then the
                // kinds of plugins' own; the spaces, which are room, after the
                // widgets that show something. In passes, not sorted: no sort's
                // frame on the device's stack.
                let rank = |known: &Known| {
                    categories::ALL
                        .iter()
                        .position(|k| *k == known.kind)
                        .unwrap_or(categories::ALL.len())
                };
                for room in [false, true] {
                    for wanted in 0..=categories::ALL.len() {
                        for known in &self.known {
                            let shown = self.widgets.iter().any(|w| base(w) == known.id);
                            let wanted_here = (repeatable(&known.id) || !shown)
                                && repeatable(&known.id) == room
                                && rank(known) == wanted;
                            if wanted_here {
                                let kind = messages.copy(host, &known.kind)?;
                                push(&mut items, Item::row(&known.title, &kind)?)?;
                                push(offered, copy(&known.id)?)?;
                            }
                        }
                    }
                }
                self.list.replace(items);
            }
        }
        Ok(())
    }

    /// Draws the rows under the title; adding with nothing left says so.
    ///
    /// # Errors
    /// Allocation, font, catalog and drawing errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let footer = match &self.mode {
            Mode::Add(offered) if offered.is_empty() => {
                let all = Messages::new()?.copy(host, "core.widgets.edit.all-shown")?;
                let footer = Footer::Empty {
                    title: &all,
                    text: "",
                };
                return self.list.draw_with(host, frame, &self.title, true, footer);
            }
            _ => Footer::None,
        };
        self.list.draw_with(host, frame, &self.title, true, footer)
    }

    /// The screen shows again (back from adding): the list read again.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and catalog errors.
    pub fn shown(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.read(host)?;
        self.rows(host, &mut Messages::new()?)
    }

    /// The list as it is, to change and [`store`](Self::store): the list
    /// itself stays until the new one is written.
    fn copied(&self) -> Result<Vec<String>, Error> {
        let mut widgets = Vec::new();
        widgets
            .try_reserve_exact(self.widgets.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        for id in &self.widgets {
            widgets.push(copy(id)?);
        }
        Ok(widgets)
    }

    /// Writes `widgets` as the setting's list (nothing where it is none).
    fn store(&mut self, host: &Host<'_>, widgets: Vec<String>, close: bool) -> Result<(), Error> {
        let mut scratch = buffer(COMMANDS)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        if let Some(setting) = &self.setting {
            let mut text = String::new();
            for (n, id) in widgets.iter().enumerate() {
                if n > 0 {
                    text.push_str(", ");
                }
                text.push_str(id);
            }
            commands.set_setting(setting, SettingValue::Text(&text))?;
            self.widgets = widgets;
        }
        if close {
            commands.back()?;
        }
        Ok(())
    }

    /// Input: the grips, then the rows.
    ///
    /// # Errors
    /// Allocation and the host's refusals of the commands.
    #[inline(never)]
    pub fn input(
        &mut self,
        ui: &Ui,
        host: &Host<'_>,
        registry: Option<&Registry>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        if let Input::Pointer {
            position,
            phase,
            contact,
            ..
        } = *input
            && let Some(handled) = self.pointer(host, position, phase, contact)?
        {
            return Ok(handled);
        }
        let step = self.list.input(host, input)?;
        let ItemStep::Chosen(index) = step else {
            return Ok(ItemList::handled(step));
        };
        match &self.mode {
            Mode::Add(offered) => {
                let Some(id) = offered.get(index) else {
                    return Ok(Handled::Yes);
                };
                let mut widgets = self.copied()?;
                push(&mut widgets, copy(id)?)?;
                self.store(host, widgets, true)?;
            }
            Mode::List { .. } if index >= self.widgets.len() => self.open_adding(ui, host)?,
            Mode::List { .. } => self.ask(ui, host, registry, index)?,
        }
        Ok(Handled::Yes)
    }

    /// A finger on a widget's grip takes it along; where it is lifted it goes.
    fn pointer(
        &mut self,
        host: &Host<'_>,
        position: Point,
        phase: PointerPhase,
        contact: u32,
    ) -> Result<Option<Handled>, Error> {
        let Mode::List { drag, .. } = &mut self.mode else {
            return Ok(None);
        };
        match (phase, *drag) {
            (PointerPhase::Down, _) => {
                let Some(row) = self
                    .list
                    .grip_at(position)
                    .filter(|r| *r < self.widgets.len())
                else {
                    *drag = None;
                    return Ok(None);
                };
                *drag = Some(Drag {
                    contact,
                    from: row,
                    to: row,
                });
            }
            (PointerPhase::Move, Some(mut held)) if held.contact == contact => {
                let to = self
                    .list
                    .drop_row(position.y)
                    .unwrap_or(held.to)
                    .min(self.widgets.len().saturating_sub(1));
                if to == held.to {
                    return Ok(Some(Handled::Yes));
                }
                held.to = to;
                *drag = Some(held);
                self.list.set_drag(Some((held.from, held.to)));
            }
            (PointerPhase::Up, Some(held)) if held.contact == contact => {
                *drag = None;
                self.list.set_drag(None);
                let to = self
                    .list
                    .drop_row(position.y)
                    .unwrap_or(held.to)
                    .min(self.widgets.len().saturating_sub(1));
                if to != held.from {
                    let mut widgets = self.copied()?;
                    let moved = widgets.remove(held.from);
                    widgets.insert(to, moved);
                    self.store(host, widgets, false)?;
                    self.list.set_focus(to);
                    self.rows(host, &mut Messages::new()?)?;
                }
            }
            (PointerPhase::Cancel, Some(held)) if held.contact == contact => {
                *drag = None;
                self.list.set_drag(None);
            }
            _ => return Ok(None),
        }
        redraw(host).map(Some)
    }

    /// *Add a widget*: this screen again, adding.
    fn open_adding(&self, ui: &Ui, host: &Host<'_>) -> Result<(), Error> {
        let id = self.setting.as_deref().unwrap_or("");
        let params = [Param::new(ID, id)?, Param::new(ADD, "1")?];
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let handover = ui.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::screen(SCREEN)?.handover(handover))
    }

    /// Asks what to do with the widget at `at`.
    fn ask(
        &mut self,
        ui: &Ui,
        host: &Host<'_>,
        registry: Option<&Registry>,
        at: usize,
    ) -> Result<(), Error> {
        let Some(id) = self.widgets.get(at) else {
            return Ok(());
        };
        let mut options: Vec<&'static str> = Vec::new();
        if at > 0 {
            push(&mut options, "up")?;
        }
        if at + 1 < self.widgets.len() {
            push(&mut options, "down")?;
        }
        if let Some(registry) = registry
            && has_settings(host, registry, base(id))?
        {
            push(&mut options, "settings")?;
        }
        push(&mut options, "remove")?;
        let mut messages = Messages::new()?;
        let mut choices = String::new();
        for option in &options {
            if !choices.is_empty() {
                choices.push('\n');
            }
            let mut key = String::from("core.widgets.edit.");
            key.push_str(option);
            choices.push_str(&messages.copy(host, &key)?);
        }
        let title = name_in(host, &mut messages, &self.known, id)?;
        let question = Question::choice(&title, "", &choices)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = ui.ask(host, question, &mut input, &mut out)?;
        if let Mode::List { asked, .. } = &mut self.mode {
            *asked = Some((request, at, options));
        }
        Ok(())
    }

    /// The answer to what to do with a widget: done, and kept.
    ///
    /// # Errors
    /// Allocation and the host's refusals of the commands.
    pub fn answered(&mut self, ui: &Ui, host: &Host<'_>, answer: &Answered) -> Result<bool, Error> {
        let Mode::List { asked, .. } = &mut self.mode else {
            return Ok(false);
        };
        let Some((request, at, options)) = asked.take_if(|(r, ..)| *r == answer.request()) else {
            return Ok(false);
        };
        if answer.outcome() == AnswerOutcome::Typing {
            *asked = Some((request, at, options));
            return Ok(true);
        }
        if answer.outcome() != AnswerOutcome::Answered || at >= self.widgets.len() {
            return Ok(true);
        }
        let Some(option) = options.get(answer.choice() as usize).copied() else {
            return Ok(true);
        };
        let mut focus = at;
        if !matches!(option, "up" | "down" | "remove") {
            // Its own settings, on their page.
            let params = [Param::new(ID, &self.widgets[at])?];
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            let handover = ui.handover(host, &params, &mut input, &mut out)?;
            let mut scratch = [0; 256];
            CommandSender::new(host, &mut scratch)
                .open(Open::slot("core.settings")?.handover(handover))?;
            return Ok(true);
        }
        let mut widgets = self.copied()?;
        match option {
            "up" if at > 0 => {
                widgets.swap(at, at - 1);
                focus = at - 1;
            }
            "down" if at + 1 < widgets.len() => {
                widgets.swap(at, at + 1);
                focus = at + 1;
            }
            "remove" => {
                widgets.remove(at);
            }
            // The list read again meanwhile has no row to move it to.
            _ => return Ok(true),
        }
        self.store(host, widgets, false)?;
        self.list.set_focus(focus);
        self.rows(host, &mut Messages::new()?)?;
        redraw(host)?;
        Ok(true)
    }
}

/// The title of `id` where the registry knows it as a list setting.
fn list_setting(host: &Host<'_>, registry: &Registry, id: &str) -> Result<Option<String>, Error> {
    let Some((plugin, _)) = id.split_once('.') else {
        return Ok(None);
    };
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_PAGES {
        let page = match registry.settings(host, plugin, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(None),
        };
        for row in page.rows() {
            if row.key() == id {
                return if row.kind() == RegistrySettingKind::List {
                    copy(row.title()).map(Some)
                } else {
                    Ok(None)
                };
            }
        }
        match page.end().next_cursor() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(None)
}

/// Every widget the registry knows, in its order.
fn known(
    host: &Host<'_>,
    registry: &Registry,
    messages: &mut Messages,
) -> Result<Vec<Known>, Error> {
    let _ = messages;
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut found = Vec::new();
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_PAGES {
        let page = match registry.widgets(host, "", cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => break,
        };
        for row in page.rows() {
            push(
                &mut found,
                Known {
                    id: copy(row.id())?,
                    title: copy(row.title())?,
                    kind: copy(row.kind())?,
                },
            )?;
        }
        match page.end().next_cursor() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(found)
}

/// Whether widget `id` has settings of its own (a settings group of its id
/// with a setting in it).
fn has_settings(host: &Host<'_>, registry: &Registry, id: &str) -> Result<bool, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    match registry.settings(host, id, PageCursor::start(), &mut input, &mut out) {
        Ok(page) => Ok(page.rows().next().is_some()),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(false),
    }
}
