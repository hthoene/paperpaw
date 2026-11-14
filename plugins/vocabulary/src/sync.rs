//! Decks from the files on the card: the folder `Flashcards` (settings can
//! name another), a CSV, TSV or TXT file a deck named like it. The design
//! is `paperpaw-notes/paperpaw/designs/flashcards-sd-card.md` (part 1).
//!
//! The reader keeps its own decks with what it learnt of each card; a file
//! is an exchange file, **merged** into the deck it is linked to. Cards
//! match by key, never by line: by the `id` column where the file is the
//! reader's own for this deck (it has the line `#paperpaw:deck 7`, that is
//! this deck: its ids are card numbers) and the deck has that card, else by the front text (trimmed, at most
//! 500 characters, spaces collapsed, case folded, hashed to 64 bits; not
//! Unicode-normalised); of equal fronts the n-th line goes with the n-th
//! card. A line with no card is added, a card
//! whose back or example differs from its line is changed (what was
//! learnt stays), a card whose line is gone stays, unless the deck's switch
//! (`mirror`, in the ledger, off to begin with) says to remove it. A changed front, with
//! no `id`, is a line with no card and a card with no line: the new one
//! starts new.
//!
//! What is linked is kept in `cards/sources.txt` (file, deck, size,
//! modification time, hash of the content at the last merge): after it,
//! the folder is listed (names, sizes and times, no file read) and only
//! files that differ are read again, and a file renamed is found by its
//! hash.
//!
//! The work is done a step at a time ([`Job::step`]), so that a screen
//! shows how far it is and *Back* stops it between two steps: a file of
//! at most [`MAX_FILE`] bytes and [`cards::MAX_CARDS`] lines is hashed and
//! read through to count its cards and problems, [`STEP`] bytes at a time,
//! then merged in slices. Heap stays small however big the deck is: the
//! cards whose key falls in one slice of the key space (about [`SLICE`] of
//! them, at most [`CROWD`]: more is a failure to tell, not a crash) are
//! kept as keys (24 bytes each, in one allocation of at most 55 KiB), the
//! changes and new lines of the slice as 12 bytes each (at most 27 KiB),
//! while the file is read for lines of that slice. The peak is about
//! 85 KiB at worst and 45 KiB for a slice of [`SLICE`]. The deck is written anew beside itself (see
//! `cards::rewrite_with`), so a power cut leaves the old deck or the new.
//! A merge is a function of the file and the deck, so one cut off or done
//! again changes nothing it did not: the ledger is written last.

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{Storage, StorageError};

use crate::apkg;
use crate::cards::{self, MAX_NAME, MAX_TEXT, Problem, Texts};
use crate::files::{part_of, put_in_place};
use crate::lines::{self, copy, push, same_folded, trimmed_cut};
use crate::table::{self, Fault, Item, Layout};

/// The folder files are looked for in, if the setting names none.
pub(crate) const DEFAULT_FOLDER: &str = "Flashcards";
/// Cards of the deck kept as keys at a time (and about as many lines of
/// the file): 24 bytes each.
const SLICE: usize = 1536;
/// More keys, changes or new lines than this in one slice (a deck of equal
/// fronts) are not merged.
const CROWD: usize = SLICE + SLICE / 2;
/// The biggest file read: some 40,000 lines of vocabulary. A bigger one
/// (a dictionary dump) would take minutes of passes.
pub(crate) const MAX_FILE: u64 = 4 * 1024 * 1024;
/// How much of a file one step hashes or counts (streamed, 512 bytes at a time:
/// no buffer of it is held). Small, so that a turn of work stays short.
pub(crate) const STEP: u64 = 16 * 1024;
/// The most the ledger may be: about 100 files at 300 bytes.
const LEDGER_MAX: usize = 128 * 1024;
/// Problems kept to be told.
const MOST_PROBLEMS: usize = 20;
const LEDGER_HEADER: &str = "paperpaw-sources 1";
const EXTENSIONS: [&str; 4] = [".tsv", ".csv", ".txt", ".apkg"];

/// The ledger's file.
const LEDGER: &str = ".paperpaw/plugins/vocabulary/cards/sources.txt";

#[cfg(test)]
pub(crate) fn ledger_path() -> &'static str {
    LEDGER
}

/// Whether `text` ends in `ending` (ASCII), whatever the case.
pub(crate) fn ends_with_ignore_case(text: &str, ending: &str) -> bool {
    text.len() >= ending.len()
        && text
            .as_bytes()
            .get(text.len() - ending.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(ending.as_bytes()))
}

/// The folder as a setting says it: no slashes around it.
pub(crate) fn folder_of(setting: &str) -> Result<String, StorageError> {
    let name = setting
        .trim()
        .trim_matches(|c| c == '/' || c == '\\')
        .trim();
    // Not the plugin's own data (its .txt files are no decks), nor a way up.
    let own = cards::DIR.trim_matches('/');
    let inside = name
        .get(..own.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(own))
        && matches!(name.as_bytes().get(own.len()), None | Some(b'/'));
    if name.is_empty() || inside || name.split(['/', '\\']).any(|part| part == "..") {
        copy(DEFAULT_FOLDER)
    } else {
        copy(name)
    }
}

// ---- The files ---------------------------------------------------------

/// A file in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub name: String,
    pub size: u64,
    pub modified: Option<i64>,
}

/// A file's name without its extension.
fn stem(file: &str) -> &str {
    EXTENSIONS
        .iter()
        .find(|e| ends_with_ignore_case(file, e))
        .and_then(|e| file.get(..file.len() - e.len()))
        .unwrap_or(file)
}

/// The deck a file's name makes: without its extension.
pub(crate) fn deck_name(file: &str) -> Result<String, StorageError> {
    copy(trimmed_cut(stem(file), MAX_NAME))
}

/// The deck of a file in the folder listed as `listed`: its name, but where
/// an earlier file has the same name (`Spanish.csv`, `Spanish.tsv`), the
/// extension is in it too.
pub(crate) fn deck_name_among(file: &str, listed: &[String]) -> Result<String, StorageError> {
    let name = trimmed_cut(stem(file), MAX_NAME);
    let taken = listed
        .iter()
        .take_while(|other| other.as_str() != file)
        .any(|other| same_folded(trimmed_cut(stem(other), MAX_NAME), name));
    if !taken {
        return copy(name);
    }
    let extension = EXTENSIONS
        .iter()
        .find(|e| ends_with_ignore_case(file, e))
        .and_then(|e| e.get(1..))
        .unwrap_or("");
    let mut both = String::new();
    push(&mut both, name)?;
    push(&mut both, " (")?;
    push(&mut both, extension)?;
    push(&mut both, ")")?;
    copy(trimmed_cut(&both, MAX_NAME))
}

/// The files of `folder` that make decks, by name, and whether the folder
/// is there. Files starting with `_` or `.` and folders are no decks. A
/// refused allocation is an error: no file is left out for it.
pub(crate) fn files(
    storage: &dyn Storage,
    folder: &str,
) -> Result<(bool, Vec<Found>), StorageError> {
    let mut found: Vec<Found> = Vec::new();
    let mut refused = false;
    let listed = storage.list(folder, &mut |entry| {
        let name = entry.name;
        if refused
            || entry.is_dir
            || name.starts_with(['_', '.'])
            || !EXTENSIONS.iter().any(|e| ends_with_ignore_case(name, e))
            || found.len() >= 2 * cards::MAX_DECKS
        {
            return;
        }
        let (Ok(name), Ok(())) = (copy(name), lines::reserve(&mut found, 1)) else {
            refused = true;
            return;
        };
        found.push(Found {
            name,
            size: entry.size,
            modified: entry.modified,
        });
    });
    if refused {
        return Err(StorageError::TooLarge);
    }
    paperpaw_sdk::sort::sort_by(&mut found, |a, b| {
        lines::folded(&a.name)
            .cmp(lines::folded(&b.name))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok((listed.is_ok(), found))
}

/// What was merged of a file last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    pub name: String,
    pub deck: u32,
    pub size: u64,
    /// 0 where the card tells no time.
    pub modified: i64,
    /// 0 for a file that was tried and is no deck (see [`Outcome`]).
    pub hash: u64,
    /// The deck's cards that are not in the file any more are removed at
    /// an update (the sixth field of the line; off where it is not there).
    pub mirror: bool,
    /// Lines of the file the last update left out (unreadable or without
    /// room): a save would drop them (the seventh field; 0 where it is not
    /// there).
    pub left_out: usize,
}

impl Source {
    /// Whether the file was tried and failed as it is.
    pub(crate) fn failed(&self) -> bool {
        self.hash == 0
    }
}

/// What the ledger holds: the files linked, and the lines it did not
/// understand, which are written back as they are.
#[derive(Debug, Clone, Default)]
pub(crate) struct Ledger {
    pub sources: Vec<Source>,
    other: Vec<String>,
}

impl Ledger {
    fn find(&self, name: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.name == name)
    }
}

/// One line of the ledger as a source: `None` where it is none.
fn parse_source(line: &str) -> Result<Option<Source>, StorageError> {
    let mut f = line.split('\t');
    let parsed = (|| {
        Some((
            f.next()?,
            f.next()?.parse().ok()?,
            f.next()?.parse().ok()?,
            f.next()?.parse().ok()?,
            u64::from_str_radix(f.next()?, 16).ok()?,
            f.next() == Some("1"),
            f.next().and_then(|n| n.parse().ok()).unwrap_or(0),
        ))
    })();
    let Some((name, deck, size, modified, hash, mirror, left_out)) = parsed else {
        return Ok(None);
    };
    if name.is_empty() {
        return Ok(None);
    }
    Ok(Some(Source {
        name: copy(name)?,
        deck,
        size,
        modified,
        hash,
        mirror,
        left_out,
    }))
}

/// The ledger: none is no sources. One that cannot be read (a card
/// error, a file too big, one of another version, a refused allocation) is
/// an error, so that no link is forgotten for it.
pub(crate) fn sources(storage: &dyn Storage) -> Result<Ledger, StorageError> {
    let mut ledger = Ledger::default();
    if !storage.exists(LEDGER) {
        return Ok(ledger);
    }
    // Too big to be the ledger: unreadable, like one of another version (a
    // refused allocation is `TooLarge`, told apart).
    if storage.size(LEDGER)? > LEDGER_MAX as u64 {
        return Err(StorageError::Io);
    }
    let mut first = true;
    let mut failure = None;
    lines::each_line(storage, LEDGER, |_, line| {
        let step = (|| -> Result<(), StorageError> {
            if core::mem::take(&mut first) {
                return if line == LEDGER_HEADER {
                    Ok(())
                } else {
                    Err(StorageError::Io)
                };
            }
            match parse_source(line)? {
                Some(source) => {
                    lines::reserve(&mut ledger.sources, 1)?;
                    ledger.sources.push(source);
                }
                None if line.trim().is_empty() => {}
                None => {
                    lines::reserve(&mut ledger.other, 1)?;
                    ledger.other.push(copy(line)?);
                }
            }
            Ok(())
        })();
        match step {
            Ok(()) => ControlFlow::Continue(()),
            Err(e) => {
                failure = Some(e);
                ControlFlow::Break(())
            }
        }
    })?;
    match failure {
        Some(e) => Err(e),
        None => Ok(ledger),
    }
}

pub(crate) fn write_sources(storage: &dyn Storage, ledger: &Ledger) -> Result<(), StorageError> {
    let part = part_of(LEDGER)?;
    {
        let mut sink = lines::Sink::new(storage, &part)?;
        sink.put(LEDGER_HEADER)?;
        sink.put("\n")?;
        for s in &ledger.sources {
            // A tab or a line break in a name cannot be (the card's names
            // have none that matter): written as spaces.
            let mut buf = [0u8; 4];
            for c in s.name.chars() {
                sink.put(if c.is_control() {
                    " "
                } else {
                    c.encode_utf8(&mut buf)
                })?;
            }
            sink.put("\t")?;
            sink.put_number(s.deck)?;
            sink.put("\t")?;
            sink.put_number(s.size)?;
            sink.put("\t")?;
            sink.put_signed(s.modified)?;
            sink.put("\t")?;
            sink.put_hex(s.hash)?;
            sink.put("\t")?;
            sink.put(if s.mirror { "1" } else { "0" })?;
            sink.put("\t")?;
            sink.put_number(s.left_out as u64)?;
            sink.put("\n")?;
        }
        for line in &ledger.other {
            sink.put(line)?;
            sink.put("\n")?;
        }
        sink.finish()?;
    }
    // The folder may be missing (a cut before it was made).
    storage.create_dir(cards::DIR).ok();
    put_in_place(storage, &part, LEDGER)
}

/// The file `deck` is linked to, if one is.
pub(crate) fn link_of(ledger: &Ledger, deck: u32) -> Option<&Source> {
    ledger
        .sources
        .iter()
        .find(|s| s.deck == deck && s.deck != 0)
}

/// Sets the switch of the deck's link: whether the cards not in its file
/// any more are removed at an update. `false` where the deck has no link or
/// the ledger cannot be read.
pub(crate) fn set_mirror(storage: &dyn Storage, deck: u32, on: bool) -> bool {
    let Ok(mut ledger) = sources(storage) else {
        return false;
    };
    let Some(source) = ledger
        .sources
        .iter_mut()
        .find(|s| s.deck == deck && s.deck != 0)
    else {
        return false;
    };
    source.mirror = on;
    write_sources(storage, &ledger).is_ok()
}

/// Whether `file` is as the ledger last saw it (size and time).
fn unchanged(found: &Found, source: &Source) -> bool {
    found.size == source.size && found.modified.unwrap_or(0) == source.modified
}

/// What a look at the folder shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Pending {
    pub folder: bool,
    /// Files that make decks.
    pub files: usize,
    /// Of them, new or changed since they were merged.
    pub todo: usize,
    /// Of the others, those that were tried and are not decks (no cards,
    /// too big, too many equal fronts): told, not tried again until they
    /// change.
    pub failed: usize,
}

/// One listing of the folder against the ledger: no file is read.
pub(crate) fn pending(storage: &dyn Storage, folder: &str) -> Result<Pending, StorageError> {
    let (folder_there, found) = files(storage, folder)?;
    // A ledger that cannot be read is told as no sources (a refused
    // allocation is an error for the whole look).
    let known = match sources(storage) {
        Err(StorageError::TooLarge) => return Err(StorageError::TooLarge),
        other => other.unwrap_or_default(),
    };
    let (mut todo, mut failed) = (0, 0);
    for f in &found {
        match known.find(&f.name) {
            Some(s) if unchanged(f, s) => failed += usize::from(s.failed()),
            _ => todo += 1,
        }
    }
    Ok(Pending {
        folder: folder_there,
        files: found.len(),
        todo,
        failed,
    })
}

/// Makes the folder, and a note in it of how files are to be made.
pub(crate) fn make_folder(
    storage: &dyn Storage,
    folder: &str,
    readme: &str,
) -> Result<(), StorageError> {
    storage.create_dir(folder).ok();
    let mut note = String::new();
    push(&mut note, folder)?;
    push(&mut note, "/_Read me.txt")?;
    if !storage.exists(&note) {
        storage.write(&note, readme.as_bytes())?;
    }
    Ok(())
}

// ---- Keys --------------------------------------------------------------

pub(crate) const FNV64: u64 = 0xcbf2_9ce4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

pub(crate) fn feed(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(FNV64_PRIME)
}

/// What a front is matched by: its text trimmed, spaces collapsed, case
/// folded, then hashed (64 bits: with 20,000 cards a collision is not to
/// be feared, with 32 it would be).
pub(crate) fn front_key(front: &str) -> u64 {
    let (mut hash, mut space, mut any) = (FNV64, false, false);
    for c in front.chars() {
        if c.is_whitespace() {
            space = any;
            continue;
        }
        if space {
            hash = feed(hash, b' ');
            space = false;
        }
        any = true;
        for lower in c.to_lowercase() {
            let mut buf = [0u8; 4];
            for &b in lower.encode_utf8(&mut buf).as_bytes() {
                hash = feed(hash, b);
            }
        }
    }
    hash
}

/// What the texts of a card amount to, for telling whether a line differs
/// from its card: of the cleaned texts, as they are kept.
fn texts_hash(parts: &[&str]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for part in parts {
        for byte in trimmed_cut(part, MAX_TEXT).bytes().chain([0]) {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }
    hash
}

/// Bits, one for each card or line.
#[derive(Debug, Default)]
struct Bits(Vec<u8>);

impl Bits {
    fn new(count: usize) -> Result<Self, StorageError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(count.div_ceil(8))
            .map_err(|_| StorageError::TooLarge)?;
        bytes.resize(count.div_ceil(8), 0);
        Ok(Self(bytes))
    }

    fn get(&self, at: usize) -> bool {
        self.0.get(at / 8).is_some_and(|b| (b >> (at % 8)) & 1 == 1)
    }

    fn set(&mut self, at: usize) {
        if let Some(b) = self.0.get_mut(at / 8) {
            *b |= 1 << (at % 8);
        }
    }
}

// ---- The job -----------------------------------------------------------

/// What came of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Merged,
    /// Its content is what was merged before.
    Same,
    /// It could not be read (or the deck written).
    Unreadable,
    /// 50 decks already.
    TooManyDecks,
    /// More than a few thousand cards share a front: not merged.
    Crowded,
    /// Not one card in it, and lines that are none: no deck is made.
    NoCards,
    /// More than [`MAX_FILE`] bytes or [`cards::MAX_CARDS`] lines.
    TooLarge,
    /// A UTF-16 file (a spreadsheet's "Unicode text").
    Utf16,
    /// An Anki deck (`.apkg`) with no notes, or no deck of Anki's.
    NoNotes,
    /// An Anki deck of the newest Anki, exported without its older format.
    NewerAnki,
    /// An Anki deck with more files, a larger database or more notes than
    /// are read ([`apkg`]).
    DeckTooLarge,
}

impl Outcome {
    /// Whether the file itself is to blame: it is not tried again as it is.
    fn is_the_file(self) -> bool {
        matches!(
            self,
            Outcome::NoCards
                | Outcome::Crowded
                | Outcome::TooLarge
                | Outcome::Utf16
                | Outcome::NoNotes
                | Outcome::NewerAnki
                | Outcome::DeckTooLarge
        )
    }
}

/// What was done with a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileReport {
    pub file: String,
    pub deck: String,
    pub outcome: Outcome,
    pub added: usize,
    pub changed: usize,
    /// Cards removed, where the deck's link says to.
    pub removed: usize,
    /// The removal of cards was asked for and not done: lines were left out
    /// (a card whose line could not be read is no card to remove).
    pub removal_held: bool,
    pub same: usize,
    /// Lines left out (see `problems` for the first).
    pub skipped: usize,
    /// Lines that did not fit in a deck of 20,000 cards.
    pub no_room: usize,
    /// The first lines left out, and why.
    pub problems: Vec<(u32, Fault)>,
}

impl FileReport {
    /// A report of nothing, made without allocating.
    const fn empty() -> Self {
        Self {
            file: String::new(),
            deck: String::new(),
            outcome: Outcome::Merged,
            added: 0,
            changed: 0,
            removed: 0,
            removal_held: false,
            same: 0,
            skipped: 0,
            no_room: 0,
            problems: Vec::new(),
        }
    }

    fn new(file: &str, outcome: Outcome) -> Result<Self, StorageError> {
        Ok(Self {
            file: copy(file)?,
            deck: deck_name(file)?,
            outcome,
            added: 0,
            changed: 0,
            removed: 0,
            removal_held: false,
            same: 0,
            skipped: 0,
            no_room: 0,
            problems: Vec::new(),
        })
    }
}

/// A question the job waits for the answer to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Question {
    pub file: String,
    pub deck: String,
}

/// What the job did with a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    Working,
    /// A question is waiting ([`Job::question`]) for [`Job::answer`].
    Ask,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// An Anki deck's (`.apkg`) database inflated, in one step.
    Convert,
    /// Its notes written as a text file, in one step.
    Notes,
    /// The file hashed, a step at a time.
    Hash,
    /// Its lines counted, a step at a time.
    Count,
    Decide,
    Asked,
    Merge,
    /// The key of a round (1 the card's number, 2 the front) and the slice.
    Slice {
        round: u8,
        slice: usize,
    },
    /// The cards no line had removed (a deck whose link says so).
    Remove,
    Finish,
}

#[derive(Debug)]
struct Work {
    found: Found,
    path: String,
    /// The size of the text that is read: the file's, or what an Anki
    /// deck was written as.
    text_size: u64,
    layout: Layout,
    stage: Stage,
    deck: u32,
    hash: u64,
    /// How much of the file is hashed, and where the count goes on.
    hashed: u64,
    resume: table::Resume,
    /// Lines that are cards, and whether any has an `id`.
    lines: usize,
    with_ids: bool,
    /// The deck's link says to remove the cards the file lacks.
    mirror: bool,
    /// Cards the deck has.
    cards: usize,
    slices: usize,
    /// Lines and cards matched by an earlier slice.
    matched_lines: Bits,
    matched_cards: Bits,
    report: FileReport,
}

/// A line of the slice to be written: a change to the card with the number
/// `id`, or (`add`) a new card.
#[derive(Debug, Clone, Copy)]
struct Edit {
    id: u32,
    /// Where the line begins in the file (a file is at most [`MAX_FILE`]).
    offset: u32,
    add: bool,
}

/// Which files a job takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The new and changed (the flash cards opening by themselves).
    Auto,
    /// Those and the ones that failed as they are.
    Manual,
    /// Every file, its content compared.
    All,
}

/// A card of the deck, kept as its key while a slice is merged.
#[derive(Debug, Clone, Copy)]
struct Entry {
    key: u64,
    id: u32,
    order: u32,
    text: u32,
}

/// The files to merge, one step at a time.
#[derive(Debug)]
pub(crate) struct Job {
    folder: String,
    todo: Vec<Found>,
    /// Every file of the folder, for finding a file renamed.
    listed: Vec<String>,
    at: usize,
    work: Option<Work>,
    question: Option<Question>,
    pub(crate) reports: Vec<FileReport>,
    /// A refused allocation ended the job before its last file or dropped
    /// a report.
    refused: bool,
}

impl Job {
    /// The job for the files of `folder` that `mode` takes. A refused
    /// allocation is an error: no job is made of part of the files.
    pub(crate) fn new(
        storage: &dyn Storage,
        folder: &str,
        mode: Mode,
    ) -> Result<Self, StorageError> {
        // What a deck left in the plugin's folder when the power went.
        apkg::tidy(storage);
        let (_, found) = files(storage, folder)?;
        // A ledger that cannot be read: every file is taken, and fails,
        // so that nothing is written over it.
        let known = sources(storage);
        if matches!(known, Err(StorageError::TooLarge)) {
            return Err(StorageError::TooLarge);
        }
        let mut listed: Vec<String> = Vec::new();
        lines::reserve(&mut listed, found.len())?;
        for f in &found {
            listed.push(copy(&f.name)?);
        }
        let mut todo: Vec<Found> = Vec::new();
        lines::reserve(&mut todo, found.len())?;
        for f in found {
            let take = match (&known, mode) {
                (Err(_), _) | (_, Mode::All) => true,
                (Ok(known), _) => known
                    .find(&f.name)
                    .is_none_or(|s| !unchanged(&f, s) || (s.failed() && mode == Mode::Manual)),
            };
            if take {
                todo.push(f);
            }
        }
        Ok(Self {
            folder: copy(folder)?,
            todo,
            listed,
            at: 0,
            work: None,
            question: None,
            reports: Vec::new(),
            refused: false,
        })
    }

    /// The job for one file of `folder`, if it is new or changed.
    pub(crate) fn for_file(
        storage: &dyn Storage,
        folder: &str,
        file: &str,
    ) -> Result<Self, StorageError> {
        let mut job = Self::new(storage, folder, Mode::Manual)?;
        job.todo.retain(|f| f.name == file);
        Ok(job)
    }

    /// Whether a refused allocation cut the job short or dropped a report.
    #[cfg_attr(not(test), allow(dead_code, reason = "the shell tells it"))]
    pub(crate) fn refused(&self) -> bool {
        self.refused
    }

    /// The question the job waits for the answer to.
    pub(crate) fn question(&self) -> Option<&Question> {
        self.question.as_ref()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.todo.is_empty()
    }

    /// How far it is, in thousandths, and the file it is at.
    pub(crate) fn progress(&self) -> (u16, &str) {
        let total = self.todo.len().max(1);
        let name = self.todo.get(self.at).map_or("", |f| f.name.as_str());
        let within = self.work.as_ref().map_or(0, |w| match w.stage {
            Stage::Convert => 0,
            Stage::Notes => 10,
            Stage::Hash => (50 * w.hashed / w.text_size.max(1)).min(50) as usize,
            Stage::Count => (50 + 50 * w.resume.at / w.text_size.max(1)).min(100) as usize,
            Stage::Decide | Stage::Asked => 100,
            Stage::Merge => 100,
            Stage::Slice { round, slice } => {
                let rounds = if w.with_ids { 2 } else { 1 };
                let before = if w.with_ids {
                    usize::from(round) - 1
                } else {
                    0
                };
                100 + 800 * (before * w.slices + slice) / (rounds * w.slices).max(1)
            }
            Stage::Remove | Stage::Finish => 900,
        });
        let permille = (self.at * 1000 + within) / total;
        (permille.min(1000) as u16, name)
    }

    /// The answer to the question asked: `true` to merge into the deck
    /// that has the file's name, `false` to make a new deck.
    pub(crate) fn answer(&mut self, storage: &dyn Storage, link: bool) {
        self.question = None;
        let Some(work) = self.work.as_ref() else {
            return;
        };
        let outcome = match deck_for_answer(storage, &work.found.name, &self.listed, link) {
            Ok(Ok(deck)) => {
                if let Some(work) = self.work.as_mut() {
                    work.deck = deck;
                    work.stage = Stage::Merge;
                }
                return;
            }
            Ok(Err(_)) => Outcome::TooManyDecks,
            Err(_) => Outcome::Unreadable,
        };
        self.finish_with(storage, outcome);
    }

    fn finish_with(&mut self, storage: &dyn Storage, outcome: Outcome) {
        if let Some(mut work) = self.work.take() {
            if apkg::is_apkg(&work.found.name) {
                apkg::tidy(storage);
            }
            if outcome.is_the_file() {
                note_failed(storage, &work.found);
            }
            work.report.outcome = outcome;
            self.keep(work.report);
        }
        self.at += 1;
    }

    /// A report kept for telling; dropped (and `refused` set) where the
    /// heap refuses room for it.
    fn keep(&mut self, report: FileReport) {
        if lines::reserve(&mut self.reports, 1).is_ok() {
            self.reports.push(report);
        } else {
            self.refused = true;
        }
    }

    /// The work for the file the job is at.
    fn begin(&self, found: &Found) -> Result<Work, StorageError> {
        let found = Found {
            name: copy(&found.name)?,
            size: found.size,
            modified: found.modified,
        };
        let mut path = String::new();
        push(&mut path, &self.folder)?;
        push(&mut path, "/")?;
        push(&mut path, &found.name)?;
        let deck = apkg::is_apkg(&found.name);
        Ok(Work {
            report: FileReport::new(&found.name, Outcome::Merged)?,
            text_size: found.size,
            found,
            path,
            layout: Layout {
                separator: b'\t',
                columns: table::Columns {
                    front: 0,
                    back: None,
                    example: None,
                    id: None,
                },
                html: false,
                paperpaw: None,
                utf16: false,
                latin1: false,
                start: 0,
                line: 1,
            },
            stage: if deck { Stage::Convert } else { Stage::Hash },
            deck: 0,
            hash: FNV64,
            hashed: 0,
            resume: table::Resume { at: 0, line: 0 },
            lines: 0,
            with_ids: false,
            mirror: false,
            cards: 0,
            slices: 1,
            matched_lines: Bits::default(),
            matched_cards: Bits::default(),
        })
    }

    /// Does one step: the next piece of the work.
    pub(crate) fn step(&mut self, storage: &dyn Storage) -> Step {
        if self.question.is_some() {
            return Step::Ask;
        }
        if self.work.is_none() {
            let Some(found) = self.todo.get(self.at) else {
                return Step::Done;
            };
            match self.begin(found) {
                Ok(work) => self.work = Some(work),
                Err(_) => {
                    // No room even to begin the file: the job ends here, and
                    // says so, instead of skipping files one by one.
                    self.refused = true;
                    self.at = self.todo.len();
                    return Step::Done;
                }
            }
            return Step::Working;
        }
        let stage = self.work.as_ref().map_or(Stage::Finish, |w| w.stage);
        let done = match stage {
            Stage::Convert => self.convert(storage),
            Stage::Notes => self.notes(storage),
            Stage::Hash => self.hash(storage),
            Stage::Count => self.count(storage),
            Stage::Decide => self.decide(storage),
            // Waiting for the answer (asked above).
            Stage::Asked => Ok(()),
            Stage::Merge => self.begin_merge(storage),
            Stage::Slice { round, slice } => self.slice(storage, round, slice),
            Stage::Remove => self.remove(storage),
            Stage::Finish => self.finish(storage),
        };
        match done {
            Ok(()) => {}
            Err(Failure::Outcome(outcome)) => self.finish_with(storage, outcome),
            Err(Failure::Storage) => self.finish_with(storage, Outcome::Unreadable),
        }
        if self.question.is_some() {
            Step::Ask
        } else {
            Step::Working
        }
    }

    /// An Anki deck's database inflated to the plugin's folder (see
    /// [`apkg`]): a step of its own, as long as the card is slow, and
    /// then the notes in the next. A step ends well within the X4's
    /// watchdog because the database is at most [`apkg::MAX_DB`].
    fn convert(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        apkg::inflate(storage, &work.path).map_err(fault_of)?;
        work.stage = Stage::Notes;
        Ok(())
    }

    /// The notes of the inflated deck written as text, read page by page,
    /// so the heap needs no more than for a text file; the text is then
    /// merged as a file is, from its own place.
    fn notes(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        let notes = apkg::notes(storage).map_err(fault_of)?;
        // Notes left out as too long count as lines left out: the cards
        // they may have made are not removed for lacking.
        work.report.skipped += notes.skipped;
        work.path = copy(apkg::text_path())?;
        work.text_size = storage.size(&work.path)?;
        work.stage = Stage::Hash;
        Ok(())
    }

    /// The file hashed, [`STEP`] bytes at a time; at its end, a file with
    /// the content merged before is done, else its layout is looked at.
    fn hash(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        let too_large = Failure::Outcome(Outcome::TooLarge);
        if work.text_size > MAX_FILE {
            return Err(too_large);
        }
        let end = work.hashed + STEP;
        let mut chunk = [0u8; 512];
        while work.hashed < end {
            let n = storage.read_at(&work.path, work.hashed, &mut chunk)?;
            if n == 0 {
                break;
            }
            work.hash = chunk[..n].iter().fold(work.hash, |h, &b| feed(h, b));
            work.hashed += n as u64;
            if work.hashed > MAX_FILE {
                return Err(too_large);
            }
            if work.hashed >= end {
                return Ok(());
            }
        }
        // Content as it was merged: nothing to do (the time may have
        // changed, a copy to the card does it).
        let known = sources(storage)?;
        if let Some(source) = known.find(&work.found.name)
            && source.hash == work.hash
            && cards::decks(storage)?.iter().any(|d| d.id == source.deck)
        {
            let mut kept = known;
            for s in &mut kept.sources {
                if s.name == work.found.name {
                    s.size = work.found.size;
                    s.modified = work.found.modified.unwrap_or(0);
                }
            }
            write_sources(storage, &kept)?;
            return Err(Failure::Outcome(Outcome::Same));
        }
        work.layout = table::detect(storage, &work.path)?;
        if work.layout.utf16 {
            return Err(Failure::Outcome(Outcome::Utf16));
        }
        work.resume = table::Resume {
            at: work.layout.start,
            line: work.layout.line,
        };
        work.stage = Stage::Count;
        Ok(())
    }

    /// The lines counted, [`STEP`] bytes at a time: the cards, and what is
    /// wrong.
    fn count(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        let layout = work.layout.clone();
        let (mut lines, mut with_ids, mut too_many) = (work.lines, work.with_ids, false);
        let mut report = core::mem::replace(&mut work.report, FileReport::empty());
        let mut refused = false;
        let next = table::scan(
            storage,
            &work.path,
            &layout,
            work.resume,
            Some(STEP),
            |item| {
                match item {
                    Item::Record(record) => {
                        lines += 1;
                        with_ids |= record.id.is_some();
                        if lines > cards::MAX_CARDS {
                            too_many = true;
                            return ControlFlow::Break(());
                        }
                    }
                    Item::Skipped { line, fault } => {
                        report.skipped += 1;
                        if report.problems.len() < MOST_PROBLEMS {
                            if lines::reserve(&mut report.problems, 1).is_err() {
                                refused = true;
                                return ControlFlow::Break(());
                            }
                            report.problems.push((line, fault));
                        }
                    }
                }
                ControlFlow::Continue(())
            },
        );
        work.report = report;
        let next = next?;
        if refused {
            return Err(Failure::Storage);
        }
        if too_many {
            return Err(Failure::Outcome(Outcome::TooLarge));
        }
        work.lines = lines;
        work.with_ids = with_ids;
        if let Some(next) = next {
            work.resume = next;
            return Ok(());
        }
        work.with_ids = with_ids && layout.columns.id.is_some();
        if lines == 0 && work.report.skipped > 0 {
            return Err(Failure::Outcome(Outcome::NoCards));
        }
        work.stage = Stage::Decide;
        Ok(())
    }

    /// Which deck the file is for: the one it is linked to, the one of a
    /// file it was renamed from, one of its name that is asked about, or a
    /// new one.
    fn decide(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        let decks = cards::decks(storage)?;
        let mut known = sources(storage)?;
        let exists = |id: u32| decks.iter().any(|d| d.id == id);
        let name = deck_name_among(&work.found.name, &self.listed)?;
        if let Some(source) = known.find(&work.found.name).filter(|s| exists(s.deck)) {
            work.deck = source.deck;
            work.mirror = source.mirror;
            work.stage = Stage::Merge;
            return Ok(());
        }
        // A file that was there and is not now, with this content: this
        // one, renamed. Its deck follows the name where it had the old one.
        let renamed = known.sources.iter().position(|s| {
            !s.failed() && s.hash == work.hash && !self.listed.contains(&s.name) && exists(s.deck)
        });
        if let Some(at) = renamed {
            let (deck, mirror) = known
                .sources
                .get(at)
                .map(|s| (s.deck, s.mirror))
                .ok_or(Failure::Storage)?;
            let old = known.sources.get(at).map(|s| deck_name(&s.name));
            if let Some(old) = old {
                let old = old?;
                let current = decks.iter().find(|d| d.id == deck);
                if current.is_some_and(|d| same_folded(&d.name, &old)) {
                    // A name that cannot be set stays (the deck is the same).
                    let _ = cards::rename_deck(storage, deck, &name);
                }
            }
            work.deck = deck;
            work.mirror = mirror;
            known.sources.remove(at);
            write_sources(storage, &known)?;
            work.stage = Stage::Merge;
            return Ok(());
        }
        // A deck of that name no file is linked to: asked, never taken.
        let same_name = decks
            .iter()
            .any(|d| same_folded(&d.name, &name) && !known.sources.iter().any(|s| s.deck == d.id));
        if same_name {
            self.question = Some(Question {
                file: copy(&work.found.name)?,
                deck: name,
            });
            work.stage = Stage::Asked;
            return Ok(());
        }
        match cards::add_deck(storage, &name) {
            Ok(deck) => {
                work.deck = deck;
                work.stage = Stage::Merge;
                Ok(())
            }
            Err(Problem::Full) => Err(Failure::Outcome(Outcome::TooManyDecks)),
            Err(_) => Err(Failure::Storage),
        }
    }

    /// An empty deck takes the lines as they come; else the slices are
    /// counted and merged one at a time.
    fn begin_merge(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        // The ids are the cards' numbers only in a file saved from this deck.
        work.with_ids &= work.layout.paperpaw == Some(work.deck);
        cards::fold(storage)?;
        let mut count = 0usize;
        cards::each_card(storage, work.deck, |_, _| {
            count += 1;
            ControlFlow::Continue(())
        })?;
        work.cards = count;
        if count == 0 {
            // A deck of no cards: all of the file's, in its order.
            let (path, layout) = (&work.path, &work.layout);
            let mut no_room = 0usize;
            let added = cards::add_each(storage, work.deck, |push| {
                table::each(storage, path, layout, None, |item| {
                    let Item::Record(record) = item else {
                        return ControlFlow::Continue(());
                    };
                    let texts = Texts {
                        front: record.front,
                        back: record.back,
                        example: record.example,
                    };
                    if push(texts) {
                        ControlFlow::Continue(())
                    } else {
                        no_room += 1;
                        ControlFlow::Break(())
                    }
                })
            })?;
            work.report.added = added;
            work.report.no_room = work.lines.saturating_sub(added);
            work.stage = Stage::Finish;
            return Ok(());
        }
        work.slices = work.lines.max(count).div_ceil(SLICE).max(1);
        work.matched_lines = Bits::new(work.lines)?;
        work.matched_cards = Bits::new(count)?;
        work.stage = Stage::Slice {
            round: if work.with_ids { 1 } else { 2 },
            slice: 0,
        };
        Ok(())
    }

    /// One slice of one round: the cards of the deck and the lines of the
    /// file whose key falls in it matched, the changes and the new cards
    /// written.
    fn slice(&mut self, storage: &dyn Storage, round: u8, slice: usize) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        let slices = work.slices;
        let in_slice = |key: u64| (key % slices as u64) as usize == slice;
        let by_front = round == 2;
        let entries = deck_entries(storage, work, by_front, &in_slice)?;
        let (edits, same) = match_lines(storage, work, entries, by_front, &in_slice)?;
        work.report.same += same;
        if !edits.is_empty() {
            write_slice(storage, work, by_front, edits)?;
        }
        // The next slice, the next round, or the end.
        work.stage = match (round, slice + 1 < slices) {
            (_, true) => Stage::Slice {
                round,
                slice: slice + 1,
            },
            (1, false) => Stage::Slice { round: 2, slice: 0 },
            // Never from a file with no card in it: an empty file is more
            // likely a mistake than an order to empty the deck.
            _ if work.mirror && work.lines > 0 && work.report.skipped == 0 => Stage::Remove,
            _ => {
                // A line left out may be a card that was learnt: nothing
                // is removed until every line of the file is read.
                work.report.removal_held = work.mirror && work.report.skipped > 0;
                Stage::Finish
            }
        };
        Ok(())
    }

    /// The cards of the deck that no line of the file had removed: the
    /// deck is written anew without them. Cards added by this update lie
    /// after the ones counted at its start and stay.
    fn remove(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        let (mut index, mut removed) = (0usize, 0usize);
        cards::rewrite(storage, work.deck, |_| {
            let this = index;
            index += 1;
            let vanished = this < work.cards && !work.matched_cards.get(this);
            removed += usize::from(vanished);
            !vanished
        })?;
        work.report.removed = removed;
        work.stage = Stage::Finish;
        Ok(())
    }

    /// The ledger told, the file done.
    fn finish(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.take() else {
            return Ok(());
        };
        if apkg::is_apkg(&work.found.name) {
            apkg::tidy(storage);
        }
        let written = sources(storage).and_then(|mut known| {
            known.sources.retain(|s| s.name != work.found.name);
            lines::reserve(&mut known.sources, 1)?;
            known.sources.push(Source {
                name: copy(&work.found.name)?,
                deck: work.deck,
                size: work.found.size,
                modified: work.found.modified.unwrap_or(0),
                hash: work.hash,
                mirror: work.mirror,
                left_out: work.report.skipped + work.report.no_room,
            });
            tidy(storage, &mut known, |name| {
                self.listed.iter().any(|l| l == name)
            });
            write_sources(storage, &known)
        });
        let mut report = work.report;
        if written.is_err() {
            report.outcome = Outcome::Unreadable;
        }
        if let Ok(decks) = cards::decks(storage)
            && let Some(deck) = decks.iter().find(|d| d.id == work.deck)
        {
            // The name as the deck has it; the file's stays where the heap refuses.
            if let Ok(name) = copy(&deck.name) {
                report.deck = name;
            }
        }
        self.keep(report);
        self.at += 1;
        Ok(())
    }
}

mod merge;
pub(crate) use self::merge::*;

#[cfg(test)]
#[path = "sync_tests.rs"]
pub(crate) mod tests;
