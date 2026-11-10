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
use alloc::{string::String, vec::Vec};
use core::cell::{Cell, OnceCell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_NO_MEMORY, ERR_NO_SPACE};

/// Calls inside calls at most (the nesting budget of the instance registry).
pub(crate) const MAX_NESTING: usize = 8;

/// Cache folders whose bytes are counted at once (open documents are at most
/// 8, and one holds one folder).
const MAX_COUNTED: usize = 16;

/// A cache folder's key in the count (FNV-1a 64 of its path).
fn folder_key(folder: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in folder.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

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
#[derive(Clone, Copy, Debug)]
pub(crate) struct FormatBook<'a> {
    /// The root-1 path of the opened book (the folder for a `Folder`).
    pub(crate) path: &'a str,
    pub(crate) kind: BookKind,
    /// `Siblings`: the opened file's name within the folder, else "".
    pub(crate) opened: &'a str,
    /// The document id the call is for.
    pub(crate) document: u32,
    /// The card path of the document's cache folder, root 6 (`""`: the
    /// document has none yet).
    pub(crate) cache: &'a str,
    /// The card path of the user's CSS file, root 4, in the calls that may
    /// read it (ops 2 and 3); `""` for none.
    pub(crate) css: &'a str,
    /// The most bytes the document's cache may hold (`document.md` 3, Cache
    /// quota).
    pub(crate) cache_cap: u64,
    /// Where the cache's bytes are counted: filled in for the visitor of
    /// [`FormatScope::current`], `None` as given to [`FormatScope::enter`].
    pub(crate) quota: Option<&'a FormatScope>,
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
    cache: String,
    css: String,
    cache_cap: u64,
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
    /// The places for the calls, made at the first call (a device that never
    /// opens a document through a format holds none of the 1 KiB they take).
    slots: OnceCell<alloc::boxed::Box<[Slot]>>,
    /// The bytes in each cache folder (the folder's key, bytes), least
    /// recently used first: counted here because a cache lives longer than a
    /// call and is shared by the documents of one conversion.
    used: RefCell<Vec<(u64, u64)>>,
}

impl Default for FormatScope {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for FormatScope {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FormatScope")
            .field(
                "calls",
                &self.places().iter().filter(|s| s.live.get()).count(),
            )
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
            slots: OnceCell::new(),
            used: RefCell::new(Vec::new()),
        }
    }

    /// The places made so far (none before the first call).
    fn places(&self) -> &[Slot] {
        self.slots.get().map_or(&[], |slots| &slots[..])
    }

    /// The places, made now if they are not (fallibly, once).
    fn places_made(&self) -> Result<&[Slot], i32> {
        if let Some(slots) = self.slots.get() {
            return Ok(&slots[..]);
        }
        let mut made = Vec::new();
        made.try_reserve_exact(MAX_NESTING)
            .map_err(|_| ERR_NO_MEMORY)?;
        made.extend((0..MAX_NESTING).map(|_| Slot::new()));
        // Nothing sets it between the check and here: the host is one thread.
        let _ = self.slots.set(made.into_boxed_slice());
        self.slots
            .get()
            .map(|slots| &slots[..])
            .ok_or(ERR_NO_MEMORY)
    }

    /// `add` bytes more and `credit` bytes fewer in the cache folder `folder`
    /// (whoever's document writes to it): nothing changes and the answer is
    /// `ERR_NO_SPACE` where the cache would hold more than `cap`.
    ///
    /// # Errors
    /// `ERR_NO_SPACE`, `ERR_BUSY` (the count is borrowed), `ERR_NO_MEMORY`.
    pub(crate) fn charge(&self, folder: &str, add: u64, credit: u64, cap: u64) -> Result<(), i32> {
        let mut used = self.used.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let key = folder_key(folder);
        // The count touched last is the one kept longest: a full table drops
        // the one least used, which is a closed document's.
        let count = match used.iter().position(|(k, _)| *k == key) {
            Some(at) => used.remove(at).1,
            None => 0,
        };
        let next = count.saturating_sub(credit).saturating_add(add);
        if next > cap {
            // Put back, at the end: the room is certain (it was just taken).
            used.push((key, count));
            return Err(ERR_NO_SPACE);
        }
        if used.len() >= MAX_COUNTED {
            used.remove(0);
        }
        if used.try_reserve(1).is_err() {
            return Err(ERR_NO_MEMORY);
        }
        used.push((key, next));
        Ok(())
    }

    /// The bytes counted in `folder`: 0 where nothing is.
    pub(crate) fn total(&self, folder: &str) -> u64 {
        let key = folder_key(folder);
        self.used
            .try_borrow()
            .ok()
            .and_then(|used| used.iter().find(|(k, _)| *k == key).map(|(_, n)| *n))
            .unwrap_or(0)
    }

    /// `folder` holds `bytes` (a finished cache found on the card), unless it
    /// is counted already. Best effort: a count that cannot be made is 0.
    pub(crate) fn seed(&self, folder: &str, bytes: u64) {
        if self.total(folder) == 0 {
            let _ = self.charge(folder, bytes, 0, u64::MAX);
        }
    }

    /// `folder` is made new or was removed: its count starts at 0.
    pub(crate) fn reset(&self, folder: &str) {
        if let Ok(mut used) = self.used.try_borrow_mut() {
            let key = folder_key(folder);
            used.retain(|(k, _)| *k != key);
        }
    }

    /// A call of the format plugin `owner` (its host owner id) for the
    /// document begins; it ends when the guard is dropped.
    ///
    /// # Errors
    /// `ERR_BUSY` when `owner` has a call under way already, `ERR_FULL` when
    /// [`MAX_NESTING`] calls are, `ERR_NO_MEMORY` when the copies cannot be had.
    pub(crate) fn enter(&self, owner: u64, book: &FormatBook<'_>) -> Result<Guard<'_>, i32> {
        let slots = self.places_made()?;
        if slots.iter().any(|s| s.live.get() && s.owner.get() == owner) {
            return Err(ERR_BUSY);
        }
        let slot = slots.iter().find(|s| !s.live.get()).ok_or(ERR_FULL)?;
        let entry = Entry {
            path: text(book.path)?,
            opened: text(book.opened)?,
            kind: book.kind,
            document: book.document,
            cache: text(book.cache)?,
            css: text(book.css)?,
            cache_cap: book.cache_cap,
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
            .places()
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
            cache: &entry.cache,
            css: &entry.css,
            cache_cap: entry.cache_cap,
            quota: Some(self),
        });
        true
    }
}

#[cfg(test)]
#[path = "format_scope_tests.rs"]
mod tests;
