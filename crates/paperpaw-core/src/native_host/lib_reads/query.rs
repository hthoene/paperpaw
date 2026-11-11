//! Op 2 `query` over the catalog (`docs/abi/library.md` 2.1): the books a
//! `lib_query` finds, in its order, as `lib_entry` rows from its cursor.
//!
//! Memory: no list outlives the call and none grows with the catalog. The
//! catalog is read a line at a time (the SDK's `try_each_at`) and a book that
//! is found only leaves a *slot* behind: its sort key (cut at 96 bytes of text
//! and the path) and where its line starts. A pass keeps the [`WINDOW`]
//! first slots after a bound (keyset selection); a cursor of `k` rows is
//! reached by `k / WINDOW` passes that keep keys only, each starting after the
//! last key of the pass before, so the page costs `cursor / 32 + 2` scans of the
//! catalog and never a list of the result. The first pass counts what is
//! found (`end.total`). The rows are made from the slots of the last pass,
//! one line of the catalog a row (title, author and series read into 288 bytes).
//! A book that is not kept costs no allocation: its key is made in one scratch
//! key (the buffers of the slot it pushes out come back to it) and compared
//! with the worst slot before it is moved into one.
//! Checkpoints (library.md 2.1) are a cache the contract leaves out; none yet.
//!
//! The order is the SDK's `Query::key` (title, author then title, series then
//! number, added last first), a descending flag reverses all of it, and a tie
//! is the path. Sort 4 (recently opened) is the place in `opened.txt`, then
//! the title. A lowercase is made character by character (no final-sigma rule),
//! so the order is stable for one catalog generation, which is all the
//! contract asks. A catalog that changes under a call (its generation does) is
//! read again; where the caller named a generation, that is `ERR_STATE`.
use super::book::{self, flag};
use super::{CatalogReads, fail, valid_prefix, zeroed};
use crate::abi7::context::RowsEnd;
use crate::native_host::lib_service::Rows;
use crate::native_host::registrations::{self as wire, Record};
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE};
use paperpaw_sdk::data;
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::library::{self, catalog};

/// Slots a pass keeps: the page, or a step towards the cursor.
const WINDOW: usize = 32;
/// Bytes of the lowercase text a key holds.
const KEY_TEXT: usize = 96;
/// Books the opened list names at most (the SDK's `MAX_OPENED`).
const OPENED_MAX: usize = 100;
/// Tries at a catalog that changes under the call.
const TRIES: usize = 3;
/// Bytes of the cut forms in a `lib_entry`.
const TITLE: usize = 128;
const AUTHOR: usize = 96;
const SERIES: usize = 64;
const NUMBER: usize = 16;
/// `lib_entry.progress` of a book never opened.
const NEVER_OPENED: u32 = 255;

/// Where a book stands in the order: the first key, the lowercase text and the
/// path. Compared as they are laid out.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    first: u32,
    text: String,
    path: String,
}

/// A book that was found: its key and where its line starts.
struct Slot {
    key: Key,
    offset: u64,
}

/// The first `want` slots after a bound, in order.
struct Selection {
    slots: Vec<Slot>,
    want: usize,
    descending: bool,
}

impl Selection {
    fn new(want: usize, descending: bool) -> Result<Self, i32> {
        let mut slots = Vec::new();
        slots.try_reserve_exact(want).map_err(|_| ERR_NO_MEMORY)?;
        Ok(Self {
            slots,
            want,
            descending,
        })
    }

    fn order(&self, a: &Key, b: &Key) -> Ordering {
        if self.descending { b.cmp(a) } else { a.cmp(b) }
    }

    /// Takes the book whose key is in `scratch` if it is after `after` and
    /// before the worst slot kept (or the window is not full): the key moves
    /// into the slot and the scratch takes the dropped slot's buffers, so a
    /// book that is not kept costs no allocation.
    fn offer(&mut self, scratch: &mut Key, offset: u64, after: Option<&Key>) {
        if self.want == 0 || after.is_some_and(|a| self.order(scratch, a) != Ordering::Greater) {
            return;
        }
        let full = self.slots.len() >= self.want;
        if full
            && self
                .slots
                .last()
                .is_none_or(|worst| self.order(scratch, &worst.key) != Ordering::Less)
        {
            return;
        }
        let at = self
            .slots
            .partition_point(|s| self.order(&s.key, scratch) == Ordering::Less);
        let fresh = if full {
            self.slots.pop().map(|slot| slot.key).unwrap_or_default()
        } else {
            Key::default()
        };
        let key = core::mem::replace(scratch, fresh);
        // The room is there: the window was reserved, and one slot is out.
        self.slots.insert(at, Slot { key, offset });
    }
}

/// `text` appended to `out` in lowercase, reserved as it grows, until `out`
/// is `cap` bytes (a character may pass it by two).
fn push_lower(out: &mut String, text: &str, cap: usize) -> Result<(), i32> {
    for ch in text.chars() {
        if out.len() >= cap {
            break;
        }
        for low in ch.to_lowercase() {
            out.try_reserve(4).map_err(|_| ERR_NO_MEMORY)?;
            out.push(low);
        }
    }
    Ok(())
}

fn push(out: &mut String, text: &str) -> Result<(), i32> {
    out.try_reserve(text.len()).map_err(|_| ERR_NO_MEMORY)?;
    out.push_str(text);
    Ok(())
}

/// A copy of `text`, reserved fallibly.
fn copy(text: &str) -> Result<String, i32> {
    let mut out = String::new();
    push(&mut out, text)?;
    Ok(out)
}

/// `text` cut to at most `max` bytes at a character.
fn head(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The sort of the wire.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    Title,
    Author,
    Series,
    Added,
    Recent,
}

/// What the call asks, made ready: the conditions of the books, the order,
/// and the opened list for sort 4.
struct Engine<'a> {
    reads: &'a CatalogReads,
    /// Show (but `new`), author, series, tag, folder and day.
    base: catalog::Query,
    new_only: bool,
    words: Vec<String>,
    archive: Option<String>,
    order: Order,
    /// The hashes of the paths opened last, the last first (sort 4 only).
    opened: Vec<u64>,
    /// A book a pending delete holds is not listed.
    hide: &'a dyn Fn(&str) -> bool,
}

impl<'a> Engine<'a> {
    fn new(
        reads: &'a CatalogReads,
        q: &wire::ServiceLibQuery<'_>,
        archive: &str,
        hide: &'a dyn Fn(&str) -> bool,
    ) -> Result<Self, i32> {
        use catalog::Show;
        let mut base = catalog::Query::new();
        base.show = match q.show {
            1 => Show::Unread,
            2 => Show::Reading,
            3 => Show::Finished,
            4 => Show::Favourites,
            _ => Show::All,
        };
        let named = |text: &str| -> Result<Option<String>, i32> {
            if text.is_empty() {
                Ok(None)
            } else {
                copy(text).map(Some)
            }
        };
        base.author = named(q.author)?;
        base.series = named(q.series)?;
        base.tag = named(q.tag)?;
        base.folder = named(q.folder)?;
        base.added_since = (q.added_since != 0).then_some(q.added_since);
        let mut words = Vec::new();
        for word in q.words.split_whitespace() {
            words.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            let mut lower = String::new();
            push_lower(&mut lower, word, usize::MAX)?;
            words.push(lower);
        }
        // The archive folder, left out where the query says so.
        let folder = archive.trim_matches('/');
        let archive = if q.flags & 0b10 != 0 && !folder.is_empty() {
            Some(copy(folder)?)
        } else {
            None
        };
        let order = match q.sort {
            1 => Order::Author,
            2 => Order::Series,
            3 => Order::Added,
            4 => Order::Recent,
            _ => Order::Title,
        };
        let opened = if order == Order::Recent {
            opened_list(reads)?
        } else {
            Vec::new()
        };
        Ok(Self {
            reads,
            base,
            new_only: q.show == 5,
            words,
            archive,
            order,
            opened,
            hide,
        })
    }

    /// Whether the book is found.
    fn keeps(&self, record: &catalog::Record, words: &mut String) -> Result<bool, i32> {
        if (self.hide)(&record.path) || !self.base.finds(record) {
            return Ok(false);
        }
        if let Some(archive) = &self.archive
            && in_folder(&record.path, archive)
        {
            return Ok(false);
        }
        if self.new_only {
            let opened = record.status.opened || record.status.progress.is_some();
            if !book::is_new(self.reads, record.added, opened) {
                return Ok(false);
            }
        }
        self.has_words(record, words)
    }

    /// Whether every word is in the title, author, series, tags or file name.
    fn has_words(&self, record: &catalog::Record, text: &mut String) -> Result<bool, i32> {
        if self.words.is_empty() {
            return Ok(true);
        }
        // One buffer for every book of the pass.
        text.clear();
        for piece in [&record.title, &record.author, &record.series] {
            push_lower(text, piece, usize::MAX)?;
            push(text, " ")?;
        }
        for tag in record.tags() {
            push_lower(text, tag, usize::MAX)?;
            push(text, " ")?;
        }
        push_lower(
            text,
            record.path.rsplit('/').next().unwrap_or_default(),
            usize::MAX,
        )?;
        Ok(self.words.iter().all(|word| text.contains(word.as_str())))
    }

    /// Where the book goes in the order, made in `key` (its buffers are
    /// reused, and the text stops at [`KEY_TEXT`] bytes).
    fn fill(&self, record: &catalog::Record, key: &mut Key) -> Result<(), i32> {
        key.text.clear();
        key.path.clear();
        key.first = 0;
        let text = &mut key.text;
        match self.order {
            Order::Title => push_lower(text, &record.title, KEY_TEXT)?,
            Order::Author => {
                push_lower(text, &record.author, KEY_TEXT)?;
                push(text, "\u{1}")?;
                push_lower(text, &record.title, KEY_TEXT)?;
            }
            Order::Series => {
                // The ten digits of the number are kept whole.
                push_lower(text, &record.series, KEY_TEXT - 11)?;
                text.truncate(head(text, KEY_TEXT - 11).len());
                push(text, "\u{1}")?;
                let mut digits = [b'0'; 10];
                let mut rest = catalog::series_order(&record.number);
                for slot in digits.iter_mut().rev() {
                    *slot = b'0' + (rest % 10) as u8;
                    rest /= 10;
                }
                push(text, core::str::from_utf8(&digits).unwrap_or_default())?;
            }
            Order::Added => {
                key.first = u32::MAX - record.added.unwrap_or(0);
                push_lower(text, &record.title, KEY_TEXT)?;
            }
            Order::Recent => {
                let hash = fnv1a64(&record.path);
                key.first = self
                    .opened
                    .iter()
                    .position(|h| *h == hash)
                    .and_then(|at| u32::try_from(at).ok())
                    .unwrap_or(u32::MAX);
                push_lower(text, &record.title, KEY_TEXT)?;
            }
        }
        text.truncate(head(text, KEY_TEXT).len());
        push(&mut key.path, &record.path)
    }

    /// One scan of the catalog: the books found are offered to `selection`
    /// (those after `after`); returns how many were found in all.
    fn pass(&self, selection: &mut Selection, after: Option<&Key>) -> Result<u32, i32> {
        let (mut found, mut failure) = (0_u32, None);
        // The key of the book being looked at and the text the words are
        // looked for in: made once, filled for each book.
        let (mut scratch, mut words) = (Key::default(), String::new());
        catalog::try_each_at(&*self.reads.storage, &mut |offset, record| {
            if failure.is_some() {
                return;
            }
            let outcome = self.keeps(&record, &mut words).and_then(|kept| {
                if kept {
                    self.fill(&record, &mut scratch)?;
                    selection.offer(&mut scratch, offset, after);
                }
                Ok(kept)
            });
            match outcome {
                Ok(kept) => found = found.saturating_add(u32::from(kept)),
                Err(status) => failure = Some(status),
            }
        })
        .map_err(fail)?;
        failure.map_or(Ok(found), Err)
    }
}

/// Whether `path` is `folder` or below it.
fn in_folder(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// The paths opened last as hashes, the last first.
fn opened_list(reads: &CatalogReads) -> Result<Vec<u64>, i32> {
    let mut file = String::new();
    file.try_reserve_exact(library::DIR.len() + "/opened.txt".len())
        .map_err(|_| ERR_NO_MEMORY)?;
    file.push_str(library::DIR);
    file.push_str("/opened.txt");
    let text = data::Format::FIRST
        .try_load(&*reads.storage, &file, OPENED_MAX * 512)
        .map_err(fail)?;
    let mut hashes = Vec::new();
    hashes
        .try_reserve_exact(OPENED_MAX)
        .map_err(|_| ERR_NO_MEMORY)?;
    for line in text.as_deref().unwrap_or_default().lines().take(OPENED_MAX) {
        if !line.is_empty() {
            hashes.push(fnv1a64(line));
        }
    }
    Ok(hashes)
}

/// The rows from `cursor`, and where the page ends. `hide` names the books a
/// pending delete holds.
pub(super) fn query(
    reads: &CatalogReads,
    q: &wire::ServiceLibQuery<'_>,
    archive: &str,
    hide: &dyn Fn(&str) -> bool,
    visit: Rows<'_>,
) -> Result<RowsEnd, i32> {
    let engine = Engine::new(reads, q, archive, hide)?;
    for _ in 0..TRIES {
        let generation = reads.generation()?;
        if q.generation != 0 && q.generation != generation {
            return Err(ERR_STATE);
        }
        let (total, skipped, slots) = select(&engine, q)?;
        if reads.generation()? != generation {
            // Read again where the caller named no generation to hold.
            if q.generation != 0 {
                return Err(ERR_STATE);
            }
            continue;
        }
        let rows = emit(reads, &slots, visit)?;
        return Ok(RowsEnd {
            total,
            generation,
            next: skipped + rows,
        });
    }
    Err(ERR_STATE)
}

/// What the passes found: how many books in all, how many rows the cursor
/// skipped (all there are, where the cursor is past them) and the slots of the
/// page.
fn select(
    engine: &Engine<'_>,
    q: &wire::ServiceLibQuery<'_>,
) -> Result<(u32, u32, Vec<Slot>), i32> {
    let descending = q.flags & 1 != 0;
    let (mut after, mut skipped, mut counted) = (None::<Key>, 0_u32, None);
    while skipped < q.cursor {
        let step = usize::try_from(q.cursor - skipped).map_or(WINDOW, |n| n.min(WINDOW));
        let mut selection = Selection::new(step, descending)?;
        let found = engine.pass(&mut selection, after.as_ref())?;
        let total = *counted.get_or_insert(found);
        let taken = u32::try_from(selection.slots.len()).unwrap_or(u32::MAX);
        skipped = skipped.saturating_add(taken);
        if selection.slots.len() < step {
            // The cursor is past the last book.
            return Ok((total, skipped.min(total), Vec::new()));
        }
        after = selection.slots.pop().map(|slot| slot.key);
    }
    let mut selection = Selection::new(WINDOW, descending)?;
    let found = engine.pass(&mut selection, after.as_ref())?;
    Ok((counted.unwrap_or(found), skipped, selection.slots))
}

/// The `lib_entry` of each slot, until the page is full; how many were taken.
fn emit(reads: &CatalogReads, slots: &[Slot], visit: Rows<'_>) -> Result<u32, i32> {
    if slots.is_empty() {
        return Ok(0);
    }
    let changes = catalog::try_changes(&*reads.storage).map_err(fail)?;
    let mut buffer = zeroed(TITLE + AUTHOR + SERIES)?;
    let mut rows = 0_u32;
    for slot in slots {
        if row(reads, &changes, slot, &mut buffer, visit)?.is_break() {
            break;
        }
        rows += 1;
    }
    Ok(rows)
}

/// What of a line `row` keeps beyond the three texts: the numbers.
struct Line {
    number: [u8; 32],
    number_len: usize,
    bytes: [u32; 3],
    status: catalog::Status,
    added: Option<u32>,
    size: Option<u64>,
}

/// The line at the slot's offset, which is that book still: its numbers and
/// the cut forms of its texts in `buffer`; a line that is another book's
/// means the catalog changed (`ERR_STATE`).
fn line(
    reads: &CatalogReads,
    changes: &catalog::Changes,
    slot: &Slot,
    buffer: &mut [u8],
) -> Result<(Line, catalog::Filled), i32> {
    let (title_b, rest) = buffer.split_at_mut(TITLE);
    let (author_b, series_b) = rest.split_at_mut(AUTHOR);
    let mut line = None;
    let seen = catalog::try_book_at_with(
        &*reads.storage,
        changes,
        slot.offset,
        catalog::BookTexts::new(title_b, author_b, series_b),
        &mut |view, filled| {
            let mut number = [0_u8; 32];
            let given = view.number.as_bytes();
            let kept = given.len().min(number.len());
            number[..kept].copy_from_slice(&given[..kept]);
            if view.path == slot.key.path {
                line = Some((
                    Line {
                        number,
                        number_len: kept,
                        bytes: [view.title_bytes, view.author_bytes, view.series_bytes],
                        status: view.status,
                        added: view.added,
                        size: view.size,
                    },
                    filled,
                ));
            }
        },
    )
    .map_err(fail)?;
    match (seen, line) {
        (true, Some(found)) => Ok(found),
        // The book's line is gone or is another's now.
        _ => Err(ERR_STATE),
    }
}

/// `Break` when the page cannot take the row: it is not counted.
fn row(
    reads: &CatalogReads,
    changes: &catalog::Changes,
    slot: &Slot,
    buffer: &mut [u8],
    visit: Rows<'_>,
) -> Result<ControlFlow<()>, i32> {
    let (line, filled) = line(reads, changes, slot, buffer)?;
    let (title_b, rest) = buffer.split_at(TITLE);
    let (author_b, series_b) = rest.split_at(AUTHOR);
    let title = valid_prefix(&title_b[..filled.title]);
    let author = valid_prefix(&author_b[..filled.author]);
    let series = valid_prefix(&series_b[..filled.series]);
    let number = head(
        core::str::from_utf8(&line.number[..line.number_len]).unwrap_or_default(),
        NUMBER,
    );
    let status = line.status;
    let opened = status.opened || status.progress.is_some();
    let indexed = book::indexed(reads, &slot.key.path, line.size)?;
    let mut flags = 0;
    for (set, bit) in [
        (status.favourite, flag::FAVOURITE),
        (opened, flag::OPENED),
        (status.progress == Some(100), flag::FINISHED),
        (book::is_new(reads, line.added, opened), flag::NEW),
        (indexed, flag::INDEXED),
        (bytes(title) < line.bytes[0], flag::CUT[0]),
        (bytes(author) < line.bytes[1], flag::CUT[1]),
        (bytes(series) < line.bytes[2], flag::CUT[2]),
    ] {
        if set {
            flags |= bit;
        }
    }
    Ok(visit(Record::ServiceLibEntry(wire::ServiceLibEntry {
        key: u32::try_from(slot.offset).map_err(|_| ERR_NO_SPACE)?,
        title,
        author,
        series,
        number,
        progress: status.progress.map_or(NEVER_OPENED, u32::from),
        flags,
        added: line.added.unwrap_or(0),
        badges: "",
    })))
}

fn bytes(text: &str) -> u32 {
    u32::try_from(text.len()).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
