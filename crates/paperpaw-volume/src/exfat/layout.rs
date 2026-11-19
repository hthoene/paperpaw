//! exFAT's on-disk constants, checksums, name hash and up-case table.
//!
//! The constants, the boot sector checks, the checksums, the name hash and
//! the up-case table (`upcase.bin`, the specification's recommended table)
//! come from hadris-fat-raw 0.3.0 (hadris 3.0.0-rc.3, MIT, The Hadris Project
//! Contributors, see `LICENSE-hadris.txt`), written anew over byte slices:
//! no `bytemuck`, no tables in RAM.

pub(crate) const VOLUME_DIRTY: u16 = 0x0002;

pub(crate) const FAT_END: u32 = 0xFFFF_FFFF;
pub(crate) const MAX_CLUSTER_COUNT: u32 = 0xFFFF_FFF5;

pub(crate) const ENTRY: usize = 32;
/// The device's block: what one write puts on the card whole or not at all.
pub(crate) const BLOCK: u64 = 512;
pub(crate) const IN_USE: u8 = 0x80;
pub(crate) const SECONDARY: u8 = 0x40;
pub(crate) const ENTRY_BITMAP: u8 = 0x81;
pub(crate) const ENTRY_UPCASE: u8 = 0x82;
pub(crate) const ENTRY_LABEL: u8 = 0x83;
pub(crate) const ENTRY_FILE: u8 = 0x85;
pub(crate) const ENTRY_STREAM: u8 = 0xC0;
pub(crate) const ENTRY_NAME: u8 = 0xC1;
/// An entry no longer in use that is not the directory's end: put where a
/// slot is passed over.
pub(crate) const ENTRY_UNUSED: u8 = 0x05;

pub(crate) const ATTR_DIRECTORY: u16 = 0x0010;
pub(crate) const ATTR_ARCHIVE: u16 = 0x0020;

pub(crate) const ALLOCATION_POSSIBLE: u8 = 0x01;
pub(crate) const NO_FAT_CHAIN: u8 = 0x02;

pub(crate) const NAME_UNITS: usize = 15;
pub(crate) const MAX_NAME: usize = 255;
/// The entries of the longest entry set: the file entry, its stream entry
/// and 17 name entries.
pub(crate) const MAX_SET: usize = 19;
pub(crate) const MAX_DIRECTORY: u64 = 256 << 20;

/// 1980-01-01 00:00:00, the time of a device without a clock.
pub(crate) const NO_TIME: u32 = (1 << 21) | (1 << 16);

pub(crate) fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

pub(crate) fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32_at(bytes, at)) | (u64::from(u32_at(bytes, at + 4)) << 32)
}

/// What a boot sector says of its volume, in bytes from the volume's start.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geometry {
    pub(crate) fat: u64,
    pub(crate) heap: u64,
    pub(crate) cluster_shift: u8,
    pub(crate) clusters: u32,
    pub(crate) root: u32,
    pub(crate) flags: u16,
}

/// Checks a boot sector and gives its geometry; `None` where a field is
/// wrong, or the volume keeps two FATs (TexFAT, which nothing here writes).
pub(crate) fn parse_boot(boot: &[u8; 512]) -> Option<Geometry> {
    if &boot[3..11] != b"EXFAT   " || u16_at(boot, 510) != 0xAA55 {
        return None;
    }
    if boot[11..64].iter().any(|&b| b != 0) || boot[105] != 1 {
        return None;
    }
    let sector_shift = boot[108];
    let per_cluster_shift = boot[109];
    if !(9..=12).contains(&sector_shift) || per_cluster_shift > 25 - sector_shift {
        return None;
    }
    if boot[110] != 1 {
        return None;
    }
    let sector = 1u64 << sector_shift;
    let volume_len = u64_at(boot, 72);
    let fat_offset = u64::from(u32_at(boot, 80));
    let fat_length = u64::from(u32_at(boot, 84));
    let heap_offset = u64::from(u32_at(boot, 88));
    let count = u32_at(boot, 92);
    if volume_len < ((1 << 20) >> sector_shift) || volume_len.checked_mul(sector).is_none() {
        return None;
    }
    if fat_offset < 24 || fat_offset + fat_length > heap_offset {
        return None;
    }
    if (u64::from(count) + 2) * 4 > fat_length * sector || heap_offset > volume_len {
        return None;
    }
    if count == 0
        || count > MAX_CLUSTER_COUNT
        || u64::from(count) > (volume_len - heap_offset) >> per_cluster_shift
    {
        return None;
    }
    let root = u32_at(boot, 96);
    if root < 2 || root - 2 >= count {
        return None;
    }
    Some(Geometry {
        fat: fat_offset * sector,
        heap: heap_offset * sector,
        cluster_shift: sector_shift + per_cluster_shift,
        clusters: count,
        root,
        flags: u16_at(boot, 106),
    })
}

/// The `SetChecksum` of the entries in `set` (whole entries).
pub(crate) fn set_checksum(set: &[u8]) -> u16 {
    let mut sum = 0u16;
    for (at, &byte) in set.iter().enumerate() {
        if at == 2 || at == 3 {
            continue;
        }
        sum = sum.rotate_right(1).wrapping_add(u16::from(byte));
    }
    sum
}

/// Adds one up-cased code unit to a `NameHash` that started at 0.
pub(crate) fn hash_unit(mut hash: u16, unit: u16) -> u16 {
    for byte in unit.to_le_bytes() {
        hash = hash.rotate_right(1).wrapping_add(u16::from(byte));
    }
    hash
}

/// Whether `unit` may appear in a file name.
pub(crate) fn valid_unit(unit: u16) -> bool {
    unit >= 0x20
        && !matches!(
            unit,
            0x22 | 0x2A | 0x2F | 0x3A | 0x3C | 0x3E | 0x3F | 0x5C | 0x7C
        )
}

/// The specification's recommended up-case table, compressed as on a card:
/// a mapping for each code unit in turn, or `0xFFFF` and how many units
/// map to themselves.
pub(crate) static UPCASE: &[u8; 5836] = include_bytes!("upcase.bin");
/// The `TableChecksum` of [`UPCASE`].
pub(crate) const UPCASE_CHECKSUM: u32 = 0xE619_D30D;

/// `unit` in upper case, as every formatter's table says (the recommended
/// one). The card's own table is not read: it takes up to 128 KiB, and
/// only a table that differs from the recommended one above U+007F would
/// tell another name hash.
pub(crate) fn upcase(unit: u16) -> u16 {
    if unit < 0x80 {
        return if (0x61..=0x7A).contains(&unit) {
            unit - 0x20
        } else {
            unit
        };
    }
    let want = u32::from(unit);
    let mut code = 0u32;
    let mut at = 0;
    while at + 1 < UPCASE.len() && code <= want {
        let entry = u16_at(UPCASE, at);
        at += 2;
        if entry == 0xFFFF && code != 0xFFFF && at + 1 < UPCASE.len() {
            code += u32::from(u16_at(UPCASE, at));
            at += 2;
            if want < code {
                return unit;
            }
        } else {
            if code == want {
                return entry;
            }
            code += 1;
        }
    }
    unit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upcases_as_the_recommended_table() {
        assert_eq!(upcase(u16::from(b'a')), u16::from(b'A'));
        assert_eq!(upcase(u16::from(b'~')), u16::from(b'~'));
        assert_eq!(upcase(0xE9), 0xC9);
        assert_eq!(upcase(0x3B1), 0x391);
        assert_eq!(upcase(0x430), 0x410);
        assert_eq!(upcase(0xFF41), 0xFF21);
        assert_eq!(upcase(0xFFFF), 0xFFFF);
        assert_eq!(upcase(0x4E2D), 0x4E2D);
        // The table's checksum, as the specification gives it.
        let sum = UPCASE.iter().fold(0u32, |sum, &b| {
            sum.rotate_right(1).wrapping_add(u32::from(b))
        });
        assert_eq!(sum, UPCASE_CHECKSUM);
    }
}
