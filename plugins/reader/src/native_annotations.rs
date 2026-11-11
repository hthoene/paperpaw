//! What the annotations screen decides without a host: the notes of a book as
//! Markdown (the form readwise, joplin and others read, so it stays as it
//! is), the file they go to, the rows of the list, and the export that runs
//! the stage `reader.notes-exported` after a successful write only. The
//! interface-6 `annotations.rs` is the reference until it goes at activation;
//! every allocation here is fallible.
use alloc::string::String;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::{concat, copy, format};

/// Where exported notes go, beside the library folder.
pub const NOTES: &str = "Notes";

/// What an annotation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// A place marked.
    Bookmark,
    /// Text marked.
    Highlight,
}

/// One annotation as the list and the export need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Note<'a> {
    /// A bookmark or a highlight.
    pub kind: Kind,
    /// Reading progress at its start, in percent.
    pub percent: u8,
    /// A snippet of the text.
    pub text: &'a str,
    /// What the reader noted on it (one line, may be empty).
    pub note: &'a str,
}

impl<'a> Note<'a> {
    /// An annotation.
    #[must_use]
    pub const fn new(kind: Kind, percent: u8, text: &'a str, note: &'a str) -> Self {
        Self {
            kind,
            percent,
            text,
            note,
        }
    }
}

/// The words the Markdown is headed with, in the reader's language.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Headings<'a> {
    /// `reader.highlights`.
    pub highlights: &'a str,
    /// `reader.bookmarks`.
    pub bookmarks: &'a str,
}

impl<'a> Headings<'a> {
    /// The two headings.
    #[must_use]
    pub const fn new(highlights: &'a str, bookmarks: &'a str) -> Self {
        Self {
            highlights,
            bookmarks,
        }
    }
}

/// The notes of a book as Markdown: its title and author, the highlights as
/// quotes (with their note), the bookmarks as a list, each with where it is
/// in the book.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn markdown(
    title: &str,
    author: &str,
    headings: &Headings<'_>,
    all: &[Note<'_>],
) -> Result<String, Error> {
    let mut text = format(format_args!("# {title}\n\n"))?;
    if !author.is_empty() {
        append(&mut text, format_args!("{author}\n\n"))?;
    }
    let mut first = true;
    for note in all.iter().filter(|n| n.kind == Kind::Highlight) {
        if first {
            append(&mut text, format_args!("## {}\n\n", headings.highlights))?;
            first = false;
        }
        append(
            &mut text,
            format_args!("> {}\n>\n> {} %\n\n", note.text, note.percent),
        )?;
        if !note.note.is_empty() {
            append(&mut text, format_args!("{}\n\n", note.note))?;
        }
    }
    let mut first = true;
    for note in all.iter().filter(|n| n.kind == Kind::Bookmark) {
        if first {
            append(&mut text, format_args!("## {}\n\n", headings.bookmarks))?;
            first = false;
        }
        append(
            &mut text,
            format_args!("- {} %: {}\n", note.percent, note.text),
        )?;
    }
    if !first {
        append(&mut text, format_args!("\n"))?;
    }
    Ok(text)
}

fn append(target: &mut String, args: core::fmt::Arguments<'_>) -> Result<(), Error> {
    paperpaw_ui_kit::text::write_to(target, args)
}

/// The folder notes are exported to: `Notes` beside the library folder.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn notes_dir(library_folder: &str) -> Result<String, Error> {
    let parent = paperpaw_sdk::files::parent(library_folder);
    if parent.is_empty() {
        copy(NOTES)
    } else {
        concat(&[parent, "/", NOTES])
    }
}

/// The name of the notes file (without folder): `Author - Title.md`, made
/// fit for every storage; `notes.md` where nothing is left of it. An empty
/// `title` is the book's file name.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn file_name(path: &str, title: &str, author: &str) -> Result<String, Error> {
    let title = if title.is_empty() {
        paperpaw_sdk::files::name(path)
    } else {
        title
    };
    let base = if author.is_empty() {
        copy(title)?
    } else {
        concat(&[author, " - ", title])?
    };
    let name = paperpaw_sdk::web::try_clean_name(&base, 100)
        .map_err(|_| Error::NoMemory)?
        .map_or_else(|| copy("notes"), Ok)?;
    concat(&[&name, ".md"])
}

/// The title the notes carry: the book's own, else its file name.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn title_of(path: &str, title: &str) -> Result<String, Error> {
    copy(if title.is_empty() {
        paperpaw_sdk::files::name(path)
    } else {
        title
    })
}

/// The row of an annotation: what it says as the title, and the line below
/// it (the note, else what it is, then the progress).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn row(
    note: &Note<'_>,
    bookmark: &str,
    highlight: &str,
    quoted: impl FnOnce(&str) -> Result<String, Error>,
) -> Result<(String, String), Error> {
    let (kind, title) = match note.kind {
        Kind::Bookmark => (bookmark, copy(note.text)?),
        Kind::Highlight => (highlight, quoted(note.text)?),
    };
    let what = if note.note.is_empty() {
        kind
    } else {
        note.note
    };
    let line = format(format_args!("{what} · {} %", note.percent))?;
    Ok((title, line))
}

/// What the export does: the notes `text` are written to `file`, and only
/// when that worked the stage runs (`announce`; whatever it does is not the
/// export's). Returns whether the notes were written.
pub fn export<W, E>(
    write: impl FnOnce(&str, &[u8]) -> Result<(), W>,
    file: &str,
    text: &str,
    announce: impl FnOnce() -> Result<(), E>,
) -> bool {
    crate::native_notes::after_write(write(file, text.as_bytes()), announce)
}

#[cfg(test)]
mod tests;
