//! *Go to the book* for a word held on the list (interface 6: the option
//! `go-to`, offered where the book is on the card): the book opens at the
//! word's place, by `command.open` into `core.reader` (the reader the user bound
//! there) with the book's path and the place's section and offset
//! (`docs/abi/document.md`, *Opening at a place*).

use paperpaw_sdk::native::commands::CommandSender;
use paperpaw_sdk::native::files::Files;
use paperpaw_sdk::native::storage::Root;
use paperpaw_sdk::native::ui::{Open, Param, Ui};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::mem::buffer;
use paperpaw_ui_kit::text;

use crate::store::Word;

/// The handover parameters of `core.reader`.
const PATH: &str = "path";
const SECTION: &str = "section";
const OFFSET: &str = "offset";
/// The buffers of a call.
const BUFFER: usize = 4096;

/// Whether the book of `word` is on the card (a word kept without its book's
/// path, or a book since taken off, has none to go to).
///
/// # Errors
/// [`Error::NoMemory`] only: every other refusal of the host counts as no book.
pub(crate) fn book_there(host: &Host<'_>, word: &Word) -> Result<bool, Error> {
    if word.path.is_empty() {
        return Ok(false);
    }
    let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
    let files = Files::open(host)?;
    let found = files.stat(host, Root::Files, &word.path, &mut input, &mut out);
    let there = match found {
        Ok(entry) => Ok(!entry.is_folder()),
        // A missing file or a path the host does not take is no book of this card.
        Err(Error::NoMemory) => Err(Error::NoMemory),
        // Any other refusal (card full, busy, …) leaves the hold on the
        // delete question: no book to go to.
        Err(_) => Ok(false),
    };
    let released = files.release(host);
    let there = there?;
    released?;
    Ok(there)
}

/// Opens the book of `word` at the word's place.
///
/// # Errors
/// The host's refusals.
pub(crate) fn open_book(host: &Host<'_>, ui: &Ui, word: &Word) -> Result<(), Error> {
    let (mut section, mut offset) = ([0u8; 20], [0u8; 20]);
    let params = [
        Param::new(PATH, &word.path)?,
        Param::new(SECTION, text::decimal(word.section as usize, &mut section))?,
        Param::new(OFFSET, text::decimal(word.offset as usize, &mut offset))?,
    ];
    let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0u8; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
}
