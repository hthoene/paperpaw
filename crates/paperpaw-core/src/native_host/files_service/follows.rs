//! Which moves of the user's files (root 1) the library follows with its
//! journal entry and job (`files.md` 2.6): every folder, every file a format
//! reads under its old or its new name, and every file the library keeps
//! details for (a book it has read, whose format may be off now). Any other
//! single file (a plugin's export, a flash card list and its `.part`) has
//! nothing in the catalog to carry: its move is the card's rename alone, so a
//! save made of two renames in one callback never finds the first one's job
//! in its way (#612: the job is stepped by the main loop, after the callback).
//!
//! By name only, as the walk tells books (`Formats::supports`), nothing is
//! read: a name without an extension counts as a book wherever a format
//! tells files by their first bytes, and a book's `.bak` sibling counts as
//! the book (the carry takes `a.epub.bak` for one, it may be named back).
use paperpaw_platform::Storage;
use paperpaw_sdk::{Formats, files, library};

/// Whether the library follows the root-1 move of `from` to `to` (paths of
/// the card `storage`; `folder`: `from` is one). Without the kernel's
/// `formats` (a fixture frame) nobody can say what a book is: every move is
/// followed.
pub(crate) fn library_follows(
    formats: Option<&Formats>,
    storage: &dyn Storage,
    folder: bool,
    from: &str,
    to: &str,
) -> bool {
    let Some(formats) = formats else {
        return true;
    };
    folder || may_be_book(formats, from) || may_be_book(formats, to) || kept(storage, from)
}

/// Whether the library keeps details for the file at `path`: it read the
/// book or the book was opened, whatever the formats say now (its plugin
/// may be off or not loaded), so its position and notes go with it. The
/// two names are the SDK's (`library`: `details/<h>/<key>.ini`, earlier
/// `books/<key>.ini`), put together without memory; the tests hold them to
/// what the SDK writes.
fn kept(storage: &dyn Storage, path: &str) -> bool {
    let mut digits = [0_u8; library::KEY_LEN];
    let key = library::key_into(path, &mut digits);
    let part = key.get(..2).unwrap_or("00");
    let mut name = [0_u8; 64];
    [
        &[library::DIR, "/details/", part, "/", key, ".ini"][..],
        &[library::DIR, "/books/", key, ".ini"],
    ]
    .iter()
    // A name that does not fit (none does not): followed, as before.
    .any(|parts| joined(&mut name, parts).is_none_or(|file| storage.exists(file)))
}

/// `parts` one after the other in `out`; `None` where they do not fit.
fn joined<'o>(out: &'o mut [u8; 64], parts: &[&str]) -> Option<&'o str> {
    let mut at = 0_usize;
    for part in parts {
        let end = at.checked_add(part.len())?;
        out.get_mut(at..end)?.copy_from_slice(part.as_bytes());
        at = end;
    }
    core::str::from_utf8(out.get(..at)?).ok()
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
    use paperpaw_sdk::{FormatSpec, Id, MemoryStorage, document::DocumentError};

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
        let card = MemoryStorage::new();
        let follows = |folder, from, to| library_follows(Some(&formats), &card, folder, from, to);
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
        assert!(library_follows(None, &card, false, "a.csv", "b.csv"));
    }

    /// Senior R1 on #1443: a book whose format plugin is off is no book by
    /// its name, but the library keeps details for it and so follows it.
    #[test]
    fn a_file_the_library_keeps_details_for_is_followed_whatever_its_name() {
        let formats = Formats::new();
        let card = MemoryStorage::new();
        let follows = |from, to| library_follows(Some(&formats), &card, false, from, to);
        for book in ["Books/a.mobi", "Books/b.mobi"] {
            card.write(book, b"book").unwrap();
            assert!(!follows(book, "Books/c.mobi"), "{book}: nothing kept yet");
        }
        // Details as the SDK writes them.
        library::provide(
            &card,
            "Books/a.mobi",
            library::Provided::default().title(alloc::string::String::from("Kept")),
        );
        assert!(library::book(&card, "Books/a.mobi").is_some());
        assert!(follows("Books/a.mobi", "Books/c.mobi"));
        // Where earlier versions kept them (the SDK still reads them there).
        let old = alloc::format!(
            "{}/books/{}.ini",
            library::DIR,
            library::key("Books/b.mobi")
        );
        card.write(&old, b"[book]\ntitle = Old\n").unwrap();
        assert!(follows("Books/b.mobi", "Books/c.mobi"));
        // Only the file moved from counts.
        assert!(!follows("Books/c.csv", "Books/a.mobi"));
        // The longest name fits.
        let mut name = [0_u8; 64];
        let key = "0123456789abcdef";
        let file = joined(
            &mut name,
            &[library::DIR, "/details/", "01", "/", key, ".ini"],
        );
        assert_eq!(
            file,
            Some(".paperpaw/library/details/01/0123456789abcdef.ini")
        );
        assert_eq!(joined(&mut name, &[key, key, key, key, "x"]), None);
    }

    #[test]
    fn a_name_without_extension_is_a_book_where_a_format_reads_first_bytes() {
        let mut formats = Formats::new();
        formats
            .add(spec("test.zip", "zip").sniff(|head| head.starts_with(b"PK")))
            .unwrap();
        let card = MemoryStorage::new();
        assert!(library_follows(
            Some(&formats),
            &card,
            false,
            "Books/download",
            "Books/story"
        ));
        assert!(library_follows(
            Some(&formats),
            &card,
            false,
            "Books/a.csv",
            "Books/story"
        ));
        assert!(!library_follows(
            Some(&formats),
            &card,
            false,
            "Books/a.csv",
            "Books/b.csv"
        ));
        // Short and odd names never panic.
        for name in ["", ".", "bak", ".bak", "é.bak", "a/.bak"] {
            let _ = library_follows(Some(&formats), &card, false, name, name);
        }
    }
}
