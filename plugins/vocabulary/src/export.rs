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

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{Storage, StorageError};

use crate::cards::{self, MAX_NAME};
use crate::sync::{self, FNV64, FileReport, Job, MAX_FILE, Outcome as Merge, Source, feed};
use crate::table;

/// How much of the output is collected before it is written.
const PIECE: usize = 8 * 1024;
const BOM: &str = "\u{feff}";

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
    /// The file could not be written.
    Failed,
    /// The file the deck is linked to has lines the last update could not
    /// make cards of (or could not read at all): a save would drop them.
    LeftOut,
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

/// A file name for a deck: its name without what a card's file system or
/// a computer's does not allow, and not starting with `_` or `.` (such a
/// file is no deck, so it would not be read again).
pub(crate) fn file_name_of(deck: &str) -> String {
    let name: String = deck
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let name = name
        .trim()
        .trim_start_matches(['_', '.'])
        .trim_end_matches(['.', ' ']);
    let name = cards::clean(name, MAX_NAME);
    if name.is_empty() {
        String::from("Deck.tsv")
    } else {
        alloc::format!("{name}.tsv")
    }
}

/// The name the copy of `file` made before it is replaced gets.
pub(crate) fn backup_name(file: &str) -> String {
    match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => alloc::format!("_{stem}.bak.{ext}"),
        _ => alloc::format!("_{file}.bak"),
    }
}

/// The job that reads the file the deck is linked to, where it is new or
/// changed: a save goes after it, so that it never writes over what was
/// edited on a computer. `None` where there is nothing to read first.
pub(crate) fn update_first(storage: &dyn Storage, folder: &str, deck: u32) -> Option<Job> {
    let file = sync::sources(storage)
        .ok()
        .and_then(|ledger| sync::link_of(&ledger, deck).map(|s| s.name.clone()))?;
    Some(Job::for_file(storage, folder, &file)).filter(|job| !job.is_empty())
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
fn field(out: &mut String, text: &str, separator: char) {
    let quote = text
        .chars()
        .any(|c| c == separator || c == '"' || c == '\n' || c == '\r');
    if !quote {
        out.push_str(text);
        return;
    }
    out.push('"');
    for c in text.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
}

fn outcome_of(error: StorageError) -> Outcome {
    match error {
        StorageError::Full => Outcome::Full,
        StorageError::TooLarge => Outcome::TooLarge,
        _ => Outcome::Failed,
    }
}

impl Saver {
    pub(crate) fn new(folder: &str, deck: u32) -> Self {
        Self {
            folder: folder.to_string(),
            deck,
            name: String::new(),
            file: String::new(),
            stage: Stage::Begin,
            separator: b'\t',
            next: 0,
            total: 0,
            done: 0,
            hash: FNV64,
            size: 0,
            backup: None,
        }
    }

    fn path(&self) -> String {
        alloc::format!("{}/{}", self.folder, self.file)
    }

    fn part(&self) -> String {
        paperpaw_sdk::files::part_path(&self.path())
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
    fn push(&mut self, storage: &dyn Storage, text: &str) -> Result<(), Outcome> {
        self.size += text.len() as u64;
        if self.size > MAX_FILE {
            return Err(Outcome::TooLarge);
        }
        self.hash = text.bytes().fold(self.hash, feed);
        storage
            .append(&self.part(), text.as_bytes())
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
                if self.stage != Stage::Begin || !self.file.is_empty() {
                    let _ = storage.remove(&self.part());
                }
                Step::Done(self.report(outcome))
            }
        }
    }

    fn report(&self, outcome: Outcome) -> Report {
        Report {
            deck: self.name.clone(),
            file: self.file.clone(),
            cards: self.done,
            backup: self.backup.clone(),
            outcome,
        }
    }

    /// The file chosen, the part begun with its marker and header.
    fn begin(&mut self, storage: &dyn Storage) -> Result<(), Outcome> {
        let decks = cards::decks(storage).map_err(|_| Outcome::Failed)?;
        let Some(deck) = decks.iter().find(|d| d.id == self.deck) else {
            return Err(Outcome::NoDeck);
        };
        self.name.clone_from(&deck.name);
        self.total = cards::sizes(storage).get(&self.deck).copied().unwrap_or(0);
        let ledger = sync::sources(storage).map_err(|_| Outcome::Unreadable)?;
        if sync::link_of(&ledger, self.deck).is_some_and(|s| s.left_out > 0 || s.failed()) {
            self.file = String::new();
            return Err(Outcome::LeftOut);
        }
        let _ = storage.create_dir(&self.folder);
        self.file = match sync::link_of(&ledger, self.deck) {
            Some(link) => link.name.clone(),
            None => {
                let path = paperpaw_sdk::files::free_path(
                    storage,
                    &self.folder,
                    &file_name_of(&self.name),
                );
                path.rsplit('/').next().unwrap_or("Deck.tsv").to_string()
            }
        };
        let lower = self.file.to_lowercase();
        let csv = lower.ends_with(".csv");
        self.separator = if csv {
            // A file there keeps the separator it has.
            table::detect(storage, &self.path()).map_or(b',', |layout| layout.separator)
        } else {
            b'\t'
        };
        let sep = char::from(self.separator);
        let head = alloc::format!(
            "{}#paperpaw:deck {}\nid{sep}front{sep}back{sep}example\n",
            if csv && self.bom(storage) { BOM } else { "" },
            self.deck
        );
        // The part is begun anew: what a cut-off save left is not used.
        storage.write(&self.part(), b"").map_err(outcome_of)?;
        self.push(storage, &head)?;
        self.stage = Stage::Rows;
        Ok(())
    }

    /// Whether the file gets a byte order mark: it had one, or is new (a
    /// spreadsheet needs it to read UTF-8).
    fn bom(&self, storage: &dyn Storage) -> bool {
        let mut start = [0u8; 3];
        match storage.read_at(&self.path(), 0, &mut start) {
            Ok(n) if n > 0 => start == *BOM.as_bytes(),
            _ => true,
        }
    }

    /// Cards of the deck written until a piece is full.
    fn rows(&mut self, storage: &dyn Storage) -> Result<(), Outcome> {
        let sep = char::from(self.separator);
        let mut out = String::new();
        let (mut stop, mut written) = (None, 0usize);
        cards::each_card_from(storage, self.deck, self.next, |at, card| {
            if out.len() >= PIECE {
                stop = Some(at);
                return ControlFlow::Break(());
            }
            out.push_str(&card.state.id.to_string());
            for text in [&card.front, &card.back, &card.example] {
                out.push(sep);
                field(&mut out, text, sep);
            }
            out.push('\n');
            written += 1;
            ControlFlow::Continue(())
        })
        .map_err(|_| Outcome::Failed)?;
        self.push(storage, &out)?;
        self.done += written;
        match stop {
            Some(at) => self.next = at,
            None => self.stage = Stage::Finish,
        }
        Ok(())
    }

    /// Whether the file in place begins with this deck's marker: it was
    /// written by a save.
    fn written_here(&self, storage: &dyn Storage) -> bool {
        let mut start = [0u8; 48];
        let n = storage.read_at(&self.path(), 0, &mut start).unwrap_or(0);
        let text = String::from_utf8_lossy(&start[..n]);
        let text = text.trim_start_matches(BOM);
        text.strip_prefix("#paperpaw:deck ")
            .and_then(|rest| rest.lines().next())
            .is_some_and(|id| id.trim() == self.deck.to_string())
    }

    /// Stops the save: the part that is begun is removed.
    pub(crate) fn cancel(&self, storage: &dyn Storage) {
        if !self.file.is_empty() {
            let _ = storage.remove(&self.part());
        }
    }

    /// The old file copied, the new one put in place, the deck linked.
    fn finish(&mut self, storage: &dyn Storage) -> Result<Report, Outcome> {
        let path = self.path();
        if storage.exists(&path) {
            let backup = backup_name(&self.file);
            let copy = alloc::format!("{}/{backup}", self.folder);
            // The first copy is the file as the user had it: a file this
            // deck wrote itself (it names the deck) does not replace it.
            if !storage.exists(&copy) || !self.written_here(storage) {
                paperpaw_sdk::files::copy(storage, &path, &copy).map_err(outcome_of)?;
                self.backup = Some(backup);
            }
        }
        paperpaw_sdk::files::put_in_place(storage, &self.part(), &path).map_err(outcome_of)?;
        // The file is there: that the link cannot be written is told, but
        // the file stands (an update then reads it, and finds it the same).
        let linked = sync::sources(storage).and_then(|mut ledger| {
            let mirror = sync::link_of(&ledger, self.deck).is_some_and(|s| s.mirror);
            ledger
                .sources
                .retain(|s| s.name != self.file && s.deck != self.deck);
            ledger.sources.push(Source {
                name: self.file.clone(),
                deck: self.deck,
                size: self.size,
                modified: storage.modified(&path).unwrap_or(0),
                hash: self.hash,
                mirror,
                left_out: 0,
            });
            let listed: Vec<String> = sync::files(storage, &self.folder)
                .1
                .into_iter()
                .map(|f| f.name)
                .collect();
            sync::tidy(storage, &mut ledger, &listed);
            sync::write_sources(storage, &ledger)
        });
        Ok(self.report(if linked.is_ok() {
            Outcome::Saved
        } else {
            Outcome::NotLinked
        }))
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
