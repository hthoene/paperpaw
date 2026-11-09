//! The pictures folder on the card, read through `core.files`: the first
//! pictures by name ([`Pictures`]), for the screen's folder picture and the
//! picture over the page and for the picker of that picture.
use crate::native_sleep::Pictures;
use paperpaw_sdk::native::{Error, Host, files::Files, storage::Root};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// Pages of the folder listing read at most.
const MOST_PAGES: usize = 8;

/// The pictures of `folder` on the card (the first of them by name). A folder
/// that is not there, a file, or a card this plugin may not read has none.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn list_folder(host: &Host<'_>, folder: &str) -> Result<Pictures, Error> {
    let files = match Files::open(host) {
        Ok(files) => files,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(Pictures::default()),
    };
    let found = read_folder(host, &files, folder);
    let released = files.release(host);
    let found = found?;
    released.ok();
    Ok(found)
}

fn read_folder(host: &Host<'_>, files: &Files, folder: &str) -> Result<Pictures, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut kept = Pictures::default();
    let mut cursor = 0;
    for _ in 0..MOST_PAGES {
        let page = match files.list(host, Root::Files, folder, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // No folder, a file, no consent for the card: no pictures.
            Err(_) => break,
        };
        for entry in page.entries() {
            kept.take(entry.name(), entry.is_folder())?;
        }
        match page.next() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(kept)
}
