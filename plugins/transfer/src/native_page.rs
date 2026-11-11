//! The web page with its texts in the device's language, written into the
//! plugin's data folder for `core.net` `respond` to send (an answer made of
//! memory holds 2,048 bytes; the page is some 12 KiB). The texts, the device's
//! name and the library folder go where the page names them
//! (`/*STRINGS*/{}`), with the extensions the formats read
//! (`core.document`, for the hint where a book is sent outside the library
//! folder). The page shows *Move* and *Rename* (it hides them for a `noMove`,
//! which nothing sets since the library follows moves: `PORTING-7.md`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    document::{Documents, FormatCursor},
    storage::Root,
};
use paperpaw_ui_kit::{
    Messages,
    files::replace,
    mem::{SERVICE_BUFFER, buffer},
    style::setting_text,
    text::copy,
};

use crate::native_web::{PAGE_FILE, push_json};

const PAGE: &str = include_str!("../data/page.html");
const PLACEHOLDER: &str = "/*STRINGS*/{}";
const DEVICE_NAME: &str = "core.device-name";
pub(crate) const LIBRARY: &str = "core.library-folder";
/// The name of a device that has none set.
const FALLBACK: &str = "PaperPaw";
/// Pages of formats read at most, and extensions kept at most.
const MOST_PAGES: usize = 8;
const MOST_EXTENSIONS: usize = 128;

/// The page, finished: `head` before the strings, then the strings, then the rest.
///
/// # Errors
/// `NoMemory`, or what the texts and settings refuse.
pub fn build(host: &Host<'_>) -> Result<Vec<u8>, Error> {
    let mut messages = Messages::new()?;
    let mut json = String::new();
    json.try_reserve(2_048).map_err(|_| Error::NoMemory)?;
    json.push('{');
    for (i, (name, key)) in crate::PAGE_TEXTS.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(name);
        json.push(':');
        push_json(&mut json, &messages.copy(host, key)?)?;
    }
    let name = device_name(host)?;
    let library = setting_text(host, LIBRARY)?;
    join(&mut json, ",name:", &name)?;
    join(&mut json, ",library:", library.trim_matches('/'))?;
    json.try_reserve(32).map_err(|_| Error::NoMemory)?;
    json.push_str(",books:[");
    for (i, extension) in book_extensions(host)?.iter().enumerate() {
        if i > 0 {
            json.try_reserve(1).map_err(|_| Error::NoMemory)?;
            json.push(',');
        }
        push_json(&mut json, extension)?;
    }
    json.try_reserve(16).map_err(|_| Error::NoMemory)?;
    json.push_str("]}");
    assemble(&json)
}

/// The extensions the formats of the device read, each once. Where the
/// formats cannot be asked for the list is empty and the page does not show
/// its hint.
pub(crate) fn book_extensions(host: &Host<'_>) -> Result<Vec<String>, Error> {
    let documents = match Documents::open(host) {
        Ok(documents) => documents,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(Vec::new()),
    };
    let read = read_extensions(host, &documents);
    let released = documents.release(host);
    let found = read?;
    released?;
    Ok(found)
}

fn read_extensions(host: &Host<'_>, documents: &Documents) -> Result<Vec<String>, Error> {
    let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
    let mut found: Vec<String> = Vec::new();
    let mut cursor = FormatCursor::start();
    for _ in 0..MOST_PAGES {
        let page = match documents.formats(host, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // Not known: no hint, never a failed page.
            Err(_) => return Ok(Vec::new()),
        };
        for format in page.rows() {
            for extension in format.extensions() {
                if found.len() < MOST_EXTENSIONS && !found.iter().any(|known| known == extension) {
                    found.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    found.push(copy(extension)?);
                }
            }
        }
        match page.end().next() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(found)
}

/// The device's name, as the page and the reader's own network call it.
///
/// # Errors
/// `NoMemory`, or what the settings refuse.
pub fn device_name(host: &Host<'_>) -> Result<String, Error> {
    let name = setting_text(host, DEVICE_NAME)?;
    if name.is_empty() {
        copy(FALLBACK)
    } else {
        Ok(name)
    }
}

fn join(json: &mut String, name: &str, text: &str) -> Result<(), Error> {
    json.try_reserve(name.len()).map_err(|_| Error::NoMemory)?;
    json.push_str(name);
    push_json(json, text)
}

/// The page with `json` where the strings go.
fn assemble(json: &str) -> Result<Vec<u8>, Error> {
    let at = PAGE.find(PLACEHOLDER).ok_or(Error::Invalid)?;
    let (head, tail) = (&PAGE[..at], &PAGE[at + PLACEHOLDER.len()..]);
    let mut page = Vec::new();
    page.try_reserve_exact(head.len() + json.len() + tail.len())
        .map_err(|_| Error::NoMemory)?;
    page.extend_from_slice(head.as_bytes());
    page.extend_from_slice(json.as_bytes());
    page.extend_from_slice(tail.as_bytes());
    Ok(page)
}

/// Writes the page for the requests of this time of serving.
///
/// # Errors
/// What the texts, the settings or the storage refuse.
pub fn write(host: &Host<'_>) -> Result<(), Error> {
    let page = build(host)?;
    replace(host, Root::Data, PAGE_FILE, &page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strings_go_where_the_page_names_them_and_nowhere_else() {
        let page = assemble("{a:1}").unwrap();
        let page = core::str::from_utf8(&page).unwrap();
        assert!(page.contains("const T = {a:1};"));
        assert!(!page.contains(PLACEHOLDER));
        assert_eq!(page.len(), PAGE.len() - PLACEHOLDER.len() + 5);
    }

    /// Every text the page reads is one the page names, and the page
    /// does not read a text that is not given (`T.name`, `T.library` and `T.books`
    /// are the three the plugin adds; `T.noMove` is never set, so the page
    /// shows *Move* and *Rename*).
    #[test]
    fn every_text_the_page_reads_is_given() {
        let mut read: Vec<&str> = Vec::new();
        for part in PAGE.split("T.").skip(1) {
            let end = part
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(part.len());
            read.push(&part[..end]);
        }
        let given: Vec<&str> = crate::PAGE_TEXTS.iter().map(|(name, _)| *name).collect();
        for name in read {
            assert!(
                given.contains(&name) || ["name", "library", "books", "noMove"].contains(&name),
                "{name}"
            );
        }
    }
}
