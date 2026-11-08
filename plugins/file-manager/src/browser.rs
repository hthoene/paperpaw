//! The folder on show and what is done to its entries: the page held, the
//! focus on the list or on the buttons below it, what is picked to be moved or
//! copied, and the changes, over the card as [`Storage`]. Host-free: the
//! screen draws it and the plugin asks the questions.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;

use crate::model::folder::{Entry, Folder};
use crate::model::path::{self, copy, is_dir, is_within, join};
use crate::model::{Error, ops};

/// The folder PaperPaw had up to 0.3: taken over as it starts, and where it
/// could not take a file over the old one stays.
const OLD_DIR: &str = ".quireink";

/// What was picked to be moved or copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picked {
    pub path: String,
    pub copy: bool,
}

/// The buttons of the bar at the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Button {
    NewFolder,
    Paste,
    Cancel,
}

/// What the line above the bar says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Note {
    /// `name` is picked to be moved or copied.
    Picked {
        copy: bool,
        name: String,
    },
    Failed(Error),
    /// PaperPaw's own folder is not changed here.
    OwnData,
    /// The name asked for does not work on the card.
    BadName,
    /// Done; the library was not told, so a book's reading place and notes
    /// stay with its old name (see `PORTING-7.md`).
    NotTold,
}

/// The folder on show.
#[derive(Debug)]
pub(crate) struct Browser {
    /// The shown folder; empty at the top of the card.
    pub(crate) dir: String,
    hidden: bool,
    /// How many entries it has.
    len: usize,
    /// The entries of the page held, the first of them number `first`: only a
    /// page is held, however large the folder.
    first: usize,
    page: Vec<Entry>,
    /// The entry the focus is on (or would be).
    cursor: usize,
    /// The button the focus is on, when it is on the bar.
    bar: Option<usize>,
    /// Entries a page of the screen shows.
    rows: usize,
    pub(crate) picked: Option<Picked>,
    pub(crate) note: Option<Note>,
}

/// A name for a file or folder that every file system takes as it is, the
/// memory card's (FAT) and Windows' too: one line, none of `/ \ : * ? " < > |`,
/// not ending in a dot (which they drop).
pub(crate) fn valid_name(name: &str) -> Option<&str> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.ends_with('.')
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'])
        || name.chars().any(char::is_control);
    (!bad).then_some(name)
}

/// Whether `path` is PaperPaw's own folder or lies in it, at any depth, with
/// every setting, note and statistic, or in the folder of its old name: not
/// deleted, renamed or moved here.
pub(crate) fn is_own_data(storage: &dyn Storage, path: &str) -> bool {
    [paperpaw_sdk::data::DIR, OLD_DIR]
        .iter()
        .any(|dir| is_within(storage, path, dir))
}

impl Browser {
    /// The folder `dir`, not read yet ([`Browser::reload`]), `rows` entries to
    /// a page.
    pub(crate) fn new(dir: String, hidden: bool, rows: usize) -> Self {
        Self {
            dir,
            hidden,
            len: 0,
            first: 0,
            page: Vec::new(),
            cursor: 0,
            bar: None,
            rows: rows.max(1),
            picked: None,
            note: None,
        }
    }

    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    pub(crate) const fn rows(&self) -> usize {
        self.rows
    }

    pub(crate) const fn first(&self) -> usize {
        self.first
    }

    pub(crate) fn page(&self) -> &[Entry] {
        &self.page
    }

    /// The entry number `index`, if its page is held.
    pub(crate) fn entry(&self, index: usize) -> Option<&Entry> {
        self.page.get(index.checked_sub(self.first)?)
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    /// The button the focus is on, if it is on the bar.
    pub(crate) const fn bar(&self) -> Option<usize> {
        self.bar
    }

    pub(crate) fn buttons(&self) -> &'static [Button] {
        if self.picked.is_some() {
            &[Button::Paste, Button::Cancel]
        } else {
            &[Button::NewFolder]
        }
    }

    pub(crate) fn set_hidden(&mut self, hidden: bool) -> bool {
        let changed = self.hidden != hidden;
        self.hidden = hidden;
        changed
    }

    fn folder<'a>(&'a self, storage: &'a dyn Storage) -> Folder<'a> {
        Folder {
            storage,
            dir: &self.dir,
            hidden: self.hidden,
        }
    }

    /// The path of entry `index`, and whether it is a folder.
    pub(crate) fn path_of(&self, index: usize) -> Result<Option<(String, bool)>, Error> {
        let Some(entry) = self.entry(index) else {
            return Ok(None);
        };
        Ok(Some((join(&self.dir, &entry.name)?, entry.is_dir)))
    }

    /// Takes `rows` as the entries of a page; the page held is read anew where
    /// they changed.
    pub(crate) fn set_rows(&mut self, storage: &dyn Storage, rows: usize) {
        let rows = rows.max(1);
        if rows != self.rows {
            self.rows = rows;
            self.reload(storage, None);
        }
    }

    /// Shows the folder as it is now, reading only the page shown: the one
    /// with entry `at` on it, focused (where it went, the one at its place),
    /// else the one the focus is on. A folder that cannot be read shows empty
    /// and says why.
    pub(crate) fn reload(&mut self, storage: &dyn Storage, at: Option<Entry>) {
        let rows = self.rows;
        let focus = self.cursor;
        let folder = self.folder(storage);
        let shown = (|| {
            let len = folder.count()?;
            if len == 0 {
                return Ok((0, 0, 0, Vec::new()));
            }
            let mut focus = focus.min(len - 1);
            if let Some(at) = &at {
                let (first, page) = folder.page_of(at, rows)?;
                if let Some(i) = page.iter().position(|e| e.name == at.name) {
                    return Ok((len, first, first + i, page));
                }
                focus = folder.rank(at)?.min(len - 1);
            }
            let page = folder.page(focus / rows, rows, len)?;
            Ok((len, focus - focus % rows, focus, page))
        })();
        match shown {
            Ok((len, first, cursor, page)) => {
                self.len = len;
                self.first = first;
                self.cursor = cursor;
                self.page = page;
            }
            Err(error) => {
                self.len = 0;
                self.first = 0;
                self.cursor = 0;
                self.page = Vec::new();
                self.note = Some(Note::Failed(error));
            }
        }
    }

    /// Reads the page the focus is on, if it is not held: the next or the one
    /// before from the one held, as the buttons go.
    pub(crate) fn show_focus(&mut self, storage: &dyn Storage) {
        let rows = self.rows;
        let first = self.cursor - self.cursor % rows;
        if self.len == 0 || first == self.first && !self.page.is_empty() {
            return;
        }
        let folder = self.folder(storage);
        let page = if first == self.first + rows {
            folder.after(self.page.last(), rows)
        } else if first + rows == self.first {
            folder.before(self.page.first(), rows)
        } else {
            folder.page(first / rows, rows, self.len)
        };
        match page {
            Ok(page) => {
                self.first = first;
                self.page = page;
            }
            Err(error) => {
                self.note = Some(Note::Failed(error));
                self.reload(storage, None);
            }
        }
    }

    /// Moves the focus one entry on (`1`) or back (`-1`), over the list and
    /// the bar below it, one round: past the last button to the first entry,
    /// and back.
    pub(crate) fn step(&mut self, storage: &dyn Storage, delta: isize) {
        let buttons = self.buttons().len();
        let (len, cursor) = (self.len, self.cursor);
        match self.bar {
            None if delta > 0 && (len == 0 || cursor + 1 >= len) => self.bar = Some(0),
            None if delta < 0 && cursor == 0 => self.bar = Some(buttons.saturating_sub(1)),
            None => {
                self.cursor = cursor
                    .saturating_add_signed(delta)
                    .min(len.saturating_sub(1));
            }
            Some(i) if delta > 0 && i + 1 < buttons => self.bar = Some(i + 1),
            Some(_) if delta > 0 => {
                self.bar = None;
                self.cursor = 0;
            }
            Some(0) => {
                self.bar = None;
                self.cursor = len.saturating_sub(1);
            }
            Some(i) => self.bar = Some(i - 1),
        }
        self.show_focus(storage);
    }

    /// Moves the focus a page of the screen on (`1`) or back (`-1`).
    /// Whether it moved.
    pub(crate) fn turn(&mut self, storage: &dyn Storage, forward: bool) -> bool {
        if self.bar.is_some() || self.len == 0 {
            return false;
        }
        let rows = self.rows;
        let to = if forward {
            self.cursor.saturating_add(rows).min(self.len - 1)
        } else {
            self.cursor.saturating_sub(rows)
        };
        let moved = to != self.cursor;
        self.cursor = to;
        self.show_focus(storage);
        moved
    }

    /// Puts the focus on entry `index` of the list.
    pub(crate) fn focus_entry(&mut self, index: usize) {
        self.cursor = index.min(self.len.saturating_sub(1));
        self.bar = None;
    }

    /// Puts the focus on button `index`.
    pub(crate) fn focus_button(&mut self, index: usize) {
        self.bar = Some(index.min(self.buttons().len().saturating_sub(1)));
    }

    /// Opens folder `dir` at entry `at` (from its first without).
    pub(crate) fn open_at(&mut self, storage: &dyn Storage, dir: String, at: Option<Entry>) {
        self.dir = dir;
        self.cursor = 0;
        self.bar = None;
        self.note = None;
        self.reload(storage, at);
    }

    /// Opens the folder at entry `index`.
    pub(crate) fn enter(&mut self, storage: &dyn Storage, index: usize) {
        match self.path_of(index) {
            Ok(Some((path, true))) => self.open_at(storage, path, None),
            Ok(_) => {}
            Err(error) => self.note = Some(Note::Failed(error)),
        }
    }

    /// Back to the folder above; `false` at the top. The folder just left
    /// stays chosen.
    pub(crate) fn up(&mut self, storage: &dyn Storage) -> bool {
        if self.dir.is_empty() {
            return false;
        }
        let made = (|| {
            let left = Entry::new(path::name(&self.dir), true, 0)?;
            Ok((copy(path::parent(&self.dir))?, left))
        })();
        match made {
            Ok((parent, left)) => self.open_at(storage, parent, Some(left)),
            Err(error) => self.note = Some(Note::Failed(error)),
        }
        true
    }

    /// The card said the host had no memory for a call: where that was kept
    /// as a general failure, the line says so.
    pub(crate) fn lacked_memory(&mut self) {
        if self.note == Some(Note::Failed(Error::Io)) {
            self.note = Some(Note::Failed(Error::NoMemory));
        }
    }

    /// Picks the entry at `path` to be moved or copied.
    pub(crate) fn pick(&mut self, storage: &dyn Storage, path: &str, copy_it: bool) {
        if !copy_it && is_own_data(storage, path) {
            self.note = Some(Note::OwnData);
            return;
        }
        let made = (|| {
            Ok((
                Picked {
                    path: copy(path)?,
                    copy: copy_it,
                },
                copy(path::name(path))?,
            ))
        })();
        match made {
            Ok((picked, name)) => {
                self.picked = Some(picked);
                self.note = Some(Note::Picked {
                    copy: copy_it,
                    name,
                });
            }
            Err(error) => self.note = Some(Note::Failed(error)),
        }
    }

    /// Lets go of what was picked.
    pub(crate) fn cancel(&mut self) {
        self.picked = None;
        self.note = None;
        self.bar = None;
    }

    /// Puts what was picked in the shown folder, under a free name.
    pub(crate) fn paste(&mut self, storage: &dyn Storage) {
        let Some(picked) = self.picked.take() else {
            return;
        };
        let result = (|| {
            let target = path::free_path(storage, &self.dir, path::name(&picked.path))?;
            let mut told = false;
            if picked.copy {
                ops::copy(storage, &picked.path, &target)?;
            } else if path::same_path(storage, path::parent(&picked.path), &self.dir) {
                // Moved where it is: nothing to do.
            } else if is_own_data(storage, &picked.path) {
                return Err(None);
            } else {
                ops::move_to(storage, &picked.path, &target)?;
                told = true;
            }
            Ok((target, told))
        })();
        self.bar = None;
        match result {
            Ok((target, told)) => {
                self.note = told.then_some(Note::NotTold);
                self.reload_at(storage, &target);
            }
            Err(None) => self.note = Some(Note::OwnData),
            Err(Some(error)) => {
                self.note = Some(Note::Failed(error));
                self.reload(storage, None);
            }
        }
    }

    /// Shows the folder again at the entry named like `path`, where it went.
    fn reload_at(&mut self, storage: &dyn Storage, path: &str) {
        let name = path::name(path);
        let at = Entry::new(name, is_dir(storage, path), 0);
        match at {
            Ok(at) => self.reload(storage, Some(at)),
            Err(error) => {
                self.note = Some(Note::Failed(error));
                self.reload(storage, None);
            }
        }
    }

    /// Deletes `path`, as the reader said yes.
    pub(crate) fn delete(&mut self, storage: &dyn Storage, path: &str) {
        if is_own_data(storage, path) {
            self.note = Some(Note::OwnData);
            return;
        }
        let at = Entry::new(path::name(path), is_dir(storage, path), 0);
        let deleted = ops::delete(storage, path);
        self.note = deleted.err().map(Note::Failed);
        // Picked to be moved or copied, and gone with it.
        if self
            .picked
            .as_ref()
            .is_some_and(|p| is_within(storage, &p.path, path))
        {
            self.picked = None;
        }
        // The focus where it was.
        self.reload(storage, at.ok());
    }

    /// Renames `path` to `name` as typed.
    pub(crate) fn rename(&mut self, storage: &dyn Storage, path: &str, name: &str) {
        let Some(name) = valid_name(name) else {
            self.note = Some(Note::BadName);
            return;
        };
        if name == path::name(path) {
            return;
        }
        if is_own_data(storage, path) {
            self.note = Some(Note::OwnData);
            return;
        }
        match ops::rename(storage, path, name) {
            Ok(target) => {
                self.note = Some(Note::NotTold);
                self.reload_at(storage, &target);
            }
            Err(error) => {
                self.note = Some(Note::Failed(error));
                self.reload_at(storage, path);
            }
        }
    }

    /// Makes a folder called `name` in the shown folder, beside what has that
    /// name (`name (2)`).
    pub(crate) fn new_folder(&mut self, storage: &dyn Storage, name: &str) {
        let Some(name) = valid_name(name) else {
            if !name.trim().is_empty() {
                self.note = Some(Note::BadName);
            }
            return;
        };
        let made = path::free_path(storage, &self.dir, name).and_then(|target| {
            storage
                .create_dir(&target)
                .map_err(crate::model::error_of)?;
            Ok(target)
        });
        match made {
            Ok(target) => {
                self.note = None;
                let at = Entry::new(path::name(&target), true, 0);
                self.reload(storage, at.ok());
            }
            Err(error) => self.note = Some(Note::Failed(error)),
        }
    }
}

#[cfg(test)]
mod tests;
