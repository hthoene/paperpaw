//! The whole status lines other plugins draw, for the setting `home.status.line`
//! (a list of line ids, the first that is there shows): the lines its picker
//! offers (home's screen `status-items` in lines mode, as interface 6 had the
//! core screen `core.status-lines`), and the line the status line widget shows.
//!
//! A line may be named by an id it had before (interface 6's
//! `StatusLineSpec::also`): the kernel's line of items was the reader's
//! `reader.status-line.items`. The registry does not tell of such ids, so they
//! are known here ([`OLD_IDS`]): the picker shows such a line as chosen and a
//! change writes its id now; the widget draws it.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    registry::{PageCursor, Registry, StatusScope},
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer},
    status_items::{Entry, view::read_entries},
    text::copy,
};

/// Lines by an id they had before: the id now, then the old one.
pub const OLD_IDS: [(&str, &str); 1] = [("core.status-line.items", "reader.status-line.items")];

/// The ids a list setting holds, in order: separated by commas.
fn ids(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|id| !id.is_empty())
}

/// The id of line `id` now: its own where `id` is one it had before, else `id`.
#[must_use]
pub fn canonical(id: &str) -> &str {
    OLD_IDS
        .iter()
        .find(|(_, old)| *old == id)
        .map_or(id, |(now, _)| now)
}

/// Gives each of `entries` (lines, as the registry lists them) the id it had
/// before, where it had one.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn with_old_ids(entries: &mut [Entry]) -> Result<(), Error> {
    for entry in entries.iter_mut() {
        if let Some((_, old)) = OLD_IDS.iter().find(|(now, _)| *now == entry.id) {
            entry.also = Some(copy(old)?);
        }
    }
    Ok(())
}

/// The lines to choose from: those the registry lists (`core.registry`, the
/// status declarations of kind line), each with the id it had before.
///
/// # Errors
/// Registry, allocation and decoding errors.
pub fn entries(host: &Host<'_>) -> Result<Vec<Entry>, Error> {
    let mut entries = read_entries(host, StatusScope::Lines)?;
    with_old_ids(&mut entries)?;
    Ok(entries)
}

/// Of the ids `listed` holds, the place of the first that `is_line` (asked with
/// the id now) says is there, as interface 6 chose the line it showed.
pub fn first_shown(listed: &str, mut is_line: impl FnMut(&str) -> bool) -> Option<usize> {
    ids(listed).position(|id| is_line(canonical(id)))
}

/// The line the setting's text `listed` shows: the first of its ids (by its id
/// now) that the registry has as a line; empty where none is (the widget then
/// shows its items, as interface 6 did). A host that does not answer the
/// registry shows the first listed.
///
/// # Errors
/// Registry, allocation and decoding errors.
pub fn shown(host: &Host<'_>, listed: &str) -> Result<String, Error> {
    if ids(listed).next().is_none() {
        return Ok(String::new());
    }
    let found = match registered(host, listed) {
        Err(Error::Unsupported) => Some(0),
        other => other?,
    };
    match found.and_then(|at| ids(listed).nth(at)) {
        Some(id) => copy(canonical(id)),
        None => Ok(String::new()),
    }
}

/// The place in `listed` of the first id that names a registered line.
fn registered(host: &Host<'_>, listed: &str) -> Result<Option<usize>, Error> {
    let registry = Registry::open(host)?;
    let found = first_registered(&registry, host, listed);
    let released = registry.release(host);
    let found = found?;
    released?;
    Ok(found)
}

/// [`registered`], read a page at a time; nothing is kept but the place.
fn first_registered(
    registry: &Registry,
    host: &Host<'_>,
    listed: &str,
) -> Result<Option<usize>, Error> {
    let mut input = buffer(256)?;
    let mut out = buffer(SERVICE_BUFFER)?;
    for _ in 0..3 {
        let mut best: Option<usize> = None;
        let mut cursor = PageCursor::start();
        let result = loop {
            match registry.status(host, StatusScope::Lines, cursor, &mut input, &mut out) {
                Ok(page) => {
                    for row in page.rows() {
                        if let Some(at) = first_shown(listed, |id| id == row.id()) {
                            best = Some(best.map_or(at, |best| best.min(at)));
                        }
                    }
                    match page.end().next_cursor() {
                        Some(next) => cursor = next,
                        None => break Ok(()),
                    }
                }
                Err(error) => break Err(error),
            }
        };
        match result {
            Ok(()) => return Ok(best),
            // The registry changed while it was read: again from the start.
            Err(Error::State) => {}
            Err(error) => return Err(error),
        }
    }
    Err(Error::State)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::fail_alloc;
    use alloc::string::ToString;

    fn line(id: &str) -> Entry {
        Entry {
            id: id.to_string(),
            title: id.to_string(),
            also: None,
        }
    }

    #[test]
    fn an_old_id_is_the_line_it_names_now() {
        assert_eq!(
            canonical("reader.status-line.items"),
            "core.status-line.items"
        );
        assert_eq!(
            canonical("core.status-line.items"),
            "core.status-line.items"
        );
        assert_eq!(canonical("other.line"), "other.line");
    }

    #[test]
    fn the_kernels_line_of_items_gets_its_old_id() {
        let mut entries = [
            line("core.status-line.items"),
            line("reader.status-line.progress"),
        ];
        with_old_ids(&mut entries).unwrap();
        assert_eq!(entries[0].also.as_deref(), Some("reader.status-line.items"));
        assert_eq!(entries[1].also, None);
    }

    #[test]
    fn the_first_listed_line_that_is_there_shows() {
        let there = |id: &str| ["a.line", "core.status-line.items"].contains(&id);
        assert_eq!(first_shown("gone.line, a.line", there), Some(1));
        assert_eq!(
            first_shown("reader.status-line.items, a.line", there),
            Some(0)
        );
        assert_eq!(first_shown("gone.line", there), None);
        assert_eq!(first_shown(" , ", there), None);
        assert_eq!(first_shown("", there), None);
    }

    #[test]
    fn every_allocation_of_the_old_ids_may_be_refused() {
        fail_alloc::sweep(|| {
            let mut entries = fail_alloc::exempt(|| [line("core.status-line.items")]);
            with_old_ids(&mut entries)
        });
    }
}
