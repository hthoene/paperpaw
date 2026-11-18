//! A catalog's details: its name, address, user name, password (a secret, shown
//! as dots), the folder its books go to, whether it is a WebDAV folder and
//! whether *Sync catalogs* syncs it. Every change is kept at once.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    formatting::Arg,
    ui::{Instance, TextMode},
};
use paperpaw_ui_kit::{Item, ItemList, ItemStep, Messages, mem::push, text::copy};

use super::{Ask, DOTS, Opds, Pending, View};
use crate::native_catalogs::{self as catalogs, Catalog};
use crate::native_keep as keep;
use crate::native_model::MAX_URL;

/// A field of a catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Name,
    Address,
    WebDav,
    User,
    Password,
    Folder,
    Sync,
}

/// The fields in the order they are shown.
const FIELDS: [Field; 7] = [
    Field::Name,
    Field::Address,
    Field::WebDav,
    Field::User,
    Field::Password,
    Field::Folder,
    Field::Sync,
];

/// The longest user name and name kept, in bytes.
const MAX_FIELD: usize = 255;

impl Field {
    const fn key(self) -> &'static str {
        match self {
            Self::Name => "opds.field.name",
            Self::Address => "opds.field.address",
            Self::User => "opds.field.user",
            Self::Password => "opds.field.password",
            Self::Folder => "opds.field.folder",
            Self::Sync => "opds.field.sync",
            Self::WebDav => "opds.field.webdav",
        }
    }

    /// What its row says below its name, for a catalog at `url`.
    fn help(self, url: &str) -> &'static str {
        match self {
            Self::Password if in_clear(url) => "opds.field.password.http",
            Self::Name => "opds.field.name.help",
            Self::Address => "opds.field.address.help",
            Self::User => "opds.field.user.help",
            Self::Password => "opds.field.password.help",
            Self::Folder => "opds.field.folder.help",
            Self::Sync => "opds.field.sync.help",
            Self::WebDav => "opds.field.webdav.help",
        }
    }
}

/// Whether what is sent to `url` goes unencrypted (`http`), so anyone on the
/// way can read a password with it.
fn in_clear(url: &str) -> bool {
    url.get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
}

/// The editor of one catalog.
#[derive(Debug, Default)]
pub(super) struct EditView {
    index: usize,
    catalog: Catalog,
    list: ItemList,
}

impl EditView {
    pub(super) fn draw(&mut self, host: &Host<'_>, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let title = copy(&self.catalog.name)?;
        self.list.draw_with(
            host,
            &mut frame,
            &title,
            true,
            paperpaw_ui_kit::Footer::None,
        )
    }
}

impl Opds {
    /// The editor opened for the catalog that was asked for.
    pub(super) fn opened_edit(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let index = match self.pending.take() {
            Some(Pending::Edit(index)) => index,
            other => {
                self.pending = other;
                usize::MAX
            }
        };
        self.reload(host)?;
        let catalog = match self.catalogs.get(index) {
            Some(found) => found.try_clone()?,
            None => Catalog::default(),
        };
        let mut view = EditView {
            index,
            catalog,
            list: ItemList::default(),
        };
        self.build_edit(host, &mut view)?;
        self.screens.open(instance, View::Edit(view))
    }

    /// The rows, as the catalog is now.
    fn build_edit(&self, host: &Host<'_>, view: &mut EditView) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let c = &view.catalog;
        let kept = catalogs::password(&self.vault(host), c)?.is_some();
        let (on, off) = (say.copy(host, "core.on")?, say.copy(host, "core.off")?);
        let mut items = Vec::new();
        for field in FIELDS {
            let value = match field {
                Field::Name => copy(&c.name)?,
                Field::Address => copy(&c.url)?,
                Field::User => copy(&c.user)?,
                Field::Password if kept => copy(DOTS)?,
                Field::Password => String::new(),
                Field::Folder => c.shown_folder(&self.folder)?,
                Field::Sync => copy(if c.sync { &on } else { &off })?,
                Field::WebDav => copy(if c.webdav { &on } else { &off })?,
            };
            let title = say.copy(host, field.key())?;
            let note = say.copy(host, field.help(&c.url))?;
            push(&mut items, Item::noted(&title, &value, &note)?)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        Ok(())
    }

    /// Makes the rows of the editor on show anew and asks for a redraw.
    fn refresh_edit(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(View::Edit(top)) = self.screens.top(crate::declarations::EDIT_SCREEN) else {
            return Ok(());
        };
        let mut view = EditView {
            index: top.index,
            catalog: top.catalog.try_clone()?,
            list: ItemList::default(),
        };
        view.list.set_focus(top.list.focus());
        self.build_edit(host, &mut view)?;
        if let Some(View::Edit(top)) = self.screens.top(crate::declarations::EDIT_SCREEN) {
            *top = view;
        }
        Self::redraw(host)
    }

    pub(super) fn edit_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(View::Edit(view)) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step
            && let Some(field) = FIELDS.get(index).copied()
        {
            self.edit_field(host, field)?;
        }
        Ok(ItemList::handled(step))
    }

    fn edit_view(&mut self) -> Option<&mut EditView> {
        match self.screens.top(crate::declarations::EDIT_SCREEN) {
            Some(View::Edit(view)) => Some(view),
            _ => None,
        }
    }

    /// A field was chosen: a switch turns, the others ask for their text.
    fn edit_field(&mut self, host: &Host<'_>, field: Field) -> Result<(), Error> {
        let library = copy(&self.folder)?;
        let Some(view) = self.edit_view() else {
            return Ok(());
        };
        match field {
            Field::WebDav => view.catalog.webdav = !view.catalog.webdav,
            Field::Sync => view.catalog.sync = !view.catalog.sync,
            _ => {
                let c = &view.catalog;
                let initial = match field {
                    Field::Name => Some(copy(&c.name)?),
                    Field::Address => Some(copy(&c.url)?),
                    Field::User => Some(copy(&c.user)?),
                    Field::Folder => Some(c.shown_folder(&library)?),
                    _ => None,
                };
                let title = Messages::new()?.copy(host, field.key())?;
                let mode = matches!(field, Field::Address).then_some(TextMode::Url);
                return self.ask_text(
                    host,
                    &title,
                    initial.as_deref(),
                    mode,
                    field == Field::Password,
                    Ask::Field(field),
                );
            }
        }
        self.save_edit(host)?;
        self.refresh_edit(host)
    }

    /// Keeps the catalog as it is now in the list, unless the list cannot be
    /// read: then it is left as it is, and the change is not kept.
    fn save_edit(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(view) = self.edit_view() else {
            return Ok(());
        };
        let (index, catalog) = (view.index, view.catalog.try_clone()?);
        self.reload(host)?;
        if self.unreadable {
            return Self::notice_key(host, "opds.unreadable");
        }
        if let Some(slot) = self.catalogs.get_mut(index) {
            *slot = catalog;
            self.save_list(host)?;
        }
        Ok(())
    }

    /// What was typed for `field`.
    pub(super) fn field_typed(
        &mut self,
        host: &Host<'_>,
        field: Field,
        text: &str,
    ) -> Result<(), Error> {
        let trimmed = text.trim();
        if trimmed.len() > MAX_URL.max(MAX_FIELD) || text.len() > 1024 {
            return Self::notice_key(host, "opds.not-kept");
        }
        let Some(view) = self.edit_view() else {
            return Ok(());
        };
        match field {
            Field::Name if !trimmed.is_empty() => view.catalog.name = copy(trimmed)?,
            Field::Address if !trimmed.is_empty() => {
                let from = copy(&view.catalog.url)?;
                view.catalog.url = copy(trimmed)?;
                self.move_password(host, &from, trimmed)?;
            }
            Field::User => view.catalog.user = copy(trimmed)?,
            // Passwords may begin or end with spaces.
            Field::Password => self.set_password(host, text)?,
            Field::Folder => view.catalog.folder = copy(trimmed.trim_matches('/'))?,
            _ => return Ok(()),
        }
        self.save_edit(host)?;
        self.refresh_edit(host)
    }

    /// The label of the catalog's password among the secrets.
    fn secret_label(host: &Host<'_>, name: &str) -> Result<String, Error> {
        Messages::new()?.format(
            host,
            "opds.secret",
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        )
    }

    /// The password follows the catalog to its new address.
    fn move_password(&mut self, host: &Host<'_>, from: &str, to: &str) -> Result<(), Error> {
        let Some(view) = self.edit_view() else {
            return Ok(());
        };
        let label = Self::secret_label(host, &view.catalog.name)?;
        let moved = catalogs::move_secret(&self.vault(host), from, to, &label)?;
        if !moved {
            Self::notice_key(host, "opds.not-kept")?;
        }
        Ok(())
    }

    /// Keeps the password typed among the secrets: an empty one takes it away.
    /// Said where the device keeps none (the shared text) or it was not kept.
    fn set_password(&mut self, host: &Host<'_>, text: &str) -> Result<(), Error> {
        let Some(view) = self.edit_view() else {
            return Ok(());
        };
        let (name, label) = (
            keep::name_of(&view.catalog.url)?,
            Self::secret_label(host, &view.catalog.name)?,
        );
        view.catalog.legacy = keep::Secret::default();
        let vault = self.vault(host);
        let done = if text.is_empty() {
            catalogs::Vault::remove(&vault, &name)
        } else {
            catalogs::Vault::keep(&vault, &name, &label, text)
        };
        match done {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(Error::Unsupported) => Self::notice_key(host, "core.secrets-unsupported"),
            Err(_) => Self::notice_key(host, "opds.not-kept"),
        }
    }
}
