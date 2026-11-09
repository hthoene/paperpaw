//! The picture chooser, without the host: the rows (*No picture*, then the
//! pictures of the folder), which one is the chosen picture, and what a
//! choice sets. The same rules as the interface-6 `pick.rs`.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{items::Item, mem::push, text::copy};

/// What marks the chosen row at its right.
pub const MARK: &str = "•";

/// The folder of the pictures from its setting: without the slashes round
/// it, as the card's paths are written.
#[must_use]
pub fn folder(setting: &str) -> &str {
    setting.trim_matches('/')
}

/// The rows of the chooser: *No picture* (`none`), then one per name, the
/// chosen one marked. An empty `chosen` means no picture.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn rows(none: &str, names: &[String], chosen: &str) -> Result<Vec<Item>, Error> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(names.len().saturating_add(1))
        .map_err(|_| Error::NoMemory)?;
    push(&mut rows, Item::row(none, mark(chosen.is_empty()))?)?;
    for name in names {
        push(&mut rows, Item::row(name, mark(name == chosen))?)?;
    }
    Ok(rows)
}

const fn mark(here: bool) -> &'static str {
    if here { MARK } else { "" }
}

/// The row the focus starts on: the chosen picture's, else the first.
#[must_use]
pub fn start(names: &[String], chosen: &str) -> usize {
    names
        .iter()
        .position(|name| name == chosen)
        .map_or(0, |at| at + 1)
}

/// What choosing row `index` sets the `picture` setting to: nothing for
/// *No picture* (the empty name), the name for a picture, none for a row
/// there is not.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn choice(names: &[String], index: usize) -> Result<Option<String>, Error> {
    match index {
        0 => Ok(Some(String::new())),
        n => names.get(n - 1).map(|name| copy(name)).transpose(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fail_alloc::sweep;
    use alloc::string::ToString;
    use paperpaw_ui_kit::items::Item;

    fn names() -> Vec<String> {
        ["a.jpg", "b.png"].iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn the_folder_has_no_slashes_round_it() {
        assert_eq!(folder("/wallpapers/"), "wallpapers");
        assert_eq!(folder("a/b"), "a/b");
        assert_eq!(folder(""), "");
    }

    #[test]
    fn the_chosen_row_is_marked_and_the_focus_starts_on_it() {
        let names = names();
        let rows = rows("No picture", &names, "b.png").unwrap();
        let marked: Vec<bool> = rows
            .iter()
            .map(|row| matches!(row, Item::Row(row) if row.value == MARK))
            .collect();
        assert_eq!(marked, [false, false, true]);
        assert_eq!(start(&names, "b.png"), 2);
        // No picture chosen: the mark is on the first row, the focus too.
        let rows = rows_marked(&names, "");
        assert_eq!(rows, [true, false, false]);
        assert_eq!(start(&names, ""), 0);
        // A picture no longer in the folder marks no row.
        assert_eq!(rows_marked(&names, "gone.jpg"), [false, false, false]);
        assert_eq!(start(&names, "gone.jpg"), 0);
    }

    fn rows_marked(names: &[String], chosen: &str) -> Vec<bool> {
        rows("No picture", names, chosen)
            .unwrap()
            .iter()
            .map(|row| matches!(row, Item::Row(row) if row.value == MARK))
            .collect()
    }

    #[test]
    fn a_choice_names_the_picture_none_for_a_row_that_is_not_there() {
        let names = names();
        assert_eq!(choice(&names, 0).unwrap(), Some(String::new()));
        assert_eq!(choice(&names, 1).unwrap().as_deref(), Some("a.jpg"));
        assert_eq!(choice(&names, 2).unwrap().as_deref(), Some("b.png"));
        assert_eq!(choice(&names, 3).unwrap(), None);
        assert_eq!(choice(&[], 1).unwrap(), None);
    }

    #[test]
    fn refused_allocations_are_errors() {
        let names = names();
        sweep(|| {
            let rows = rows("No picture", &names, "a.jpg")?;
            assert_eq!(rows.len(), 3);
            assert!(choice(&names, 1)?.is_some());
            Ok(())
        });
    }
}
