//! `Storage::same_entry` on FAT: by the entry, not by the spelling.

use super::*;

#[test]
fn a_folder_is_itself_in_any_case_and_no_other() {
    let s = storage();
    s.write(".paperpaw/secrets.txt", b"password").unwrap();
    s.write("Books/a.epub", b"a book").unwrap();
    s.write("books2/b.epub", b"another").unwrap();
    assert_eq!(s.same_entry(".paperpaw", ".PaperPaw"), Some(true));
    assert_eq!(s.same_entry("/Books/", "books"), Some(true));
    assert_eq!(s.same_entry("Books/a.epub", "BOOKS/A.EPUB"), Some(true));
    assert_eq!(s.same_entry("Books", "books2"), Some(false));
    assert_eq!(s.same_entry("Books/a.epub", "books2/b.epub"), Some(false));
    // A folder and a file are never one.
    assert_eq!(s.same_entry("Books", "Books/a.epub"), Some(false));
}

/// A short alias of a long name names nothing here (as for every other
/// call), so it is not the folder; nor is a path that names nothing.
#[test]
fn an_alias_or_a_missing_path_is_not_the_entry() {
    let s = storage();
    s.write(".paperpaw/secrets.txt", b"password").unwrap();
    assert_eq!(s.same_entry(".paperpaw", "PAPERP~1"), Some(false));
    assert_eq!(s.same_entry(".paperpaw", "gone"), Some(false));
    assert_eq!(s.same_entry("gone/deeper", "gone/deeper"), Some(false));
    assert_eq!(s.same_entry("..", ".paperpaw"), None);
}

/// Empty files hold no clusters: two of them cannot be told apart.
#[test]
fn empty_files_are_told_apart_only_by_their_spelling() {
    let s = storage();
    s.write("one.txt", b"").unwrap();
    s.write("two.txt", b"").unwrap();
    assert_eq!(s.same_entry("one.txt", "ONE.TXT"), Some(true));
    assert_eq!(s.same_entry("one.txt", "two.txt"), None);
}
