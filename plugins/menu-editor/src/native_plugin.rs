//! The flat menu editor: four screens over the registry and typed commands.
//!
//! Views own copies of what they show; fonts, canvas and registry pages never
//! outlive their callback. The host stays authoritative: a rejected edit leaves
//! the old view in place until the registry publishes the new menu.
use crate::native_menu::{Candidate, ListRenderer, MenuGraph, ScreenState};
use alloc::{format, string::String, vec::Vec};
use paperpaw_ui_kit::{
    ListFocus, ListInput, Messages, Style, focus,
    list::row as prepare_row,
    paint::{self, Paint},
    text,
};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::{CommandSender, RedrawMode},
        events::Event,
        fonts::{Font, FontRole},
        registrations,
        registry::Registry,
        reply::Reply,
        snapshot,
        ui::{Instance, Ui},
    },
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/menu_editor_declarations.rs"));
}

/// Callback tokens, as declared.
const EVENTS: u32 = 2;
const OPEN: u32 = 1;
const MENUS: u32 = 10;
const ENTRIES: u32 = 11;
const ENTRY: u32 = 12;
const ADD: u32 = 13;

const REGISTRY_INPUT: usize = 512;
const REGISTRY_OUTPUT: usize = 4096;
const COMMANDS: usize = 4096;
const OPTIONS: [&str; 4] = [
    "menu-editor.up",
    "menu-editor.down",
    "menu-editor.remove",
    "menu-editor.cancel",
];

fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// Registry reads need a query buffer and a page buffer.
struct Scratch {
    input: Vec<u8>,
    output: Vec<u8>,
}
impl Scratch {
    fn new() -> Result<Self, Error> {
        Ok(Self {
            input: buffer(REGISTRY_INPUT)?,
            output: buffer(REGISTRY_OUTPUT)?,
        })
    }
}

/// A list with its rows, focus and last drawn geometry.
#[derive(Debug, Default)]
struct Rows {
    rows: Vec<(String, String)>,
    focus: ListFocus,
    renderer: ListRenderer,
}
impl Rows {
    fn push(&mut self, label: &str, value: &str) -> Result<(), Error> {
        self.rows.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.rows.push((text::copy(label)?, text::copy(value)?));
        Ok(())
    }
}

#[derive(Debug)]
enum View {
    Menus {
        ids: Vec<String>,
        list: Rows,
    },
    Entries {
        menu: String,
        count: usize,
        list: Rows,
    },
    Entry {
        menu: String,
        index: usize,
        title: String,
        refused: bool,
        focus: usize,
        region: Rect,
        /// Top of the first option and the option height, from the last frame.
        options: (u16, u16),
    },
    Add {
        menu: String,
        candidates: Vec<Candidate>,
        list: Rows,
    },
}

#[derive(Debug)]
struct Slot {
    instance: Instance,
    view: View,
}

/// The interface-7 menu editor.
#[derive(Debug, Default)]
pub struct NativeMenuEditor {
    ui: Option<Ui>,
    slots: Vec<Slot>,
    top: [Option<Instance>; 4],
    /// Where the entry just moved went: its menu and index, for the list.
    moved: Option<(String, usize)>,
}

fn screen_token(screen: &str) -> Option<u32> {
    Some(match screen {
        "menu-editor.menus" => MENUS,
        "menu-editor.entries" => ENTRIES,
        "menu-editor.entry" => ENTRY,
        "menu-editor.add" => ADD,
        _ => return None,
    })
}

impl Plugin for NativeMenuEditor {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self {
            ui: Some(Ui::open(host)?),
            ..Self::default()
        })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::ActionRun {
                action: "menu-editor.open",
                ..
            } if token == OPEN => {
                let mut scratch = [0; 256];
                let ui = self.ui.as_ref().ok_or(Error::State)?;
                ScreenState::Menus.open(ui, host, &mut scratch, &mut [0; 0])
            }
            Event::ScreenOpened {
                screen,
                instance,
                handover,
                ..
            } => self.opened(host, screen, instance, handover),
            Event::ScreenShown {
                screen,
                instance: Some(instance),
                ..
            } => {
                if let Some(slot) =
                    screen_token(screen).and_then(|t| self.top.get_mut(t as usize - 10))
                {
                    *slot = Some(instance);
                }
                Ok(())
            }
            Event::ScreenClosed { instance, .. } => {
                self.slots.retain(|slot| slot.instance != instance);
                for top in &mut self.top {
                    if *top == Some(instance) {
                        *top = None;
                    }
                }
                Ok(())
            }
            Event::RegistryChanged { .. } if token == EVENTS => self.changed(host),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let slot = self.top_slot(token).ok_or(Error::State)?;
        slot.view.draw(host, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(at) = self.top_index(token) else {
            return Ok(Handled::No);
        };
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let outcome = self.slots[at].view.input(host, &input)?;
        match outcome {
            Outcome::Handled(handled) => Ok(handled),
            Outcome::Open(state) => {
                let mut scratch = buffer(COMMANDS)?;
                let mut output = buffer(REGISTRY_OUTPUT)?;
                state.open(ui, host, &mut scratch, &mut output)?;
                Ok(Handled::Yes)
            }
            Outcome::Back => {
                self.close_top(host)?;
                Ok(Handled::Yes)
            }
            Outcome::Edit(edit) => {
                let overlay = matches!(self.slots[at].view, View::Entry { .. } | View::Add { .. });
                match self.apply(host, edit) {
                    Ok(moved) => {
                        self.moved = Some(moved);
                        if overlay {
                            self.close_top(host)?;
                        }
                        Ok(Handled::Yes)
                    }
                    // Settings or this editor would become unreachable: the
                    // entry's overlay says so; nothing was changed.
                    Err(Error::Denied) => {
                        if let View::Entry { refused, .. } = &mut self.slots[at].view {
                            *refused = true;
                            let mut scratch = [0; 64];
                            CommandSender::new(host, &mut scratch)
                                .queue_redraw(RedrawMode::Partial)?;
                        }
                        Ok(Handled::Yes)
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }
}

/// What a view asks for after input.
enum Outcome {
    Handled(Handled),
    Open(ScreenState),
    Edit(Edit),
    /// Close the overlay without a change.
    Back,
}

/// An edit of one menu, staged through the typed commands.
#[derive(Debug)]
enum Edit {
    Move {
        menu: String,
        from: usize,
        to: usize,
    },
    Remove {
        menu: String,
        index: usize,
    },
    Add {
        menu: String,
        entry: String,
    },
    Reset {
        menu: String,
    },
    ResetAll,
}

impl NativeMenuEditor {
    fn close_top(&self, host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).back()
    }

    fn top_index(&self, token: u32) -> Option<usize> {
        let instance = (*self.top.get(token.checked_sub(10)? as usize)?)?;
        self.slots.iter().position(|slot| slot.instance == instance)
    }

    fn top_slot(&mut self, token: u32) -> Option<&mut Slot> {
        let at = self.top_index(token)?;
        self.slots.get_mut(at)
    }

    fn graph(host: &Host<'_>) -> Result<MenuGraph, Error> {
        let registry = Registry::open(host)?;
        let mut scratch = Scratch::new()?;
        let graph = MenuGraph::read(&registry, host, &mut scratch.input, &mut scratch.output);
        let released = registry.release(host);
        let graph = graph?;
        released?;
        Ok(graph)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        screen: &str,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let mut input = buffer(REGISTRY_INPUT)?;
        let mut output = buffer(REGISTRY_OUTPUT)?;
        let state = ScreenState::read(ui, host, screen, handover, &mut input, &mut output)?;
        let graph = Self::graph(host)?;
        let view = View::build(host, state, &graph)?;
        self.slots.retain(|slot| slot.instance != instance);
        self.slots.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.slots.push(Slot { instance, view });
        Ok(())
    }

    /// Rebuilds the list views from the registry after a menu changed.
    fn changed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.slots.is_empty() {
            return Ok(());
        }
        let graph = Self::graph(host)?;
        for slot in &mut self.slots {
            slot.view.refresh(host, &graph, self.moved.as_ref())?;
        }
        self.moved = None;
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    fn apply(&mut self, host: &Host<'_>, edit: Edit) -> Result<(String, usize), Error> {
        let graph = Self::graph(host)?;
        let mut scratch = buffer(COMMANDS)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        match edit {
            Edit::Move { menu, from, to } => {
                graph.move_entry(&mut commands, &menu, from, to)?;
                Ok((menu, to))
            }
            Edit::Remove { menu, index } => {
                let to = graph.remove(&mut commands, &menu, index)?;
                Ok((menu, to))
            }
            Edit::Add { menu, entry } => {
                graph.add(&mut commands, &menu, &entry)?;
                Ok((menu, 0))
            }
            Edit::Reset { menu } => {
                graph.reset(&mut commands, &menu)?;
                Ok((menu, 0))
            }
            Edit::ResetAll => {
                for menu in graph.menus().iter().filter(|menu| menu.user_defined()) {
                    graph.reset(&mut commands, menu.id())?;
                }
                Ok((String::new(), 0))
            }
        }
    }
}

impl View {
    fn build(host: &Host<'_>, state: ScreenState, graph: &MenuGraph) -> Result<Self, Error> {
        let mut view = match state {
            ScreenState::Menus => Self::Menus {
                ids: Vec::new(),
                list: Rows::default(),
            },
            ScreenState::Entries { menu } => Self::Entries {
                menu,
                count: 0,
                list: Rows::default(),
            },
            ScreenState::Entry { menu, index } => {
                let display = snapshot::capture_snapshot(host)?;
                Self::Entry {
                    menu,
                    index,
                    title: String::new(),
                    refused: false,
                    focus: 0,
                    region: Rect::new(0, 0, display.display_width(), display.display_height()),
                    options: (0, 0),
                }
            }
            ScreenState::Add { menu } => Self::Add {
                menu,
                candidates: Vec::new(),
                list: Rows::default(),
            },
        };
        view.refresh(host, graph, None)?;
        Ok(view)
    }

    fn text(host: &Host<'_>, key: &str) -> Result<String, Error> {
        Messages::new()?.copy(host, key)
    }

    /// Reads rows anew from `graph`; the host's answer is the only truth.
    fn refresh(
        &mut self,
        host: &Host<'_>,
        graph: &MenuGraph,
        moved: Option<&(String, usize)>,
    ) -> Result<(), Error> {
        match self {
            Self::Menus { ids, list } => {
                let mut rows = Rows::default();
                let mut found = Vec::new();
                for (index, name) in graph.display_order()? {
                    let menu = &graph.menus()[index];
                    let changed = if menu.user_defined() {
                        Self::text(host, "menu-editor.changed")?
                    } else {
                        String::new()
                    };
                    rows.push(&name, &format!("{changed} ›"))?;
                    found.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    found.push(text::copy(menu.id())?);
                }
                if graph.menus().iter().any(|menu| menu.user_defined()) {
                    rows.push(&Self::text(host, "menu-editor.reset-all")?, "")?;
                }
                rows.focus.set(list.focus.focus());
                rows.focus.clamp(rows.rows.len());
                rows.renderer = core::mem::take(&mut list.renderer);
                *ids = found;
                *list = rows;
            }
            Self::Entries { menu, count, list } => {
                let Some(entry) = graph.menus().iter().find(|m| m.id() == menu.as_str()) else {
                    return Err(Error::NotFound);
                };
                let mut rows = Rows::default();
                for (id, title) in entry.entries().iter().zip(entry.entry_titles()) {
                    let more = if graph.menus().iter().any(|m| m.id() == id) {
                        "›"
                    } else {
                        ""
                    };
                    rows.push(title, more)?;
                }
                *count = entry.entries().len();
                rows.push(&Self::text(host, "menu-editor.add")?, "›")?;
                if entry.user_defined() {
                    rows.push(&Self::text(host, "menu-editor.reset")?, "")?;
                }
                rows.focus.set(list.focus.focus());
                if let Some((from, to)) = moved
                    && from == menu
                {
                    rows.focus.set(*to);
                }
                rows.focus.clamp(rows.rows.len());
                rows.renderer = core::mem::take(&mut list.renderer);
                *list = rows;
            }
            Self::Entry {
                menu, index, title, ..
            } => {
                let entry = graph.menus().iter().find(|m| m.id() == menu.as_str());
                *title = match entry.and_then(|m| m.entry_titles().get(*index)) {
                    Some(name) => text::copy(name)?,
                    None => String::new(),
                };
            }
            Self::Add {
                menu,
                candidates,
                list,
            } => Self::read_add(host, graph, menu, candidates, list)?,
        }
        Ok(())
    }

    fn read_add(
        host: &Host<'_>,
        graph: &MenuGraph,
        menu: &str,
        candidates: &mut Vec<Candidate>,
        list: &mut Rows,
    ) -> Result<(), Error> {
        let registry = Registry::open(host)?;
        let mut scratch = Scratch::new()?;
        let found = graph.candidates(
            &registry,
            host,
            menu,
            &mut scratch.input,
            &mut scratch.output,
        );
        let released = registry.release(host);
        let found = found?;
        released?;
        let mut rows = Rows::default();
        for candidate in &found {
            let beside = match candidate.hidden_by() {
                Some(plugin) => {
                    let mut messages = Messages::new()?;
                    messages.format(
                        host,
                        "menu-editor.hidden-by",
                        &[paperpaw_sdk::native::formatting::Arg::Text {
                            name: "plugin",
                            value: plugin,
                        }],
                    )?
                }
                None if candidate.is_menu() => String::from("›"),
                None => String::new(),
            };
            rows.push(candidate.title(), &beside)?;
        }
        rows.focus.set(list.focus.focus());
        rows.focus.clamp(rows.rows.len());
        rows.renderer = core::mem::take(&mut list.renderer);
        *candidates = found;
        *list = rows;
        Ok(())
    }

    fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        match self {
            Self::Menus { list, .. } => {
                let title = Self::text(host, "menu-editor.title")?;
                list.renderer
                    .draw(host, frame, &title, &list.rows, list.focus.focus(), true)
            }
            Self::Entries { menu, list, .. } => {
                let title = Self::title_of(host, menu)?;
                list.renderer
                    .draw(host, frame, &title, &list.rows, list.focus.focus(), true)
            }
            Self::Add { list, .. } => {
                let title = Self::text(host, "menu-editor.add")?;
                list.renderer
                    .draw(host, frame, &title, &list.rows, list.focus.focus(), true)
            }
            Self::Entry {
                title,
                refused,
                focus,
                region,
                options,
                ..
            } => Self::draw_entry(host, frame, title, *refused, *focus, region, options),
        }
    }

    /// The entry options as an overlay on the bottom of the screen.
    #[allow(
        clippy::too_many_arguments,
        reason = "one overlay and its retained geometry"
    )]
    fn draw_entry(
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        refused: bool,
        focus: usize,
        region: &mut Rect,
        options: &mut (u16, u16),
    ) -> Result<(), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let high = (u32::from(style.row) * (OPTIONS.len() as u32 + 1) + u32::from(style.gap) * 2)
            .min(u32::from(height)) as u16;
        *region = Rect::new(0, height - high, width, high);
        let mut paint = Vec::new();
        paint::push(&mut paint, Paint::Fill(*region, style.paper))?;
        paint::push(
            &mut paint,
            Paint::Fill(
                Rect::new(region.x, region.y, width, (style.line * 2).max(2)),
                style.ink,
            ),
        )?;
        let title_area = Rect::new(0, region.y + style.gap, width, style.row);
        let room = f32::from(width) - 2.0 * f32::from(style.margin);
        let fitted = text::fit(&font, style.title, title, room)?;
        text::label(
            &mut paint,
            &font,
            &fitted,
            style.title,
            f32::from(style.margin),
            title_area,
            style.ink,
            None,
        )?;
        let top = region.y + style.row + style.gap;
        *options = (top, style.row);
        if refused {
            let message = Self::text(host, "menu-editor.locked")?;
            text::label(
                &mut paint,
                &font,
                &message,
                style.small,
                f32::from(style.margin),
                Rect::new(0, top, width, style.row),
                style.muted,
                None,
            )?;
        } else {
            for (slot, key) in OPTIONS.iter().enumerate() {
                let label = Self::text(host, key)?;
                let rect = Rect::new(
                    0,
                    top.saturating_add((slot as u16).saturating_mul(style.row)),
                    width,
                    style.row,
                );
                prepare_row(&mut paint, &font, &style, rect, &label, "", slot == focus)?;
            }
        }
        paint::replay(frame, &font, paint.iter())?;
        font.release()
    }

    fn title_of(host: &Host<'_>, menu: &str) -> Result<String, Error> {
        let graph = NativeMenuEditor::graph(host)?;
        match graph.menus().iter().find(|m| m.id() == menu) {
            Some(found) => text::copy(found.title()),
            None => Self::text(host, "menu-editor.title"),
        }
    }

    fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Outcome, Error> {
        match self {
            Self::Menus { ids, list } => {
                let len = list.rows.len();
                match list.focus.input(host, &list.renderer, len, input)? {
                    ListInput::Chosen(index) => Ok(match ids.get(index) {
                        Some(menu) => Outcome::Open(ScreenState::Entries {
                            menu: text::copy(menu)?,
                        }),
                        None => Outcome::Edit(Edit::ResetAll),
                    }),
                    other => Ok(Outcome::Handled(focus::handled(other))),
                }
            }
            Self::Entries { menu, count, list } => {
                let len = list.rows.len();
                match list.focus.input(host, &list.renderer, len, input)? {
                    ListInput::Chosen(index) if index < *count => {
                        Ok(Outcome::Open(ScreenState::Entry {
                            menu: text::copy(menu)?,
                            index,
                        }))
                    }
                    ListInput::Chosen(index) if index == *count => {
                        Ok(Outcome::Open(ScreenState::Add {
                            menu: text::copy(menu)?,
                        }))
                    }
                    ListInput::Chosen(_) => Ok(Outcome::Edit(Edit::Reset {
                        menu: text::copy(menu)?,
                    })),
                    other => Ok(Outcome::Handled(focus::handled(other))),
                }
            }
            Self::Add {
                menu,
                candidates,
                list,
            } => {
                let len = list.rows.len();
                match list.focus.input(host, &list.renderer, len, input)? {
                    ListInput::Chosen(index) => match candidates.get(index) {
                        Some(candidate) => Ok(Outcome::Edit(Edit::Add {
                            menu: text::copy(menu)?,
                            entry: text::copy(candidate.id())?,
                        })),
                        None => Ok(Outcome::Handled(Handled::Yes)),
                    },
                    other => Ok(Outcome::Handled(focus::handled(other))),
                }
            }
            Self::Entry {
                menu,
                index,
                refused,
                focus,
                region,
                options,
                ..
            } => Self::entry_input(host, input, menu, *index, refused, focus, *region, *options),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one overlay's state and geometry"
    )]
    fn entry_input(
        host: &Host<'_>,
        input: &Input<'_>,
        menu: &str,
        index: usize,
        refused: &mut bool,
        focus: &mut usize,
        region: Rect,
        options: (u16, u16),
    ) -> Result<Outcome, Error> {
        match entry_step(input, *focus, *refused, region, options) {
            EntryStep::Ignored => Ok(Outcome::Handled(Handled::No)),
            EntryStep::Taken => Ok(Outcome::Handled(Handled::Yes)),
            EntryStep::Focus(to) => {
                Self::move_focus(host, focus, to)?;
                Ok(Outcome::Handled(Handled::Yes))
            }
            EntryStep::Chosen(_) if *refused => Ok(Outcome::Back),
            EntryStep::Chosen(option) => {
                *focus = option;
                let menu = text::copy(menu)?;
                Ok(match option {
                    0 if index > 0 => Outcome::Edit(Edit::Move {
                        menu,
                        from: index,
                        to: index - 1,
                    }),
                    1 => Outcome::Edit(Edit::Move {
                        menu,
                        from: index,
                        to: index + 1,
                    }),
                    2 => Outcome::Edit(Edit::Remove { menu, index }),
                    _ => Outcome::Back,
                })
            }
        }
    }

    fn move_focus(host: &Host<'_>, focus: &mut usize, to: usize) -> Result<(), Error> {
        if to != *focus {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
            *focus = to;
        }
        Ok(())
    }
}

/// What input meant to the entry overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryStep {
    /// Not for the overlay.
    Ignored,
    /// Taken without effect (a tap on the overlay between the options).
    Taken,
    /// The focus should go to this option.
    Focus(usize),
    /// This option was chosen.
    Chosen(usize),
}

/// The overlay's input rules: the options as rows under the title; while the
/// refusal shows, Confirm or a tap on it closes it.
fn entry_step(
    input: &Input<'_>,
    focus: usize,
    refused: bool,
    region: Rect,
    options: (u16, u16),
) -> EntryStep {
    match *input {
        Input::Action { action, .. } => match action {
            "core.confirm" if refused => EntryStep::Chosen(OPTIONS.len() - 1),
            "core.confirm" => EntryStep::Chosen(focus),
            "core.up" | "core.left" | "core.previous" | "core.prev-page" => {
                EntryStep::Focus(focus.saturating_sub(1))
            }
            "core.down" | "core.right" | "core.next" | "core.next-page" => {
                EntryStep::Focus((focus + 1).min(OPTIONS.len() - 1))
            }
            _ => EntryStep::Ignored,
        },
        Input::Gesture {
            position,
            kind: paperpaw_sdk::native::callbacks::InputGestureKind::Tap,
            ..
        } if region.contains(position) => {
            let (top, high) = options;
            let below = position.y - i32::from(top);
            if refused {
                EntryStep::Chosen(OPTIONS.len() - 1)
            } else if below >= 0 && high > 0 && (below as usize / usize::from(high)) < OPTIONS.len()
            {
                EntryStep::Chosen(below as usize / usize::from(high))
            } else {
                EntryStep::Taken
            }
        }
        _ => EntryStep::Ignored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{Point, native::callbacks::InputGestureKind};

    const REGION: Rect = Rect::new(0, 500, 480, 300);
    const OPTIONS_AT: (u16, u16) = (580, 50);

    fn action(action: &str) -> Input<'_> {
        Input::Action { now_ms: 0, action }
    }
    fn tap(y: i32) -> Input<'static> {
        Input::Gesture {
            now_ms: 0,
            position: Point::new(20, y),
            kind: InputGestureKind::Tap,
        }
    }
    fn step(input: &Input<'_>, focus: usize, refused: bool) -> EntryStep {
        entry_step(input, focus, refused, REGION, OPTIONS_AT)
    }

    #[test]
    fn keys_move_between_the_four_options_and_stop_at_the_ends() {
        assert_eq!(step(&action("core.down"), 0, false), EntryStep::Focus(1));
        assert_eq!(step(&action("core.down"), 3, false), EntryStep::Focus(3));
        assert_eq!(step(&action("core.up"), 0, false), EntryStep::Focus(0));
        assert_eq!(
            step(&action("core.confirm"), 2, false),
            EntryStep::Chosen(2)
        );
        assert_eq!(step(&action("core.menu"), 2, false), EntryStep::Ignored);
    }

    #[test]
    fn a_tap_chooses_the_option_under_it_and_only_inside_the_overlay() {
        assert_eq!(step(&tap(580), 0, false), EntryStep::Chosen(0));
        assert_eq!(step(&tap(679), 0, false), EntryStep::Chosen(1));
        assert_eq!(step(&tap(779), 0, false), EntryStep::Chosen(3));
        assert_eq!(step(&tap(780), 0, false), EntryStep::Taken);
        assert_eq!(step(&tap(520), 0, false), EntryStep::Taken);
        assert_eq!(step(&tap(100), 0, false), EntryStep::Ignored);
    }

    #[test]
    fn while_the_refusal_shows_confirm_or_a_tap_on_it_closes_it() {
        assert_eq!(step(&action("core.confirm"), 0, true), EntryStep::Chosen(3));
        assert_eq!(step(&tap(520), 0, true), EntryStep::Chosen(3));
        assert_eq!(step(&tap(100), 0, true), EntryStep::Ignored);
    }

    #[test]
    fn screen_tokens_follow_the_declarations() {
        assert_eq!(screen_token("menu-editor.menus"), Some(10));
        assert_eq!(screen_token("menu-editor.add"), Some(13));
        assert_eq!(screen_token("menu.screen"), None);
    }
}
