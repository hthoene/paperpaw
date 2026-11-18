//! Choosing a folder for a setting (`core.choose-folder`, parameter `id`):
//! the rows of the folder on show and the name of a new folder, free of host
//! calls. The first row uses the shown folder, the next goes to its parent
//! (not at the top) and *New folder* makes one in it; then come its folders.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{cmp_lowercase, concat, copy},
};

/// Folders of one folder shown at most: a longer list is cut (the first
/// ones, in the order of their names).
pub const MOST_FOLDERS: usize = 256;

/// What a row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pick {
    /// Use the shown folder.
    Use,
    /// Go to the parent folder.
    Up,
    /// Make a folder in the shown one.
    New,
    /// Go into the folder at this index of the names.
    Folder(usize),
}

/// The rows of the folder `dir` (empty at the top) with `folders` folders.
///
/// # Errors
/// `NoMemory`.
pub fn picks(dir: &str, folders: usize) -> Result<Vec<Pick>, Error> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(folders.saturating_add(3))
        .map_err(|_| Error::NoMemory)?;
    push(&mut rows, Pick::Use)?;
    if !dir.is_empty() {
        push(&mut rows, Pick::Up)?;
    }
    push(&mut rows, Pick::New)?;
    for at in 0..folders {
        push(&mut rows, Pick::Folder(at))?;
    }
    Ok(rows)
}

/// The parent of `dir`: empty for a folder at the top.
#[must_use]
pub fn parent(dir: &str) -> &str {
    dir.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// The last segment of `dir`.
#[must_use]
pub fn last(dir: &str) -> &str {
    dir.rsplit('/').next().unwrap_or(dir)
}

/// The path of `name` in `dir`.
///
/// # Errors
/// `NoMemory`.
pub fn join(dir: &str, name: &str) -> Result<String, Error> {
    if dir.is_empty() {
        copy(name)
    } else {
        concat(&[dir, "/", name])
    }
}

/// Puts the names in order, case aside.
pub fn sort(names: &mut [String]) {
    paperpaw_ui_kit::mem::stable_sort_by(names, |a, b| cmp_lowercase(a, b));
}

/// `typed` made a name for a new folder that every file system takes as it is,
/// the memory card's (FAT) and Windows' too: without the characters they refuse
/// (`/ \ : * ? " < > |`, control characters), without dots at the start (hidden)
/// and dots or spaces at the end (dropped); `None` where nothing is left.
///
/// # Errors
/// `NoMemory`.
pub fn folder_name(typed: &str) -> Result<Option<String>, Error> {
    let mut name = String::new();
    name.try_reserve(typed.len()).map_err(|_| Error::NoMemory)?;
    for c in typed.chars() {
        if !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control() {
            name.push(c);
        }
    }
    let name = name
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim_start();
    if name.is_empty() {
        return Ok(None);
    }
    copy(name).map(Some)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::vec;

    #[test]
    fn the_top_has_no_way_up() {
        assert_eq!(
            picks("", 2).unwrap(),
            [Pick::Use, Pick::New, Pick::Folder(0), Pick::Folder(1)]
        );
        assert_eq!(picks("books", 0).unwrap(), [Pick::Use, Pick::Up, Pick::New]);
    }

    #[test]
    fn paths_go_up_down_and_name_their_last_folder() {
        assert_eq!(parent("books/Crime"), "books");
        assert_eq!(parent("books"), "");
        assert_eq!(parent(""), "");
        assert_eq!(last("books/Crime"), "Crime");
        assert_eq!(last("books"), "books");
        assert_eq!(join("", "books").unwrap(), "books");
        assert_eq!(join("books", "Crime").unwrap(), "books/Crime");
    }

    #[test]
    fn folders_are_listed_by_name_with_case_aside() {
        let mut names = vec![
            String::from("b"),
            String::from("Archive"),
            String::from("a"),
        ];
        sort(&mut names);
        assert_eq!(names, ["a", "Archive", "b"]);
    }

    #[test]
    fn folder_names_suit_every_file_system() {
        let made = |typed| folder_name(typed).unwrap();
        assert_eq!(made("  Fantasy  ").as_deref(), Some("Fantasy"));
        assert_eq!(made("Sci/Fi: *new*?").as_deref(), Some("SciFi new"));
        assert_eq!(made("..hidden").as_deref(), Some("hidden"));
        assert_eq!(made("Notes. . ").as_deref(), Some("Notes"));
        assert_eq!(made("a\"b<c>d|e").as_deref(), Some("abcde"));
        assert_eq!(made(" . . "), None);
        assert_eq!(made("..."), None);
        assert_eq!(made(""), None);
    }
}
