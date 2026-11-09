//! The device's one table of storage paths that are in use by a host
//! writer (`files.md` section 3, `network.md` 2.1): a saving `core.net`
//! request holds its target and its staged file, so that `core.files` answers
//! `ERR_BUSY` for a `move` or `remove_tree` that would take them away, and
//! `core.net` refuses to save over a path `core.files` has in use.
//!
//! Claims are counted: the same owner may claim a path it already holds (a
//! request that replaces its older one claims first, so that a refusal changes
//! nothing) and each release gives one back. Every allocation is fallible.
//! The table is kept once per device in the host views; the files side asks it
//! through [`PathsInUse::holds`], the net side claims and releases.
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};

/// Whether `a` and `b` are one name where case does not matter, as FAT
/// compares names: letter by letter, each letter taken to its upper case (one
/// letter to one letter; `ß` is no `SS`), not only the ASCII ones. No
/// allocation.
pub(crate) fn same_name(a: &str, b: &str) -> bool {
    let mut b = b.chars();
    for x in a.chars() {
        match b.next() {
            Some(y) if x == y || x.to_uppercase().eq(y.to_uppercase()) => {}
            _ => return false,
        }
    }
    b.next().is_none()
}

/// One path a host writer holds.
struct Claim {
    owner: u64,
    /// The path as the storage names it (joined).
    path: String,
}

/// The paths in use, device-wide.
#[derive(Default)]
pub(crate) struct PathsInUse {
    claims: RefCell<Vec<Claim>>,
}

impl core::fmt::Debug for PathsInUse {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PathsInUse")
            .field("claims", &self.claims.try_borrow().map_or(0, |c| c.len()))
            .finish()
    }
}

impl PathsInUse {
    /// `owner` holds `path`, once more.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`; `ERR_BUSY` when the table is borrowed.
    pub(crate) fn claim(&self, owner: u64, path: &str) -> Result<(), i32> {
        let mut claims = self.claims.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        claims.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        let mut copy = String::new();
        copy.try_reserve_exact(path.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        copy.push_str(path);
        claims.push(Claim { owner, path: copy });
        Ok(())
    }

    /// Gives one claim of `owner` on `path` back; whether it held one.
    pub(crate) fn release(&self, owner: u64, path: &str) -> bool {
        let Ok(mut claims) = self.claims.try_borrow_mut() else {
            return false;
        };
        let Some(at) = claims
            .iter()
            .position(|c| c.owner == owner && c.path == path)
        else {
            return false;
        };
        claims.remove(at);
        true
    }

    /// Whether any claim holds `path` or a folder above it, or lies inside
    /// the folder `path` (moving or removing a folder takes what is in it
    /// away). `ignore_case` for a storage that takes names in any case for one.
    /// A table that is borrowed answers `true` (the safe side).
    pub(crate) fn holds(&self, path: &str, ignore_case: bool) -> bool {
        self.holds_except(path, ignore_case, None)
    }

    /// As [`Self::holds`], not counting the claims of `owner` (who may claim a
    /// path it holds already).
    pub(crate) fn holds_except(&self, path: &str, ignore_case: bool, owner: Option<u64>) -> bool {
        let alike = |a: &str, b: &str| {
            if ignore_case { same_name(a, b) } else { a == b }
        };
        // `inner` is below the folder `outer`: its first segments are `outer`.
        let inside = |inner: &str, outer: &str| {
            let mut rest = inner.chars();
            let mut head = 0;
            for _ in outer.chars() {
                match rest.next() {
                    Some(c) => head += c.len_utf8(),
                    None => return false,
                }
            }
            inner.is_char_boundary(head)
                && inner[head..].starts_with('/')
                && alike(&inner[..head], outer)
        };
        self.claims.try_borrow().map_or(true, |claims| {
            claims
                .iter()
                .filter(|c| Some(c.owner) != owner)
                .any(|c| alike(&c.path, path) || inside(&c.path, path) || inside(path, &c.path))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_alike_letter_by_letter_in_any_alphabet() {
        for (a, b) in [
            ("Books/a.epub", "books/A.EPUB"),
            ("Bücher/é.txt", "BÜCHER/É.TXT"),
            ("Книги/файл", "книги/ФАЙЛ"),
            ("ÅNGSTRÖM", "ångström"),
        ] {
            assert!(same_name(a, b), "{a} {b}");
            assert!(same_name(b, a), "{b} {a}");
        }
        // A letter is one letter: `ß` is not `SS`, and the lengths must match.
        for (a, b) in [("ß", "SS"), ("a", "ab"), ("ab", "a"), ("é", "e"), ("", "a")] {
            assert!(!same_name(a, b), "{a} {b}");
        }
        assert!(same_name("", ""));
    }

    #[test]
    fn a_claim_is_held_under_any_spelling_the_storage_takes_for_it() {
        let table = PathsInUse::default();
        table.claim(1, "Bücher/Äpfel.epub").unwrap();
        table.claim(1, "Bücher/Äpfel.epub.pp-staged").unwrap();
        assert!(table.holds("bücher/äpfel.epub", true));
        assert!(table.holds("BÜCHER/ÄPFEL.EPUB.PP-STAGED", true));
        assert!(table.holds("Bücher", true), "the folder above");
        assert!(table.holds("BÜCHER", true));
        assert!(
            !table.holds("bücher/äpfel.epub", false),
            "case matters here"
        );
        assert!(!table.holds("Bücher/Birnen.epub", true));
        assert!(!table.holds("Bücherei", true), "not a folder above");
        // The same owner is no obstacle to itself where it is excepted.
        assert!(!table.holds_except("bücher/äpfel.epub", true, Some(1)));
        assert!(table.holds_except("bücher/äpfel.epub", true, Some(2)));
    }
}
