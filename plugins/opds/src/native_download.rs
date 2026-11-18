//! A book downloaded from a catalog: its file name (authors and title, else the
//! name in its address), the check that a file is the book it says (a page a
//! server answers with instead, a sign-in or an error, is not), and its
//! import into the library (`Library::import`, a free name, so no book of the
//! reader's is replaced). A book comes into a file of its own and goes into the
//! library only once it is whole, so a download stopped or failed leaves
//! nothing there.
use alloc::string::String;

use paperpaw_sdk::{
    native::{
        Error, Host,
        library::{Existing, Import, Library},
    },
    web::{try_clean_name, try_file_name},
};
use paperpaw_ui_kit::{
    mem::buffer,
    text::{concat, copy},
};

use crate::native_files::Files;
use crate::native_model::Entry;

/// The longest part of a file name made of a title, in UTF-16 units.
const NAME_UNITS: usize = 120;

/// The file name for a book: its authors and title, else the name in its
/// address.
///
/// # Errors
/// `NoMemory`.
pub fn book_name(entry: &Entry, url: &str, ext: &str) -> Result<String, Error> {
    let title = entry.title.trim();
    let author = entry
        .authors
        .split(" & ")
        .next()
        .map(str::trim)
        .unwrap_or_default();
    let name = if !author.is_empty() && !title.is_empty() {
        concat(&[author, " - ", title])?
    } else {
        copy(title)?
    };
    match try_clean_name(&name, NAME_UNITS).map_err(|_| Error::NoMemory)? {
        Some(name) => concat(&[&name, ".", ext]),
        None => {
            let name = try_file_name(url).map_err(|_| Error::NoMemory)?;
            let tail = concat(&[".", ext])?;
            if name.ends_with(tail.as_str()) {
                Ok(name)
            } else {
                concat(&[&name, &tail])
            }
        }
    }
}

/// Whether the file `path`, downloaded as `name`, may be the book its name
/// says, by its first bytes. Formats told by a mark need it (a ZIP's for EPUB,
/// CBZ and ODT; PDF's, RTF's, Mobipocket's); text formats only must not be a
/// web page.
///
/// # Errors
/// `NoMemory`.
pub fn may_be_book(files: &dyn Files, path: &str, name: &str) -> Result<bool, Error> {
    let mut head = buffer(68)?;
    let Ok(read) = files.read_at(path, 0, &mut head) else {
        return Ok(false);
    };
    let head = head.get(..read).unwrap_or_default();
    let extension = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    let is = |ext: &str| extension.eq_ignore_ascii_case(ext);
    Ok(if is("epub") || is("cbz") || is("odt") {
        head.starts_with(b"PK")
    } else if is("pdf") {
        head.starts_with(b"%PDF")
    } else if is("rtf") {
        head.starts_with(b"{\\rtf")
    } else if is("mobi") || is("azw3") {
        head.get(60..68) == Some(b"BOOKMOBI")
    } else if is("html") || is("htm") {
        !head.is_empty()
    } else {
        !head.is_empty() && !is_web_page(head)
    })
}

/// Whether `head`, the start of a file, begins a web page.
fn is_web_page(head: &[u8]) -> bool {
    let head = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    let start = head
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(head.len());
    let head = head.get(start..).unwrap_or_default();
    let begins = |mark: &[u8]| {
        head.get(..mark.len())
            .is_some_and(|h| h.eq_ignore_ascii_case(mark))
    };
    begins(b"<!doctype html") || begins(b"<html")
}

/// Puts the downloaded file `from` into the library as `name` in `folder`
/// (below the library folder; the host makes it). The file moves where the
/// library shares the card, else it is copied and removed. With `Existing`
/// the name taken is a `State` error (`Refuse`), a free name (`FreeName`).
/// Returns the book's path.
///
/// # Errors
/// `NoMemory`; what the library says (`State` for a name that exists under
/// `Refuse`, `Full`, `Invalid` for a name the card does not take, `Busy`).
pub fn import(
    host: &Host<'_>,
    library: &Library,
    from: &str,
    folder: &str,
    name: &str,
    existing: Existing,
) -> Result<String, Error> {
    let (mut input, mut out) = (buffer(2048)?, buffer(4096)?);
    let file = Import::new(from, name).folder(folder).existing(existing);
    let path = library.import(host, &file, &mut input, &mut out)?;
    copy(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    #[test]
    fn books_are_named_after_author_and_title() {
        let entry = Entry {
            title: String::from("Pride and Prejudice"),
            authors: String::from("Austen, Jane & Someone Else"),
            ..Entry::default()
        };
        assert_eq!(
            book_name(&entry, "https://a.org/1342.epub.noimages", "epub").unwrap(),
            "Austen, Jane - Pride and Prejudice.epub"
        );
        let untitled = Entry::default();
        assert_eq!(
            book_name(&untitled, "https://a.org/get/1342", "epub").unwrap(),
            "1342.epub"
        );
        assert_eq!(
            book_name(&untitled, "https://a.org/b.epub", "epub").unwrap(),
            "b.epub"
        );
    }

    #[test]
    fn a_hostile_title_is_a_safe_name() {
        let entry = Entry {
            title: String::from("../../etc/passwd: <b>\u{0}x</b>?"),
            authors: String::from("A/B"),
            ..Entry::default()
        };
        let name = book_name(&entry, "https://a.org/x", "epub").unwrap();
        assert!(
            !name.contains('/') && !name.contains(':') && !name.contains('<'),
            "{name}"
        );
        assert!(name.ends_with(".epub"));
        let long = Entry {
            title: "é".repeat(500),
            ..Entry::default()
        };
        let name = book_name(&long, "https://a.org/x", "epub").unwrap();
        assert!(name.encode_utf16().count() <= NAME_UNITS + 5);
    }
}
