//! The device key behind `core.secrets` on the X4 (decision 138): 32 random
//! bytes kept in the flash's `nvs` partition, never on the card, so secrets
//! sealed with it do not move with the card. One record at [`OFFSET`] into the
//! partition (the web flasher's Wi-Fi record, [`crate::provision`], is in the
//! first sector; this is the fourth, `0xC000` with the partition at `0x9000`):
//!
//! ```text
//! MAGIC (8) | key (32) | CRC-32 of the first 40 bytes, little endian (4)
//! ```
//!
//! What the firmware does with what [`read`] finds:
//! - [`Sector::Key`]: use it. A key that reads is never replaced.
//! - [`Sector::Erased`] or [`Sector::Damaged`]: nothing sealed under it can be
//!   opened, so a first seal makes a new key (only from a strong random
//!   source) and writes [`record`] after erasing the sector; opening answers
//!   "absent".
//! - A flash that cannot be read says nothing about the sector: nothing is
//!   erased then.
//!
//! The sealed value is `nonce (12) | ciphertext | tag (16)` ([`split`]).

/// What the record begins with.
pub const MAGIC: [u8; 8] = *b"PPDEVKY1";
/// The key's length: AES-256.
pub const KEY: usize = 32;
/// The record's length (a multiple of 4: the flash writes words).
pub const LEN: usize = MAGIC.len() + KEY + 4;
/// The sector the record is in, and its offset into the `nvs` partition (the
/// partition has four sectors, five on the X4 Classic; the first is the Wi-Fi
/// record's).
pub const SECTOR: usize = 4096;
/// Where in the `nvs` partition the record is.
pub const OFFSET: u32 = 3 * SECTOR as u32;

/// The sealed value's nonce.
pub const NONCE: usize = 12;
/// The sealed value's authentication tag.
pub const TAG: usize = 16;
/// The longest plain value (`core.secrets`).
pub const MAX_PLAIN: usize = 1024;
/// The longest sealed value.
pub const MAX_SEALED: usize = NONCE + MAX_PLAIN + TAG;

/// What the key's sector holds.
#[derive(PartialEq, Eq)]
pub enum Sector {
    /// A key.
    Key([u8; KEY]),
    /// Nothing written: the erased flash.
    Erased,
    /// Our own record that does not read: it begins with [`MAGIC`] (or a
    /// prefix of it, the rest erased: a write cut short) but the checksum
    /// fails or it is too short. No key; the sector is ours to erase.
    Damaged,
    /// Anything else: data that is not ours (an NVS page of the stock
    /// firmware, of the factory's NVS). No key, and the sector is **never
    /// erased or written** over it.
    Foreign,
}

/// Never prints the key (a log line or a failed assertion must not show it).
impl core::fmt::Debug for Sector {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Key(_) => f.write_str("Key(<redacted>)"),
            Self::Erased => f.write_str("Erased"),
            Self::Damaged => f.write_str("Damaged"),
            Self::Foreign => f.write_str("Foreign"),
        }
    }
}

/// The first [`LEN`] bytes of the sector, as read.
#[must_use]
pub fn read(bytes: &[u8]) -> Sector {
    let Some(bytes) = bytes.get(..LEN) else {
        return Sector::Damaged;
    };
    if bytes.iter().all(|&b| b == 0xFF) {
        return Sector::Erased;
    }
    let (body, crc) = bytes.split_at(LEN - 4);
    if !body.starts_with(&MAGIC) {
        // A write cut short inside the magic: a prefix of it, then erased.
        let kept = bytes
            .iter()
            .zip(MAGIC)
            .take_while(|&(&have, want)| have == want)
            .count();
        let torn = kept > 0 && bytes[kept..].iter().all(|&b| b == 0xFF);
        return if torn {
            Sector::Damaged
        } else {
            Sector::Foreign
        };
    }
    if crc != crc32(body).to_le_bytes() {
        return Sector::Damaged;
    }
    let mut key = [0u8; KEY];
    key.copy_from_slice(&body[MAGIC.len()..]);
    Sector::Key(key)
}

/// The record that keeps `key`.
#[must_use]
pub fn record(key: &[u8; KEY]) -> [u8; LEN] {
    let mut out = [0u8; LEN];
    out[..MAGIC.len()].copy_from_slice(&MAGIC);
    out[MAGIC.len()..LEN - 4].copy_from_slice(key);
    let crc = crc32(&out[..LEN - 4]);
    out[LEN - 4..].copy_from_slice(&crc.to_le_bytes());
    out
}

/// A sealed value's parts: nonce, ciphertext, tag. `None` when it is too short
/// or longer than [`MAX_SEALED`].
#[must_use]
pub fn split(sealed: &[u8]) -> Option<(&[u8; NONCE], &[u8], &[u8; TAG])> {
    if sealed.len() < NONCE + TAG || sealed.len() > MAX_SEALED {
        return None;
    }
    let (nonce, rest) = sealed.split_at(NONCE);
    let (cipher, tag) = rest.split_at(rest.len() - TAG);
    Some((nonce.try_into().ok()?, cipher, tag.try_into().ok()?))
}

/// CRC-32 (IEEE), bit by bit: 40 bytes, once per read.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The web flasher keeps the key through a reinstall (decision 139):
    /// paperpaw.org `static/flash/device.js` has `NVS_AT = 0x9000` and
    /// `KEY_AT` (this record's sector, `0xC000`) and skips that sector in
    /// every write. A change here, or in a table's `nvs` line, must change
    /// there too; its own test reads `SECTOR`, `OFFSET` and these tables.
    #[test]
    fn the_key_sector_is_where_the_web_flasher_keeps_it() {
        const NVS_AT: u32 = 0x9000;
        const KEY_AT: u32 = 0xC000;
        assert_eq!(NVS_AT + OFFSET, KEY_AT);
        assert_eq!(SECTOR, 4096);
        for (name, table) in [
            (
                "partitions.csv",
                include_str!("../../../shells/xteink/partitions.csv"),
            ),
            (
                "partitions-classic.csv",
                include_str!("../../../shells/xteink/partitions-classic.csv"),
            ),
        ] {
            let nvs = table
                .lines()
                .find(|line| line.trim_start().starts_with("nvs,"))
                .unwrap_or_else(|| panic!("{name}: no nvs line"));
            let field = |n: usize| {
                let text = nvs.split(',').nth(n).unwrap().trim();
                u32::from_str_radix(text.trim_start_matches("0x"), 16).unwrap()
            };
            let (at, size) = (field(3), field(4));
            assert_eq!(at, NVS_AT, "{name}: nvs begins elsewhere");
            assert!(
                at + size >= KEY_AT + SECTOR as u32,
                "{name}: nvs does not reach the key sector"
            );
        }
    }

    fn key() -> [u8; KEY] {
        core::array::from_fn(|i| i as u8 ^ 0x5A)
    }

    #[test]
    fn debug_never_shows_the_key() {
        use alloc::format;
        assert_eq!(format!("{:?}", Sector::Key(key())), "Key(<redacted>)");
        assert_eq!(format!("{:?}", Sector::Erased), "Erased");
        assert_eq!(format!("{:?}", Sector::Damaged), "Damaged");
        assert_eq!(format!("{:?}", Sector::Foreign), "Foreign");
    }

    #[test]
    fn crc32_is_the_ieee_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn a_record_gives_its_key_back() {
        let mut sector = [0xFF; SECTOR];
        sector[..LEN].copy_from_slice(&record(&key()));
        assert_eq!(read(&sector), Sector::Key(key()));
    }

    #[test]
    fn erased_flash_is_no_key_and_is_not_damage() {
        assert_eq!(read(&[0xFF; SECTOR]), Sector::Erased);
    }

    #[test]
    fn any_changed_bit_of_our_record_is_damage_and_one_in_the_magic_is_not_ours() {
        let good = record(&key());
        for at in 0..LEN {
            for bit in 0..8 {
                let mut bad = good;
                bad[at] ^= 1 << bit;
                // Past the magic the record is still ours, with a bad
                // checksum; in it the bytes are no longer ours to erase.
                let expected = if at < MAGIC.len() {
                    Sector::Foreign
                } else {
                    Sector::Damaged
                };
                assert_eq!(read(&bad), expected, "byte {at} bit {bit}");
            }
        }
    }

    #[test]
    fn a_write_cut_short_is_damage() {
        let good = record(&key());
        // Power went at some byte: the rest is still erased.
        for cut in 1..LEN {
            let mut torn = [0xFF; LEN];
            torn[..cut].copy_from_slice(&good[..cut]);
            assert_ne!(read(&torn), Sector::Key(key()), "cut at {cut}");
        }
        assert_eq!(read(&good[..LEN - 1]), Sector::Damaged);
        assert_eq!(read(&[]), Sector::Damaged);
    }

    #[test]
    fn foreign_data_is_foreign_and_never_ours_to_erase() {
        assert_eq!(read(&[0u8; SECTOR]), Sector::Foreign);
        assert_eq!(read(&[0x5A; LEN]), Sector::Foreign);
        // An NVS page of the ESP-IDF (state word, sequence number, entries):
        // the factory's or the stock firmware's pages at the same place.
        let mut nvs = [0xFF_u8; SECTOR];
        nvs[..8].copy_from_slice(&[0xFE, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00]);
        nvs[32..48].copy_from_slice(b"\x01\x01\x01\xFFhw_calib\0\0\0\0");
        assert_eq!(read(&nvs), Sector::Foreign);
    }

    #[test]
    fn a_torn_prefix_of_the_magic_is_ours() {
        for kept in 1..MAGIC.len() {
            let mut torn = [0xFF_u8; LEN];
            torn[..kept].copy_from_slice(&MAGIC[..kept]);
            assert_eq!(read(&torn), Sector::Damaged, "{kept} bytes");
        }
    }

    #[test]
    fn a_sealed_value_has_its_parts() {
        let mut sealed = [0u8; NONCE + 5 + TAG];
        for (i, b) in sealed.iter_mut().enumerate() {
            *b = i as u8;
        }
        let (nonce, cipher, tag) = split(&sealed).unwrap();
        assert_eq!(nonce[0], 0);
        assert_eq!(cipher, &sealed[NONCE..NONCE + 5]);
        assert_eq!(tag[0], (NONCE + 5) as u8);
        // The empty value has a tag too.
        assert!(split(&sealed[..NONCE + TAG]).is_some());
        assert!(split(&sealed[..NONCE + TAG - 1]).is_none());
        assert!(split(&[0u8; MAX_SEALED]).is_some());
        assert!(split(&[0u8; MAX_SEALED + 1]).is_none());
    }
}
