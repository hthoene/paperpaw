//! The hyphenation tables of data plugins: files on the card that a
//! plugin's `[hyphenation]` section names (`docs/abi/proposals/
//! declarative-registration.md`, "The `[hyphenation]` data-plugin kind"),
//! read with `paperpaw-hyphen`'s card-backed reader.
//!
//! A table is opened when a word of its language is first hyphenated, and
//! at most [`OPEN`] are open at once (the least recently used is closed
//! first): about 20 KB of heap for the largest shipped table (`de`), the
//! patterns staying on the card. A table is untrusted for as long as it is
//! read (contract D2): the reader checks it as it opens and bounds every
//! read. A read that fails closes it; it is opened once more at the next
//! word (B3: a card read may fail once and work again), and after a second
//! failure, or a table that is not sound, the language is given up until
//! the card changes (`Hyphenation::card_changed`). Nothing panics.
//!
//! A file is never read with what was kept of another: the kernel closes a
//! plugin's tables as it removes or updates the plugin, and all of them
//! when the card was mounted anew or written by a computer.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::fmt;

use paperpaw_platform::Storage;

/// Most tables open at once: a book's language and the UI's.
pub(super) const OPEN: usize = 2;
/// Blocks of patterns each open table keeps (`paperpaw-hyphen`'s
/// measurements: 4 instead of 8 costs `de` 2 % more reads and saves 1 KB).
#[cfg(feature = "kernel")]
const CACHE: usize = 4;
/// Failed opens or reads after which a table is given up.
const GIVE_UP: u8 = 2;

/// A table as a data plugin names it.
struct Entry {
    plugin: String,
    /// The language tag, in lowercase.
    tag: String,
    /// The file on the storage: the plugin's folder and the path.
    path: String,
    /// Tells this entry from any other, also one added after it was taken
    /// out: what [`Hyphenator`](super::Hyphenator)s and open readers hold.
    serial: u32,
    /// Failed opens or reads since the card last changed.
    failures: Cell<u8>,
}

impl Entry {
    fn given_up(&self) -> bool {
        self.failures.get() >= GIVE_UP
    }

    fn failed(&self) {
        self.failures.set(self.failures.get().saturating_add(1));
    }

    fn give_up(&self) {
        self.failures.set(GIVE_UP);
    }
}

/// An open table.
trait Reader {
    fn hyphenate(&mut self, left: usize, right: usize, word: &str, out: &mut dyn FnMut(usize));
    fn is_failed(&self) -> bool;
}

struct Open {
    serial: u32,
    /// When it was used last.
    stamp: u64,
    reader: Box<dyn Reader>,
}

/// The tables of data plugins, and those open.
#[derive(Default)]
pub(super) struct CardTables {
    storage: Option<Rc<dyn Storage>>,
    entries: Vec<Entry>,
    next_serial: u32,
    open: RefCell<Vec<Open>>,
    clock: Cell<u64>,
}

impl fmt::Debug for CardTables {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CardTables")
            .field("tables", &self.entries.len())
            .field("open", &self.open.try_borrow().map_or(0, |o| o.len()))
            .finish_non_exhaustive()
    }
}

impl CardTables {
    /// The serial number of the table for a book's language `tag`: the tag
    /// (in any case, `_` read as `-`) exactly, then without its last
    /// subtag, again and again; never a longer tag, never another language.
    /// Of several plugins' tables for it, the greatest plugin id's (D4),
    /// leaving out those given up.
    pub(super) fn find(&self, tag: &str) -> Option<u32> {
        self.entry(tag).map(|e| e.serial)
    }

    fn entry(&self, tag: &str) -> Option<&Entry> {
        let mut tag = tag;
        loop {
            let found = self
                .entries
                .iter()
                .filter(|e| !e.given_up() && same_tag(&e.tag, tag))
                .reduce(|a, b| if wins(&b.plugin, &a.plugin) { b } else { a });
            if found.is_some() {
                return found;
            }
            tag = &tag[..tag.rfind(['-', '_'])?];
        }
    }

    /// Hyphenates `word` with table `serial`, opening it where it is not
    /// open; the minimums `left` and `right` where they are not 0.
    pub(super) fn hyphenate(
        &self,
        serial: u32,
        (left, right): (usize, usize),
        word: &str,
        out: &mut dyn FnMut(usize),
    ) {
        let Some(entry) = self.entries.iter().find(|e| e.serial == serial) else {
            return;
        };
        // Called again from `out`: the word is left whole.
        let Ok(mut open) = self.open.try_borrow_mut() else {
            return;
        };
        let at = match open.iter().position(|o| o.serial == serial) {
            Some(at) => at,
            None => {
                if entry.given_up() {
                    return;
                }
                // The least recently used is closed first: its memory is
                // free before the next is opened.
                if open.len() >= OPEN
                    && let Some(oldest) = (0..open.len()).min_by_key(|&i| open[i].stamp)
                {
                    open.swap_remove(oldest);
                }
                if open.try_reserve(1).is_err() {
                    return;
                }
                let Some(reader) = self.open_reader(entry) else {
                    return;
                };
                open.push(Open {
                    serial,
                    stamp: 0,
                    reader,
                });
                open.len() - 1
            }
        };
        self.clock.set(self.clock.get() + 1);
        let table = &mut open[at];
        table.stamp = self.clock.get();
        table.reader.hyphenate(left, right, word, out);
        if table.reader.is_failed() {
            open.swap_remove(at);
            entry.failed();
        }
    }

    /// Opens `entry`'s file; a failure is counted (or the table given up,
    /// where it is not sound or not there).
    #[cfg(feature = "kernel")]
    fn open_reader(&self, entry: &Entry) -> Option<Box<dyn Reader>> {
        use paperpaw_hyphen::{CheckError, OpenError, StoredPatterns};
        use paperpaw_platform::StorageError;

        let storage = self.storage.clone()?;
        let len = match storage.size(&entry.path) {
            Ok(len) => len,
            Err(StorageError::NotFound | StorageError::InvalidPath) => {
                entry.give_up();
                return None;
            }
            Err(_) => {
                entry.failed();
                return None;
            }
        };
        let mut path = String::new();
        if path.try_reserve_exact(entry.path.len()).is_err() {
            entry.failed();
            return None;
        }
        path.push_str(&entry.path);
        let file = CardFile { storage, path, len };
        match StoredPatterns::open(file, len, CACHE) {
            Ok(reader) => Some(Box::new(reader)),
            Err(OpenError::Check(CheckError::Source(_))) | Err(OpenError::NoMemory) => {
                entry.failed();
                None
            }
            // Not sound: it stays so until the file changes.
            Err(_) => {
                entry.give_up();
                None
            }
        }
    }

    #[cfg(not(feature = "kernel"))]
    fn open_reader(&self, _: &Entry) -> Option<Box<dyn Reader>> {
        // Only the kernel adds tables of the card.
        None
    }
}

#[cfg(feature = "kernel")]
impl CardTables {
    pub(super) fn set_storage(&mut self, storage: Rc<dyn Storage>) {
        self.storage = Some(storage);
        self.open.get_mut().clear();
    }

    pub(super) fn add<'t>(
        &mut self,
        plugin: &str,
        folder: &str,
        tables: impl ExactSizeIterator<Item = (&'t str, &'t str)>,
    ) -> Result<(), alloc::collections::TryReserveError> {
        let mut new = Vec::new();
        new.try_reserve_exact(tables.len())?;
        for (tag, path) in tables {
            let mut lower = copy(&[tag])?;
            lower.make_ascii_lowercase();
            new.push(Entry {
                plugin: copy(&[plugin])?,
                tag: lower,
                path: copy(&[folder, "/", path])?,
                serial: self.next_serial.wrapping_add(new.len() as u32),
                failures: Cell::new(0),
            });
        }
        self.entries.try_reserve(new.len())?;
        self.next_serial = self.next_serial.wrapping_add(new.len() as u32);
        self.entries.extend(new);
        Ok(())
    }

    pub(super) fn remove(&mut self, plugin: &str) {
        self.entries.retain(|e| e.plugin != plugin);
        let entries = &self.entries;
        self.open
            .get_mut()
            .retain(|o| entries.iter().any(|e| e.serial == o.serial));
    }

    pub(super) fn collisions<'a>(
        &'a self,
        plugin: &'a str,
    ) -> impl Iterator<Item = (CardTable<'a>, CardTable<'a>)> + 'a {
        self.entries
            .iter()
            .filter(move |e| e.plugin != plugin)
            .filter_map(move |other| {
                let mine = self
                    .entries
                    .iter()
                    .find(|e| e.plugin == plugin && e.tag == other.tag)?;
                let (lost, kept) = if wins(&mine.plugin, &other.plugin) {
                    (other, mine)
                } else {
                    (mine, other)
                };
                Some((CardTable { entry: lost }, CardTable { entry: kept }))
            })
    }

    pub(super) fn table(&self, tag: &str) -> Option<CardTable<'_>> {
        self.entry(tag).map(|entry| CardTable { entry })
    }

    pub(super) fn changed(&self) {
        if let Ok(mut open) = self.open.try_borrow_mut() {
            open.clear();
        }
        for entry in &self.entries {
            entry.failures.set(0);
        }
    }
}

/// A data plugin's hyphenation table, as the kernel tells of it.
#[cfg(feature = "kernel")]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[derive(Clone, Copy)]
pub struct CardTable<'a> {
    entry: &'a Entry,
}

#[cfg(feature = "kernel")]
impl<'a> CardTable<'a> {
    /// The plugin that names it.
    pub fn plugin(&self) -> &'a str {
        &self.entry.plugin
    }

    /// The language tag, in lowercase.
    pub fn tag(&self) -> &'a str {
        &self.entry.tag
    }

    /// The file on the storage.
    pub fn path(&self) -> &'a str {
        &self.entry.path
    }

    /// How warnings name it: `<plugin>:<tag>`.
    pub fn name(&self) -> String {
        alloc::format!("{}:{}", self.entry.plugin, self.entry.tag)
    }
}

#[cfg(feature = "kernel")]
impl fmt::Debug for CardTable<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CardTable")
            .field("plugin", &self.entry.plugin)
            .field("tag", &self.entry.tag)
            .field("path", &self.entry.path)
            .finish()
    }
}

/// A table's file, read through the storage.
#[cfg(feature = "kernel")]
struct CardFile {
    storage: Rc<dyn Storage>,
    path: String,
    /// The size when it was opened.
    len: u64,
}

#[cfg(feature = "kernel")]
impl paperpaw_hyphen::Source for CardFile {
    type Error = paperpaw_platform::StorageError;

    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), Self::Error> {
        let mut done = 0;
        while done < buf.len() {
            let n = self
                .storage
                .read_at(&self.path, at + done as u64, &mut buf[done..])?;
            if n == 0 {
                // Shorter than when it was opened.
                return Err(paperpaw_platform::StorageError::InvalidData);
            }
            done += n;
        }
        Ok(())
    }
}

#[cfg(feature = "kernel")]
impl Reader for paperpaw_hyphen::StoredPatterns<CardFile> {
    fn hyphenate(&mut self, left: usize, right: usize, word: &str, out: &mut dyn FnMut(usize)) {
        self.hyphenate_with_minimums(left, right, word, out);
    }

    fn is_failed(&self) -> bool {
        paperpaw_hyphen::StoredPatterns::is_failed(self)
    }
}

/// Whether a table's `tag` (lowercase) is a book's `wanted`, in any case
/// and with `_` read as `-`.
fn same_tag(tag: &str, wanted: &str) -> bool {
    tag.len() == wanted.len()
        && tag.bytes().zip(wanted.bytes()).all(|(t, w)| {
            t == if w == b'_' {
                b'-'
            } else {
                w.to_ascii_lowercase()
            }
        })
}

/// Whether plugin `a` has a language over plugin `b` (D4): the greater id,
/// compared byte-wise in lowercase.
fn wins(a: &str, b: &str) -> bool {
    a.bytes()
        .map(|c| c.to_ascii_lowercase())
        .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
        .is_gt()
}

/// `parts` joined into a string made with room for them, or the allocation
/// error.
#[cfg(feature = "kernel")]
fn copy(parts: &[&str]) -> Result<String, alloc::collections::TryReserveError> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len()).sum())?;
    parts.iter().for_each(|p| out.push_str(p));
    Ok(out)
}
