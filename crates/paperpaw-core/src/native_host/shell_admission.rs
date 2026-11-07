//! What a loader reads from a format-5 plugin's metadata record before it
//! places the image: the record's body checked against what this host offers
//! (the same [`paperpaw_native::metadata::check`] the desktop uses), and the services the
//! plugin declares, copied for `Hosted::new` (a query of a service the plugin
//! did not declare is refused there).
//!
//! The record is the loader's own bytes (read from the file, at most 2 KiB);
//! this only borrows them, and allocates fallibly for what it keeps.
use paperpaw_native::metadata::{self, Host, Refusal};

use super::{DeclaredServices, DeclaredServicesError};

/// Why a record is not admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission<'a> {
    /// The metadata check refused it (malformed, another profile, a service
    /// the host does not offer as declared, a required feature).
    Refused(Refusal<'a>),
    /// Its declarations are not a list this host keeps (too many, unordered).
    Invalid,
    /// No room to keep the declarations.
    NoRoom,
}

/// What an admitted record says: the services it declares and the prefix
/// facts the loader compares with the file's header (read once, by the
/// check).
#[derive(Debug)]
pub struct Admitted {
    pub declared: DeclaredServices,
    pub abi_major: u16,
    pub needs_minor: u16,
    pub entry_bytes: u16,
}

/// Checks `record` against `host` and keeps its declared services and the
/// features of `host` it asked for (`DeclaredServices::features`): a host
/// that advertises a feature checks it there, per instance, before it serves
/// what the feature gates. `host.features` is the shell's static catalog and
/// the very one the check uses.
///
/// # Errors
/// [`Admission`]: the check's refusal, an invalid list, or no room.
pub fn admit<'a>(record: &'a [u8], host: &Host<'static>) -> Result<Admitted, Admission<'a>> {
    let checked = metadata::check(record, host).map_err(Admission::Refused)?;
    let declared = DeclaredServices::try_from_metadata(&checked, host.features).map_err(
        |error| match error {
            DeclaredServicesError::Invalid => Admission::Invalid,
            DeclaredServicesError::NoMemory => Admission::NoRoom,
        },
    )?;
    Ok(Admitted {
        declared,
        abi_major: checked.abi_major,
        needs_minor: checked.needs_minor,
        entry_bytes: checked.entry_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use paperpaw_native::metadata::{Feature, Profile, Service};

    const TARGET: &str = "esp32c3-riscv32imc-ilp32-soft-le";
    const OFFERED: &[Service<'static>] = &[
        Service {
            key: "core.plugin",
            major: 1,
            semantic_revision: 0,
            min_table_bytes: 0,
        },
        Service {
            key: "core.usage",
            major: 1,
            semantic_revision: 0,
            min_table_bytes: 0,
        },
    ];

    const REGISTRY: &str = "uses.registry";
    const DEVICE_WIFI: &str = "uses.wifi";
    /// What a shell that wired two families would advertise.
    const WIRED: &[Feature<'static>] = &[
        Feature {
            key: REGISTRY,
            revision: 1,
        },
        Feature {
            key: DEVICE_WIFI,
            revision: 1,
        },
    ];

    fn host() -> Host<'static> {
        Host {
            abi_major: 7,
            minor: 3,
            profile: Profile {
                target: TARGET,
                entry_layout: 1,
                entry_bytes: 20,
            },
            services: OFFERED,
            features: &[],
        }
    }

    fn wired() -> Host<'static> {
        Host {
            features: WIRED,
            ..host()
        }
    }

    /// A metadata record (format 2) of a plugin for [`host`] that declares
    /// `services` (key, major, semantic revision, minimum table bytes).
    fn record(services: &[(&str, u16, u16, u32)]) -> Vec<u8> {
        record_with(services, &[])
    }

    /// As [`record`], also asking for `features` (key, revision, required).
    fn record_with(services: &[(&str, u16, u16, u32)], features: &[(&str, u16, bool)]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(TARGET.as_bytes());
        // The entry export: total, kind 1 / reserved 0, entry bytes, name.
        let name = b"PAPERPAW_ENTRY";
        body.extend_from_slice(&(10 + name.len() as u16).to_le_bytes());
        body.extend_from_slice(&[1, 0]);
        body.extend_from_slice(&20u32.to_le_bytes());
        body.extend_from_slice(&(name.len() as u16).to_le_bytes());
        body.extend_from_slice(name);
        for &(key, major, revision, table) in services {
            body.extend_from_slice(&(12 + key.len() as u16).to_le_bytes());
            body.extend_from_slice(&(key.len() as u16).to_le_bytes());
            body.extend_from_slice(&major.to_le_bytes());
            body.extend_from_slice(&revision.to_le_bytes());
            body.extend_from_slice(&table.to_le_bytes());
            body.extend_from_slice(key.as_bytes());
        }
        for &(key, revision, required) in features {
            body.extend_from_slice(&(8 + key.len() as u16).to_le_bytes());
            body.extend_from_slice(&(key.len() as u16).to_le_bytes());
            body.extend_from_slice(&revision.to_le_bytes());
            body.extend_from_slice(&u16::from(required).to_le_bytes());
            body.extend_from_slice(key.as_bytes());
        }
        let total = 32 + body.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"PPAM");
        out.extend_from_slice(&2u16.to_le_bytes()); // format
        out.extend_from_slice(&32u16.to_le_bytes()); // prefix bytes
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&7u16.to_le_bytes()); // abi major
        out.extend_from_slice(&1u16.to_le_bytes()); // needs minor
        out.extend_from_slice(&1u16.to_le_bytes()); // entry layout
        out.extend_from_slice(&20u16.to_le_bytes()); // entry bytes
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        out.extend_from_slice(&(TARGET.len() as u16).to_le_bytes());
        out.extend_from_slice(&(services.len() as u16).to_le_bytes());
        out.extend_from_slice(&(features.len() as u16).to_le_bytes()); // features
        out.extend_from_slice(&1u16.to_le_bytes()); // exports
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn a_declared_service_can_be_queried_and_an_undeclared_one_is_refused() {
        let bytes = record(&[("core.plugin", 1, 0, 0)]);
        let admitted = admit(&bytes, &host()).unwrap();
        let declared = admitted.declared;
        assert_eq!(
            (
                admitted.abi_major,
                admitted.needs_minor,
                admitted.entry_bytes
            ),
            (7, 1, 20)
        );
        assert_eq!(declared.count(), 1);
        assert_eq!(declared.declared("core.plugin"), Some((1, 0)));
        // Offered by the host but not declared by this plugin: not its to ask.
        assert_eq!(declared.declared("core.usage"), None);
    }

    #[test]
    fn a_plugin_that_declares_nothing_has_nothing_to_query() {
        let bytes = record(&[]);
        let declared = admit(&bytes, &host()).unwrap().declared;
        assert_eq!(declared.count(), 0);
        assert_eq!(declared.declared("core.plugin"), None);
    }

    #[test]
    fn a_service_the_host_does_not_offer_refuses_the_plugin_with_its_key() {
        let bytes = record(&[("core.nothing", 1, 0, 0)]);
        assert_eq!(
            admit(&bytes, &host()).unwrap_err(),
            Admission::Refused(Refusal::Service {
                key: "core.nothing"
            })
        );
        // Another revision than the catalog's is not the same service.
        let bytes = record(&[("core.plugin", 1, 1, 0)]);
        assert!(matches!(
            admit(&bytes, &host()).unwrap_err(),
            Admission::Refused(Refusal::Service { .. })
        ));
    }

    #[test]
    fn an_unordered_list_or_a_record_for_another_profile_is_refused() {
        let bytes = record(&[("core.usage", 1, 0, 0), ("core.plugin", 1, 0, 0)]);
        assert_eq!(
            admit(&bytes, &host()).unwrap_err(),
            Admission::Refused(Refusal::Malformed)
        );
        let mut other = host();
        other.profile.target = "linux-x86_64-sysv64-le";
        let bytes = record(&[]);
        assert_eq!(
            admit(&bytes, &other).unwrap_err(),
            Admission::Refused(Refusal::Incompatible)
        );
    }

    #[test]
    fn a_record_cut_short_or_with_a_byte_too_many_is_refused() {
        let bytes = record(&[("core.plugin", 1, 0, 0)]);
        assert_eq!(
            admit(&bytes[..bytes.len() - 1], &host()).unwrap_err(),
            Admission::Refused(Refusal::Malformed)
        );
        let mut long = bytes;
        long.push(0);
        assert_eq!(
            admit(&long, &host()).unwrap_err(),
            Admission::Refused(Refusal::Malformed)
        );
    }

    #[test]
    fn a_feature_is_refused_while_its_bit_is_not_kept_and_served_once_it_is() {
        use paperpaw_native::host_glue::{ERR_DENIED, ERR_UNSUPPORTED};
        // The plugin asks for nothing: the host's feature is advertised, and
        // this instance has no bit for it.
        let bytes = record(&[]);
        let features = admit(&bytes, &wired()).unwrap().declared.features();
        assert_eq!(features.feature(REGISTRY), Err(ERR_DENIED));
        assert_eq!(features.feature(DEVICE_WIFI), Err(ERR_DENIED));
        // Asking for one keeps that bit alone.
        let bytes = record_with(&[], &[(REGISTRY, 1, false)]);
        let features = admit(&bytes, &wired()).unwrap().declared.features();
        assert_eq!(features.feature(REGISTRY), Ok(()));
        assert_eq!(features.feature(DEVICE_WIFI), Err(ERR_DENIED));
        // A required one is the same; the bits survive the record's drop.
        let bytes = record_with(&[], &[(DEVICE_WIFI, 1, true), (REGISTRY, 1, false)]);
        let features = admit(&bytes, &wired()).unwrap().declared.features();
        drop(bytes);
        assert_eq!(features.feature(REGISTRY), Ok(()));
        assert_eq!(features.feature(DEVICE_WIFI), Ok(()));
        // A feature the host does not advertise is unsupported, not denied.
        assert_eq!(features.feature("uses.other"), Err(ERR_UNSUPPORTED));
    }

    #[test]
    fn a_host_that_advertises_nothing_keeps_no_bit_and_refuses_a_required_feature() {
        use paperpaw_native::host_glue::ERR_UNSUPPORTED;
        let bytes = record_with(&[], &[(REGISTRY, 1, false)]);
        let features = admit(&bytes, &host()).unwrap().declared.features();
        assert_eq!(features.feature(REGISTRY), Err(ERR_UNSUPPORTED));
        let bytes = record_with(&[], &[(REGISTRY, 1, true)]);
        assert_eq!(
            admit(&bytes, &host()).unwrap_err(),
            Admission::Refused(Refusal::RequiredFeature {
                key: REGISTRY,
                revision: 1
            })
        );
        // A revision newer than the host's: required is refused, optional is
        // kept without a bit.
        let bytes = record_with(&[], &[(REGISTRY, 2, true)]);
        assert!(matches!(
            admit(&bytes, &wired()).unwrap_err(),
            Admission::Refused(Refusal::RequiredFeature { .. })
        ));
        let bytes = record_with(&[], &[(REGISTRY, 2, false)]);
        let features = admit(&bytes, &wired()).unwrap().declared.features();
        assert!(features.feature(REGISTRY).is_err());
    }
}
