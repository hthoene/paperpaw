//! *Cards on the SD card* on interface 7: the way into the files of the folder
//! (`vocabulary.sd-menu`: *Update*, a row to save each deck, a switch for each
//! deck linked to a file) and the screen that does the work
//! (`vocabulary.sd-card`: reads the files into the decks, asks where a deck of
//! the file's name is there already, or saves a deck to its file, and says what
//! came of each).
//!
//! The model is host-free (over [`Storage`] and [`Words`]); the shell drives it
//! one bounded step each time the host calls back for the work
//! (`native_plugin`), and draws it with [`ItemList`] (the progress is a
//! percentage in the first row, where interface 6 drew a bar).
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::{Storage, StorageError};
use paperpaw_ui_kit::{Item, ItemList, text};

use crate::cards::Deck;
use crate::export::{self, Saver, file_name_of};
use crate::native_words::Words;
use crate::sync::{self, FileReport, Job, Ledger, Mode, Outcome, Pending, Question};
use crate::table::Fault;

/// The screen of the files' menu.
pub(crate) const MENU: &str = "vocabulary.sd-menu";
/// The screen that updates from the files, or saves a deck.
pub(crate) const CARD: &str = "vocabulary.sd-card";
/// The hand-over key that reads every file anew, not only the new and changed.
pub(crate) const FORCE: &str = "force";
/// The hand-over key of the deck (its number) to save to its file, after an
/// update of that file if it changed.
pub(crate) const SAVE: &str = "save";
/// The hand-over key of the screen the flash cards open by themselves: the files
/// that failed as they are are not tried again.
pub(crate) const AUTO: &str = "auto";
/// Rows of the result at most.
const MOST_ROWS: usize = 40;
/// What the action rows show at the right.
const GO: &str = "›";

/// A refused allocation or a catalog that cannot say.
fn memory(_: StorageError) -> Error {
    Error::NoMemory
}

/// A copy of `text` that may be refused.
fn copy(text: &str) -> Result<String, Error> {
    paperpaw_ui_kit::text::copy(text)
}

/// One result row, made with its room reserved.
fn one(title: String, note: String) -> Result<Vec<(String, String)>, Error> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
    rows.push((title, note));
    Ok(rows)
}

/// A list of `items`, its room reserved.
fn list_of<const N: usize>(items: [Item; N]) -> Result<ItemList, Error> {
    let mut all = Vec::new();
    all.try_reserve_exact(N).map_err(|_| Error::NoMemory)?;
    all.extend(items);
    Ok(ItemList::new(all))
}

/// The folder the setting names (the default where the setting is empty or
/// unusable).
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn folder_of(setting: &str) -> Result<String, Error> {
    sync::folder_of(setting).map_err(memory)
}

/// What a hand-over of the update screen names: the files taken, and the deck
/// to save.
///
/// # Errors
/// [`Error::Invalid`] for a key given twice differently.
pub(crate) fn named<'a>(
    params: impl Iterator<Item = Result<(&'a str, &'a str), Error>>,
) -> Result<(Mode, Option<u32>), Error> {
    let (mut force, mut auto, mut save): (Option<&str>, Option<&str>, Option<&str>) =
        (None, None, None);
    for param in params {
        let (key, value) = param?;
        let seen = match key {
            FORCE => &mut force,
            AUTO => &mut auto,
            SAVE => &mut save,
            _ => continue,
        };
        if seen.is_some_and(|seen| seen != value) {
            return Err(Error::Invalid);
        }
        *seen = Some(value);
    }
    let mode = if force.is_some() {
        Mode::All
    } else if auto.is_some() {
        Mode::Auto
    } else {
        Mode::Manual
    };
    Ok((mode, save.and_then(|deck| deck.parse().ok())))
}

// ---- The menu --------------------------------------------------------------

/// What a row of the menu does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// Reads the files into the decks.
    Update,
    /// Saves the deck of this number to its file.
    Save(u32),
    /// Switches the removal of the cards the deck's file lacks.
    Mirror(u32),
}

/// What the menu shows.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shown<'a> {
    pub folder: &'a str,
    pub pending: Pending,
    pub decks: &'a [Deck],
    pub ledger: &'a Ledger,
}

/// The menu's screen.
#[derive(Debug)]
pub(crate) struct MenuView {
    list: ItemList,
    targets: Vec<Target>,
}

impl MenuView {
    /// The view of what is there now.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the catalog says.
    pub(crate) fn new(words: &mut impl Words, shown: Shown<'_>) -> Result<Self, Error> {
        let (items, targets) = rows(words, shown)?;
        Ok(Self {
            list: ItemList::new(items),
            targets,
        })
    }

    /// Shows what is there now, the focus kept (on the last row where there
    /// are fewer).
    ///
    /// # Errors
    /// As [`Self::new`]; the view is as it was.
    pub(crate) fn refresh(
        &mut self,
        words: &mut impl Words,
        shown: Shown<'_>,
    ) -> Result<(), Error> {
        let focus = self.list.focus();
        let (items, targets) = rows(words, shown)?;
        self.list.replace(items);
        self.list
            .set_focus(focus.min(targets.len().saturating_sub(1)));
        self.targets = targets;
        Ok(())
    }

    /// What the row `index` does.
    pub(crate) fn target(&self, index: usize) -> Option<Target> {
        self.targets.get(index).copied()
    }

    /// The list, to draw and to take input.
    pub(crate) fn list(&mut self) -> &mut ItemList {
        &mut self.list
    }
}

fn rows(words: &mut impl Words, shown: Shown<'_>) -> Result<(Vec<Item>, Vec<Target>), Error> {
    let mut items = Vec::new();
    let mut targets = Vec::new();
    let room = 1 + 2 * shown.decks.len();
    items.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
    targets
        .try_reserve_exact(room)
        .map_err(|_| Error::NoMemory)?;
    let note = crate::native_decks::files_note(words, shown.folder, shown.pending)?;
    items.push(Item::whole(
        &words.copy("vocabulary.sd.update")?,
        GO,
        &note,
    )?);
    targets.push(Target::Update);
    for deck in shown.decks {
        let link = sync::link_of(shown.ledger, deck.id);
        let (key, file) = match link {
            Some(link) if crate::apkg::is_apkg(&link.name) => {
                ("vocabulary.sd.save.anki", copy(&link.name)?)
            }
            Some(link) => ("vocabulary.sd.save.linked", copy(&link.name)?),
            None => (
                "vocabulary.sd.save.new",
                file_name_of(&deck.name).map_err(memory)?,
            ),
        };
        let title = words.format(
            "vocabulary.sd.save",
            &[Arg::Text {
                name: "deck",
                value: &deck.name,
            }],
        )?;
        let note = words.format(
            key,
            &[
                Arg::Text {
                    name: "file",
                    value: &file,
                },
                Arg::Text {
                    name: "folder",
                    value: shown.folder,
                },
            ],
        )?;
        items.push(Item::whole(&title, GO, &note)?);
        targets.push(Target::Save(deck.id));
        if let Some(link) = link {
            let title = words.format(
                "vocabulary.sd.mirror",
                &[Arg::Text {
                    name: "deck",
                    value: &deck.name,
                }],
            )?;
            let value = words.copy(if link.mirror {
                "vocabulary.sd.mirror.on"
            } else {
                "vocabulary.sd.mirror.off"
            })?;
            let help = words.format(
                "vocabulary.sd.mirror.help",
                &[Arg::Text {
                    name: "file",
                    value: &link.name,
                }],
            )?;
            items.push(Item::whole(&title, &value, &help)?);
            targets.push(Target::Mirror(deck.id));
        }
    }
    Ok((items, targets))
}

// ---- The work ---------------------------------------------------------------

/// Where the work is.
#[derive(Debug)]
enum Phase {
    Working,
    /// A deck of the file's name is there already: add to it, or make a new
    /// one.
    Asking(Question),
    Done,
}

/// What a step of the work asks of the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Next {
    /// Another step is to be done.
    pub more: bool,
    /// What the screen shows changed enough to be drawn again.
    pub redraw: bool,
}

/// The update (or the save) a screen is doing.
#[derive(Debug)]
pub(crate) struct Run {
    list: ItemList,
    phase: Phase,
    job: Option<Job>,
    folder: String,
    mode: Mode,
    /// The deck to save to its file, when the screen is for that.
    save: Option<u32>,
    saver: Option<Saver>,
    /// What the update before a save said.
    before: Vec<(String, String)>,
    /// The progress (in tens of per mille) and the file shown last: drawn again
    /// only when one of them changes.
    shown: (u16, String),
    /// What the result says: a title and a note for each row.
    rows: Vec<(String, String)>,
    started: bool,
}

impl Run {
    /// A run over the files of `folder`: `mode` says which, `save` a deck to
    /// save to its file.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn new(folder: &str, mode: Mode, save: Option<u32>) -> Result<Self, Error> {
        Ok(Self {
            list: ItemList::default(),
            phase: Phase::Working,
            job: None,
            folder: copy(folder)?,
            mode,
            save,
            saver: None,
            before: Vec::new(),
            shown: (u16::MAX, String::new()),
            rows: Vec::new(),
            started: false,
        })
    }

    /// Whether the work is going (not asking, not done).
    pub(crate) fn working(&self) -> bool {
        matches!(self.phase, Phase::Working)
    }

    /// Whether a question waits for the reader.
    pub(crate) fn asking(&self) -> bool {
        matches!(self.phase, Phase::Asking(_))
    }

    /// The list, to draw and to take input.
    pub(crate) fn list(&mut self) -> &mut ItemList {
        &mut self.list
    }

    /// Leaves a save off: no part of a file stays in the folder.
    pub(crate) fn cancel(&mut self, storage: &dyn Storage) {
        if let Some(saver) = self.saver.take() {
            saver.cancel(storage);
        }
    }

    /// One more step of the job (or of the save). Where it fails for want of
    /// memory the run ends with a line that says so.
    pub(crate) fn step(&mut self, storage: &dyn Storage, words: &mut impl Words) -> Next {
        match self.advance(storage, words) {
            Ok(next) => next,
            Err(_) => {
                self.cancel(storage);
                self.job = None;
                // Said where the catalog can say it; else the list is empty.
                let rows = words
                    .copy("vocabulary.sd.nomemory")
                    .and_then(|note| one(String::new(), note))
                    .unwrap_or_default();
                self.finish_quietly(rows);
                Next {
                    more: false,
                    redraw: true,
                }
            }
        }
    }

    fn advance(&mut self, storage: &dyn Storage, words: &mut impl Words) -> Result<Next, Error> {
        if !self.started {
            self.started = true;
            return self.start(storage, words);
        }
        if let Some(saver) = self.saver.as_mut() {
            return match saver.step(storage) {
                export::Step::Working => {
                    let (permille, file) = saver.progress();
                    let redraw = worth_drawing(&mut self.shown, permille, file)?;
                    Ok(Next { more: true, redraw })
                }
                export::Step::Done(report) => {
                    self.saver = None;
                    let mut rows = core::mem::take(&mut self.before);
                    save_rows(words, &report, &mut rows)?;
                    self.finish(rows)?;
                    Ok(Next {
                        more: false,
                        redraw: true,
                    })
                }
            };
        }
        let Some(job) = self.job.as_mut() else {
            return Ok(Next {
                more: false,
                redraw: false,
            });
        };
        match job.step(storage) {
            sync::Step::Working => {
                let (permille, file) = job.progress();
                let redraw = worth_drawing(&mut self.shown, permille, file)?;
                Ok(Next { more: true, redraw })
            }
            sync::Step::Ask => {
                let Some(question) = job.question() else {
                    return Ok(Next {
                        more: false,
                        redraw: false,
                    });
                };
                let question = Question {
                    file: copy(&question.file)?,
                    deck: copy(&question.deck)?,
                };
                self.phase = Phase::Asking(question);
                Ok(Next {
                    more: false,
                    redraw: true,
                })
            }
            sync::Step::Done => self.updated(storage, words),
        }
    }

    /// The folder looked at; made where it is not there.
    fn start(&mut self, storage: &dyn Storage, words: &mut impl Words) -> Result<Next, Error> {
        let done = |run: &mut Self, rows| -> Result<Next, Error> {
            run.finish(rows)?;
            Ok(Next {
                more: false,
                redraw: true,
            })
        };
        if let Some(deck) = self.save {
            // The file is read first where it changed, so that a save never
            // writes over what was edited on a computer.
            match export::update_first(storage, &self.folder, deck) {
                Ok(Some(job)) => self.job = Some(job),
                Ok(None) => return self.begin_save(words, deck),
                // Not seen what changed on a computer: no save over it.
                Err(_) => return self.save_failed(words),
            }
            return Ok(Next {
                more: true,
                redraw: true,
            });
        }
        let seen = sync::pending(storage, &self.folder).unwrap_or_default();
        if !seen.folder {
            let readme = words.copy("vocabulary.sd.readme")?;
            sync::make_folder(storage, &self.folder, &readme).ok();
            let note = words.format(
                "vocabulary.sd.made",
                &[Arg::Text {
                    name: "folder",
                    value: &self.folder,
                }],
            )?;
            return done(self, one(String::new(), note)?);
        }
        let Ok(job) = Job::new(storage, &self.folder, self.mode) else {
            let note = words.copy("vocabulary.sd.nomemory")?;
            return done(self, one(String::new(), note)?);
        };
        if job.is_empty() {
            let key = if seen.files == 0 {
                "vocabulary.sd.row.empty"
            } else {
                "vocabulary.sd.uptodate"
            };
            let note = words.format(
                key,
                &[Arg::Text {
                    name: "folder",
                    value: &self.folder,
                }],
            )?;
            return done(self, one(String::new(), note)?);
        }
        self.job = Some(job);
        Ok(Next {
            more: true,
            redraw: true,
        })
    }

    fn begin_save(&mut self, words: &mut impl Words, deck: u32) -> Result<Next, Error> {
        match Saver::new(&self.folder, deck) {
            Ok(saver) => {
                self.saver = Some(saver);
                Ok(Next {
                    more: true,
                    redraw: true,
                })
            }
            Err(_) => self.save_failed(words),
        }
    }

    fn save_failed(&mut self, words: &mut impl Words) -> Result<Next, Error> {
        let rows = one(String::new(), words.copy("vocabulary.sd.save.failed")?)?;
        self.finish(rows)?;
        Ok(Next {
            more: false,
            redraw: true,
        })
    }

    fn updated(&mut self, _storage: &dyn Storage, words: &mut impl Words) -> Result<Next, Error> {
        let reports = self.job.take().map(|job| job.reports).unwrap_or_default();
        let mut rows = Vec::new();
        for report in &reports {
            report_rows(words, report, &mut rows)?;
        }
        if let Some(deck) = self.save {
            if export::may_save(&reports) {
                // On with the save: the file is as the deck has it now.
                self.before = rows;
                return self.begin_save(words, deck);
            }
            push(
                &mut rows,
                String::new(),
                words.copy("vocabulary.sd.save.not-updated")?,
            )?;
            self.finish(rows)?;
            return Ok(Next {
                more: false,
                redraw: true,
            });
        }
        if rows.is_empty() {
            push(
                &mut rows,
                String::new(),
                words.copy("vocabulary.sd.uptodate")?,
            )?;
        }
        self.finish(rows)?;
        Ok(Next {
            more: false,
            redraw: true,
        })
    }

    /// The reader's answer to the question: add to the deck there, or make a
    /// new one.
    pub(crate) fn answer(&mut self, storage: &dyn Storage, link: bool) {
        if let Some(job) = self.job.as_mut() {
            job.answer(storage, link);
        }
        self.phase = Phase::Working;
    }

    /// The rows shown when all is done.
    fn finish(&mut self, mut rows: Vec<(String, String)>) -> Result<(), Error> {
        rows.truncate(MOST_ROWS);
        let mut items = Vec::new();
        items
            .try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for (title, note) in &rows {
            let title = if title.is_empty() { " " } else { title };
            items.push(Item::whole(title, "", note)?);
        }
        self.rows = rows;
        self.list = ItemList::new(items);
        self.phase = Phase::Done;
        Ok(())
    }

    /// As [`Self::finish`], where no row can be made: an empty result.
    fn finish_quietly(&mut self, rows: Vec<(String, String)>) {
        if self.finish(rows).is_err() {
            self.rows = Vec::new();
            self.list = ItemList::default();
            self.phase = Phase::Done;
        }
    }

    /// Ends the run where a step cannot be done at all (the card is gone, the
    /// heap refuses the service's buffers): with `note` as its one line, if
    /// there is one. A save left off keeps no part of a file in the folder
    /// only where the card still answers; else the part is the next save's to
    /// remove.
    pub(crate) fn abort(&mut self, note: Option<String>) {
        self.saver = None;
        self.job = None;
        let rows = note
            .and_then(|note| one(String::new(), note).ok())
            .unwrap_or_default();
        self.finish_quietly(rows);
    }

    /// What the screen shows now: the progress, the question, or the result.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the catalog says.
    pub(crate) fn show(&mut self, words: &mut impl Words) -> Result<(), Error> {
        match &self.phase {
            Phase::Working => {
                let (permille, file) = match (&self.saver, &self.job) {
                    (Some(saver), _) => saver.progress(),
                    (None, Some(job)) => job.progress(),
                    (None, None) => (0, ""),
                };
                let key = if self.saver.is_some() {
                    "vocabulary.sd.saving"
                } else {
                    "vocabulary.sd.reading"
                };
                let title = words.format(
                    key,
                    &[Arg::Text {
                        name: "file",
                        value: file,
                    }],
                )?;
                let mut digits = [0u8; 20];
                let mut value = String::new();
                value.try_reserve(8).map_err(|_| Error::NoMemory)?;
                value.push_str(text::decimal(usize::from(permille / 10), &mut digits));
                value.push_str(" %");
                self.list = list_of([Item::row(&title, &value)?])?;
            }
            Phase::Asking(question) => {
                let ask = words.format(
                    "vocabulary.sd.ask",
                    &[
                        Arg::Text {
                            name: "deck",
                            value: &question.deck,
                        },
                        Arg::Text {
                            name: "file",
                            value: &question.file,
                        },
                    ],
                )?;
                let focus = self.list.focus();
                self.list = list_of([
                    Item::heading(&ask)?,
                    Item::row(&words.copy("vocabulary.sd.ask.link")?, GO)?,
                    Item::row(&words.copy("vocabulary.sd.ask.new")?, GO)?,
                ])?;
                // Rows 1 and 2 are the choices; the heading takes no focus.
                self.list.set_focus(focus.max(1));
            }
            Phase::Done => {}
        }
        Ok(())
    }
}

/// Whether progress at `permille` of `file` differs from what `shown` says
/// (in tens of per mille and the file), and so is drawn; `shown` is then what
/// is drawn.
fn worth_drawing(shown: &mut (u16, String), permille: u16, file: &str) -> Result<bool, Error> {
    if shown.0 == permille / 10 && shown.1 == file {
        return Ok(false);
    }
    let name = copy(file)?;
    *shown = (permille / 10, name);
    Ok(true)
}

fn number<'a>(name: &'a str, value: usize) -> Arg<'a> {
    Arg::Int {
        name,
        value: i64::try_from(value).unwrap_or(i64::MAX),
    }
}

/// What a file's report says.
fn report_rows(
    words: &mut impl Words,
    report: &FileReport,
    rows: &mut Vec<(String, String)>,
) -> Result<(), Error> {
    let title = copy(if report.deck.is_empty() {
        &report.file
    } else {
        &report.deck
    })?;
    let note = match report.outcome {
        Outcome::Merged => words.format(
            if report.removed > 0 {
                "vocabulary.sd.result.removed"
            } else {
                "vocabulary.sd.result"
            },
            &[
                number("added", report.added),
                number("changed", report.changed),
                number("removed", report.removed),
                number("same", report.same),
            ],
        )?,
        Outcome::Same => words.copy("vocabulary.sd.outcome.same")?,
        Outcome::Unreadable => words.copy("vocabulary.sd.outcome.unreadable")?,
        Outcome::TooManyDecks => words.copy("vocabulary.sd.outcome.too-many")?,
        Outcome::Crowded => words.copy("vocabulary.sd.outcome.crowded")?,
        Outcome::NoCards => words.copy("vocabulary.sd.outcome.no-cards")?,
        Outcome::TooLarge => words.copy("vocabulary.sd.outcome.too-large")?,
        Outcome::Utf16 => words.copy("vocabulary.sd.outcome.utf16")?,
        Outcome::NoNotes => words.copy("vocabulary.sd.outcome.no-notes")?,
        Outcome::NewerAnki => words.copy("vocabulary.sd.outcome.newer-anki")?,
        Outcome::DeckTooLarge => words.copy("vocabulary.sd.outcome.deck-too-large")?,
    };
    push(rows, title, note)?;
    if report.removal_held {
        push(
            rows,
            String::new(),
            words.copy("vocabulary.sd.removal-held")?,
        )?;
    }
    if report.no_room > 0 {
        let note = words.format("vocabulary.sd.no-room", &[count(report.no_room)])?;
        push(rows, String::new(), note)?;
    }
    if report.skipped > 0 {
        let key = if crate::apkg::is_apkg(&report.file) {
            "vocabulary.sd.skipped-notes"
        } else {
            "vocabulary.sd.skipped"
        };
        push(
            rows,
            words.format(key, &[count(report.skipped)])?,
            String::new(),
        )?;
    }
    for (line, fault) in &report.problems {
        let key = match fault {
            Fault::NoFront => "vocabulary.sd.fault.no-front",
            Fault::NotUtf8 => "vocabulary.sd.fault.not-utf8",
            Fault::TooLong => "vocabulary.sd.fault.too-long",
            Fault::OpenQuote => "vocabulary.sd.fault.open-quote",
        };
        let title = words.format("vocabulary.sd.line", &[number("line", *line as usize)])?;
        push(rows, title, words.copy(key)?)?;
    }
    Ok(())
}

fn count<'a>(value: usize) -> Arg<'a> {
    Arg::Count(i64::try_from(value).unwrap_or(i64::MAX))
}

fn push(rows: &mut Vec<(String, String)>, title: String, note: String) -> Result<(), Error> {
    rows.try_reserve(1).map_err(|_| Error::NoMemory)?;
    rows.push((title, note));
    Ok(())
}

/// What a save said, as rows.
fn save_rows(
    words: &mut impl Words,
    report: &export::Report,
    rows: &mut Vec<(String, String)>,
) -> Result<(), Error> {
    let file = Arg::Text {
        name: "file",
        value: &report.file,
    };
    let note = match report.outcome {
        export::Outcome::Saved => {
            words.format("vocabulary.sd.saved", &[count(report.cards), file])?
        }
        export::Outcome::NotLinked => words.format("vocabulary.sd.saved.unlinked", &[file])?,
        export::Outcome::NoDeck => words.copy("vocabulary.sd.save.no-deck")?,
        export::Outcome::Unreadable => words.copy("vocabulary.sd.save.unreadable")?,
        export::Outcome::Full => words.copy("vocabulary.sd.save.full")?,
        export::Outcome::TooLarge => words.copy("vocabulary.sd.save.too-large")?,
        export::Outcome::Failed => words.copy("vocabulary.sd.save.failed")?,
        export::Outcome::LeftOut => words.copy("vocabulary.sd.save.left-out")?,
        export::Outcome::AnkiFile => words.format("vocabulary.sd.save.anki-refused", &[file])?,
    };
    push(rows, copy(&report.deck)?, note)?;
    if let Some(backup) = &report.backup
        && matches!(
            report.outcome,
            export::Outcome::Saved | export::Outcome::NotLinked
        )
    {
        let note = words.format(
            "vocabulary.sd.save.backup",
            &[Arg::Text {
                name: "file",
                value: backup,
            }],
        )?;
        push(rows, String::new(), note)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_sd_tests.rs"]
mod tests;
