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
//! Entries live exactly as long as their guard. Entering reserves the room
//! and the path fallibly before the format is entered, so neither the call
//! nor the guard's drop allocates. Nested calls are bounded by [`MAX_NESTING`].
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
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
    owner: u64,
    path: String,
    opened: String,
    kind: BookKind,
    document: u32,
}

/// The `core.format` calls under way, innermost last.
#[derive(Default)]
pub(crate) struct FormatScope {
    calls: RefCell<Vec<Entry>>,
}

impl core::fmt::Debug for FormatScope {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FormatScope")
            .field(
                "calls",
                &self.calls.try_borrow().map_or(0, |calls| calls.len()),
            )
            .finish()
    }
}

/// Ends the call's scope when dropped.
#[must_use = "the scope ends when the guard is dropped"]
pub(crate) struct Guard<'a> {
    scope: &'a FormatScope,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        // Never borrowed across a call, so it is free here; if not, there is
        // nothing safe to do but leave the entry (its owner's retirement
        // clears nothing), which `enter`'s bound still limits.
        if let Ok(mut calls) = self.scope.calls.try_borrow_mut() {
            calls.pop();
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
            calls: RefCell::new(Vec::new()),
        }
    }

    /// A call of the format plugin `owner` (its host owner id) for the
    /// document begins; it ends when the guard is dropped.
    ///
    /// # Errors
    /// `ERR_FULL` when [`MAX_NESTING`] calls are under way, `ERR_NO_MEMORY`
    /// when the room or the copies cannot be had, `ERR_BUSY` when the scope is
    /// in use.
    pub(crate) fn enter(&self, owner: u64, book: &FormatBook<'_>) -> Result<Guard<'_>, i32> {
        let entry = Entry {
            owner,
            path: text(book.path)?,
            opened: text(book.opened)?,
            kind: book.kind,
            document: book.document,
        };
        let mut calls = self.calls.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if calls.len() >= MAX_NESTING {
            return Err(ERR_FULL);
        }
        calls.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        calls.push(entry);
        Ok(Guard { scope: self })
    }

    /// Whether `owner` is inside a format call (its innermost one wins); if
    /// so `visit` is called once with what the call is for. Nothing is
    /// allocated, and `visit` must not enter a scope.
    pub(crate) fn current(&self, owner: u64, visit: &mut dyn FnMut(&FormatBook<'_>)) -> bool {
        let Ok(calls) = self.calls.try_borrow() else {
            return false;
        };
        let Some(entry) = calls.iter().rev().find(|entry| entry.owner == owner) else {
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
