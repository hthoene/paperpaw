//! The settings screens as an interface-7 plugin: the main page, a group's
//! page, choosing a value, themes and searching, all lists of the shared
//! kit over the registry's settings, with host dialogs for questions.
use crate::native_data::{self, Reader};
use crate::native_model::{
    self as model, Context, Detail, Group, Kind, PageRow, Setting, Val, flag,
};
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, PreviewSetting, RedrawMode, ResetTarget},
        events::{AnswerOutcome, Answered, Event},
        formatting::Arg,
        registrations,
        reply::Reply,
        settings::SettingValue,
        ui::{Instance, Notify, Open, Param, Question, RequestId, TextWrite, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
    uitext,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/settings_declarations.rs"));
}

/// Screen tokens, as declared.
pub const LIST: u32 = 10;
/// A group's page.
pub const GROUP: u32 = 11;
/// Choosing a value.
pub const CHOOSE: u32 = 12;
/// Choosing a theme.
pub const THEMES: u32 = 13;
/// Search results.
pub const SEARCH: u32 = 14;
/// The event subscriptions' token.
pub const EVENTS: u32 = 3;

/// The longest search text.
const QUERY_LIMIT: usize = 256;
/// The longest text of a text setting.
const TEXT_LIMIT: usize = 4096;
const THEME_KIND: &str = "core.theme";
/// Appearance settings a theme never changes: what the reader needs to read
/// the interface at all.
const THEME_KEPT: [&str; 3] = ["core.ui-size", "core.ui-contrast", "core.logo"];
const LOOK_GROUP: &str = "core.group.look";
/// The marker of the theme in use.
const MARK: &str = "•";

/// What a row of the main page opens.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Link {
    Group { id: String, page: String },
    Page(String),
    Advanced,
    Search,
    Reset,
}

/// The main page.
#[derive(Debug, Default)]
struct ListView {
    links: Vec<Option<Link>>,
    list: ItemList,
    stale: bool,
}

/// A row of a group's page.
#[derive(Debug)]
enum Row {
    Heading(String),
    Setting(Box<Entry>),
    Reset,
    /// An action plugins add to the group: its id, name and what it shows at the right.
    Action(String, String, String),
    Theme,
    /// A part of the group: its id and name.
    Part(String, String),
    More,
}

/// A setting on a page with what shows it.
#[derive(Debug)]
struct Entry {
    setting: Setting,
    detail: Detail,
    /// Value and name of each option, for choices.
    options: Vec<(String, String)>,
}

/// A number being changed with the buttons: its row and the value it had.
#[derive(Debug)]
struct Editing {
    row: usize,
    before: i64,
    now: i64,
}

/// A group's page.
#[derive(Debug, Default)]
struct GroupView {
    group: String,
    more: bool,
    focus: String,
    /// A book scope is open on this screen.
    scoped: bool,
    rows: Vec<Row>,
    list: ItemList,
    title: String,
    editing: Option<Editing>,
    touch: bool,
    stale: bool,
}

/// Choosing a value of a setting.
#[derive(Debug, Default)]
struct ChooseView {
    key: String,
    title: String,
    /// Value and name of each option.
    options: Vec<(String, String)>,
    /// Actions that offer more ways to choose, after the options.
    more: Vec<(String, String)>,
    list: ItemList,
    stale: bool,
}

/// Choosing a theme.
#[derive(Debug, Default)]
struct ThemesView {
    /// Preset id and name; the first is "none".
    themes: Vec<(String, String)>,
    list: ItemList,
    stale: bool,
}

/// What a search found and where it leads.
#[derive(Debug)]
enum Target {
    Setting { group: String, key: String },
    Page(String),
}

/// Search results.
#[derive(Debug, Default)]
struct SearchView {
    query: String,
    hits: Vec<Target>,
    list: ItemList,
    stale: bool,
}

#[derive(Debug)]
enum View {
    List(ListView),
    Group(GroupView),
    Choose(ChooseView),
    Themes(ThemesView),
    Search(SearchView),
}

impl View {
    fn list(&mut self) -> &mut ItemList {
        match self {
            Self::List(v) => &mut v.list,
            Self::Group(v) => &mut v.list,
            Self::Choose(v) => &mut v.list,
            Self::Themes(v) => &mut v.list,
            Self::Search(v) => &mut v.list,
        }
    }

    fn stale(&mut self) -> &mut bool {
        match self {
            Self::List(v) => &mut v.stale,
            Self::Group(v) => &mut v.stale,
            Self::Choose(v) => &mut v.stale,
            Self::Themes(v) => &mut v.stale,
            Self::Search(v) => &mut v.stale,
        }
    }
}

/// What an open question is for.
#[derive(Debug, PartialEq, Eq)]
enum Asked {
    /// Reset the settings of a group (empty: all).
    Reset(String),
    /// A new text for setting `key`.
    Text(String),
    /// What to search for.
    Search,
}

/// The interface-7 settings plugin.
#[derive(Debug, Default)]
pub struct NativeSettings {
    ui: Option<Ui>,
    screens: Screens<View>,
    asked: Option<(RequestId, Asked)>,
}

impl Plugin for NativeSettings {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
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
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        match event {
            Event::ScreenOpened {
                screen,
                instance,
                handover,
                ..
            } => self.opened(host, screen, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.shown(host, token, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            Event::SettingChanged { key, .. } if token == EVENTS => self.changed(host, key),
            Event::PluginsChanged { .. } | Event::RegistryChanged { .. } if token == EVENTS => {
                self.changed(host, "")
            }
            Event::PreviewChanged { .. } if token == EVENTS => Self::redraw(host),
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let mut messages = Messages::new()?;
        match view {
            View::List(v) => {
                let title = messages.copy(host, "settings.title")?;
                v.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
            View::Group(v) => {
                let title = copy(&v.title)?;
                v.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
            View::Choose(v) => {
                let title = copy(&v.title)?;
                v.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
            View::Themes(v) => {
                let title = messages.copy(host, "settings.theme")?;
                v.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
            View::Search(v) => {
                let title = messages.format(
                    host,
                    "settings.search.title",
                    &[Arg::Text {
                        name: "query",
                        value: &v.query,
                    }],
                )?;
                let none = messages.copy(host, "settings.search.none")?;
                let footer = if v.hits.is_empty() {
                    Footer::Empty {
                        title: &none,
                        text: "",
                    }
                } else {
                    Footer::None
                };
                v.list.draw_with(host, &mut frame, &title, true, footer)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // A number being changed with the buttons takes them.
        if let View::Group(v) = view
            && v.editing.is_some()
            && let Some(handled) = Self::edit_input(host, v, &input)?
        {
            return Ok(handled);
        }
        let step = view.list().input(host, &input)?;
        let confirm = matches!(
            input,
            Input::Action {
                action: "core.confirm",
                ..
            }
        );
        match step {
            ItemStep::Chosen(index) => self.chosen(host, token, index, confirm)?,
            ItemStep::Step(index, direction) => self.stepped(host, index, i64::from(direction))?,
            ItemStep::Held(_) => return Ok(Handled::No),
            _ => {}
        }
        Ok(ItemList::handled(step))
    }
}

/// The `View` of an instance, mutably.
fn group_mut(screens: &mut Screens<View>, token: u32) -> Option<&mut GroupView> {
    match screens.top(token)? {
        View::Group(v) => Some(v),
        _ => None,
    }
}

impl NativeSettings {
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

    fn notify(host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        let mut scratch = buffer(1024)?;
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    }

    /// Opens `screen` with the hand-over `params`.
    fn open(&self, host: &Host<'_>, screen: &str, params: &[(&str, &str)]) -> Result<(), Error> {
        let mut made = Vec::new();
        for (key, value) in params {
            push(&mut made, Param::new(key, value)?)?;
        }
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let mut open = Open::screen(screen)?;
        if !made.is_empty() {
            open = open.handover(self.ui()?.handover(host, &made, &mut input, &mut output)?);
        }
        let mut scratch = buffer(1024)?;
        CommandSender::new(host, &mut scratch).open(open)
    }

    /// Opens a screen filling `slot` with the hand-over `params`.
    fn open_slot(&self, host: &Host<'_>, slot: &str, params: &[(&str, &str)]) -> Result<(), Error> {
        let mut made = Vec::new();
        for (key, value) in params {
            push(&mut made, Param::new(key, value)?)?;
        }
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let handover = self.ui()?.handover(host, &made, &mut input, &mut output)?;
        let mut scratch = buffer(1024)?;
        CommandSender::new(host, &mut scratch).open(Open::slot(slot)?.handover(handover))
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        screen: &str,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        let (mut id, mut focus, mut text, mut scope, mut more) = (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            false,
        );
        if let Some(handover) = handover {
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut output = buffer(SERVICE_BUFFER)?;
            for param in self.ui()?.params(host, handover, &mut input, &mut output)? {
                let param = param?;
                match param.key() {
                    "id" => id = copy(param.value())?,
                    "focus" => focus = copy(param.value())?,
                    "text" => text = copy(param.value())?,
                    "scope" => scope = copy(param.value())?,
                    "more" => more = param.value() == "1",
                    _ => {}
                }
            }
        }
        if !scope.is_empty() {
            let mut scratch = buffer(1024)?;
            CommandSender::new(host, &mut scratch).set_scope(Some(&scope))?;
        }
        let mut reader = Reader::open(host)?;
        let view = Self::build(
            host,
            &mut reader,
            screen,
            (id, focus, text, !scope.is_empty(), more),
        );
        let view = reader.finish(view)?;
        self.screens.open(instance, view)
    }

    fn build(
        host: &Host<'_>,
        reader: &mut Reader<'_, '_>,
        screen: &str,
        (id, focus, text, scoped, more): (String, String, String, bool, bool),
    ) -> Result<View, Error> {
        Ok(match screen {
            "settings.list" if id.is_empty() && focus.is_empty() => {
                let mut view = ListView::default();
                build_list(host, reader, &mut view)?;
                View::List(view)
            }
            // A group, or the page of a setting to focus, on the main screen.
            "settings.list" | "settings.group" => {
                let mut view = GroupView {
                    group: id,
                    more,
                    focus,
                    scoped,
                    touch: paperpaw_sdk::native::snapshot::capture_snapshot(host)?.has_touch(),
                    ..GroupView::default()
                };
                build_group(host, reader, &mut view)?;
                View::Group(view)
            }
            "settings.choose" => {
                let mut view = ChooseView {
                    key: id,
                    ..ChooseView::default()
                };
                build_choose(host, reader, &mut view)?;
                View::Choose(view)
            }
            "settings.themes" => {
                let mut view = ThemesView::default();
                build_themes(host, reader, &mut view)?;
                View::Themes(view)
            }
            "settings.search" => {
                let mut view = SearchView {
                    query: copy(text.trim())?,
                    ..SearchView::default()
                };
                build_search(host, reader, &mut view)?;
                View::Search(view)
            }
            _ => return Err(Error::NotFound),
        })
    }

    /// Makes one view anew, keeping its focus.
    fn rebuild(host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let mut reader = Reader::open(host)?;
        let made = match view {
            View::List(v) => build_list(host, &mut reader, v),
            View::Group(v) => build_group(host, &mut reader, v),
            View::Choose(v) => build_choose(host, &mut reader, v),
            View::Themes(v) => build_themes(host, &mut reader, v),
            View::Search(_) => Ok(()),
        };
        reader.finish(made)?;
        *view.stale() = false;
        Ok(())
    }

    fn shown(&mut self, host: &Host<'_>, token: u32, instance: Instance) -> Result<(), Error> {
        self.screens.shown(token, instance)?;
        if let Some(view) = self.screens.get(instance)
            && *view.stale()
        {
            Self::rebuild(host, view)?;
        }
        Ok(())
    }

    /// A setting (`key`; empty: some) or the plugins changed: what shows is
    /// made anew where it is on top, and noted where it is covered.
    fn changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let tops = [LIST, GROUP, CHOOSE, THEMES];
        let mut any = false;
        for token in tops {
            let Some(top) = self.screens.top_instance(token) else {
                continue;
            };
            let Some(view) = self.screens.get(top) else {
                continue;
            };
            // A value of a row alone changes that row; anything else, the page.
            if let View::Group(v) = view
                && !key.is_empty()
                && v.editing.is_none()
                && refresh_row(host, v, key)?
            {
                any = true;
                continue;
            }
            Self::rebuild(host, view)?;
            any = true;
        }
        // Covered pages are made anew when they show.
        for view in self.screens.all() {
            *view.stale() = true;
        }
        for token in tops {
            if let Some(top) = self.screens.top_instance(token)
                && let Some(view) = self.screens.get(top)
            {
                *view.stale() = false;
            }
        }
        if any { Self::redraw(host) } else { Ok(()) }
    }

    fn chosen(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        confirm: bool,
    ) -> Result<(), Error> {
        match token {
            LIST => self.list_chosen(host, index),
            GROUP => self.group_chosen(host, index, confirm),
            CHOOSE => self.choose_chosen(host, index),
            THEMES => self.themes_chosen(host, index),
            SEARCH => self.search_chosen(host, index),
            _ => Ok(()),
        }
    }

    fn list_chosen(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(View::List(v)) = self.screens.top(LIST) else {
            return Ok(());
        };
        let Some(link) = v.links.get(index).cloned().flatten() else {
            return Ok(());
        };
        match link {
            Link::Group { id, page } => {
                if page.is_empty() {
                    self.open(host, "settings.group", &[("id", &id)])
                } else {
                    // The group's own page, a screen of its plugin.
                    self.open(host, &page, &[("id", &id)])
                }
            }
            Link::Page(action) => {
                let mut scratch = buffer(256)?;
                CommandSender::new(host, &mut scratch).run_action(&action)
            }
            Link::Advanced => {
                let on = native_data::read_bool(host, model::ADVANCED)?;
                let mut scratch = buffer(256)?;
                CommandSender::new(host, &mut scratch)
                    .set_setting(model::ADVANCED, SettingValue::Bool(!on))
            }
            Link::Search => {
                let title = Messages::new()?.copy(host, "settings.search")?;
                self.ask(host, Question::text(&title, "")?, Asked::Search)
            }
            Link::Reset => self.ask_reset(host, "", ""),
        }
    }

    fn group_chosen(&mut self, host: &Host<'_>, index: usize, confirm: bool) -> Result<(), Error> {
        let Some(v) = group_mut(&mut self.screens, GROUP) else {
            return Ok(());
        };
        let group = copy(&v.group)?;
        let title = copy(&v.title)?;
        let Some(row) = v.rows.get(index) else {
            return Ok(());
        };
        match row {
            Row::Heading(_) => Ok(()),
            Row::Reset => self.ask_reset(host, &group, &title),
            Row::Theme => self.open(host, "settings.themes", &[]),
            Row::Action(action, ..) => {
                let action = copy(action)?;
                let mut scratch = buffer(256)?;
                CommandSender::new(host, &mut scratch).run_action(&action)
            }
            Row::Part(part, _) => {
                let part = copy(part)?;
                self.open(host, "settings.group", &[("id", &part)])
            }
            Row::More => self.open(host, "settings.group", &[("id", &group), ("more", "1")]),
            Row::Setting(entry) => {
                let setting = entry.setting.clone();
                let detail = entry.detail.clone();
                let options = entry.options.len();
                self.activate(host, index, &setting, &detail, options, confirm)
            }
        }
    }

    /// What choosing a setting does: open its editor, ask for text, change a
    /// switch or a choice of two at once, or start changing a number.
    fn activate(
        &mut self,
        host: &Host<'_>,
        index: usize,
        setting: &Setting,
        detail: &Detail,
        options: usize,
        confirm: bool,
    ) -> Result<(), Error> {
        match detail.editor_kind {
            1 => return self.open(host, &detail.editor, &[("id", &setting.key)]),
            2 => return self.open_slot(host, &detail.editor, &[("id", &setting.key)]),
            _ => {}
        }
        match setting.kind {
            Kind::Int => {
                let touch = group_mut(&mut self.screens, GROUP).is_some_and(|v| v.touch);
                // Touch screens change numbers with their − and +; the
                // confirm button starts it anywhere.
                if !touch || confirm {
                    self.start_editing(host, index)?;
                }
                Ok(())
            }
            Kind::Bool => self.change(host, index, 1),
            Kind::Choice | Kind::Language if options == 2 => self.change(host, index, 1),
            Kind::Choice | Kind::Language => {
                self.open_slot(host, "core.choose-value", &[("id", &setting.key)])
            }
            Kind::Text if !setting.has(flag::SECRET) => {
                let title = copy(&setting.title)?;
                let initial = match &setting.value {
                    Some(Val::Text(text)) => copy(text)?,
                    _ => String::new(),
                };
                let mut input = buffer(SERVICE_BUFFER)?;
                let mut output = buffer(SERVICE_BUFFER)?;
                let request =
                    self.ui()?
                        .ask(host, Question::text(&title, "")?, &mut input, &mut output)?;
                self.ui()?.text_write(
                    host,
                    request.text(),
                    0,
                    &initial,
                    TextWrite::Last,
                    &mut input,
                    &mut output,
                )?;
                self.asked = Some((request, Asked::Text(copy(&setting.key)?)));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn start_editing(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(v) = group_mut(&mut self.screens, GROUP) else {
            return Ok(());
        };
        if let Some(Row::Setting(entry)) = v.rows.get(index)
            && let Some(Val::Int(now)) = entry.setting.value
        {
            v.editing = Some(Editing {
                row: index,
                before: now,
                now,
            });
            v.rebuild_items(host)?;
            Self::redraw(host)?;
        }
        Ok(())
    }

    /// A tap on the − or + of number row `index`.
    fn stepped(&mut self, host: &Host<'_>, index: usize, direction: i64) -> Result<(), Error> {
        self.change(host, index, direction)
    }

    /// Changes the setting of row `index` by `direction` steps at once.
    fn change(&mut self, host: &Host<'_>, index: usize, direction: i64) -> Result<(), Error> {
        let Some(v) = group_mut(&mut self.screens, GROUP) else {
            return Ok(());
        };
        let Some(Row::Setting(entry)) = v.rows.get(index) else {
            return Ok(());
        };
        let next = match (entry.setting.kind, &entry.setting.value) {
            (Kind::Choice | Kind::Language, Some(Val::Text(now))) => {
                // Of two options, the other one.
                entry
                    .options
                    .iter()
                    .find(|(value, _)| value != now)
                    .map(|(value, _)| Val::Text(value.clone()))
            }
            (kind, Some(value)) => model::stepped(kind, value, &entry.detail, direction),
            _ => None,
        };
        let key = copy(&entry.setting.key)?;
        if let Some(next) = next {
            set_value(host, &key, &next)?;
            Self::redraw(host)?;
        }
        Ok(())
    }

    /// The buttons while a number is changed: left and right change it, confirm
    /// keeps it, back goes back to the value before.
    fn edit_input(
        host: &Host<'_>,
        v: &mut GroupView,
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
        let Some(editing) = v.editing.as_mut() else {
            return Ok(None);
        };
        let Some(Row::Setting(entry)) = v.rows.get(editing.row) else {
            v.editing = None;
            return Ok(None);
        };
        let key = copy(&entry.setting.key)?;
        let mut scratch = buffer(1024)?;
        if direction == 0 {
            let (keep, before, now) = (action == "core.confirm", editing.before, editing.now);
            v.editing = None;
            let mut commands = CommandSender::new(host, &mut scratch);
            if keep && now != before {
                commands.set_setting(&key, SettingValue::Int(now))?;
            }
            commands.end_preview()?;
            v.rebuild_items(host)?;
            Self::redraw(host)?;
            return Ok(Some(Handled::Yes));
        }
        let unit = entry.detail.step.max(1);
        let next = editing
            .now
            .saturating_add(direction * unit)
            .clamp(entry.detail.min, entry.detail.max.max(entry.detail.min));
        if next != editing.now {
            editing.now = next;
            let mut values = buffer(64)?;
            CommandSender::new(host, &mut scratch).preview_settings(
                &[PreviewSetting::new(&key, SettingValue::Int(next))],
                &mut values,
            )?;
            v.rebuild_items(host)?;
            Self::redraw(host)?;
        }
        Ok(Some(Handled::Yes))
    }

    fn choose_chosen(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(View::Choose(v)) = self.screens.top(CHOOSE) else {
            return Ok(());
        };
        let mut scratch = buffer(1024)?;
        if let Some(i) = index.checked_sub(v.options.len())
            && let Some((action, _)) = v.more.get(i)
        {
            return CommandSender::new(host, &mut scratch).run_action(action);
        }
        let Some((value, _)) = v.options.get(index) else {
            return Ok(());
        };
        let mut commands = CommandSender::new(host, &mut scratch);
        commands.close_screen()?;
        commands.set_setting(&v.key, SettingValue::Text(value))
    }

    fn themes_chosen(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(View::Themes(v)) = self.screens.top(THEMES) else {
            return Ok(());
        };
        let Some((id, _)) = v.themes.get(index) else {
            return Ok(());
        };
        let id = copy(id)?;
        let mut scratch = buffer(1024)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        if id.is_empty() {
            // None: the appearance as PaperPaw comes.
            let mut reader = Reader::open(host)?;
            let keys = look_settings(&mut reader);
            for key in reader.finish(keys)? {
                commands.reset_setting(ResetTarget::Setting(&key))?;
            }
        } else {
            commands.apply_preset(&id, paperpaw_sdk::native::commands::PresetMode::Apply)?;
        }
        commands.close_screen()
    }

    fn search_chosen(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(View::Search(v)) = self.screens.top(SEARCH) else {
            return Ok(());
        };
        match v.hits.get(index) {
            Some(Target::Setting { group, key }) => {
                let (group, key) = (copy(group)?, copy(key)?);
                self.open(host, "settings.group", &[("id", &group), ("focus", &key)])
            }
            Some(Target::Page(action)) => {
                let action = copy(action)?;
                let mut scratch = buffer(256)?;
                CommandSender::new(host, &mut scratch).run_action(&action)
            }
            None => Ok(()),
        }
    }

    fn ask(&mut self, host: &Host<'_>, question: Question<'_>, asked: Asked) -> Result<(), Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self.ui()?.ask(host, question, &mut input, &mut output)?;
        self.asked = Some((request, asked));
        Ok(())
    }

    /// Asks whether to reset the settings of `group` (`title`), or all.
    fn ask_reset(&mut self, host: &Host<'_>, group: &str, title: &str) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let question = if group.is_empty() {
            messages.copy(host, "settings.reset-all.question")?
        } else {
            messages.format(
                host,
                "settings.reset-page.question",
                &[Arg::Text {
                    name: "page",
                    value: title,
                }],
            )?
        };
        let heading = messages.copy(host, "settings.title")?;
        let yes = messages.copy(host, "settings.option.reset")?;
        let no = messages.copy(host, "settings.option.cancel")?;
        let choices = concat(&[&yes, "\n", &no])?;
        let asked = Asked::Reset(copy(group)?);
        self.ask(
            host,
            Question::choice(&heading, &question, &choices)?,
            asked,
        )
    }

    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match asked {
            Asked::Reset(group) if answer.choice() == 0 => self.reset(host, &group),
            Asked::Reset(_) => Ok(()),
            Asked::Text(key) => {
                let text = uitext::read_all(self.ui()?, host, request.text(), TEXT_LIMIT)?;
                let mut scratch = buffer(TEXT_LIMIT + 256)?;
                CommandSender::new(host, &mut scratch)
                    .set_setting(&key, SettingValue::Text(text.trim()))
            }
            Asked::Search => {
                let text = uitext::read_all(self.ui()?, host, request.text(), QUERY_LIMIT)?;
                let text = text.trim();
                if text.is_empty() {
                    return Ok(());
                }
                self.open(host, "settings.search", &[("text", text)])
            }
        }
    }

    /// Resets the settings `group` holds that show (empty: all but the
    /// language); what is kept on reset, secrets and the reader's own stay.
    fn reset(&mut self, host: &Host<'_>, group: &str) -> Result<(), Error> {
        let mut reader = Reader::open(host)?;
        let found = (|| {
            let groups = reader.groups()?;
            let all = reader.settings("")?;
            let cx = context(host, false)?;
            let mut keys: Vec<String> = Vec::new();
            for setting in &all {
                if setting.has(flag::KEPT_ON_RESET)
                    || setting.has(flag::SECRET)
                    || setting.is_default && !setting.in_scope
                    || setting.key == model::SCOPE_ONLY
                {
                    continue;
                }
                let chosen = if group.is_empty() {
                    setting.key != model::LANGUAGE
                } else {
                    model::group_is(&groups, setting, group)
                        && model::visible(&groups, cx, setting, None)
                };
                if chosen {
                    push(&mut keys, copy(&setting.key)?)?;
                }
            }
            Ok(keys)
        })();
        let keys = reader.finish(found)?;
        let mut scratch = buffer(512)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        for key in &keys {
            commands.reset_setting(ResetTarget::Setting(key))?;
        }
        // "Only this book" last: clearing it first would send later resets to
        // the global settings instead of the book's values.
        commands.reset_setting(ResetTarget::Setting(model::SCOPE_ONLY))?;
        Self::notify(host, "settings.reset-done")
    }
}

/// Stages a new value for `key`.
fn set_value(host: &Host<'_>, key: &str, value: &Val) -> Result<(), Error> {
    let mut scratch = buffer(TEXT_LIMIT + 256)?;
    let mut commands = CommandSender::new(host, &mut scratch);
    match value {
        Val::Bool(v) => commands.set_setting(key, SettingValue::Bool(*v)),
        Val::Int(v) => commands.set_setting(key, SettingValue::Int(*v)),
        Val::Text(v) => commands.set_setting(key, SettingValue::Text(v)),
    }
}

/// What decides which settings show, read now.
fn context(host: &Host<'_>, scoped: bool) -> Result<Context, Error> {
    Ok(Context {
        advanced: native_data::read_bool(host, model::ADVANCED)?,
        developer: native_data::read_bool(host, model::DEVELOPER)?,
        scoped,
    })
}

/// Whether `setting` is listed: `offered` and, unless `advanced_ok`, shown at
/// the current level. Its number of choices is asked only when that decides.
fn listed(
    reader: &mut Reader<'_, '_>,
    groups: &[Group],
    cx: Context,
    setting: &Setting,
    advanced_ok: bool,
) -> Result<bool, Error> {
    let first = if advanced_ok {
        model::offered(groups, cx, setting, None)
    } else {
        model::visible(groups, cx, setting, None)
    };
    if !first {
        return Ok(false);
    }
    if setting.kind != Kind::Choice {
        return Ok(true);
    }
    let options = reader.detail(&setting.key)?.choices;
    Ok(options >= 2)
}

/// The group a setting is listed in.
fn listed_in<'a>(groups: &[Group], setting: &'a Setting) -> &'a str {
    if groups.iter().any(|g| g.id == setting.group) {
        &setting.group
    } else {
        model::OTHER_GROUP
    }
}

/// The groups that hold a listed setting (`advanced_ok`: advanced ones too).
fn holding(
    reader: &mut Reader<'_, '_>,
    groups: &[Group],
    cx: Context,
    all: &[Setting],
    advanced_ok: bool,
) -> Result<Vec<String>, Error> {
    let mut found: Vec<String> = Vec::new();
    for setting in all {
        let group = listed_in(groups, setting);
        if !found.iter().any(|g| g == group) && listed(reader, groups, cx, setting, advanced_ok)? {
            push(&mut found, copy(group)?)?;
        }
    }
    Ok(found)
}

/// A catalog text; a key the catalog lacks is shown as it is.
fn translate(messages: &mut Messages, host: &Host<'_>, key: &str) -> Result<String, Error> {
    match messages.copy(host, key) {
        Err(Error::NotFound | Error::Invalid) => copy(key),
        other => other,
    }
}

fn on_off(messages: &mut Messages, host: &Host<'_>, on: bool) -> Result<String, Error> {
    messages.copy(host, if on { "core.on" } else { "core.off" })
}

/// The value of a setting as its row shows it.
fn value_text(
    messages: &mut Messages,
    host: &Host<'_>,
    entry: &Entry,
    editing: Option<i64>,
) -> Result<String, Error> {
    let setting = &entry.setting;
    let text = match (&setting.value, editing) {
        (_, Some(now)) => model::with_unit(now, &entry.detail.unit)?,
        (Some(Val::Bool(on)), _) => on_off(messages, host, *on)?,
        (Some(Val::Int(v)), _) => model::with_unit(*v, &entry.detail.unit)?,
        (Some(Val::Text(v)), _) => match entry.options.iter().find(|(value, _)| value == v) {
            Some((_, name)) => copy(name)?,
            None => copy(v)?,
        },
        (None, _) => String::new(),
    };
    let text = if editing.is_some() {
        concat(&["‹ ", &text, " ›"])?
    } else if entry.detail.editor_kind != 0
        || (matches!(setting.kind, Kind::Choice | Kind::Language) && entry.options.len() != 2)
    {
        concat(&[&text, " ›"])?
    } else {
        text
    };
    // Kept for the book alone: marked.
    if setting.in_scope {
        concat(&["• ", &text])
    } else {
        Ok(text)
    }
}

impl GroupView {
    /// The item for row `index`.
    fn item(&self, messages: &mut Messages, host: &Host<'_>, index: usize) -> Result<Item, Error> {
        Ok(match &self.rows[index] {
            Row::Heading(title) => Item::heading(title)?,
            Row::Setting(entry) => {
                let editing = self
                    .editing
                    .as_ref()
                    .filter(|e| e.row == index)
                    .map(|e| e.now);
                let value = value_text(messages, host, entry, editing)?;
                let mut item = Item::noted(&entry.setting.title, &value, &entry.detail.help)?;
                if self.touch
                    && entry.setting.kind == Kind::Int
                    && entry.detail.editor_kind == 0
                    && let Item::Row(row) = &mut item
                {
                    row.stepper = true;
                }
                item
            }
            Row::Reset => Item::row(&messages.copy(host, "settings.reset-page")?, "")?,
            Row::Theme => Item::row(&messages.copy(host, "settings.theme")?, "›")?,
            Row::Action(_, title, badge) => Item::row(title, badge)?,
            Row::Part(_, title) => Item::row(title, "›")?,
            Row::More => Item::row(&messages.copy(host, "settings.more")?, "›")?,
        })
    }

    /// Makes the list's items again from the rows (their values changed).
    fn rebuild_items(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(self.rows.len())
            .map_err(|_| Error::NoMemory)?;
        for index in 0..self.rows.len() {
            items.push(self.item(&mut messages, host, index)?);
        }
        self.list.replace(items);
        Ok(())
    }
}

/// Builds the main page.
fn build_list(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut ListView,
) -> Result<(), Error> {
    let groups = reader.groups()?;
    let all = reader.settings("")?;
    let cx = context(host, false)?;
    let held = holding(reader, &groups, cx, &all, false)?;
    let widgets = reader.widget_ids()?;
    let pages = reader.menu(model::PAGES_MENU)?;
    let mut messages = Messages::new()?;
    // (order, headings before entries of the same order, link or heading, title)
    let mut placed: Vec<(i32, bool, Option<Link>, String)> = Vec::new();
    for PageRow {
        action,
        section,
        order,
        title,
        ..
    } in pages
    {
        let link = (!section).then_some(Link::Page(action));
        push(&mut placed, (order, !section, link, title))?;
    }
    for index in model::main_groups(&groups, &held, |id| widgets.iter().any(|w| w == id))? {
        let g = &groups[index];
        let order = if g.order == 0 {
            model::UNORDERED
        } else {
            g.order
        };
        let link = Link::Group {
            id: copy(&g.id)?,
            page: copy(&g.page)?,
        };
        push(&mut placed, (order, true, Some(link), copy(&g.title)?))?;
    }
    push(
        &mut placed,
        (
            i32::MIN,
            true,
            Some(Link::Search),
            messages.copy(host, "settings.search")?,
        ),
    )?;
    push(
        &mut placed,
        (
            i32::MAX,
            true,
            Some(Link::Advanced),
            messages.copy(host, "settings.advanced")?,
        ),
    )?;
    push(
        &mut placed,
        (
            i32::MAX,
            true,
            Some(Link::Reset),
            messages.copy(host, "settings.reset-all")?,
        ),
    )?;
    paperpaw_ui_kit::mem::stable_sort_by(&mut placed, |a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    // Headings with nothing under them go.
    let keep: Vec<bool> = (0..placed.len())
        .map(|i| placed[i].2.is_some() || placed.get(i + 1).is_some_and(|next| next.2.is_some()))
        .collect();
    let advanced = cx.advanced;
    let mut items = Vec::new();
    view.links = Vec::new();
    for (i, (_, _, link, title)) in placed.into_iter().enumerate() {
        if !keep[i] {
            continue;
        }
        let item = match &link {
            None => Item::heading(&title)?,
            Some(Link::Advanced) => Item::row(&title, &on_off(&mut messages, host, advanced)?)?,
            Some(Link::Reset) => Item::row(&title, "")?,
            Some(_) => Item::row(&title, "›")?,
        };
        push(&mut items, item)?;
        push(&mut view.links, link)?;
    }
    view.list.replace(items);
    Ok(())
}

/// Builds a group's page.
fn build_group(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut GroupView,
) -> Result<(), Error> {
    let groups = reader.groups()?;
    let all = reader.settings("")?;
    let cx = context(host, view.scoped)?;
    let mut messages = Messages::new()?;
    let of = if view.group.is_empty() {
        // A setting to focus, on its own group's page.
        match all.iter().find(|s| s.key == view.focus) {
            Some(s) => copy(listed_in(&groups, s))?,
            None => copy(model::OTHER_GROUP)?,
        }
    } else {
        copy(&view.group)?
    };
    view.group = copy(&of)?;
    let title = match groups.iter().find(|g| g.id == of) {
        Some(g) => copy(&g.title)?,
        None => String::new(),
    };
    view.title = if view.more {
        concat(&[&title, " · ", &messages.copy(host, "settings.more")?])?
    } else {
        title
    };
    // The page's settings: the basic ones, or the advanced ones alone.
    let mut specs: Vec<&Setting> = Vec::new();
    let mut advanced_offered = false;
    for s in all.iter().filter(|s| model::group_is(&groups, s, &of)) {
        if view.more {
            if s.has(flag::ADVANCED) && listed(reader, &groups, cx, s, true)? {
                push(&mut specs, s)?;
            }
        } else {
            if listed(reader, &groups, cx, s, false)? {
                push(&mut specs, s)?;
            }
            advanced_offered |= s.has(flag::ADVANCED) && model::offered(&groups, cx, s, None);
        }
    }
    // A page with nothing basic shows its advanced settings at once.
    if specs.is_empty() && !view.more {
        for s in all
            .iter()
            .filter(|s| model::group_is(&groups, s, &of) && s.has(flag::ADVANCED))
        {
            if listed(reader, &groups, cx, s, true)? {
                push(&mut specs, s)?;
            }
        }
    }
    let has_more = !view.more && advanced_offered && !specs.iter().any(|s| s.has(flag::ADVANCED));
    let mut pool: Vec<Option<Entry>> = Vec::new();
    for s in &specs {
        let detail = reader.detail(&s.key)?;
        let options = if matches!(s.kind, Kind::Choice | Kind::Language) {
            reader.choices(&s.key)?
        } else {
            Vec::new()
        };
        push(
            &mut pool,
            Some(Entry {
                setting: (*s).clone(),
                detail,
                options,
            }),
        )?;
    }
    let rows = page_rows(
        host,
        reader,
        view,
        (&groups, cx, &all, &of),
        pool,
        (has_more, !specs.is_empty()),
    )?;
    view.rows = rows;
    view.rebuild_items(host)?;
    if !view.focus.is_empty() {
        let at = view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Setting(e) if e.setting.key == view.focus));
        if let Some(at) = at {
            view.list.set_focus(at);
        }
        view.focus = String::new();
    }
    Ok(())
}

/// The rows of a group's page from its settings: parts, actions, themes, the
/// settings by section, "More" and the reset.
fn page_rows(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &GroupView,
    (groups, cx, all, of): (&[Group], Context, &[Setting], &str),
    mut pool: Vec<Option<Entry>>,
    (has_more, has_specs): (bool, bool),
) -> Result<Vec<Row>, Error> {
    let mut messages = Messages::new()?;
    let mut rows: Vec<Row> = Vec::new();
    if !view.more {
        // Its parts, each a page of its own, where they hold something.
        let held = holding(reader, groups, cx, all, true)?;
        for g in groups.iter().filter(|g| g.parent == of) {
            if held.iter().any(|h| model::within(groups, h, &g.id)) {
                push(&mut rows, Row::Part(copy(&g.id)?, copy(&g.title)?))?;
            }
        }
        // What plugins do with the group.
        for page in reader.menu(of)? {
            if !page.section {
                push(&mut rows, Row::Action(page.action, page.title, copy("›")?))?;
            }
        }
        if of == LOOK_GROUP && !reader.presets(THEME_KIND)?.is_empty() {
            push(&mut rows, Row::Theme)?;
        }
    }
    let sections = model::section_order(pool.iter().flatten().map(|e| e.detail.section.as_str()))?;
    // Headings only where there is more than one part.
    let headed = sections.len() > 1;
    let sections: Vec<String> = sections.into_iter().map(copy).collect::<Result<_, _>>()?;
    for section in &sections {
        if headed && !section.is_empty() {
            push(
                &mut rows,
                Row::Heading(translate(&mut messages, host, section)?),
            )?;
        }
        for slot in pool.iter_mut() {
            if slot.as_ref().is_some_and(|e| e.detail.section == *section)
                && let Some(entry) = slot.take()
            {
                push(&mut rows, Row::Setting(Box::new(entry)))?;
            }
        }
    }
    if has_more {
        push(&mut rows, Row::More)?;
    }
    if has_specs {
        push(&mut rows, Row::Reset)?;
    }
    Ok(rows)
}

/// Reads one setting's value again into its row. `false` when the page does
/// not show it, or when its value decides what else shows (the page is made
/// anew then).
fn refresh_row(host: &Host<'_>, view: &mut GroupView, key: &str) -> Result<bool, Error> {
    let Some(at) = view
        .rows
        .iter()
        .position(|r| matches!(r, Row::Setting(e) if e.setting.key == key))
    else {
        return Ok(false);
    };
    if let Row::Setting(entry) = &mut view.rows[at] {
        if entry.setting.has(flag::GOVERNS) {
            return Ok(false);
        }
        entry.setting.value = native_data::read(host, key)?;
    }
    view.rebuild_items(host)?;
    Ok(true)
}

/// Builds the chooser of a setting's value.
fn build_choose(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut ChooseView,
) -> Result<(), Error> {
    let all = reader.settings("")?;
    let Some(setting) = all.iter().find(|s| s.key == view.key) else {
        return Ok(());
    };
    view.title = copy(&setting.title)?;
    view.options = reader.choices(&view.key)?;
    // More ways to choose: the entries of the menu named like the setting.
    view.more = Vec::new();
    for page in reader.menu(&view.key)? {
        if !page.section {
            push(&mut view.more, (page.action, page.title))?;
        }
    }
    let current = native_data::read(host, &view.key)?;
    let mut items = Vec::new();
    for (value, name) in &view.options {
        let mark = if matches!(&current, Some(Val::Text(now)) if now == value) {
            MARK
        } else {
            ""
        };
        push(&mut items, Item::row(name, mark)?)?;
    }
    for (_, title) in &view.more {
        push(&mut items, Item::row(title, "›")?)?;
    }
    let at = view
        .options
        .iter()
        .position(|(value, _)| matches!(&current, Some(Val::Text(now)) if now == value))
        .unwrap_or(0);
    view.list.replace(items);
    view.list.set_focus(at);
    Ok(())
}

/// The appearance settings a theme covers.
fn look_settings(reader: &mut Reader<'_, '_>) -> Result<Vec<String>, Error> {
    let groups = reader.groups()?;
    let all = reader.settings("")?;
    let mut keys = Vec::new();
    for s in &all {
        if model::within(&groups, listed_in(&groups, s), LOOK_GROUP)
            && !THEME_KEPT.contains(&s.key.as_str())
        {
            push(&mut keys, copy(&s.key)?)?;
        }
    }
    Ok(keys)
}

/// Builds the chooser of themes: none first, then the presets, the one in use marked.
fn build_themes(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut ThemesView,
) -> Result<(), Error> {
    let mut messages = Messages::new()?;
    view.themes = Vec::new();
    push(
        &mut view.themes,
        (String::new(), messages.copy(host, "settings.theme.none")?),
    )?;
    for (id, name) in reader.presets(THEME_KIND)? {
        push(&mut view.themes, (id, name))?;
    }
    let covered = look_settings(reader)?;
    let all = reader.settings("")?;
    let mut in_use: Option<usize> = None;
    // None: every covered setting has its default.
    if all
        .iter()
        .filter(|s| covered.contains(&s.key))
        .all(|s| s.is_default)
    {
        in_use = Some(0);
    }
    for index in 1..view.themes.len() {
        if in_use.is_some() {
            break;
        }
        let values = reader.preset_values(&view.themes[index].0)?;
        if model::applied(&all, &covered, &values) {
            in_use = Some(index);
        }
    }
    let mut items = Vec::new();
    for (index, (_, name)) in view.themes.iter().enumerate() {
        let mark = if in_use == Some(index) { MARK } else { "" };
        push(&mut items, Item::row(name, mark)?)?;
    }
    let at = in_use.unwrap_or(0);
    view.list.replace(items);
    view.list.set_focus(at);
    Ok(())
}

/// Finds the settings and pages whose names and descriptions hold the words.
fn build_search(
    host: &Host<'_>,
    reader: &mut Reader<'_, '_>,
    view: &mut SearchView,
) -> Result<(), Error> {
    let words = model::words(&view.query)?;
    view.hits = Vec::new();
    let mut items = Vec::new();
    if words.is_empty() {
        view.list.replace(items);
        return Ok(());
    }
    let groups = reader.groups()?;
    let all = reader.settings("")?;
    let cx = context(host, false)?;
    for s in &all {
        if !listed(reader, &groups, cx, s, false)? {
            continue;
        }
        let help = reader.detail(&s.key)?.help;
        if !model::matches(&concat(&[&s.title, " ", &help])?, &words)? {
            continue;
        }
        let group = listed_in(&groups, s);
        let note = groups
            .iter()
            .find(|g| g.id == group)
            .map_or("", |g| g.title.as_str());
        push(&mut items, Item::noted(&s.title, "›", note)?)?;
        push(
            &mut view.hits,
            Target::Setting {
                group: copy(group)?,
                key: copy(&s.key)?,
            },
        )?;
    }
    let mut messages = Messages::new()?;
    let settings_title = messages.copy(host, "settings.title")?;
    for page in reader.menu(model::PAGES_MENU)? {
        if !page.section && model::matches(&page.title, &words)? {
            push(&mut items, Item::noted(&page.title, "›", &settings_title)?)?;
            push(&mut view.hits, Target::Page(page.action))?;
        }
    }
    view.list.replace(items);
    Ok(())
}
