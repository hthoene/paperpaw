//! The settings screens as an interface-7 plugin: the main page, a group's
//! page, choosing a value, themes and searching, all lists of the shared
//! kit over the registry's settings, with host dialogs for questions.
use crate::native_data::{self, Fields, Reader};
use crate::native_model::{
    self as model, Context, Detail, Group, Kind, PageRow, Setting, Val, flag,
};
use crate::native_secrets::{self, Choice, SecretView, SecretsView, Typed};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, PreviewSetting, RedrawMode, ResetTarget},
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        formatting::Arg,
        registrations,
        reply::Reply,
        settings::SettingValue,
        ui::{Instance, Notify, Open, Param, Question, RequestId, Ui},
    },
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
    uitext,
};

mod build;
use build::{
    build_choose, build_group, build_list, build_search, build_themes, context, look_settings,
    refresh_row, set_value,
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
/// Passwords and keys.
pub const SECRETS: u32 = 15;
/// One password or key.
pub const SECRET: u32 = 16;
/// The event subscriptions' token.
pub const EVENTS: u32 = 3;
/// The action that opens the passwords and keys.
pub const OPEN_SECRETS: u32 = 4;

/// The longest search text.
const QUERY_LIMIT: usize = 256;
/// The longest text of a text setting.
const TEXT_LIMIT: usize = 4096;
const THEME_KIND: &str = "core.theme";
/// The kind of presets that give widgets a look as a whole.
const WIDGET_LOOK_KIND: &str = "core.widget-look";
/// Appearance settings a theme never changes: what the reader needs to read
/// the interface at all.
const THEME_KEPT: [&str; 4] = [
    "core.ui-size",
    "core.ui-contrast",
    "core.logo",
    "core.widgets.frame",
];
const LOOK_GROUP: &str = "core.group.look";
/// The marker of the theme in use.
const MARK: &str = "•";

/// What a row of the main page opens.
#[derive(Debug, PartialEq, Eq)]
enum Link {
    Group { id: String, page: String },
    Page(String),
    Advanced,
    Search,
    Reset,
    Restart,
}

/// The main page.
#[derive(Debug, Default)]
struct ListView {
    links: Vec<Option<Link>>,
    list: ItemList,
    stale: bool,
}

/// What the chooser of presets chooses: a theme, or a look of widgets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Look {
    #[default]
    Theme,
    Widgets,
}

impl Look {
    /// The kind of the presets chosen.
    fn kind(self) -> &'static str {
        match self {
            Look::Theme => THEME_KIND,
            Look::Widgets => WIDGET_LOOK_KIND,
        }
    }

    /// The translation key of its name.
    fn title(self) -> &'static str {
        match self {
            Look::Theme => "settings.theme",
            Look::Widgets => "settings.widget-look",
        }
    }

    /// The parameters that open its chooser.
    fn params(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Look::Theme => &[],
            Look::Widgets => &[("mode", "widgets")],
        }
    }
}

/// A row of a group's page.
#[derive(Debug)]
enum Row {
    Heading(String),
    /// A setting: its place in the view's entries.
    Setting(usize),
    Reset,
    /// An action plugins add to the group: its id, name and what it shows at the right.
    Action(String, String, String),
    /// The chooser of themes, or of looks of widgets.
    Look(Look),
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

/// What choosing a setting needs, copied out of its row.
#[derive(Debug)]
struct Act {
    kind: Kind,
    editor_kind: u32,
    editor: String,
    key: String,
    title: String,
    /// The text a text setting starts from.
    initial: String,
    secret: bool,
    options: usize,
}

impl Act {
    fn of(entry: &Entry) -> Result<Self, Error> {
        let initial = match (&entry.setting.value, entry.setting.kind) {
            (Some(Val::Text(text)), Kind::Text) => copy(text)?,
            _ => String::new(),
        };
        Ok(Self {
            kind: entry.setting.kind,
            editor_kind: entry.detail.editor_kind,
            editor: copy(&entry.detail.editor)?,
            key: copy(&entry.setting.key)?,
            title: copy(&entry.setting.title)?,
            initial,
            secret: entry.setting.has(flag::SECRET),
            options: entry.options.len(),
        })
    }
}

impl Link {
    /// A copy made with fallible allocation.
    fn try_clone(&self) -> Result<Self, Error> {
        Ok(match self {
            Self::Group { id, page } => Self::Group {
                id: copy(id)?,
                page: copy(page)?,
            },
            Self::Page(action) => Self::Page(copy(action)?),
            Self::Advanced => Self::Advanced,
            Self::Search => Self::Search,
            Self::Reset => Self::Reset,
            Self::Restart => Self::Restart,
        })
    }
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
    /// The settings the rows show, in the rows' order.
    entries: Vec<Entry>,
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
    /// Themes, or looks of widgets.
    look: Look,
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
    Secrets(SecretsView),
    Secret(SecretView),
}

impl View {
    fn list(&mut self) -> &mut ItemList {
        match self {
            Self::List(v) => &mut v.list,
            Self::Group(v) => &mut v.list,
            Self::Choose(v) => &mut v.list,
            Self::Themes(v) => &mut v.list,
            Self::Search(v) => &mut v.list,
            Self::Secrets(v) => &mut v.list,
            Self::Secret(v) => &mut v.list,
        }
    }

    fn stale(&mut self) -> &mut bool {
        match self {
            Self::List(v) => &mut v.stale,
            Self::Group(v) => &mut v.stale,
            Self::Choose(v) => &mut v.stale,
            Self::Themes(v) => &mut v.stale,
            Self::Search(v) => &mut v.stale,
            Self::Secrets(v) => &mut v.stale,
            Self::Secret(v) => &mut v.stale,
        }
    }
}

/// What an open question is for.
#[derive(Debug, PartialEq, Eq)]
enum Asked {
    /// Reset the settings of a group (empty: all).
    Reset(String),
    /// Restart PaperPaw.
    Restart,
    /// A new text for setting `key`.
    Text(String),
    /// What to search for.
    Search,
    /// A new value for the secret `name` of owner `plugin`.
    Secret { plugin: String, name: String },
    /// Whether to remove that secret.
    RemoveSecret { plugin: String, name: String },
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
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let Some(event) = read_event!(incoming; kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::ActionRun, kinds::SettingChanged, kinds::PluginsChanged, kinds::RegistryChanged, kinds::PreviewChanged, kinds::Answered)
        else {
            return Ok(());
        };
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
            Event::ActionRun { .. } if token == OPEN_SECRETS => {
                self.open(host, "settings.secrets", &[])
            }
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
                let title = messages.copy(host, v.look.title())?;
                v.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
            View::Secrets(v) => {
                let title = messages.copy(host, "settings.secrets")?;
                let none = messages.copy(host, "settings.secrets.none")?;
                let footer = if v.kept.is_empty() {
                    Footer::Empty {
                        title: &none,
                        text: "",
                    }
                } else {
                    Footer::None
                };
                v.list.draw_with(host, &mut frame, &title, true, footer)
            }
            View::Secret(v) => {
                let title = copy(&v.title)?;
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
            ItemStep::Step(index, direction) => {
                self.stepped(host, token, index, i64::from(direction))?;
            }
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
        let mut look = Look::Theme;
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
                    "mode" if param.value() == "widgets" => look = Look::Widgets,
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
            look,
        );
        let view = reader.finish(view)?;
        self.screens.open(instance, view)
    }

    fn build(
        host: &Host<'_>,
        reader: &mut Reader<'_, '_>,
        screen: &str,
        (id, focus, text, scoped, more): (String, String, String, bool, bool),
        look: Look,
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
                let mut view = ThemesView {
                    look,
                    ..ThemesView::default()
                };
                build_themes(host, reader, &mut view)?;
                View::Themes(view)
            }
            "settings.secrets" => {
                let mut view = SecretsView::default();
                view.build(host)?;
                View::Secrets(view)
            }
            "settings.secret" => {
                let (plugin, name) = native_secrets::split_id(&id).ok_or(Error::Invalid)?;
                let mut view = SecretView {
                    plugin: copy(plugin)?,
                    name: copy(name)?,
                    ..SecretView::default()
                };
                view.build(host)?;
                View::Secret(view)
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
            View::Secrets(v) => v.build(host),
            View::Secret(v) => v.build(host),
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
        // By what the screen shows, not by the token it was declared with: the
        // main screen shows a group's page when it is opened with an id or a
        // setting to focus.
        let Some(view) = self.screens.top(token) else {
            return Ok(());
        };
        match view {
            View::List(_) => self.list_chosen(host, token, index),
            View::Group(_) => self.group_chosen(host, token, index, confirm),
            View::Choose(_) => self.choose_chosen(host, token, index),
            View::Themes(_) => self.themes_chosen(host, token, index),
            View::Search(_) => self.search_chosen(host, token, index),
            View::Secrets(_) => self.secrets_chosen(host, token, index),
            View::Secret(_) => self.secret_chosen(host, token, index),
        }
    }

    /// A secret of the list: what can be done with it.
    fn secrets_chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(View::Secrets(v)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(secret) = v.kept.get(index) else {
            return Ok(());
        };
        let id = native_secrets::id_of(&secret.plugin, &secret.name)?;
        self.open(host, "settings.secret", &[("id", &id)])
    }

    /// Type a secret anew, or remove it (asked first: it cannot be had back).
    fn secret_chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(View::Secret(v)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(choice) = v.choices.get(index).copied() else {
            return Ok(());
        };
        let (plugin, name, title) = (copy(&v.plugin)?, copy(&v.name)?, copy(&v.title)?);
        match choice {
            Choice::Change => {
                // Typed as a password: masked, never put on the clipboard.
                let question = Question::text(&title, "")?.secret()?;
                self.ask(host, question, Asked::Secret { plugin, name })
            }
            // One that is not kept (any more) only goes back.
            Choice::Remove if title.is_empty() => {
                native_secrets::remove(host, &plugin, &name)?;
                self.secrets_gone(host)
            }
            Choice::Remove => {
                let mut say = Messages::new()?;
                let question = say.format(
                    host,
                    "settings.secret.remove.question",
                    &[Arg::Text {
                        name: "name",
                        value: &title,
                    }],
                )?;
                let text = say.copy(host, "settings.secret.remove.text")?;
                let yes = say.copy(host, "settings.secret.remove")?;
                let no = say.copy(host, "settings.secret.keep")?;
                let labels = concat(&[&yes, "\n", &no])?;
                let question = Question::confirm(&question, &text)?.confirm_labels(&labels)?;
                self.ask(host, question, Asked::RemoveSecret { plugin, name })
            }
        }
    }

    /// The secrets changed: the pages that show them are made anew when they
    /// show, and the screen of the secret goes.
    fn secrets_gone(&mut self, host: &Host<'_>) -> Result<(), Error> {
        for view in self.screens.all() {
            *view.stale() = true;
        }
        let mut scratch = buffer(64)?;
        CommandSender::new(host, &mut scratch).back()
    }

    fn list_chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(View::List(v)) = self.screens.top(token) else {
            return Ok(());
        };
        let link = match v.links.get(index) {
            Some(Some(link)) => link.try_clone()?,
            _ => return Ok(()),
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
            Link::Restart => self.ask_restart(host),
        }
    }

    fn group_chosen(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        confirm: bool,
    ) -> Result<(), Error> {
        let Some(v) = group_mut(&mut self.screens, token) else {
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
            Row::Look(look) => {
                let look = *look;
                self.open(host, "settings.themes", look.params())
            }
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
            Row::Setting(at) => {
                let Some(entry) = v.entries.get(*at) else {
                    return Ok(());
                };
                let act = Act::of(entry)?;
                self.activate(host, token, index, act, confirm)
            }
        }
    }

    /// What choosing a setting does: open its editor, ask for text, change a
    /// switch or a choice of two at once, or start changing a number.
    fn activate(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        act: Act,
        confirm: bool,
    ) -> Result<(), Error> {
        match act.editor_kind {
            1 => return self.open(host, &act.editor, &[("id", &act.key)]),
            2 => return self.open_slot(host, &act.editor, &[("id", &act.key)]),
            _ => {}
        }
        match act.kind {
            Kind::Int => {
                let touch = group_mut(&mut self.screens, token).is_some_and(|v| v.touch);
                // Touch screens change numbers with their − and +; the
                // confirm button starts it anywhere.
                if !touch || confirm {
                    self.start_editing(host, token, index)?;
                }
                Ok(())
            }
            Kind::Bool => self.change(host, token, index, 1),
            Kind::Choice | Kind::Language if act.options == 2 => self.change(host, token, index, 1),
            Kind::Choice | Kind::Language => {
                self.open_slot(host, "core.choose-value", &[("id", &act.key)])
            }
            Kind::Text if !act.secret => {
                let mut input = buffer(SERVICE_BUFFER)?;
                let mut output = buffer(SERVICE_BUFFER)?;
                let request = self.ui()?.ask(
                    host,
                    Question::text(&act.title, "")?,
                    &mut input,
                    &mut output,
                )?;
                // In parts: the text may be longer than one part holds.
                uitext::write_all(self.ui()?, host, request.text(), &act.initial)?;
                self.asked = Some((request, Asked::Text(act.key)));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn start_editing(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(v) = group_mut(&mut self.screens, token) else {
            return Ok(());
        };
        if let Some(entry) = v.entry(index)
            && let Some(Val::Int(now)) = entry.setting.value
        {
            // Everything that can be refused first; then the state changes.
            let items = v.items_for(host, (&v.rows, &v.entries), Some((index, now)))?;
            Self::redraw(host)?;
            v.editing = Some(Editing {
                row: index,
                before: now,
                now,
            });
            v.list.replace(items);
        }
        Ok(())
    }

    /// A tap on the − or + of number row `index`.
    fn stepped(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        direction: i64,
    ) -> Result<(), Error> {
        self.change(host, token, index, direction)
    }

    /// Changes the setting of row `index` by `direction` steps at once.
    fn change(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        direction: i64,
    ) -> Result<(), Error> {
        let Some(v) = group_mut(&mut self.screens, token) else {
            return Ok(());
        };
        let Some(entry) = v.entry(index) else {
            return Ok(());
        };
        let next = match (entry.setting.kind, &entry.setting.value) {
            (Kind::Choice | Kind::Language, Some(Val::Text(now))) => {
                // Of two options, the other one.
                match entry.options.iter().find(|(value, _)| value != now) {
                    Some((value, _)) => Some(Val::Text(copy(value)?)),
                    None => None,
                }
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

    /// The buttons while a number is changed: right and up raise it, left and
    /// down lower it (as in Beta 3), confirm keeps it, back goes back to the
    /// value before. The state changes only once everything that can be refused
    /// went through.
    fn edit_input(
        host: &Host<'_>,
        v: &mut GroupView,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let Input::Action { action, .. } = *input else {
            return Ok(None);
        };
        let direction = match action {
            "core.right" | "core.up" | "core.next" | "core.next-page" => 1,
            "core.left" | "core.down" | "core.previous" | "core.prev-page" => -1,
            "core.confirm" | "core.back" => 0,
            _ => return Ok(None),
        };
        let Some(editing) = v.editing.as_ref() else {
            return Ok(None);
        };
        let (row, before, now) = (editing.row, editing.before, editing.now);
        let Some(entry) = v.entry(row) else {
            v.editing = None;
            return Ok(None);
        };
        let key = copy(&entry.setting.key)?;
        let (unit, min, max) = (entry.detail.step.max(1), entry.detail.min, entry.detail.max);
        let mut scratch = buffer(1024)?;
        if direction == 0 {
            let items = v.items_for(host, (&v.rows, &v.entries), None)?;
            let mut commands = CommandSender::new(host, &mut scratch);
            if action == "core.confirm" && now != before {
                commands.set_setting(&key, SettingValue::Int(now))?;
            }
            commands.end_preview()?;
            Self::redraw(host)?;
            v.editing = None;
            v.list.replace(items);
            return Ok(Some(Handled::Yes));
        }
        let next = now
            .saturating_add(direction * unit)
            .clamp(min, max.max(min));
        if next != now {
            let items = v.items_for(host, (&v.rows, &v.entries), Some((row, next)))?;
            let mut values = buffer(64)?;
            CommandSender::new(host, &mut scratch).preview_settings(
                &[PreviewSetting::new(&key, SettingValue::Int(next))],
                &mut values,
            )?;
            Self::redraw(host)?;
            if let Some(editing) = v.editing.as_mut() {
                editing.now = next;
            }
            v.list.replace(items);
        }
        Ok(Some(Handled::Yes))
    }

    fn choose_chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(View::Choose(v)) = self.screens.top(token) else {
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

    fn themes_chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(View::Themes(v)) = self.screens.top(token) else {
            return Ok(());
        };
        let look = v.look;
        let Some((id, _)) = v.themes.get(index) else {
            return Ok(());
        };
        let id = copy(id)?;
        let mut scratch = buffer(1024)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        if id.is_empty() {
            // None: the appearance as PaperPaw comes.
            let mut reader = Reader::open(host)?;
            let keys = look_settings(&mut reader, look);
            for key in reader.finish(keys)? {
                commands.reset_setting(ResetTarget::Setting(&key))?;
            }
        } else {
            commands.apply_preset(&id, paperpaw_sdk::native::commands::PresetMode::Apply)?;
        }
        commands.close_screen()
    }

    fn search_chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(View::Search(v)) = self.screens.top(token) else {
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

    /// Asks whether to restart PaperPaw (what plugins saved goes along).
    fn ask_restart(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let question = messages.copy(host, "settings.restart.question")?;
        let heading = messages.copy(host, "settings.title")?;
        let yes = messages.copy(host, "settings.option.restart")?;
        let no = messages.copy(host, "settings.option.cancel")?;
        // Cancel first: it is what the dialog starts on.
        let choices = concat(&[&no, "\n", &yes])?;
        self.ask(
            host,
            Question::choice(&heading, &question, &choices)?,
            Asked::Restart,
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
            Asked::Restart if answer.choice() == 1 => {
                let mut scratch = buffer(256)?;
                // Refused where the shell does not allow it (`uses.power` is
                // not required): say so, and let that notice fail silently.
                if CommandSender::new(host, &mut scratch).restart().is_err() {
                    Self::notify(host, "settings.restart.refused").unwrap_or(());
                }
                Ok(())
            }
            Asked::Restart => Ok(()),
            Asked::Text(key) => {
                let text = uitext::read_all(self.ui()?, host, request.text(), TEXT_LIMIT)?;
                let mut scratch = buffer(TEXT_LIMIT + 256)?;
                CommandSender::new(host, &mut scratch)
                    .set_setting(&key, SettingValue::Text(text.trim()))
            }
            Asked::Secret { plugin, name } => {
                let typed = native_secrets::read_typed(self.ui()?, host, request)?;
                let done = match &typed {
                    Typed::Value(value) => native_secrets::change(host, &plugin, &name, value),
                    Typed::TooLong => {
                        native_secrets::wipe_typed(typed);
                        return Self::notify(host, "settings.secret.too-long");
                    }
                    // Nothing typed keeps the secret as it was.
                    Typed::Nothing => Ok(()),
                };
                native_secrets::wipe_typed(typed);
                match done {
                    Ok(()) => self.secrets_gone(host),
                    Err(Error::NoMemory) => Err(Error::NoMemory),
                    // Not saved (card away, secret gone, not changeable): said.
                    Err(_) => Self::notify(host, "settings.secret.failed"),
                }
            }
            Asked::RemoveSecret { plugin, name } if answer.choice() == 1 => {
                match native_secrets::remove(host, &plugin, &name) {
                    Ok(()) => self.secrets_gone(host),
                    Err(Error::NoMemory) => Err(Error::NoMemory),
                    Err(_) => Self::notify(host, "settings.secret.failed"),
                }
            }
            Asked::RemoveSecret { .. } => Ok(()),
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
        // A placement's page: what that placement set goes, the widget's own
        // settings stay.
        if model::unplaced(group).is_some() {
            let mut scratch = buffer(512)?;
            return match CommandSender::new(host, &mut scratch)
                .reset_setting(ResetTarget::Placement(group))
            {
                Ok(()) => Self::notify(host, "settings.reset-done"),
                Err(Error::NoMemory) => Err(Error::NoMemory),
                // Refused (not allowed, queue full): said, not silent.
                Err(_) => Self::notify(host, "settings.reset-failed"),
            };
        }
        let mut reader = Reader::open(host)?;
        let found = (|| {
            let groups = reader.groups()?;
            let cx = context(host, false)?;
            let mut keys: Vec<String> = Vec::new();
            reader.each_setting(
                "",
                Fields::Light,
                |_| true,
                |_, s| model::reset_step(&mut keys, &groups, cx, &s, group),
            )?;
            model::reset_end(&mut keys)?;
            Ok(keys)
        })();
        let keys = reader.finish(found)?;
        let mut scratch = buffer(512)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        for key in &keys {
            commands.reset_setting(ResetTarget::Setting(key))?;
        }
        Self::notify(host, "settings.reset-done")
    }
}
