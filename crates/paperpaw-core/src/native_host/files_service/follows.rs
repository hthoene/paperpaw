//! Which moves of the user's files (root 1) the library follows with its
//! journal entry and job (`files.md` 2.6): every folder, and every file a
//! format reads under its old or its new name. A single file that is no book
//! either way (a plugin's export, a flash card list and its `.part`) has
//! nothing in the catalog to carry: its move is the card's rename alone, so a
//! save made of two renames in one callback never finds the first one's job
//! in its way (#612: the job is stepped by the main loop, after the callback).
//!
//! By name only, as the walk tells books (`Formats::supports`), nothing is
//! read: a name without an extension counts as a book wherever a format
//! tells files by their first bytes, and a book's `.bak` sibling counts as
//! the book (the carry takes `a.epub.bak` for one, it may be named back).
use paperpaw_sdk::{Formats, files};

/// Whether the library follows the root-1 move of `from` to `to` (card
/// paths; `folder`: `from` is one). Without the kernel's `formats` (a
/// fixture frame) nobody can say what a book is: every move is followed.
pub(crate) fn library_follows(
    formats: Option<&Formats>,
    folder: bool,
    from: &str,
    to: &str,
) -> bool {
    let Some(formats) = formats else {
        return true;
    };
    folder || may_be_book(formats, from) || may_be_book(formats, to)
}

/// Whether a format reads the file at `path`, going by its name, with or
/// without a `.bak` at its end.
fn may_be_book(formats: &Formats, path: &str) -> bool {
    let name = files::name(path);
    let known = |name: &str| formats.supports(name) || formats.may_sniff(name);
    known(name) || without_bak(name).is_some_and(known)
}

/// `name` without the `.bak` it ends in (any case), if it does.
fn without_bak(name: &str) -> Option<&str> {
    let at = name.len().checked_sub(4)?;
    let end = name.get(at..)?;
    end.eq_ignore_ascii_case(".bak")
        .then(|| name.get(..at))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{borrow::Cow, boxed::Box, vec};
    use paperpaw_sdk::{FormatSpec, Id, document::DocumentError};

    fn spec(id: &'static str, extension: &'static str) -> FormatSpec {
        FormatSpec::new(
            Id::new_static(id),
            "Book",
            vec![Cow::Borrowed(extension)],
            Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        )
    }

    #[test]
    fn the_library_follows_folders_and_books_but_no_other_single_file() {
        let mut formats = Formats::new();
        formats.add(spec("test.epub", "epub")).unwrap();
        let follows = |folder, from, to| library_follows(Some(&formats), folder, from, to);
        // A flash card save: the copy kept, then the new file (#612).
        assert!(!follows(
            false,
            "Flashcards/_Spanish.bak.csv.part",
            "Flashcards/_Spanish.bak.csv"
        ));
        assert!(!follows(
            false,
            "Flashcards/Spanish.csv.part",
            "Flashcards/Spanish.csv"
        ));
        // Without an extension: no format here tells files by their bytes.
        assert!(!follows(false, "Notes/README", "Notes/LICENCE"));
        // A book under either name, in any case, and its `.bak` sibling.
        assert!(follows(false, "Books/a.epub", "Books/b.epub"));
        assert!(follows(false, "Books/a.EPUB", "Books/a.old"));
        assert!(follows(false, "Books/a.part", "Books/a.epub"));
        assert!(follows(false, "Books/a.epub.bak", "Books/a.old"));
        assert!(follows(false, "Books/a.old", "Books/a.epub.BAK"));
        // A folder, whatever its name: books may be below it.
        assert!(follows(true, "Notes", "Old notes"));
        // Nobody to ask: as before.
        assert!(library_follows(None, false, "a.csv", "b.csv"));
    }

    #[test]
    fn a_name_without_extension_is_a_book_where_a_format_reads_first_bytes() {
        let mut formats = Formats::new();
        formats
            .add(spec("test.zip", "zip").sniff(|head| head.starts_with(b"PK")))
            .unwrap();
        assert!(library_follows(
            Some(&formats),
            false,
            "Books/download",
            "Books/story"
        ));
        assert!(library_follows(
            Some(&formats),
            false,
            "Books/a.csv",
            "Books/story"
        ));
        assert!(!library_follows(
            Some(&formats),
            false,
            "Books/a.csv",
            "Books/b.csv"
        ));
        // Short and odd names never panic.
        for name in ["", ".", "bak", ".bak", "é.bak", "a/.bak"] {
            let _ = library_follows(Some(&formats), false, name, name);
        }
    }
}
