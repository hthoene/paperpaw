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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the `core.files` busy check asks it once #339 is in"
        )
    )]
    pub(crate) fn holds(&self, path: &str, ignore_case: bool) -> bool {
        self.holds_except(path, ignore_case, None)
    }

    /// As [`Self::holds`], not counting the claims of `owner` (who may claim a
    /// path it holds already).
    pub(crate) fn holds_except(&self, path: &str, ignore_case: bool, owner: Option<u64>) -> bool {
        let alike = |a: &str, b: &str| {
            if ignore_case {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        let inside = |inner: &str, outer: &str| {
            inner.len() > outer.len()
                && inner.is_char_boundary(outer.len())
                && inner.as_bytes()[outer.len()] == b'/'
                && alike(&inner[..outer.len()], outer)
        };
        self.claims.try_borrow().map_or(true, |claims| {
            claims
                .iter()
                .filter(|c| Some(c.owner) != owner)
                .any(|c| alike(&c.path, path) || inside(&c.path, path) || inside(path, &c.path))
        })
    }
}
