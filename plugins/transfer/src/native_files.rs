//! The card as the page's requests see it: `core.files` root 1 (the user's
//! files), and the request bodies of the data folder (root 0).
//!
//! Books go through the library (`core.library`) so what it keeps for them
//! (position, notes, fields) follows or goes with them: a book moved is
//! `Library::move_to`, a book deleted `Library::delete` (with its notes: the
//! user asked on the page), a book sent into the library folder
//! `Library::import` (catalogued at once). Everything else is `core.files`
//! (the host's library follows every move on root 1, `files.md` 2.6, and
//! forgets what is deleted). Where the library cannot be had the same calls
//! are `core.files`'.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::files::{has_extension, name as file_name, parent};
use paperpaw_sdk::native::{
    Error, Host,
    files::{self, Files},
    library::{Existing, Import, Library},
    storage::{self, Root},
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer},
    style::setting_text,
    text::copy,
};

use crate::native_page::{LIBRARY, book_extensions};
use crate::native_web::Card;

/// Pages of a folder read at most (32 entries each): well over a folder of
/// thousands of books.
const MOST_PAGES: usize = 4096;

/// A [`Card`] on the host, with its two buffers for the calls of one request.
pub struct HostCard<'a, 'h> {
    host: &'a Host<'h>,
    files: &'a Files,
    /// The library, where it can be had: without it books are files.
    library: Option<&'a Library>,
    input: Vec<u8>,
    out: Vec<u8>,
    /// The library's input (4,096 bytes), made when the library is first used.
    library_input: Vec<u8>,
}

impl<'a, 'h> HostCard<'a, 'h> {
    /// # Errors
    /// `NoMemory`.
    pub fn new(
        host: &'a Host<'h>,
        files: &'a Files,
        library: Option<&'a Library>,
    ) -> Result<Self, Error> {
        Ok(Self {
            host,
            files,
            library,
            input: buffer(files::INPUT_BYTES)?,
            out: buffer(files::OUTPUT_BYTES)?,
            library_input: Vec::new(),
        })
    }

    /// The library, with its input buffer made, for a call.
    fn library_ready(&mut self) -> Result<Option<&'a Library>, Error> {
        let Some(library) = self.library else {
            return Ok(None);
        };
        if self.library_input.is_empty() {
            self.library_input = buffer(SERVICE_BUFFER)?;
        }
        Ok(Some(library))
    }

    /// Whether the library takes the file at `path` for a book (the preflight
    /// of a delete says `OK` for a book and `Invalid` for any other file).
    /// What it cannot answer is a plain file.
    fn is_book(&mut self, path: &str) -> Result<bool, Error> {
        let host = self.host;
        let Some(library) = self.library_ready()? else {
            return Ok(false);
        };
        let (input, out) = (&mut self.library_input, &mut self.out);
        match library.can_delete(host, &[path], input, out) {
            Ok(report) => Ok(report.rows().next().is_some_and(|row| row.status().is_ok())),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(false),
        }
    }

    /// Puts the body in the library as a book where `path` is a book's name in
    /// the library folder or below it; `None` where it is not (or the library
    /// refuses the names: `Invalid`, nothing changed).
    fn import(&mut self, body: &str, path: &str) -> Result<Option<String>, Error> {
        let (host, name) = (self.host, file_name(path));
        if self.library.is_none() {
            return Ok(None);
        }
        let folder = setting_text(host, LIBRARY)?;
        let Some(inside) = below(parent(path), &folder) else {
            return Ok(None);
        };
        if !is_book_name(name, &book_extensions(host)?) {
            return Ok(None);
        }
        let Some(library) = self.library_ready()? else {
            return Ok(None);
        };
        let book = Import::new(body, name)
            .folder(inside)
            .existing(Existing::FreeName);
        let (input, out) = (&mut self.library_input, &mut self.out);
        match library.import(host, &book, input, out) {
            Ok(taken) => copy(taken).map(Some),
            Err(Error::Invalid) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// What of `path` is below `folder` (the library's folder, `""` for the card
/// itself; case is ignored as the card does); `None` where it is not in it.
fn below<'p>(path: &'p str, folder: &str) -> Option<&'p str> {
    let mut rest = path.trim_matches('/');
    let folder = folder.trim_matches('/');
    if folder.is_empty() {
        return Some(rest);
    }
    for segment in folder.split('/') {
        let (head, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let same = head
            .chars()
            .flat_map(char::to_lowercase)
            .eq(segment.chars().flat_map(char::to_lowercase));
        if head.is_empty() || !same {
            return None;
        }
        rest = tail;
    }
    Some(rest)
}

/// Whether `name` ends in one of the extensions the formats read.
fn is_book_name(name: &str, extensions: &[String]) -> bool {
    extensions
        .iter()
        .any(|extension| has_extension(name, extension))
}

impl Card for HostCard<'_, '_> {
    fn stat(&mut self, path: &str) -> Result<Option<(bool, u64)>, Error> {
        let found = self
            .files
            .stat(self.host, Root::Files, path, &mut self.input, &mut self.out);
        match found {
            Ok(entry) => Ok(Some((entry.is_folder(), entry.size()))),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn list(
        &mut self,
        dir: &str,
        take: &mut dyn FnMut(&str, bool, u64) -> Result<bool, Error>,
    ) -> Result<(), Error> {
        let mut cursor = 0;
        for _ in 0..MOST_PAGES {
            let page = self.files.list(
                self.host,
                Root::Files,
                dir,
                cursor,
                &mut self.input,
                &mut self.out,
            )?;
            for entry in page.entries() {
                // Named in no path: not reachable by any request.
                if !entry.unaddressable() && !take(entry.name(), entry.is_folder(), entry.size())? {
                    return Ok(());
                }
            }
            match page.next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(())
    }

    fn make_dir(&mut self, path: &str) -> Result<(), Error> {
        self.files
            .make_dir(self.host, Root::Files, path, &mut self.input, &mut self.out)
    }

    fn remove(&mut self, path: &str, is_dir: bool) -> Result<(), Error> {
        if !is_dir && self.is_book(path)? {
            let host = self.host;
            if let Some(library) = self.library_ready()? {
                // With its notes: the user asked on the page.
                let (input, out) = (&mut self.library_input, &mut self.out);
                let report = library.delete(host, &[path], true, input, out)?;
                return report
                    .rows()
                    .next()
                    .map_or(Err(Error::Invalid), |row| row.status());
            }
        }
        // A folder: the host forgets the books below it.
        self.files
            .remove_tree(self.host, Root::Files, path, &mut self.input, &mut self.out)
            .map(|_| ())
    }

    fn move_to(&mut self, from: &str, to: &str, is_dir: bool) -> Result<(), Error> {
        if !is_dir && self.is_book(from)? {
            let host = self.host;
            if let Some(library) = self.library_ready()? {
                let (input, out) = (&mut self.library_input, &mut self.out);
                return library.move_to(host, from, to, false, input, out);
            }
        }
        // The host's library follows every move on root 1 (`files.md` 2.6).
        self.files.move_entry(
            self.host,
            Root::Files,
            from,
            Root::Files,
            to,
            false,
            &mut self.input,
            &mut self.out,
        )
    }

    fn free(&mut self) -> Result<Option<u64>, Error> {
        let space = self
            .files
            .space(self.host, Root::Files, &mut self.input, &mut self.out)?;
        Ok(space.free())
    }

    fn put(&mut self, body: Option<&str>, path: &str) -> Result<String, Error> {
        match body {
            Some(body) => {
                if let Some(taken) = self.import(body, path)? {
                    return Ok(taken);
                }
                // From the data folder to the card, never over what is there.
                self.files.move_entry(
                    self.host,
                    Root::Data,
                    body,
                    Root::Files,
                    path,
                    false,
                    &mut self.input,
                    &mut self.out,
                )?;
            }
            None => storage::write(self.host, Root::Files, path, &[])?,
        }
        copy(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn the_library_folder_is_found_in_a_path_as_the_card_names_things() {
        assert_eq!(below("Books", "Books"), Some(""));
        assert_eq!(below("books/Sub/Deep", "/Books/"), Some("Sub/Deep"));
        assert_eq!(below("Books/a", "Books/a"), Some(""));
        assert_eq!(below("Books/A/b", "books/a"), Some("b"));
        // Another folder that starts the same, or one above it, is not in it.
        assert_eq!(below("Books2", "Books"), None);
        assert_eq!(below("Books", "Books/Sub"), None);
        assert_eq!(below("", "Books"), None);
        assert_eq!(below("Other/Books", "Books"), None);
        // No folder set: the card itself.
        assert_eq!(below("a/b", ""), Some("a/b"));
        assert_eq!(below("", "/"), Some(""));
        assert_eq!(below("Bücher/x", "BÜCHER"), Some("x"));
    }

    #[test]
    fn a_book_name_ends_in_an_extension_of_the_formats() {
        let formats = ["epub".to_string(), "fb2.zip".to_string()];
        assert!(is_book_name("a.epub", &formats));
        assert!(is_book_name("A B.EPUB", &formats));
        assert!(is_book_name("x.fb2.zip", &formats));
        assert!(!is_book_name("epub", &formats));
        assert!(!is_book_name("a.epub.txt", &formats));
        assert!(!is_book_name("a.epub", &[]));
    }
}
