//! Which driver owns a volume: read from the boot sector at mount.

/// A file system this crate can mount.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum VolumeKind {
    /// FAT32 (and, read-only in spirit, FAT12/16): [`crate::FileSystem`].
    Fat,
    /// exFAT: the second driver, behind the same interface.
    ExFat,
}

/// Tells which driver owns the volume whose first sector is `boot`.
///
/// `None` means a sector no driver knows: a damaged volume, not a FAT one.
#[must_use]
pub fn detect(boot: &[u8]) -> Option<VolumeKind> {
    if boot.len() < 512 || boot[510] != 0x55 || boot[511] != 0xAA {
        return None;
    }
    // exFAT names itself in the OEM field and keeps the BPB area zero.
    if &boot[3..11] == b"EXFAT   " {
        return Some(VolumeKind::ExFat);
    }
    // A FAT boot sector starts with a jump and has a power-of-two sector size.
    let sector = u16::from_le_bytes([boot[11], boot[12]]);
    if (boot[0] == 0xEB || boot[0] == 0xE9) && sector.is_power_of_two() && sector >= 512 {
        return Some(VolumeKind::Fat);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_the_types_apart() {
        let mut s = [0_u8; 512];
        s[510] = 0x55;
        s[511] = 0xAA;
        assert_eq!(detect(&s), None);
        s[0] = 0xEB;
        s[11..13].copy_from_slice(&512_u16.to_le_bytes());
        assert_eq!(detect(&s), Some(VolumeKind::Fat));
        s[3..11].copy_from_slice(b"EXFAT   ");
        s[11..13].fill(0);
        assert_eq!(detect(&s), Some(VolumeKind::ExFat));
        s[511] = 0;
        assert_eq!(detect(&s), None);
        assert_eq!(detect(&s[..100]), None);
    }
}
