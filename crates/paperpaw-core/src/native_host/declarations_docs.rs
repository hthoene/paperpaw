//! The syntax of the document, library and lookup declarations
//! (`docs/abi/document.md` sections 1 and 7, `docs/abi/library.md` section 1):
//! `format.spec`, `reader.spec`, `lookup.spec`, `book_field.spec`,
//! `book_badge.spec` and `book_data.spec`. Like every check in the parent, one
//! record at a time without a registry: that a reader's screen is the plugin's
//! own, that extensions are unique within a plugin and the per-plugin and
//! device limits belong to the commit.
//!
//! Statuses: unlike the other declarations in the parent, an unknown flag bit,
//! kind or enum value of these records is `ERR_INVALID` (the contract lists no
//! `ERR_UNSUPPORTED` for them); a well-formed record is refused by the parent
//! with `ERR_UNSUPPORTED` until the host admits it.
use super::{Check, ensure, full_id, relative};
use crate::native_host::registrations::{
    BookBadgeSpec, BookDataSpec, BookFieldSpec, FormatSpec, LookupSpec, ReaderSpec,
};

/// `format.spec.flags`: prepared, sniffs, folder, whole_pages, siblings.
const FORMAT_FLAGS: u32 = 0b1_1111;
/// Most extensions of one format, and the longest.
pub(crate) const EXTENSIONS_MAX: usize = 16;
pub(crate) const EXTENSION_MAX_BYTES: usize = 31;
/// Most formats a reader lists.
pub(crate) const READER_FORMATS_MAX: usize = 8;
/// Most languages of a lookup provider, and the longest tag (BCP 47).
pub(crate) const LANGUAGES_MAX: usize = 32;
pub(crate) const LANGUAGE_TAG_MAX_BYTES: usize = 35;
/// `lookup.spec.flags`: bit 0 network.
const LOOKUP_FLAGS: u32 = 1;
/// `book_field.spec.flags`: bit 0 in the catalog.
const FIELD_FLAGS: u32 = 1;
/// `book_data.spec.kind`: 0 user data, 1 cache.
const DATA_KIND_MAX: u32 = 1;

/// Lines of a list field: none for an empty text, else every line non-empty.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').filter(|_| !text.is_empty())
}

/// One extension: lowercase ASCII letters and digits, with inner dots
/// (`fb2.zip`), no leading or trailing dot, 1 to 31 bytes.
fn extension(text: &str) -> Check {
    ensure(
        (1..=EXTENSION_MAX_BYTES).contains(&text.len())
            && !text.starts_with('.')
            && !text.ends_with('.')
            && !text.contains("..")
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.'),
    )
}

/// The syntax of one `format.spec`; unknown flag bits are `ERR_INVALID` here
/// (the contract names that status).
pub(crate) fn format_check(plugin_id: &str, spec: &FormatSpec<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let FormatSpec {
        name,
        title,
        extensions,
        flags,
        priority: _,
        token: _,
    } = spec;
    relative(plugin_id, name)?;
    ensure(!title.is_empty() && flags & !FORMAT_FLAGS == 0)?;
    ensure(!extensions.is_empty())?;
    let mut count = 0;
    for (at, text) in lines(extensions).enumerate() {
        extension(text)?;
        // Unique within the format, so one claim is one row.
        ensure(lines(extensions).take(at).all(|before| before != text))?;
        count += 1;
    }
    ensure(count <= EXTENSIONS_MAX)
}

/// The syntax of one `reader.spec`.
pub(crate) fn reader_check(plugin_id: &str, spec: &ReaderSpec<'_>) -> Check {
    let ReaderSpec {
        name,
        screen,
        formats,
        flags,
    } = spec;
    relative(plugin_id, name)?;
    // Whether it is one of the plugin's own screens is the commit's.
    full_id(screen)?;
    ensure(*flags == 0)?;
    let mut count = 0;
    for text in lines(formats) {
        full_id(text)?;
        count += 1;
    }
    ensure(count <= READER_FORMATS_MAX)
}

/// One BCP 47 tag: letters, digits and single hyphens between parts.
fn language(text: &str) -> Check {
    ensure(
        (1..=LANGUAGE_TAG_MAX_BYTES).contains(&text.len())
            && text
                .split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric())),
    )
}

/// The longest `lookup.spec` name: what `lookup_entry.provider` holds.
pub(crate) const LOOKUP_NAME_MAX_BYTES: usize = 64;

/// The syntax of one `lookup.spec`.
pub(crate) fn lookup_check(plugin_id: &str, spec: &LookupSpec<'_>) -> Check {
    let LookupSpec {
        name,
        title,
        languages,
        flags,
        token: _,
    } = spec;
    relative(plugin_id, name)?;
    // `lookup_entry.provider` holds 64 bytes (`document.md` section 7).
    ensure(name.len() <= LOOKUP_NAME_MAX_BYTES)?;
    ensure(!title.is_empty() && flags & !LOOKUP_FLAGS == 0)?;
    let mut count = 0;
    for text in lines(languages) {
        language(text)?;
        count += 1;
    }
    ensure(count <= LANGUAGES_MAX)
}

/// The syntax of one `book_field.spec`.
pub(crate) fn book_field_check(plugin_id: &str, spec: &BookFieldSpec<'_>) -> Check {
    let BookFieldSpec { name, title, flags } = spec;
    relative(plugin_id, name)?;
    ensure(!title.is_empty() && flags & !FIELD_FLAGS == 0)
}

/// The syntax of one `book_badge.spec`; its flags are reserved.
pub(crate) fn book_badge_check(plugin_id: &str, spec: &BookBadgeSpec<'_>) -> Check {
    let BookBadgeSpec {
        name,
        token: _,
        flags,
    } = spec;
    relative(plugin_id, name)?;
    ensure(*flags == 0)
}

/// The longest `book_data.spec` name (one folder name).
pub(crate) const BOOK_DATA_NAME_MAX_BYTES: usize = 96;

/// The syntax of one `book_data.spec` (`library.md` section 1): the name is
/// one folder name of 1 to 96 bytes of `a-z`, `0-9` and `-` (the folder is
/// `books/<name>/<book key>/` in the plugin's Data, so no separator and no
/// `.` can be in it). That a plugin names a folder once is the commit's
/// (`declaration_index`).
pub(crate) fn book_data_check(_plugin_id: &str, spec: &BookDataSpec<'_>) -> Check {
    let BookDataSpec { name, kind } = spec;
    ensure(
        (1..=BOOK_DATA_NAME_MAX_BYTES).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && *kind <= DATA_KIND_MAX,
    )
}

#[cfg(test)]
#[path = "declarations_docs_tests.rs"]
mod tests;
