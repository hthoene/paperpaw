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
use super::query::{Engine, KEY_TEXT, Key, TRIES, WINDOW, copy, head, push, push_lower};
use super::{CatalogReads, fail};
use crate::abi7::context::RowsEnd;
use crate::native_host::lib_service::Rows;
use crate::native_host::registrations::{self as wire, Record};
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::library::{catalog, index};

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
    let (folder, _) = inner.split_once('/')?;
    path.get(..path.len() - inner.len() + folder.len())
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
/// many books have it and the first of them in the list's order.
#[derive(Default)]
struct Name {
    key: String,
    count: u32,
    first_book: Key,
    has_book: bool,
}

impl Name {
    fn text(&self) -> &str {
        self.key.split_once('\0').map_or("", |(_, text)| text)
    }

    /// Its lowercase, as it is ordered by.
    fn lower(&self) -> &str {
        self.key.split_once('\0').map_or("", |(lower, _)| lower)
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
        (name.count, name.has_book) = (1, false);
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

    /// One scan of the catalog: every name of every book found goes to `pass`.
    fn pass(&self, pass: &mut Pass<'_>) -> Result<(), i32> {
        let (mut name, mut words, mut book) = (String::new(), String::new(), Key::default());
        let mut failure = None;
        catalog::try_each_at(&*self.engine.reads.storage, &mut |_, record| {
            if failure.is_some() {
                return;
            }
            let outcome = self.engine.keeps(&record, &mut words).and_then(|kept| {
                if !kept {
                    return Ok(());
                }
                self.each(&record, &mut |text| {
                    name_key(&mut name, text)?;
                    if let Some(counted) = pass.counted.as_deref_mut() {
                        counted.note(fnv1a64(&name))?;
                    }
                    if let Some(at) = pass.window.offer(&mut name, pass.after)
                        && pass.books
                    {
                        self.engine.fill(&record, &mut book)?;
                        pass.window.book(at, &mut book);
                    }
                    Ok(())
                })
            });
            if let Err(status) = outcome {
                failure = Some(status);
            }
        })
        .map_err(fail)?;
        failure.map_or(Ok(()), Err)
    }

    /// How many names there are, from the count the first scan made: where
    /// they were more than it holds, the hashes are counted half by half, a
    /// scan each, until every part fits.
    fn count(&self, mut counted: Distinct) -> Result<u32, i32> {
        if !counted.over {
            return Ok(counted.len());
        }
        let mut parts: Vec<(u64, u64)> = Vec::new();
        parts.try_reserve(2).map_err(|_| ERR_NO_MEMORY)?;
        parts.push((0, u64::MAX));
        let mut window = Window::new(0, false)?;
        let mut total = 0_u32;
        // The whole was over: its halves first.
        let mut split = true;
        while let Some((low, high)) = parts.pop() {
            if !split {
                counted.hashes.clear();
                (counted.low, counted.high, counted.over) = (low, high, false);
                self.pass(&mut Pass {
                    window: &mut window,
                    after: None,
                    counted: Some(&mut counted),
                    books: false,
                })?;
            }
            if (split || counted.over) && low < high {
                let middle = low + (high - low) / 2;
                parts.try_reserve(2).map_err(|_| ERR_NO_MEMORY)?;
                parts.push((middle + 1, high));
                parts.push((low, middle));
            } else {
                total = total.saturating_add(counted.len());
            }
            split = false;
        }
        Ok(total)
    }

    /// A scan for `window`; the first of a call counts the names too.
    fn scan(
        &self,
        window: &mut Window,
        after: Option<&str>,
        total: &mut Option<u32>,
        books: bool,
    ) -> Result<u32, i32> {
        let mut counted = total.is_none().then(Distinct::new);
        self.pass(&mut Pass {
            window,
            after,
            counted: counted.as_mut(),
            books,
        })?;
        match (*total, counted) {
            (Some(known), _) => Ok(known),
            (None, Some(counted)) => Ok(*total.insert(self.count(counted)?)),
            (None, None) => Err(ERR_STATE),
        }
    }

    /// What the scans found: how many names in all, how many the cursor
    /// skipped (all there are, where it is past them) and the names of the
    /// page.
    fn select(&self, cursor: u32) -> Result<(u32, u32, Vec<Name>), i32> {
        let (mut after, mut skipped, mut total) = (None::<String>, 0_u32, None);
        while skipped < cursor {
            let step = usize::try_from(cursor - skipped).map_or(WINDOW, |n| n.min(WINDOW));
            let mut window = Window::new(step, self.descending)?;
            let found = self.scan(&mut window, after.as_deref(), &mut total, false)?;
            let taken = u32::try_from(window.names.len()).unwrap_or(u32::MAX);
            skipped = skipped.saturating_add(taken);
            if window.names.len() < step {
                // The cursor is past the last name.
                return Ok((found, skipped.min(found), Vec::new()));
            }
            after = window.names.pop().map(|name| name.key);
        }
        let mut window = Window::new(WINDOW, self.descending)?;
        let books = self.facet.jump() == Jump::Book;
        let found = self.scan(&mut window, after.as_deref(), &mut total, books)?;
        Ok((found, skipped, window.names))
    }

    /// `first` of each of `names`: how many of the books found come before
    /// the name's first book, or before the books of its sort key
    /// ([`Jump`]), in the list's order. One scan.
    fn firsts(&self, names: &[Name]) -> Result<Vec<u32>, i32> {
        let jump = self.facet.jump();
        let before = |a: &Key, b: &Key| if self.descending { b.cmp(a) } else { a.cmp(b) };
        let before_text = |a: &str, b: &str| if self.descending { b.cmp(a) } else { a.cmp(b) };
        // The names by where their books begin (as they are kept, but for
        // the letters), and for each place how many books come right before
        // it: after the place before.
        let mut order: Vec<usize> = Vec::new();
        order
            .try_reserve_exact(names.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        order.extend(0..names.len());
        match jump {
            Jump::Book => {
                order.sort_unstable_by(|a, b| before(&names[*a].first_book, &names[*b].first_book));
            }
            // By the cut the books are counted by: a character across it is
            // left out, which can put a name before one it is kept after.
            Jump::Block(cut) => order.sort_unstable_by(|a, b| {
                let (one, other) = (head(names[*a].lower(), cut), head(names[*b].lower(), cut));
                before_text(one, other).then(a.cmp(b))
            }),
            Jump::None => {}
        }
        let mut steps = zeroes(names.len())?;
        let (mut words, mut book) = (String::new(), Key::default());
        let mut failure = None;
        catalog::try_each_at(&*self.engine.reads.storage, &mut |_, record| {
            if failure.is_some() {
                return;
            }
            let outcome = self.engine.keeps(&record, &mut words).and_then(|kept| {
                if !kept {
                    return Ok(());
                }
                // The names that begin up to this book: it comes before the rest.
                let at = if let Jump::Block(cut) = jump {
                    let text = &mut book.text;
                    text.clear();
                    let value = match self.facet {
                        Facet::Series => &record.series,
                        _ => &record.author,
                    };
                    push_lower(text, value, cut)?;
                    let block = head(text, cut);
                    order.partition_point(|name| {
                        before_text(head(names[*name].lower(), cut), block) != Ordering::Greater
                    })
                } else {
                    self.engine.fill(&record, &mut book)?;
                    order.partition_point(|name| {
                        before(&names[*name].first_book, &book) != Ordering::Greater
                    })
                };
                if let Some(step) = steps.get_mut(at) {
                    *step = step.saturating_add(1);
                }
                Ok(())
            });
            if let Err(status) = outcome {
                failure = Some(status);
            }
        })
        .map_err(fail)?;
        if let Some(status) = failure {
            return Err(status);
        }
        let mut firsts = zeroes(names.len())?;
        let mut sum = 0_u32;
        for (place, name) in order.iter().enumerate() {
            sum = sum.saturating_add(steps[place]);
            firsts[*name] = sum;
        }
        Ok(firsts)
    }
}

/// `len` zeroes, reserved fallibly.
fn zeroes(len: usize) -> Result<Vec<u32>, i32> {
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| ERR_NO_MEMORY)?;
    out.resize(len, 0);
    Ok(out)
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
    let mut prefix = String::new();
    if facet == Facet::Folders {
        let mut failure = None;
        let mut root = |folder: &str| {
            let folder = folder.trim_matches('/');
            if !folder.is_empty() {
                failure = copy(folder)
                    .and_then(|mut path| push(&mut path, "/").map(|()| prefix = path))
                    .err();
            }
        };
        if q.folder.is_empty() {
            reads.library_folder(&mut root);
        } else {
            root(q.folder);
        }
        if let Some(status) = failure {
            return Err(status);
        }
    }
    let lists = Lists {
        engine: Engine::new(reads, &sorted, archive, hide)?,
        facet,
        prefix,
        descending: q.flags & 1 != 0,
    };
    for _ in 0..TRIES {
        let generation = reads.generation()?;
        if q.generation != 0 && q.generation != generation {
            return Err(ERR_STATE);
        }
        let (total, skipped, names) = lists.select(q.cursor)?;
        let firsts = if facet.jump() != Jump::None && !names.is_empty() {
            Some(lists.firsts(&names)?)
        } else {
            None
        };
        if reads.generation()? != generation {
            // Read again where the caller named no generation to hold.
            if q.generation != 0 {
                return Err(ERR_STATE);
            }
            continue;
        }
        let mut rows = 0_u32;
        for (at, name) in names.iter().enumerate() {
            let first = firsts
                .as_ref()
                .and_then(|firsts| firsts.get(at).copied())
                .unwrap_or(NO_JUMP);
            let row = wire::ServiceLibFacet {
                text: name.text(),
                count: name.count,
                first,
            };
            if visit(Record::ServiceLibFacet(row)).is_break() {
                break;
            }
            rows += 1;
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
