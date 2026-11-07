//! Fallible allocation helpers: every growth is reserved first, so a refused
//! allocation is [`Error::NoMemory`] and never an abort.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;

/// Reply capacity every `core.ui` operation accepts, and so the size of the
/// buffers handed to its calls.
pub const SERVICE_BUFFER: usize = 4096;

/// A zeroed buffer of `len` bytes.
///
/// # Errors
/// [`Error::NoMemory`] when it cannot be allocated.
pub fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// Appends `item` after reserving room for it.
///
/// # Errors
/// [`Error::NoMemory`] when the vector cannot grow; it is left unchanged.
pub fn push<T>(items: &mut Vec<T>, item: T) -> Result<(), Error> {
    items.try_reserve(1).map_err(|_| Error::NoMemory)?;
    items.push(item);
    Ok(())
}

/// The items of a fallible iterator, collected with reserved growth.
///
/// # Errors
/// The iterator's first error, or [`Error::NoMemory`].
pub fn collect<T>(items: impl IntoIterator<Item = Result<T, Error>>) -> Result<Vec<T>, Error> {
    let mut found = Vec::new();
    for item in items {
        push(&mut found, item?)?;
    }
    Ok(found)
}

/// Appends the items of `more` with reserved growth.
///
/// # Errors
/// [`Error::NoMemory`]; items appended before the refusal stay.
pub fn extend<T>(items: &mut Vec<T>, more: impl IntoIterator<Item = T>) -> Result<(), Error> {
    for item in more {
        push(items, item)?;
    }
    Ok(())
}

/// A copy of `items`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy_strings(items: &[String]) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        found.push(crate::text::copy(item)?);
    }
    Ok(found)
}

/// `text` in lower case, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn lowercase(text: &str) -> Result<String, Error> {
    let mut result = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        result
            .try_reserve(c.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        result.push(c);
    }
    Ok(result)
}

/// `items` joined by `separator`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn join(items: &[String], separator: &str) -> Result<String, Error> {
    let mut len = 0usize;
    for (at, item) in items.iter().enumerate() {
        if at > 0 {
            len = len.checked_add(separator.len()).ok_or(Error::NoMemory)?;
        }
        len = len.checked_add(item.len()).ok_or(Error::NoMemory)?;
    }
    let mut result = String::new();
    result.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    for (at, item) in items.iter().enumerate() {
        if at > 0 {
            result.push_str(separator);
        }
        result.push_str(item);
    }
    Ok(result)
}

/// `text` with every `from` character replaced by `to`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn replace_char(text: &str, from: char, to: char) -> Result<String, Error> {
    let mut result = String::new();
    result
        .try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    for c in text.chars() {
        // Both are ASCII where this is used; a wider `to` could outgrow the reservation.
        let c = if c == from { to } else { c };
        result
            .try_reserve(c.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        result.push(c);
    }
    Ok(result)
}

/// Sorts `items` stably in place: an insertion sort
/// by binary search, so it needs no scratch allocation. Order is that of
/// `sort_by` with the same comparison.
pub fn stable_sort_by<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> core::cmp::Ordering) {
    for at in 1..items.len() {
        let place = items[..at].partition_point(|item| compare(item, &items[at]).is_le());
        items[place..=at].rotate_right(1);
    }
}

/// A vector holding `item` alone, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn one<T>(item: T) -> Result<Vec<T>, Error> {
    let mut found = Vec::new();
    found.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
    found.push(item);
    Ok(found)
}

/// The text of each of `items`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn strs(items: &[String]) -> Result<Vec<&str>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        found.push(item.as_str());
    }
    Ok(found)
}

/// A copy of `items`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy_strs<'a>(items: &[&'a str]) -> Result<Vec<&'a str>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    found.extend_from_slice(items);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_matches_the_standard_conversion_for_plain_text() {
        assert_eq!(lowercase("Ärger ÜBER Ǆ").unwrap(), "ärger über ǆ");
    }

    #[test]
    fn the_sort_is_stable_and_matches_the_standard_one() {
        let mut items: alloc::vec::Vec<(u8, u8)> =
            [(2, 0), (1, 1), (2, 2), (1, 3), (0, 4), (2, 5)].into();
        let mut expected = items.clone();
        expected.sort_by_key(|a| a.0);
        stable_sort_by(&mut items, |a, b| a.0.cmp(&b.0));
        assert_eq!(items, expected);
    }

    #[test]
    fn join_and_replace_build_what_the_standard_ones_do() {
        let items = [String::from("a"), String::from("b")];
        assert_eq!(join(&items, ",").unwrap(), "a,b");
        assert_eq!(join(&[], ",").unwrap(), "");
        assert_eq!(replace_char("a-b-c", '-', ' ').unwrap(), "a b c");
    }

    #[test]
    fn collect_stops_at_the_first_error() {
        let items = [Ok(1), Err(Error::Invalid), Ok(3)];
        assert_eq!(collect(items), Err(Error::Invalid));
        assert_eq!(collect([Ok(1), Ok(2)]).unwrap(), [1, 2]);
    }

    #[test]
    fn helpers_survive_every_refused_allocation() {
        let items = [String::from("alpha"), String::from("beta")];
        crate::fail_alloc::sweep(|| buffer(64).map(|b| b.len()));
        crate::fail_alloc::sweep(|| copy_strings(&items).map(|v| v.len()));
        crate::fail_alloc::sweep(|| join(&items, ",").map(|s| s.len()));
        crate::fail_alloc::sweep(|| lowercase("ÄRGER Über").map(|s| s.len()));
        crate::fail_alloc::sweep(|| replace_char("a-b-c", '-', ' ').map(|s| s.len()));
        crate::fail_alloc::sweep(|| collect((0..40).map(Ok)).map(|v| v.len()));
    }
}
