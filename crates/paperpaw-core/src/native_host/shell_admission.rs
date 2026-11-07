//! What a loader reads from a format-5 plugin's metadata record before it
//! places the image: the record's body checked against what this host offers
//! (the same [`metadata::check`] the desktop uses), and the services the
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

/// Checks `record` against `host` and keeps its declared services.
///
/// # Errors
/// [`Admission`]: the check's refusal, an invalid list, or no room.
pub fn admit<'a>(record: &'a [u8], host: &Host<'_>) -> Result<Admitted, Admission<'a>> {
    let checked = metadata::check(record, host).map_err(Admission::Refused)?;
    let declared =
        DeclaredServices::try_from_iter(checked.services()).map_err(|error| match error {
            DeclaredServicesError::Invalid => Admission::Invalid,
            DeclaredServicesError::NoMemory => Admission::NoRoom,
        })?;
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
    use paperpaw_native::metadata::{Profile, Service};

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

    /// A metadata record (format 2) of a plugin for [`host`] that declares
    /// `services` (key, major, semantic revision, minimum table bytes).
    fn record(services: &[(&str, u16, u16, u32)]) -> Vec<u8> {
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
        out.extend_from_slice(&0u16.to_le_bytes()); // features
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
}
