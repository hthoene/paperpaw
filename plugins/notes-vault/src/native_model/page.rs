//! Lists of any length, a window at a time: a source of entries is read
//! through once for the window that follows the last entry shown, keeping
//! only the window in memory. Memory is the window's, never the vault's.
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::cmp_lowercase;

use super::text::copy;

/// Entries of a window.
pub const WINDOW: usize = 100;

/// An entry of a list, as a source offers it (borrowed, so that an entry the
/// window does not keep costs nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowRef<'a> {
    /// What is shown: a note's title, a folder's name.
    pub title: &'a str,
    /// Where it is on the storage.
    pub path: &'a str,
    pub folder: bool,
    /// When it was last changed, if it is known.
    pub modified: i64,
}

/// An entry of a list that a window keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub title: String,
    pub path: String,
    pub folder: bool,
    pub modified: i64,
}

impl Row {
    /// The entry kept.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn of(row: &RowRef<'_>) -> Result<Self, Error> {
        Ok(Self {
            title: copy(row.title)?,
            path: copy(row.path)?,
            folder: row.folder,
            modified: row.modified,
        })
    }

    /// The entry as a source offers it.
    #[must_use]
    pub fn as_ref(&self) -> RowRef<'_> {
        RowRef {
            title: &self.title,
            path: &self.path,
            folder: self.folder,
            modified: self.modified,
        }
    }
}

/// How a list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Folders first, then notes, each by name.
    Name,
    /// The latest change first.
    Recent,
}

impl Order {
    /// Where `a` is in the order against `b`: folders first, then by rank,
    /// then by name without regard to case, then by path.
    #[must_use]
    pub fn compare(self, a: &RowRef<'_>, b: &RowRef<'_>) -> Ordering {
        u8::from(!a.folder)
            .cmp(&u8::from(!b.folder))
            .then_with(|| match self {
                Order::Name => Ordering::Equal,
                // The latest first (`b` against `a`: no negation to overflow).
                Order::Recent => b.modified.cmp(&a.modified),
            })
            .then_with(|| cmp_lowercase(a.title, b.title))
            .then_with(|| a.path.cmp(b.path))
    }
}

/// A window of a list: its entries, whether more follow.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Window {
    pub rows: Vec<Row>,
    pub more: bool,
}

/// What a source calls for each entry it has; an error ends the reading.
pub type Emit<'a> = dyn FnMut(RowRef<'_>) -> Result<(), Error> + 'a;

/// What offers the entries of a list: it calls the function it is given
/// once for each.
pub type Feed<'a> = dyn FnMut(&mut Emit<'_>) -> Result<(), Error> + 'a;

/// Collects the window that follows `after` (the last entry shown) from the
/// entries `source` offers (in any order).
///
/// # Errors
/// [`Error::NoMemory`], or what the source says.
pub fn window(order: Order, after: Option<&Row>, source: &mut Feed<'_>) -> Result<Window, Error> {
    // The `WINDOW + 1` first entries after `after`, in order: the last
    // only says that there are more.
    let mut kept: Vec<Row> = Vec::new();
    kept.try_reserve_exact(WINDOW + 1)
        .map_err(|_| Error::NoMemory)?;
    source(&mut |row: RowRef<'_>| {
        if after.is_some_and(|after| order.compare(&row, &after.as_ref()) != Ordering::Greater) {
            return Ok(());
        }
        if kept.len() > WINDOW
            && kept
                .last()
                .is_some_and(|last| order.compare(&row, &last.as_ref()) != Ordering::Less)
        {
            return Ok(());
        }
        let at = kept.partition_point(|k| order.compare(&k.as_ref(), &row) == Ordering::Less);
        if kept
            .get(at)
            .is_some_and(|k| order.compare(&k.as_ref(), &row) == Ordering::Equal)
        {
            return Ok(());
        }
        let made = Row::of(&row)?;
        if kept.len() > WINDOW {
            kept.pop();
        }
        // The room is reserved: the window never holds more than WINDOW + 1.
        kept.insert(at.min(kept.len()), made);
        Ok(())
    })?;
    let more = kept.len() > WINDOW;
    kept.truncate(WINDOW);
    Ok(Window { rows: kept, more })
}
