//! The count of a book's pages (view slice R2b): the start of every page of
//! one book in one layout, laid out a few at a time in the background and
//! kept in a file of 8 bytes a page (`paperpaw_sdk::reading::PageIndex`, as
//! interface 6 kept it), so that a book of thousands of pages needs no RAM
//! for it. Another layout (or another edition of the book) has a file of its
//! own; the file of the layout left is removed.

use alloc::string::String;
use core::fmt::Write as _;

use paperpaw_sdk::document::Locator;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::Host;
use paperpaw_sdk::native::device::{Device, DeviceClass};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_sdk::reading::PageIndex;

use crate::native_pages::Reading;
use paperpaw_sdk::Storage;
use paperpaw_sdk::digest::fnv1a64;

/// Pages laid out in one step (a step is one `core.work` run).
pub const PAGES_PER_STEP: usize = 8;
/// The same on a small device (the X4 lays a page out slowly).
pub const PAGES_PER_STEP_SMALL: usize = 1;
/// The wait between two steps, in milliseconds.
pub const DELAY_MS: u32 = 2;
/// The same on a small device.
pub const DELAY_SMALL_MS: u32 = 500;
/// The key of the work that counts.
pub const WORK_KEY: &str = "count";

/// One page, as a step needs to know it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Laid {
    pub start: Locator,
    pub end: Locator,
    /// The place is past the end of the book: there is no page.
    pub past_end: bool,
    /// The host refused a font read while the page was laid out: letters
    /// measured 0 wide, so the page is not to be trusted.
    pub unreliable: bool,
}

/// Steps that may be repeated for a font that could not be read (a card pulled)
/// before the count stops; the pages counted so far are used.
pub const RETRIES: u8 = 3;
/// Index files kept, this layout's and others' (as Beta 3's `INDEXES_KEPT`).
pub const KEPT: usize = 4;
/// The data file that lists the index files kept, oldest first.
pub const KEPT_FILE: &str = "pages.list";
/// The most of it that is read.
const KEPT_MAX: usize = 1024;

/// What one step did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// More pages remain.
    More,
    /// The book is counted to its end.
    Done,
    /// A page could not be laid out or the file takes no more: the pages
    /// counted so far stay in use.
    Stopped,
}

/// Pages a step lays out and the wait before the next, for a device of the
/// memory class `class` (none: the host did not tell, so small): only a small
/// device lays out slowly, as in Beta 3.
#[must_use]
pub const fn pace(class: Option<DeviceClass>) -> (usize, u32) {
    match class {
        Some(DeviceClass::Medium | DeviceClass::Large) => (PAGES_PER_STEP, DELAY_MS),
        _ => (PAGES_PER_STEP_SMALL, DELAY_SMALL_MS),
    }
}

/// The memory class `core.device` tells, for [`pace`]; none when the host does
/// not answer. The reply buffer is the op's whole `max_reply` (4,096 bytes,
/// `device.md` 6.1): the host refuses a smaller one with `NoSpace`.
pub fn device_class(host: &Host<'_>) -> Option<DeviceClass> {
    let device = Device::open(host).ok()?;
    let (mut input, mut out) = (buffer(256).ok()?, buffer(SERVICE_BUFFER).ok()?);
    let identity = device.device(host, &mut input, &mut out).ok()?;
    Some(identity.class())
}

/// What a layout is made of, as text: the settings (the reading font and the
/// hyphenation among them) and the display, in whole
/// numbers (no float formatting on the small device).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn layout_text(
    reading: &Reading,
    display: (u16, u16, u16),
    whole_pages: bool,
    filtered: u64,
) -> Result<String, Error> {
    let mut text = String::new();
    text.try_reserve(128).map_err(|_| Error::NoMemory)?;
    write!(
        text,
        "{}/{}/{}/{}/{}/{}/{}x{}x{}/{}/{}-{}/{}:{}/{}:{}/{}/{:x}/{:x}/t{}:{}{}",
        (reading.size_pt * 10.0) as u32,
        reading.line_spacing,
        reading.word_spacing,
        reading.margin,
        reading.margin_ends,
        u8::from(reading.justify),
        display.0,
        display.1,
        display.2,
        u8::from(whole_pages),
        // The room of the status lines: another room, other page breaks.
        if whole_pages { 0 } else { reading.status.0 },
        if whole_pages { 0 } else { reading.status.1 },
        // How paragraphs are set apart and kept, and the book's own styles.
        reading.paragraphs.0,
        reading.paragraphs.1,
        reading.lines_kept.0,
        reading.lines_kept.1,
        u8::from(reading.balanced),
        reading.book,
        // The font and the hyphenation: other letters, other pages.
        reading.text,
        // The letters: the space, kerning and ligatures set other widths.
        reading.letters.spacing,
        u8::from(reading.letters.kerning),
        u8::from(reading.letters.ligatures),
    )
    .map_err(|_| Error::NoMemory)?;
    // The text filters' rules (none: the name stays as it was).
    if filtered != 0 {
        write!(text, "/f{filtered:x}").map_err(|_| Error::NoMemory)?;
    }
    Ok(text)
}

/// What tells one edition of the book at a path from another for its page
/// index, beside the layout (#458, as Beta 3's key did).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edition {
    /// The conversion its places belong to.
    pub conversion: u64,
    /// The file's size: another file at the same path.
    pub size: u64,
    /// The file's time, where the card tells it.
    pub modified: Option<u64>,
    /// The packs of letters the reading font draws (a bit each, 0 where it
    /// cannot tell): another pack, other widths.
    pub packs: u64,
}

/// The file of the index of this book in this layout: the book's path, its
/// edition, the layout's key and the version of the layout itself (another
/// layout, other pages) tell them apart.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn file_name(path: &str, edition: Edition, layout: &str) -> Result<String, Error> {
    let mut hashed = String::new();
    hashed
        .try_reserve(path.len() + layout.len() + 96)
        .map_err(|_| Error::NoMemory)?;
    hashed.push_str(path);
    write!(
        hashed,
        "|{:x}|{}|{}|{:x}|v{}|",
        edition.conversion,
        edition.size,
        edition.modified.unwrap_or(u64::MAX),
        edition.packs,
        paperpaw_sdk::layout::VERSION,
    )
    .map_err(|_| Error::NoMemory)?;
    hashed.push_str(layout);
    let mut name = String::new();
    name.try_reserve(26).map_err(|_| Error::NoMemory)?;
    write!(name, "pages-{:016x}.bin", fnv1a64(&hashed)).map_err(|_| Error::NoMemory)?;
    Ok(name)
}

/// The list of kept files with `name` added as the newest, and the files
/// beyond `keep` (the oldest) in `evicted`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn kept_with(
    list: &str,
    name: &str,
    keep: usize,
    evicted: &mut alloc::vec::Vec<String>,
) -> Result<String, Error> {
    let mut names: alloc::vec::Vec<&str> = alloc::vec::Vec::new();
    // Room for every line of the list (a damaged one may be long) and the name.
    names
        .try_reserve_exact(list.lines().count() + 1)
        .map_err(|_| Error::NoMemory)?;
    for line in list.lines().filter(|l| !l.is_empty() && *l != name) {
        names.push(line);
    }
    names.push(name);
    let drop = names.len().saturating_sub(keep);
    let mut out = String::new();
    out.try_reserve_exact(list.len() + name.len() + 2)
        .map_err(|_| Error::NoMemory)?;
    evicted
        .try_reserve(names.len().saturating_sub(keep))
        .map_err(|_| Error::NoMemory)?;
    for (i, line) in names.iter().enumerate() {
        if i < drop {
            let mut gone = String::new();
            gone.try_reserve_exact(line.len())
                .map_err(|_| Error::NoMemory)?;
            gone.push_str(line);
            evicted.try_reserve(1).map_err(|_| Error::NoMemory)?;
            evicted.push(gone);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Notes `name` as the newest index file and removes the files of the layouts
/// used longest ago, keeping [`KEPT`]. A list or a file that cannot be
/// handled leaves the files alone: they take room, not data.
pub fn keep(storage: &dyn Storage, name: &str) {
    let Ok(mut buffer) = paperpaw_ui_kit::mem::buffer(KEPT_MAX) else {
        return;
    };
    let read = storage.read_at(KEPT_FILE, 0, &mut buffer).unwrap_or(0);
    let list = core::str::from_utf8(&buffer[..read]).unwrap_or("");
    let mut evicted = alloc::vec::Vec::new();
    let Ok(updated) = kept_with(list, name, KEPT, &mut evicted) else {
        return;
    };
    // Already the newest, nothing to remove: the card is not written (a book
    // opened again at the start, #403).
    if updated == list {
        return;
    }
    if storage.write(KEPT_FILE, updated.as_bytes()).is_err() {
        return;
    }
    for old in &evicted {
        storage.remove(old).ok();
    }
}

/// Entries of the index read at once where a search has come near its page
/// (a block of a card): the last steps of the search, and the pages turned
/// to next, are then answered from them, not by a read of 8 bytes each
/// (each is a call of the storage, about a millisecond on the X4, and a
/// page turn looks several positions up).
const WINDOW: u32 = 64;
/// Bytes of an entry of the index.
const ENTRY: usize = 8;

/// Entries of the index kept from the read made last (see [`WINDOW`]). The
/// index only grows at its end, so what was read stays as it is. Its bytes
/// are taken from the heap at the first read; where there is no memory for
/// them nothing is kept, and every entry is read by itself as before.
#[derive(Default)]
struct Window {
    /// The page of its first entry.
    first: u32,
    /// Its entries.
    count: u32,
    bytes: alloc::vec::Vec<u8>,
}

impl Window {
    /// Where page `page` starts, where it is kept.
    fn get(&self, page: u32) -> Option<Locator> {
        let i = page.checked_sub(self.first).filter(|i| *i < self.count)? as usize;
        let entry = self.bytes.get(i * ENTRY..(i + 1) * ENTRY)?;
        let word =
            |i: usize| u32::from_le_bytes([entry[i], entry[i + 1], entry[i + 2], entry[i + 3]]);
        Some(Locator::new(word(0), word(4)))
    }
}

impl core::fmt::Debug for Window {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Window")
            .field("first", &self.first)
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}

/// The count of one open book in one layout.
#[derive(Debug)]
pub struct Counter {
    index: PageIndex,
    name: String,
    window: core::cell::RefCell<Window>,
    /// Where the next step goes on (none: after the last page counted).
    next: Option<Locator>,
    stopped: bool,
    /// Steps in a row that met an unreadable font.
    failures: u8,
}

impl Counter {
    /// The index in the file `name`, as far as it goes.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn open(storage: &dyn Storage, name: String) -> Result<Self, Error> {
        let mut copy = String::new();
        copy.try_reserve_exact(name.len())
            .map_err(|_| Error::NoMemory)?;
        copy.push_str(&name);
        Ok(Self {
            index: PageIndex::open(storage, copy),
            name,
            window: core::cell::RefCell::default(),
            next: None,
            stopped: false,
            failures: 0,
        })
    }

    /// Whether pages remain to be counted (a stopped count has none to wait
    /// for).
    #[must_use]
    pub fn counting(&self) -> bool {
        !self.stopped && !self.index.is_complete()
    }

    /// The file's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The page that starts at `at` and the pages of the book, counted from 1,
    /// once the count is complete and `at` is the start of a page.
    #[must_use]
    pub fn page_at(&self, storage: &dyn Storage, at: (u32, u32)) -> Option<(u32, u32)> {
        if !self.index.is_complete() {
            return None;
        }
        let (page, exact) = self.search(storage, Locator::new(at.0, at.1))?;
        exact.then(|| (page + 1, self.index.pages()))
    }

    /// The page (from 0) `at` is on and whether it starts that page, once the
    /// count is complete.
    #[must_use]
    pub fn locate(&self, storage: &dyn Storage, at: (u32, u32)) -> Option<(u32, bool)> {
        if !self.index.is_complete() {
            return None;
        }
        self.search(storage, Locator::new(at.0, at.1))
    }

    /// Where page `page` starts: from the entries kept, else read by itself.
    fn entry(&self, storage: &dyn Storage, page: u32) -> Option<Locator> {
        let kept = self.window.borrow().get(page);
        kept.or_else(|| self.index.entry(storage, page))
    }

    /// Reads up to [`WINDOW`] entries from page `first` on into the window
    /// (none past the pages there are); nothing is kept where they cannot be
    /// read or there is no memory for them.
    fn keep_from(&self, storage: &dyn Storage, first: u32) {
        let mut window = self.window.borrow_mut();
        window.count = 0;
        let bytes = WINDOW as usize * ENTRY;
        if window.bytes.len() < bytes {
            if window.bytes.try_reserve_exact(bytes).is_err() {
                return;
            }
            window.bytes.resize(bytes, 0);
        }
        let want = self.index.pages().saturating_sub(first).min(WINDOW) as usize * ENTRY;
        let read = storage
            .read_at(
                &self.name,
                u64::from(first) * ENTRY as u64,
                &mut window.bytes[..want],
            )
            .unwrap_or(0);
        window.first = first;
        window.count = (read / ENTRY) as u32;
    }

    /// The page `at` is on (the last that starts at or before it) and whether
    /// it starts there: `PageIndex::locate`, with the entries near the page
    /// read at once and kept (see [`WINDOW`]).
    fn search(&self, storage: &dyn Storage, at: Locator) -> Option<(u32, bool)> {
        let pages = self.index.pages();
        let (mut low, mut high) = (0u32, pages);
        // Where the entries kept tell on which side of them the page is, the
        // search starts there: a page turned to after the one found last is
        // found among them without a read.
        {
            let window = self.window.borrow();
            let count = window.count.min(pages.saturating_sub(window.first));
            if count > 0 {
                let last = window.first + count - 1;
                if window.get(window.first).is_some_and(|start| start <= at) {
                    low = window.first + 1;
                }
                if window.get(last).is_some_and(|start| start > at) {
                    high = last;
                }
            }
        }
        while low < high {
            // Near its page: the entries left to look at, the one before them
            // (the page itself, where the search ends on `low`) and as many
            // after them (the pages turned to next) are read at once.
            if high - low < WINDOW / 2 {
                let first = low.saturating_sub(1);
                let kept = {
                    let window = self.window.borrow();
                    window.first <= first && high <= window.first + window.count
                };
                if !kept {
                    self.keep_from(storage, first);
                }
            }
            let mid = low + (high - low) / 2;
            if self.entry(storage, mid)? <= at {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        // A page starts at its first block, after the markup, so the start
        // `(section, 0)` of a chapter that begins a file is before its first
        // page: it is on that page (and starts it), not on the last page of
        // the section before, or on none before the first page of the book.
        // Pages never go over two sections.
        let Some(page) = low.checked_sub(1) else {
            return self
                .entry(storage, 0)
                .filter(|first| first.section == at.section)
                .map(|_| (0, true));
        };
        let start = self.entry(storage, page)?;
        if start.section < at.section
            && let Some(next) = page.checked_add(1)
            && self
                .entry(storage, next)
                .is_some_and(|first| first.section == at.section)
        {
            return Some((next, true));
        }
        Some((page, start == at))
    }

    /// The pages counted so far and whether they are all.
    #[must_use]
    pub fn counted(&self) -> (u32, bool) {
        (self.index.pages(), self.index.is_complete())
    }

    /// Where page `page` (from 0) starts, where it is counted.
    #[must_use]
    pub fn start(&self, storage: &dyn Storage, page: u32) -> Option<(u32, u32)> {
        if page >= self.index.pages() {
            return None;
        }
        let at = self.entry(storage, page)?;
        Some((at.section, at.offset))
    }

    /// The pages of the book, once counted.
    #[must_use]
    pub fn pages(&self) -> Option<u32> {
        self.index.is_complete().then(|| self.index.pages())
    }

    /// Whether the complete index has a page after the one starting at `at`.
    #[must_use]
    pub fn promises_more(&self, storage: &dyn Storage, at: (u32, u32)) -> bool {
        self.locate(storage, at)
            .is_some_and(|(page, _)| page.saturating_add(1) < self.index.pages())
    }

    /// The count is thrown away (a page the index promised did not come).
    pub fn forget(self, storage: &dyn Storage) {
        // A file that stays takes room, not data; the next layout names its own.
        storage.remove(&self.name).ok();
    }

    /// Lays out up to `per_step` pages with `lay` and adds them.
    pub fn step(
        &mut self,
        storage: &dyn Storage,
        per_step: usize,
        lay: &mut dyn FnMut(Locator) -> Result<Laid, Error>,
    ) -> Step {
        if self.stopped {
            return Step::Stopped;
        }
        if self.index.is_complete() {
            return Step::Done;
        }
        let Some(mut next) = self.resume(storage, lay) else {
            self.stopped = true;
            return Step::Stopped;
        };
        let mut starts = [Locator::START; PAGES_PER_STEP];
        let mut n = 0;
        let mut done = false;
        for slot in starts.iter_mut().take(per_step.min(PAGES_PER_STEP)) {
            let Ok(page) = lay(next) else {
                self.stopped = true;
                return Step::Stopped;
            };
            if page.unreliable {
                // The pages of this step are not kept; they are laid out again
                // at the next step, for a few steps; then the card is out.
                self.failures += 1;
                if self.failures >= RETRIES {
                    self.stopped = true;
                    return Step::Stopped;
                }
                return Step::More;
            }
            if page.past_end {
                done = true;
                break;
            }
            *slot = page.start;
            n += 1;
            if page.end <= page.start {
                // A page that ends where it started would never move on.
                done = true;
                break;
            }
            next = page.end;
        }
        self.failures = 0;
        if n > 0 && !self.index.append(storage, &starts[..n]) {
            self.stopped = true;
            return Step::Stopped;
        }
        self.next = Some(next);
        if done {
            self.index.finish(storage);
            if !self.index.is_complete() {
                self.stopped = true;
                return Step::Stopped;
            }
            return Step::Done;
        }
        Step::More
    }

    /// Where the count goes on: after the last page of the file.
    fn resume(
        &self,
        storage: &dyn Storage,
        lay: &mut dyn FnMut(Locator) -> Result<Laid, Error>,
    ) -> Option<Locator> {
        if let Some(next) = self.next {
            return Some(next);
        }
        match self.index.last(storage) {
            Some(last) => lay(last).ok().map(|page| page.end),
            None if self.index.pages() == 0 => Some(Locator::START),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    /// A book of `pages` pages, 10 characters each, in one section.
    fn book(pages: u32) -> impl FnMut(Locator) -> Result<Laid, Error> {
        move |at| {
            let page = at.offset / 10;
            Ok(Laid {
                start: at,
                end: Locator::new(at.section, at.offset + 10),
                past_end: page >= pages,
                unreliable: false,
            })
        }
    }

    #[test]
    fn counts_in_steps_and_finds_the_page() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-x.bin")).unwrap();
        let mut lay = book(20);
        assert!(counter.counting());
        assert_eq!(counter.step(&storage, 8, &mut lay), Step::More);
        assert_eq!(counter.step(&storage, 8, &mut lay), Step::More);
        assert_eq!(counter.page_at(&storage, (0, 0)), None);
        assert_eq!(counter.step(&storage, 8, &mut lay), Step::Done);
        assert!(!counter.counting());
        assert_eq!(counter.pages(), Some(20));
        assert_eq!(counter.page_at(&storage, (0, 0)), Some((1, 20)));
        assert_eq!(counter.page_at(&storage, (0, 190)), Some((20, 20)));
        assert_eq!(counter.page_at(&storage, (0, 195)), None);
    }

    /// A storage that counts the reads it is asked for.
    struct Counting(MemoryStorage, core::cell::Cell<usize>);

    impl Storage for Counting {
        fn read_at(
            &self,
            path: &str,
            at: u64,
            buf: &mut [u8],
        ) -> Result<usize, paperpaw_sdk::StorageError> {
            self.1.set(self.1.get() + 1);
            self.0.read_at(path, at, buf)
        }
        fn size(&self, path: &str) -> Result<u64, paperpaw_sdk::StorageError> {
            self.0.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.append(path, data)
        }
        fn list(
            &self,
            dir: &str,
            visit: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
        ) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.remove(path)
        }
    }

    /// Books of 3 sections of 3 pages; a page starts at its first block, 3
    /// characters into its section (the markup), as the layout makes them.
    fn sections(at: Locator) -> Result<Laid, Error> {
        let start = Locator::new(at.section, at.offset.max(3));
        let end = if start.offset >= 23 {
            Locator::new(at.section + 1, 0)
        } else {
            Locator::new(at.section, start.offset + 10)
        };
        Ok(Laid {
            start,
            end,
            past_end: at.section >= 3,
            unreliable: false,
        })
    }

    /// A chapter that begins a file starts at `(section, 0)`, before the
    /// first page of its section: it is on that page, which it starts, not on
    /// the last page of the section before (the chapter's pages left were one
    /// too many), and the first chapter is on the first page (found in a fork
    /// by Alexandre Cormier, alexcormier/paperpaw@958537c).
    #[test]
    fn a_chapter_that_begins_a_file_is_on_its_first_page() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-s.bin")).unwrap();
        while counter.step(&storage, 4, &mut sections) == Step::More {}
        assert_eq!(counter.pages(), Some(9));
        assert_eq!(counter.locate(&storage, (0, 0)), Some((0, true)));
        assert_eq!(counter.locate(&storage, (1, 0)), Some((3, true)));
        assert_eq!(counter.locate(&storage, (2, 0)), Some((6, true)));
        assert_eq!(counter.locate(&storage, (1, 3)), Some((3, true)));
        assert_eq!(counter.locate(&storage, (1, 5)), Some((3, false)));
        assert_eq!(counter.locate(&storage, (1, 13)), Some((4, true)));
        assert_eq!(counter.page_at(&storage, (2, 0)), Some((7, 9)));
        // Past the last page of the book: still on the last page.
        assert_eq!(counter.locate(&storage, (3, 0)), Some((8, false)));
    }

    /// A page is found with a few reads, not one for each step of the search,
    /// and the pages turned to after it with none (#305): in 4,000 pages every
    /// position looked up was 13 reads of 8 bytes.
    #[test]
    fn a_page_is_found_with_few_reads_and_the_next_ones_with_none() {
        let storage = Counting(MemoryStorage::new(), core::cell::Cell::new(0));
        let mut counter = Counter::open(&storage, String::from("pages-w.bin")).unwrap();
        while counter.step(&storage, 500, &mut book(4000)) == Step::More {}
        assert_eq!(counter.pages(), Some(4000));
        let counter = Counter::open(&storage, String::from("pages-w.bin")).unwrap();
        let reads = |run: &dyn Fn()| {
            let before = storage.1.get();
            run();
            storage.1.get() - before
        };
        let found = reads(&|| {
            assert_eq!(counter.locate(&storage, (0, 27_185)), Some((2718, false)));
        });
        assert!(found <= 8, "{found} reads");
        // Asked again (the status line, the chapter), and turned on from
        // there page after page.
        let turned = reads(&|| {
            assert_eq!(counter.locate(&storage, (0, 27_185)), Some((2718, false)));
            for page in 2719..2750u32 {
                assert_eq!(counter.locate(&storage, (0, page * 10)), Some((page, true)));
                assert_eq!(
                    counter.page_at(&storage, (0, page * 10)),
                    Some((page + 1, 4000))
                );
                assert_eq!(counter.start(&storage, page), Some((0, page * 10)));
            }
        });
        assert_eq!(turned, 0);
        // Back past where it began, and anywhere: found as before.
        for page in (0..4000u32).rev().step_by(7).chain([0, 1, 3998, 3999]) {
            assert_eq!(counter.locate(&storage, (0, page * 10)), Some((page, true)));
            assert_eq!(
                counter.locate(&storage, (0, page * 10 + 3)),
                Some((page, false))
            );
        }
        assert_eq!(counter.locate(&storage, (9, 0)), Some((3999, false)));
    }

    /// Pages counted after some were read are found too.
    #[test]
    fn pages_counted_after_a_read_are_found() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-v.bin")).unwrap();
        let mut lay = book(50);
        assert_eq!(counter.step(&storage, 20, &mut lay), Step::More);
        let (so_far, _) = counter.counted();
        assert!((1..50).contains(&so_far), "{so_far}");
        let last = so_far - 1;
        assert_eq!(counter.start(&storage, last), Some((0, last * 10)));
        assert_eq!(
            counter.search(&storage, Locator::new(0, last * 10 + 5)),
            Some((last, false))
        );
        while counter.step(&storage, 20, &mut lay) == Step::More {}
        assert_eq!(
            counter.locate(&storage, (0, last * 10 + 5)),
            Some((last, false))
        );
        assert_eq!(counter.locate(&storage, (0, 485)), Some((48, false)));
        assert_eq!(counter.locate(&storage, (0, 9999)), Some((49, false)));
    }

    #[test]
    fn goes_on_from_the_file_after_a_new_open() {
        let storage = MemoryStorage::new();
        let mut first = Counter::open(&storage, String::from("pages-y.bin")).unwrap();
        assert_eq!(first.step(&storage, 8, &mut book(12)), Step::More);
        let mut again = Counter::open(&storage, String::from("pages-y.bin")).unwrap();
        assert_eq!(again.step(&storage, 8, &mut book(12)), Step::Done);
        assert_eq!(again.pages(), Some(12));
    }

    #[test]
    fn a_page_that_cannot_be_laid_out_stops_the_count_and_keeps_the_pages() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-z.bin")).unwrap();
        let mut lay = |at: Locator| {
            if at.offset >= 30 {
                Err(Error::NoMemory)
            } else {
                book(100)(at)
            }
        };
        assert_eq!(counter.step(&storage, 8, &mut lay), Step::Stopped);
        assert!(!counter.counting());
        assert_eq!(counter.pages(), None);
    }

    #[test]
    fn an_unreadable_font_repeats_the_step_then_stops() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-u.bin")).unwrap();
        let mut bad = |at: Locator| {
            book(100)(at).map(|page| Laid {
                unreliable: true,
                ..page
            })
        };
        assert_eq!(counter.step(&storage, 8, &mut bad), Step::More);
        assert_eq!(counter.step(&storage, 8, &mut bad), Step::More);
        assert_eq!(counter.step(&storage, 8, &mut bad), Step::Stopped);
        // A readable step in between starts the count of failures again.
        let mut again = Counter::open(&storage, String::from("pages-v.bin")).unwrap();
        assert_eq!(again.step(&storage, 8, &mut bad), Step::More);
        assert_eq!(again.step(&storage, 8, &mut book(100)), Step::More);
        assert_eq!(again.step(&storage, 8, &mut bad), Step::More);
        assert_eq!(again.step(&storage, 8, &mut bad), Step::More);
        assert_eq!(again.page_at(&storage, (0, 0)), None);
    }

    #[test]
    fn the_few_files_of_other_layouts_are_kept() {
        let mut gone = alloc::vec::Vec::new();
        let one = kept_with("", "a", 2, &mut gone).unwrap();
        let two = kept_with(&one, "b", 2, &mut gone).unwrap();
        assert!(gone.is_empty());
        // Used again: moves to the newest place.
        let again = kept_with(&two, "a", 2, &mut gone).unwrap();
        assert_eq!(again, "b\na\n");
        let three = kept_with(&again, "c", 2, &mut gone).unwrap();
        assert_eq!(three, "a\nc\n");
        assert_eq!(gone, ["b"]);
    }

    #[test]
    fn keep_removes_the_oldest_file_beyond_the_few() {
        let storage = MemoryStorage::new();
        for name in ["p1", "p2", "p3", "p4", "p5"] {
            storage.write(name, b"x").unwrap();
            keep(&storage, name);
        }
        assert!(!storage.exists("p1"));
        assert!(storage.exists("p2") && storage.exists("p5"));
    }

    #[test]
    fn a_small_device_lays_out_a_page_a_step() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-s.bin")).unwrap();
        let mut lay = book(3);
        for _ in 0..3 {
            assert_eq!(
                counter.step(&storage, PAGES_PER_STEP_SMALL, &mut lay),
                Step::More
            );
        }
        assert_eq!(
            counter.step(&storage, PAGES_PER_STEP_SMALL, &mut lay),
            Step::Done
        );
        assert_eq!(counter.pages(), Some(3));
    }

    #[test]
    fn the_name_follows_the_font_and_the_hyphenation() {
        let base = Reading::new(12, 120, 100, 4, 4, true);
        let name = |reading: &Reading| {
            let layout = layout_text(reading, (480, 800, 220), false, 0).unwrap();
            file_name("/b.epub", edition(1), &layout).unwrap()
        };
        let sans = base.with_text("Source Sans 3", (true, 0, 0));
        assert_eq!(name(&sans), name(&sans));
        // Another font, another pagination.
        assert_ne!(name(&sans), name(&base.with_text("Literata", (true, 0, 0))));
        // Hyphenation on or off, and its minimums.
        assert_ne!(
            name(&sans),
            name(&base.with_text("Source Sans 3", (false, 0, 0)))
        );
        assert_ne!(
            name(&sans),
            name(&base.with_text("Source Sans 3", (true, 3, 0)))
        );
        assert_ne!(
            name(&sans),
            name(&base.with_text("Source Sans 3", (true, 0, 3)))
        );
    }

    #[test]
    fn the_name_follows_the_letters_so_old_page_counts_are_left() {
        use crate::native_typeset::Letters;
        let base = Reading::new(12, 120, 100, 4, 4, true);
        let name = |letters: Letters| {
            let layout =
                layout_text(&base.with_letters(letters), (480, 800, 220), false, 0).unwrap();
            file_name("/b.epub", edition(1), &layout).unwrap()
        };
        let default = name(Letters::DEFAULT);
        assert_eq!(default, name(Letters::DEFAULT));
        assert_ne!(default, name(Letters::new(8, true, true)));
        assert_ne!(default, name(Letters::new(0, false, true)));
        assert_ne!(default, name(Letters::new(0, true, false)));
        // A count made before the letters were laid is no count of today's pages.
        let before = layout_text(&base, (480, 800, 220), false, 0).unwrap();
        assert!(before.ends_with("/t0:11"), "{before}");
        assert!(!before.is_empty());
    }

    fn edition(conversion: u64) -> Edition {
        Edition {
            conversion,
            size: 1_000,
            modified: None,
            packs: 0,
        }
    }

    #[test]
    fn the_name_follows_book_edition_and_layout() {
        let a = file_name("/b.epub", edition(1), "12/100").unwrap();
        assert_eq!(a, file_name("/b.epub", edition(1), "12/100").unwrap());
        assert_ne!(a, file_name("/b.epub", edition(2), "12/100").unwrap());
        assert_ne!(a, file_name("/b.epub", edition(1), "14/100").unwrap());
        assert_ne!(a, file_name("/c.epub", edition(1), "12/100").unwrap());
        assert!(a.starts_with("pages-") && a.ends_with(".bin"));
    }

    #[test]
    fn another_file_at_the_path_or_another_pack_is_another_index() {
        let a = file_name("/b.epub", edition(1), "12/100").unwrap();
        // The book replaced by another at the same path: another size or time.
        let size = Edition {
            size: 1_001,
            ..edition(1)
        };
        let time = Edition {
            modified: Some(5),
            ..edition(1)
        };
        // A pack of letters came or went: other widths, other pages.
        let pack = Edition {
            packs: 7,
            ..edition(1)
        };
        for other in [size, time, pack] {
            assert_ne!(a, file_name("/b.epub", other, "12/100").unwrap());
        }
    }

    #[test]
    fn an_index_promising_pages_past_the_text_says_so() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-x.bin")).unwrap();
        let mut lay = book(5);
        while counter.step(&storage, 8, &mut lay) == Step::More {}
        assert_eq!(counter.pages(), Some(5));
        // Page 3 of 5 has pages after it, the last none; a place the index
        // does not know promises nothing.
        assert!(counter.promises_more(&storage, (0, 20)));
        assert!(!counter.promises_more(&storage, (0, 40)));
        // Counting: nothing is promised yet.
        let counting = Counter::open(&storage, String::from("pages-y.bin")).unwrap();
        assert!(!counting.promises_more(&storage, (0, 0)));
    }
}
