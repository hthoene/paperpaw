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

use super::config;
use crate::StorageError;
use crate::data::Error;













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
        emit_lines(&[], &mut |piece| text.push_str(piece));
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
