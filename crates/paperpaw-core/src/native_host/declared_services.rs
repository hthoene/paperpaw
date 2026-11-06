//! Admission-owned service requirements. No metadata borrow survives admission.
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_native::metadata::Declared;

/// Why copying admission requirements failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Too many declarations, invalid key bounds or noncanonical keys.
    Invalid,
    /// Allocation failed before admission.
    NoMemory,
}

#[derive(Debug)]
struct Entry {
    start: u16,
    len: u8,
    major: u16,
    revision: u16,
}

/// Host-only copy of checked metadata service requirements.
#[derive(Debug)]
pub struct DeclaredServices {
    keys: Box<str>,
    entries: Box<[Entry]>,
}

impl DeclaredServices {
    /// No declared services; all service queries are refused.
    #[must_use]
    pub fn none() -> Self {
        Self {
            keys: Box::default(),
            entries: Box::default(),
        }
    }

    /// Copy the canonical service list produced by metadata admission.
    ///
    /// # Errors
    /// Returns Invalid for an unordered or oversized list, NoMemory
    /// if the bounded copy cannot be allocated.
    pub fn try_from_iter<'a>(services: impl Iterator<Item = Declared<'a>>) -> Result<Self, Error> {
        let mut keys = String::new();
        let mut entries = Vec::<Entry>::new();
        for Declared {
            key,
            major,
            semantic_revision: revision,
        } in services
        {
            if entries.len() == 32
                || key.is_empty()
                || key.len() > 128
                || entries.last().is_some_and(|entry| {
                    &keys[usize::from(entry.start)
                        ..usize::from(entry.start) + usize::from(entry.len)]
                        >= key
                })
            {
                return Err(Error::Invalid);
            }
            keys.try_reserve_exact(key.len())
                .map_err(|_| Error::NoMemory)?;
            entries.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
            entries.push(Entry {
                start: keys.len() as u16,
                len: key.len() as u8,
                major,
                revision,
            });
            keys.push_str(key);
        }
        Ok(Self {
            keys: keys.into_boxed_str(),
            entries: entries.into_boxed_slice(),
        })
    }

    /// Number of declared service keys admitted for this plugin.
    #[must_use]
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn declared(&self, key: &str) -> Option<(u16, u16)> {
        self.entries
            .iter()
            .find(|entry| {
                &self.keys
                    [usize::from(entry.start)..usize::from(entry.start) + usize::from(entry.len)]
                    == key
            })
            .map(|entry| (entry.major, entry.revision))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
    use super::*;
    fn copy<const N: usize>(entries: [(&str, u16, u16); N]) -> Result<DeclaredServices, Error> {
        DeclaredServices::try_from_iter(entries.into_iter().map(
            |(key, major, semantic_revision)| Declared {
                key,
                major,
                semantic_revision,
            },
        ))
    }
    #[test]
    fn bounded_canonical_copy_is_independent_and_none_refuses() {
        assert_eq!(DeclaredServices::none().declared("core.usage"), None);
        let services = copy([("core.plugin", 1, 0), ("core.usage", 2, 3)]).unwrap();
        assert_eq!(services.declared("core.usage"), Some((2, 3)));
        assert_eq!(services.declared("core.usage/arg"), None);
        assert!(copy([("core.usage", 1, 0); 2]).is_err());
        assert!(copy([("z", 1, 0), ("a", 1, 0)]).is_err());
        assert!(copy([("", 1, 0)]).is_err());
    }
    #[test]
    fn admission_count_and_key_bounds_are_enforced() {
        let keys: alloc::vec::Vec<_> = (0..33)
            .map(|index| alloc::format!("service.{index:02}"))
            .collect();
        let iter = || {
            keys.iter().map(|key| Declared {
                key,
                major: 1,
                semantic_revision: 0,
            })
        };
        assert_eq!(
            DeclaredServices::try_from_iter(iter().take(32))
                .unwrap()
                .count(),
            32
        );
        assert_eq!(
            DeclaredServices::try_from_iter(iter()).unwrap_err(),
            Error::Invalid
        );
        let key = "x".repeat(128);
        assert_eq!(copy([(key.as_str(), 1, 0)]).unwrap().count(), 1);
        let key = "x".repeat(129);
        assert_eq!(copy([(key.as_str(), 1, 0)]).unwrap_err(), Error::Invalid);
    }
}
