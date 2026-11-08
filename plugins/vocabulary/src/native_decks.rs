//! The flash cards' first screen on interface 7 (interface 6: `decks.rs`):
//! *Review* with how many cards are due, *Edit with your phone*, *Cards on
//! the SD card* with what its folder holds, then the decks with how many
//! cards each has. A deck chosen is reviewed alone.
//!
//! The view is made from owned data (the numbers are the shell's to give:
//! what is due depends on the clock and the settings), so the rows are made
//! and tested over [`Words`] without a host. Choosing a row opens another
//! screen with a hand-over; those screens, and this one's registration, wait
//! for their ports (see `PORTING-7.md`).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::commands::CommandSender;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::ui::{Open, Param, Ui};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Handled, Storage, StorageError};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep, text};

use crate::cards;
use crate::native_words::{HostWords, Words};
use crate::sync::Pending;

/// This screen's id.
pub(crate) const SCREEN: &str = "vocabulary.cards";
/// The review screen's id, and the hand-over key that names a deck.
const REVIEW: &str = "vocabulary.review";
const DECK: &str = "deck";
/// The phone page's and the SD card menu's ids.
const PHONE: &str = "vocabulary.phone";
const SD_MENU: &str = "vocabulary.sd-menu";

/// What the flash cards' screen knows, as numbers and names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Numbers {
    /// Cards due now, and new ones to show today.
    pub due: usize,
    /// The decks in the order they were made, with how many cards each has.
    pub decks: Vec<(u32, String, usize)>,
    /// The SD card's folder (its name as shown) and what it holds.
    pub folder: String,
    pub pending: Pending,
}

impl Numbers {
    /// The decks and their sizes over `storage`; `due`, `folder` and
    /// `pending` are the shell's.
    ///
    /// # Errors
    /// What reading the decks says (a refused allocation is
    /// [`StorageError::TooLarge`]).
    pub(crate) fn read(
        storage: &dyn Storage,
        due: usize,
        folder: &str,
        pending: Pending,
    ) -> Result<Self, StorageError> {
        let sizes = cards::sizes(storage)?;
        let list = cards::decks(storage)?;
        let mut decks = Vec::new();
        decks
            .try_reserve_exact(list.len())
            .map_err(|_| StorageError::TooLarge)?;
        for deck in list {
            let n = sizes.get(&deck.id).copied().unwrap_or(0);
            decks.push((deck.id, deck.name, n));
        }
        Ok(Self {
            due,
            decks,
            folder: text::copy(folder).map_err(|_| StorageError::TooLarge)?,
            pending,
        })
    }
}

/// What a row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// Review the cards due, of all decks.
    Review,
    /// Review one deck.
    Deck(u32),
    /// The phone page.
    Phone,
    /// The SD card's menu.
    Files,
}

/// What the row of the SD card's files says: what there is to do.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn files_note(
    words: &mut impl Words,
    folder: &str,
    files: Pending,
) -> Result<String, Error> {
    let name = Arg::Text {
        name: "folder",
        value: folder,
    };
    if !files.folder {
        words.format("vocabulary.sd.row.none", &[name])
    } else if files.todo > 0 {
        words.format(
            "vocabulary.sd.row.todo",
            &[Arg::Count(files.todo as i64), name],
        )
    } else if files.failed > 0 {
        words.format(
            "vocabulary.sd.row.failed",
            &[Arg::Count(files.failed as i64), name],
        )
    } else if files.files == 0 {
        words.format("vocabulary.sd.row.empty", &[name])
    } else {
        words.format("vocabulary.sd.row.same", &[name])
    }
}

/// The rows: review, the phone, the SD card, then a row for each deck, and
/// what each does.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn rows(
    words: &mut impl Words,
    numbers: &Numbers,
) -> Result<(Vec<Item>, Vec<Target>), Error> {
    let mut items = Vec::new();
    let mut targets = Vec::new();
    let room = 3 + numbers.decks.len();
    items.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
    targets
        .try_reserve_exact(room)
        .map_err(|_| Error::NoMemory)?;
    let due = words.format("vocabulary.cards.due", &[Arg::Count(numbers.due as i64)])?;
    items.push(Item::noted(
        &words.copy("vocabulary.cards.review")?,
        "›",
        &due,
    )?);
    targets.push(Target::Review);
    let phone = if numbers.decks.is_empty() {
        "vocabulary.cards.phone.first"
    } else {
        "vocabulary.cards.phone.help"
    };
    items.push(Item::noted(
        &words.copy("vocabulary.cards.phone")?,
        "›",
        &words.copy(phone)?,
    )?);
    targets.push(Target::Phone);
    let files = files_note(words, &numbers.folder, numbers.pending)?;
    items.push(Item::noted(&words.copy("vocabulary.sd.row")?, "›", &files)?);
    targets.push(Target::Files);
    for (id, name, cards) in &numbers.decks {
        let count = words.format("vocabulary.cards.count", &[Arg::Count(*cards as i64)])?;
        items.push(Item::noted(name, "›", &count)?);
        targets.push(Target::Deck(*id));
    }
    Ok((items, targets))
}

/// The flash cards' screen.
pub(crate) struct DecksView {
    list: ItemList,
    targets: Vec<Target>,
}

impl DecksView {
    /// The view of `numbers`, in the language the host's catalog has.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn new(host: &Host<'_>, numbers: &Numbers) -> Result<Self, Error> {
        Self::build(&mut HostWords::new(host)?, numbers)
    }

    /// As [`Self::new`], with the texts given.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn build(words: &mut impl Words, numbers: &Numbers) -> Result<Self, Error> {
        let (items, targets) = rows(words, numbers)?;
        Ok(Self {
            list: ItemList::new(items),
            targets,
        })
    }

    /// What the row `index` does.
    pub(crate) fn target(&self, index: usize) -> Option<Target> {
        self.targets.get(index).copied()
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        self.list
            .draw_with(host, frame, "vocabulary.cards.title", true, Footer::None)
    }

    /// A chosen row opens its screen.
    pub(crate) fn input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let step = self.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step
            && let Some(target) = self.target(index)
        {
            open(host, ui, target)?;
        }
        Ok(ItemList::handled(step))
    }
}

/// The screen a target opens, and the hand-over key and value it takes.
pub(crate) fn screen_of(target: Target) -> (&'static str, Option<u32>) {
    match target {
        Target::Review => (REVIEW, None),
        Target::Deck(id) => (REVIEW, Some(id)),
        Target::Phone => (PHONE, None),
        Target::Files => (SD_MENU, None),
    }
}

/// Opens the screen of `target`; a deck is named in the hand-over.
///
/// # Errors
/// What the host says.
pub(crate) fn open(host: &Host<'_>, ui: &Ui, target: Target) -> Result<(), Error> {
    let (screen, deck) = screen_of(target);
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let mut open = Open::screen(screen)?;
    if let Some(deck) = deck {
        let mut digits = [0u8; 20];
        let id = text::decimal(deck as usize, &mut digits);
        open =
            open.handover(ui.handover(host, &[Param::new(DECK, id)?], &mut input, &mut output)?);
    }
    CommandSender::new(host, &mut input).open(open)
}

#[cfg(test)]
#[path = "native_decks_tests.rs"]
mod tests;
