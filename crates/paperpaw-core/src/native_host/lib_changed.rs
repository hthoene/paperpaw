//! What `core.library` tells the plugins changed (`event.library_changed`,
//! tag 91, `docs/abi/library.md` section 3): the bits raised since the last
//! pass, coalesced, and the one book they were about.
//!
//! The library host holds one pending change and never reaches a plugin: the
//! kernel's pump takes it between callbacks (`library_events`) and queues it
//! for the plugins that subscribed. Bits are OR-ed; the path is kept while
//! every change was about the same book and is empty (several books, "re-read
//! what you show") once another book, or no book, was named, or where the
//! path could not be kept.
use alloc::string::String;
use core::cell::RefCell;

/// `event.library_changed.what`: books added, removed or moved.
pub(crate) const BOOKS: u32 = 1;
/// Status (finished, favourite, opened).
pub(crate) const STATUS: u32 = 1 << 1;
/// Positions.
pub(crate) const POSITIONS: u32 = 1 << 2;
/// Annotations (a committed edit transaction).
pub(crate) const ANNOTATIONS: u32 = 1 << 3;
/// Fields or badges.
pub(crate) const FIELDS: u32 = 1 << 4;
/// The walk progressed or ended.
pub(crate) const WALK: u32 = 1 << 5;
/// A cover is ready.
#[expect(
    dead_code,
    reason = "the cover job raises it; this goes with the first caller"
)]
pub(crate) const COVER: u32 = 1 << 6;
/// The bits the wire defines.
const KNOWN: u32 = (1 << 7) - 1;

/// The longest path the record carries (`event.library_changed.path`).
const PATH_MAX: usize = 512;

/// The change waiting to be told.
#[derive(Default)]
pub(crate) struct Changes(RefCell<Waiting>);

#[derive(Default)]
struct Waiting {
    what: u32,
    /// The one book every change was about; empty for none or several.
    path: String,
}

impl Changes {
    /// Notes that `what` changed for the book at `path` (empty: no one book).
    /// Bits the wire does not define are ignored. Never fails: a path that
    /// cannot be kept (too long, no memory) is "several books", which is
    /// always true and only asks the plugin to read more.
    pub(crate) fn raise(&self, what: u32, path: &str) {
        let what = what & KNOWN;
        if what == 0 {
            return;
        }
        let Ok(mut waiting) = self.0.try_borrow_mut() else {
            return;
        };
        let first = waiting.what == 0;
        waiting.what |= what;
        let same = !first && waiting.path == path;
        if first {
            waiting.path.clear();
            // Too long or no room: stays empty.
            if !path.is_empty()
                && path.len() <= PATH_MAX
                && waiting.path.try_reserve_exact(path.len()).is_ok()
            {
                waiting.path.push_str(path);
            }
        } else if !same {
            waiting.path.clear();
        }
    }

    /// Hands the waiting change to `visit` (`what`, `path`) and forgets it;
    /// `false` when none waits (or the state is in use: the next pass asks
    /// again).
    pub(crate) fn take(&self, visit: &mut dyn FnMut(u32, &str)) -> bool {
        let (what, path) = {
            let Ok(mut waiting) = self.0.try_borrow_mut() else {
                return false;
            };
            if waiting.what == 0 {
                return false;
            }
            (
                core::mem::take(&mut waiting.what),
                core::mem::take(&mut waiting.path),
            )
        };
        // The state is free again while `visit` runs: a change raised from
        // inside it waits for the next pass.
        visit(what, &path);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn taken(changes: &Changes) -> Option<(u32, String)> {
        let mut got = None;
        changes
            .take(&mut |what, path| got = Some((what, path.to_string())))
            .then_some(())?;
        got
    }

    #[test]
    fn bits_are_or_ed_and_the_one_book_is_kept() {
        let changes = Changes::default();
        assert_eq!(taken(&changes), None);
        changes.raise(ANNOTATIONS, "books/a.epub");
        changes.raise(STATUS, "books/a.epub");
        assert_eq!(
            taken(&changes),
            Some((ANNOTATIONS | STATUS, "books/a.epub".to_string()))
        );
        // Told once: nothing waits now.
        assert_eq!(taken(&changes), None);
    }

    #[test]
    fn another_book_or_no_book_makes_the_path_empty() {
        let changes = Changes::default();
        changes.raise(ANNOTATIONS, "books/a.epub");
        changes.raise(ANNOTATIONS, "books/b.epub");
        assert_eq!(taken(&changes), Some((ANNOTATIONS, String::new())));
        changes.raise(BOOKS, "");
        changes.raise(ANNOTATIONS, "books/a.epub");
        assert_eq!(taken(&changes), Some((BOOKS | ANNOTATIONS, String::new())));
        // The path of the next change starts afresh.
        changes.raise(ANNOTATIONS, "books/c.epub");
        assert_eq!(
            taken(&changes),
            Some((ANNOTATIONS, "books/c.epub".to_string()))
        );
    }

    #[test]
    fn a_path_over_the_cap_and_unknown_bits_are_never_carried() {
        let changes = Changes::default();
        changes.raise(1 << 20, "books/a.epub");
        assert_eq!(taken(&changes), None);
        let long = "a".repeat(PATH_MAX + 1);
        changes.raise(ANNOTATIONS | 1 << 20, &long);
        assert_eq!(taken(&changes), Some((ANNOTATIONS, String::new())));
        let exact = "b".repeat(PATH_MAX);
        changes.raise(ANNOTATIONS, &exact);
        assert_eq!(taken(&changes), Some((ANNOTATIONS, exact)));
    }

    #[test]
    fn a_change_raised_while_one_is_told_waits_for_the_next_pass() {
        let changes = Changes::default();
        changes.raise(STATUS, "books/a.epub");
        let mut seen = None;
        changes.take(&mut |what, _| {
            changes.raise(FIELDS, "books/a.epub");
            seen = Some(what);
        });
        assert_eq!(seen, Some(STATUS));
        assert_eq!(taken(&changes), Some((FIELDS, "books/a.epub".to_string())));
    }
}
