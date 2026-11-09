//! The files answers are written to: each request gets one of its own,
//! so two answers that arrive together (two books tapped one after the
//! other, a feed and a search description) never write into one file.
//! A file goes once its answer is handled; what is left (the device was
//! switched off during a download, a screen was closed before its answer
//! came, a sync was stopped) goes at the next start.

use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::Cell;

use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Env, Storage, files};

use crate::PLUGIN;

/// Parameter of a request: the file its answer is written to.
pub(crate) const FILE: &str = "file";

/// The files used by versions before this one, one for each kind of
/// request.
const FIXED: [&str; 5] = [
    "browse.xml",
    "download.tmp",
    "cover.tmp",
    "sync.xml",
    "sync.tmp",
];

fn dir() -> String {
    format!("{}/tmp", Env::plugin_dir(PLUGIN.as_str()))
}

/// Names the files: one counter for the plugin's screens and its sync.
#[derive(Debug, Clone, Default)]
pub(crate) struct TempFiles(Rc<Cell<u32>>);

impl TempFiles {
    /// A file for the answer to a new request, ending in `ext`.
    pub(crate) fn next(&self, ext: &str) -> String {
        let n = self.0.get().wrapping_add(1);
        self.0.set(n);
        format!("{}/{n}.{ext}", dir())
    }
}

/// The file `reply` was written to, if it is one of these.
pub(crate) fn file(reply: &HttpReply) -> Option<&str> {
    let path = reply.context.get(FILE)?;
    let name = path.strip_prefix(&dir())?.strip_prefix('/')?;
    files::is_plain_name(name).then_some(path)
}

/// Removes the file `reply` was written to, if it is still there (one
/// moved into the library is not).
pub(crate) fn discard(storage: &dyn Storage, reply: &HttpReply) {
    if let Some(path) = file(reply) {
        let _ = storage.remove(path);
    }
}

/// Whether the file at `path`, downloaded as `name`, may be the book its
/// name says, by its first bytes: a page a server answers with instead (a
/// sign-in, an error) is not. Formats told by a mark need it (a ZIP's
/// for EPUB, CBZ and ODT; PDF's, RTF's, Mobipocket's); text formats only
/// must not be a web page.
pub(crate) fn may_be_book(storage: &dyn Storage, path: &str, name: &str) -> bool {
    let mut head = [0u8; 68];
    let Ok(read) = storage.read_at(path, 0, &mut head) else {
        return false;
    };
    let head = &head[..read.min(head.len())];
    let extension = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    let is = |ext: &str| extension.eq_ignore_ascii_case(ext);
    if is("epub") || is("cbz") || is("odt") {
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
    }
}

/// Whether `head`, the start of a file, begins a web page.
fn is_web_page(head: &[u8]) -> bool {
    let head = files::without_bom(head);
    let start = head
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(head.len());
    let head = &head[start..];
    let begins = |mark: &[u8]| {
        head.get(..mark.len())
            .is_some_and(|h| h.eq_ignore_ascii_case(mark))
    };
    begins(b"<!doctype html") || begins(b"<html")
}

/// Removes what was left before this start.
pub(crate) fn clean(storage: &dyn Storage) {
    let _ = files::delete(storage, &dir());
    let plugin = Env::plugin_dir(PLUGIN.as_str());
    for name in FIXED {
        let _ = storage.remove(&format!("{plugin}/{name}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::Params;

    fn reply(file: &str) -> HttpReply {
        let mut reply = HttpReply::new(200, String::new());
        reply.context = Params::new().with(FILE, file);
        reply
    }

    #[test]
    fn books_are_told_from_pages() {
        let mobi = [&[0u8; 60][..], b"BOOKMOBI"].concat();
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with("zip", "PK\x03\x04")
            .with("pdf", "%PDF-1.7")
            .with("mobi", mobi)
            .with("text", "Chapter 1")
            .with("page", "\u{feff}\n <!DOCTYPE html><html>Sign in</html>")
            .with("empty", "");
        let book = |path: &str, name: &str| may_be_book(&storage, path, name);
        assert!(book("zip", "a.epub") && book("pdf", "a.PDF") && book("mobi", "a.azw3"));
        assert!(book("text", "a.txt") && book("page", "a.html"));
        assert!(!book("page", "a.epub") && !book("page", "a.txt") && !book("text", "a.pdf"));
        assert!(!book("empty", "a.txt") && !book("missing", "a.epub"));
    }

    #[test]
    fn every_request_has_a_file_of_its_own() {
        let temp = TempFiles::default();
        let shared = temp.clone();
        let (a, b) = (temp.next("tmp"), shared.next("tmp"));
        assert_ne!(a, b);
        assert_eq!(file(&reply(&a)), Some(a.as_str()));
        // Only the plugin's own files are ever removed.
        assert_eq!(file(&reply("books/a.epub")), None);
        assert_eq!(file(&reply(&format!("{}/../a.epub", dir()))), None);
    }
}
