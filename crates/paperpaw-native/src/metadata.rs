//! Bounded compatibility metadata admission shared by native shells.
//!
//! The wire fields follow abi/generated/metadata-layout.json in declaration
//! order. Descriptor and manifest tests pin every width and bound. Parsing
//! allocates nothing and returns borrowed keys in refusal diagnostics.

/// Maximum metadata size and declaration counts from the descriptor.
pub const MAX_BYTES: usize = 2048;
const PREFIX_SIZE: usize = 32;
const MAX_KEY_BYTES: usize = 128;
const MAX_SERVICES: usize = 32;
const MAX_FEATURES: usize = 32;
const EXPORT_SIZE: usize = 10;
const SERVICE_SIZE: usize = 12;
const FEATURE_SIZE: usize = 8;

pub use crate::profile::{BUILD_PROFILE, Profile, retarget, retargeted_len};
const _: () =
    assert!(PREFIX_SIZE == crate::profile::PREFIX_SIZE && MAX_BYTES == crate::profile::MAX_BYTES);

/// An implemented service. Every requirement field must match exactly.
#[derive(Clone, Copy, Debug)]
pub struct Service<'a> {
    pub key: &'a str,
    pub major: u16,
    pub semantic_revision: u16,
    pub min_table_bytes: u32,
}

/// An implemented semantic feature. Revisions are additive.
#[derive(Clone, Copy, Debug)]
pub struct Feature<'a> {
    pub key: &'a str,
    pub revision: u16,
}

/// Capabilities actually implemented by this shell, never inferred from slots.
#[derive(Clone, Copy, Debug)]
pub struct Host<'a> {
    pub abi_major: u16,
    pub minor: u16,
    pub profile: Profile<'a>,
    pub services: &'a [Service<'a>],
    pub features: &'a [Feature<'a>],
}

/// Refusal before executing any plugin code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal<'a> {
    /// Invalid framing, lengths, text, flags, counts or canonical order.
    Malformed,
    /// ABI version, target profile or entry layout is not supported.
    Incompatible,
    /// A service is absent or differs from its exact catalog contract.
    Service { key: &'a str },
    /// A required feature is absent or needs a newer revision.
    RequiredFeature { key: &'a str, revision: u16 },
}

/// A service requirement from validated admission metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declared<'a> {
    pub key: &'a str,
    pub major: u16,
    pub semantic_revision: u16,
}

/// Validated metadata. Optional unavailable features are absent from the result.
#[derive(Debug)]
pub struct Checked<'a> {
    pub abi_major: u16,
    pub needs_minor: u16,
    pub entry_bytes: u16,
    available: [Option<&'a str>; MAX_FEATURES],
    service_bytes: &'a [u8],
    feature_bytes: &'a [u8],
}

/// A feature the metadata asks for, available or not (`core.registry` op 20).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Requested<'a> {
    /// The feature key (`uses.*`).
    pub key: &'a str,
    /// The least revision it needs.
    pub revision: u16,
    /// Whether the plugin is refused without it (flags bit 0).
    pub required: bool,
}

/// The declared services of validated metadata, as the one span they were
/// checked in: a host keeps it where the metadata outlives it (a linked
/// plugin's, in the program's read-only data) instead of a copy.
#[derive(Clone, Copy, Debug)]
pub struct ServiceList<'a> {
    bytes: &'a [u8],
}

impl<'a> ServiceList<'a> {
    /// Declared services in canonical order, borrowing the validated bytes.
    /// Repeated iteration allocates nothing and retains no per-service array.
    pub fn iter(&self) -> impl Iterator<Item = Declared<'a>> + use<'a> {
        let mut cursor = Cursor {
            bytes: self.bytes,
            at: 0,
        };
        core::iter::from_fn(move || {
            // check validated every record before retaining this exact span.
            // Fallible reads also keep this visitor safe if framing changes.
            let len = cursor.key(SERVICE_SIZE).ok()?;
            let major = cursor.u16().ok()?;
            let semantic_revision = cursor.u16().ok()?;
            cursor.u32().ok()?;
            let key = cursor.text(len).ok()?;
            Some(Declared {
                key,
                major,
                semantic_revision,
            })
        })
    }
}

impl<'a> Checked<'a> {
    /// The declared services, as long as the metadata they were read from.
    pub fn service_list(&self) -> ServiceList<'a> {
        ServiceList {
            bytes: self.service_bytes,
        }
    }
}

impl Checked<'_> {
    /// Declared services in canonical order, borrowing the validated bytes.
    /// Repeated iteration allocates nothing and retains no per-service array.
    pub fn services(&self) -> impl Iterator<Item = Declared<'_>> {
        self.service_list().iter()
    }

    /// Available requested feature keys in canonical order, without allocation.
    pub fn available_features(&self) -> impl Iterator<Item = &str> {
        self.available.iter().flatten().copied()
    }

    /// Every requested feature in canonical order, the unavailable optional
    /// ones too, borrowing the validated bytes; nothing is allocated.
    pub fn requested_features(&self) -> impl Iterator<Item = Requested<'_>> {
        let mut cursor = Cursor {
            bytes: self.feature_bytes,
            at: 0,
        };
        core::iter::from_fn(move || {
            // check validated every record; fallible reads keep this safe.
            let len = cursor.key(FEATURE_SIZE).ok()?;
            let revision = cursor.u16().ok()?;
            let flags = cursor.u16().ok()?;
            let key = cursor.text(len).ok()?;
            Some(Requested {
                key,
                revision,
                required: flags & 1 != 0,
            })
        })
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], Refusal<'a>> {
        let end = self.at.checked_add(len).ok_or(Refusal::Malformed)?;
        let value = self.bytes.get(self.at..end).ok_or(Refusal::Malformed)?;
        self.at = end;
        Ok(value)
    }
    fn u16(&mut self) -> Result<u16, Refusal<'a>> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
    fn u32(&mut self) -> Result<u32, Refusal<'a>> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
    fn text(&mut self, len: usize) -> Result<&'a str, Refusal<'a>> {
        require((1..=MAX_KEY_BYTES).contains(&len))?;
        core::str::from_utf8(self.take(len)?).map_err(|_| Refusal::Malformed)
    }
    fn key(&mut self, size: usize) -> Result<usize, Refusal<'a>> {
        let total = usize::from(self.u16()?);
        let len = usize::from(self.u16()?);
        require(total == size + len)?;
        Ok(len)
    }
}

fn require(ok: bool) -> Result<(), Refusal<'static>> {
    if ok { Ok(()) } else { Err(Refusal::Malformed) }
}

struct Prefix {
    major: u16,
    minor: u16,
    layout: u16,
    entry_bytes: u16,
    target_bytes: usize,
    services: usize,
    features: usize,
}

fn prefix<'a>(cursor: &mut Cursor<'a>) -> Result<Prefix, Refusal<'a>> {
    require((PREFIX_SIZE..=MAX_BYTES).contains(&cursor.bytes.len()))?;
    require(cursor.take(4)? == b"PPAM")?;
    require(cursor.u16()? == 2)?;
    require(usize::from(cursor.u16()?) == PREFIX_SIZE)?;
    require(cursor.u32()? as usize == cursor.bytes.len())?;
    let major = cursor.u16()?;
    let minor = cursor.u16()?;
    let layout = cursor.u16()?;
    let entry_bytes = cursor.u16()?;
    require(cursor.u32()? == 0)?;
    let target_bytes = usize::from(cursor.u16()?);
    let services = usize::from(cursor.u16()?);
    let features = usize::from(cursor.u16()?);
    require(cursor.u16()? == 1)?;
    require(minor != 0 && services <= MAX_SERVICES && features <= MAX_FEATURES)?;
    Ok(Prefix {
        major,
        minor,
        layout,
        entry_bytes,
        target_bytes,
        services,
        features,
    })
}

fn export<'a>(cursor: &mut Cursor<'a>, entry_bytes: u16) -> Result<(), Refusal<'a>> {
    let total = usize::from(cursor.u16()?);
    require(cursor.take(2)? == [1, 0])?;
    require(cursor.u32()? == u32::from(entry_bytes))?;
    let len = usize::from(cursor.u16()?);
    require(total == EXPORT_SIZE + len)?;
    require(cursor.text(len)? == "PAPERPAW_ENTRY")
}

fn canonical<'a>(previous: &mut Option<&'a str>, key: &'a str) -> Result<(), Refusal<'a>> {
    require(previous.is_none_or(|old| old < key))?;
    *previous = Some(key);
    Ok(())
}

fn services<'a>(cursor: &mut Cursor<'a>, count: usize, host: &Host<'_>) -> Result<(), Refusal<'a>> {
    let mut previous = None;
    for _ in 0..count {
        let len = cursor.key(SERVICE_SIZE)?;
        let major = cursor.u16()?;
        let semantic_revision = cursor.u16()?;
        let min_table_bytes = cursor.u32()?;
        let key = cursor.text(len)?;
        canonical(&mut previous, key)?;
        if !host.services.iter().any(|service| {
            service.key == key
                && service.major == major
                && service.semantic_revision == semantic_revision
                && service.min_table_bytes == min_table_bytes
        }) {
            return Err(Refusal::Service { key });
        }
    }
    Ok(())
}

/// Check the entire bounded metadata record against implemented capabilities.
/// No data is accepted or exposed until every record has been validated.
///
/// # Errors
/// Returns the offending required feature/service key, or a framing/version refusal.
// One copy of the reader, not one per caller: it is not on a hot path.
#[inline(never)]
pub fn check<'a>(bytes: &'a [u8], host: &Host<'_>) -> Result<Checked<'a>, Refusal<'a>> {
    let mut cursor = Cursor { bytes, at: 0 };
    let prefix = prefix(&mut cursor)?;
    let target = cursor.text(prefix.target_bytes)?;
    if prefix.major != host.abi_major
        || prefix.minor > host.minor
        || target != host.profile.target
        || prefix.layout != host.profile.entry_layout
        || prefix.entry_bytes != host.profile.entry_bytes
    {
        return Err(Refusal::Incompatible);
    }
    export(&mut cursor, prefix.entry_bytes)?;
    let services_start = cursor.at;
    services(&mut cursor, prefix.services, host)?;
    let service_bytes = &bytes[services_start..cursor.at];
    let mut available = [None; MAX_FEATURES];
    let mut previous = None;
    let features_start = cursor.at;
    for slot in available.iter_mut().take(prefix.features) {
        let len = cursor.key(FEATURE_SIZE)?;
        let revision = cursor.u16()?;
        let flags = cursor.u16()?;
        require(flags & !1 == 0)?;
        let key = cursor.text(len)?;
        canonical(&mut previous, key)?;
        if host
            .features
            .iter()
            .any(|feature| feature.key == key && revision <= feature.revision)
        {
            *slot = Some(key);
        } else if flags & 1 != 0 {
            return Err(Refusal::RequiredFeature { key, revision });
        }
    }
    let feature_bytes = bytes
        .get(features_start..cursor.at)
        .ok_or(Refusal::Malformed)?;
    require(cursor.at == bytes.len())?;
    Ok(Checked {
        abi_major: prefix.major,
        needs_minor: prefix.minor,
        entry_bytes: prefix.entry_bytes,
        available,
        service_bytes,
        feature_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn layout(value: &Value, fields: &[(&str, usize)], size: usize) {
        let actual = value["fields"].as_array().unwrap();
        assert_eq!(actual.len(), fields.len());
        let mut offset = 0;
        for (field, (name, width)) in actual.iter().zip(fields) {
            assert_eq!(field["name"], *name);
            assert_eq!(field["width"], *width);
            assert_eq!(field["offset"], offset);
            offset += width;
        }
        assert_eq!(offset, size);
        assert_eq!(value["size"], size);
    }

    #[test]
    fn sequential_reader_matches_the_generated_descriptor() {
        let value: Value =
            serde_json::from_str(include_str!("../../../abi/generated/metadata-layout.json"))
                .unwrap();
        let descriptor = &value["descriptor"];
        assert_eq!(descriptor["magic"], "PPAM");
        assert_eq!(descriptor["format"], 2);
        for (name, bound) in [
            ("max_bytes", MAX_BYTES),
            ("max_key_bytes", MAX_KEY_BYTES),
            ("max_services", MAX_SERVICES),
            ("max_features", MAX_FEATURES),
            ("max_exports", 1),
        ] {
            assert_eq!(descriptor[name], bound);
        }
        layout(
            &value["prefix"],
            &[
                ("magic", 4),
                ("layout", 2),
                ("prefix_bytes", 2),
                ("total_bytes", 4),
                ("abi_major", 2),
                ("needs_minor", 2),
                ("entry_layout", 2),
                ("entry_bytes", 2),
                ("flags", 4),
                ("target_bytes", 2),
                ("services", 2),
                ("features", 2),
                ("exports", 2),
            ],
            PREFIX_SIZE,
        );
        layout(
            &value["records"]["export"],
            &[
                ("record_bytes", 2),
                ("kind", 1),
                ("reserved", 1),
                ("expected_bytes", 4),
                ("name_bytes", 2),
            ],
            EXPORT_SIZE,
        );
        layout(
            &value["records"]["service"],
            &[
                ("record_bytes", 2),
                ("key_bytes", 2),
                ("major", 2),
                ("semantic_revision", 2),
                ("min_table_bytes", 4),
            ],
            SERVICE_SIZE,
        );
        layout(
            &value["records"]["feature"],
            &[
                ("record_bytes", 2),
                ("key_bytes", 2),
                ("revision", 2),
                ("flags", 2),
            ],
            FEATURE_SIZE,
        );
    }
}
