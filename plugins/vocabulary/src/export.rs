//! *Save to card*: a deck written to a file in the folder of the SD card
//! (see [`sync`](crate::sync) for the other way, and
//! `paperpaw-notes/paperpaw/designs/flashcards-sd-card.md`, part 2).
//!
//! The file is an exchange file: a line `#paperpaw:deck 7` (this deck: so
//! that an update matches its lines by the `id` column, which is only safe
//! for a file that comes from the deck itself), a header line
//! `id front back example`, and a card on each line, in the order of the
//! deck. What was learnt is not in it. A deck linked to a file (it came
//! from one, or was saved to one) goes to the same file, else to
//! `<deck>.tsv` under a name no other file has. A tab separates the fields,
//! except in a `.csv` the deck is linked to, which keeps the separator it
//! has (a comma if there is no file yet) and gets a byte order mark, which
//! spreadsheets need to read UTF-8; a field is in quotes where it has the
//! separator, a quote or a line break.
//!
//! The file is made beside its place (`<file>.part`) a piece at a time
//! ([`Saver::step`], so that a screen shows how far it is and *Back* stops
//! it, leaving the folder as it was), and put in place when whole: a power
//! cut leaves the old file or the new one. What was there is copied to
//! `_<name>.bak.<extension>` first, if there is no copy yet or the file was
//! not written by a save of this deck (names with a `_` are no decks): the
//! first copy is the file as the user had it. A save is refused while the
//! last update left lines out, which a save would drop.
//! The deck is linked to the file after, with the hash of what was written, so
//! the next update finds the file unchanged. Heap: one piece of the output
//! (about 8 KiB) and the card in hand.

use alloc::string::String;
use core::ops::ControlFlow;

use paperpaw_sdk::{Storage, StorageError};

use crate::cards::{self, MAX_NAME};
use crate::files::{copy_file, part_of, put_in_place};
use crate::lines::{self, copy, push, push_number};
use crate::sync::{self, FNV64, FileReport, Job, MAX_FILE, Outcome as Merge, Source, feed};
use crate::table;

/// How much of the output is collected before it is written.
const PIECE: usize = 8 * 1024;
const BOM: &str = "\u{feff}";
/// Names tried for a deck's file before one after the last is taken.
const FREE_NAMES: u32 = 1 << 16;

/// How it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Saved,
    /// The deck is not there any more.
    NoDeck,
    /// The ledger could not be read: nothing was written.
    Unreadable,
    /// The file is saved, the link to the deck could not be written.
    NotLinked,
    /// The card is full.
    Full,
    /// More than a file may be (4 MiB: an update would not read it).
    TooLarge,
    /// The file could not be written (a refused allocation too: the save is
    /// cut off and can be made again).
    Failed,
    /// The file the deck is linked to has lines the last update could not
    /// make cards of (or could not read at all): a save would drop them.
    LeftOut,
    /// The deck is linked to an Anki file (`.apkg`), which is only read.
    AnkiFile,
}

/// What was done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub deck: String,
    pub file: String,
    pub cards: usize,
    /// The copy of the file that was there before, if there was one.
    pub backup: Option<String>,
    pub outcome: Outcome,
}

/// What a step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    Working,
    Done(Report),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Begin,
    Rows,
    Finish,
}

/// A deck being saved, a step at a time.
#[derive(Debug)]
pub(crate) struct Saver {
    folder: String,
    deck: u32,
    name: String,
    file: String,
    /// The file's path and the part beside it, once the file is chosen.
    path: String,
    part: String,
    stage: Stage,
    separator: u8,
    /// Where the next card's line begins in the deck's file.
    next: u64,
    total: usize,
    done: usize,
    /// The hash and size of what was written to the part.
    hash: u64,
    size: u64,
    backup: Option<String>,
}

/// A refused allocation (or any failure of a text helper) cuts a save off:
/// it is told as "could not be written", never as a file too large.
fn mem<T>(result: Result<T, StorageError>) -> Result<T, Outcome> {
    result.map_err(|_| Outcome::Failed)
}

/// A file name for a deck: its name without what a card's file system or
/// a computer's does not allow, and not starting with `_` or `.` (such a
/// file is no deck, so it would not be read again).
///
/// # Errors
/// An error where the heap refuses the name.
pub(crate) fn file_name_of(deck: &str) -> Result<String, StorageError> {
    let mut name = String::new();
    // Every replacement is one byte for one character of at least that
    // many: the name never outgrows what is reserved.
    name.try_reserve_exact(deck.len())
        .map_err(|_| StorageError::TooLarge)?;
    for c in deck.chars() {
        name.push(match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => ' ',
            c => c,
        });
    }
    let name = name
        .trim()
        .trim_start_matches(['_', '.'])
        .trim_end_matches(['.', ' ']);
    let name = cards::clean(name, MAX_NAME)?;
    if name.is_empty() {
        return copy("Deck.tsv");
    }
    let mut file = name;
    push(&mut file, ".tsv")?;
    Ok(file)
}

/// The name the copy of `file` made before it is replaced gets.
///
/// # Errors
/// An error where the heap refuses the name.
pub(crate) fn backup_name(file: &str) -> Result<String, StorageError> {
    let mut out = String::new();
    push(&mut out, "_")?;
    match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            push(&mut out, stem)?;
            push(&mut out, ".bak.")?;
            push(&mut out, ext)?;
        }
        _ => {
            push(&mut out, file)?;
            push(&mut out, ".bak")?;
        }
    }
    Ok(out)
}

/// `dir/name` in `into`, which is emptied first.
fn path_in(into: &mut String, dir: &str, name: &str) -> Result<(), StorageError> {
    into.clear();
    push(into, dir)?;
    push(into, "/")?;
    push(into, name)
}

/// A name for `name` in `dir` that nothing has: `name`, else `name (2)`,
/// `name (3)`, … (before the extension); no allocation per name tried.
fn free_name(storage: &dyn Storage, dir: &str, name: &str) -> Result<String, StorageError> {
    let (stem, ext) = match name.rsplit_once('.') {
        // The extension with its dot, so that it is one piece to put back.
        Some((stem, _)) if !stem.is_empty() => (stem, name.get(stem.len()..).unwrap_or("")),
        _ => (name, ""),
    };
    let mut path = String::new();
    let taken = |path: &mut String, candidate: &str| -> Result<bool, StorageError> {
        path_in(path, dir, candidate)?;
        Ok(storage.exists(path) || paperpaw_sdk::files::is_dir(storage, path))
    };
    if !taken(&mut path, name)? {
        return copy(name);
    }
    let mut candidate = String::new();
    for n in 2..=FREE_NAMES + 1 {
        candidate.clear();
        push(&mut candidate, stem)?;
        push(&mut candidate, " (")?;
        push_number(&mut candidate, n)?;
        push(&mut candidate, ")")?;
        push(&mut candidate, ext)?;
        // Past them, only a storage that says everything is there: the name
        // after the last.
        if n == FREE_NAMES + 1 || !taken(&mut path, &candidate)? {
            return Ok(candidate);
        }
    }
    Ok(candidate)
}

/// The job that reads the file the deck is linked to, where it is new or
/// changed: a save goes after it, so that it never writes over what was
/// edited on a computer. `None` where there is nothing to read first.
///
/// # Errors
/// An error where the ledger or the job cannot be had (a refused
/// allocation): the save must not go on, for it would not have seen what
/// changed on a computer.
pub(crate) fn update_first(
    storage: &dyn Storage,
    folder: &str,
    deck: u32,
) -> Result<Option<Job>, StorageError> {
    let ledger = sync::sources(storage)?;
    let Some(link) = sync::link_of(&ledger, deck) else {
        return Ok(None);
    };
    let job = Job::for_file(storage, folder, &link.name)?;
    Ok(Some(job).filter(|job| !job.is_empty()))
}

/// Whether what the update before a save did allows the save: the file was
/// merged, or was as it had been. A file that could not be read or merged
/// may have changes the deck lacks, so it is not written over.
pub(crate) fn may_save(reports: &[FileReport]) -> bool {
    reports
        .iter()
        .all(|r| matches!(r.outcome, Merge::Merged | Merge::Same))
}

/// A field, in quotes where it needs them.
fn field(out: &mut String, text: &str, separator: char) -> Result<(), StorageError> {
    let quote = text
        .chars()
        .any(|c| c == separator || c == '"' || c == '\n' || c == '\r');
    if !quote {
        return push(out, text);
    }
    // At most two bytes for each (a doubled quote), and the quotes.
    out.try_reserve(text.len().saturating_mul(2).saturating_add(2))
        .map_err(|_| StorageError::TooLarge)?;
    out.push('"');
    for c in text.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    Ok(())
}

/// [`outcome_of`] where a `TooLarge` can only be a refused allocation of the
/// copy (the file's size is checked as it is written): "could not be written".
fn outcome_of_copy(error: StorageError) -> Outcome {
    match error {
        StorageError::Full => Outcome::Full,
        _ => Outcome::Failed,
    }
}

fn outcome_of(error: StorageError) -> Outcome {
    match error {
        StorageError::Full => Outcome::Full,
        StorageError::TooLarge => Outcome::TooLarge,
        _ => Outcome::Failed,
    }
}

/// `number` in decimal, in `digits`.
fn decimal(number: u32, digits: &mut [u8; 10]) -> &str {
    let (mut at, mut rest) = (digits.len(), number);
    loop {
        at -= 1;
        if let Some(slot) = digits.get_mut(at) {
            *slot = b'0' + (rest % 10) as u8;
        }
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    digits
        .get(at..)
        .and_then(|d| core::str::from_utf8(d).ok())
        .unwrap_or("0")
}

impl Saver {
    /// A save of `deck` into `folder`.
    ///
    /// # Errors
    /// An error where the heap refuses the folder's name.
    pub(crate) fn new(folder: &str, deck: u32) -> Result<Self, StorageError> {
        Ok(Self {
            folder: copy(folder)?,
            deck,
            name: String::new(),
            file: String::new(),
            path: String::new(),
            part: String::new(),
            stage: Stage::Begin,
            separator: b'\t',
            next: 0,
            total: 0,
            done: 0,
            hash: FNV64,
            size: 0,
            backup: None,
        })
    }

    /// How far it is, in thousandths, and the file.
    pub(crate) fn progress(&self) -> (u16, &str) {
        let permille = match self.stage {
            Stage::Begin => 0,
            Stage::Rows => 50 + 900 * self.done / self.total.max(1),
            Stage::Finish => 950,
        };
        (permille.min(1000) as u16, &self.file)
    }

    /// Writes `text` to the part, and counts it.
    fn put(&mut self, storage: &dyn Storage, text: &str) -> Result<(), Outcome> {
        self.size += text.len() as u64;
        if self.size > MAX_FILE {
            return Err(Outcome::TooLarge);
        }
        self.hash = text.bytes().fold(self.hash, feed);
        storage
            .append(&self.part, text.as_bytes())
            .map_err(outcome_of)
    }

    /// One more step of the work.
    pub(crate) fn step(&mut self, storage: &dyn Storage) -> Step {
        let result = match self.stage {
            Stage::Begin => self.begin(storage).map(|()| None),
            Stage::Rows => self.rows(storage).map(|()| None),
            Stage::Finish => self.finish(storage).map(Some),
        };
        match result {
            Ok(None) => Step::Working,
            Ok(Some(report)) => Step::Done(report),
            Err(outcome) => {
                // Nothing is left of a part that cannot be finished.
                self.cancel(storage);
                Step::Done(self.report(outcome))
            }
        }
    }

    /// The report of a save that is over: what it holds is moved, not copied.
    fn report(&mut self, outcome: Outcome) -> Report {
        Report {
            deck: core::mem::take(&mut self.name),
            file: core::mem::take(&mut self.file),
            cards: self.done,
            backup: self.backup.take(),
            outcome,
        }
    }

    /// The file chosen, the part begun with its marker and header.
    fn begin(&mut self, storage: &dyn Storage) -> Result<(), Outcome> {
        let decks = cards::decks(storage).map_err(|_| Outcome::Failed)?;
        let Some(deck) = decks.iter().find(|d| d.id == self.deck) else {
            return Err(Outcome::NoDeck);
        };
        self.name = mem(copy(&deck.name))?;
        // Only how far the save is shown by it: a refusal leaves it unknown.
        self.total = cards::sizes(storage)
            .unwrap_or_default()
            .get(&self.deck)
            .copied()
            .unwrap_or(0);
        let ledger = sync::sources(storage).map_err(|_| Outcome::Unreadable)?;
        let link = sync::link_of(&ledger, self.deck);
        if link.is_some_and(|s| s.left_out > 0 || s.failed()) {
            return Err(Outcome::LeftOut);
        }
        if let Some(link) = link
            && crate::apkg::is_apkg(&link.name)
        {
            self.file = mem(copy(&link.name))?;
            return Err(Outcome::AnkiFile);
        }
        let _ = storage.create_dir(&self.folder);
        self.file =
            match link {
                Some(link) => mem(copy(&link.name))?,
                None => mem(file_name_of(&self.name)
                    .and_then(|name| free_name(storage, &self.folder, &name)))?,
            };
        mem(path_in(&mut self.path, &self.folder, &self.file))?;
        self.part = mem(part_of(&self.path))?;
        let csv = sync::ends_with_ignore_case(&self.file, ".csv");
        self.separator = if csv {
            // A file there keeps the separator it has.
            table::detect(storage, &self.path).map_or(b',', |layout| layout.separator)
        } else {
            b'\t'
        };
        let mut sep = [0u8; 4];
        let sep: &str = char::from(self.separator).encode_utf8(&mut sep);
        let mut head = String::new();
        if csv && self.bom(storage) {
            mem(push(&mut head, BOM))?;
        }
        mem(push(&mut head, "#paperpaw:deck "))?;
        mem(push_number(&mut head, self.deck))?;
        for text in ["\nid", sep, "front", sep, "back", sep, "example\n"] {
            mem(push(&mut head, text))?;
        }
        // The part is begun anew: what a cut-off save left is not used.
        storage.write(&self.part, b"").map_err(outcome_of)?;
        self.put(storage, &head)?;
        self.stage = Stage::Rows;
        Ok(())
    }

    /// Whether the file gets a byte order mark: it had one, or is new (a
    /// spreadsheet needs it to read UTF-8).
    fn bom(&self, storage: &dyn Storage) -> bool {
        let mut start = [0u8; 3];
        match storage.read_at(&self.path, 0, &mut start) {
            Ok(n) if n > 0 => start == *BOM.as_bytes(),
            _ => true,
        }
    }

    /// Cards of the deck written until a piece is full.
    fn rows(&mut self, storage: &dyn Storage) -> Result<(), Outcome> {
        let sep = char::from(self.separator);
        let mut out = String::new();
        mem(out
            .try_reserve_exact(PIECE)
            .map_err(|_| StorageError::TooLarge))?;
        let (mut stop, mut written) = (None, 0usize);
        let mut failed = Ok(());
        cards::each_card_from(storage, self.deck, self.next, |at, card| {
            if out.len() >= PIECE {
                stop = Some(at);
                return ControlFlow::Break(());
            }
            let mut line = push_number(&mut out, card.state.id);
            for text in [&card.front, &card.back, &card.example] {
                if line.is_ok() {
                    let mut buf = [0u8; 4];
                    line = push(&mut out, sep.encode_utf8(&mut buf))
                        .and_then(|()| field(&mut out, text, sep));
                }
            }
            if line.is_err() || push(&mut out, "\n").is_err() {
                failed = Err(Outcome::Failed);
                return ControlFlow::Break(());
            }
            written += 1;
            ControlFlow::Continue(())
        })
        .map_err(|_| Outcome::Failed)?;
        failed?;
        self.put(storage, &out)?;
        self.done += written;
        match stop {
            Some(at) => self.next = at,
            None => self.stage = Stage::Finish,
        }
        Ok(())
    }

    /// Whether the file in place begins with this deck's marker: it was
    /// written by a save. Where that cannot be seen (a refused allocation),
    /// it is taken as not: a copy is made, which is the safe side.
    fn written_here(&self, storage: &dyn Storage) -> bool {
        let mut start = [0u8; 48];
        let n = storage.read_at(&self.path, 0, &mut start).unwrap_or(0);
        let Ok(text) = lines::lossy(start.get(..n).unwrap_or(&[])) else {
            return false;
        };
        let text = text.trim_start_matches(BOM);
        let mut digits = [0u8; 10];
        text.strip_prefix("#paperpaw:deck ")
            .and_then(|rest| rest.lines().next())
            .is_some_and(|id| id.trim() == decimal(self.deck, &mut digits))
    }

    /// Stops the save: the part that is begun is removed.
    pub(crate) fn cancel(&self, storage: &dyn Storage) {
        if !self.part.is_empty() {
            let _ = storage.remove(&self.part);
        }
    }

    /// The old file copied, the new one put in place, the deck linked.
    fn finish(&mut self, storage: &dyn Storage) -> Result<Report, Outcome> {
        if storage.exists(&self.path) {
            let backup = mem(backup_name(&self.file))?;
            let mut copy_to = String::new();
            mem(path_in(&mut copy_to, &self.folder, &backup))?;
            // The first copy is the file as the user had it: a file this
            // deck wrote itself (it names the deck) does not replace it.
            if !storage.exists(&copy_to) || !self.written_here(storage) {
                copy_file(storage, &self.path, &copy_to).map_err(outcome_of_copy)?;
                self.backup = Some(backup);
            }
        }
        put_in_place(storage, &self.part, &self.path).map_err(outcome_of_copy)?;
        // The file is there: that the link cannot be written is told, but
        // the file stands (an update then reads it, and finds it the same).
        let linked = self.link(storage);
        let outcome = if linked.is_ok() {
            Outcome::Saved
        } else {
            Outcome::NotLinked
        };
        Ok(self.report(outcome))
    }

    /// The ledger with this deck linked to the file, written.
    fn link(&self, storage: &dyn Storage) -> Result<(), StorageError> {
        let mut ledger = sync::sources(storage)?;
        let mirror = sync::link_of(&ledger, self.deck).is_some_and(|s| s.mirror);
        ledger
            .sources
            .retain(|s| s.name != self.file && s.deck != self.deck);
        let source = Source {
            name: copy(&self.file)?,
            deck: self.deck,
            size: self.size,
            modified: storage.modified(&self.path).unwrap_or(0),
            hash: self.hash,
            mirror,
            left_out: 0,
        };
        lines::reserve(&mut ledger.sources, 1)?;
        ledger.sources.push(source);
        let (_, found) = sync::files(storage, &self.folder)?;
        sync::tidy(storage, &mut ledger, |name| {
            found.iter().any(|f| f.name == name)
        });
        sync::write_sources(storage, &ledger)
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
