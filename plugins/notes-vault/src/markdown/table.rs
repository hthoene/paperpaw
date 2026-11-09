//! Tables written with pipes (`| a | b |`, a row of dashes under the
//! first line): each row is a line of cells, set apart.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::DocumentError;

use crate::fallible::{self, TryPush, TryPushStr};

/// Cells of a row read at most; the rest are left out.
const MAX_CELLS: usize = 16;

/// The cells of a row of a table, outer pipes taken off. A pipe inside
/// `[[…]]`, backticks, or written `\|` does not part cells.
pub(super) fn cells(row: &str) -> Result<Vec<&str>, DocumentError> {
    let row = row.trim();
    let row = row.strip_prefix('|').unwrap_or(row);
    let row = row
        .strip_suffix('|')
        .filter(|_| !row.ends_with("\\|"))
        .unwrap_or(row);
    let mut cells = Vec::new();
    fallible::reserve_exact(&mut cells, MAX_CELLS)?;
    let mut from = 0;
    let (mut brackets, mut code) = (false, false);
    let bytes = row.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'`' => code = !code,
            b'[' if !code && bytes.get(i + 1) == Some(&b'[') => {
                brackets = true;
                i += 1;
            }
            b']' if brackets && bytes.get(i + 1) == Some(&b']') => {
                brackets = false;
                i += 1;
            }
            b'|' if !brackets && !code => {
                if cells.len() < MAX_CELLS {
                    cells.push(row[from..i].trim());
                }
                from = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if cells.len() < MAX_CELLS {
        cells.push(row[from.min(row.len())..].trim());
    }
    Ok(cells)
}

/// Whether `line` can be the first line of a table.
pub(super) fn is_candidate(line: &str) -> bool {
    line.contains('|') && !line.trim_start().starts_with('>') && !line.trim().is_empty()
}

/// Whether `line` is the row of dashes under a table's first line
/// (`|---|:--:|`).
pub(super) fn is_separator(line: &str) -> bool {
    let line = line.trim();
    line.contains('-')
        && line.contains('|')
        && line
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// The row as HTML cells, each `tag` (`td` or `th`), its words read by
/// `inline`.
pub(super) fn row(
    line: &str,
    tag: &str,
    inline: &mut dyn FnMut(&str, &mut String) -> Result<(), DocumentError>,
    out: &mut String,
) -> Result<(), DocumentError> {
    out.try_push_str("<tr>")?;
    for cell in cells(line)? {
        out.try_push('<')?;
        out.try_push_str(tag)?;
        out.try_push('>')?;
        inline(cell, out)?;
        out.try_push_str("</")?;
        out.try_push_str(tag)?;
        out.try_push('>')?;
    }
    out.try_push_str("</tr>\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_parted_by_pipes_outside_links_and_code() {
        assert_eq!(cells("| a | b |").unwrap(), ["a", "b"]);
        assert_eq!(cells("a | b | c").unwrap(), ["a", "b", "c"]);
        assert_eq!(
            cells("| [[n|label]] | `x|y` | a\\|b |").unwrap(),
            ["[[n|label]]", "`x|y`", "a\\|b"]
        );
        assert_eq!(cells("|a||").unwrap(), ["a", ""]);
        assert_eq!(cells("|").unwrap().len(), 1);
        assert_eq!(cells(&"x|".repeat(100)).unwrap().len(), MAX_CELLS);
    }

    #[test]
    fn the_row_of_dashes() {
        assert!(is_separator("|---|:--:|"));
        assert!(is_separator("--- | ---"));
        assert!(!is_separator("| a | b |"));
        assert!(!is_separator("- item"));
        assert!(!is_separator("|||"));
    }
}
