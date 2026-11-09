//! Book information (`core.book-info`, parameter `path`): what the library
//! knows of one book, as rows of a list: series, how far it is read, the
//! fields plugins keep, the file with its size, and the folder. What the
//! SDK cannot give yet (the cover, the tags' text, the language and the
//! description, which are the format's) is in `PORTING-7.md`.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    library::{Book, BookRef, FieldCursor, Library},
};
use paperpaw_ui_kit::{
    Item, Messages,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy, decimal, decimal_u64},
};

/// What the screen shows of a book: its title and author (above the rows),
/// the labelled rows.
#[derive(Debug, Default)]
pub struct Info {
    pub title: String,
    pub author: String,
    /// Label and value, in order.
    pub rows: Vec<(String, String)>,
}

/// The most field rows read (the host's own cap).
const MOST_FIELDS: u32 = 64;

/// Reads what is known of the book at `path`: from the library's catalog.
/// A book the catalog does not know (gone, or not catalogued yet) still
/// shows its file name and folder.
///
/// # Errors
/// The host's refusal or `NoMemory`.
pub fn read(host: &Host<'_>, library: &Library, path: &str) -> Result<Info, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut say = Messages::new()?;
    let mut info = Info::default();
    let found = library.book(host, BookRef::path(path)?, &mut input, &mut out);
    let name = file_name(path);
    match found {
        Ok(book) => {
            info.title = copy(book.title())?;
            info.author = copy(book.author())?;
            series_row(host, &mut say, &mut info, book)?;
            if let Some(progress) = book.progress() {
                let mut digits = [0; 20];
                let value = concat(&[decimal(usize::from(progress), &mut digits), " %"])?;
                row(host, &mut say, &mut info, "files.info.progress", value)?;
            }
            let reference = book.reference()?;
            let (size, format) = (book.size(), upper(book.format())?);
            let fields = book.fields();
            fields_rows(host, library, reference, fields, &mut info)?;
            file_row(host, &mut say, &mut info, &format, size)?;
        }
        Err(Error::NotFound | Error::State) => {
            info.title = copy(stem(name))?;
            let format = upper(extension(name))?;
            let value = if format.is_empty() {
                copy(name)?
            } else {
                format
            };
            row(host, &mut say, &mut info, "files.info.file", value)?;
        }
        Err(error) => return Err(error),
    }
    let folder = copy(parent(path))?;
    row(host, &mut say, &mut info, "files.info.folder", folder)?;
    Ok(info)
}

fn row(
    host: &Host<'_>,
    say: &mut Messages,
    info: &mut Info,
    label: &str,
    value: String,
) -> Result<(), Error> {
    push(&mut info.rows, (say.copy(host, label)?, value))
}

fn series_row(
    host: &Host<'_>,
    say: &mut Messages,
    info: &mut Info,
    book: Book<'_>,
) -> Result<(), Error> {
    if book.series().is_empty() {
        return Ok(());
    }
    let value = if book.number().is_empty() {
        copy(book.series())?
    } else {
        let number = say.format(
            host,
            "files.series-number",
            &[paperpaw_sdk::native::formatting::Arg::Text {
                name: "number",
                value: book.number(),
            }],
        )?;
        concat(&[book.series(), " · ", &number])?
    };
    row(host, say, info, "files.info.series", value)
}

/// The fields plugins keep with the book, each where it has a value.
fn fields_rows(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    count: u32,
    info: &mut Info,
) -> Result<(), Error> {
    if count == 0 {
        return Ok(());
    }
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut cursor = FieldCursor::start();
    for _ in 0..=MOST_FIELDS {
        let page = library.fields(host, book, cursor, &mut input, &mut out)?;
        for field in page.rows() {
            let field = field?;
            if !field.value().is_empty() {
                push(&mut info.rows, (copy(field.title())?, copy(field.value())?))?;
            }
        }
        match page.end().next() {
            Some(next) if !page.is_empty() => cursor = next,
            _ => break,
        }
    }
    Ok(())
}

/// The file's row: its format and its size as the language writes it,
/// `EPUB · 2,4 MB`.
fn file_row(
    host: &Host<'_>,
    say: &mut Messages,
    info: &mut Info,
    format: &str,
    size: u64,
) -> Result<(), Error> {
    let mark = say
        .copy(host, "core.decimal")
        .ok()
        .filter(|mark| !mark.is_empty())
        .map_or_else(|| copy("."), Ok)?;
    let size = say.digits(host, &size_text(size, &mark)?)?;
    let value = if format.is_empty() {
        size
    } else {
        concat(&[format, " · ", &size])?
    };
    row(host, say, info, "files.info.file", value)
}

/// A size as people read it: `820 B`, `14 KB`, `2,3 MB`.
pub fn size_text(bytes: u64, mark: &str) -> Result<String, Error> {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    let mut digits = [0; 20];
    let unit = UNITS.get(unit).copied().unwrap_or("B");
    if unit == "B" || value >= 10.0 {
        concat(&[decimal_u64((value + 0.5) as u64, &mut digits), " ", unit])
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        let whole = copy(decimal_u64(tenths / 10, &mut digits))?;
        concat(&[
            &whole,
            mark,
            decimal_u64(tenths % 10, &mut digits),
            " ",
            unit,
        ])
    }
}

/// The text in capitals (a format as the list says it).
pub fn upper(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    for letter in text.chars().flat_map(char::to_uppercase) {
        out.try_reserve(letter.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        out.push(letter);
    }
    Ok(out)
}

/// The last part of a path.
#[must_use]
pub fn file_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

/// A path without its last part.
#[must_use]
pub fn parent(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent)
}

/// A file name without its extension.
#[must_use]
pub fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// A file name's extension (none for no dot).
#[must_use]
pub fn extension(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(_, extension)| extension)
}

/// The rows as the list shows them: the label with the value.
///
/// # Errors
/// `NoMemory` when a text cannot be kept.
pub fn items(info: &Info) -> Result<Vec<Item>, Error> {
    let mut items = Vec::new();
    push(&mut items, Item::heading(&info.title)?)?;
    if !info.author.is_empty() {
        push(&mut items, Item::heading(&info.author)?)?;
    }
    for (label, value) in &info.rows {
        push(&mut items, Item::row(label, value)?)?;
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    #[test]
    fn sizes_are_bytes_kilobytes_or_megabytes_with_the_mark_given() {
        assert_eq!(size_text(820, ",").unwrap(), "820 B");
        assert_eq!(size_text(14 * 1024, ",").unwrap(), "14 KB");
        assert_eq!(size_text(2_400_000, ",").unwrap(), "2,3 MB");
        assert_eq!(size_text(2_400_000, ".").unwrap(), "2.3 MB");
        assert_eq!(size_text(0, ".").unwrap(), "0 B");
        assert!(size_text(u64::MAX, ".").unwrap().ends_with("GB"));
    }

    #[test]
    fn a_path_is_split_into_folder_name_stem_and_extension() {
        assert_eq!(file_name("books/A/b.fb2.zip"), "b.fb2.zip");
        assert_eq!(parent("books/A/b.epub"), "books/A");
        assert_eq!(parent("b.epub"), "");
        assert_eq!(stem("b.fb2.zip"), "b.fb2");
        assert_eq!(extension("b.fb2.zip"), "zip");
        assert_eq!(extension("README"), "");
        assert_eq!(upper("epub").unwrap(), "EPUB");
    }

    #[test]
    fn the_rows_are_a_heading_for_the_title_and_one_noted_row_each() {
        let info = Info {
            title: "T".into(),
            author: "A".into(),
            rows: alloc::vec![("Folder".into(), "books".into())],
        };
        assert_eq!(items(&info).unwrap().len(), 3);
        let bare = Info {
            title: "T".into(),
            ..Info::default()
        };
        assert_eq!(items(&bare).unwrap().len(), 1);
    }
}
