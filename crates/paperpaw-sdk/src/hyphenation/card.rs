//! The hyphenation tables of data plugins: files on the card that a
//! plugin's `[hyphenation]` section names (`docs/abi/proposals/
//! declarative-registration.md`, "The `[hyphenation]` data-plugin kind"),
//! read with `paperpaw-hyphen`'s card-backed reader.
//!
//! A table is opened when a word of its language is first hyphenated: its
//! index and exceptions are kept, its patterns stay on the card and are
//! read a few blocks at a time (about 26 reads a word for `de`; opening it
//! reads it once, 708 reads). At most [`OPEN`] are open at once, together
//! within [`BUDGET`] bytes of heap, checked before anything is taken: about
//! 20 KB for the largest shipped table (`de`), at most about 42 KiB for any
//! table the shared check lets through. A table is untrusted for as long as
//! it is read (contract D2): the reader checks it as it opens and bounds
//! every read. A read that fails closes it; it is opened once more at the
//! next word (B3: a card read may fail once and work again), and after a
//! second failure, or a table that is not sound, the language is given up
//! until the card changes (`Hyphenation::card_changed`). Nothing panics.
//!
//! The kernel's build opens tables: the opener is the kernel's functions,
//! set with the storage (`set_storage`), not code compiled into each
//! plugin. A plugin built without the `kernel` feature hyphenates with the
//! same tables, opened by the kernel's code, as the kernel does; where no
//! storage was set nothing is opened and nothing open is closed.
//!
//! Which tables stay open: to make room for another, only a table not used
//! for [`HOT`] words is closed (the least recently used first). A book in
//! two languages, a book's language and the UI's: each stays open while it
//! is in use and both fit; where they do not, the words of the one that
//! came second are left whole until the other has been idle for [`HOT`]
//! words (about a page), instead of each closing the other at every word
//! (708 reads each time for `de`): accepted behaviour (Senior #816). The
//! kernel closes them all when a book is closed, before the device sleeps
//! and before Wi-Fi, network, store and firmware work that needs the heap
//! (`Hyphenation::close_tables`), a plugin's as it
//! removes or updates the plugin, and all of them when the card was
//! mounted anew or written by a computer: a file is never read with what
//! was kept of another.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::fmt;

use paperpaw_platform::Storage;

/// Most tables open at once: a book's language and the UI's.
pub(super) const OPEN: usize = 2;
/// Bytes of heap all open tables take at most, together, while one is
/// opened too (the X4 has about 256 KiB): two of the largest shipped (`de`,
/// about 20 KB) fit, two of the largest the check lets through (about
/// 42 KiB each) do not.
pub(super) const BUDGET: usize = 48 * 1024;
/// Words a table must have been idle (others hyphenated meanwhile) before
/// it is closed to make room for another: about a page.
pub(super) const HOT: u64 = 256;
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
    /// The heap its table takes open, once an open found it (0 before):
    /// with no room for it, the next words do not read the card again.
    needed: Cell<usize>,
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
    /// Bytes of heap it holds.
    fn heap(&self) -> usize;
}

/// Why a table was not opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unopened {
    /// A read failed, or memory: counted, tried again at a later word.
    Failed,
    /// Not there or not sound: given up until the card changes.
    GiveUp,
    /// More than the room left in [`BUDGET`]: nothing counted.
    NoRoom,
}

/// How tables are opened: the kernel's functions (see the [module
/// documentation](self)), called by every build of this code.
#[derive(Clone, Copy)]
struct Opener {
    /// Opens the table at the path: asks `room` for the bytes it may take,
    /// with at most what it takes (from its header), before it takes any;
    /// `room` makes room and says how much there is, `None` for too little.
    open: fn(&Rc<dyn Storage>, &str, &mut Room<'_>) -> Opened,
}

/// A table opened, or why not.
type Opened = Result<Box<dyn Reader>, Unopened>;

/// What makes room for a table to be opened (see [`Opener`]).
type Room<'a> = dyn FnMut(usize) -> Option<usize> + 'a;

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
    /// Set with the storage, by the kernel's build.
    opener: Option<Opener>,
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
    /// open and there is room; the minimums `left` and `right` where they
    /// are not 0. A word is left whole where the table is not open.
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
        self.clock.set(self.clock.get() + 1);
        let at = match open.iter().position(|o| o.serial == serial) {
            Some(at) => at,
            None => match self.open_one(&mut open, entry) {
                Some(at) => at,
                None => return,
            },
        };
        let table = &mut open[at];
        table.stamp = self.clock.get();
        table.reader.hyphenate(left, right, word, out);
        if table.reader.is_failed() {
            open.swap_remove(at);
            entry.failed();
        }
    }

    /// Opens `entry`'s table with the kernel's opener, after closing what
    /// must be closed for room (only tables idle for [`HOT`] words); where
    /// it is in `open`. A failure is counted, or the table given up.
    fn open_one(&self, open: &mut Vec<Open>, entry: &Entry) -> Option<usize> {
        let (Some(storage), Some(opener)) = (&self.storage, self.opener) else {
            return None;
        };
        if entry.given_up() {
            return None;
        }
        let unopened = |why: Unopened| match why {
            Unopened::Failed => entry.failed(),
            Unopened::GiveUp => entry.give_up(),
            Unopened::NoRoom => {}
        };
        let used = |open: &Vec<Open>| open.iter().map(|o| o.reader.heap()).sum::<usize>();
        let clock = self.clock.get();
        let idle = clock.saturating_sub(HOT);
        // No room, and none of the open ones idle long enough to be closed
        // for it: the card is not read (that would be at every word).
        let full = open.len() >= OPEN || used(open) + entry.needed.get() > BUDGET;
        if entry.needed.get() > BUDGET || full && open.iter().all(|o| o.stamp > idle) {
            return None;
        }
        let mut room = |needed: usize| {
            entry.needed.set(needed);
            if needed > BUDGET {
                return None;
            }
            while open.len() >= OPEN || used(open) + needed > BUDGET {
                // The least recently used of those idle long enough.
                let oldest = (0..open.len())
                    .filter(|&i| open[i].stamp <= idle)
                    .min_by_key(|&i| open[i].stamp)?;
                // Its memory is free before the next is opened.
                open.swap_remove(oldest);
            }
            open.try_reserve(1).ok()?;
            Some(BUDGET - used(open))
        };
        let reader = (opener.open)(storage, &entry.path, &mut room)
            .map_err(unopened)
            .ok()?;
        open.push(Open {
            serial: entry.serial,
            stamp: 0,
            reader,
        });
        Some(open.len() - 1)
    }

    /// How many tables are open, and their heap.
    #[cfg(feature = "kernel")]
    pub(super) fn open_heap(&self) -> (usize, usize) {
        self.open.try_borrow().map_or((0, 0), |open| {
            (open.len(), open.iter().map(|o| o.reader.heap()).sum())
        })
    }

    /// Closes every table open; none is given up.
    pub(super) fn close_all(&self) {
        if let Ok(mut open) = self.open.try_borrow_mut() {
            open.clear();
        }
    }
}

/// The kernel's opener: `paperpaw-hyphen`'s card-backed reader.
#[cfg(feature = "kernel")]
mod kernel_opener {
    use super::{CACHE, CardFile, Opened, Room, Unopened};
    use alloc::boxed::Box;
    use alloc::rc::Rc;
    use alloc::string::String;
    use paperpaw_hyphen::{CheckError, OpenError, StoredPatterns};
    use paperpaw_platform::{Storage, StorageError};

    /// The file at `path`, its size taken now.
    fn file(storage: &Rc<dyn Storage>, path: &str) -> Result<CardFile, Unopened> {
        let len = match storage.size(path) {
            Ok(len) => len,
            Err(StorageError::NotFound | StorageError::InvalidPath) => {
                return Err(Unopened::GiveUp);
            }
            Err(_) => return Err(Unopened::Failed),
        };
        let mut copy = String::new();
        copy.try_reserve_exact(path.len())
            .map_err(|_| Unopened::Failed)?;
        copy.push_str(path);
        Ok(CardFile {
            storage: storage.clone(),
            path: copy,
            len,
        })
    }

    pub(super) fn open(storage: &Rc<dyn Storage>, path: &str, room: &mut Room<'_>) -> Opened {
        let mut file = file(storage, path)?;
        let needed = StoredPatterns::heap_bound(&mut file, CACHE).map_err(|e| match e {
            CheckError::Source(_) => Unopened::Failed,
            _ => Unopened::GiveUp,
        })?;
        // Less room than its header's bound: not now, nothing counted.
        let room = room(needed)
            .filter(|&room| room >= needed)
            .ok_or(Unopened::NoRoom)?;
        let len = file.len;
        match StoredPatterns::open_within(file, len, CACHE, room) {
            Ok(reader) => Ok(Box::new(reader)),
            // There is room for what its header says it takes (above): over
            // budget all the same, its header lied (or the file changed since).
            // Not sound: given up until the card or the plugin changes, with no
            // second eviction for it and no read again at a later word.
            Err(OpenError::OverBudget { .. }) => Err(Unopened::GiveUp),
            Err(OpenError::Check(CheckError::Source(_)) | OpenError::NoMemory) => {
                Err(Unopened::Failed)
            }
            // Not sound: it stays so until the file changes.
            Err(_) => Err(Unopened::GiveUp),
        }
    }
}

#[cfg(feature = "kernel")]
impl CardTables {
    pub(super) fn set_storage(&mut self, storage: Rc<dyn Storage>) {
        self.storage = Some(storage);
        self.opener = Some(Opener {
            open: kernel_opener::open,
        });
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
                needed: Cell::new(0),
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
            entry.needed.set(0);
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

    fn heap(&self) -> usize {
        self.heap_bytes()
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

#[cfg(test)]
mod tests {
    //! The opener and the budget, with made-up tables: what the kernel's
    //! opener is called for, what is closed for room and what is not.

    extern crate std;

    use super::*;
    use core::cell::Cell;

    /// A table of `heap` bytes that marks every word at 1.
    struct Fake {
        heap: usize,
    }

    impl Reader for Fake {
        fn hyphenate(&mut self, _: usize, _: usize, _: &str, out: &mut dyn FnMut(usize)) {
            out(1);
        }
        fn is_failed(&self) -> bool {
            false
        }
        fn heap(&self) -> usize {
            self.heap
        }
    }

    std::thread_local! {
        static OPENED: Cell<u32> = const { Cell::new(0) };
        /// Calls of the opener (each reads the card), opened or not.
        static TRIED: Cell<u32> = const { Cell::new(0) };
    }

    /// Tables are as large as their path says (`big` 30 KiB, else 10 KiB).
    fn size(path: &str) -> usize {
        if path.starts_with("big") {
            30 * 1024
        } else {
            10 * 1024
        }
    }

    fn open(
        _: &Rc<dyn Storage>,
        path: &str,
        room: &mut Room<'_>,
    ) -> Result<Box<dyn Reader>, Unopened> {
        TRIED.with(|t| t.set(t.get() + 1));
        if room(size(path)).is_none_or(|room| size(path) > room) {
            return Err(Unopened::NoRoom);
        }
        OPENED.with(|o| o.set(o.get() + 1));
        Ok(Box::new(Fake { heap: size(path) }))
    }

    /// Tables `paths`, by serial number 0, 1, …; with the made-up opener
    /// where `opener`.
    fn tables(paths: &[&str], opener: bool) -> CardTables {
        let mut card = CardTables {
            storage: Some(Rc::new(crate::MemoryStorage::new())),
            opener: opener.then_some(Opener { open }),
            ..CardTables::default()
        };
        for (n, path) in paths.iter().enumerate() {
            card.entries.push(Entry {
                plugin: String::from("p"),
                tag: alloc::format!("t{n}"),
                path: String::from(*path),
                serial: n as u32,
                failures: Cell::new(0),
                needed: Cell::new(0),
            });
        }
        card
    }

    fn marks(card: &CardTables, serial: u32) -> usize {
        let mut n = 0;
        card.hyphenate(serial, (0, 0), "word", &mut |_| n += 1);
        n
    }

    fn opened() -> u32 {
        OPENED.with(|o| o.replace(0))
    }

    fn tried() -> u32 {
        TRIED.with(|t| t.replace(0))
    }

    #[test]
    fn tables_are_opened_by_the_opener_set_once_each() {
        opened();
        let card = tables(&["a", "b"], true);
        for _ in 0..10 {
            assert_eq!((marks(&card, 0), marks(&card, 1)), (1, 1));
        }
        assert_eq!(opened(), 2);
        card.close_all();
        assert_eq!(marks(&card, 0), 1);
        assert_eq!(opened(), 1);
    }

    /// Without the kernel's opener (no storage set) nothing is opened, and
    /// nothing open is closed for a table that cannot be.
    #[test]
    fn without_an_opener_nothing_open_is_closed() {
        opened();
        let card = tables(&["a", "b", "c"], true);
        marks(&card, 0);
        marks(&card, 1);
        let mut without = tables(&["a", "b", "c"], false);
        without.open = core::mem::take(&mut *card.open.borrow_mut()).into();
        for _ in 0..(HOT as usize * 2) {
            assert_eq!(marks(&without, 2), 0);
        }
        assert_eq!(without.open.borrow().len(), 2);
        assert_eq!(opened(), 2);
    }

    /// Two tables in use that do not fit together: the second's words are
    /// left whole, the first is not closed at every word; once the first
    /// is idle for HOT words, it is closed for the second.
    #[test]
    fn tables_in_use_are_not_closed_for_each_other() {
        opened();
        tried();
        let card = tables(&["big-1", "big-2"], true);
        for _ in 0..20 {
            assert_eq!(marks(&card, 0), 1);
            assert_eq!(marks(&card, 1), 0);
        }
        assert_eq!(opened(), 1, "the second never opened past the budget");
        assert_eq!(tried(), 2, "the second's file read once, not at every word");
        for _ in 0..HOT {
            marks(&card, 1);
        }
        assert_eq!(marks(&card, 1), 1);
        assert_eq!(opened(), 1);
        assert_eq!(tried(), 1, "read again only once the first was idle");
        let heap: usize = card.open.borrow().iter().map(|o| o.reader.heap()).sum();
        assert!(heap <= BUDGET);
    }

    /// Patterns that break before every `b`, and exceptions of about 4 KB:
    /// a sound table that needs more heap than a header saying "no blocks,
    /// no exceptions" bounds.
    #[cfg(feature = "kernel")]
    fn sound_table() -> Vec<u8> {
        let mut exceptions = String::new();
        for first in 'a'..='z' {
            for second in 'a'..='z' {
                exceptions.push_str(&alloc::format!("q{first}{second}-x "));
            }
        }
        crate::hyphen::compile("1b", &exceptions, 1, 1)
    }

    /// A card whose first read of a table's header says it has no blocks
    /// and no exceptions: a header that understates the table.
    #[cfg(feature = "kernel")]
    struct Lying {
        files: crate::MemoryStorage,
        lied: Cell<bool>,
        /// Reads and sizes asked of the card.
        reads: Cell<u32>,
    }

    #[cfg(feature = "kernel")]
    impl Storage for Lying {
        fn read_at(
            &self,
            path: &str,
            offset: u64,
            buf: &mut [u8],
        ) -> Result<usize, paperpaw_platform::StorageError> {
            self.reads.set(self.reads.get() + 1);
            let read = self.files.read_at(path, offset, buf)?;
            if offset == 0 && read >= 20 && !self.lied.replace(true) {
                let len = self.files.size(path)? as u32;
                buf[12..16].copy_from_slice(&0u32.to_le_bytes());
                buf[16..20].copy_from_slice(&len.to_le_bytes());
            }
            Ok(read)
        }
        fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
            self.reads.set(self.reads.get() + 1);
            self.files.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.files.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.files.append(path, data)
        }
        fn list(
            &self,
            dir: &str,
            visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), paperpaw_platform::StorageError> {
            self.files.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
            self.files.remove(path)
        }
    }

    /// A header that understates its table, with all the room its bound
    /// asks for: given up (Senior #816, remark 3), never `NoRoom`, which
    /// would close more tables for it and read the card at every word.
    #[cfg(feature = "kernel")]
    #[test]
    fn a_header_that_understates_its_table_is_given_up() {
        let files = crate::MemoryStorage::new();
        files.write("t.pph", &sound_table()).expect("in memory");
        let storage: Rc<dyn Storage> = Rc::new(Lying {
            files,
            lied: Cell::new(false),
            reads: Cell::new(0),
        });
        let mut asked = 0;
        let mut room = |needed: usize| {
            asked = needed;
            Some(needed)
        };
        let opened = kernel_opener::open(&storage, "t.pph", &mut room);
        assert!(asked > 0);
        assert!(matches!(opened, Err(Unopened::GiveUp)));
        // Its header read truly, the same table opens with its bound.
        let mut room = |needed: usize| Some(needed);
        assert!(kernel_opener::open(&storage, "t.pph", &mut room).is_ok());
    }

    /// The kernel's opener with room for exactly what the header bounds:
    /// as when the other tables open leave no more.
    #[cfg(feature = "kernel")]
    fn open_at_bound(storage: &Rc<dyn Storage>, path: &str, room: &mut Room<'_>) -> Opened {
        kernel_opener::open(storage, path, &mut |needed| room(needed).map(|_| needed))
    }

    /// A table given up for its header reads nothing more at the next words,
    /// until the card changes; then it is opened again.
    #[cfg(feature = "kernel")]
    #[test]
    fn a_table_given_up_for_its_header_is_not_read_again() {
        let files = crate::MemoryStorage::new();
        files.write("t.pph", &sound_table()).expect("in memory");
        let card = Rc::new(Lying {
            files,
            lied: Cell::new(false),
            reads: Cell::new(0),
        });
        let tables = CardTables {
            storage: Some(card.clone() as Rc<dyn Storage>),
            opener: Some(Opener {
                open: open_at_bound,
            }),
            entries: alloc::vec![Entry {
                plugin: String::from("p"),
                tag: String::from("t"),
                path: String::from("t.pph"),
                serial: 0,
                failures: Cell::new(0),
                needed: Cell::new(0),
            }],
            ..CardTables::default()
        };
        let hyphenate = |word: &str| {
            let mut n = 0;
            tables.hyphenate(0, (1, 1), word, &mut |_| n += 1);
            n
        };
        assert_eq!(hyphenate("abba"), 0);
        assert!(tables.entries[0].given_up());
        let reads = card.reads.get();
        assert!(reads > 0);
        for word in ["abba", "cabbage", "ebb"] {
            assert_eq!(hyphenate(word), 0);
        }
        assert_eq!(card.reads.get(), reads, "a table given up is not read");
        // The card changed (mounted anew): its header now reads truly.
        tables.changed();
        assert!(hyphenate("abba") > 0);
        assert!(card.reads.get() > reads);
    }

    /// A sound table with less room than its bound: `NoRoom`, nothing
    /// counted; it opens once the room is there.
    #[cfg(feature = "kernel")]
    #[test]
    fn a_sound_table_without_room_is_not_a_failure() {
        let files = crate::MemoryStorage::new();
        files.write("t.pph", &sound_table()).expect("in memory");
        let storage: Rc<dyn Storage> = Rc::new(files);
        let mut short = |needed: usize| Some(needed - 1);
        let opened = kernel_opener::open(&storage, "t.pph", &mut short);
        assert!(matches!(opened, Err(Unopened::NoRoom)));
        let mut none = |_: usize| None;
        let opened = kernel_opener::open(&storage, "t.pph", &mut none);
        assert!(matches!(opened, Err(Unopened::NoRoom)));
        let mut room = |needed: usize| Some(needed);
        assert!(kernel_opener::open(&storage, "t.pph", &mut room).is_ok());
    }

    /// At most OPEN tables, however small: the idle one is closed.
    #[test]
    fn at_most_two_are_open() {
        opened();
        let card = tables(&["a", "b", "c"], true);
        marks(&card, 0);
        marks(&card, 1);
        for _ in 0..HOT {
            marks(&card, 1);
        }
        assert_eq!(marks(&card, 2), 1);
        assert_eq!(card.open.borrow().len(), OPEN);
        assert!(card.open.borrow().iter().all(|o| o.serial != 0));
        assert_eq!(opened(), 3);
    }
}
