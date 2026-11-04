//! *Passwords and keys*: every password and key plugins keep (see
//! [`paperpaw_sdk::secrets`]) at a glance, never shown; a tap offers to
//! type it anew (on the keyboard or, where offered, on a phone) or to
//! remove it.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{Confirmed, Typed};
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Commands, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Screen,
};

use crate::{ID, page};

/// The list.
pub(crate) const SCREEN: Id = Id::new_static("settings.secrets");
/// What to do with one; parameter `id`.
pub(crate) const SECRET_SCREEN: Id = Id::new_static("settings.secret");
/// Opens the list.
pub(crate) const OPEN: Id = Id::new_static("settings.secrets.open");
/// Emitted by the keyboard with a secret typed anew (parameters `id` and
/// `text`).
pub(crate) const ENTERED: Event<Typed> = Event::new("settings.secret-entered");
/// The answer to "remove it?": it cannot be had back.
const REMOVE: Event<Confirmed> = Event::new("settings.secret-remove");

/// How a kept secret shows.
const HIDDEN: &str = "••••••";

/// Keeps a secret typed anew.
pub(crate) fn entered(env: &Env<'_>, typed: &Typed, out: &mut Commands) {
    let Some(id) = typed.context.get(ID) else {
        return;
    };
    let text = typed.text.trim();
    if !text.is_empty() && env.secrets().has(id) {
        out.push(Command::set_secret(SecretChange::keep(id, "", text)));
    }
}

/// The list of secrets.
pub(crate) struct SecretList {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
}

impl SecretList {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let (style, header, area) = page(env);
        Self {
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            style,
            header,
        }
    }

    fn open(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let id = cx
            .env()
            .secrets()
            .listed()
            .nth(index)
            .map(|s| s.id.to_string());
        if let Some(id) = id {
            cx.command(Command::OpenScreen(
                SECRET_SCREEN,
                Params::new().with(ID, id),
            ));
        }
    }
}

impl Screen for SecretList {
    /// No pictures on it: whole-screen frames need no extra pass to count
    /// them (see [`Screen::pictures_in_grays`]).
    fn pictures_in_grays(&self) -> bool {
        false
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let len = env.secrets().listed().count();
        let title = env.i18n().tr("settings.secrets");
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        if len == 0 {
            let area = self.list.area();
            ui.message(area, env.i18n().tr("settings.secrets.none"));
            return;
        }
        let secrets: Vec<_> = env.secrets().listed().collect();
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let label = env.i18n().tr(secrets[index].label);
            ui::Row::value(label, HIDDEN)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = cx.env().secrets().listed().count();
        if let Some(change) = self.list.navigate_in(cx.env(), action, len) {
            change.request(cx);
            return Handled::Yes;
        }
        if *action == actions::CONFIRM {
            self.open(self.list.focus(), cx);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = cx.env().secrets().listed().count();
        let paging = self.list.paging(len);
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, len) {
            Some(index) => {
                self.list.set_focus(index);
                self.open(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}

/// What can be done with one secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Change,
    Remove,
}

/// Change or remove one secret.
pub(crate) struct SecretPage {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    id: String,
    title: String,
    choices: Vec<Choice>,
}

impl SecretPage {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let (style, header, area) = page(env);
        let id = params.get(ID).unwrap_or_default().to_string();
        let secret = env.secrets().listed().find(|s| s.id == id);
        let title = secret.map_or_else(String::new, |s| env.i18n().tr(s.label).to_string());
        let mut choices = Vec::new();
        if secret.is_some_and(|s| s.changeable) {
            choices.push(Choice::Change);
        }
        choices.push(Choice::Remove);
        Self {
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            style,
            header,
            id,
            title,
            choices,
        }
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        match self.choices.get(index) {
            Some(Choice::Change) => cx.command(
                paperpaw_sdk::ask::text(self.title.clone(), &ENTERED)
                    .password()
                    .with(ID, self.id.clone())
                    .command(),
            ),
            // Kept: asked first, as it cannot be had back.
            Some(Choice::Remove) if cx.env().secrets().has(&self.id) => {
                let env = *cx.env();
                let question = env.i18n().format(
                    "settings.secret.remove.question",
                    &[("name", paperpaw_sdk::Arg::Str(&self.title))],
                );
                cx.command(
                    paperpaw_sdk::ask::confirm(question, &REMOVE)
                        .text(env.i18n().tr("settings.secret.remove.text"))
                        .yes(env.i18n().tr("settings.secret.remove"))
                        .no(env.i18n().tr("settings.secret.keep"))
                        .command(),
                );
            }
            Some(Choice::Remove) => self.remove(cx),
            None => {}
        }
    }

    fn remove(&self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::set_secret(SecretChange::remove(&self.id)));
        cx.command(Command::Back);
    }
}

impl Screen for SecretPage {
    /// No pictures on it: whole-screen frames need no extra pass to count
    /// them (see [`Screen::pictures_in_grays`]).
    fn pictures_in_grays(&self) -> bool {
        false
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let len = self.choices.len();
        self.header
            .draw(&mut ui, &self.title, None, self.list.paging(len));
        let choices = &self.choices;
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let key = match choices[index] {
                Choice::Change => "settings.secret.change",
                Choice::Remove => "settings.secret.remove",
            };
            ui::Row::value(env.i18n().tr(key), "")
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if let Some(change) = self.list.navigate_in(cx.env(), action, self.choices.len()) {
            change.request(cx);
            return Handled::Yes;
        }
        if *action == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.choices.len());
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.choices.len()) {
            Some(index) => {
                self.list.set_focus(index);
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        // Typed anew: back to the list.
        if *event == ENTERED {
            cx.command(Command::Back);
        }
        if REMOVE.read(event, params).is_some_and(|answer| answer.yes) {
            self.remove(cx);
        }
    }
}
