//! Decks from the files on the card: the folder `Flashcards` (settings can
//! name another), a CSV, TSV or TXT file a deck named like it. The design
//! is `paperpaw-notes/paperpaw/designs/flashcards-sd-card.md` (part 1).
//!
//! The reader keeps its own decks with what it learnt of each card; a file
//! is an exchange file, **merged** into the deck it is linked to. Cards
//! match by key, never by line: by the `id` column where the file is the
//! reader's own (it has the line `#paperpaw:`, so its ids are card numbers)
//! and the deck has that card, else by the front text (trimmed, at most
//! 500 characters, spaces collapsed, case folded, hashed to 64 bits; not
//! Unicode-normalised); of equal fronts the n-th line goes with the n-th
//! card. A line with no card is added, a card
//! whose back or example differs from its line is changed (what was
//! learnt stays), a card whose line is gone stays. A changed front, with
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

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{Storage, StorageError};

use crate::cards::{self, MAX_NAME, MAX_TEXT, Problem, Texts, clean};
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
/// How much of a file one step hashes or counts.
const STEP: u64 = 64 * 1024;
/// The most the ledger may be: about 100 files at 300 bytes.
const LEDGER_MAX: usize = 128 * 1024;
/// Problems kept to be told.
const MOST_PROBLEMS: usize = 20;
const LEDGER_HEADER: &str = "paperpaw-sources 1";
const EXTENSIONS: [&str; 3] = [".tsv", ".csv", ".txt"];

fn ledger_path() -> String {
    cards::path("sources.txt")
}

/// The folder as a setting says it: no slashes around it.
pub(crate) fn folder_of(setting: &str) -> String {
    let name = setting
        .trim()
        .trim_matches(|c| c == '/' || c == '\\')
        .trim();
    // Not the plugin's own data (its .txt files are no decks), nor a way up.
    let own = cards::path("").to_lowercase();
    let own = own.trim_matches('/');
    let lower = name.to_lowercase();
    let inside = lower == own || lower.starts_with(&alloc::format!("{own}/"));
    if name.is_empty() || inside || name.split(['/', '\\']).any(|part| part == "..") {
        String::from(DEFAULT_FOLDER)
    } else {
        String::from(name)
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

/// The deck a file's name makes: without its extension.
pub(crate) fn deck_name(file: &str) -> String {
    let stem = EXTENSIONS
        .iter()
        .find(|e| file.to_lowercase().ends_with(*e))
        .map_or(file, |e| &file[..file.len() - e.len()]);
    clean(stem, MAX_NAME)
}

/// The deck of a file in the folder listed as `listed`: its name, but where
/// an earlier file has the same name (`Spanish.csv`, `Spanish.tsv`), the
/// extension is in it too.
fn deck_name_among(file: &str, listed: &[String]) -> String {
    let name = deck_name(file);
    let lower = name.to_lowercase();
    let taken = listed
        .iter()
        .take_while(|other| other.as_str() != file)
        .any(|other| deck_name(other).to_lowercase() == lower);
    if !taken {
        return name;
    }
    let extension = EXTENSIONS
        .iter()
        .find(|e| file.to_lowercase().ends_with(*e))
        .map_or("", |e| &e[1..]);
    clean(&alloc::format!("{name} ({extension})"), MAX_NAME)
}

/// The files of `folder` that make decks, by name, and whether the folder
/// is there. Files starting with `_` or `.` and folders are no decks.
pub(crate) fn files(storage: &dyn Storage, folder: &str) -> (bool, Vec<Found>) {
    let mut found = Vec::new();
    let listed = storage.list(folder, &mut |entry| {
        let name = entry.name;
        let lower = name.to_lowercase();
        if entry.is_dir
            || name.starts_with(['_', '.'])
            || !EXTENSIONS.iter().any(|e| lower.ends_with(e))
            || found.len() >= 2 * cards::MAX_DECKS
        {
            return;
        }
        found.push(Found {
            name: name.to_string(),
            size: entry.size,
            modified: entry.modified,
        });
    });
    found.sort_by_key(|f| f.name.to_lowercase());
    (listed.is_ok(), found)
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

/// The ledger: none is no sources. One that cannot be read (a card
/// error, a file too big, one of another version) is an error, so that no
/// link is forgotten for it.
pub(crate) fn sources(storage: &dyn Storage) -> Result<Ledger, StorageError> {
    let mut ledger = Ledger::default();
    if !storage.exists(&ledger_path()) {
        return Ok(ledger);
    }
    let bytes = paperpaw_sdk::read_file(storage, &ledger_path(), LEDGER_MAX)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines();
    match lines.next() {
        Some(LEDGER_HEADER) | None => {}
        Some(_) => return Err(StorageError::Io),
    }
    for line in lines {
        let mut f = line.split('\t');
        let parsed = (|| {
            Some(Source {
                name: f.next()?.to_string(),
                deck: f.next()?.parse().ok()?,
                size: f.next()?.parse().ok()?,
                modified: f.next()?.parse().ok()?,
                hash: u64::from_str_radix(f.next()?, 16).ok()?,
            })
        })();
        match parsed {
            Some(source) if !source.name.is_empty() => ledger.sources.push(source),
            _ if line.trim().is_empty() => {}
            _ => ledger.other.push(line.to_string()),
        }
    }
    Ok(ledger)
}

fn write_sources(storage: &dyn Storage, ledger: &Ledger) -> Result<(), StorageError> {
    let mut text = alloc::format!("{LEDGER_HEADER}\n");
    for s in &ledger.sources {
        // A tab or a line break in a name cannot be (the card's names
        // have none that matter): written as spaces.
        let name: String = s
            .name
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        text.push_str(&alloc::format!(
            "{name}\t{}\t{}\t{}\t{:x}\n",
            s.deck,
            s.size,
            s.modified,
            s.hash
        ));
    }
    for line in &ledger.other {
        text.push_str(line);
        text.push('\n');
    }
    storage.create_dir(&cards::path("")).ok();
    let part = paperpaw_sdk::files::part_path(&ledger_path());
    storage.write(&part, text.as_bytes())?;
    paperpaw_sdk::files::put_in_place(storage, &part, &ledger_path())
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
pub(crate) fn pending(storage: &dyn Storage, folder: &str) -> Pending {
    let (folder_there, found) = files(storage, folder);
    let known = sources(storage).unwrap_or_default();
    let (mut todo, mut failed) = (0, 0);
    for f in &found {
        match known.find(&f.name) {
            Some(s) if unchanged(f, s) => failed += usize::from(s.failed()),
            _ => todo += 1,
        }
    }
    Pending {
        folder: folder_there,
        files: found.len(),
        todo,
        failed,
    }
}

/// Makes the folder, and a note in it of how files are to be made.
pub(crate) fn make_folder(storage: &dyn Storage, folder: &str, readme: &str) {
    let _ = storage.create_dir(folder);
    let note = alloc::format!("{folder}/_Read me.txt");
    if !storage.exists(&note) {
        let _ = storage.write(&note, readme.as_bytes());
    }
}

// ---- Keys --------------------------------------------------------------

const FNV64: u64 = 0xcbf2_9ce4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

fn feed(hash: u64, byte: u8) -> u64 {
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
        for byte in clean(part, MAX_TEXT).bytes().chain([0]) {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }
    hash
}

/// Bits, one for each card or line.
#[derive(Debug, Default)]
struct Bits(Vec<u8>);

impl Bits {
    fn new(count: usize) -> Self {
        Self(alloc::vec![0; count.div_ceil(8)])
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
}

impl Outcome {
    /// Whether the file itself is to blame: it is not tried again as it is.
    fn is_the_file(self) -> bool {
        matches!(
            self,
            Outcome::NoCards | Outcome::Crowded | Outcome::TooLarge | Outcome::Utf16
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
    pub same: usize,
    /// Lines left out (see `problems` for the first).
    pub skipped: usize,
    /// Lines that did not fit in a deck of 20,000 cards.
    pub no_room: usize,
    /// The first lines left out, and why.
    pub problems: Vec<(u32, Fault)>,
}

impl FileReport {
    fn new(file: &str, outcome: Outcome) -> Self {
        Self {
            file: file.to_string(),
            deck: deck_name(file),
            outcome,
            added: 0,
            changed: 0,
            same: 0,
            skipped: 0,
            no_room: 0,
            problems: Vec::new(),
        }
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
    Ask(Question),
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
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
    Finish,
}

#[derive(Debug)]
struct Work {
    found: Found,
    path: String,
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
}

impl Job {
    /// The job for the files of `folder` that `mode` takes.
    pub(crate) fn new(storage: &dyn Storage, folder: &str, mode: Mode) -> Self {
        let (_, found) = files(storage, folder);
        // A ledger that cannot be read: every file is taken, and fails,
        // so that nothing is written over it.
        let known = sources(storage);
        let listed = found.iter().map(|f| f.name.clone()).collect();
        let todo = found
            .into_iter()
            .filter(|f| match (&known, mode) {
                (Err(_), _) | (_, Mode::All) => true,
                (Ok(known), _) => known
                    .find(&f.name)
                    .is_none_or(|s| !unchanged(f, s) || (s.failed() && mode == Mode::Manual)),
            })
            .collect();
        Self {
            folder: folder.to_string(),
            todo,
            listed,
            at: 0,
            work: None,
            question: None,
            reports: Vec::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.todo.is_empty()
    }

    /// How far it is, in thousandths, and the file it is at.
    pub(crate) fn progress(&self) -> (u16, &str) {
        let total = self.todo.len().max(1);
        let name = self.todo.get(self.at).map_or("", |f| f.name.as_str());
        let within = self.work.as_ref().map_or(0, |w| match w.stage {
            Stage::Hash => (50 * w.hashed / w.found.size.max(1)).min(50) as usize,
            Stage::Count => (50 + 50 * w.resume.at / w.found.size.max(1)).min(100) as usize,
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
            Stage::Finish => 900,
        });
        let permille = (self.at * 1000 + within) / total;
        (permille.min(1000) as u16, name)
    }

    /// The answer to the question asked: `true` to merge into the deck
    /// that has the file's name, `false` to make a new deck.
    pub(crate) fn answer(&mut self, storage: &dyn Storage, link: bool) {
        self.question = None;
        let Some(work) = self.work.as_mut() else {
            return;
        };
        let name = deck_name_among(&work.found.name, &self.listed);
        let deck = if link {
            cards::deck_named(storage, &name)
        } else {
            let other = alloc::format!("{} (2)", clean(&name, MAX_NAME - 4));
            cards::add_deck(storage, &other)
        };
        match deck {
            Ok(deck) => {
                work.deck = deck;
                work.stage = Stage::Merge;
            }
            Err(_) => self.finish_with(storage, Outcome::TooManyDecks),
        }
    }

    fn finish_with(&mut self, storage: &dyn Storage, outcome: Outcome) {
        if let Some(mut work) = self.work.take() {
            if outcome.is_the_file() {
                note_failed(storage, &work.found);
            }
            work.report.outcome = outcome;
            self.reports.push(work.report);
        }
        self.at += 1;
    }

    /// Does one step: the next piece of the work.
    pub(crate) fn step(&mut self, storage: &dyn Storage) -> Step {
        if let Some(question) = &self.question {
            return Step::Ask(question.clone());
        }
        if self.work.is_none() {
            let Some(found) = self.todo.get(self.at).cloned() else {
                return Step::Done;
            };
            let path = alloc::format!("{}/{}", self.folder, found.name);
            self.work = Some(Work {
                report: FileReport::new(&found.name, Outcome::Merged),
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
                    paperpaw: false,
                    utf16: false,
                    latin1: false,
                    start: 0,
                    line: 1,
                },
                stage: Stage::Hash,
                deck: 0,
                hash: FNV64,
                hashed: 0,
                resume: table::Resume { at: 0, line: 0 },
                lines: 0,
                with_ids: false,
                cards: 0,
                slices: 1,
                matched_lines: Bits::default(),
                matched_cards: Bits::default(),
            });
            return Step::Working;
        }
        let stage = self.work.as_ref().map_or(Stage::Finish, |w| w.stage);
        let done = match stage {
            Stage::Hash => self.hash(storage),
            Stage::Count => self.count(storage),
            Stage::Decide => self.decide(storage),
            // Waiting for the answer (asked above).
            Stage::Asked => Ok(()),
            Stage::Merge => self.begin_merge(storage),
            Stage::Slice { round, slice } => self.slice(storage, round, slice),
            Stage::Finish => self.finish(storage),
        };
        match done {
            Ok(()) => {}
            Err(Failure::Outcome(outcome)) => self.finish_with(storage, outcome),
            Err(Failure::Storage) => self.finish_with(storage, Outcome::Unreadable),
        }
        match &self.question {
            Some(question) => Step::Ask(question.clone()),
            None => Step::Working,
        }
    }

    /// The file hashed, [`STEP`] bytes at a time; at its end, a file with
    /// the content merged before is done, else its layout is looked at.
    fn hash(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.as_mut() else {
            return Ok(());
        };
        let too_large = Failure::Outcome(Outcome::TooLarge);
        if work.found.size > MAX_FILE {
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
            let mut kept = known.clone();
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
        let mut report = core::mem::replace(&mut work.report, FileReport::new("", Outcome::Merged));
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
                            report.problems.push((line, fault));
                        }
                    }
                }
                ControlFlow::Continue(())
            },
        );
        work.report = report;
        let next = next?;
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
        let known = sources(storage)?;
        let exists = |id: u32| decks.iter().any(|d| d.id == id);
        let name = deck_name_among(&work.found.name, &self.listed);
        if let Some(source) = known.find(&work.found.name).filter(|s| exists(s.deck)) {
            work.deck = source.deck;
            work.stage = Stage::Merge;
            return Ok(());
        }
        // A file that was there and is not now, with this content: this
        // one, renamed. Its deck follows the name where it had the old one.
        let renamed = known.sources.iter().find(|s| {
            !s.failed() && s.hash == work.hash && !self.listed.contains(&s.name) && exists(s.deck)
        });
        if let Some(source) = renamed {
            let old = deck_name(&source.name);
            let current = decks.iter().find(|d| d.id == source.deck);
            if current.is_some_and(|d| d.name.to_lowercase() == old.to_lowercase()) {
                let _ = cards::rename_deck(storage, source.deck, &name);
            }
            work.deck = source.deck;
            let gone = source.name.clone();
            let mut kept = known.clone();
            kept.sources.retain(|s| s.name != gone);
            write_sources(storage, &kept)?;
            work.stage = Stage::Merge;
            return Ok(());
        }
        // A deck of that name no file is linked to: asked, never taken.
        let lower = name.to_lowercase();
        let same_name = decks.iter().any(|d| {
            d.name.to_lowercase() == lower && !known.sources.iter().any(|s| s.deck == d.id)
        });
        if same_name {
            work.stage = Stage::Asked;
            self.question = Some(Question {
                file: work.found.name.clone(),
                deck: name,
            });
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
        cards::fold(storage)?;
        let mut count = 0usize;
        cards::each_card(storage, work.deck, |_, _| {
            count += 1;
            ControlFlow::Continue(())
        })?;
        work.cards = count;
        if count == 0 {
            // A deck of no cards: all of the file's, in its order.
            let (path, layout) = (work.path.clone(), work.layout.clone());
            let mut no_room = 0usize;
            let added = cards::add_each(storage, work.deck, |push| {
                table::each(storage, &path, &layout, None, |item| {
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
        work.matched_lines = Bits::new(work.lines);
        work.matched_cards = Bits::new(count);
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
            _ => Stage::Finish,
        };
        Ok(())
    }

    /// The ledger told, the file done.
    fn finish(&mut self, storage: &dyn Storage) -> Result<(), Failure> {
        let Some(work) = self.work.take() else {
            return Ok(());
        };
        let written = sources(storage).and_then(|mut known| {
            known.sources.retain(|s| s.name != work.found.name);
            known.sources.push(Source {
                name: work.found.name.clone(),
                deck: work.deck,
                size: work.found.size,
                modified: work.found.modified.unwrap_or(0),
                hash: work.hash,
            });
            tidy(storage, &mut known, &self.listed);
            write_sources(storage, &known)
        });
        let mut report = work.report;
        if written.is_err() {
            report.outcome = Outcome::Unreadable;
        }
        if let Ok(decks) = cards::decks(storage)
            && let Some(deck) = decks.iter().find(|d| d.id == work.deck)
        {
            report.deck.clone_from(&deck.name);
        }
        self.reports.push(report);
        self.at += 1;
        Ok(())
    }
}

/// Room for `count` of a kind in one allocation, or `Crowded`: a failed
/// allocation is a failure to tell, not the end of the program.
fn room<T>(count: usize) -> Result<Vec<T>, Failure> {
    let mut vec = Vec::new();
    vec.try_reserve_exact(count)
        .map_err(|_| Failure::Outcome(Outcome::Crowded))?;
    Ok(vec)
}

/// The deck's cards of the slice that no line has yet, by key. At most
/// [`CROWD`] of them: a deck whose cards share a front fills a slice.
fn deck_entries(
    storage: &dyn Storage,
    work: &Work,
    by_front: bool,
    in_slice: &dyn Fn(u64) -> bool,
) -> Result<Vec<Entry>, Failure> {
    let most = CROWD.min(work.cards);
    let mut entries: Vec<Entry> = room(most)?;
    let mut order = 0usize;
    let mut crowded = false;
    cards::each_card(storage, work.deck, |_, card| {
        let this = order;
        order += 1;
        if work.matched_cards.get(this) {
            return ControlFlow::Continue(());
        }
        let key = if by_front {
            front_key(&card.front)
        } else {
            u64::from(card.state.id)
        };
        if !in_slice(key) {
            return ControlFlow::Continue(());
        }
        if entries.len() >= most {
            crowded = true;
            return ControlFlow::Break(());
        }
        let text = if by_front {
            texts_hash(&[&card.back, &card.example])
        } else {
            texts_hash(&[&card.front, &card.back, &card.example])
        };
        entries.push(Entry {
            key,
            id: card.state.id,
            order: this as u32,
            text,
        });
        ControlFlow::Continue(())
    })?;
    if crowded {
        return Err(Failure::Outcome(Outcome::Crowded));
    }
    entries.sort_unstable_by_key(|e| (e.key, e.order));
    Ok(entries)
}

/// The file's lines of the slice: matched with a card of `entries`, or new.
/// What is to be written, and how many lines were the same as their card.
fn match_lines(
    storage: &dyn Storage,
    work: &mut Work,
    entries: Vec<Entry>,
    by_front: bool,
    in_slice: &dyn Fn(u64) -> bool,
) -> Result<(Vec<Edit>, usize), Failure> {
    let mut used = alloc::vec![false; entries.len()];
    let most = CROWD.min(work.lines);
    let mut edits: Vec<Edit> = room(most)?;
    let (mut same, mut crowded, mut index) = (0usize, false, 0usize);
    table::each(storage, &work.path, &work.layout, None, |item| {
        let Item::Record(record) = item else {
            return ControlFlow::Continue(());
        };
        let this = index;
        index += 1;
        if work.matched_lines.get(this) {
            return ControlFlow::Continue(());
        }
        // The key is made of the text the card will have.
        let key = if by_front {
            Some(front_key(&clean(&record.front, MAX_TEXT)))
        } else {
            record.id.map(u64::from)
        };
        let Some(key) = key.filter(|k| in_slice(*k)) else {
            return ControlFlow::Continue(());
        };
        let offset = u32::try_from(record.offset).unwrap_or(u32::MAX);
        let start = entries.partition_point(|e| e.key < key);
        let hit = (start..entries.len())
            .take_while(|&i| entries[i].key == key)
            .find(|&i| !used[i]);
        if let Some(i) = hit {
            used[i] = true;
            work.matched_lines.set(this);
            work.matched_cards.set(entries[i].order as usize);
            let text = if by_front {
                texts_hash(&[&record.back, &record.example])
            } else {
                texts_hash(&[&record.front, &record.back, &record.example])
            };
            if text == entries[i].text {
                same += 1;
            } else if edits.len() < most {
                edits.push(Edit {
                    id: entries[i].id,
                    offset,
                    add: false,
                });
            } else {
                crowded = true;
                return ControlFlow::Break(());
            }
        } else if by_front {
            work.matched_lines.set(this);
            if edits.len() >= most {
                crowded = true;
                return ControlFlow::Break(());
            }
            edits.push(Edit {
                id: 0,
                offset,
                add: true,
            });
        }
        ControlFlow::Continue(())
    })?;
    if crowded {
        return Err(Failure::Outcome(Outcome::Crowded));
    }
    Ok((edits, same))
}

/// The changes and the new cards of a slice written, the deck anew beside
/// itself.
fn write_slice(
    storage: &dyn Storage,
    work: &mut Work,
    by_front: bool,
    mut edits: Vec<Edit>,
) -> Result<(), Failure> {
    // The changes by card number, then the new lines in the file's order.
    edits.sort_unstable_by_key(|e| (e.add, e.id, e.offset));
    let split = edits.partition_point(|e| !e.add);
    let (changes, adds) = edits.split_at(split);
    let (path, layout) = (work.path.clone(), work.layout.clone());
    let read = |offset: u32| table::record_at(storage, &path, &layout, u64::from(offset));
    let mut changed = 0usize;
    let mut pending = adds.iter();
    let added = cards::rewrite_with(
        storage,
        work.deck,
        |card| {
            if let Ok(i) = changes.binary_search_by_key(&card.state.id, |c| c.id)
                && let Some(record) = read(changes[i].offset)
            {
                if !by_front {
                    card.front = clean(&record.front, MAX_TEXT);
                }
                card.back = clean(&record.back, MAX_TEXT);
                card.example = clean(&record.example, MAX_TEXT);
                changed += 1;
            }
            true
        },
        &mut || loop {
            let edit = pending.next()?;
            if let Some(record) = read(edit.offset) {
                return Some(Texts {
                    front: record.front,
                    back: record.back,
                    example: record.example,
                });
            }
        },
    )
    .map_err(|_| Failure::Storage)?;
    work.report.changed += changed;
    work.report.added += added;
    work.report.no_room += adds.len() - added.min(adds.len());
    Ok(())
}

/// A file that cannot be a deck as it is, kept in the ledger by its size
/// and time (hash 0), so that it is told and not tried at every opening. A
/// file linked to a deck keeps its deck.
fn note_failed(storage: &dyn Storage, found: &Found) {
    let Ok(mut ledger) = sources(storage) else {
        return;
    };
    let deck = ledger.find(&found.name).map_or(0, |s| s.deck);
    ledger.sources.retain(|s| s.name != found.name);
    ledger.sources.push(Source {
        name: found.name.clone(),
        deck,
        size: found.size,
        modified: found.modified.unwrap_or(0),
        hash: 0,
    });
    let _ = write_sources(storage, &ledger);
}

/// The ledger without what can be of no use: a link to a deck that is gone,
/// a failed file that is not in the folder any more.
fn tidy(storage: &dyn Storage, ledger: &mut Ledger, listed: &[String]) {
    let Ok(decks) = cards::decks(storage) else {
        return;
    };
    ledger.sources.retain(|s| {
        if s.deck == 0 {
            listed.contains(&s.name)
        } else {
            decks.iter().any(|d| d.id == s.deck)
        }
    });
}

/// Why a step could not go on.
#[derive(Debug)]
enum Failure {
    Outcome(Outcome),
    Storage,
}

impl From<StorageError> for Failure {
    fn from(_: StorageError) -> Self {
        Failure::Storage
    }
}

impl From<Problem> for Failure {
    fn from(_: Problem) -> Self {
        Failure::Storage
    }
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
