//! The format calls under way, for the host roots that only a call for a
//! document may use (`docs/abi/document.md`, "Roots": root 3 the book, read
//! only).
//!
//! The document service enters a scope around each `core.format` call
//! ([`FormatScope::enter`]); the file service asks whether its caller is inside
//! one ([`FormatScope::current`]) and what book that call is for. The scope
//! only states facts: the book's path, how the format reads its book
//! ([`BookKind`]) and, for a `siblings` format, the opened file's name; the
//! path join and the containment check are the file service's.
//!
//! Entries live exactly as long as their guard, also when it is dropped while
//! unwinding: ending a call clears a plain flag and never needs a borrow, so a
//! stale "inside a call" can never be seen. Entering reserves the copies
//! fallibly before the format is entered; the drop never allocates. Calls in
//! calls are bounded by [`MAX_NESTING`], and an owner has one call at a time.
use alloc::string::String;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_NO_MEMORY};

/// Calls inside calls at most (the nesting budget of the instance registry).
pub(crate) const MAX_NESTING: usize = 8;

/// `format.spec.flags` bit 2: a book is a folder.
const FOLDER: u32 = 1 << 2;
/// `format.spec.flags` bit 4: root 3 is the book's folder and below.
const SIBLINGS: u32 = 1 << 4;

/// What root 3 is for a format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BookKind {
    /// The book file alone.
    File,
    /// The book is a folder: the folder and below.
    Folder,
    /// The book's folder and below; the call's `opened` names the file.
    Siblings,
}

impl BookKind {
    /// The kind of a format with these `format.spec.flags` (declarations
    /// refuse `folder` together with `siblings`; `folder` would win).
    pub(crate) const fn of_flags(flags: u32) -> Self {
        if flags & FOLDER != 0 {
            Self::Folder
        } else if flags & SIBLINGS != 0 {
            Self::Siblings
        } else {
            Self::File
        }
    }
}

/// What a format call is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FormatBook<'a> {
    /// The root-1 path of the opened book (the folder for a `Folder`).
    pub(crate) path: &'a str,
    pub(crate) kind: BookKind,
    /// `Siblings`: the opened file's name within the folder, else "".
    pub(crate) opened: &'a str,
    /// The document id the call is for.
    pub(crate) document: u32,
}

impl FormatBook<'_> {
    /// The folder root 3 is for a `Folder` or `Siblings` format, as the
    /// storage joins it: the path itself for a folder book, its parent folder
    /// for a siblings format; none for a single file.
    pub(crate) fn folder(&self) -> Option<&str> {
        match self.kind {
            BookKind::File => None,
            BookKind::Folder => Some(self.path.trim_end_matches('/')),
            BookKind::Siblings => Some(
                self.path
                    .trim_end_matches('/')
                    .rsplit_once('/')
                    .map_or("", |(parent, _)| parent),
            ),
        }
    }
}

struct Entry {
    path: String,
    opened: String,
    kind: BookKind,
    document: u32,
}

/// One place for a call under way. Whether it is in use and whose it is are
/// plain cells, so ending a call never needs a borrow and can never leave a
/// stale entry: `live` is cleared first and `current` looks at nothing else.
struct Slot {
    live: Cell<bool>,
    owner: Cell<u64>,
    entry: RefCell<Option<Entry>>,
}

impl Slot {
    const fn new() -> Self {
        Self {
            live: Cell::new(false),
            owner: Cell::new(0),
            entry: RefCell::new(None),
        }
    }
}

/// The `core.format` calls under way, one place each, [`MAX_NESTING`] at most.
///
/// A plugin is never inside two of its own format calls: a format that calls
/// into the opener's service and is called again is the same instance's
/// re-entry, `ERR_BUSY` in `format_call::call`. So an owner has at most one
/// entry, which is the call its running callback is for; `enter` refuses a
/// second one (`ERR_BUSY`) instead of guessing between them, and `current`
/// answers for exactly that call or for none.
pub(crate) struct FormatScope {
    slots: [Slot; MAX_NESTING],
}

impl Default for FormatScope {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for FormatScope {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FormatScope")
            .field("calls", &self.slots.iter().filter(|s| s.live.get()).count())
            .finish()
    }
}

/// Ends the call's scope when dropped, also while unwinding.
#[must_use = "the scope ends when the guard is dropped"]
pub(crate) struct Guard<'a> {
    slot: &'a Slot,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        // First, so that nothing can see the call again whatever follows.
        self.slot.live.set(false);
        // The copies are freed now if nobody is looking at them; else the
        // next call to use the place frees them.
        if let Ok(mut entry) = self.slot.entry.try_borrow_mut() {
            *entry = None;
        }
    }
}

fn text(source: &str) -> Result<String, i32> {
    let mut out = String::new();
    out.try_reserve_exact(source.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    out.push_str(source);
    Ok(out)
}

impl FormatScope {
    pub(crate) const fn new() -> Self {
        Self {
            slots: [const { Slot::new() }; MAX_NESTING],
        }
    }

    /// A call of the format plugin `owner` (its host owner id) for the
    /// document begins; it ends when the guard is dropped.
    ///
    /// # Errors
    /// `ERR_BUSY` when `owner` has a call under way already, `ERR_FULL` when
    /// [`MAX_NESTING`] calls are, `ERR_NO_MEMORY` when the copies cannot be had.
    pub(crate) fn enter(&self, owner: u64, book: &FormatBook<'_>) -> Result<Guard<'_>, i32> {
        if self
            .slots
            .iter()
            .any(|s| s.live.get() && s.owner.get() == owner)
        {
            return Err(ERR_BUSY);
        }
        let slot = self.slots.iter().find(|s| !s.live.get()).ok_or(ERR_FULL)?;
        let entry = Entry {
            path: text(book.path)?,
            opened: text(book.opened)?,
            kind: book.kind,
            document: book.document,
        };
        // A place that is not live is not looked at (`current` goes by `live`).
        *slot.entry.try_borrow_mut().map_err(|_| ERR_BUSY)? = Some(entry);
        slot.owner.set(owner);
        slot.live.set(true);
        Ok(Guard { slot })
    }

    /// Whether `owner` is inside a format call; if so `visit` is called once
    /// with what that call is for. Nothing is allocated. A call that cannot be
    /// looked at is none: it fails closed.
    pub(crate) fn current(&self, owner: u64, visit: &mut dyn FnMut(&FormatBook<'_>)) -> bool {
        let Some(slot) = self
            .slots
            .iter()
            .find(|s| s.live.get() && s.owner.get() == owner)
        else {
            return false;
        };
        let Ok(entry) = slot.entry.try_borrow() else {
            return false;
        };
        let Some(entry) = entry.as_ref() else {
            return false;
        };
        visit(&FormatBook {
            path: &entry.path,
            kind: entry.kind,
            opened: &entry.opened,
            document: entry.document,
        });
        true
    }
}

#[cfg(test)]
#[path = "format_scope_tests.rs"]
mod tests;
