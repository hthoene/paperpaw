//! The reader at the start of the device: the book read last is opened again
//! when the setting `reader.open-last` asks for it (Beta 3's `reopen_last_book`).
//!
//! The book is the one `state.ini` keeps (`native_state`, written when a book
//! is opened); a book gone from the card is not opened, so the start screen
//! stays. The book opens through a hand-over with its path, in the reader the
//! user bound to `core.reader`, as the home screen opens a book.

use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    files::Files,
    settings::{self, SettingValue},
    storage::{DataStorage, Root},
    ui::{Open, Param, Ui},
};
use paperpaw_ui_kit::mem::buffer;

use crate::native_state;

/// The setting that asks for the last book to open at the start.
pub const OPEN_LAST: &str = "reader.open-last";

/// Opens the book read last, if the setting asks for it and the book is still
/// on the card. Nothing to open is not an error.
///
/// # Errors
/// `NoMemory` and the host's refusals.
pub fn reopen_last(host: &Host<'_>, ui: &Ui) -> Result<(), Error> {
    let mut bytes = [0; 32];
    if !matches!(
        settings::read_setting(host, OPEN_LAST, &mut bytes)?,
        SettingValue::Bool(true)
    ) {
        return Ok(());
    }
    let Some(path) = last_book(host)? else {
        return Ok(());
    };
    if !on_the_card(host, &path)? {
        return Ok(());
    }
    // A path of at most 512 bytes (`MAX_PATH`) and its key in; the host asks for room
    // for the largest answer of the operation (`max_reply` 4,096), though the handle is 8.
    let (mut input, mut out) = (buffer(1280)?, buffer(4096)?);
    let params = [Param::new("path", &path)?];
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
}

/// The book `state.ini` names, if it has one.
fn last_book(host: &Host<'_>) -> Result<Option<alloc::string::String>, Error> {
    let storage = DataStorage::new(host);
    let data = match paperpaw_sdk::read_file(&storage, native_state::STATE, native_state::MAX_STATE)
    {
        Ok(data) => data,
        // No file, or one too large to be the reader's: no last book.
        Err(_) => return Ok(None),
    };
    match core::str::from_utf8(&data) {
        Ok(state) => native_state::last_in(state),
        Err(_) => Ok(None),
    }
}

/// Whether the file at `path` is on the card (a book moved or removed is not).
fn on_the_card(host: &Host<'_>, path: &str) -> Result<bool, Error> {
    let files = Files::open(host)?;
    // The stat answers up to `max_reply` (4,096 bytes) and the host asks for room for it.
    let (mut input, mut out) = (buffer(1024)?, buffer(4096)?);
    let found = match files.stat(host, Root::Files, path, &mut input, &mut out) {
        Ok(_) => Ok(true),
        Err(Error::NotFound | Error::Invalid | Error::NoSpace) => Ok(false),
        Err(error) => Err(error),
    };
    files.release(host).ok();
    found
}

/// Keeps `path` as the book read last, for the next start (`state.ini`, as
/// Beta 3 did when a book of the library opened). A file that cannot be written
/// is not told: the last book is then the one before.
pub fn remember(host: &Host<'_>, path: &str) {
    let Ok(text) = native_state::state_text(path) else {
        return;
    };
    let storage = DataStorage::new(host);
    // The same book as last time: the file is left as it is (no wear on the card).
    if paperpaw_sdk::read_file(&storage, native_state::STATE, native_state::MAX_STATE)
        .is_ok_and(|kept| kept == text.as_bytes())
    {
        return;
    }
    paperpaw_sdk::Storage::write(&storage, native_state::STATE, text.as_bytes()).ok();
}

/// A book moved (`event.book_moved`): the last book read follows it, as Beta 3's
/// `book_moved` did. Best effort, like [`remember`].
pub fn follow(host: &Host<'_>, from: &str, to: &str) {
    if last_book(host).ok().flatten().as_deref() == Some(from) {
        remember(host, to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The state a move leaves: the new path, read back as the last book.
    #[test]
    fn a_moved_last_book_is_read_back_at_its_new_place() {
        let kept = native_state::state_text("books/a.epub").unwrap();
        assert_eq!(
            native_state::last_in(&kept).unwrap().as_deref(),
            Some("books/a.epub")
        );
        let moved = native_state::state_text("books/new/a.epub").unwrap();
        assert_eq!(
            native_state::last_in(&moved).unwrap().as_deref(),
            Some("books/new/a.epub")
        );
    }
}
