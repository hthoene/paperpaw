//! The menu as an interface-7 plugin: a sheet at the bottom of the screen over
//! what is open, listing the rows the registry answers for a menu. Actions
//! close the menu and run; switches, numbers and choices of two change in
//! place; longer choices, text and a setting's own editor open their screens;
//! submenus open as a sheet over this one.
use crate::native_data::Reader;
use crate::native_model::{
    self as model, Act, Choice, Kind, Row, SettingInfo, SettingKind, Value, with_unit,
};
use crate::native_panel_screen::{self as panel_screen, Outcome, PanelView};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, kinds},
        registrations,
        reply::Reply,
        settings::SettingValue,
        ui::{HandoverId, Instance, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/menu_declarations.rs"));
}

/// The screen's token, as declared.
pub const LIST: u32 = 10;
/// The quick panel's token, as declared.
pub const PANEL: u32 = 11;
/// The event subscriptions' token.
pub const EVENTS: u32 = 3;

/// A menu on screen.
#[derive(Debug, Default)]
struct ListView {
    menu: String,
    depth: usize,
    hide: String,
    /// The rows shown, one item each.
    rows: Vec<Row>,
    /// The settings among them.
    infos: Vec<SettingInfo>,
    list: ItemList,
    /// The number being changed with the buttons: its row.
    editing: Option<usize>,
    /// The focus is shown: always with buttons, on touch screens once
    /// buttons are used (a tap needs no focus).
    keys: bool,
    touch: bool,
    /// Where the sheet was drawn last.
    region: Rect,
    /// Made anew when it shows again.
    stale: bool,
}

impl ListView {
    fn info(&self, key: &str) -> Option<&SettingInfo> {
        self.infos.iter().find(|i| i.key == key)
    }
}

/// The interface-7 menu plugin.
#[derive(Debug, Default)]
pub struct NativeMenu {
    ui: Option<Ui>,
    screens: Screens<ListView>,
    /// The quick panels open.
    panels: Screens<PanelView>,
}

impl Plugin for NativeMenu {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
            .chain(image::HELP)
        {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let Some(event) = read_event!(incoming; kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::SettingChanged, kinds::PluginsChanged, kinds::RegistryChanged)
        else {
            return Ok(());
        };
        match event {
            Event::ScreenOpened {
                screen: "menu.panel",
                instance,
                handover,
                ..
            } => self.panel_opened(host, instance, handover),
            Event::ScreenOpened {
                instance, handover, ..
            } => self.opened(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.shown(host, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                self.panels.close(instance);
                Ok(())
            }
            Event::SettingChanged { .. }
            | Event::PluginsChanged { .. }
            | Event::RegistryChanged { .. }
                if token == EVENTS =>
            {
                self.changed(host)
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token == PANEL {
            let view = self.panels.top(PANEL).ok_or(Error::State)?;
            view.draw(host, &mut frame, true)?;
            return Ok(());
        }
        let view = self.screens.top(token).ok_or(Error::State)?;
        if view.rows.is_empty() {
            // Nothing to list: the sheet says so.
            let mut messages = Messages::new()?;
            let empty = messages.copy(host, "menu.empty")?;
            let mut items = Vec::new();
            push(&mut items, Item::row(&empty, "")?)?;
            let mut list = ItemList::new(items);
            view.region = list.draw_sheet(host, &mut frame, false)?;
            return Ok(());
        }
        view.region = view.list.draw_sheet(host, &mut frame, view.keys)?;
        Ok(())
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        if token == PANEL {
            return self.panel_input(host, &input);
        }
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // The menu button toggles the menu; a tap outside the sheet closes it.
        let outside = match input {
            Input::Action {
                action: "core.menu",
                ..
            } => true,
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => !view.region.contains(position),
            _ => false,
        };
        if outside {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).back()?;
            return Ok(Handled::Yes);
        }
        if view.editing.is_some()
            && let Some(handled) = Self::edit_input(host, view, &input)?
        {
            return Ok(handled);
        }
        // The first press of a button shows where the focus is.
        if !view.keys && is_move(&input) {
            view.keys = true;
            Self::redraw(host)?;
            return Ok(Handled::Yes);
        }
        let step = view.list.input(host, &input)?;
        let confirm = matches!(
            input,
            Input::Action {
                action: "core.confirm",
                ..
            }
        );
        match step {
            ItemStep::Chosen(index) => self.chosen(host, token, index, confirm)?,
            ItemStep::Step(index, direction) => {
                self.step(host, token, index, i64::from(direction))?;
            }
            ItemStep::Held(_) => return Ok(Handled::No),
            _ => {}
        }
        Ok(ItemList::handled(step))
    }
}

/// Whether `input` moves the focus.
fn is_move(input: &Input<'_>) -> bool {
    matches!(
        input,
        Input::Action {
            action: "core.up"
                | "core.left"
                | "core.previous"
                | "core.down"
                | "core.right"
                | "core.next",
            ..
        }
    )
}

impl NativeMenu {
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    fn redraw(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// A panel opened: which menu, and where it hangs.
    fn panel_opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        let (mut menu, mut mode) = (String::new(), String::new());
        if let Some(handover) = handover {
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut output = buffer(SERVICE_BUFFER)?;
            for param in self.ui()?.params(host, handover, &mut input, &mut output)? {
                let param = param?;
                match param.key() {
                    model::PARAM_MENU => menu = copy(param.value())?,
                    panel_screen::PARAM_MODE => mode = copy(param.value())?,
                    _ => {}
                }
            }
        }
        if menu.is_empty() {
            menu = copy(model::MAIN)?;
        }
        let view = PanelView::new(host, &menu, &mode)?;
        self.panels.open(instance, view)
    }

    /// Input for the top panel; what it asks to open is opened here, where
    /// the hand-over service is.
    fn panel_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.panels.top(PANEL) else {
            return Ok(Handled::No);
        };
        match view.input(host, input)? {
            Outcome::Handled(handled) => Ok(handled),
            Outcome::Screen { screen, key } => {
                self.open(host, &screen, &[("id", &key)])?;
                Ok(Handled::Yes)
            }
            Outcome::Slot { slot, param, value } => {
                self.open_slot(host, slot, &[(param, &value)])?;
                Ok(Handled::Yes)
            }
            Outcome::SettingSlot { slot, key } => {
                self.open_slot(host, &slot, &[("id", &key)])?;
                Ok(Handled::Yes)
            }
        }
    }

    /// Opens `screen` with the hand-over `params`.
    fn open(&self, host: &Host<'_>, screen: &str, params: &[(&str, &str)]) -> Result<(), Error> {
        let open = Open::screen(screen)?;
        self.send_open(host, open, params)
    }

    /// Opens a screen filling `slot` with the hand-over `params`.
    fn open_slot(&self, host: &Host<'_>, slot: &str, params: &[(&str, &str)]) -> Result<(), Error> {
        let open = Open::slot(slot)?;
        self.send_open(host, open, params)
    }

    fn send_open(
        &self,
        host: &Host<'_>,
        open: Open<'_>,
        params: &[(&str, &str)],
    ) -> Result<(), Error> {
        let mut made = Vec::new();
        for (key, value) in params {
            push(&mut made, Param::new(key, value)?)?;
        }
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let handover = self.ui()?.handover(host, &made, &mut input, &mut output)?;
        let mut scratch = buffer(1024)?;
        CommandSender::new(host, &mut scratch).open(open.handover(handover))
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        let (mut menu, mut depth, mut hide) = (String::new(), None, String::new());
        if let Some(handover) = handover {
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut output = buffer(SERVICE_BUFFER)?;
            for param in self.ui()?.params(host, handover, &mut input, &mut output)? {
                let param = param?;
                match param.key() {
                    model::PARAM_MENU => menu = copy(param.value())?,
                    model::PARAM_DEPTH => depth = Some(copy(param.value())?),
                    model::PARAM_HIDE => hide = copy(param.value())?,
                    _ => {}
                }
            }
        }
        if menu.is_empty() {
            menu = copy(model::MAIN)?;
        }
        let mut view = ListView {
            menu,
            depth: model::depth_of(depth.as_deref()),
            hide,
            touch: paperpaw_sdk::native::snapshot::capture_snapshot(host)?.has_touch(),
            ..ListView::default()
        };
        view.keys = !view.touch;
        Self::rebuild(host, &mut view)?;
        self.screens.open(instance, view)
    }

    fn shown(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        if let Some(panel) = self.panels.get(instance) {
            if panel.is_stale() {
                panel.read(host)?;
            }
            return self.panels.shown(PANEL, instance);
        }
        self.screens.shown(LIST, instance)?;
        if let Some(view) = self.screens.get(instance)
            && view.stale
        {
            Self::rebuild(host, view)?;
        }
        Ok(())
    }

    /// Reads the menu and makes its rows and items anew, aside; the view
    /// changes only when all of it was made.
    fn rebuild(host: &Host<'_>, view: &mut ListView) -> Result<(), Error> {
        let mut reader = Reader::open(host)?;
        let made = Self::read(&mut reader, view);
        let (rows, infos) = reader.finish(made)?;
        let items = Self::items(host, &rows, &infos, view.editing, view.touch)?;
        let focus = view.list.focus();
        let first = model::first_choosable(&rows);
        view.rows = rows;
        view.infos = infos;
        view.list.replace(items);
        view.list.set_focus(if view.stale || focus > 0 {
            focus
        } else {
            first
        });
        view.stale = false;
        Ok(())
    }

    fn read(
        reader: &mut Reader<'_, '_>,
        view: &ListView,
    ) -> Result<(Vec<Row>, Vec<SettingInfo>), Error> {
        let rows = reader.menu(&view.menu)?;
        let mut infos: Vec<SettingInfo> = Vec::new();
        for row in rows.iter().filter(|r| r.kind == Kind::Setting) {
            if let Some(info) = reader.setting(&row.item)? {
                push(&mut infos, info)?;
            }
        }
        let rows = model::shown(rows, &view.hide, |row| {
            row.kind == Kind::Setting && !infos.iter().any(|i| i.key == row.item && i.listed())
        })?;
        infos.retain(|i| rows.iter().any(|r| r.item == i.key));
        Ok((rows, infos))
    }

    /// The items for `rows`: a number being changed shows its value between
    /// arrows.
    fn items(
        host: &Host<'_>,
        rows: &[Row],
        infos: &[SettingInfo],
        editing: Option<usize>,
        touch: bool,
    ) -> Result<Vec<Item>, Error> {
        let mut messages = Messages::new()?;
        let on = messages.copy(host, "core.on")?;
        let off = messages.copy(host, "core.off")?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for (index, row) in rows.iter().enumerate() {
            if row.kind == Kind::Section {
                items.push(Item::heading(&row.title)?);
                continue;
            }
            let info = infos.iter().find(|i| i.key == row.item);
            let value = match (row.kind, info) {
                (Kind::Submenu, _) => copy("›")?,
                (Kind::Setting, Some(info)) => {
                    let text = value_text(info, &on, &off, &mut |key| messages.copy(host, key))?;
                    if editing == Some(index) {
                        concat(&["‹ ", &text, " ›"])?
                    } else {
                        text
                    }
                }
                _ if row.checked() => copy(&on)?,
                _ => copy(&row.badge)?,
            };
            let mut item = Item::noted(&row.title, &value, &row.describe)?;
            if let Item::Row(r) = &mut item {
                r.disabled = !row.choosable();
                r.stepper = touch && info.is_some_and(|i| i.kind == SettingKind::Int);
                // A switch is told as one: the row is on or off.
                if let (Kind::Setting, Some(Value::Bool(on))) =
                    (row.kind, info.and_then(|i| i.value.as_ref()))
                {
                    r.on = Some(*on);
                }
            }
            items.push(item);
        }
        Ok(items)
    }

    /// The setting or the registry changed: the menu on top is made anew, the
    /// others when they show.
    fn changed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        for panel in self.panels.all() {
            panel.mark_stale();
        }
        if let Some(panel) = self.panels.top(PANEL) {
            panel.read(host)?;
            Self::redraw(host)?;
        }
        let top = self.screens.top_instance(LIST);
        for view in self.screens.all() {
            view.stale = true;
        }
        if let Some(top) = top
            && let Some(view) = self.screens.get(top)
        {
            Self::rebuild(host, view)?;
            Self::redraw(host)?;
        }
        Ok(())
    }

    fn chosen(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        confirm: bool,
    ) -> Result<(), Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(row) = view.rows.get(index) else {
            return Ok(());
        };
        match model::choose(row, view.depth)? {
            Choice::Nothing => Ok(()),
            Choice::OpenMenu { menu, depth } => {
                let depth = depth_text(depth);
                self.open(
                    host,
                    "menu.list",
                    &[
                        (model::PARAM_MENU, &menu),
                        (model::PARAM_DEPTH, depth.as_str()),
                    ],
                )
            }
            Choice::Run { action, back } => {
                // This menu and those below it close, so the action reaches
                // the screen the menu was opened on.
                let mut scratch = buffer(1024)?;
                let mut commands = CommandSender::new(host, &mut scratch);
                for _ in 0..back {
                    commands.back()?;
                }
                commands.run_action(&action)
            }
            Choice::Setting(key) => self.setting_chosen(host, token, index, &key, confirm),
        }
    }

    fn setting_chosen(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        key: &str,
        confirm: bool,
    ) -> Result<(), Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(info) = view.info(key) else {
            return Ok(());
        };
        match model::act(info, view.touch, confirm)? {
            Act::Nothing => Ok(()),
            Act::Screen(screen) => self.open(host, &screen, &[("id", key)]),
            Act::Slot(slot) => self.open_slot(host, &slot, &[("id", key)]),
            Act::Values => self.open_slot(host, "core.choose-value", &[("id", key)]),
            Act::Page => self.open_slot(host, "core.settings", &[("focus", key)]),
            Act::Step => self.step(host, token, index, 1),
            Act::StartNumber => {
                let Some(view) = self.screens.top(token) else {
                    return Ok(());
                };
                let items = Self::items(host, &view.rows, &view.infos, Some(index), view.touch)?;
                Self::redraw(host)?;
                view.editing = Some(index);
                view.list.replace(items);
                Ok(())
            }
        }
    }

    /// Changes the setting of row `index` by `direction` steps at once: a
    /// switch toggles, a choice of two takes the other, a number stops at its
    /// ends.
    fn step(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        direction: i64,
    ) -> Result<(), Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(info) = view.rows.get(index).and_then(|row| view.info(&row.item)) else {
            return Ok(());
        };
        let key = copy(&info.key)?;
        let next = match info.kind {
            SettingKind::Choice | SettingKind::Language => {
                let Some(Value::Text(now)) = &info.value else {
                    return Ok(());
                };
                let mut reader = Reader::open(host)?;
                let values = reader.choice_values(&key);
                let values = reader.finish(values)?;
                let at = values.iter().position(|v| v == now).unwrap_or(0);
                let len = values.len().max(1);
                let to = (at as i64 + direction).rem_euclid(len as i64) as usize;
                match values.get(to) {
                    Some(v) if v != now => Value::Text(copy(v)?),
                    _ => return Ok(()),
                }
            }
            _ => match model::stepped(info, direction) {
                Some(next) => next,
                None => return Ok(()),
            },
        };
        let mut scratch = buffer(1024)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        match &next {
            Value::Bool(v) => commands.set_setting(&key, SettingValue::Bool(*v))?,
            Value::Int(v) => commands.set_setting(&key, SettingValue::Int(*v))?,
            Value::Text(v) => commands.set_setting(&key, SettingValue::Text(v))?,
        }
        commands.queue_redraw(RedrawMode::Partial)
    }

    /// The buttons while a number is being changed: left/right (up/down,
    /// pages) change it at once, confirm and back end it.
    fn edit_input(
        host: &Host<'_>,
        view: &mut ListView,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let Input::Action { action, .. } = *input else {
            return Ok(None);
        };
        let direction = match action {
            "core.right" | "core.down" | "core.next" | "core.next-page" => 1,
            "core.left" | "core.up" | "core.previous" | "core.prev-page" => -1,
            "core.confirm" | "core.back" => 0,
            _ => return Ok(None),
        };
        let Some(row) = view.editing else {
            return Ok(None);
        };
        if direction == 0 {
            let items = Self::items(host, &view.rows, &view.infos, None, view.touch)?;
            Self::redraw(host)?;
            view.editing = None;
            view.list.replace(items);
            return Ok(Some(Handled::Yes));
        }
        let Some(info) = view.rows.get(row).and_then(|r| view.info(&r.item)) else {
            view.editing = None;
            return Ok(None);
        };
        if let Some(Value::Int(next)) = model::stepped(info, direction) {
            let mut scratch = buffer(256)?;
            CommandSender::new(host, &mut scratch)
                .set_setting(&info.key, SettingValue::Int(next))?;
        }
        Ok(Some(Handled::Yes))
    }
}

/// The value of a setting as its row shows it. A choice's value is a translation
/// key (`Kind::Choice`): `translate` gives its text in the language of the
/// display, so the row says *Auto*, not `power.mode.auto`. A value that has no text says itself.
pub(crate) fn value_text(
    info: &SettingInfo,
    on: &str,
    off: &str,
    translate: &mut dyn FnMut(&str) -> Result<String, Error>,
) -> Result<String, Error> {
    match &info.value {
        Some(Value::Bool(true)) => copy(on),
        Some(Value::Bool(false)) => copy(off),
        Some(Value::Int(v)) => with_unit(*v, &info.unit),
        Some(Value::Text(v)) if info.kind == SettingKind::Choice && !v.is_empty() => {
            // A value that is no key of the catalogs is said as it is (as `tr` does).
            match translate(v) {
                Err(Error::NotFound | Error::Invalid) => copy(v),
                said => said,
            }
        }
        Some(Value::Text(v)) => copy(v),
        None => copy(""),
    }
}

/// A depth as text.
fn depth_text(depth: usize) -> DepthText {
    let mut digits = [0u8; 20];
    let text = paperpaw_ui_kit::text::decimal(depth, &mut digits);
    let mut out = DepthText([0; 20], text.len());
    out.0[..text.len()].copy_from_slice(text.as_bytes());
    out
}

/// A small depth as text without allocating.
struct DepthText([u8; 20], usize);

impl DepthText {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0[..self.1]).unwrap_or("0")
    }
}
