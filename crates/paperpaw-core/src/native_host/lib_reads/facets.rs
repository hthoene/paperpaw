//! Op 5 `facets` over the catalog (`docs/abi/library.md` 2.1): the authors,
//! series, tags, first letters or folders of the books a `lib_query` finds,
//! each with how many of those books it has, as `lib_facet` rows from its
//! cursor.
//!
//! The books are op 2's ([`Engine`]: the same filters, the archive left out
//! and the books a pending delete holds). A book is under its author (whole,
//! as the catalog has it; the books without one under the empty name), its
//! series (none without one), each of its tags, the first letter of its title
//! (`library::index::letter`) or the folder it lies in right inside the
//! query's folder (interface 6's `shelves::folder_in`). The names go by their
//! lowercase cut at 96 bytes, then as they are; a descending query reverses
//! them. `first` is a place among the same books in the order of that list
//! (op 2's sort by author, series or title): for a letter the place of its
//! first book; for an author or a series the place where the books begin
//! whose sort key is the name's (names that differ only in their case, or
//! only after the cut of the sort key, begin together: their books lie
//! mixed there).
//!
//! Memory: nothing grows with the catalog. A pass reads the catalog a line at
//! a time and keeps the [`WINDOW`] first names after a bound (as op 2 keeps
//! books), each one buffer (its lowercase, a NUL and the name, at most 353
//! bytes; some 40 for a usual name) and, for a letter, the key of its first
//! book (op 2's key: at most 97 bytes and the path; 27 letters at most). The
//! number of
//! names (`end.total`) is counted with at most [`HASHES`] 64-bit hashes at
//! once (4 KiB, grown as names are found): a catalog with more names is
//! counted in more passes, each over a part of the hashes, so the number
//! holds whatever the catalog holds (two names of one hash count as one).
//! Scans of a page: `cursor / 32 + 1`, one more where there is a jump, and
//! about `names / 256` more where the names are more than [`HASHES`]. Heap
//! of a call at its most, measured
//! (`lib_reads_oom_tests`): see there; it does not grow with the catalog.
//!
//! A page that goes on where the call before ended (the same query, the
//! same generation, its `next` as cursor) starts from the place kept
//! ([`Kept`]: the last name served and the count), so it costs the scan of
//! the page and the one of the jumps, not the steps to its cursor. One
//! place is kept, the last call's (a name key, 353 bytes at most, in the
//! buffer it was made in, 706 at most): a cache
//! as library.md 2.1's checkpoints are, though not its table yet. It changes
//! no answer: it is left unused, and none is kept, while a pending delete
//! hides a book.
use super::query::{Engine, KEY_TEXT, Key, TRIES, WINDOW, copy, head, push, push_lower};
use super::{CatalogReads, fail};
use crate::abi7::context::RowsEnd;
use crate::kit::library::{catalog, index};
use crate::native_host::lib_service::Rows;
use crate::native_host::registrations::{self as wire, Record};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;
use core::cmp::Ordering;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::digest::{Fnv1a64, fnv1a64};

/// Bytes of a `lib_facet.text`.
const TEXT: usize = 256;
/// `lib_facet.first` where there is no jump (tags, folders).
const NO_JUMP: u32 = u32::MAX;
/// Hashes of names held at once while they are counted.
const HASHES: usize = 512;

/// The list asked (`lib_query.facet`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Facet {
    Authors,
    Series,
    Tags,
    Letters,
    Folders,
}

impl Facet {
    fn of(code: u32) -> Self {
        match code {
            1 => Self::Series,
            2 => Self::Tags,
            3 => Self::Letters,
            4 => Self::Folders,
            _ => Self::Authors,
        }
    }

    /// The sort of the wire `first` counts in (by title where there is no
    /// jump: none is counted).
    fn sort(self) -> u32 {
        match self {
            Self::Authors => 1,
            Self::Series => 2,
            Self::Letters | Self::Tags | Self::Folders => 0,
        }
    }

    fn jump(self) -> Jump {
        match self {
            Self::Authors => Jump::Block(KEY_TEXT),
            // Op 2's key of a series keeps room for the number.
            Self::Series => Jump::Block(KEY_TEXT - 11),
            Self::Letters => Jump::Book,
            Self::Tags | Self::Folders => Jump::None,
        }
    }
}

/// What `first` of a list is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Jump {
    /// No jump: a book can be under several names.
    None,
    /// Where the books begin whose sort key begins with the name's lowercase
    /// cut at so many bytes (the books of a name lie together in op 2's
    /// order).
    Block(usize),
    /// The place of the name's first book (the books of a letter do not lie
    /// together: `É` is under `E`, and after `z` by title).
    Book,
}

/// The folder of the book at `path` right in the folder of `prefix` (its
/// path with a `/`, empty for the card's top), its whole path; none for a
/// book right in it or outside.
fn folder_in<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let inner = path.strip_prefix(prefix)?;
    path.get(..prefix.len() + at(inner, b'/')?)
}

/// Where the first `byte` (an ASCII one) of `text` is.
fn at(text: &str, byte: u8) -> Option<usize> {
    text.bytes().position(|b| b == byte)
}

/// Where the name `text` goes among the names, made in `out`: its lowercase
/// cut at [`KEY_TEXT`] bytes, a NUL, and the name as a row shows it (cut at
/// [`TEXT`] bytes).
fn name_key(out: &mut String, text: &str) -> Result<(), i32> {
    out.clear();
    push_lower(out, text, KEY_TEXT)?;
    out.truncate(head(out, KEY_TEXT).len());
    push(out, "\0")?;
    push(out, head(text, TEXT))
}

/// A name that was found: where it goes (and, after the NUL, its text), how
/// many books have it, the first of them in the list's order and, once
/// [`Lists::firsts`] counted it, its `first`.
#[derive(Default)]
struct Name {
    key: String,
    count: u32,
    first: u32,
    first_book: Key,
    has_book: bool,
}

impl Name {
    /// Its lowercase, as it is ordered by, and its text.
    fn parts(&self) -> (&str, &str) {
        let cut = at(&self.key, 0).unwrap_or(0);
        (
            self.key.get(..cut).unwrap_or(""),
            self.key.get(cut + 1..).unwrap_or(""),
        )
    }
}

/// The first `want` names after a bound, in order.
struct Window {
    names: Vec<Name>,
    want: usize,
    descending: bool,
}

impl Window {
    fn new(want: usize, descending: bool) -> Result<Self, i32> {
        let mut names = Vec::new();
        names.try_reserve_exact(want).map_err(|_| ERR_NO_MEMORY)?;
        Ok(Self {
            names,
            want,
            descending,
        })
    }

    fn order<T: Ord + ?Sized>(&self, a: &T, b: &T) -> Ordering {
        if self.descending { b.cmp(a) } else { a.cmp(b) }
    }

    /// A book has the name whose key is in `scratch`: counted where the name
    /// is kept, and kept where it is after `after` and before the last name
    /// kept (or there is room). Returns its slot. A name that is not kept
    /// costs no allocation: `scratch` takes the buffer of the name pushed out.
    fn offer(&mut self, scratch: &mut String, after: Option<&str>) -> Option<usize> {
        if self.want == 0
            || after.is_some_and(|a| self.order(scratch.as_str(), a) != Ordering::Greater)
        {
            return None;
        }
        let found = self
            .names
            .binary_search_by(|name| self.order(name.key.as_str(), scratch.as_str()));
        let at = match found {
            Ok(at) => {
                let name = &mut self.names[at];
                name.count = name.count.saturating_add(1);
                return Some(at);
            }
            Err(at) => at,
        };
        let full = self.names.len() >= self.want;
        if full && at == self.names.len() {
            return None;
        }
        let mut name = if full {
            self.names.pop().unwrap_or_default()
        } else {
            Name::default()
        };
        core::mem::swap(&mut name.key, scratch);
        (name.count, name.first, name.has_book) = (1, 0, false);
        // The room is there: the window was reserved, and one name is out.
        self.names.insert(at, name);
        Some(at)
    }

    /// The book whose key is in `book` has the name in slot `at`: kept where
    /// it comes before the name's first book so far (`book` takes the other
    /// key's buffers).
    fn book(&mut self, at: usize, book: &mut Key) {
        let earlier = self.names.get(at).is_some_and(|name| {
            !name.has_book || self.order(&*book, &name.first_book) == Ordering::Less
        });
        if earlier && let Some(name) = self.names.get_mut(at) {
            core::mem::swap(&mut name.first_book, book);
            name.has_book = true;
        }
    }
}

/// The names counted: the hashes of those whose hash is from `low` to `high`,
/// sorted, [`HASHES`] at most; `over` once there were more.
struct Distinct {
    hashes: Vec<u64>,
    low: u64,
    high: u64,
    over: bool,
}

impl Distinct {
    fn new() -> Self {
        Self {
            hashes: Vec::new(),
            low: 0,
            high: u64::MAX,
            over: false,
        }
    }

    fn note(&mut self, hash: u64) -> Result<(), i32> {
        if self.over || hash < self.low || hash > self.high {
            return Ok(());
        }
        if let Err(at) = self.hashes.binary_search(&hash) {
            if self.hashes.len() >= HASHES {
                self.over = true;
            } else {
                self.hashes.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                self.hashes.insert(at, hash);
            }
        }
        Ok(())
    }

    fn len(&self) -> u32 {
        u32::try_from(self.hashes.len()).unwrap_or(u32::MAX)
    }
}

/// Where a page ended, for the call that goes on from there: the names
/// before `cursor` are those up to `after` (a name's key), and there are
/// `total` in all, for the query of `stamp` under `generation`.
pub(super) struct Kept {
    stamp: u64,
    generation: u32,
    cursor: u32,
    total: u32,
    after: String,
}

/// The mark of what a query lists: its list and direction, its filters, the
/// archive, the folder the folders are right in, and the day (the new books
/// are by it). Not its cursor, nor its sort (a list has its own).
fn stamp(reads: &CatalogReads, q: &wire::ServiceLibQuery<'_>, archive: &str, prefix: &str) -> u64 {
    let mut hash = Fnv1a64::new();
    let day = reads.env.today().map_or(0, |day| day.wrapping_add(1));
    for number in [q.facet, q.flags, q.show, q.added_since, day] {
        hash.feed(&number.to_le_bytes());
    }
    for text in [
        q.words, q.author, q.series, q.tag, q.folder, archive, prefix,
    ] {
        hash.feed(text.as_bytes());
        // No text holds this byte: where one ends is part of the mark.
        hash.feed(&[0xff]);
    }
    hash.value()
}

/// What one scan of the catalog does with the names it meets.
struct Pass<'p> {
    window: &'p mut Window,
    /// Only names after this one are kept.
    after: Option<&'p str>,
    counted: Option<&'p mut Distinct>,
    /// The first book of each name kept is looked for.
    books: bool,
}

/// The call made ready: the books of the query and which list of them.
struct Lists<'a> {
    engine: Engine<'a>,
    facet: Facet,
    /// The folder the folders are right in, with its `/` (empty: the top).
    prefix: String,
    descending: bool,
}

impl Lists<'_> {
    /// The names `record` is under.
    fn each(
        &self,
        record: &catalog::Record,
        f: &mut dyn FnMut(&str) -> Result<(), i32>,
    ) -> Result<(), i32> {
        match self.facet {
            Facet::Authors => f(&record.author),
            Facet::Series if record.series.is_empty() => Ok(()),
            Facet::Series => f(&record.series),
            Facet::Tags => record.tags().try_for_each(f),
            Facet::Letters => {
                let letter = [index::letter(&record.title)];
                f(core::str::from_utf8(&letter).unwrap_or("#"))
            }
            Facet::Folders => folder_in(&record.path, &self.prefix).map_or(Ok(()), f),
        }
    }

    /// One scan of the catalog: every book found goes to `f`.
    fn books(&self, f: &mut dyn FnMut(&catalog::Record) -> Result<(), i32>) -> Result<(), i32> {
        let (mut words, mut failure) = (String::new(), None);
        catalog::try_each_at_with(
            &*self.engine.reads.storage,
            &self.engine.changes,
            &mut |_, record| {
                if failure.is_none() {
                    failure = self
                        .engine
                        .keeps(&record, &mut words)
                        .and_then(|kept| if kept { f(&record) } else { Ok(()) })
                        .err();
                }
            },
        )
        .map_err(fail)?;
        failure.map_or(Ok(()), Err)
    }

    /// One scan of the catalog: every name of every book found goes to `pass`.
    fn pass(&self, pass: &mut Pass<'_>) -> Result<(), i32> {
        let (mut name, mut book) = (String::new(), Key::default());
        self.books(&mut |record| {
            self.each(record, &mut |text| {
                name_key(&mut name, text)?;
                if let Some(counted) = pass.counted.as_deref_mut() {
                    counted.note(fnv1a64(&name))?;
                }
                if let Some(at) = pass.window.offer(&mut name, pass.after)
                    && pass.books
                {
                    self.engine.fill(record, &mut book)?;
                    pass.window.book(at, &mut book);
                }
                Ok(())
            })
        })
    }

    /// How many names there are, from the count the first scan made: where
    /// they were more than it holds, the hashes are counted half by half, a
    /// scan each, until every part fits (the left half first; after a part
    /// that fits, the part beside it or beside the half it closes).
    fn count(&self, mut counted: Distinct) -> Result<u32, i32> {
        if !counted.over {
            return Ok(counted.len());
        }
        let mut window = Window::new(0, false)?;
        // The whole was over: its lower half first.
        let (mut low, mut high, mut total) = (0_u64, u64::MAX / 2, 0_u32);
        loop {
            counted.hashes.clear();
            (counted.low, counted.high, counted.over) = (low, high, false);
            self.pass(&mut Pass {
                window: &mut window,
                after: None,
                counted: Some(&mut counted),
                books: false,
            })?;
            if counted.over && low < high {
                high = low + (high - low) / 2;
                continue;
            }
            total = total.saturating_add(counted.len());
            if high == u64::MAX {
                return Ok(total);
            }
            // The part that begins after this one: as wide as its start is even.
            low = high + 1;
            high = low + ((low & low.wrapping_neg()) - 1);
        }
    }

    /// What the scans found: how many names in all, how many the cursor
    /// skipped (all there are, where it is past them) and the names of the
    /// page. The first scan counts the names too; from a place kept (`from`,
    /// which is at `cursor`) the page is the first scan and nothing is counted.
    fn select(&self, cursor: u32, from: Option<Kept>) -> Result<(u32, u32, Vec<Name>), i32> {
        let (mut after, mut skipped, mut total) = match from {
            Some(kept) => (Some(kept.after), kept.cursor, Some(kept.total)),
            None => (None, 0, None),
        };
        loop {
            // A step towards the cursor, or at it the page.
            let page = skipped >= cursor;
            let want = if page {
                WINDOW
            } else {
                usize::try_from(cursor - skipped).map_or(WINDOW, |n| n.min(WINDOW))
            };
            let mut window = Window::new(want, self.descending)?;
            let mut counted = Distinct::new();
            self.pass(&mut Pass {
                window: &mut window,
                after: after.as_deref(),
                counted: total.is_none().then_some(&mut counted),
                books: page && self.facet.jump() == Jump::Book,
            })?;
            let found = match total {
                Some(known) => known,
                None => *total.insert(self.count(counted)?),
            };
            if page {
                return Ok((found, skipped, window.names));
            }
            let taken = u32::try_from(window.names.len()).unwrap_or(u32::MAX);
            skipped = skipped.saturating_add(taken);
            if window.names.len() < want {
                // The cursor is past the last name.
                return Ok((found, skipped.min(found), Vec::new()));
            }
            after = window.names.pop().map(|name| name.key);
        }
    }

    /// Where the books of `name` begin in the list's order ([`Jump`]): as a
    /// [`Key`] is compared.
    fn place<'n>(&self, name: &'n Name) -> (u32, &'n str) {
        match self.facet.jump() {
            Jump::Block(cut) => (0, head(name.parts().0, cut)),
            _ => (name.first_book.first, name.first_book.text.as_str()),
        }
    }

    /// Whether the books of `name` begin after `place`, in the list's order.
    fn after(&self, name: &Name, place: (u32, &str)) -> bool {
        let order = self.place(name).cmp(&place);
        order
            == if self.descending {
                Ordering::Less
            } else {
                Ordering::Greater
            }
    }

    /// `first` of each of `names` (the page: [`WINDOW`] at most): how many
    /// of the books found come before the name's first book, or before the
    /// books of its sort key ([`Jump`]), in the list's order. One scan.
    fn firsts(&self, names: &mut [Name]) -> Result<(), i32> {
        // The names by where their books begin, each after those that begin
        // before it or with it (as they are kept, but for the letters, and for
        // a name with a character across the cut the books are counted by:
        // it is left out, which can put a name before one it is kept after).
        let mut order = [0_u8; WINDOW];
        let order = &mut order[..names.len().min(WINDOW)];
        for at in 0..order.len() {
            let mut to = at;
            while to > 0 && self.after(&names[usize::from(order[to - 1])], self.place(&names[at])) {
                order[to] = order[to - 1];
                to -= 1;
            }
            order[to] = at as u8;
        }
        // For each place how many books come right before it (after the
        // place before), counted in the name's `first`.
        let mut book = Key::default();
        self.books(&mut |record| {
            if let Jump::Block(cut) = self.facet.jump() {
                let text = &mut book.text;
                text.clear();
                let value = match self.facet {
                    Facet::Series => &record.series,
                    _ => &record.author,
                };
                push_lower(text, value, cut)?;
                text.truncate(head(text, cut).len());
            } else {
                self.engine.fill(record, &mut book)?;
            }
            // The names that begin up to this book: it comes before the rest.
            let here = (book.first, book.text.as_str());
            let at = order.partition_point(|name| !self.after(&names[usize::from(*name)], here));
            if let Some(name) = order.get(at) {
                let first = &mut names[usize::from(*name)].first;
                *first = first.saturating_add(1);
            }
            Ok(())
        })?;
        let mut sum = 0_u32;
        for name in order.iter() {
            let first = &mut names[usize::from(*name)].first;
            sum = sum.saturating_add(*first);
            *first = sum;
        }
        Ok(())
    }
}

/// The folder the folders of a query are right in, with its `/` (empty: the
/// card's top): the query's `folder`, or the library folder where it names
/// none.
fn folders_in(reads: &CatalogReads, folder: &str) -> Result<String, i32> {
    let mut prefix = String::new();
    let mut failure = None;
    let mut root = |mut folder: &str| {
        while let Some(rest) = folder.strip_prefix('/') {
            folder = rest;
        }
        while let Some(rest) = folder.strip_suffix('/') {
            folder = rest;
        }
        if !folder.is_empty() {
            failure = copy(folder)
                .and_then(|mut path| push(&mut path, "/").map(|()| prefix = path))
                .err();
        }
    };
    if folder.is_empty() {
        reads.library_folder(&mut root);
    } else {
        root(folder);
    }
    if let Some(status) = failure {
        return Err(status);
    }
    Ok(prefix)
}

/// The rows from `q.cursor`, and where the page ends. `hide` names the books
/// a pending delete holds.
pub(super) fn facets(
    reads: &CatalogReads,
    q: &wire::ServiceLibQuery<'_>,
    archive: &str,
    hide: &dyn Fn(&str) -> bool,
    visit: Rows<'_>,
) -> Result<RowsEnd, i32> {
    let facet = Facet::of(q.facet);
    // The books in the order `first` counts in.
    let sorted = wire::ServiceLibQuery {
        sort: facet.sort(),
        ..*q
    };
    let prefix = if facet == Facet::Folders {
        folders_in(reads, q.folder)?
    } else {
        String::new()
    };
    let stamp = stamp(reads, q, archive, &prefix);
    // Whether a pending delete hid a book from the scans.
    let hid = Cell::new(false);
    let hide = |path: &str| {
        let hidden = hide(path);
        if hidden {
            hid.set(true);
        }
        hidden
    };
    let lists = Lists {
        engine: Engine::new(reads, &sorted, archive, &hide)?,
        facet,
        prefix,
        descending: q.flags & 1 != 0,
    };
    for _ in 0..TRIES {
        let generation = reads.generation()?;
        if q.generation != 0 && q.generation != generation {
            return Err(ERR_STATE);
        }
        // The place the call before ended at, where this one goes on from it.
        let mut kept = reads.names.take().filter(|kept| {
            q.cursor != 0
                && kept.cursor == q.cursor
                && kept.stamp == stamp
                && kept.generation == generation
        });
        let (total, skipped, mut names) = loop {
            hid.set(false);
            let went_on = kept.is_some();
            let found = lists.select(q.cursor, kept.take())?;
            // A book being deleted may have taken a name from before the
            // place: then from the start, as a call without one.
            if !(went_on && hid.get()) {
                break found;
            }
        };
        let jumps = facet.jump() != Jump::None;
        if jumps && !names.is_empty() {
            lists.firsts(&mut names)?;
        }
        if reads.generation()? != generation {
            // Read again where the caller named no generation to hold.
            if q.generation != 0 {
                return Err(ERR_STATE);
            }
            continue;
        }
        let mut rows = 0_u32;
        for name in &names {
            let row = wire::ServiceLibFacet {
                text: name.parts().1,
                count: name.count,
                first: if jumps { name.first } else { NO_JUMP },
            };
            if visit(Record::ServiceLibFacet(row)).is_break() {
                break;
            }
            rows += 1;
        }
        // Where this page ended, for a call that goes on from there.
        names.truncate(rows as usize);
        if let Some(last) = names.pop()
            && !hid.get()
        {
            // The key's buffer as it is: shrinking it is an allocation that
            // cannot be refused, and it holds 706 bytes at most.
            reads.names.set(Some(Kept {
                stamp,
                generation,
                cursor: skipped + rows,
                total,
                after: last.key,
            }));
        }
        return Ok(RowsEnd {
            total,
            generation,
            next: skipped + rows,
        });
    }
    Err(ERR_STATE)
}

#[cfg(test)]
#[path = "facets_tests.rs"]
mod tests;
