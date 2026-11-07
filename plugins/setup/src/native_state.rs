//! What the setup remembers: marks in the plugin's data. The guide is done or
//! skipped (`done`); the guide was opened and its mark may not be writable
//! (`trial`); a book was opened on this device (`opened`), so a device used
//! before the guide came is not given it.
use paperpaw_sdk::native::{
    Error, Host,
    storage::{self, Root},
};

/// The file whose being there says the setup was done or skipped.
const DONE: &str = "done";
/// The file written before the guide opens: where it cannot be, the done mark
/// could not be either.
const TRIAL: &str = "trial";
/// The file whose being there says a book was opened.
const OPENED: &str = "opened";

/// Whether the file `name` is there.
fn exists(host: &Host<'_>, name: &str) -> Result<bool, Error> {
    match storage::size(host, Root::Data, name) {
        Ok(_) => Ok(true),
        Err(Error::NotFound) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Whether the setup was done or skipped.
pub fn is_done(host: &Host<'_>) -> Result<bool, Error> {
    exists(host, DONE)
}

/// Notes that the guide is about to open: `false` where the storage cannot be
/// written (the guide would come at every start with its choices lost).
pub fn try_trial(host: &Host<'_>) -> bool {
    storage::write(host, Root::Data, TRIAL, b"").is_ok()
}

/// Notes that the setup was done or skipped. `false` where that cannot be
/// written: the reader is told then.
pub fn mark_done(host: &Host<'_>) -> bool {
    if storage::write(host, Root::Data, DONE, b"").is_err() {
        return false;
    }
    let _ = storage::remove(host, Root::Data, TRIAL);
    true
}

/// Whether a book was opened on this device since the setup began to note it.
pub fn has_opened_book(host: &Host<'_>) -> Result<bool, Error> {
    exists(host, OPENED)
}

/// Notes the first book opened, once.
pub fn note_book_opened(host: &Host<'_>) -> Result<(), Error> {
    if exists(host, OPENED)? {
        return Ok(());
    }
    storage::write(host, Root::Data, OPENED, b"")
}
