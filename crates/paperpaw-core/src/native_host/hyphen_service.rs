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
//!
//! Each open table keeps the answers of the last [`WORDS_KEPT`] words asked
//! of it (16 bytes each, 1 KiB a table, made with the table and gone with
//! it): a word asked again (a name fitted in every band of a frame, a list
//! drawn again) reads nothing from the card. Closing the table (another
//! language, a changed card or plugin set, a failure) drops them.
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
/// Words whose answers an open table keeps.
const WORDS_KEPT: usize = 64;

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
    /// The answers of the words asked last; empty where there was no room.
    kept: Vec<Kept>,
    /// The entry the next answer replaces.
    next_kept: usize,
}

/// A word's answer, by the hash of the word and its minimums: the break
/// places as a mask of the characters they come before (a word has at most
/// [`WORD_CHARS`]).
#[derive(Clone, Copy)]
struct Kept {
    hash: u64,
    breaks: u64,
}

impl Kept {
    /// Not used yet: no answer breaks before the first character.
    const EMPTY: Self = Self {
        hash: 0,
        breaks: u64::MAX,
    };
}

/// FNV-1a of the word and its minimums: what a kept answer is found by.
fn word_hash(word: &str, left: usize, right: usize) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let minimums = [left, right].map(|m| u8::try_from(m).unwrap_or(u8::MAX));
    let bytes = word.bytes().chain([0]).chain(minimums);
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

impl Open {
    /// The kept answer for `word` (of `hash`), into `places`; its count.
    fn kept(&self, hash: u64, word: &str, places: &mut [u8; BREAKS_MAX]) -> Option<usize> {
        let kept = self
            .kept
            .iter()
            .find(|k| k.breaks != u64::MAX && k.hash == hash)?;
        let mut count = 0;
        for (i, (at, _)) in word.char_indices().enumerate().take(64) {
            if kept.breaks >> i & 1 == 0 {
                continue;
            }
            let (Some(place), Ok(byte)) = (places.get_mut(count), u8::try_from(at)) else {
                return None;
            };
            *place = byte;
            count += 1;
        }
        Some(count)
    }

    /// Keeps the answer `places` of `word` (of `hash`), in place of the
    /// oldest; not where a place is no character's start.
    fn keep(&mut self, hash: u64, word: &str, places: &[u8]) {
        let Some(slot) = self.kept.get_mut(self.next_kept) else {
            return;
        };
        let mut breaks = 0u64;
        let mut found = 0;
        for (i, (at, _)) in word.char_indices().enumerate().take(64) {
            if places.contains(&u8::try_from(at).unwrap_or(u8::MAX)) {
                breaks |= 1 << i;
                found += 1;
            }
        }
        if found != places.len() {
            return;
        }
        *slot = Kept { hash, breaks };
        self.next_kept = (self.next_kept + 1) % WORDS_KEPT;
    }
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
    /// No room for another table at the last try: the word count before
    /// which none can come (a table gives its room only once idle), and how
    /// many were open then (one closed otherwise makes room at once).
    no_room: Option<(u64, usize)>,
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
    let hash = word_hash(word, l, r);
    let entry = &mut tables.open[index];
    entry.last_word = now;
    if let Some(kept) = entry.kept(hash, word, &mut places) {
        count = kept;
    } else {
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
        entry.keep(hash, word, &places[..count]);
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
    // No room at the last try and none since: the card is not read for each
    // word of the language that waits (#483).
    if tables
        .no_room
        .is_some_and(|(until, open)| now < until && tables.open.len() == open)
    {
        return None;
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
        let Some(idle) = idle else {
            wait_for_room(tables, now);
            return None;
        };
        tables.open.remove(idle);
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
            // The answers kept: none where there is no room for them.
            let mut kept = Vec::new();
            if kept.try_reserve_exact(WORDS_KEPT).is_ok() {
                kept.resize(WORDS_KEPT, Kept::EMPTY);
            }
            tables.open.push(Open {
                tag: owned,
                epoch,
                table,
                last_word: now,
                kept,
                next_kept: 0,
            });
            Some(tables.open.len() - 1)
        }
        // The device's heap may have room at the next word: tried again.
        Err(OpenError::NoMemory) => None,
        Err(OpenError::OverBudget { .. }) => {
            wait_for_room(tables, now);
            None
        }
        Err(OpenError::Check(error)) => {
            let unsound = matches!(error, CheckError::Table(_));
            strike(tables, &owned, epoch, unsound);
            None
        }
        Err(_) => None,
    }
}

/// No room now: the next try is when the open table used longest ago can be
/// idle, or a while from now where none is open.
fn wait_for_room(tables: &mut Tables, now: u64) {
    let oldest = tables.open.iter().map(|o| o.last_word).min().unwrap_or(now);
    tables.no_room = Some((oldest.saturating_add(IDLE_WORDS), tables.open.len()));
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
