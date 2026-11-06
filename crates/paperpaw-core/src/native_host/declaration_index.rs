//! Compact name index shared by construction checks and guarded lookups.
use super::registrations::Record;

#[derive(Clone, Copy, Default)]
pub(crate) struct NameEntry {
    pub(crate) hash: u32,
    pub(crate) batch: u16,
    pub(crate) record_kind: u16,
}
impl NameEntry {
    pub(crate) fn kind(self) -> u8 {
        ((self.record_kind >> 3) & 7) as u8
    }
    pub(crate) fn is_extension(self) -> bool {
        self.record_kind & (1 << 6) != 0
    }
    pub(crate) fn record(self) -> usize {
        usize::from(self.record_kind & 7)
    }
    pub(crate) fn order(self) -> (u8, u32) {
        (self.kind(), self.hash)
    }
}
pub(crate) fn key(record: Record<'_>) -> Option<(u8, &str)> {
    Some(match record {
        Record::SettingGroup(r) => (0, r.name),
        Record::SettingSpec(r) => (1, r.name),
        Record::SettingExtra(r) => (2, r.name),
        Record::ResourceHyphenation(r) => (3, r.language),
        Record::ResourceFontFace(r) => (4, r.name),
        Record::ResourceFontExtension(r) => (4, r.name),
        _ => return None,
    })
}
pub(crate) fn same_name(kind: u8, a: &str, b: &str) -> bool {
    if kind == 3 {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}
/// FNV-1a over the full relative name; language tags are case-insensitive.
pub(crate) fn name_hash(kind: u8, name: &str) -> u32 {
    name.bytes().fold(2_166_136_261, |hash, byte| {
        let byte = if kind == 3 {
            byte.to_ascii_lowercase()
        } else {
            byte
        };
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}
const _: () = assert!(core::mem::size_of::<NameEntry>() == 8);

// The low three bits of record_kind hold the record index.
const _: () = assert!(super::registrations::REGISTRATION_MAX_RECORDS <= 8);
