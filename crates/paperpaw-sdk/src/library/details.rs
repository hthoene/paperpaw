//! The text of a book's details file, built without an infallible
//! allocation and not at all where it would be over what is read back:
//! the same text as [`config::set_all`] makes from the entries (a key set
//! twice keeps its first place and its last value; a section or key a file
//! would not parse is left out; entries without a section first, then each
//! section in the order its first key came), but a device short of memory
//! gets [`Error::OutOfMemory`] instead of an abort, and the size is
//! counted, and compared with [`MAX_DETAILS`], before the text is made.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use super::fallible::{copy, join};
use super::{Book, DETAILS, MAX_DETAILS, MAX_TAGS, config, fit, known_detail};
use crate::StorageError;
use crate::data::Error;

/// One `key = value` line of a section.
struct Line<'a> {
    section: &'a str,
    key: Cow<'a, str>,
    value: Cow<'a, str>,
}

/// Letters, digits, `_`, `-`, `.` and `@` (what [`config::parse`] takes as
/// a key).
fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'@'))
}

/// Identifiers with one space between each, or none at all.
fn is_section(text: &str) -> bool {
    text.is_empty() || text.split(' ').all(is_identifier)
}

fn memory<T, E>(result: Result<T, E>) -> Result<T, Error> {
    result.map_err(|_| Error::OutOfMemory)
}

/// The decimal digits of `number`, reserved fallibly.
fn number(number: u64) -> Result<String, Error> {
    let mut digits = [0_u8; 20];
    let mut at = digits.len();
    let mut rest = number;
    loop {
        at = at.saturating_sub(1);
        if let Some(digit) = digits.get_mut(at) {
            *digit = b'0' + (rest % 10) as u8;
        }
        rest /= 10;
        if rest == 0 || at == 0 {
            break;
        }
    }
    let text = digits
        .get(at..)
        .map_or("", |d| core::str::from_utf8(d).unwrap_or_default());
    memory(copy(text))
}

/// Adds `section` / `key` = `value`: after the lines there are, or in
/// place of a line of the same key (the last value is the one kept).
fn add<'a>(
    lines: &mut Vec<Line<'a>>,
    section: &'a str,
    key: Cow<'a, str>,
    value: Cow<'a, str>,
) -> Result<(), Error> {
    if !is_section(section) || !is_identifier(&key) {
        return Ok(());
    }
    if let Some(line) = lines
        .iter_mut()
        .find(|line| line.section == section && line.key == key)
    {
        line.value = value;
        return Ok(());
    }
    memory(lines.try_reserve(1))?;
    lines.push(Line {
        section,
        key,
        value,
    });
    Ok(())
}

/// Every line of the details of `book`, with what `before` holds that the
/// library does not know kept.
fn collect<'a>(book: &'a Book, before: &'a str) -> Result<Vec<Line<'a>>, Error> {
    let mut lines = Vec::new();
    let book_line = |lines: &mut Vec<Line<'a>>, key: &'a str, value: Cow<'a, str>| {
        add(lines, "book", Cow::Borrowed(key), value)
    };
    book_line(&mut lines, "size", number(book.size)?.into())?;
    book_line(
        &mut lines,
        "indexed",
        (if book.indexed { "on" } else { "off" }).into(),
    )?;
    book_line(&mut lines, "version", number(u64::from(DETAILS))?.into())?;
    for (key, value) in [
        ("title", &book.title),
        ("author", &book.author),
        ("series", &book.series),
        ("series-index", &book.series_index),
    ] {
        if let Some(value) = value {
            book_line(&mut lines, key, fit(value).into())?;
        }
    }
    for (i, tag) in book.tags.iter().take(MAX_TAGS).enumerate() {
        let key = memory(join(&["tag.", &number(i as u64 + 1)?]))?;
        add(&mut lines, "book", key.into(), fit(tag).into())?;
    }
    if let Some(cover) = &book.cover {
        book_line(&mut lines, "cover", fit(&cover.path).into())?;
        book_line(
            &mut lines,
            "cover-width",
            number(u64::from(cover.width))?.into(),
        )?;
        book_line(
            &mut lines,
            "cover-height",
            number(u64::from(cover.height))?.into(),
        )?;
    }
    if let Some(progress) = book.progress {
        book_line(&mut lines, "progress", number(u64::from(progress))?.into())?;
    }
    if let Some(added) = book.added {
        book_line(&mut lines, "added", number(u64::from(added))?.into())?;
    }
    for (id, value) in &book.fields {
        add(&mut lines, "field", id.as_str().into(), fit(value).into())?;
    }
    for entry in config::parse(before).filter_map(Result::ok) {
        if !known_detail(entry.section, entry.key) {
            let value = memory(entry.value.try_as_str())?;
            add(&mut lines, entry.section, entry.key.into(), value)?;
        }
    }
    Ok(lines)
}

/// Whether `value` is written as it is ([`config::quote`]).
fn is_plain(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && !value.starts_with('"')
        && !value.contains(['\n', '\r'])
}

/// `value` as [`config::quote`] writes it, in pieces.
fn quoted(value: &str, put: &mut dyn FnMut(&str)) {
    if is_plain(value) {
        put(value);
        return;
    }
    put("\"");
    let mut utf8 = [0_u8; 4];
    for c in value.chars() {
        match c {
            '"' => put("\\\""),
            '\\' => put("\\\\"),
            '\n' => put("\\n"),
            '\t' => put("\\t"),
            '\r' => {}
            c => put(c.encode_utf8(&mut utf8)),
        }
    }
    put("\"");
}

fn put_line(line: &Line<'_>, put: &mut dyn FnMut(&str)) {
    put(&line.key);
    put(" = ");
    quoted(&line.value, put);
    put("\n");
}

/// The file in pieces: the lines without a section, then each section in
/// the order of its first line, a blank line before every one but the
/// section of the very first line (as `set` appended them one by one to an
/// empty file).
fn emit(lines: &[Line<'_>], put: &mut dyn FnMut(&str)) {
    for line in lines.iter().filter(|line| line.section.is_empty()) {
        put_line(line, put);
    }
    for (i, head) in lines.iter().enumerate() {
        let first = !head.section.is_empty()
            && !lines
                .iter()
                .take(i)
                .any(|line| line.section == head.section);
        if !first {
            continue;
        }
        if i > 0 {
            put("\n");
        }
        put("[");
        put(head.section);
        put("]\n");
        for line in lines
            .iter()
            .skip(i)
            .filter(|line| line.section == head.section)
        {
            put_line(line, put);
        }
    }
}

/// The text of the details of `book`, with what `before` holds that the
/// library does not know kept.
///
/// # Errors
/// [`Error::OutOfMemory`] where the text or what it is made of cannot be
/// had, [`StorageError::TooLarge`] where the text would be over
/// [`MAX_DETAILS`] (nothing of it is made then).
pub(super) fn try_details_text(book: &Book, before: &str) -> Result<String, Error> {
    let lines = collect(book, before)?;
    let mut size = 0_usize;
    emit(&lines, &mut |piece| size = size.saturating_add(piece.len()));
    if size > MAX_DETAILS {
        return Err(StorageError::TooLarge.into());
    }
    let mut text = String::new();
    memory(text.try_reserve_exact(size))?;
    emit(&lines, &mut |piece| text.push_str(piece));
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_quoted_as_the_config_crate_quotes_it() {
        for value in [
            "",
            "plain",
            " lead",
            "trail ",
            "\"start",
            "in \"the\" middle",
            "a\\b",
            "two\nlines",
            "tab\there",
            "ret\rurn",
            "ünï ✓ 😀",
            "\r",
        ] {
            let mut out = String::new();
            quoted(value, &mut |piece| out.push_str(piece));
            assert_eq!(out, config::quote(value).as_ref(), "{value:?}");
        }
    }

    #[test]
    fn a_number_is_its_decimal_digits() {
        for n in [0, 1, 9, 10, 99, 100, 20_000, u64::from(u32::MAX), u64::MAX] {
            assert_eq!(number(n).unwrap(), alloc::format!("{n}"));
        }
    }

    #[test]
    fn a_section_or_key_a_file_would_not_parse_is_left_out() {
        let mut lines = Vec::new();
        add(&mut lines, "book", "bad key".into(), "1".into()).unwrap();
        add(&mut lines, "bad]", "k".into(), "1".into()).unwrap();
        add(&mut lines, "menu core.reader-menu", "k".into(), "1".into()).unwrap();
        add(&mut lines, "", "top".into(), "1".into()).unwrap();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn no_line_is_no_text() {
        let mut text = String::new();
        emit(&[], &mut |piece| text.push_str(piece));
        assert_eq!(text, "");
    }

    #[test]
    fn text_over_the_limit_is_refused_before_it_is_made() {
        let book = Book {
            fields: (0..40)
                .map(|i| (alloc::format!("a.field{i}"), "x".repeat(1000)))
                .collect(),
            ..Book::default()
        };
        assert_eq!(
            try_details_text(&book, ""),
            Err(StorageError::TooLarge.into())
        );
    }
}
