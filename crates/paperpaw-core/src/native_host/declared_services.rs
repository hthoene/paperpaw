//! Admission-owned service requirements. A loaded plugin's are copied (no
//! borrow of its metadata survives admission); a linked plugin's metadata is
//! the program's read-only data, and its list is borrowed from there (#248).
use super::admitted_features::AdmittedFeatures;
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_native::metadata::{Checked, Declared, ServiceList};

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

/// Where the requirements are kept.
#[derive(Debug)]
enum Kept {
    /// Copied from metadata that may go (a loaded plugin's).
    Copied {
        keys: Box<str>,
        entries: Box<[Entry]>,
    },
    /// Checked metadata kept for the whole run (a linked plugin's).
    Linked(ServiceList<'static>),
}

/// Host-only service requirements from checked metadata.
#[derive(Debug)]
pub struct DeclaredServices {
    kept: Kept,
    features: AdmittedFeatures,
}

impl DeclaredServices {
    /// No declared services; all service queries are refused.
    #[must_use]
    pub fn none() -> Self {
        Self {
            kept: Kept::Copied {
                keys: Box::default(),
                entries: Box::default(),
            },
            features: AdmittedFeatures::NONE,
        }
    }

    /// The requirements of a linked plugin's checked metadata, borrowed: the
    /// metadata is the program's read-only data. The list is checked as a
    /// copy would be; nothing is allocated.
    ///
    /// # Errors
    /// Returns Invalid for an unordered or oversized list.
    pub(crate) fn linked(services: ServiceList<'static>) -> Result<Self, Error> {
        let mut last: Option<&str> = None;
        for (count, Declared { key, .. }) in services.iter().enumerate() {
            if count == MAX_SERVICES || !key_ok(key) || last.is_some_and(|last| last >= key) {
                return Err(Error::Invalid);
            }
            last = Some(key);
        }
        Ok(Self {
            kept: Kept::Linked(services),
            features: AdmittedFeatures::NONE,
        })
    }

    /// Retain service requirements and available features from checked metadata.
    /// `host` must be the exact feature catalog used by `metadata::check`;
    /// Checked retains no revision information to validate another catalog.
    ///
    /// # Errors
    /// Returns Invalid for an unsupported catalog or invalid service list,
    /// NoMemory if the service requirements cannot be copied.
    pub fn try_from_metadata(
        checked: &Checked<'_>,
        host: &'static [paperpaw_native::metadata::Feature<'static>],
    ) -> Result<Self, Error> {
        let features = AdmittedFeatures::from_checked(checked, host)?;
        let mut declared = Self::try_from_iter(checked.services())?;
        declared.features = features;
        Ok(declared)
    }

    pub(crate) fn with_features(mut self, features: AdmittedFeatures) -> Self {
        self.features = features;
        self
    }

    /// Per-instance features preserved at admission, without metadata borrows.
    #[must_use]
    pub const fn features(&self) -> AdmittedFeatures {
        self.features
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
            if entries.len() == MAX_SERVICES
                || !key_ok(key)
                || entries.last().is_some_and(|entry| entry.key(&keys) >= key)
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
            kept: Kept::Copied {
                keys: keys.into_boxed_str(),
                entries: entries.into_boxed_slice(),
            },
            features: AdmittedFeatures::NONE,
        })
    }

    /// Number of declared service keys admitted for this plugin.
    #[must_use]
    pub fn count(&self) -> usize {
        match &self.kept {
            Kept::Copied { entries, .. } => entries.len(),
            Kept::Linked(services) => services.iter().count(),
        }
    }

    pub(crate) fn declared(&self, key: &str) -> Option<(u16, u16)> {
        match &self.kept {
            Kept::Copied { keys, entries } => entries
                .iter()
                .find(|entry| entry.key(keys) == key)
                .map(|entry| (entry.major, entry.revision)),
            Kept::Linked(services) => services
                .iter()
                .find(|declared| declared.key == key)
                .map(|declared| (declared.major, declared.semantic_revision)),
        }
    }
}

/// Declared services a plugin may have at most.
const MAX_SERVICES: usize = 32;

/// A key within the admission bounds (1 to 128 bytes).
fn key_ok(key: &str) -> bool {
    (1..=128).contains(&key.len())
}

impl Entry {
    /// Its key in `keys`.
    fn key<'k>(&self, keys: &'k str) -> &'k str {
        let start = usize::from(self.start);
        keys.get(start..start + usize::from(self.len))
            .unwrap_or_default()
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

    #[test]
    fn a_linked_list_is_borrowed_and_answers_as_its_copy() {
        use paperpaw_native::metadata::{self, Host, Profile, Service};
        static BYTES: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/widgets/native/metadata.host.bin"
        ));
        let services =
            ["core.registry", "core.ui", "core.library", "core.files"].map(|key| Service {
                key,
                major: 1,
                semantic_revision: 0,
                min_table_bytes: 0,
            });
        let host = Host {
            abi_major: 7,
            minor: 1,
            profile: Profile {
                target: "linux-x86_64-sysv64-le",
                entry_layout: 1,
                entry_bytes: 32,
            },
            services: &services,
            features: &[],
        };
        let checked = metadata::check(BYTES, &host).unwrap();
        let linked = DeclaredServices::linked(checked.service_list()).unwrap();
        let copied = DeclaredServices::try_from_iter(checked.services()).unwrap();
        assert!(matches!(linked.kept, Kept::Linked(_)), "nothing copied");
        assert!(linked.count() > 0);
        assert_eq!(linked.count(), copied.count());
        for declared in checked.services() {
            assert_eq!(linked.declared(declared.key), copied.declared(declared.key));
            assert!(linked.declared(declared.key).is_some());
        }
        assert_eq!(linked.declared("core.nothing"), None);
    }
}
