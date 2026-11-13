//! The list of words looked up, on interface 7 (interface 6: `WordsScreen` in
//! `lib.rs`): newest first, each with the sentence it was found in (else the
//! book's title) as its note; holding a word asks whether to delete it.
//!
//! The list can hold 2,000 words, so only the rows of the page shown are made
//! ([`WordRows`], a [`RowSource`] over [`store::range`]); the page count is
//! known from the word count and the fixed row height. What interface 6 also
//! offered waits for its host pieces and is not offered (`PORTING-7.md`):
//! looking the word up again (the dictionary's panel on `core.lookup`). *Export*
//! is the row after the last word ([`crate::native_export`]); holding a word
//! whose book is on the card offers *Go to the book* ([`crate::native_goto`])
//! beside *Delete*.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::ui::Question;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Handled, Storage};
use paperpaw_ui_kit::window::{RowSource, WindowList};
use paperpaw_ui_kit::{Footer, ItemList, ItemStep, RowItem, text};

use crate::native_review::storage_error;
use crate::native_words::Words;
use crate::store::{self, Word};

/// The screen's id (relative to the plugin: `list`).
pub(crate) const SCREEN: &str = "vocabulary.list";

/// What the input of the list means.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The input is not for the list.
    Ignored,
    /// The list took it (the focus moved, a page was turned).
    Taken,
    /// A word is tapped or chosen: looking it up waits for the dictionary's
    /// panel, so the screen says how to delete it instead.
    Tapped,
    /// A word is held (or *Menu*): its row.
    Held(usize),
    /// *Export* below the words is chosen.
    Export,
}

/// The words as rows, newest first, one page at a time.
pub(crate) struct WordRows<'s> {
    storage: &'s dyn Storage,
    count: usize,
    /// The title and the note of the *Export* row below the words; none: no
    /// such row.
    export: Option<(String, String)>,
}

impl<'s> WordRows<'s> {
    pub(crate) fn new(storage: &'s dyn Storage, count: usize) -> Self {
        Self {
            storage,
            count,
            export: None,
        }
    }

    /// The same with the *Export* row below the words (where there are any).
    pub(crate) fn with_export(mut self, title: String, note: String) -> Self {
        self.export = Some((title, note));
        self
    }

    /// The *Export* row, if it is among the rows `first..first + max`: it is the
    /// one after the last word.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn export_row(&self, first: usize, max: usize) -> Result<Option<RowItem>, Error> {
        let Some((title, note)) = self.export.as_ref().filter(|_| self.count > 0) else {
            return Ok(None);
        };
        if first > self.count || first.saturating_add(max) <= self.count {
            return Ok(None);
        }
        Ok(Some(RowItem {
            title: text::copy(title)?,
            value: text::copy("›")?,
            note: text::copy(note)?,
            zone: None,
            stepper: false,
            disabled: false,
            on: None,
            field: false,
        }))
    }

    /// How many rows there are: the words, and *Export* below them.
    pub(crate) fn len_with_export(&self) -> usize {
        match (&self.export, self.count) {
            (Some(_), n) if n > 0 => n + 1,
            _ => self.count,
        }
    }

    /// The rows `first..first + max`: row `r` is the word numbered
    /// `count - 1 - r` in the file.
    ///
    /// # Errors
    /// What reading the words says, by [`storage_error`].
    pub(crate) fn rows(&self, first: usize, max: usize) -> Result<Vec<RowItem>, Error> {
        let last = first.saturating_add(max).min(self.count);
        if first >= last {
            return Ok(Vec::new());
        }
        let (from, to) = (self.count - last, self.count - first);
        let words = store::range(self.storage, from, to).map_err(storage_error)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(words.len())
            .map_err(|_| Error::NoMemory)?;
        // The file's order is oldest first: the list is the other way round.
        for (_, word) in words.iter().rev() {
            let note = if word.sentence.is_empty() {
                &word.title
            } else {
                &word.sentence
            };
            rows.push(RowItem {
                title: text::copy(&word.word)?,
                value: String::new(),
                note: text::copy(note)?,
                zone: None,
                stepper: false,
                disabled: false,
                on: None,
                field: false,
            });
        }
        Ok(rows)
    }

    /// The word of row `row`.
    ///
    /// # Errors
    /// What reading the words says, by [`storage_error`]; `NotFound` where
    /// the list changed and there is no such word.
    pub(crate) fn word(&self, row: usize) -> Result<Word, Error> {
        if row >= self.count {
            return Err(Error::NotFound);
        }
        let n = self.count - 1 - row;
        let mut words = store::range(self.storage, n, n + 1).map_err(storage_error)?;
        words.pop().map(|(_, word)| word).ok_or(Error::NotFound)
    }
}

impl RowSource for WordRows<'_> {
    fn len(&self) -> usize {
        self.len_with_export()
    }

    fn fill(
        &mut self,
        _host: &Host<'_>,
        first: usize,
        max: usize,
        out: &mut Vec<RowItem>,
    ) -> Result<(), Error> {
        let mut rows = self.rows(first, max)?;
        if let Some(row) = self.export_row(first, max)? {
            rows.try_reserve(1).map_err(|_| Error::NoMemory)?;
            rows.push(row);
        }
        *out = rows;
        Ok(())
    }
}

/// The question whether to delete `word`: its title, its text, and the two
/// answers (labels, one per line, *yes* first).
pub(crate) struct DeleteTexts {
    pub title: String,
    pub text: String,
    pub labels: String,
}

/// Deletes `word` and counts the words again: whether it was deleted, and how
/// many there are now (0 where they cannot be counted).
pub(crate) fn delete(storage: &dyn Storage, word: &Word) -> (bool, usize) {
    let deleted = store::remove(storage, word).is_ok();
    (deleted, store::count(storage).unwrap_or(0))
}

/// The texts of the question about deleting `word`.
///
/// # Errors
/// [`Error::NoMemory`] and what the catalog says.
pub(crate) fn delete_texts(words: &mut impl Words, word: &str) -> Result<DeleteTexts, Error> {
    let title = words.format(
        "vocabulary.delete.question",
        &[Arg::Text {
            name: "word",
            value: word,
        }],
    )?;
    let labels = text::concat(&[
        &words.copy("vocabulary.option.delete")?,
        "\n",
        &words.copy("vocabulary.delete.keep")?,
    ])?;
    Ok(DeleteTexts {
        title,
        text: words.copy("vocabulary.delete.text")?,
        labels,
    })
}

impl DeleteTexts {
    /// The question, asked as a yes-or-no with the two labels.
    ///
    /// # Errors
    /// What building the question says.
    pub(crate) fn question(&self) -> Result<Question<'_>, Error> {
        Question::confirm(&self.title, &self.text)?.confirm_labels(&self.labels)
    }
}

/// The list's screen.
#[derive(Debug)]
pub(crate) struct WordsView {
    list: WindowList,
    /// What the list shows when there is no word (and where its header's
    /// back arrow is).
    empty: ItemList,
    /// Words in all, as counted when the screen was shown.
    count: usize,
}

impl WordsView {
    /// A view of the `count` words there are.
    pub(crate) fn new(count: usize) -> Self {
        Self {
            list: WindowList::new(true),
            empty: ItemList::new(Vec::new()),
            count,
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// The words are counted again (one deleted, or looked up meanwhile); the
    /// focus stays on a row that is there.
    pub(crate) fn recount(&mut self, count: usize) {
        self.count = count;
        // The row after the last word is *Export*.
        self.list.set_focus(self.list.focus().min(count));
    }

    /// Draws the page the focus is on, or what there is to say with no word.
    ///
    /// # Errors
    /// Allocation, font, catalog, storage and drawing errors.
    pub(crate) fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        storage: &dyn Storage,
        words: &mut impl Words,
    ) -> Result<(), Error> {
        if self.count == 0 {
            let (title, text) = (
                words.copy("vocabulary.empty.title")?,
                words.copy("vocabulary.empty")?,
            );
            let footer = Footer::Empty {
                title: &title,
                text: &text,
            };
            return self
                .empty
                .draw_with(host, frame, "vocabulary.title", true, footer);
        }
        let (title, note) = (
            words.copy("vocabulary.export")?,
            words.copy("vocabulary.export.help")?,
        );
        let mut rows = WordRows::new(storage, self.count).with_export(title, note);
        self.list
            .draw(host, frame, "vocabulary.title", &mut rows, true)
    }

    /// What the input does to the list.
    ///
    /// # Errors
    /// What queueing a command says.
    pub(crate) fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Step, Error> {
        if self.count == 0 {
            return Ok(match self.empty.input(host, input)? {
                ItemStep::Ignored => Step::Ignored,
                _ => Step::Taken,
            });
        }
        // *Menu* is what holding a word is.
        if matches!(
            input,
            Input::Action {
                action: "core.menu",
                ..
            }
        ) {
            return Ok(self.held(self.list.focus()));
        }
        Ok(match self.list.input(host, self.count + 1, input)? {
            ItemStep::Ignored => Step::Ignored,
            ItemStep::Held(row) => self.held(row),
            ItemStep::Chosen(row) if row == self.count => Step::Export,
            ItemStep::Chosen(_) => Step::Tapped,
            _ => Step::Taken,
        })
    }
}

impl WordsView {
    /// A hold on `row`: a word's; the *Export* row has nothing to hold.
    fn held(&self, row: usize) -> Step {
        if row < self.count {
            Step::Held(row)
        } else {
            Step::Taken
        }
    }
}

/// `Handled` for a [`Step`] that is not a hold.
pub(crate) fn handled(step: &Step) -> Handled {
    if *step == Step::Ignored {
        Handled::No
    } else {
        Handled::Yes
    }
}

#[cfg(test)]
#[path = "native_list_tests.rs"]
mod tests;
