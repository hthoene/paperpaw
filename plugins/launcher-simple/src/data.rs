//! What the launcher reads from the host and keeps: the rows of its three
//! menus and a setting's range from the registry, the books from the library,
//! the time. Everything allocates fallibly; nothing is read by a key press.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    library::{BookRef, Library, Query, Sort},
    registry::{MenuQuery, Registry, RegistryMenuKind, RegistryMenuRow, RegistrySettingKind},
    settings::SettingValue,
    ui::{Param, Ui},
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer, push},
    rows,
    text::copy,
};

/// `reg_menu_row.flags` (registry.md section 3).
const CHECKED: u32 = 1;
const ENABLED: u32 = 1 << 1;

/// The most rows of one menu the launcher reads.
pub const MOST_ROWS: usize = 24;
/// The most books the library screen lists.
pub const MOST_BOOKS: usize = 60;

/// What a row of a menu is, for what the launcher makes of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Runs an action.
    Action,
    /// A setting, changed in place.
    Setting,
    /// Another menu: runs the action of its id, which opens it.
    Submenu,
}

/// A row of a menu, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The action, setting or submenu.
    pub item: String,
    pub kind: Kind,
    /// Shown as a switch, on.
    pub on: bool,
    /// Faint: nothing to choose now.
    pub faint: bool,
    /// The label, already translated.
    pub title: String,
    /// The icon's name, empty for none.
    pub icon: String,
    /// What its plugin shows beside it, empty for none.
    pub badge: String,
}

/// A number setting's value and range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Number {
    pub value: i64,
    pub min: i64,
    pub max: i64,
    pub step: i64,
}

/// What a setting is, as far as the panel needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setting {
    Switch(bool),
    Number(Number),
    /// Anything else: the panel leaves it out.
    Other,
}

/// A book the library listed: where it is in the catalog, and how far it is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Book {
    pub key: u32,
    pub generation: u32,
    pub title: String,
    pub author: String,
    /// 0 to 100; 0 for a book never opened.
    pub progress: u8,
    pub opened: bool,
}

/// An open registry with its transport buffers.
pub struct Reader<'h, 'cb> {
    host: &'h Host<'cb>,
    registry: Registry,
    input: Vec<u8>,
    output: Vec<u8>,
}

impl<'h, 'cb> Reader<'h, 'cb> {
    /// Buffers first: a refused allocation must not leave a handle open.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn open(host: &'h Host<'cb>) -> Result<Self, Error> {
        let input = buffer(512)?;
        let output = buffer(4096)?;
        Ok(Self {
            host,
            registry: Registry::open(host)?,
            input,
            output,
        })
    }

    /// Releases the registry, whatever `result` is.
    ///
    /// # Errors
    /// The error of `result`, else of the release.
    pub fn finish<T>(self, result: Result<T, Error>) -> Result<T, Error> {
        let released = self.registry.release(self.host);
        let value = result?;
        released?;
        Ok(value)
    }

    /// The rows `menu` shows now (the user's own definition if there is one,
    /// else the declared rows), headings left out, at most [`MOST_ROWS`].
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn menu(&mut self, menu: &str) -> Result<Vec<Row>, Error> {
        let mut found: Vec<Row> = Vec::new();
        rows!(cursor => self.registry.menu(self.host, menu, MenuQuery::Current, cursor, &mut self.input, &mut self.output), |r| {
            match row(&r)? {
                Some(one) if found.len() < MOST_ROWS => push(&mut found, one),
                _ => Ok(()),
            }
        })?;
        Ok(found)
    }

    /// The setting `key`: a switch with its state, a number with its range;
    /// `None` for an id that is no setting.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals but `NotFound`.
    pub fn setting(&mut self, key: &str) -> Result<Option<Setting>, Error> {
        let found = match self
            .registry
            .setting(self.host, key, &mut self.input, &mut self.output)
        {
            Ok(found) => found,
            Err(Error::NotFound) => return Ok(None),
            Err(other) => return Err(other),
        };
        let basic = found.setting();
        let extra = found.extra();
        Ok(Some(match (basic.kind(), basic.value()) {
            (RegistrySettingKind::Bool, Some(SettingValue::Bool(on))) => Setting::Switch(on),
            (RegistrySettingKind::Int, Some(SettingValue::Int(value))) => Setting::Number(Number {
                value,
                min: i64::from(extra.min()),
                max: i64::from(extra.max()),
                step: i64::from(extra.step()).max(1),
            }),
            _ => Setting::Other,
        }))
    }
}

/// A row of the registry, owned. A heading or a kind a newer host adds is
/// left out (`None`).
fn row(row: &RegistryMenuRow<'_>) -> Result<Option<Row>, Error> {
    let kind = match row.kind() {
        RegistryMenuKind::Action => Kind::Action,
        RegistryMenuKind::Setting => Kind::Setting,
        RegistryMenuKind::Submenu => Kind::Submenu,
        _ => return Ok(None),
    };
    Ok(Some(Row {
        item: copy(row.item())?,
        kind,
        on: row.flags() & CHECKED != 0,
        faint: row.flags() & ENABLED == 0,
        title: copy(row.title())?,
        icon: copy(row.icon())?,
        badge: copy(row.badge())?,
    }))
}

/// Runs `work` with the library service open and releases it whatever `work`
/// ends in; a failure of `work` is reported before one of the release.
///
/// # Errors
/// The host's refusals, whatever `work` returns.
pub fn with_library<T>(
    host: &Host<'_>,
    work: impl FnOnce(&Library) -> Result<T, Error>,
) -> Result<T, Error> {
    let library = Library::open(host)?;
    let done = work(&library);
    let released = library.release(host);
    let done = done?;
    released?;
    Ok(done)
}

/// The first `most` books of the library, the one opened last first and the
/// ones never opened after them in the catalog's order.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn books(host: &Host<'_>, most: usize) -> Result<Vec<Book>, Error> {
    with_library(host, |library| {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut found: Vec<Book> = Vec::new();
        let mut query = Query::new().sort(Sort::RecentlyOpened);
        loop {
            let page = library.query(host, &query, &mut input, &mut out)?;
            let generation = page.end().generation();
            for entry in page.rows() {
                let entry = entry?;
                if found.len() == most {
                    return Ok(found);
                }
                push(
                    &mut found,
                    Book {
                        key: entry.key(),
                        generation,
                        title: copy(entry.title())?,
                        author: copy(entry.author())?,
                        progress: entry.progress().unwrap_or(0).min(100),
                        opened: entry.opened(),
                    },
                )?;
            }
            match page.end().next() {
                Some(cursor) => query = query.cursor(cursor),
                None => return Ok(found),
            }
        }
    })
}

/// Opens `book` in the reader the user bound to `core.reader`: a hand-over
/// with its path. A book gone from the catalog is not opened.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn open_book(host: &Host<'_>, ui: &Ui, book: &Book) -> Result<(), Error> {
    use paperpaw_sdk::native::{commands::CommandSender, ui::Open};
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let path = with_library(host, |library| {
        let reference = BookRef::catalog(book.key, book.generation)?;
        match library.book(host, reference, &mut input, &mut out) {
            Ok(found) => copy(found.path()).map(Some),
            Err(Error::State | Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    })?;
    let Some(path) = path else {
        return Ok(());
    };
    let params = [Param::new("path", &path)?];
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
}
