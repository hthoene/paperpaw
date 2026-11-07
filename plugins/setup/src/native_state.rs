//! What the setup remembers and what it asks of the library: two marks in the
//! plugin's data (the guide is done or skipped; the guide was opened and its
//! mark may not be writable) and whether a book was ever opened.
use alloc::vec;
use paperpaw_sdk::native::{
    Error, Host,
    library::{Library, Query, Show},
    storage::{self, Root},
};

/// The file whose being there says the setup was done or skipped.
const DONE: &str = "done";
/// The file written before the guide opens: where it cannot be, the done mark
/// could not be either.
const TRIAL: &str = "trial";

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

/// Whether a book was opened before: the setup does not come to a device that
/// was used already.
pub fn has_opened_book(host: &Host<'_>) -> Result<bool, Error> {
    let library = Library::open(host)?;
    let found = opened(host, &library);
    let released = library.release(host);
    let found = found?;
    released?;
    Ok(found)
}

fn opened(host: &Host<'_>, library: &Library) -> Result<bool, Error> {
    let mut input = buffer(512)?;
    let mut output = buffer(4096)?;
    for show in [Show::Reading, Show::Finished] {
        let page = library.query(host, &Query::new().show(show), &mut input, &mut output)?;
        if page.end().total() > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn buffer(len: usize) -> Result<alloc::vec::Vec<u8>, Error> {
    let mut bytes = vec::Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}
