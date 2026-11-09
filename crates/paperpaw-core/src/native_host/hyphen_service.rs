//! Slot 38 `text.hyphenate` (`docs/abi/document.md` section 6.1): where a word
//! may break, from the pattern tables of the data plugins' `[hyphenation]`
//! files, read from the card by `paperpaw-hyphen`'s card-backed reader.
//!
//! The host keeps at most two tables open (a book's language and the UI's),
//! together at most 48 KiB of heap, checked from the table's header before
//! anything is taken. To make room only a table idle for 256 words is closed;
//! else the newer language's words are left whole (`OK`, no bytes). A read
//! that fails closes the table and the next word opens it again; after a
//! second failure, or for a table that is not sound, the language answers
//! `ERR_NOT_FOUND` until the card changes. Nothing is allocated per call
//! beyond a table's own fallible open; a table is untrusted input.
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_hyphen::{CACHE_BLOCKS, CheckError, MAX_MINIMUM, OpenError, Source, StoredPatterns};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UNSUPPORTED, ERR_UTF8, PpBuffer,
    PpBytes, buffers,
};

/// Longest `word` in bytes, and the most characters a word may have to break.
pub(crate) const WORD_MAX: usize = 192;
const WORD_CHARS: usize = 48;
/// At most this many break places are answered (a word of 48 characters).
const BREAKS_MAX: usize = WORD_CHARS - 1;
/// The longest language tag.
const TAG_MAX: usize = 35;
/// Open tables, and the heap they may take together.
const OPEN_MAX: usize = 2;
const HEAP_BUDGET: usize = 48 * 1024;
/// Words a table must have been idle before another language may close it.
const IDLE_WORDS: u64 = 256;

/// One table file, read at a position (a card file).
pub(crate) trait HyphenFile {
    /// The size in bytes now.
    fn size(&self) -> u64;
    /// Fills `buf` from `at`; `false` when it cannot be filled.
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> bool;
}

/// What the shell provides: the registry's tables and the card.
pub(crate) trait HyphenHost {
    /// The table the data plugins declare for exactly `tag` (case-insensitive),
    /// the greatest plugin id's where several name it (D4); `None` for none.
    /// The key is the host's own number for that declaration.
    fn find(&self, tag: &str) -> Option<u64>;
    /// Opens the table file of `key` and tells its identity (what tells the
    /// file from another one). `ERR_IO` for a card failure.
    fn open(&self, key: u64) -> Result<(Box<dyn HyphenFile>, u64), i32>;
    /// A number that changes when the card does (inserted, removed, written).
    fn card_epoch(&self) -> u64;
}

/// What the shell hands the frame for `text.hyphenate`.
pub(crate) struct HyphenServices(pub(crate) Rc<HyphenState>);

struct Card(Box<dyn HyphenFile>);
impl Source for Card {
    type Error = ();
    fn len(&self) -> u64 {
        self.0.size()
    }
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), ()> {
        if self.0.read_at(at, buf) {
            Ok(())
        } else {
            Err(())
        }
    }
}

struct Open {
    /// The resolved tag, lowercase.
    tag: String,
    epoch: u64,
    table: StoredPatterns<Card>,
    last_word: u64,
}

struct Strike {
    tag: String,
    epoch: u64,
    strikes: u8,
}

#[derive(Default)]
struct Tables {
    open: Vec<Open>,
    strikes: Vec<Strike>,
    words: u64,
}

/// The host and its open tables.
pub(crate) struct HyphenState {
    host: Rc<dyn HyphenHost>,
    tables: RefCell<Tables>,
}

impl HyphenState {
    pub(crate) fn new(host: Rc<dyn HyphenHost>) -> Self {
        Self {
            host,
            tables: RefCell::new(Tables::default()),
        }
    }

    /// Heap held by the open tables (for the budget and for tests).
    #[cfg(test)]
    pub(crate) fn open_heap(&self) -> usize {
        self.tables
            .borrow()
            .open
            .iter()
            .map(|o| o.table.heap_bytes())
            .sum()
    }

    /// Open tables (for tests).
    #[cfg(test)]
    pub(crate) fn open_count(&self) -> usize {
        self.tables.borrow().open.len()
    }
}

fn tag_ok(tag: &[u8]) -> bool {
    (1..=TAG_MAX).contains(&tag.len())
        && tag.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

/// `text.hyphenate`: the break places of `word` into `out` (one byte each);
/// the count, or a negative status.
///
/// # Errors
/// `ERR_INVALID` (tag, size, minimums), `ERR_UTF8`, `ERR_NOT_FOUND` (no table
/// for the tag or its shorter forms), `ERR_NO_SPACE` (`out` too small, nothing
/// written), `ERR_BUSY` (re-entered).
pub(crate) fn hyphenate(
    state: &HyphenState,
    language: &[u8],
    word: &[u8],
    left: u32,
    right: u32,
    out: &mut [u8],
) -> Result<usize, i32> {
    if !tag_ok(language) || left > MAX_MINIMUM as u32 || right > MAX_MINIMUM as u32 {
        return Err(ERR_INVALID);
    }
    if word.len() > WORD_MAX {
        return Err(ERR_INVALID);
    }
    let word = core::str::from_utf8(word).map_err(|_| ERR_UTF8)?;
    let tag = core::str::from_utf8(language).map_err(|_| ERR_INVALID)?;
    let mut tables = state.tables.try_borrow_mut().map_err(|_| ERR_BUSY)?;
    let epoch = state.host.card_epoch();
    tables.strikes.retain(|s| s.epoch == epoch);
    let (resolved, key) = resolve(state, &tables, tag).ok_or(ERR_NOT_FOUND)?;
    if word.is_empty() {
        return Ok(0);
    }
    let chars = word.chars().count();
    let (l, r) = (left as usize, right as usize);
    if chars > WORD_CHARS || (l != 0 && r != 0 && chars < l + r) {
        return Ok(0);
    }
    tables.words += 1;
    let now = tables.words;
    let Some(index) = ensure_open(state, &mut tables, resolved, key, epoch, now) else {
        // Cannot be read at this moment: the word is left whole.
        return Ok(0);
    };
    let mut places = [0u8; BREAKS_MAX];
    let mut count = 0;
    {
        let entry = &mut tables.open[index];
        entry.last_word = now;
        entry.table.hyphenate_with_minimums(l, r, word, &mut |at| {
            if count < BREAKS_MAX
                && at > 0
                && at < word.len()
                && let Ok(byte) = u8::try_from(at)
            {
                places[count] = byte;
                count += 1;
            }
        });
        if entry.table.is_failed() {
            // The table is closed; its own name is the strike's.
            let closed = tables.open.remove(index);
            strike(&mut tables, &closed.tag, epoch, false);
            return Ok(0);
        }
    }
    if count > out.len() {
        return Err(ERR_NO_SPACE);
    }
    out[..count].copy_from_slice(&places[..count]);
    Ok(count)
}

/// The tag, then it without its last `-` part, until a table is found that
/// has not failed twice; the form found borrows from `tag` (no allocation).
fn resolve<'t>(state: &HyphenState, tables: &Tables, tag: &'t str) -> Option<(&'t str, u64)> {
    let mut candidate = tag;
    loop {
        if !tables
            .strikes
            .iter()
            .any(|s| s.strikes >= 2 && s.tag.eq_ignore_ascii_case(candidate))
            && let Some(key) = state.host.find(candidate)
        {
            return Some((candidate, key));
        }
        candidate = candidate.rsplit_once('-')?.0;
    }
}

/// Counts a failure of the table of `tag` in this card's epoch (two close
/// the language until the card changes; `unsound` is two at once). The
/// history is kept whether or not the table opens again: it ends only with
/// the epoch. A failure that cannot be written down (no room for its entry)
/// is not counted: the table is tried again, never shut out wrongly.
fn strike(tables: &mut Tables, tag: &str, epoch: u64, unsound: bool) {
    if let Some(s) = tables
        .strikes
        .iter_mut()
        .find(|s| s.tag.eq_ignore_ascii_case(tag))
    {
        s.strikes = if unsound {
            2
        } else {
            s.strikes.saturating_add(1)
        };
        return;
    }
    let mut owned = String::new();
    if owned.try_reserve_exact(tag.len()).is_err() || tables.strikes.try_reserve(1).is_err() {
        return;
    }
    owned.extend(tag.chars().map(|c| c.to_ascii_lowercase()));
    tables.strikes.push(Strike {
        tag: owned,
        epoch,
        strikes: if unsound { 2 } else { 1 },
    });
}

/// The index of the open table of `tag`, opening it within the rules.
fn ensure_open(
    state: &HyphenState,
    tables: &mut Tables,
    tag: &str,
    key: u64,
    epoch: u64,
    now: u64,
) -> Option<usize> {
    // A table opened before the card changed is dropped.
    tables.open.retain(|o| o.epoch == epoch);
    if let Some(index) = tables
        .open
        .iter()
        .position(|o| o.tag.eq_ignore_ascii_case(tag))
    {
        return Some(index);
    }
    let (file, identity) = match state.host.open(key) {
        Ok(opened) => opened,
        Err(_) => {
            strike(tables, tag, epoch, false);
            return None;
        }
    };
    let mut source = Card(file);
    let bound = match StoredPatterns::heap_bound(&mut source, CACHE_BLOCKS) {
        Ok(bound) => bound,
        Err(error) => {
            let unsound = matches!(error, CheckError::Table(_));
            strike(tables, tag, epoch, unsound);
            return None;
        }
    };
    // Room: a free slot and the heap, closing only tables idle long enough.
    loop {
        let used: usize = tables.open.iter().map(|o| o.table.heap_bytes()).sum();
        if tables.open.len() < OPEN_MAX && used.checked_add(bound).is_some_and(|t| t <= HEAP_BUDGET)
        {
            break;
        }
        let idle = tables
            .open
            .iter()
            .enumerate()
            .filter(|(_, o)| now.saturating_sub(o.last_word) >= IDLE_WORDS)
            .max_by_key(|(_, o)| now.saturating_sub(o.last_word))
            .map(|(i, _)| i);
        // Nothing idle long enough: the newer language's words stay whole.
        tables.open.remove(idle?);
    }
    let used: usize = tables.open.iter().map(|o| o.table.heap_bytes()).sum();
    if tables.open.try_reserve(1).is_err() {
        return None;
    }
    let mut owned = String::new();
    owned.try_reserve_exact(tag.len()).ok()?;
    owned.extend(tag.chars().map(|c| c.to_ascii_lowercase()));
    match StoredPatterns::open_within(source, identity, CACHE_BLOCKS, HEAP_BUDGET - used) {
        Ok(table) => {
            // A strike stays: a table that opens and then fails to read,
            // again and again, is shut out at the second.
            tables.open.push(Open {
                tag: owned,
                epoch,
                table,
                last_word: now,
            });
            Some(tables.open.len() - 1)
        }
        Err(OpenError::NoMemory | OpenError::OverBudget { .. }) => None,
        Err(OpenError::Check(error)) => {
            let unsound = matches!(error, CheckError::Table(_));
            strike(tables, &owned, epoch, unsound);
            None
        }
        Err(_) => None,
    }
}

/// Slot 38 as the plugin calls it: the pairs are checked, then the count of
/// break places or a negative status.
///
/// # Safety
/// The pairs are live and disjoint for the call (the generated slot contract).
pub(super) unsafe fn slot(
    resources: &super::resources::OwnerResources,
    language: PpBytes,
    word: PpBytes,
    left: u32,
    right: u32,
    out: PpBuffer,
) -> i32 {
    // SAFETY: as documented; the sizes are the contract's.
    let (language, word, out) = unsafe {
        let language = match buffers::bytes_in(language, TAG_MAX) {
            Ok(bytes) => bytes,
            Err(status) => return status,
        };
        let word = match buffers::bytes_in(word, WORD_MAX) {
            Ok(bytes) => bytes,
            Err(status) => return status,
        };
        let out = match buffers::buffer_out(out, buffers::TEXT_MAX) {
            Ok(bytes) => bytes,
            Err(status) => return status,
        };
        (language, word, out)
    };
    let Some(services) = resources.hosts().hyphen() else {
        return ERR_UNSUPPORTED;
    };
    // The answer goes through a stack array: `out` may be uninitialised.
    let mut places = [0u8; BREAKS_MAX];
    let room = out.len().min(BREAKS_MAX);
    match hyphenate(
        &services.0,
        language,
        word,
        left,
        right,
        &mut places[..room],
    ) {
        Ok(count) => {
            for (slot, byte) in out.iter_mut().zip(&places[..count]) {
                slot.write(*byte);
            }
            i32::try_from(count).unwrap_or(0)
        }
        Err(status) => status,
    }
}

#[cfg(test)]
#[path = "hyphen_service_tests.rs"]
mod tests;
