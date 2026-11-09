//! The card side of `text.hyphenate`: the data plugins' `[hyphenation]` files
//! read from a [`Storage`]. The shell tells it which languages the installed
//! plugins declare (a snapshot, replaced when plugins change) and when the
//! card changes; nothing here knows a shell. Table files are untrusted input,
//! checked by the service before use.
use super::hyphen_service::{HyphenFile, HyphenHost};
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND};
use paperpaw_platform::{Storage, StorageError};

/// The longest declared language tag and table path (declaration bounds).
const TAG_MAX: usize = 35;
const PATH_MAX: usize = 512;

struct Table {
    tag: String,
    path: String,
    plugin: u64,
}

/// A table file on the card, read at positions.
struct CardFile {
    storage: Rc<dyn Storage>,
    path: String,
    size: u64,
}

impl HyphenFile for CardFile {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> bool {
        let mut done = 0;
        while done < buf.len() {
            let Some(from) = at.checked_add(done as u64) else {
                return false;
            };
            match self.storage.read_at(&self.path, from, &mut buf[done..]) {
                Ok(0) | Err(_) => return false,
                Ok(n) => done += n,
            }
        }
        true
    }
}

/// The hyphenation tables of the installed data plugins, on the card.
pub(crate) struct CardTables {
    storage: Rc<dyn Storage>,
    tables: RefCell<Vec<Table>>,
    epoch: Cell<u64>,
}

impl CardTables {
    /// No tables yet.
    pub(crate) fn new(storage: Rc<dyn Storage>) -> Self {
        Self {
            storage,
            tables: RefCell::new(Vec::new()),
            epoch: Cell::new(1),
        }
    }

    /// Replaces the declared tables: `(tag, card path, plugin id)`, in any
    /// order. All or nothing: on an error the old ones stay.
    ///
    /// # Errors
    /// `ERR_INVALID` (an empty or overlong tag or path), `ERR_NO_MEMORY`.
    pub(crate) fn replace<'a>(
        &self,
        declared: impl Iterator<Item = (&'a str, &'a str, u64)>,
    ) -> Result<(), i32> {
        let mut next: Vec<Table> = Vec::new();
        for (tag, path, plugin) in declared {
            if tag.is_empty() || tag.len() > TAG_MAX || path.is_empty() || path.len() > PATH_MAX {
                return Err(ERR_INVALID);
            }
            next.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            next.push(Table {
                tag: copy(tag)?,
                path: copy(path)?,
                plugin,
            });
        }
        *self.tables.try_borrow_mut().map_err(|_| ERR_NO_MEMORY)? = next;
        self.card_changed();
        Ok(())
    }

    /// The card was inserted, removed or written: open tables are checked
    /// again.
    pub(crate) fn card_changed(&self) {
        self.epoch.set(self.epoch.get().wrapping_add(1));
    }
}

fn copy(text: &str) -> Result<String, i32> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    Ok(owned)
}

/// FNV-1a of what tells a file from another one.
fn identity(path: &str, size: u64, modified: Option<i64>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut take = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    };
    path.bytes().for_each(&mut take);
    size.to_le_bytes().into_iter().for_each(&mut take);
    modified
        .unwrap_or(i64::MIN)
        .to_le_bytes()
        .into_iter()
        .for_each(&mut take);
    hash
}

impl HyphenHost for CardTables {
    fn find(&self, tag: &str) -> Option<u64> {
        let tables = self.tables.try_borrow().ok()?;
        tables
            .iter()
            .enumerate()
            .filter(|(_, t)| t.tag.eq_ignore_ascii_case(tag))
            .max_by_key(|(_, t)| t.plugin)
            .map(|(index, _)| index as u64)
    }

    fn open(&self, key: u64) -> Result<(Box<dyn HyphenFile>, u64), i32> {
        let tables = self.tables.try_borrow().map_err(|_| ERR_IO)?;
        let table = usize::try_from(key)
            .ok()
            .and_then(|index| tables.get(index))
            .ok_or(ERR_NOT_FOUND)?;
        let size = self
            .storage
            .size(&table.path)
            .map_err(|error| match error {
                StorageError::NotFound => ERR_NOT_FOUND,
                _ => ERR_IO,
            })?;
        let id = identity(&table.path, size, self.storage.modified(&table.path));
        let file = CardFile {
            storage: Rc::clone(&self.storage),
            path: copy(&table.path)?,
            size,
        };
        let boxed: Box<dyn HyphenFile> = super::shared::try_box(file).ok_or(ERR_NO_MEMORY)?;
        Ok((boxed, id))
    }

    fn card_epoch(&self) -> u64 {
        self.epoch.get()
    }
}

#[cfg(test)]
#[path = "hyphen_card_tests.rs"]
mod tests;
