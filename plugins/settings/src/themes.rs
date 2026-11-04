//! Choosing a theme: the presets of the kind `core.theme` (see
//! `paperpaw_sdk::presets::THEME`), offered at the top of the appearance
//! settings where any plugin brings one. A theme gives the appearance
//! settings its values and the others back their defaults; the user's
//! changes after it are theirs. *None* first gives them all their
//! defaults again. What the reader needs to read at all (the size of the
//! text, the contrast) no theme changes (`presets::theme_settings`).
//!
//! Looks of widgets (`presets::WIDGET_LOOK`) are chosen the same way,
//! below the themes: their frames, room, sizes and alignment
//! (`presets::widget_look_settings`).

use super::*;

use paperpaw_sdk::SettingValue;
use paperpaw_sdk::presets::{self, Preset};

/// The chooser of themes (or of looks of widgets, with parameter `mode`
/// [`WIDGETS`]).
pub(super) const SCREEN: Id = Id::new_static("settings.themes");
/// Looks of widgets rather than themes.
const WIDGETS: &str = "widgets";
/// No theme: the appearance as PaperPaw comes, first in the chooser.
const NONE: Id = Id::new_static("settings.theme-none");

/// What is chosen: a theme, or a look of widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Chooser {
    Theme,
    Widgets,
}

impl Chooser {
    fn of(params: &Params) -> Self {
        if params.get(paperpaw_sdk::builtin::params::MODE) == Some(WIDGETS) {
            Chooser::Widgets
        } else {
            Chooser::Theme
        }
    }

    /// The parameters that open its chooser.
    pub(super) fn params(self) -> Params {
        match self {
            Chooser::Theme => Params::new(),
            Chooser::Widgets => Params::new().with(paperpaw_sdk::builtin::params::MODE, WIDGETS),
        }
    }

    /// The translation key of its name.
    pub(super) fn title(self) -> &'static str {
        match self {
            Chooser::Theme => "settings.theme",
            Chooser::Widgets => "settings.widget-look",
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Chooser::Theme => presets::THEME,
            Chooser::Widgets => presets::WIDGET_LOOK,
        }
    }

    fn covered(self, env: &Env<'_>) -> Vec<Id> {
        match self {
            Chooser::Theme => presets::theme_settings(env.settings()),
            Chooser::Widgets => presets::widget_look_settings(env.settings(), env.widgets()),
        }
    }

    fn changes_of(self, env: &Env<'_>, preset: &Preset) -> Vec<(Id, Option<SettingValue>)> {
        match self {
            Chooser::Theme => presets::theme_changes(env.settings(), preset),
            Chooser::Widgets => presets::widget_look_changes(env.settings(), env.widgets(), preset),
        }
    }
}

/// What applying preset `id` (or [`NONE`]) makes of the settings.
fn changes(env: &Env<'_>, chooser: Chooser, id: &Id) -> Option<Vec<(Id, Option<SettingValue>)>> {
    if *id == NONE {
        let covered = chooser.covered(env);
        return Some(covered.into_iter().map(|id| (id, None)).collect());
    }
    let preset = themes(env, chooser).find(|theme| theme.id == *id)?;
    Some(chooser.changes_of(env, preset))
}

/// The presets there are, in the order of the plugins.
pub(super) fn themes<'e>(env: &Env<'e>, chooser: Chooser) -> impl Iterator<Item = &'e Preset> + 'e {
    env.settings().presets().of_kind(chooser.kind())
}

/// The preset in use: the one whose values the settings have, [`NONE`]
/// where they have their defaults.
fn applied(env: &Env<'_>, chooser: Chooser) -> Option<Id> {
    let is =
        |id: &Id| changes(env, chooser, id).is_some_and(|c| presets::applied(env.settings(), &c));
    if is(&NONE) {
        return Some(NONE);
    }
    themes(env, chooser)
        .map(|theme| theme.id.clone())
        .find(|id| is(id))
}

/// What the row shows: the preset in use, none, or the user's own.
pub(super) fn applied_name(env: &Env<'_>, chooser: Chooser) -> String {
    match applied(env, chooser) {
        Some(id) if id == NONE => env.i18n().tr("settings.theme.none").to_string(),
        Some(id) => themes(env, chooser)
            .find(|theme| theme.id == id)
            .map_or_else(String::new, |theme| env.i18n().tr(&theme.title).to_string()),
        None => env.i18n().tr("settings.theme.own").to_string(),
    }
}

#[derive(Debug)]
pub(super) struct Themes {
    /// The themes' ids, names and what each is.
    themes: Vec<(Id, String, String)>,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    chooser: Chooser,
}

impl Themes {
    pub(super) fn new(env: &Env<'_>, params: &Params) -> Self {
        let chooser = Chooser::of(params);
        let (style, header, mut list) = frame(env, true);
        let none = (
            NONE,
            env.i18n().tr("settings.theme.none").to_string(),
            env.i18n().tr("settings.theme.none.help").to_string(),
        );
        let themes: Vec<(Id, String, String)> = core::iter::once(none)
            .chain(themes(env, chooser).map(|theme| {
                let about = env.i18n().tr(&theme.description).to_string();
                (
                    theme.id.clone(),
                    env.i18n().tr(&theme.title).to_string(),
                    about,
                )
            }))
            .collect();
        let current = applied(env, chooser);
        list.set_focus(
            themes
                .iter()
                .position(|(id, ..)| Some(id) == current.as_ref())
                .unwrap_or(0),
        );
        Self {
            themes,
            style,
            header,
            list,
            chooser,
        }
    }

    /// Uses theme `index` and goes back to the settings.
    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some((id, ..)) = self.themes.get(index) else {
            return;
        };
        let env = *cx.env();
        let Some(changes) = changes(&env, self.chooser, id) else {
            return;
        };
        cx.command(Command::Back);
        for command in presets::commands(env.settings(), &changes) {
            cx.command(command);
        }
    }
}

impl Screen for Themes {
    /// No pictures on it: whole-screen frames need no extra pass to count
    /// them (see [`Screen::pictures_in_grays`]).
    fn pictures_in_grays(&self) -> bool {
        false
    }

    /// Turned or resized: laid out anew, the focus on its theme.
    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        let (style, header, fresh) = frame(cx.env(), true);
        // The focus, whether it shows and the page stay with the list.
        self.list.set_area(fresh.area());
        self.list.set_row(fresh.row());
        (self.style, self.header) = (style, header);
        cx.request_redraw();
        Handled::Yes
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let len = self.themes.len();
        let title = env.i18n().tr(self.chooser.title());
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        let current = applied(env, self.chooser);
        let themes = &self.themes;
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let (id, name, about) = &themes[index];
            let mark = if Some(id) == current.as_ref() {
                "•"
            } else {
                ""
            };
            ui::Row::value(name, mark)
                .note(about)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.themes.len();
        if let Some(change) = self.list.navigate_in(cx.env(), action, len) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.themes.len());
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.themes.len()) {
            Some(index) => {
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}
