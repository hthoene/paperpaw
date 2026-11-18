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

/// The X4 Classic's key sector: the last 4 KiB of its 16 MB flash, in no
/// partition of PaperPaw's table. Its factory `nvs` (`0x9000` to `0xE000`) is
/// kept whole and never written (it says which panel the device has), so the
/// key cannot be a page of it. A table of another firmware that has a
/// partition there (CrossPoint's `coredump` at `0xFF0000`) makes [`locate`]
/// refuse, and nothing is sealed.
pub const OUTSIDE_AT: u32 = 0x00FF_F000;
/// The flash of every device that seals: 16 MB.
pub const FLASH_LEN: u32 = 16 * 1024 * 1024;

/// Where a device keeps its key (decision 140).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The fourth sector of the `nvs` partition (`0xC000` with the partition
    /// at `0x9000`): the X4, the X3 and the X4 Pro, whose table
    /// (`partitions.csv`) has a 16 KiB `nvs` that no other user of PaperPaw's
    /// table touches.
    InNvs,
    /// A sector of the flash that no partition of the table covers: the X4
    /// Classic.
    Outside(u32),
}

/// The place of a device: `classic` for the X4 Classic.
#[must_use]
pub const fn place(classic: bool) -> Place {
    if classic {
        Place::Outside(OUTSIDE_AT)
    } else {
        Place::InNvs
    }
}

/// The sector the web flasher and every installer must leave alone, as
/// `[start, end)` (decision 139: a reinstall keeps the passwords). Kept next
/// to [`place`] so that they cannot disagree.
#[must_use]
pub const fn kept_range(classic: bool) -> (u32, u32) {
    let at = match place(classic) {
        Place::InNvs => 0x9000 + OFFSET,
        Place::Outside(at) => at,
    };
    (at, at + SECTOR as u32)
}

/// The flash address of the key sector for `place`, given the partition table:
/// the `nvs` partition's `(offset, length)` if there is one and the
/// `(offset, length)` of every partition. `None` when the table does not
/// allow the place, in which case nothing is sealed:
/// - `InNvs`: the `nvs` partition must be there and reach the fourth sector;
/// - `Outside`: the sector must lie in the flash and overlap no partition.
#[must_use]
pub fn locate(
    place: Place,
    nvs: Option<(u32, u32)>,
    mut partitions: impl Iterator<Item = (u32, u32)>,
) -> Option<u32> {
    match place {
        Place::InNvs => {
            let (offset, len) = nvs?;
            (u64::from(len) >= u64::from(OFFSET) + SECTOR as u64)
                .then(|| offset.checked_add(OFFSET))
                .flatten()
        }
        Place::Outside(at) => {
            let end = at.checked_add(SECTOR as u32)?;
            if end > FLASH_LEN {
                return None;
            }
            let covered = partitions.any(|(offset, len)| {
                let last = u64::from(offset) + u64::from(len);
                u64::from(offset) < u64::from(end) && u64::from(at) < last
            });
            (!covered).then_some(at)
        }
    }
}

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
    /// Anything else: data that is not ours (an NVS page of the stock firmware
    /// or of a dual-boot partner, #920). No key, and the sector is **never
    /// erased or written** over it: nothing is sealed.
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
    // Too short to tell: never ours to erase.
    let Some(bytes) = bytes.get(..LEN) else {
        return Sector::Foreign;
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

/// What the start-up re-save of Beta 3's veiled values came to (the host's
/// `SecretsHost::upgrade`, run once at start on a device that seals).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupUpgrade {
    /// The file was rewritten sealed.
    Rewritten,
    /// Nothing to re-save.
    Nothing,
    /// The partition table has no place for the key (an X4 Classic still on
    /// CrossPoint's table): skipped, with one note in the log at most. The
    /// veiled values stay as they are and every start is the same, so it is
    /// not an error and not worth a line on each start.
    NoKeyPlace,
    /// Anything else (the card, the flash, memory): a real failure.
    Failed(i32),
}

/// Sorts the result of the start-up re-save. `unsupported` is the status
/// `ERR_UNSUPPORTED` that the sealer answers when [`locate`] finds no place.
#[must_use]
pub fn startup_upgrade(result: Result<bool, i32>, unsupported: i32) -> StartupUpgrade {
    match result {
        Ok(true) => StartupUpgrade::Rewritten,
        Ok(false) => StartupUpgrade::Nothing,
        Err(status) if status == unsupported => StartupUpgrade::NoKeyPlace,
        Err(status) => StartupUpgrade::Failed(status),
    }
}

/// When the firmware runs the start-up re-save (Senior M1, #254): once at
/// start, then again only after weak numbers (`ERR_STATE`), and at most once
/// for each time a radio comes on. Any other outcome ends it for this start,
/// so a key place that is not ours never costs a try on every main-loop turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpgradeRetry {
    /// Nothing more to try this start.
    done: bool,
    /// The count of radio-ons at the last try.
    tried: Option<u32>,
}

impl UpgradeRetry {
    /// Whether to try now: not done, the numbers are strong, and not tried yet
    /// while this radio-on lasts (`radio_ons` counts the times a radio came on).
    #[must_use]
    pub fn due(&self, strong: bool, radio_ons: u32) -> bool {
        !self.done && strong && self.tried != Some(radio_ons)
    }

    /// A try at `radio_ons` came to `outcome`; only `weak` (the status of weak
    /// numbers, `ERR_STATE`) leaves another try for a later radio-on.
    pub fn tried(&mut self, radio_ons: u32, outcome: StartupUpgrade, weak: i32) {
        self.tried = Some(radio_ons);
        self.done = outcome != StartupUpgrade::Failed(weak);
    }

    /// Nothing more to try this start.
    #[must_use]
    pub const fn done(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_up_re_save_retries_once_per_radio_on_after_weak_numbers() {
        const WEAK: i32 = -6;
        let mut retry = UpgradeRetry::default();
        // At start (no radio yet), weak numbers.
        retry.tried(0, StartupUpgrade::Failed(WEAK), WEAK);
        assert!(!retry.done());
        assert!(!retry.due(false, 0), "not while the numbers are weak");
        assert!(retry.due(true, 1), "the first radio-on");
        retry.tried(1, StartupUpgrade::Failed(WEAK), WEAK);
        assert!(!retry.due(true, 1), "once per radio-on, not every turn");
        assert!(retry.due(true, 2), "the next radio-on");
        retry.tried(2, StartupUpgrade::Rewritten, WEAK);
        assert!(retry.done());
        assert!(!retry.due(true, 3));
        // Anything else than weak numbers ends it for this start.
        for outcome in [
            StartupUpgrade::Nothing,
            StartupUpgrade::NoKeyPlace,
            StartupUpgrade::Failed(-8),
        ] {
            let mut retry = UpgradeRetry::default();
            retry.tried(0, outcome, WEAK);
            assert!(retry.done());
            assert!(!retry.due(true, 1));
        }
    }

    #[test]
    fn a_table_without_a_key_place_skips_the_start_up_re_save_quietly() {
        const UNSUPPORTED: i32 = -1;
        assert_eq!(
            startup_upgrade(Err(UNSUPPORTED), UNSUPPORTED),
            StartupUpgrade::NoKeyPlace
        );
        assert_eq!(
            startup_upgrade(Ok(true), UNSUPPORTED),
            StartupUpgrade::Rewritten
        );
        assert_eq!(
            startup_upgrade(Ok(false), UNSUPPORTED),
            StartupUpgrade::Nothing
        );
        // A real failure is still one.
        assert_eq!(
            startup_upgrade(Err(-5), UNSUPPORTED),
            StartupUpgrade::Failed(-5)
        );
        // The table that gives no place is what makes the sealer answer so.
        assert_eq!(
            locate(
                Place::Outside(OUTSIDE_AT),
                None,
                core::iter::once((0xFF_0000, 0x1_0000))
            ),
            None
        );
    }

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

    /// A table's partitions, from a `partitions.csv` (name, type, subtype,
    /// offset, size).
    fn partitions_of(table: &str) -> alloc::vec::Vec<(&str, u32, u32)> {
        let number =
            |text: &str| u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).unwrap();
        table
            .lines()
            .filter(|line| !line.trim_start().starts_with('#') && line.contains(','))
            .map(|line| {
                let f: alloc::vec::Vec<&str> = line.split(',').collect();
                (f[0].trim(), number(f[3]), number(f[4]))
            })
            .collect()
    }

    const CROSSPOINT: &str = "\
nvs,      data, nvs,     0x9000,  0x5000,
otadata,  data, ota,     0xe000,  0x2000,
app0,     app,  ota_0,   0x10000, 0x640000,
app1,     app,  ota_1,   0x650000,0x640000,
spiffs,   data, spiffs,  0xc90000,0x360000,
coredump, data, coredump,0xFF0000,0x10000,
";

    fn find(table: &str, place: Place) -> Option<u32> {
        let parts = partitions_of(table);
        let nvs = parts.iter().find(|p| p.0 == "nvs").map(|p| (p.1, p.2));
        locate(place, nvs, parts.iter().map(|p| (p.1, p.2)))
    }

    const X4_TABLE: &str = include_str!("../../../shells/xteink/partitions.csv");
    const CLASSIC_TABLE: &str = include_str!("../../../shells/xteink/partitions-classic.csv");

    /// Decision 140, per device: the key sector, from the table each ships.
    #[test]
    fn each_device_has_its_key_sector() {
        // The X4, the X3 and the X4 Pro share partitions.csv.
        assert_eq!(find(X4_TABLE, place(false)), Some(0xC000));
        // The X4 Classic: outside every partition, not in its factory NVS.
        assert_eq!(find(CLASSIC_TABLE, place(true)), Some(0x00FF_F000));
    }

    #[test]
    fn the_classic_never_uses_its_factory_nvs() {
        assert_eq!(place(true), Place::Outside(OUTSIDE_AT));
        // It would be there if it were in nvs: 0xC000 is a page of the factory's NVS
        // (0x9000 to 0xE000), which says which panel the device has.
        assert_eq!(find(CLASSIC_TABLE, Place::InNvs), Some(0xC000));
        let nvs = partitions_of(CLASSIC_TABLE)
            .into_iter()
            .find(|p| p.0 == "nvs")
            .unwrap();
        assert!((nvs.1..nvs.1 + nvs.2).contains(&0xC000));
        assert!(!(nvs.1..nvs.1 + nvs.2).contains(&OUTSIDE_AT));
    }

    #[test]
    fn the_outside_sector_is_in_no_partition_and_is_the_last_of_the_flash() {
        assert_eq!(OUTSIDE_AT + SECTOR as u32, FLASH_LEN);
        for table in [X4_TABLE, CLASSIC_TABLE] {
            for (name, at, len) in partitions_of(table) {
                assert!(at + len <= OUTSIDE_AT, "{name} reaches the key sector");
            }
        }
    }

    /// Another firmware's table decides too: its `coredump` covers the
    /// Classic's sector (nothing is sealed there, and nothing of it is
    /// written), while its NVS holds the others' sector (risk R3: a stock
    /// firmware that writes its NVS may take it; values are then entered again).
    #[test]
    fn another_firmwares_table_is_respected() {
        assert_eq!(find(CROSSPOINT, place(true)), None);
        assert_eq!(find(CROSSPOINT, place(false)), Some(0xC000));
    }

    #[test]
    fn the_place_needs_what_it_says() {
        // No nvs partition, or one too short for the fourth sector.
        assert_eq!(locate(Place::InNvs, None, core::iter::empty()), None);
        assert_eq!(
            locate(Place::InNvs, Some((0x9000, 0x3000)), core::iter::empty()),
            None
        );
        assert_eq!(
            locate(Place::InNvs, Some((0x9000, 0x4000)), core::iter::empty()),
            Some(0xC000)
        );
        // A table claiming the sector, wholly or by one byte at either edge.
        let at = OUTSIDE_AT;
        for part in [
            (at, 1),
            (at - 1, 2),
            (0, FLASH_LEN),
            (at + SECTOR as u32 - 1, 1),
        ] {
            assert_eq!(
                locate(Place::Outside(at), None, [part].into_iter()),
                None,
                "{part:?}"
            );
        }
        // Next to it is not over it.
        assert_eq!(
            locate(Place::Outside(at), None, [(at - 4096, 4096)].into_iter()),
            Some(at)
        );
        // Out of the flash.
        assert_eq!(
            locate(Place::Outside(FLASH_LEN), None, core::iter::empty()),
            None
        );
        assert_eq!(
            locate(Place::Outside(u32::MAX - 100), None, core::iter::empty()),
            None
        );
        assert_eq!(
            locate(
                Place::InNvs,
                Some((u32::MAX - 100, 0x4000)),
                core::iter::empty()
            ),
            None
        );
    }

    /// What the web flasher skips, one range per device (paperpaw.org
    /// `static/flash/device.js`). A change here is a change there.
    #[test]
    fn the_kept_range_is_the_key_sector() {
        assert_eq!(kept_range(false), (0xC000, 0xD000));
        assert_eq!(kept_range(true), (0x00FF_F000, 0x0100_0000));
        for classic in [false, true] {
            let (start, end) = kept_range(classic);
            let table = if classic { CLASSIC_TABLE } else { X4_TABLE };
            assert_eq!(find(table, place(classic)), Some(start));
            assert_eq!(end - start, SECTOR as u32);
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
    fn any_changed_bit_is_no_key_and_only_ours_is_ever_erasable() {
        let good = record(&key());
        for at in 0..LEN {
            for bit in 0..8 {
                let mut bad = good;
                bad[at] ^= 1 << bit;
                let want = if at < MAGIC.len() {
                    // The magic no longer says it is ours.
                    Sector::Foreign
                } else {
                    Sector::Damaged
                };
                assert_eq!(read(&bad), want, "byte {at} bit {bit}");
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
            assert_eq!(read(&torn), Sector::Damaged, "cut at {cut}: ours, erasable");
        }
        assert_eq!(read(&good[..LEN - 1]), Sector::Foreign);
        assert_eq!(read(&[]), Sector::Foreign);
    }

    #[test]
    fn data_that_is_not_ours_is_foreign_and_never_erased() {
        assert_eq!(read(&[0u8; SECTOR]), Sector::Foreign);
        assert_eq!(read(&[0x5A; LEN]), Sector::Foreign);
        // An ESP-IDF NVS page header: state word, then the sequence number.
        let mut page = [0xFF; LEN];
        page[..8].copy_from_slice(&[0xFE, 0xFF, 0xFF, 0xFF, 1, 0, 0, 0]);
        assert_eq!(read(&page), Sector::Foreign);
        assert_eq!(alloc::format!("{:?}", Sector::Foreign), "Foreign");
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
