//! What can be done with a book held in the library (its sheet): the choices
//! the book allows and the path the archive gives it. Free of host calls.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy, decimal},
};

/// What the sheet offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    Open,
    /// Marks the book finished.
    Finish,
    /// Clears the mark: the book counts as read in part again.
    Unfinish,
    /// Book information (`files.info`).
    Info,
    /// Moves the book into the archive folder.
    Archive,
    /// Deletes the book, asked about first.
    Delete,
}

impl Action {
    /// The key of its words.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Open => "files.option.open",
            Self::Finish => "files.option.finished",
            Self::Unfinish => "files.option.unfinished",
            Self::Info => "files.option.info",
            Self::Archive => "files.option.archive",
            Self::Delete => "files.option.delete",
        }
    }
}

/// What is known of the book held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    pub finished: bool,
    /// An archive folder is set and the book is not in it.
    pub can_archive: bool,
    /// The library's preflight takes the book for one it can delete.
    pub can_delete: bool,
}

/// The actions the book allows, in the order they are offered.
///
/// # Errors
/// `NoMemory`.
pub fn actions(facts: Facts) -> Result<Vec<Action>, Error> {
    let mut list = Vec::new();
    push(&mut list, Action::Open)?;
    push(
        &mut list,
        if facts.finished {
            Action::Unfinish
        } else {
            Action::Finish
        },
    )?;
    push(&mut list, Action::Info)?;
    if facts.can_archive {
        push(&mut list, Action::Archive)?;
    }
    if facts.can_delete {
        push(&mut list, Action::Delete)?;
    }
    Ok(list)
}

/// Whether the book at `path` may move to `archive`: there is one, and the
/// book is not in it (at any depth).
#[must_use]
pub fn can_archive(archive: &str, path: &str) -> bool {
    let archive = archive.trim_matches('/');
    if archive.is_empty() {
        return false;
    }
    let inside = path.len() > archive.len()
        && path
            .get(..archive.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(archive))
        && path.as_bytes().get(archive.len()) == Some(&b'/');
    !inside
}

/// The last segment of a path.
#[must_use]
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The name without its last extension, and the extension with its dot.
fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(at) if at > 0 => (name.get(..at).unwrap_or(name), name.get(at..).unwrap_or("")),
        _ => (name, ""),
    }
}

/// The attempts to find a free name in the archive: the book's own name,
/// then `name (2)` and on.
pub const ATTEMPTS: u8 = 9;

/// The path of the book at `path` in `archive` at the `attempt`'th try (0 is
/// its own name).
///
/// # Errors
/// `NoMemory`.
pub fn archive_path(archive: &str, path: &str, attempt: u8) -> Result<String, Error> {
    let archive = archive.trim_matches('/');
    let name = file_name(path);
    if attempt == 0 {
        return concat(&[archive, "/", name]);
    }
    let (stem, extension) = split_name(name);
    let mut digits = [0; 20];
    let number = decimal(usize::from(attempt) + 1, &mut digits);
    concat(&[archive, "/", stem, " (", number, ")", extension])
}

/// The choices of a question: the actions' words, a line each.
///
/// # Errors
/// `NoMemory`.
pub fn choices(words: &[String]) -> Result<String, Error> {
    let mut joined = String::new();
    for (at, word) in words.iter().enumerate() {
        joined = if at == 0 {
            copy(word)?
        } else {
            concat(&[&joined, "\n", word])?
        };
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    #[test]
    fn the_sheet_offers_what_the_book_allows() {
        let all = Facts {
            finished: false,
            can_archive: true,
            can_delete: true,
        };
        assert_eq!(
            actions(all).unwrap(),
            [
                Action::Open,
                Action::Finish,
                Action::Info,
                Action::Archive,
                Action::Delete
            ]
        );
        let done = Facts {
            finished: true,
            can_archive: false,
            can_delete: false,
        };
        assert_eq!(
            actions(done).unwrap(),
            [Action::Open, Action::Unfinish, Action::Info]
        );
    }

    #[test]
    fn a_book_in_the_archive_or_without_one_is_not_archived() {
        assert!(can_archive("books/Archive", "books/a.epub"));
        assert!(!can_archive("books/Archive", "books/Archive/a.epub"));
        assert!(!can_archive("books/archive/", "books/Archive/x/a.epub"));
        assert!(can_archive("books/Archive", "books/Archive2/a.epub"));
        assert!(can_archive("books/Archive", "books/Archive"));
        assert!(!can_archive("", "books/a.epub"));
        assert!(!can_archive("/", "books/a.epub"));
    }

    #[test]
    fn the_archive_finds_a_free_name_beside_the_books_own() {
        let from = "books/Crime/a.b.epub";
        assert_eq!(
            archive_path("books/Archive/", from, 0).unwrap(),
            "books/Archive/a.b.epub"
        );
        assert_eq!(
            archive_path("books/Archive", from, 1).unwrap(),
            "books/Archive/a.b (2).epub"
        );
        assert_eq!(
            archive_path("books/Archive", "plain", 2).unwrap(),
            "books/Archive/plain (3)"
        );
        assert_eq!(archive_path("a", ".hidden", 1).unwrap(), "a/.hidden (2)");
        assert_eq!(file_name("x"), "x");
        assert_eq!(file_name("a/b/"), "");
    }

    #[test]
    fn choices_are_the_words_a_line_each() {
        let words = [String::from("Open"), String::from("Delete …")];
        assert_eq!(choices(&words).unwrap(), "Open\nDelete …");
        assert_eq!(choices(&[]).unwrap(), "");
    }
}
