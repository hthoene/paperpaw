//! A card's free space, known without reading all of its FAT at once.
//!
//! rust-fatfs counts the free clusters by reading the whole FAT when the
//! volume was not unmounted cleanly (which a reader that is switched off
//! never does): megabytes over a card's bus in one call. Here the count
//! the card keeps in its FS information sector (FAT32) serves at once, as
//! an estimate; the FAT is then counted exactly, [`STEP`] bytes of it at
//! each ask; and what the storage writes and removes from then on is
//! taken off or added, cluster by cluster.

/// Bytes of the FAT read at most each time the free space is asked for.
pub(crate) const STEP: usize = 128 * 1024;
/// Bytes of the FAT read at once, into a buffer on the stack: nothing is
/// taken from the heap, which on the X4 is small and in pieces while the
/// radio is up.
const SECTOR: usize = 512;

/// Reads bytes of the volume at an offset; `false` if it cannot.
pub(crate) type Read = dyn Fn(u64, &mut [u8]) -> bool;

/// Where the FAT is and what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geometry {
    /// The FAT's first byte in the volume.
    fat: u64,
    /// The bits of an entry: 12, 16 or 32.
    bits: u8,
    /// Clusters for data; their entries are 2 to `clusters + 1`.
    clusters: u32,
    /// Bytes of a cluster.
    cluster: u64,
    /// The FS information sector's first byte (FAT32).
    info: Option<u64>,
}

impl Geometry {
    /// The geometry the boot sector `boot` says.
    fn of(boot: &[u8; 512]) -> Option<Geometry> {
        let u16_at = |at: usize| u32::from(u16::from_le_bytes([boot[at], boot[at + 1]]));
        let u32_at =
            |at: usize| u32::from_le_bytes([boot[at], boot[at + 1], boot[at + 2], boot[at + 3]]);
        let sector = u16_at(11);
        let per_cluster = u32::from(boot[13]);
        let reserved = u16_at(14);
        let fats = u32::from(boot[16]);
        let roots = u16_at(17);
        let total = match u16_at(19) {
            0 => u32_at(32),
            total => total,
        };
        let fat_size = match u16_at(22) {
            0 => u32_at(36),
            size => size,
        };
        if sector == 0 || per_cluster == 0 || fats == 0 || fat_size == 0 {
            return None;
        }
        let root_sectors = (roots * 32).div_ceil(sector);
        let data = total.checked_sub(reserved + fats * fat_size + root_sectors)?;
        let clusters = data / per_cluster;
        let bits = match clusters {
            0..4085 => 12,
            4085..65525 => 16,
            _ => 32,
        };
        let info = (bits == 32).then(|| u64::from(u16_at(48)) * u64::from(sector));
        Some(Geometry {
            fat: u64::from(reserved) * u64::from(sector),
            bits,
            clusters,
            cluster: u64::from(sector) * u64::from(per_cluster),
            info,
        })
    }

    /// The free clusters the FS information sector says, if it says.
    fn said(&self, read: &Read) -> Option<u32> {
        let mut sector = [0u8; 512];
        if !read(self.info?, &mut sector) {
            return None;
        }
        let u32_at = |at: usize| {
            u32::from_le_bytes([sector[at], sector[at + 1], sector[at + 2], sector[at + 3]])
        };
        let signed = u32_at(0) == 0x4161_5252 && u32_at(484) == 0x6141_7272;
        let free = u32_at(488);
        (signed && free <= self.clusters).then_some(free)
    }

    /// The bytes of the FAT entries `first..first + count`.
    fn span(&self, first: u32, count: u32) -> (u64, usize) {
        match self.bits {
            12 => {
                let start = u64::from(first) * 3 / 2;
                // The last entry's two bytes.
                let end = u64::from(first + count - 1) * 3 / 2 + 2;
                (self.fat + start, (end - start) as usize)
            }
            bits => {
                let size = u64::from(bits / 8);
                (
                    self.fat + u64::from(first) * size,
                    (u64::from(count) * size) as usize,
                )
            }
        }
    }

    /// How many of the entries `first..first + count`, read into `bytes`
    /// (see [`Geometry::span`]), are free.
    fn free_in(&self, first: u32, count: u32, bytes: &[u8]) -> u32 {
        match self.bits {
            12 => {
                let base = first as usize * 3 / 2;
                (first..first + count)
                    .filter(|&n| {
                        let at = n as usize * 3 / 2 - base;
                        let (low, high) = (bytes[at], bytes[at + 1]);
                        let value = if n % 2 == 0 {
                            u16::from(low) | (u16::from(high & 0x0F) << 8)
                        } else {
                            u16::from(low >> 4) | (u16::from(high) << 4)
                        };
                        value == 0
                    })
                    .count() as u32
            }
            16 => bytes
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|e| **e == [0, 0])
                .count() as u32,
            _ => bytes
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|e| u32::from_le_bytes(**e) & 0x0FFF_FFFF == 0)
                .count() as u32,
        }
    }
}

/// The free space, as far as it is known; see the [module
/// documentation](self).
#[derive(Debug, Default)]
pub(crate) struct Free {
    geometry: Option<Geometry>,
    /// Free clusters: exact once counted, the card's own word before.
    known: Option<u64>,
    /// The count going on: the next entry, and the free ones before it.
    counting: Option<(u32, u64)>,
    /// The count is done: `known` is exact (and kept so).
    exact: bool,
}

impl Free {
    /// Counts on (one step), and the free bytes as far as known.
    pub(crate) fn ask(&mut self, read: &Read) -> Option<u64> {
        let geometry = self.geometry(read)?;
        let Some((mut next, mut free)) = self.counting else {
            return self.bytes();
        };
        let last = geometry.clusters + 2;
        // A sector's worth of entries at a time, into a buffer of that
        // size (an even number of FAT12's, which share bytes in pairs).
        let per_read = match geometry.bits {
            12 => (SECTOR as u32 * 2 / 3) & !1,
            bits => (SECTOR / usize::from(bits / 8)) as u32,
        };
        let mut buffer = [0u8; SECTOR];
        let mut read_so_far = 0;
        while next < last && read_so_far < STEP {
            let count = per_read.min(last - next);
            let (at, len) = geometry.span(next, count);
            let bytes = &mut buffer[..len];
            if !read(at, bytes) {
                self.counting = Some((next, free));
                return self.bytes();
            }
            free += u64::from(geometry.free_in(next, count, bytes));
            next += count;
            read_so_far += len;
        }
        if next >= last {
            self.known = Some(free);
            self.counting = None;
            self.exact = true;
        } else {
            self.counting = Some((next, free));
        }
        self.bytes()
    }

    /// The bytes for files in all, used and free.
    pub(crate) fn total(&mut self, read: &Read) -> Option<u64> {
        let geometry = self.geometry(read)?;
        Some(u64::from(geometry.clusters) * geometry.cluster)
    }

    /// The volume's geometry, read from its boot sector at the first ask
    /// (and the card's own count with it).
    fn geometry(&mut self, read: &Read) -> Option<Geometry> {
        if self.geometry.is_none() {
            let mut boot = [0u8; 512];
            let geometry = read(0, &mut boot).then(|| Geometry::of(&boot)).flatten()?;
            self.known = geometry.said(read).map(u64::from);
            self.counting = Some((2, 0));
            self.geometry = Some(geometry);
        }
        self.geometry
    }

    fn bytes(&self) -> Option<u64> {
        Some(self.known? * self.geometry?.cluster)
    }

    /// The clusters `bytes` take.
    pub(crate) fn clusters(&self, bytes: u64) -> u64 {
        self.geometry.map_or(0, |g| bytes.div_ceil(g.cluster))
    }

    /// `taken` clusters were taken and `freed` freed by a change of the
    /// storage. A count going on starts anew (the part counted may have
    /// changed); the count known follows.
    pub(crate) fn changed(&mut self, taken: u64, freed: u64) {
        let Some(geometry) = self.geometry else {
            return;
        };
        // Never more than the card holds: the sizes of a damaged entry
        // may tell more freed.
        if let Some(known) = self.known.as_mut() {
            *known = known
                .saturating_add(freed)
                .saturating_sub(taken)
                .min(u64::from(geometry.clusters));
        }
        if self.counting.is_some() {
            self.counting = Some((2, 0));
        }
    }

    /// Whether the free clusters were counted (and followed since).
    #[cfg(test)]
    pub(crate) fn exact(&self) -> bool {
        self.exact
    }
}
