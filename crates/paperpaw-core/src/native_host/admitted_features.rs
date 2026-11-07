//! Per-instance availability indexed by the shell's wired feature list.
use super::declared_services::Error;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_UNSUPPORTED};
use paperpaw_native::metadata::{Checked, Feature};

#[allow(dead_code, reason = "REG host wiring consumes this shared feature key")]
pub(crate) const USES_REGISTRY: &str = "uses.registry";

/// Admission-owned availability bits and the shell's static wired catalog.
/// The bitset costs four bytes; retaining the catalog slice allocates nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct AdmittedFeatures {
    bits: u32,
    host: &'static [Feature<'static>],
}

impl AdmittedFeatures {
    /// The host advertises no features.
    pub const NONE: Self = Self { bits: 0, host: &[] };

    /// Retain only availability from checked metadata, indexed by wired features.
    /// `host` must be the same catalog passed to `metadata::check`. Checked
    /// availability contains keys only, so a same-key revision mismatch cannot
    /// be detected here.
    ///
    /// # Errors
    /// Returns Invalid for more than 32 host features or an inconsistent catalog.
    pub fn from_checked(
        checked: &Checked<'_>,
        host: &'static [Feature<'static>],
    ) -> Result<Self, Error> {
        if host.len() > u32::BITS as usize {
            return Err(Error::Invalid);
        }
        let mut bits = 0;
        for key in checked.available_features() {
            let index = host
                .iter()
                .position(|candidate| candidate.key == key)
                .ok_or(Error::Invalid)?;
            let mask = 1_u32
                .checked_shl(u32::try_from(index).map_err(|_| Error::Invalid)?)
                .ok_or(Error::Invalid)?;
            bits |= mask;
        }
        Ok(Self { bits, host })
    }

    /// Features for a host test: `bits` is the admitted set indexed by `host`.
    #[cfg(test)]
    pub(crate) const fn for_test(host: &'static [Feature<'static>], bits: u32) -> Self {
        Self { bits, host }
    }

    /// Enforce host support before checking this instance's admitted availability.
    ///
    /// # Errors
    /// Unsupported means the host has not wired the family. Denied means the
    /// instance did not request a revision the host supports.
    pub fn feature(self, key: &str) -> Result<(), i32> {
        let index = self
            .host
            .iter()
            .position(|candidate| candidate.key == key)
            .ok_or(ERR_UNSUPPORTED)?;
        let mask = u32::try_from(index)
            .ok()
            .and_then(|index| 1_u32.checked_shl(index))
            .ok_or(ERR_UNSUPPORTED)?;
        if self.bits & mask == 0 {
            Err(ERR_DENIED)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
    use super::*;
    use crate::native_host::shell::DeclaredServices;
    use alloc::vec::Vec;
    use paperpaw_native::metadata::{self, Feature, Host, Profile, Refusal};

    const REGISTRY: &str = USES_REGISTRY;
    const AVAILABLE: &[Feature<'static>] = &[Feature {
        key: REGISTRY,
        revision: 1,
    }];

    fn host(features: &'static [Feature<'static>]) -> Host<'static> {
        Host {
            abi_major: 7,
            minor: 1,
            profile: Profile {
                target: "test-only-le64",
                entry_layout: u16::MAX,
                entry_bytes: 64,
            },
            services: &[],
            features,
        }
    }

    fn empty() -> Vec<u8> {
        let hex =
            include_str!("../../../../abi/fixtures/metadata/valid-empty-requirements.hex").trim();
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
            .collect()
    }

    fn requested(revision: u16, required: bool) -> Vec<u8> {
        let mut bytes = empty();
        // Append one D1 feature record to the generated empty-requirement fixture.
        bytes.extend_from_slice(&u16::try_from(8 + REGISTRY.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&u16::try_from(REGISTRY.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&revision.to_le_bytes());
        bytes.extend_from_slice(&u16::from(required).to_le_bytes());
        bytes.extend_from_slice(REGISTRY.as_bytes());
        let length = u32::try_from(bytes.len()).unwrap();
        bytes[8..12].copy_from_slice(&length.to_le_bytes());
        bytes[28..30].copy_from_slice(&1_u16.to_le_bytes());
        bytes
    }

    #[test]
    fn supported_requested_features_survive_metadata_drop_as_four_bytes() {
        let available = AVAILABLE;
        let declared = {
            let bytes = requested(1, false);
            let checked = metadata::check(&bytes, &host(available)).unwrap();
            DeclaredServices::try_from_metadata(&checked, available).unwrap()
        };
        assert_eq!(size_of_val(&declared.features().bits), 4);
        assert_eq!(declared.count(), 0);
        assert_eq!(declared.features().feature(REGISTRY), Ok(()));
        assert_eq!(
            declared.features().feature("uses.unknown"),
            Err(ERR_UNSUPPORTED)
        );
        assert_eq!(
            DeclaredServices::none().features().feature(REGISTRY),
            Err(ERR_UNSUPPORTED)
        );
    }

    #[test]
    fn optional_unavailable_or_too_new_features_never_gain_a_bit() {
        let available = AVAILABLE;
        for (revision, features) in [(1, &[][..]), (2, available)] {
            let bytes = requested(revision, false);
            let checked = metadata::check(&bytes, &host(features)).unwrap();
            let declared = DeclaredServices::try_from_metadata(&checked, features).unwrap();
            assert_eq!(
                declared.features().feature(REGISTRY),
                if features.is_empty() {
                    Err(ERR_UNSUPPORTED)
                } else {
                    Err(ERR_DENIED)
                }
            );
        }
    }

    #[test]
    fn required_unavailable_feature_is_refused_before_retention() {
        let bytes = requested(1, true);
        assert_eq!(
            metadata::check(&bytes, &host(&[])).unwrap_err(),
            Refusal::RequiredFeature {
                key: REGISTRY,
                revision: 1
            }
        );
    }
    #[test]
    fn known_feature_not_requested_is_denied_and_key_matches_abi_catalog() {
        let bytes = empty();
        let checked = metadata::check(&bytes, &host(AVAILABLE)).unwrap();
        let declared = DeclaredServices::try_from_metadata(&checked, AVAILABLE).unwrap();
        assert_eq!(declared.features().feature(REGISTRY), Err(ERR_DENIED));
        let abi = include_str!("../../../../abi/abi.json");
        assert!(abi.contains(&alloc::format!(r#""key": "{USES_REGISTRY}""#)));
    }

    #[test]
    fn host_catalog_over_32_or_inconsistent_with_admission_is_refused() {
        const TOO_MANY: &[Feature<'static>] = &[Feature {
            key: REGISTRY,
            revision: 1,
        }; 33];
        let bytes = requested(1, false);
        let checked = metadata::check(&bytes, &host(AVAILABLE)).unwrap();
        assert!(matches!(
            AdmittedFeatures::from_checked(&checked, TOO_MANY),
            Err(Error::Invalid)
        ));
        assert!(matches!(
            AdmittedFeatures::from_checked(&checked, &[]),
            Err(Error::Invalid)
        ));
    }
    #[test]
    fn last_u32_bit_is_preserved_without_truncation() {
        const LAST: &[Feature<'static>] = &{
            let mut entries = [Feature {
                key: "test.other",
                revision: 1,
            }; 32];
            entries[31] = Feature {
                key: REGISTRY,
                revision: 1,
            };
            entries
        };
        let bytes = requested(1, false);
        let checked = metadata::check(&bytes, &host(LAST)).unwrap();
        let admitted = AdmittedFeatures::from_checked(&checked, LAST).unwrap();
        assert_eq!(admitted.bits, 1_u32 << 31);
        assert_eq!(admitted.feature(REGISTRY), Ok(()));
        assert_eq!(admitted.feature("test.other"), Err(ERR_DENIED));
    }

    #[test]
    fn required_revision_above_host_support_is_refused() {
        let bytes = requested(2, true);
        assert_eq!(
            metadata::check(&bytes, &host(AVAILABLE)).unwrap_err(),
            Refusal::RequiredFeature {
                key: REGISTRY,
                revision: 2
            }
        );
    }
}
