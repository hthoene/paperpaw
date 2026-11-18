//! The embedded fonts of a book, as `fonts.txt` in the cache lists them.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::DocumentError;

use super::path;
use crate::fallible;
use crate::obfuscation::Method;

/// An embedded font as listed in `fonts.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontEntry {
    pub family: u8,
    pub bold: bool,
    pub italic: bool,
    pub method: Method,
    pub path: String,
}

/// The book's identifier and embedded fonts from the cache; an error
/// where the heap has no room for them.
pub(crate) fn fonts(
    storage: &dyn Storage,
    cache: &str,
) -> Result<(String, Vec<FontEntry>), DocumentError> {
    let Ok(file) = path(cache, "fonts.txt") else {
        return Ok((String::new(), Vec::new()));
    };
    let Ok(data) = paperpaw_sdk::read_file(storage, &file, 64 * 1024) else {
        return Ok((String::new(), Vec::new()));
    };
    let text = fallible::lossy(&data)?;
    let mut identifier = String::new();
    let mut entries = Vec::new();
    for line in text.lines() {
        // At most six fields are told apart: more is not a font line.
        let mut fields: [&str; 6] = [""; 6];
        let mut count = 0;
        for field in line.split('\t') {
            if let Some(slot) = fields.get_mut(count) {
                *slot = field;
            }
            count += 1;
        }
        match (count, fields) {
            (2, ["id", id, ..]) => identifier = fallible::string(id)?,
            (5, [family, bold, italic, method, path, _]) => {
                let Ok(family) = family.parse() else {
                    continue;
                };
                fallible::push(
                    &mut entries,
                    FontEntry {
                        family,
                        bold: bold == "1",
                        italic: italic == "1",
                        method: Method::from_code(method.parse().unwrap_or(0)),
                        path: fallible::string(path)?,
                    },
                )?;
            }
            _ => {}
        }
    }
    Ok((identifier, entries))
}
