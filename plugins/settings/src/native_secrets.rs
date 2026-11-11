//! *Passwords and keys* on interface 7: every password and key plugins keep
//! (`core.secrets`, the settings editor's `uses.secrets`), listed by label
//! and never shown; a tap offers to type one anew (replacing it, if its owner
//! allows) or to remove it. What the screens are made of, host-free where it
//! can be; every allocation is fallible and what is typed is wiped.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::{
    Error, Host,
    secrets::{SecretList, SecretName, SecretWrite, Secrets},
    ui::{RequestId, Ui},
};
use paperpaw_ui_kit::{
    Item, ItemList, Messages,
    mem::{buffer, push, wipe, wipe_text},
    text::copy,
};

/// The action that opens the list: the page's row hides while nothing is kept.
pub const OPEN: &str = "settings.secrets.open";
/// Secrets read at most (a plugin keeps 32; the plugins that keep any are few).
pub const MOST: usize = 128;
/// The longest value (`core.secrets`).
pub const MAX_VALUE: usize = 1024;
/// How a kept secret shows.
pub const HIDDEN: &str = "••••••";
/// The input and the reply a secrets call needs (`secrets.md`).
const INPUT: usize = 1280;
/// The input of `list` (the query): `secrets.md` gives it 256 bytes.
const LIST_INPUT: usize = 256;
const REPLY: usize = 4096;

/// A secret kept by some plugin: never its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub plugin: String,
    pub name: String,
    /// The label as it is shown: the owner's text for it, or the label itself.
    pub title: String,
    /// The user may type it anew.
    pub changeable: bool,
}

/// What can be done with one secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Change,
    Remove,
}

impl Choice {
    /// The text of the row.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Change => "settings.secret.change",
            Self::Remove => "settings.secret.remove",
        }
    }
}

/// What can be done with `secret`: type it anew (if its owner allows it) and
/// remove it; one that is not kept (any more) can only be removed, which is
/// nothing, and goes back.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn choices(secret: Option<&Kept>) -> Result<Vec<Choice>, Error> {
    let mut found = Vec::new();
    if secret.is_some_and(|s| s.changeable) {
        push(&mut found, Choice::Change)?;
    }
    push(&mut found, Choice::Remove)?;
    Ok(found)
}

/// The hand-over `id` of a secret's screen: owner and name, `:` between
/// (an owner's id has none; the name may).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn id_of(plugin: &str, name: &str) -> Result<String, Error> {
    paperpaw_ui_kit::text::concat(&[plugin, ":", name])
}

/// The owner and the name in a hand-over `id`; `None` for one without owner.
#[must_use]
pub fn split_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(':').filter(|(plugin, _)| !plugin.is_empty())
}

/// Opens `core.secrets` for `work` and releases it after. `None` where the
/// device has no secrets or does not let the settings list them (the list is
/// then empty).
///
/// # Errors
/// What `work` says; [`Error::NoMemory`] and the like from opening.
pub fn with<T>(
    host: &Host<'_>,
    work: impl FnOnce(&Secrets) -> Result<T, Error>,
) -> Result<Option<T>, Error> {
    let secrets = match Secrets::open(host) {
        Ok(secrets) => secrets,
        Err(Error::Unsupported | Error::NotFound | Error::Denied) => return Ok(None),
        Err(error) => return Err(error),
    };
    let result = work(&secrets);
    // Released whatever `work` did; a failed release leaves nothing to undo.
    secrets.release(host).unwrap_or(());
    result.map(Some)
}

/// What `core.secrets` says that is not a failure: the settings are not
/// allowed to list, or there is nothing.
fn nothing(error: &Error) -> bool {
    matches!(error, Error::Unsupported | Error::NotFound | Error::Denied)
}

/// The label of a secret as shown: the owner's catalogue text for it, else
/// the label as plain text, else the name.
fn title(host: &Host<'_>, say: &mut Messages, label: &str, name: &str) -> Result<String, Error> {
    if label.is_empty() {
        return copy(name);
    }
    match say.copy(host, label) {
        Ok(text) => Ok(text),
        Err(
            Error::NotFound | Error::Invalid | Error::Utf8 | Error::NoSpace | Error::Unsupported,
        ) => copy(label),
        Err(error) => Err(error),
    }
}

/// Every secret of every plugin, by label, in the host's order, at most
/// [`MOST`].
///
/// # Errors
/// [`Error::NoMemory`] and what the host says besides it not being there.
pub fn read(host: &Host<'_>) -> Result<Vec<Kept>, Error> {
    let found = with(host, |secrets| {
        let mut input = buffer(LIST_INPUT)?;
        let mut reply = buffer(REPLY)?;
        let mut say = Messages::new()?;
        let mut kept: Vec<Kept> = Vec::new();
        let mut query = SecretList::all();
        loop {
            let page = match secrets.list(host, query, &mut input, &mut reply) {
                Ok(page) => page,
                Err(error) if nothing(&error) => return Ok(kept),
                Err(error) => return Err(error),
            };
            let more = page.end().next();
            for entry in page {
                if kept.len() == MOST {
                    return Ok(kept);
                }
                let entry = entry?;
                let item = Kept {
                    plugin: copy(entry.plugin())?,
                    name: copy(entry.name())?,
                    title: title(host, &mut say, entry.label(), entry.name())?,
                    changeable: entry.changeable(),
                };
                push(&mut kept, item)?;
            }
            match more {
                Some(cursor) => query = SecretList::all().from(cursor),
                None => return Ok(kept),
            }
        }
    })?;
    Ok(found.unwrap_or_default())
}

/// Whether the settings may list any secret and one is kept, listed through
/// the caller's buffers (`input` at least 256 bytes and `reply` at least 4,096:
/// the bounds of `list`), so the check holds no buffer of its own while the
/// main page is built.
///
/// # Errors
/// [`Error::NoMemory`] and what the host says besides it not being there.
pub fn any_in(host: &Host<'_>, input: &mut [u8], reply: &mut [u8]) -> Result<bool, Error> {
    let found = with(host, |secrets| {
        match secrets.list(host, SecretList::all(), input, reply) {
            Ok(page) => Ok(page.end().total() > 0),
            Err(error) if nothing(&error) => Ok(false),
            Err(error) => Err(error),
        }
    })?;
    Ok(found.unwrap_or(false))
}

/// Whether the row of the passwords and keys shows among the main page's
/// `pages`: it is there (the menu has it) and the settings may list a secret
/// that is kept. The service is asked only if the row is there, through the
/// caller's buffers (see [`any_in`]).
///
/// # Errors
/// [`Error::NoMemory`] and what the host says besides it not being there.
pub fn shown(
    host: &Host<'_>,
    pages: &[crate::native_model::PageRow],
    (input, reply): (&mut [u8], &mut [u8]),
) -> Result<bool, Error> {
    Ok(pages.iter().any(|page| page.action == OPEN) && any_in(host, input, reply)?)
}

/// Replaces the value of owner `plugin`'s secret `name` (it must be kept and
/// changeable: the host says so). The buffers that held `value` are wiped.
///
/// # Errors
/// What the host says (`Denied`, `NotFound`, `Io`, `Full`); [`Error::NoMemory`].
pub fn change(host: &Host<'_>, plugin: &str, name: &str, value: &str) -> Result<(), Error> {
    let done = with(host, |secrets| {
        let mut input = buffer(INPUT)?;
        let mut reply = buffer(REPLY)?;
        let target = SecretName::new(name)?.owner(plugin)?;
        let result = SecretWrite::new(target, value)
            .map(SecretWrite::changeable)
            .and_then(|write| secrets.keep(host, write, &mut input, &mut reply));
        wipe(&mut input);
        wipe(&mut reply);
        result
    })?;
    done.ok_or(Error::Unsupported)
}

/// Removes owner `plugin`'s secret `name`; one that is not kept is no error.
///
/// # Errors
/// What the host says; [`Error::NoMemory`].
pub fn remove(host: &Host<'_>, plugin: &str, name: &str) -> Result<(), Error> {
    let done = with(host, |secrets| {
        let mut input = buffer(INPUT)?;
        let mut reply = buffer(REPLY)?;
        let target = SecretName::new(name)?.owner(plugin)?;
        secrets.remove(host, target, &mut input, &mut reply)
    })?;
    done.ok_or(Error::Unsupported)
}

/// What the user typed for a secret. Debug never shows the value.
#[derive(PartialEq, Eq)]
pub enum Typed {
    /// Nothing but white space: the secret stays as it is.
    Nothing,
    /// More than a value can be.
    TooLong,
    /// The value, trimmed.
    Value(String),
}

impl core::fmt::Debug for Typed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Nothing => f.write_str("Nothing"),
            Self::TooLong => f.write_str("TooLong"),
            Self::Value(_) => f.write_str("Value(..)"),
        }
    }
}

/// Reads the answer of a password question with buffers that are wiped, as
/// the text read from them is when the caller drops it (see [`wipe_typed`]).
///
/// # Errors
/// What the host says; [`Error::NoMemory`].
pub fn read_typed(ui: &Ui, host: &Host<'_>, request: RequestId) -> Result<Typed, Error> {
    let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
    let result = ui
        .text_read(host, request.text(), 0, &mut input, &mut out)
        .and_then(|part| {
            let total = usize::try_from(part.total()).map_err(|_| Error::NoSpace)?;
            if total > MAX_VALUE {
                return Ok(Typed::TooLong);
            }
            if part.data().len() != total {
                return Err(Error::Invalid);
            }
            let text = part.data().trim();
            if text.is_empty() {
                Ok(Typed::Nothing)
            } else {
                copy(text).map(Typed::Value)
            }
        });
    wipe(&mut input);
    wipe(&mut out);
    result
}

/// Zeroes and frees what was typed.
pub fn wipe_typed(typed: Typed) {
    if let Typed::Value(text) = typed {
        wipe_text(text);
    }
}

/// The rows of the list of secrets: the label and what hides the value.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn list_items(kept: &[Kept]) -> Result<Vec<Item>, Error> {
    let mut items = Vec::new();
    for secret in kept {
        push(&mut items, Item::row(&secret.title, HIDDEN)?)?;
    }
    Ok(items)
}

/// The rows of a secret's page: what can be done, never what it is.
///
/// # Errors
/// What the host says; [`Error::NoMemory`].
pub fn choice_items(host: &Host<'_>, choices: &[Choice]) -> Result<Vec<Item>, Error> {
    let mut say = Messages::new()?;
    let mut items = Vec::new();
    for choice in choices {
        let text = say.copy(host, choice.key())?;
        push(&mut items, Item::row(&text, "")?)?;
    }
    Ok(items)
}

/// The list of secrets on show.
#[derive(Debug, Default)]
pub struct SecretsView {
    pub kept: Vec<Kept>,
    pub list: ItemList,
    pub stale: bool,
}

impl SecretsView {
    /// Reads the secrets and makes the rows, keeping the focus.
    ///
    /// # Errors
    /// What [`read`] and the rows say.
    pub fn build(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let kept = read(host)?;
        let items = list_items(&kept)?;
        self.kept = kept;
        self.list.replace(items);
        Ok(())
    }
}

/// What can be done with one secret, on show.
#[derive(Debug, Default)]
pub struct SecretView {
    pub plugin: String,
    pub name: String,
    pub title: String,
    pub choices: Vec<Choice>,
    pub list: ItemList,
    pub stale: bool,
}

impl SecretView {
    /// Finds the secret and makes the rows, keeping the focus.
    ///
    /// # Errors
    /// What [`read`] and the rows say.
    pub fn build(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let kept = read(host)?;
        let found = kept
            .iter()
            .find(|s| s.plugin == self.plugin && s.name == self.name);
        let title = match found {
            Some(secret) => copy(&secret.title)?,
            None => String::new(),
        };
        let choices = choices(found)?;
        let items = choice_items(host, &choices)?;
        self.title = title;
        self.choices = choices;
        self.list.replace(items);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(changeable: bool) -> Kept {
        Kept {
            plugin: "mail".into(),
            name: "password".into(),
            title: "Mail".into(),
            changeable,
        }
    }

    #[test]
    fn what_can_be_done_follows_who_may_change_it() {
        assert_eq!(
            choices(Some(&kept(true))).unwrap(),
            [Choice::Change, Choice::Remove]
        );
        assert_eq!(choices(Some(&kept(false))).unwrap(), [Choice::Remove]);
        assert_eq!(choices(None).unwrap(), [Choice::Remove]);
    }

    #[test]
    fn an_id_keeps_the_owner_and_a_name_with_colons() {
        let id = id_of("wifi", "My: net").unwrap();
        assert_eq!(id, "wifi:My: net");
        assert_eq!(split_id(&id), Some(("wifi", "My: net")));
        assert_eq!(split_id("nowhere"), None);
        assert_eq!(split_id(":name"), None);
    }

    #[test]
    fn rows_name_the_secret_and_hide_the_value() {
        let items = list_items(&[kept(true), kept(false)]).unwrap();
        assert_eq!(items.len(), 2);
        for item in &items {
            let Item::Row(row) = item else { panic!() };
            assert_eq!(row.title, "Mail");
            assert_eq!(row.value, HIDDEN);
        }
    }

    #[test]
    fn allocation_refusals_end_in_no_memory() {
        use crate::fail_alloc::sweep;
        let secret = kept(true);
        sweep(|| choices(Some(&secret)).map(|c| c.len()));
        sweep(|| id_of("wifi", "Home").map(|id| id.len()));
        sweep(|| list_items(core::slice::from_ref(&secret)).map(|items| items.len()));
    }

    #[test]
    fn debug_never_shows_what_was_typed() {
        let typed = Typed::Value("s3cr3t-Value-42".into());
        let shown = std::format!("{typed:?} {:?}", [&typed]);
        assert!(!shown.contains("s3cr3t"), "{shown}");
        assert_eq!(std::format!("{:?}", Typed::Nothing), "Nothing");
        assert_eq!(std::format!("{:?}", Typed::TooLong), "TooLong");
    }

    #[test]
    fn what_is_typed_is_wiped() {
        wipe_typed(Typed::Value("secret".into()));
        wipe_typed(Typed::Nothing);
    }
}
