//! The clusters of an exFAT volume past its card's end, kept from the
//! driver without a write: [`Tail`].
//!
//! A volume a little longer than its card (a partition said to go past a
//! card's end, as on some cards as sold) is mounted, but the driver's
//! allocator would hand out its clusters past the end, where nothing can
//! be written. The driver reads the card through [`Tail::read`]: there
//! those clusters' FAT entries read as bad and their bitmap bits as used,
//! so it never allocates them and does not count them as room (its check
//! counts such clusters neither free nor lost). What it writes there goes
//! through [`Tail::write`], which keeps the card's own bytes: the card is
//! never changed for it, and a computer sees it as it was.

use alloc::vec::Vec;

use crate::disk::{BLOCK, Blocks, DiskError, SharedDisk};
use crate::exfat_boot::BootSizes;

/// A FAT entry marking a bad cluster (`0xFFFFFFF7`), as stored.
const BAD: [u8; 4] = 0xFFFF_FFF7u32.to_le_bytes();
/// The first cluster of the heap.
const FIRST: u32 = 2;
/// The entry type of an Allocation Bitmap in the root directory.
const BITMAP_ENTRY: u8 = 0x81;
/// The end of a FAT chain, and the highest valid cluster number.
const CHAIN_END: u32 = 0xFFFF_FFF7;
/// Most clusters of the bitmap followed through the FAT: one longer is
/// damaged (a bitmap of 2^32 clusters takes 2^29 bytes, 2^14 clusters of
/// 32 KiB).
const BITMAP_CLUSTERS_MOST: u64 = 1 << 20;

/// Bytes of the card the driver sees otherwise: from `start` to `end`
/// (absolute), each byte its `mask` bits taken from the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    start: u64,
    end: u64,
    what: What,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum What {
    /// FAT entries, each read as [`BAD`]; `start` begins an entry.
    Bad,
    /// Bitmap bytes, read with all their bits set but those below
    /// `first_bit` of the first byte.
    Used { first_bit: u8 },
}

impl Shown {
    /// The bits of the byte at `at` that are shown, and how.
    fn byte(&self, at: u64) -> (u8, u8) {
        match self.what {
            What::Bad => {
                let i = usize::try_from((at - self.start) % 4).unwrap_or(0);
                (0xFF, BAD[i])
            }
            What::Used { first_bit } => {
                let mask = if at == self.start {
                    0xFFu8 << first_bit
                } else {
                    0xFF
                };
                (mask, 0xFF)
            }
        }
    }

    /// The part of `start..end` (absolute) it covers.
    fn within(&self, start: u64, end: u64) -> Option<(u64, u64)> {
        let (from, to) = (self.start.max(start), self.end.min(end));
        (from < to).then_some((from, to))
    }
}

/// The clusters of a volume past its card's end, as the driver is shown
/// them (see the module): none on most cards.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Tail {
    /// How many clusters lie past the end.
    clusters: u32,
    /// What is shown otherwise.
    shown: Vec<Shown>,
}

impl Tail {
    /// The clusters of the volume on `disk` past its end, where it names
    /// `missing` blocks past it; none where it names none. Where the
    /// active bitmap cannot be found, only the FAT entries are shown bad:
    /// nothing is allocated there, but the room counted holds them.
    pub(crate) fn of<B: Blocks>(disk: &SharedDisk<B>, missing: u64) -> Self {
        if missing == 0 {
            return Self::default();
        }
        let Some(boot) = BootSizes::of(disk) else {
            return Self::default();
        };
        let Some(geo) = Geo::of(&boot) else {
            return Self::default();
        };
        let card = disk.blocks().saturating_mul(BLOCK as u64);
        let within = card.saturating_sub(geo.heap) >> geo.cluster_shift;
        let first = u32::try_from(within).map_or(u32::MAX, |n| n.saturating_add(FIRST));
        let end = boot.clusters.saturating_add(FIRST);
        if first >= end {
            return Self::default();
        }
        let mut shown = Vec::new();
        for fat in 0..u64::from(boot.fats.clamp(1, 2)) {
            let start = geo.fat + fat * geo.fat_len;
            shown.push(Shown {
                start: start + 4 * u64::from(first),
                end: start + 4 * u64::from(end),
                what: What::Bad,
            });
        }
        if let Some(bitmap) = bitmap_shown(disk, &boot, &geo, first - FIRST) {
            shown.extend(bitmap);
        }
        shown.sort_unstable_by_key(|s| s.start);
        Self {
            clusters: end - first,
            shown,
        }
    }

    /// How many clusters lie past the card's end.
    pub(crate) fn clusters(&self) -> u32 {
        self.clusters
    }

    /// `buf`, just read at `at`, as the driver is shown it.
    pub(crate) fn read(&self, at: u64, buf: &mut [u8]) {
        let end = at + buf.len() as u64;
        for s in &self.shown {
            let Some((from, to)) = s.within(at, end) else {
                continue;
            };
            for pos in from..to {
                let (mask, value) = s.byte(pos);
                if let Some(b) = usize::try_from(pos - at).ok().and_then(|i| buf.get_mut(i)) {
                    *b = (*b & !mask) | (value & mask);
                }
            }
        }
    }

    /// Writes `data` at `at` of `disk`, the bytes shown otherwise left as
    /// the card has them.
    pub(crate) fn write<B: Blocks>(
        &self,
        disk: &SharedDisk<B>,
        at: u64,
        data: &[u8],
    ) -> Result<(), DiskError> {
        let end = at + data.len() as u64;
        let part = |from: u64, to: u64| {
            let (a, b) = (usize::try_from(from - at), usize::try_from(to - at));
            match (a, b) {
                (Ok(a), Ok(b)) => data.get(a..b).ok_or(DiskError),
                _ => Err(DiskError),
            }
        };
        // `shown` is sorted by start and its parts do not overlap.
        let mut pos = at;
        for s in &self.shown {
            let Some((from, to)) = s.within(pos, end) else {
                continue;
            };
            if from > pos {
                disk.write_at(pos, part(pos, from)?)?;
            }
            // A byte only partly shown: its other bits are the driver's.
            let (mask, _) = s.byte(from);
            if mask != 0xFF {
                let mut card = [0u8; 1];
                disk.read_at(from, &mut card)?;
                let mine = part(from, from + 1)?.first().copied().ok_or(DiskError)?;
                disk.write_at(from, &[(mine & !mask) | (card[0] & mask)])?;
            }
            pos = to;
        }
        if pos < end {
            disk.write_at(pos, part(pos, end)?)?;
        }
        Ok(())
    }
}

/// Where a volume's parts lie, in bytes of the card.
struct Geo {
    fat: u64,
    fat_len: u64,
    heap: u64,
    cluster_shift: u32,
}

impl Geo {
    fn of(boot: &BootSizes) -> Option<Self> {
        let sector = u32::from(boot.sector_shift);
        let cluster_shift = sector + u32::from(boot.cluster_shift);
        if !(9..=12).contains(&sector) || cluster_shift > 25 {
            return None;
        }
        Some(Self {
            fat: u64::from(boot.fat.0) << sector,
            fat_len: u64::from(boot.fat.1) << sector,
            heap: u64::from(boot.heap) << sector,
            cluster_shift,
        })
    }

    /// Where `cluster` starts on the card.
    fn cluster_at(&self, cluster: u32) -> u64 {
        self.heap + (u64::from(cluster.saturating_sub(FIRST)) << self.cluster_shift)
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_le_bytes)
}

/// The bytes of the active Allocation Bitmap from bit `index` on, shown
/// used; none where the bitmap is not found in the root directory's first
/// cluster or its chain is damaged.
fn bitmap_shown<B: Blocks>(
    disk: &SharedDisk<B>,
    boot: &BootSizes,
    geo: &Geo,
    index: u32,
) -> Option<Vec<Shown>> {
    let (first, len) = active_bitmap(disk, boot, geo)?;
    let cluster_bytes = 1u64 << geo.cluster_shift;
    let from = u64::from(index / 8);
    let first_bit = u8::try_from(index % 8).ok()?;
    if from >= len || len.div_ceil(cluster_bytes) > BITMAP_CLUSTERS_MOST {
        return None;
    }
    // Walk the bitmap's chain to the cluster holding byte `from`, then
    // take each cluster's share of `from..len`.
    let mut shown = Vec::new();
    let mut cluster = first;
    let mut start = 0u64;
    while start < len {
        if !(FIRST..CHAIN_END).contains(&cluster) {
            return None;
        }
        let end = (start + cluster_bytes).min(len);
        if end > from {
            let at = geo.cluster_at(cluster);
            let lo = from.max(start);
            shown.push(Shown {
                start: at + (lo - start),
                end: at + (end - start),
                what: What::Used {
                    first_bit: if lo == from { first_bit } else { 0 },
                },
            });
        }
        start = end;
        if start < len {
            let mut entry = [0u8; 4];
            disk.read_at(geo.fat + 4 * u64::from(cluster), &mut entry)
                .ok()?;
            cluster = u32::from_le_bytes(entry);
        }
    }
    Some(shown)
}

/// The first cluster and length in bytes of the active Allocation Bitmap
/// (the second of a TexFAT volume whose second FAT is active).
fn active_bitmap<B: Blocks>(
    disk: &SharedDisk<B>,
    boot: &BootSizes,
    geo: &Geo,
) -> Option<(u32, u64)> {
    let mut sector = [0u8; BLOCK];
    disk.read_at(0, &mut sector).ok()?;
    let root = u32_at(&sector, 96)?;
    if !(FIRST..CHAIN_END).contains(&root) {
        return None;
    }
    let second = boot.fats == 2 && boot.flags & 1 != 0;
    let cluster_bytes = 1usize << geo.cluster_shift;
    let mut block = [0u8; BLOCK];
    let at = geo.cluster_at(root);
    for offset in (0..cluster_bytes).step_by(BLOCK) {
        disk.read_at(at + offset as u64, &mut block).ok()?;
        for entry in block.as_chunks::<32>().0 {
            match entry[0] {
                0 => return None,
                BITMAP_ENTRY if (entry[1] & 1 != 0) == second => {
                    let first = u32_at(entry, 20)?;
                    let low = u64::from(u32_at(entry, 24)?);
                    let high = u64::from(u32_at(entry, 28)?);
                    return Some((first, low | (high << 32)));
                }
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests panic on purpose")]
mod tests {
    use super::*;
    use crate::disk::BlockDisk;

    struct Card(Vec<u8>);

    impl Blocks for Card {
        fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
            let at = index as usize * BLOCK;
            block.copy_from_slice(self.0.get(at..at + BLOCK).ok_or(DiskError)?);
            Ok(())
        }
        fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
            let at = index as usize * BLOCK;
            self.0
                .get_mut(at..at + BLOCK)
                .ok_or(DiskError)?
                .copy_from_slice(block);
            Ok(())
        }
        fn blocks(&mut self) -> Result<u32, DiskError> {
            Ok((self.0.len() / BLOCK) as u32)
        }
    }

    /// FAT entries from byte 8 to 16, bitmap bits from bit 3 of byte 600
    /// to byte 602.
    fn tail() -> Tail {
        Tail {
            clusters: 2,
            shown: alloc::vec![
                Shown {
                    start: 8,
                    end: 16,
                    what: What::Bad,
                },
                Shown {
                    start: 600,
                    end: 602,
                    what: What::Used { first_bit: 3 },
                },
            ],
        }
    }

    #[test]
    fn what_is_shown_reads_bad_and_used() {
        let mut buf = [0x11u8; 1024];
        tail().read(0, &mut buf);
        assert_eq!(
            buf[4..20],
            [
                0x11, 0x11, 0x11, 0x11, 0xF7, 0xFF, 0xFF, 0xFF, 0xF7, 0xFF, 0xFF, 0xFF, 0x11, 0x11,
                0x11, 0x11
            ]
        );
        assert_eq!(buf[599..603], [0x11, 0xF9, 0xFF, 0x11]);
        // Read from within: the pattern keeps its place.
        let mut part = [0u8; 4];
        tail().read(10, &mut part);
        assert_eq!(part, [0xFF, 0xFF, 0xF7, 0xFF]);
    }

    #[test]
    fn writes_keep_the_cards_bytes_where_shown() {
        let disk = SharedDisk::new(BlockDisk::new(Card(alloc::vec![0x22; 4 * BLOCK])).unwrap());
        let tail = tail();
        let mut buf = [0u8; 1024];
        disk.read_at(0, &mut buf).unwrap();
        tail.read(0, &mut buf);
        // The driver changes every byte, those shown too.
        for b in &mut buf {
            *b ^= 0xFF;
        }
        tail.write(&disk, 0, &buf).unwrap();
        let mut card = [0u8; 1024];
        disk.read_at(0, &mut card).unwrap();
        assert!(card[..8].iter().all(|&b| b == 0xDD));
        assert!(card[8..16].iter().all(|&b| b == 0x22));
        assert!(card[16..600].iter().all(|&b| b == 0xDD));
        // Bits 0..3 the driver's (0x22 ^ 0xFF = 0xDD: 101), 3..8 the card's.
        assert_eq!(card[600], (0xDD & 0x07) | (0x22 & 0xF8));
        assert_eq!(card[601], 0x22);
        assert!(card[602..].iter().all(|&b| b == 0xDD));
    }
}
