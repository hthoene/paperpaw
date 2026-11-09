//! A catalog's details: its name, address, user name, password, the
//! folder its books are synced to, and whether *Sync catalogs* syncs it.
//! Every change is kept at once.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{FolderChosen, Typed};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode, Screen,
};

use crate::ID;
use crate::catalogs::{self, Catalog};

/// The keyboard's reply with a field's new text.
const ENTERED: Event<Typed> = Event::new("opds.entered");
/// The folder chooser's reply.
const FOLDER_CHOSEN: Event<FolderChosen> = Event::new("opds.folder-chosen");
/// Parameter of the reply: which field.
const FIELD: &str = "field";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Address,
    User,
    Password,
    Folder,
    Sync,
    WebDav,
}

const FIELDS: [Field; 7] = [
    Field::Name,
    Field::Address,
    Field::WebDav,
    Field::User,
    Field::Password,
    Field::Folder,
    Field::Sync,
];

impl Field {
    fn key(self) -> &'static str {
        match self {
            Field::Name => "opds.field.name",
            Field::Address => "opds.field.address",
            Field::User => "opds.field.user",
            Field::Password => "opds.field.password",
            Field::Folder => "opds.field.folder",
            Field::Sync => "opds.field.sync",
            Field::WebDav => "opds.field.webdav",
        }
    }

    /// What its row says below its name, for `catalog`.
    fn help(self, catalog: &Catalog) -> &'static str {
        match self {
            Field::Password if in_clear(&catalog.url) => "opds.field.password.http",
            Field::Name => "opds.field.name.help",
            Field::Address => "opds.field.address.help",
            Field::User => "opds.field.user.help",
            Field::Password => "opds.field.password.help",
            Field::Folder => "opds.field.folder.help",
            Field::Sync => "opds.field.sync.help",
            Field::WebDav => "opds.field.webdav.help",
        }
    }

    fn of(name: &str) -> Option<Field> {
        FIELDS.into_iter().find(|f| f.key() == name)
    }
}

/// Whether what is sent to `url` goes unencrypted (`http`), so anyone on
/// the way can read a password with it.
fn in_clear(url: &str) -> bool {
    url.get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
}

pub(crate) struct EditScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    index: usize,
    catalog: Catalog,
}

impl EditScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let index = params
            .get(ID)
            .and_then(|i| i.parse().ok())
            .unwrap_or(usize::MAX);
        let catalog = catalogs::load(env)
            .ok()
            .and_then(|all| all.get(index).cloned())
            .unwrap_or_default();
        Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            index,
            catalog,
        }
    }

    /// Keeps the catalog as it is now in the list, unless the list
    /// cannot be read: then it is left as it is, and the change is not
    /// kept.
    fn save(&self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        let Ok(mut all) = catalogs::load(env) else {
            let message = env.i18n().tr("opds.unreadable").to_string();
            return cx.command(Command::Notify(message));
        };
        if let Some(slot) = all.get_mut(self.index) {
            *slot = self.catalog.clone();
            let mut out = paperpaw_sdk::Commands::new();
            catalogs::save(env, &all, &mut out);
            for command in out {
                cx.command(command);
            }
        }
    }

    fn value(&self, env: &Env<'_>, field: Field) -> String {
        let c = &self.catalog;
        match field {
            Field::Name => c.name.clone(),
            Field::Address => c.url.clone(),
            Field::User => c.user.clone(),
            Field::Password if c.password.is_empty() => String::new(),
            Field::Password => String::from("••••"),
            Field::Folder => c.sync_folder(env),
            Field::Sync | Field::WebDav => String::new(),
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(field) = FIELDS.get(index).copied() else {
            return;
        };
        if field == Field::WebDav {
            self.catalog.webdav = !self.catalog.webdav;
            self.save(&env, cx);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
            return;
        }
        if field == Field::Sync {
            self.catalog.sync = !self.catalog.sync;
            self.save(&env, cx);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
            return;
        }
        if field == Field::Folder && env.has_slot(&slots::CHOOSE_FOLDER) {
            cx.command(
                ask::folder(&FOLDER_CHOSEN)
                    .start(self.catalog.sync_folder(&env))
                    .command(),
            );
            return;
        }
        let question = ask::text(env.i18n().tr(field.key()), &ENTERED).with(FIELD, field.key());
        let question = match field {
            Field::Password => question.password(),
            Field::Folder => question.initial(self.catalog.sync_folder(&env)),
            Field::Address | Field::WebDav => question.url().initial(self.value(&env, field)),
            _ => question.initial(self.value(&env, field)),
        };
        cx.command(question.command());
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        self.list
            .move_focus_in(cx.env(), delta, FIELDS.len())
            .request(cx);
    }
}

impl Screen for EditScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        // Rows as high as their notes need (the largest text); a note
        // changes with the catalog's kind.
        let notes = FIELDS.iter().map(|f| env.i18n().tr(f.help(&self.catalog)));
        let row = style.noted_row_for(env, self.list.area().width, notes);
        self.list.set_row(row);
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            &self.catalog.name,
            None,
            self.list.paging(FIELDS.len()),
        );
        let values: Vec<String> = FIELDS.iter().map(|&f| self.value(env, f)).collect();
        let (on, webdav) = (self.catalog.sync, self.catalog.webdav);
        let catalog = &self.catalog;
        self.list
            .draw(&mut ui, FIELDS.len(), |ui, index, rect, focused| {
                let field = FIELDS[index];
                let title = env.i18n().tr(field.key());
                let note = env.i18n().tr(field.help(catalog));
                if field == Field::Sync || field == Field::WebDav {
                    let on = if field == Field::Sync { on } else { webdav };
                    ui::Row::switch(title, on)
                        .note(note)
                        .focused(focused)
                        .draw(ui, rect);
                } else {
                    let value = &values[index];
                    ui::Row::value(title, value)
                        .note(note)
                        .focused(focused)
                        .draw(ui, rect);
                }
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
            self.move_focus(-1, cx);
        } else if [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a) {
            self.move_focus(1, cx);
        } else if *a == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(FIELDS.len());
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, FIELDS.len()) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.activate(index, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(chosen) = FOLDER_CHOSEN.read(event, params) {
            let folder = chosen.path.as_str();
            self.catalog.folder = folder.trim_matches('/').to_string();
            self.save(&env, cx);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
            return;
        }
        let Some(typed) = ENTERED.read(event, params) else {
            return;
        };
        let Some(field) = typed.context.get(FIELD).and_then(Field::of) else {
            return;
        };
        let text = typed.text.as_str();
        let trimmed = text.trim().to_string();
        let c = &mut self.catalog;
        match field {
            Field::Name if !trimmed.is_empty() => c.name = trimmed,
            Field::Address if !trimmed.is_empty() => c.url = trimmed,
            Field::User => c.user = trimmed,
            // Passwords may begin or end with spaces.
            Field::Password => c.password = text.to_string(),
            Field::Folder => c.folder = trimmed.trim_matches('/').to_string(),
            _ => return,
        }
        self.save(&env, cx);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }
}
